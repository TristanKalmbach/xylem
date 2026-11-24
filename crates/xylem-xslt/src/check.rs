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
}

impl CheckReport {
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
        let run = host.run(stylesheet, &jobs, params);
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
    let candidate = run_all(env.host, stylesheet, tests, &candidate_dir, env.cancel);
    let baseline_runs = match &baseline_sheet {
        Some(sheet) if !env.cancel.load(Ordering::Relaxed) => {
            (env.progress)(format!("Running {}…", baseline.describe()));
            Some(run_all(env.host, sheet, tests, &baseline_dir, env.cancel))
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

pub(crate) fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}
