//! `xylem-import [--protect-pi TARGET]... <src.xac|src-dir> <out-dir>` and
//! `xylem-import [--protect-pi TARGET]... --all <root> <out-root>`.
//!
//! `--protect-pi` marks extra processing-instruction targets as protected in
//! every imported package. The importer already protects PIs that the
//! stylesheets style; use this for markers your old host locked down in code.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xylem_customize::{DoctypePackage, import_all, import_xmax, verify};

const USAGE: &str = "usage: xylem-import [--protect-pi TARGET]... <src.xac|src-dir> <out-dir>\n       xylem-import [--protect-pi TARGET]... --all <root> <out-root>";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mut protect = Vec::new();
    while let Some(i) = args.iter().position(|a| a == "--protect-pi") {
        if i + 1 >= args.len() {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
        protect.push(args.remove(i + 1));
        args.remove(i);
    }
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--all", root, out_root] => {
            run_all(PathBuf::from(root), PathBuf::from(out_root), &protect)
        }
        [source, out_dir] if !source.starts_with("--") => {
            run_one(PathBuf::from(source), PathBuf::from(out_dir), &protect)
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Adds `targets` to the package's protected PI list.
fn protect_pis(dir: &Path, targets: &[String]) -> xylem_customize::Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let mut pkg = DoctypePackage::load(dir)?;
    for t in targets {
        if !pkg.behaviour.protected_pis.contains(t) {
            pkg.behaviour.protected_pis.push(t.clone());
        }
    }
    pkg.save(dir)
}

fn run_one(source: PathBuf, out_dir: PathBuf, protect: &[String]) -> ExitCode {
    let outcome = import_xmax(&source, &out_dir)
        .and_then(|report| protect_pis(&out_dir, protect).map(|()| report))
        .and_then(|report| verify(&out_dir).map(|()| report));
    match outcome {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {}: {e}", source.display());
            ExitCode::FAILURE
        }
    }
}

fn run_all(root: PathBuf, out_root: PathBuf, protect: &[String]) -> ExitCode {
    let entries = match import_all(&root, &out_root) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (mut ok, mut failed, mut warnings) = (0, 0, 0);
    for entry in &entries {
        let result = entry
            .result
            .as_ref()
            .map_err(|e| e.to_string())
            .and_then(|report| {
                protect_pis(&entry.out_dir, protect)
                    .map(|()| report)
                    .map_err(|e| e.to_string())
            });
        match result {
            Ok(report) => {
                ok += 1;
                warnings += report.warnings.len();
                println!(
                    "ok    {} <- {} ({} warnings)",
                    entry.job.dir_name,
                    entry.job.source.display(),
                    report.warnings.len()
                );
                for w in &report.warnings {
                    println!("        warning: {w}");
                }
            }
            Err(e) => {
                failed += 1;
                println!(
                    "FAIL  {} <- {}: {e}",
                    entry.job.dir_name,
                    entry.job.source.display()
                );
            }
        }
    }
    println!(
        "{} sets: {ok} imported and loaded back, {failed} failed, {warnings} warnings",
        entries.len()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
