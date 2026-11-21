//! `xylem-import <src.xac|src-dir> <out-dir>` and
//! `xylem-import --all <root> <out-root>`.

use std::path::PathBuf;
use std::process::ExitCode;

use xylem_customize::{import_all, import_xmax, verify};

const USAGE: &str =
    "usage: xylem-import <src.xac|src-dir> <out-dir>\n       xylem-import --all <root> <out-root>";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--all", root, out_root] => run_all(PathBuf::from(root), PathBuf::from(out_root)),
        [source, out_dir] if !source.starts_with("--") => {
            run_one(PathBuf::from(source), PathBuf::from(out_dir))
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn run_one(source: PathBuf, out_dir: PathBuf) -> ExitCode {
    let outcome =
        import_xmax(&source, &out_dir).and_then(|report| verify(&out_dir).map(|()| report));
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

fn run_all(root: PathBuf, out_root: PathBuf) -> ExitCode {
    let entries = match import_all(&root, &out_root) {
        Ok(entries) => entries,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (mut ok, mut failed, mut warnings) = (0, 0, 0);
    for entry in &entries {
        match &entry.result {
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
