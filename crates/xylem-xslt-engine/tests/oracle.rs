//! Compares the engine's output with the reference processor's.
//!
//! `tests/cases/<case>/expected.xml` was written by the reference host
//! (`tools/saxon-dotnet`) from `tests/cases/input.xml` with the parameters
//! in [`PARAMS`]. The comparison is `xylem-xslt`'s, so generated IDs are
//! renumbered before comparing.
//!
//! Two more tests run only when asked:
//!
//! - `live_oracle` runs the host itself on every case (set
//!   `XYLEM_SAXON_HOST` to `SaxonDotnetHost.exe`). With
//!   `XYLEM_XSLT_ENGINE_REGEN=1` it rewrites the expected files
//!   (`XYLEM_XSLT_ENGINE_CASES=parity,edge` limits either test to those cases).
//! - `suite` runs a stylesheet check suite (`XYLEM_XSLT_ENGINE_SUITE` = a
//!   suite `.toml`) and reports how many saved outputs the engine
//!   reproduces. Suites hold real documents and are never committed.
//!   `XYLEM_XSLT_ENGINE_STYLESHEET` overrides the suite's stylesheet,
//!   `XYLEM_XSLT_ENGINE_AGAINST_HOST=1` (with `XYLEM_SAXON_HOST`) compares
//!   with the host's output instead of the saved one, and
//!   `XYLEM_XSLT_ENGINE_LIMIT` runs only the first tests.
//!
//! ```text
//! XYLEM_SAXON_HOST=C:\path\SaxonDotnetHost.exe cargo test -p xylem-xslt-engine --test oracle -- --ignored
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xylem_xslt::compare::compare;
use xylem_xslt_engine::{Stylesheet, transform_file};

const CASES: &[&str] = &[
    "basics",
    "xpath2",
    "params",
    "numbering",
    "imports",
    "ids",
    "edge",
    "docs",
    "html1",
    "html2",
    "parity",
    "parity1",
];

/// The cases to run: all, or those named in `XYLEM_XSLT_ENGINE_CASES`
/// (comma-separated), so one case can be regenerated alone.
fn cases() -> Vec<&'static str> {
    match std::env::var("XYLEM_XSLT_ENGINE_CASES") {
        Ok(only) => CASES
            .iter()
            .copied()
            .filter(|c| only.split(',').any(|o| o.trim() == *c))
            .collect(),
        Err(_) => CASES.to_vec(),
    }
}
const PARAMS: &[(&str, &str)] = &[("mode", "final"), ("pageCount", "4")];

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cases")
}

/// A case's own `input.xml`, or the shared one.
fn input_of(case: &str) -> PathBuf {
    let own = cases_dir().join(case).join("input.xml");
    if own.is_file() {
        own
    } else {
        cases_dir().join("input.xml")
    }
}

fn params() -> HashMap<String, String> {
    PARAMS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn run_engine(
    stylesheet: &Path,
    input: &Path,
    params: &HashMap<String, String>,
) -> Result<Vec<u8>, String> {
    let ss = Stylesheet::compile(stylesheet)?;
    let out = transform_file(&ss, input, params)?;
    // A case folder holding a `serialize` file is the host's second step:
    // the result written as `xsl:output` asks (UTF-8 with a BOM).
    if stylesheet.with_file_name("serialize").exists() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(out.serialized.as_bytes());
        return Ok(b);
    }
    Ok(out.host_bytes())
}

fn describe(c: &xylem_xslt::compare::Comparison) -> String {
    c.differences
        .iter()
        .map(|d| {
            format!(
                "  at {}: expected {:?}, got {:?}",
                d.path, d.baseline, d.candidate
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn cases_match_reference_output() {
    let dir = cases_dir();
    let mut failures = Vec::new();
    for case in cases() {
        let style = dir.join(case).join("style.xsl");
        let expected = std::fs::read(dir.join(case).join("expected.xml")).unwrap();
        match run_engine(&style, &input_of(case), &params()) {
            Ok(got) => {
                let c = compare(&expected, &got);
                if !c.identical {
                    failures.push(format!(
                        "{case}: {} difference(s)\n{}",
                        c.count,
                        describe(&c)
                    ));
                }
            }
            Err(e) => failures.push(format!("{case}: {e}")),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

fn host() -> Option<PathBuf> {
    std::env::var_os("XYLEM_SAXON_HOST")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
}

#[test]
#[ignore = "needs XYLEM_SAXON_HOST"]
fn live_oracle() {
    let Some(exe) = host() else {
        eprintln!("XYLEM_SAXON_HOST is not set; skipping");
        return;
    };
    let host = xylem_xslt::host::Host::new(exe);
    let dir = cases_dir();
    let regen = std::env::var_os("XYLEM_XSLT_ENGINE_REGEN").is_some();
    let tmp = std::env::temp_dir().join(format!("xylem-xslt-engine-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let p: std::collections::BTreeMap<String, String> = params().into_iter().collect();
    let mut failures = Vec::new();
    for case in cases() {
        let style = dir.join(case).join("style.xsl");
        let out = if regen {
            dir.join(case).join("expected.xml")
        } else {
            tmp.join(format!("{case}.xml"))
        };
        let mut host = host.clone();
        host.serialize = dir.join(case).join("serialize").exists();
        let run = host.run(&style, &[(input_of(case), out.clone())], &p, false);
        if let Some(e) = run.jobs.first().and_then(|j| j.error.clone()).or(run.error) {
            failures.push(format!("{case}: host failed: {e}"));
            continue;
        }
        let reference = std::fs::read(&out).unwrap();
        let ours = run_engine(&style, &input_of(case), &params()).unwrap_or_default();
        let c = compare(&reference, &ours);
        if !c.identical {
            failures.push(format!("{case}: {}", describe(&c)));
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
}

#[test]
#[ignore = "needs XYLEM_XSLT_ENGINE_SUITE"]
fn suite() {
    let Some(path) = std::env::var_os("XYLEM_XSLT_ENGINE_SUITE") else {
        eprintln!("XYLEM_XSLT_ENGINE_SUITE is not set; skipping");
        return;
    };
    let suite = xylem_xslt::suite::Suite::load(Path::new(&path)).unwrap();
    let style = std::env::var_os("XYLEM_XSLT_ENGINE_STYLESHEET")
        .map(PathBuf::from)
        .or(suite.stylesheet.clone())
        .expect("suite names no stylesheet; set XYLEM_XSLT_ENGINE_STYLESHEET");
    // Compare with the live host instead of the saved outputs.
    let against_host =
        host().filter(|_| std::env::var_os("XYLEM_XSLT_ENGINE_AGAINST_HOST").is_some());
    let limit: usize = std::env::var("XYLEM_XSLT_ENGINE_LIMIT")
        .ok()
        .and_then(|l| l.parse().ok())
        .unwrap_or(usize::MAX);
    let tmp = std::env::temp_dir().join(format!("xylem-xslt-engine-suite-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let compiled = std::time::Instant::now();
    let ss = match Stylesheet::compile(&style) {
        Ok(s) => s,
        Err(e) => panic!("stylesheet does not compile yet: {e}"),
    };
    eprintln!("compiled in {:?}", compiled.elapsed());
    let (mut same, mut total) = (0, 0);
    for t in suite.tests.iter().take(limit) {
        let expected = match &against_host {
            Some(exe) => {
                let out = tmp.join(format!("{total}.xml"));
                let run = xylem_xslt::host::Host::new(exe).run(
                    &style,
                    &[(t.input.clone(), out.clone())],
                    &t.params,
                    false,
                );
                if let Some(e) = run.jobs.first().and_then(|j| j.error.clone()).or(run.error) {
                    eprintln!(
                        "{}: host failed: {}",
                        t.name,
                        e.lines().next().unwrap_or("")
                    );
                    continue;
                }
                out
            }
            None => match &t.expected {
                Some(e) => e.clone(),
                None => continue,
            },
        };
        total += 1;
        let params: HashMap<String, String> = t.params.clone().into_iter().collect();
        let started = std::time::Instant::now();
        let got = transform_file(&ss, &t.input, &params).map(|o| o.host_bytes());
        match got {
            Ok(got) => {
                if let Some(dir) = std::env::var_os("XYLEM_XSLT_ENGINE_DUMP") {
                    let _ = std::fs::write(
                        Path::new(&dir)
                            .join(format!("{}.xml", t.name.replace(['/', '\\', ':'], "_"))),
                        &got,
                    );
                }
                let c = compare(&std::fs::read(expected).unwrap(), &got);
                if c.identical {
                    same += 1;
                }
                eprintln!(
                    "{}: {} difference(s) in {:?}",
                    t.name,
                    c.count,
                    started.elapsed()
                );
                if !c.identical {
                    eprintln!("{}", describe(&c));
                }
            }
            Err(e) => eprintln!("{}: error: {}", t.name, e.lines().next().unwrap_or("")),
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    eprintln!("{same} of {total} outputs identical");
}
