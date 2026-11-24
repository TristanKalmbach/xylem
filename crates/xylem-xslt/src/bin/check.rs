//! `xylem-xslt-check`: checks that a stylesheet still produces the same
//! output on a suite of test inputs.
//!
//! ```text
//! xylem-xslt-check <suite.toml> [--stylesheet <file.xsl>]
//!                  [--against saved | committed[:<rev>] | <other.xsl>]
//!                  [--host <SaxonDotnetHost.exe>] [--work <folder>]
//! ```
//!
//! `--against` defaults to `committed` (the stylesheet as last committed to
//! git) when the suite has no saved output, and to `saved` when it does.
//! The host defaults to `XYLEM_XSLT_HOST`. Exit code 0 means every test gave
//! the same output.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use xylem_xslt::check::CheckEnv;
use xylem_xslt::{Baseline, Host, Outcome, Suite, check};

const USAGE: &str = "Usage: xylem-xslt-check <suite.toml> [--stylesheet <file.xsl>] \
[--against saved|committed[:<rev>]|<other.xsl>] [--host <program>] [--work <folder>]";

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
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value.\n{USAGE}"));
        match a.as_str() {
            "--stylesheet" => stylesheet = Some(PathBuf::from(value("--stylesheet")?)),
            "--against" => against = Some(value("--against")?),
            "--host" => host = Some(PathBuf::from(value("--host")?)),
            "--work" => work = Some(PathBuf::from(value("--work")?)),
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
    let host =
        Host::new(host.ok_or(
            "No host program: set XYLEM_XSLT_HOST or pass --host (see tools/saxon-dotnet).",
        )?);
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
