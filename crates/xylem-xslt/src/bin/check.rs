//! `xylem-xslt-check`: checks that a stylesheet still produces the same
//! output on a suite of test inputs.
//!
//! ```text
//! xylem-xslt-check <suite.toml> [--stylesheet <file.xsl>]
//!                  [--against saved | committed[:<rev>] | <other.xsl>]
//!                  [--host <SaxonDotnetHost.exe>] [--work <folder>]
//! xylem-xslt-check <suite.toml> --profile [--top <n>] [--stylesheet ...]
//! xylem-xslt-check <suite.toml> --analyze [--fix] [--scope <folder>]
//!                  [--against ...] [--stylesheet ...] [--repeat <n>]
//! ```
//!
//! `--analyze` times the suite, then lists patterns in the stylesheet (and
//! everything it includes or imports) that make it slow, with the time of the
//! template each is in. `--fix` then applies the fixes that are equivalent by
//! construction in code the tests reach, and proves them: every suite in the
//! same folder whose stylesheet uses a changed file is checked against
//! `--against` (default: the last committed version), and every top-level
//! stylesheet under `--scope` that uses a changed file is compiled.
//!
//! `--repeat <n>` runs each input n times when checking and reports the median
//! time, for comparing speed before and after.
//!
//! `--profile` times every template and function on the suite instead of
//! comparing output, and lists the `--top` (default 25) by own time.
//!
//! `--against` defaults to `committed` (the stylesheet as last committed to
//! git) when the suite has no saved output, and to `saved` when it does.
//! The host defaults to `XYLEM_XSLT_HOST`. Exit code 0 means every test gave
//! the same output.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use xylem_xslt::analyze::{Severity, analyze, apply};
use xylem_xslt::check::CheckEnv;
use xylem_xslt::{Baseline, Host, Outcome, Suite, check, profile, proof};

const USAGE: &str = "Usage: xylem-xslt-check <suite.toml> [--stylesheet <file.xsl>] \
[--against saved|committed[:<rev>]|<other.xsl>] [--host <program>] [--work <folder>] \
[--profile [--top <n>]] [--analyze [--fix] [--scope <folder>]] [--repeat <n>]";

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let mut args = std::env::args().skip(1);
    let mut suite_path = None;
    let mut stylesheet = None;
    let mut against = None;
    let mut host = std::env::var_os("XYLEM_XSLT_HOST").map(PathBuf::from);
    let mut work = None;
    let mut profiling = false;
    let mut analyzing = false;
    let mut fixing = false;
    let mut scope: Option<PathBuf> = None;
    let mut repeat = 1u32;
    let mut top = 25usize;
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value.\n{USAGE}"));
        match a.as_str() {
            "--stylesheet" => stylesheet = Some(PathBuf::from(value("--stylesheet")?)),
            "--against" => against = Some(value("--against")?),
            "--host" => host = Some(PathBuf::from(value("--host")?)),
            "--work" => work = Some(PathBuf::from(value("--work")?)),
            "--profile" => profiling = true,
            "--analyze" => analyzing = true,
            "--fix" => fixing = true,
            "--scope" => scope = Some(PathBuf::from(value("--scope")?)),
            "--repeat" => {
                repeat = value("--repeat")?
                    .parse()
                    .map_err(|_| format!("--repeat takes a number.\n{USAGE}"))?
            }
            "--top" => {
                top = value("--top")?
                    .parse()
                    .map_err(|_| format!("--top takes a number.\n{USAGE}"))?
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(true);
            }
            _ if suite_path.is_none() && !a.starts_with("--") => {
                suite_path = Some(PathBuf::from(a))
            }
            _ => return Err(format!("Unknown argument: {a}\n{USAGE}")),
        }
    }
    let suite_path = suite_path.ok_or(USAGE)?;
    let suite = Suite::load(&suite_path)?;
    let stylesheet = stylesheet
        .or_else(|| suite.stylesheet.clone())
        .ok_or("The suite names no stylesheet; pass --stylesheet.")?;
    let mut host =
        Host::new(host.ok_or(
            "No host program: set XYLEM_XSLT_HOST or pass --host (see tools/saxon-dotnet).",
        )?);
    host.repeat = repeat.max(1);
    let baseline = match against.as_deref() {
        None if suite.tests.iter().all(|t| t.expected.is_some()) => Baseline::Saved,
        None => Baseline::Committed { rev: "HEAD".into() },
        Some("saved") => Baseline::Saved,
        Some("committed") => Baseline::Committed { rev: "HEAD".into() },
        Some(s) if s.starts_with("committed:") => Baseline::Committed {
            rev: s["committed:".len()..].into(),
        },
        Some(path) => Baseline::Stylesheet(PathBuf::from(path)),
    };
    let work = work.unwrap_or_else(|| std::env::temp_dir().join("xylem-xslt").join("work"));
    let cancel = AtomicBool::new(false);
    let mut progress = |s: String| eprintln!("{s}");
    if analyzing {
        let against = match against.as_deref() {
            None => Baseline::Committed { rev: "HEAD".into() },
            Some(_) => baseline.clone(),
        };
        return run_analysis(
            &suite_path,
            &suite,
            &stylesheet,
            &host,
            &work,
            fixing,
            scope.as_deref(),
            &against,
            top,
        );
    }
    if profiling {
        let env = CheckEnv {
            host: &host,
            work: &work,
            progress: &mut progress,
            cancel: &cancel,
        };
        return print_profile(&profile(&suite, &stylesheet, env), top);
    }
    println!("Checking {}", stylesheet.display());
    println!("against {}\n", baseline.describe());
    let report = check(
        &suite,
        &stylesheet,
        &baseline,
        CheckEnv {
            host: &host,
            work: &work,
            progress: &mut progress,
            cancel: &cancel,
        },
    );
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    if let Some(c) = &report.baseline_commit {
        println!("(commit {})\n", &c[..c.len().min(12)]);
    }
    for r in &report.results {
        let time = match (r.ms, r.baseline_ms) {
            (Some(a), Some(b)) => format!("{} ms, was {} ms", a, b),
            (Some(a), None) => format!("{a} ms"),
            _ => String::new(),
        };
        match &r.outcome {
            Outcome::Identical { byte_identical } => println!(
                "same       {}  {}{}",
                r.name,
                time,
                if *byte_identical {
                    ""
                } else {
                    "  (generated IDs renumbered)"
                }
            ),
            Outcome::NoBaseline => {
                println!("no saved   {}  {}  (nothing to compare with)", r.name, time)
            }
            Outcome::Failed(e) => println!("FAILED     {}  {}", r.name, e),
            Outcome::Different(c) => {
                println!(
                    "DIFFERENT  {}  {}  {} place{}{}",
                    r.name,
                    time,
                    c.count,
                    if c.count == 1 { "" } else { "s" },
                    if c.coarse {
                        " (listing cut short: very different)"
                    } else {
                        ""
                    }
                );
                for d in &c.differences {
                    let near = d
                        .near_id
                        .as_deref()
                        .map(|id| format!(" (in id {id})"))
                        .unwrap_or_default();
                    println!("    at {}{}", d.path, near);
                    println!("      was: {}", d.baseline);
                    println!("      now: {}", d.candidate);
                }
                if c.count > c.differences.len() {
                    println!("    … and {} more", c.count - c.differences.len());
                }
                if let (Some(a), Some(b)) = (&r.baseline_output, &r.output) {
                    println!("    outputs: {}  {}", a.display(), b.display());
                }
            }
        }
    }
    Ok(report.all_identical())
}

fn print_profile(report: &xylem_xslt::ProfileReport, top: usize) -> Result<bool, String> {
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    println!("Timing {}\n", report.stylesheet.display());
    let mut ok = true;
    for t in &report.tests {
        match &t.profile {
            Ok(p) => println!("  {:>9.1} ms  {}", p.total_ms, t.name),
            Err(e) => {
                ok = false;
                println!("  FAILED        {}: {e}", t.name)
            }
        }
    }
    let c = &report.combined;
    let timed = c.timed_ms().max(f64::EPSILON);
    println!(
        "\n{} templates and functions, {:.1} ms in them. By own time:\n",
        c.templates.len(),
        c.timed_ms()
    );
    println!(
        "  {:>9}  {:>9}  {:>9}  {:>5}  template",
        "own ms", "total ms", "calls", "share"
    );
    for t in c.templates.iter().take(top) {
        let file = t.path();
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "  {:>9.1}  {:>9.1}  {:>9}  {:>4.0}%  {}  ({}:{})",
            t.self_ms,
            t.total_ms,
            t.calls,
            100.0 * t.self_ms / timed,
            t.label(),
            name,
            t.line
        );
    }
    Ok(ok)
}

#[allow(clippy::too_many_arguments)]
fn run_analysis(
    suite_path: &std::path::Path,
    suite: &Suite,
    stylesheet: &std::path::Path,
    host: &Host,
    work: &std::path::Path,
    fixing: bool,
    scope: Option<&std::path::Path>,
    against: &Baseline,
    top: usize,
) -> Result<bool, String> {
    let cancel = AtomicBool::new(false);
    let mut progress = |s: String| eprintln!("{s}");
    let timed = profile(
        suite,
        stylesheet,
        CheckEnv {
            host,
            work,
            progress: &mut progress,
            cancel: &cancel,
        },
    );
    let timing = timed.error.is_none().then_some(&timed.combined);
    let analysis = analyze(stylesheet, timing)?;
    println!(
        "Analysed {} ({} modules{})\n",
        stylesheet.display(),
        analysis.modules.len(),
        if timing.is_some() {
            ", timed on the suite"
        } else {
            ""
        }
    );
    let count = |sev: Severity| {
        analysis
            .findings
            .iter()
            .filter(|f| f.severity == sev)
            .count()
    };
    println!(
        "{} fixes, {} suggestions, {} notes; {} fixes apply automatically (in code the tests reach)\n",
        count(Severity::Fix),
        count(Severity::Suggestion),
        count(Severity::Note),
        analysis.findings.iter().filter(|f| f.auto()).count()
    );
    for f in analysis.findings.iter().take(top) {
        let kind = match f.severity {
            Severity::Fix => "FIX",
            Severity::Suggestion => "SUGGEST",
            Severity::Note => "NOTE",
        };
        let time = match (f.self_ms, f.calls) {
            (_, Some(0)) => "not reached by the tests".to_string(),
            (Some(ms), Some(calls)) => format!("{ms:.1} ms in {calls} calls"),
            _ => String::new(),
        };
        let file = f
            .file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        println!(
            "{kind:<8} {}  {}:{}  in {}  {}",
            f.rule.title(),
            file,
            f.line,
            f.within.as_deref().unwrap_or("-"),
            time
        );
        if let Some(after) = &f.after {
            println!("         was: {}", f.before);
            println!("         now: {}", after.replace('\n', "\n              "));
        }
    }
    if analysis.findings.len() > top {
        println!("… and {} more (raise --top)", analysis.findings.len() - top);
    }
    if !fixing {
        return Ok(true);
    }
    let chosen: Vec<usize> = analysis
        .findings
        .iter()
        .enumerate()
        .filter(|(_, f)| f.auto())
        .map(|(i, _)| i)
        .collect();
    if chosen.is_empty() {
        println!("\nNothing to fix automatically.");
        return Ok(true);
    }
    let applied = apply(&analysis, &chosen)?;
    println!(
        "\nApplied {} fixes to {} files{}",
        applied.findings,
        applied.files.len(),
        if applied.skipped.is_empty() {
            String::new()
        } else {
            format!(" ({} skipped: overlapping)", applied.skipped.len())
        }
    );
    for f in &applied.files {
        println!("  {}", f.display());
    }
    let folder = suite_path.parent().unwrap_or(std::path::Path::new("."));
    let suites = proof::suites_in(folder);
    let mut progress = |s: String| eprintln!("{s}");
    let p = proof::prove(
        &applied.files,
        &suites,
        against,
        scope,
        CheckEnv {
            host,
            work: &work.join("proof"),
            progress: &mut progress,
            cancel: &cancel,
        },
    );
    println!("\nProof against {}:", against.describe());
    for s in &p.suites {
        let same = s
            .report
            .results
            .iter()
            .filter(|r| matches!(r.outcome, Outcome::Identical { .. }))
            .count();
        let total = s.report.results.len();
        let (was, now): (u64, u64) = s.report.results.iter().fold((0, 0), |(a, b), r| {
            (a + r.baseline_ms.unwrap_or(0), b + r.ms.unwrap_or(0))
        });
        println!(
            "  {}  {same} of {total} the same{}{}",
            s.suite
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            if was > 0 {
                format!(", {now} ms (was {was} ms)")
            } else {
                String::new()
            },
            s.report
                .error
                .as_deref()
                .map(|e| format!("  ERROR: {e}"))
                .unwrap_or_default()
        );
    }
    if !p.unaffected.is_empty() {
        println!(
            "  {} suites do not use the changed files",
            p.unaffected.len()
        );
    }
    if scope.is_some() {
        let failed: Vec<_> = p.compiled.iter().filter(|c| !c.ok).collect();
        println!(
            "  {} stylesheets using the changed files compiled, {} failed",
            p.compiled.len() - failed.len(),
            failed.len()
        );
        for c in failed.iter().take(10) {
            println!(
                "    {}: {}",
                c.stylesheet.display(),
                c.error.as_deref().unwrap_or("")
            );
        }
    }
    for e in &p.errors {
        println!("  ERROR: {e}");
    }
    println!(
        "\n{}",
        if p.passed() {
            "Proven: no output changed."
        } else {
            "NOT proven: see above."
        }
    );
    Ok(p.passed())
}
