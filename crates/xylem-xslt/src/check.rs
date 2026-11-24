//! Running a check: the stylesheet under test on every test input, compared
//! with a baseline.

use crate::compare::{Comparison, compare};
use crate::host::{Host, JobResult};
use crate::suite::{Suite, Test};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// What the stylesheet's output is compared with.
#[derive(Debug, Clone, PartialEq)]
pub enum Baseline {
    /// The output the publishing service saved (`expected` in the suite).
    Saved,
    /// The same stylesheet as committed to git at a revision (`HEAD` for
    /// the last commit).
    Committed { rev: String },
    /// Another stylesheet's output.
    Stylesheet(PathBuf),
}

impl Baseline {
    pub fn describe(&self) -> String {
        match self {
            Baseline::Saved => "the saved output".into(),
            Baseline::Committed { rev } if rev == "HEAD" => "the last committed version".into(),
            Baseline::Committed { rev } => format!("the version committed at {rev}"),
            Baseline::Stylesheet(p) => p.display().to_string(),
        }
    }
}

/// What became of one test.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Same output (after ignoring generated IDs). `byte_identical` when
    /// not even those differed.
    Identical {
        byte_identical: bool,
    },
    Different(Comparison),
    /// The test has no saved output to compare with.
    NoBaseline,
    /// A stylesheet failed on this input or a file could not be read.
    Failed(String),
}

/// The same stylesheet's output on a second engine, compared with the
/// engine under test (see [`compare_engines`]).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineComparison {
    /// Same output (after ignoring generated IDs).
    Same,
    Different(Comparison),
    /// One engine failed, or an output could not be read.
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct TestResult {
    pub name: String,
    pub outcome: Outcome,
    /// Transform time of the stylesheet under test, in milliseconds.
    pub ms: Option<u64>,
    /// Transform time of the baseline stylesheet, when one ran.
    pub baseline_ms: Option<u64>,
    pub output: Option<PathBuf>,
    pub baseline_output: Option<PathBuf>,
    /// The reference engine's verdict, when [`compare_engines`] ran.
    pub engines: Option<EngineComparison>,
    /// The reference engine's output, when it ran.
    pub reference_output: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CheckReport {
    pub stylesheet: PathBuf,
    pub baseline: Baseline,
    /// For a committed baseline: the commit compared with.
    pub baseline_commit: Option<String>,
    pub results: Vec<TestResult>,
    /// Set when the check could not run at all.
    pub error: Option<String>,
    pub cancelled: bool,
    /// The second engine the outputs were compared with, if any.
    pub reference_engine: Option<PathBuf>,
}

impl CheckReport {
    /// Whether every test gave the same output on both engines (true when
    /// no second engine ran).
    pub fn engines_agree(&self) -> bool {
        self.results
            .iter()
            .all(|r| matches!(r.engines, None | Some(EngineComparison::Same)))
    }

    pub fn all_identical(&self) -> bool {
        self.error.is_none()
            && !self.cancelled
            && self
                .results
                .iter()
                .all(|r| matches!(r.outcome, Outcome::Identical { .. }))
    }
}

/// Where outputs go and how progress is reported.
pub struct CheckEnv<'a> {
    pub host: &'a Host,
    /// Folder for outputs (`<work>/candidate`, `<work>/baseline`) and the
    /// committed-version cache (`<work>/../committed`).
    pub work: &'a Path,
    pub progress: &'a mut dyn FnMut(String),
    pub cancel: &'a AtomicBool,
}

fn file_stem_for(i: usize, test: &Test) -> String {
    let clean: String = test
        .name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    format!("{:02}-{clean}.out.xml", i + 1)
}

/// Runs `stylesheet` on every test, grouped so tests with the same
/// parameters share one compile. Results are in test order.
fn run_all(
    host: &Host,
    stylesheet: &Path,
    tests: &[Test],
    out_dir: &Path,
    cancel: &AtomicBool,
    profile: bool,
) -> Vec<Option<JobResult>> {
    let mut groups: BTreeMap<&BTreeMap<String, String>, Vec<usize>> = BTreeMap::new();
    for (i, t) in tests.iter().enumerate() {
        groups.entry(&t.params).or_default().push(i);
    }
    let mut results: Vec<Option<JobResult>> = vec![None; tests.len()];
    for (params, members) in groups {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let jobs: Vec<(PathBuf, PathBuf)> = members
            .iter()
            .map(|&i| {
                (
                    tests[i].input.clone(),
                    out_dir.join(file_stem_for(i, &tests[i])),
                )
            })
            .collect();
        let run = host.run(stylesheet, &jobs, params, profile);
        for (&i, job) in members.iter().zip(run.jobs) {
            results[i] = Some(job);
        }
    }
    results
}

/// Checks `stylesheet` against `baseline` on every test of `suite`.
pub fn check(suite: &Suite, stylesheet: &Path, baseline: &Baseline, env: CheckEnv) -> CheckReport {
    let mut report = CheckReport {
        stylesheet: stylesheet.to_path_buf(),
        baseline: baseline.clone(),
        baseline_commit: None,
        results: Vec::new(),
        error: None,
        cancelled: false,
        reference_engine: None,
    };
    let tests = &suite.tests;
    let candidate_dir = env.work.join("candidate");
    let baseline_dir = env.work.join("baseline");
    for d in [&candidate_dir, &baseline_dir] {
        let _ = std::fs::remove_dir_all(d);
        if let Err(e) = std::fs::create_dir_all(d) {
            report.error = Some(format!("Could not create {}: {e}", d.display()));
            return report;
        }
    }

    // The baseline stylesheet, when the baseline is made by running one.
    let baseline_sheet = match baseline {
        Baseline::Saved => None,
        Baseline::Stylesheet(p) => Some(p.clone()),
        Baseline::Committed { rev } => {
            (env.progress)(format!("Getting {}…", baseline.describe()));
            let cache = env.work.parent().unwrap_or(env.work).join("committed");
            match crate::git::committed_copy(stylesheet, rev, &cache) {
                Ok((copy, commit)) => {
                    report.baseline_commit = Some(commit);
                    Some(copy)
                }
                Err(e) => {
                    report.error = Some(e);
                    return report;
                }
            }
        }
    };

    (env.progress)(format!(
        "Running the stylesheet on {} test{}…",
        tests.len(),
        plural(tests.len())
    ));
    let candidate = run_all(
        env.host,
        stylesheet,
        tests,
        &candidate_dir,
        env.cancel,
        false,
    );
    let baseline_runs = match &baseline_sheet {
        Some(sheet) if !env.cancel.load(Ordering::Relaxed) => {
            (env.progress)(format!("Running {}…", baseline.describe()));
            Some(run_all(
                env.host,
                sheet,
                tests,
                &baseline_dir,
                env.cancel,
                false,
            ))
        }
        _ => None,
    };
    if env.cancel.load(Ordering::Relaxed) {
        report.cancelled = true;
        return report;
    }

    for (i, test) in tests.iter().enumerate() {
        (env.progress)(format!("Comparing {} of {}…", i + 1, tests.len()));
        let ran = candidate[i].clone();
        let base_ran = baseline_runs.as_ref().and_then(|b| b[i].clone());
        let mut result = TestResult {
            name: test.name.clone(),
            outcome: Outcome::NoBaseline,
            ms: ran.as_ref().filter(|j| j.ok).map(|j| j.ms),
            baseline_ms: base_ran.as_ref().filter(|j| j.ok).map(|j| j.ms),
            output: ran.as_ref().filter(|j| j.ok).map(|j| j.output.clone()),
            baseline_output: None,
            engines: None,
            reference_output: None,
        };
        result.outcome = (|| {
            let ran = ran.ok_or("The stylesheet did not run.")?;
            if !ran.ok {
                return Err(format!(
                    "The stylesheet failed: {}",
                    ran.error.unwrap_or_else(|| "no reason given".into())
                ));
            }
            let base_path = match (&baseline_sheet, &base_ran) {
                (None, _) => match &test.expected {
                    Some(p) => p.clone(),
                    None => return Ok(Outcome::NoBaseline),
                },
                (Some(_), Some(b)) if b.ok => b.output.clone(),
                (Some(_), Some(b)) => {
                    return Err(format!(
                        "The baseline stylesheet failed: {}",
                        b.error.clone().unwrap_or_else(|| "no reason given".into())
                    ));
                }
                (Some(_), None) => return Err("The baseline stylesheet did not run.".into()),
            };
            result.baseline_output = Some(base_path.clone());
            let a =
                std::fs::read(&base_path).map_err(|e| format!("{}: {e}", base_path.display()))?;
            let b =
                std::fs::read(&ran.output).map_err(|e| format!("{}: {e}", ran.output.display()))?;
            let c = compare(&a, &b);
            Ok(if c.identical {
                Outcome::Identical {
                    byte_identical: c.byte_identical,
                }
            } else {
                Outcome::Different(c)
            })
        })()
        .unwrap_or_else(Outcome::Failed);
        report.results.push(result);
    }
    report
}

/// Runs the stylesheet under test on a second engine (`reference`) and
/// records, per test, whether its output matches the first engine's. Used
/// while two engines exist, to catch any difference between them early:
/// say Xylem's built-in engine checked against the .NET host.
pub fn compare_engines(
    report: &mut CheckReport,
    suite: &Suite,
    reference: &Host,
    work: &Path,
    progress: &mut dyn FnMut(String),
    cancel: &AtomicBool,
) {
    if report.error.is_some() || report.cancelled {
        return;
    }
    let dir = work.join("reference");
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        report.error = Some(format!("Could not create {}: {e}", dir.display()));
        return;
    }
    report.reference_engine = Some(reference.program.clone());
    progress(format!(
        "Running the stylesheet on {}…",
        reference.program.display()
    ));
    let runs = run_all(
        reference,
        &report.stylesheet,
        &suite.tests,
        &dir,
        cancel,
        false,
    );
    if cancel.load(Ordering::Relaxed) {
        report.cancelled = true;
        return;
    }
    for (result, run) in report.results.iter_mut().zip(runs) {
        let verdict = match (run, &result.output) {
            (None, _) => EngineComparison::Failed("The reference engine did not run.".into()),
            (Some(r), _) if !r.ok => match &result.output {
                // Both failing is agreement of a kind, but worth seeing.
                None => EngineComparison::Failed(format!(
                    "Both engines failed; the reference said: {}",
                    r.error.unwrap_or_default()
                )),
                Some(_) => EngineComparison::Failed(format!(
                    "Only the reference engine failed: {}",
                    r.error.unwrap_or_default()
                )),
            },
            (Some(_), None) => {
                EngineComparison::Failed("Only the engine under test failed.".into())
            }
            (Some(r), Some(ours)) => {
                result.reference_output = Some(r.output.clone());
                match (std::fs::read(&r.output), std::fs::read(ours)) {
                    (Ok(a), Ok(b)) => {
                        let c = compare(&a, &b);
                        if c.identical {
                            EngineComparison::Same
                        } else {
                            EngineComparison::Different(c)
                        }
                    }
                    (Err(e), _) | (_, Err(e)) => EngineComparison::Failed(e.to_string()),
                }
            }
        };
        result.engines = Some(verdict);
    }
}

pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Timings for one test.
#[derive(Debug, Clone)]
pub struct TestProfile {
    pub name: String,
    pub profile: Result<crate::profile::Profile, String>,
}

/// Where a stylesheet spends its time on a suite.
#[derive(Debug, Clone)]
pub struct ProfileReport {
    pub stylesheet: PathBuf,
    pub tests: Vec<TestProfile>,
    /// All tests added together.
    pub combined: crate::profile::Profile,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Runs `stylesheet` on every test with timing on.
pub fn profile(suite: &Suite, stylesheet: &Path, env: CheckEnv) -> ProfileReport {
    let mut report = ProfileReport {
        stylesheet: stylesheet.to_path_buf(),
        tests: Vec::new(),
        combined: Default::default(),
        error: None,
        cancelled: false,
    };
    let dir = env.work.join("profile");
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        report.error = Some(format!("Could not create {}: {e}", dir.display()));
        return report;
    }
    // The .NET host times XSLT 2.0 only; the built-in engine times both.
    match crate::stylesheet::runs_as_xslt2(stylesheet) {
        Ok(true) => {}
        Ok(false) if env.host.is_builtin() => {}
        Ok(false) => {
            report.error = Some(
                "The .NET host times XSLT 2.0 stylesheets only, and this one runs as XSLT 1.0 (its version is not 2.0). Leave the engine empty (or builtin) to time it on Xylem's engine."
                    .into(),
            );
            return report;
        }
        Err(e) => {
            report.error = Some(e);
            return report;
        }
    }
    let n = suite.tests.len();
    (env.progress)(format!("Timing the stylesheet on {n} test{}…", plural(n)));
    let runs = run_all(env.host, stylesheet, &suite.tests, &dir, env.cancel, true);
    if env.cancel.load(Ordering::Relaxed) {
        report.cancelled = true;
        return report;
    }
    let mut loaded = Vec::new();
    for (test, run) in suite.tests.iter().zip(runs) {
        let profile = match run {
            None => Err("The stylesheet did not run.".to_string()),
            Some(j) if !j.ok => Err(format!(
                "The stylesheet failed: {}",
                j.error.unwrap_or_else(|| "no reason given".into())
            )),
            Some(j) => match j.profile {
                Some(p) => crate::profile::Profile::load(&p),
                None => {
                    Err("No timings were written (only XSLT 2.0 stylesheets can be timed).".into())
                }
            },
        };
        if let Ok(p) = &profile {
            loaded.push(p.clone());
        }
        report.tests.push(TestProfile {
            name: test.name.clone(),
            profile,
        });
    }
    report.combined = crate::profile::combine(&loaded);
    report
}
