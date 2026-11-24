//! `xylem-w3c`: runs the W3C conformance test suites against Xylem's
//! built-in XSLT engine and reports pass rates per test set.
//!
//! ```text
//! xylem-w3c fetch [--dir <folder>]                    # shallow-clone both suites
//! xylem-w3c qt3   [--dir <qt3tests>] [--set <prefix>] [--show <n>] [--out <file.md>]
//! xylem-w3c xslt  [--dir <xslt30-test>] [--set <prefix>] [--show <n>] [--out <file.md>]
//! ```
//!
//! The suites are fetched at run time into `target/w3c` (or `--dir`) and
//! never copied into the repository: neither repository states a licence,
//! so they are used here only for development, the way W3C's test-suite
//! policy describes, with no conformance claim.
//!
//! - **QT3** (`w3c/qt3tests`): XPath and XQuery tests. Only tests whose
//!   spec dependency includes XPath 2.0 (`XP20`, `XP20+`) run.
//! - **XSLT** (`w3c/xslt30-test`): tests whose spec dependency includes
//!   XSLT 1.0 or 2.0 (`XSLT10+`, `XSLT20+`, `XSLT20`) run.
//!
//! Tests that need something out of scope (schema awareness, other optional
//! features, collections, parameters given as expressions, ...) are
//! skipped and counted as such. Each test set runs in its own process, so
//! a crash or a hang in one set does not stop the run.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

mod catalog;
mod qt3;
mod xslt;

/// What became of one test.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Pass,
    Fail(String),
    Skip(String),
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            2
        }
    };
    std::process::exit(code);
}

fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/w3c")
}

fn run(args: &[String]) -> Result<(), String> {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    match cmd {
        "fetch" => {
            let dir = opt("--dir").map(PathBuf::from).unwrap_or_else(default_dir);
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            for repo in ["qt3tests", "xslt30-test"] {
                if dir.join(repo).exists() {
                    println!("{repo}: already there");
                    continue;
                }
                let ok = Command::new("git")
                    .args(["clone", "--depth", "1"])
                    .arg(format!("https://github.com/w3c/{repo}.git"))
                    .current_dir(&dir)
                    .status()
                    .map_err(|e| format!("git: {e}"))?
                    .success();
                if !ok {
                    return Err(format!("could not clone {repo}"));
                }
            }
            Ok(())
        }
        // Internal: run one test set and print a JSON line per test.
        "--one-set" => {
            let kind = args.get(1).ok_or("kind")?;
            let file = PathBuf::from(args.get(2).ok_or("file")?);
            let root = PathBuf::from(args.get(3).ok_or("root")?);
            let results = match kind.as_str() {
                "qt3" => qt3::run_set(&root, &file),
                _ => xslt::run_set(&root, &file),
            };
            let mut out = std::io::stdout().lock();
            for (name, v) in results {
                let (status, why) = match v {
                    Verdict::Pass => ("pass", String::new()),
                    Verdict::Fail(w) => ("fail", w),
                    Verdict::Skip(w) => ("skip", w),
                };
                let line = serde_json::json!({"test": name, "status": status, "why": why});
                let _ = writeln!(out, "{line}");
                let _ = out.flush();
            }
            Ok(())
        }
        "qt3" | "xslt" => {
            let dir = opt("--dir").map(PathBuf::from).unwrap_or_else(|| {
                default_dir().join(if cmd == "qt3" { "qt3tests" } else { "xslt30-test" })
            });
            if !dir.join("catalog.xml").exists() {
                return Err(format!(
                    "{} has no catalog.xml; run `xylem-w3c fetch` first",
                    dir.display()
                ));
            }
            let sets = catalog::test_sets(&dir)?;
            let filter = opt("--set");
            let show: usize = opt("--show").and_then(|s| s.parse().ok()).unwrap_or(0);
            run_suite(cmd, &dir, &sets, filter.as_deref(), show, opt("--out").map(PathBuf::from))
        }
        _ => Err(
            "usage: xylem-w3c fetch | qt3 | xslt [--dir <folder>] [--set <prefix>] [--show <n>] [--out <file.md>]"
                .into(),
        ),
    }
}

#[derive(Default, Clone, Copy)]
struct Tally {
    pass: usize,
    fail: usize,
    skip: usize,
}

fn run_suite(
    kind: &str,
    root: &Path,
    sets: &[(String, PathBuf)],
    filter: Option<&str>,
    show: usize,
    out: Option<PathBuf>,
) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut failures: Vec<(String, String, String)> = Vec::new();
    let mut skip_reasons: BTreeMap<String, usize> = BTreeMap::new();
    for (name, file) in sets {
        if filter.is_some_and(|f| !name.starts_with(f)) {
            continue;
        }
        let mut child = Command::new(&exe)
            .args(["--one-set", kind])
            .arg(file)
            .arg(root)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let stdout = child.stdout.take().unwrap();
        // A set that runs too long is stopped; its remaining tests count
        // as not run (they never reach the tally).
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let t = tallies.entry(name.clone()).or_default();
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match rx.recv_timeout(left) {
                Ok(line) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else {
                        continue;
                    };
                    let test = v["test"].as_str().unwrap_or("").to_string();
                    let why = v["why"].as_str().unwrap_or("").to_string();
                    match v["status"].as_str() {
                        Some("pass") => t.pass += 1,
                        Some("fail") => {
                            t.fail += 1;
                            failures.push((name.clone(), test, why));
                        }
                        _ => {
                            t.skip += 1;
                            *skip_reasons.entry(why).or_default() += 1;
                        }
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    failures.push((name.clone(), "(set)".into(), "timed out".into()));
                    break;
                }
                Err(_) => break,
            }
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            failures.push((
                name.clone(),
                "(set)".into(),
                format!("runner exited: {status}"),
            ));
        }
        eprintln!("{name}: {} pass, {} fail, {} skip", t.pass, t.fail, t.skip);
    }
    let total = tallies.values().fold(Tally::default(), |a, t| Tally {
        pass: a.pass + t.pass,
        fail: a.fail + t.fail,
        skip: a.skip + t.skip,
    });
    let run = total.pass + total.fail;
    let rate = if run == 0 {
        0.0
    } else {
        100.0 * total.pass as f64 / run as f64
    };
    println!(
        "\n{kind}: {} passed, {} failed ({rate:.1}% of those run), {} skipped",
        total.pass, total.fail, total.skip
    );
    for (set, test, why) in failures.iter().take(show) {
        println!("  FAIL {set}/{test}: {}", why.lines().next().unwrap_or(""));
    }
    if let Some(path) = out {
        let mut md = format!(
            "# W3C {kind} results\n\nPassed {} of {} run ({rate:.1}%); {} skipped.\n\n| Test set | Pass | Fail | Skip |\n|---|---:|---:|---:|\n",
            total.pass, run, total.skip
        );
        for (set, t) in &tallies {
            md.push_str(&format!(
                "| {set} | {} | {} | {} |\n",
                t.pass, t.fail, t.skip
            ));
        }
        md.push_str("\n## Why tests were skipped\n\n| Reason | Tests |\n|---|---:|\n");
        let mut reasons: Vec<_> = skip_reasons.into_iter().collect();
        reasons.sort_by_key(|r| std::cmp::Reverse(r.1));
        for (why, n) in reasons {
            md.push_str(&format!("| {why} | {n} |\n"));
        }
        std::fs::write(&path, md).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("written to {}", path.display());
    }
    Ok(())
}
