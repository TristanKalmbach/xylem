//! Differential runs: every top-level stylesheet in a folder, on every
//! input, with every set of parameters, through two engines (a reference,
//! such as the engine a publishing service runs today, and a candidate,
//! such as Xylem's own), comparing the outputs. This is how a team shows
//! that one engine can replace another for all of its stylesheets, not only
//! the ones it has saved output for.
//!
//! Outputs that are the same are deleted as soon as they are compared, so a
//! large run does not fill the disk; outputs that differ are kept for
//! inspection. Results are appended to `results.jsonl` in the work folder
//! as each stylesheet finishes, and a run started again with the same work
//! folder skips what is already there.

use crate::compare::{Comparison, compare};
use crate::host::{Host, Run};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// What became of one stylesheet on one input with one set of parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Same output (after renumbering generated IDs).
    Same { byte_identical: bool },
    /// Different output: how many places, the first few, and where both
    /// outputs were kept.
    Different {
        places: usize,
        first: Vec<String>,
        reference: PathBuf,
        candidate: PathBuf,
    },
    /// Both engines failed. Agreeing on failure counts as agreement; the
    /// messages are kept for review.
    BothFailed {
        reference: String,
        candidate: String,
    },
    /// Only the reference engine failed.
    ReferenceFailed { error: String },
    /// Only the candidate engine failed.
    CandidateFailed { error: String },
}

impl Outcome {
    /// The engines agree: same output, or both failed.
    pub fn agrees(&self) -> bool {
        matches!(self, Outcome::Same { .. } | Outcome::BothFailed { .. })
    }
}

/// One stylesheet × input × parameter set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Case {
    pub stylesheet: PathBuf,
    pub input: PathBuf,
    pub params: BTreeMap<String, String>,
    #[serde(flatten)]
    pub outcome: Outcome,
    /// Transform times, in milliseconds, where the engine reported them.
    pub reference_ms: Option<u64>,
    pub candidate_ms: Option<u64>,
}

/// What to run.
pub struct Plan<'a> {
    pub stylesheets: &'a [PathBuf],
    pub inputs: &'a [PathBuf],
    /// Each set of parameters is a separate pass over every input.
    pub param_sets: &'a [BTreeMap<String, String>],
    pub reference: &'a Host,
    pub candidate: &'a Host,
    pub work: &'a Path,
    /// Stylesheets run at the same time.
    pub threads: usize,
}

/// Name of the results file in the work folder.
pub const RESULTS: &str = "results.jsonl";

/// Reads the results a run has written so far.
pub fn load_results(work: &Path) -> Vec<Case> {
    std::fs::read_to_string(work.join(RESULTS))
        .map(|t| {
            t.lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// A stable short name for a stylesheet's output folder.
fn slug(stylesheet: &Path, index: usize) -> String {
    let stem: String = stylesheet
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    format!("{index:03}-{stem}")
}

fn params_key(p: &BTreeMap<String, String>) -> String {
    p.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Runs the plan. `progress` is told about each finished stylesheet;
/// `cancel` stops the run between stylesheets. Returns every case, those
/// from earlier runs in the same work folder included.
pub fn run(
    plan: &Plan<'_>,
    progress: &(dyn Fn(String) + Sync),
    cancel: &AtomicBool,
) -> std::io::Result<Vec<Case>> {
    std::fs::create_dir_all(plan.work)?;
    let done: HashSet<(String, String)> = load_results(plan.work)
        .iter()
        .map(|c| {
            (
                crate::stylesheet::path_key(&c.stylesheet),
                params_key(&c.params),
            )
        })
        .collect();
    let units: Vec<(usize, &PathBuf, &BTreeMap<String, String>)> = plan
        .stylesheets
        .iter()
        .enumerate()
        .flat_map(|(i, s)| plan.param_sets.iter().map(move |p| (i, s, p)))
        .filter(|(_, s, p)| !done.contains(&(crate::stylesheet::path_key(s), params_key(p))))
        .collect();
    let total = units.len();
    let next = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let file = Mutex::new(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(plan.work.join(RESULTS))?,
    );
    std::thread::scope(|scope| {
        for _ in 0..plan.threads.max(1) {
            scope.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let n = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(index, stylesheet, params)) = units.get(n) else {
                        return;
                    };
                    let cases = run_unit(plan, index, stylesheet, params);
                    let disagree = cases.iter().filter(|c| !c.outcome.agrees()).count();
                    if let Ok(mut f) = file.lock() {
                        for c in &cases {
                            if let Ok(line) = serde_json::to_string(c) {
                                let _ = writeln!(f, "{line}");
                            }
                        }
                        let _ = f.flush();
                    }
                    let k = finished.fetch_add(1, Ordering::Relaxed) + 1;
                    progress(format!(
                        "[{k}/{total}] {} ({}): {}",
                        stylesheet.display(),
                        params_key(params),
                        if disagree == 0 {
                            format!("{} inputs agree", cases.len())
                        } else {
                            format!("{disagree} of {} differ", cases.len())
                        }
                    ));
                }
            });
        }
    });
    Ok(load_results(plan.work))
}

/// One stylesheet with one parameter set, on every input, through both
/// engines.
fn run_unit(
    plan: &Plan<'_>,
    index: usize,
    stylesheet: &Path,
    params: &BTreeMap<String, String>,
) -> Vec<Case> {
    let dir = plan
        .work
        .join(slug(stylesheet, index))
        .join(params_key(params).replace(['=', '&', '/', '\\', ':'], "_"));
    let _ = std::fs::create_dir_all(dir.join("reference"));
    let _ = std::fs::create_dir_all(dir.join("candidate"));
    let jobs = |side: &str| -> Vec<(PathBuf, PathBuf)> {
        plan.inputs
            .iter()
            .enumerate()
            .map(|(i, input)| {
                let stem = input
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (
                    input.clone(),
                    dir.join(side).join(format!("{i:03}-{stem}.out")),
                )
            })
            .collect()
    };
    let (ref_jobs, cand_jobs) = (jobs("reference"), jobs("candidate"));
    let reference = plan.reference.run(stylesheet, &ref_jobs, params, false);
    let candidate = plan.candidate.run(stylesheet, &cand_jobs, params, false);
    let job_error = |run: &Run, i: usize| -> Option<String> {
        match run.jobs.get(i) {
            Some(j) if j.ok => None,
            Some(j) => Some(j.error.clone().unwrap_or_else(|| "failed".into())),
            None => Some(run.error.clone().unwrap_or_else(|| "did not run".into())),
        }
    };
    let mut cases = Vec::new();
    for (i, input) in plan.inputs.iter().enumerate() {
        let (r_out, c_out) = (&ref_jobs[i].1, &cand_jobs[i].1);
        let outcome = match (job_error(&reference, i), job_error(&candidate, i)) {
            (Some(r), Some(c)) => Outcome::BothFailed {
                reference: r,
                candidate: c,
            },
            (Some(error), None) => Outcome::ReferenceFailed { error },
            (None, Some(error)) => Outcome::CandidateFailed { error },
            (None, None) => match (std::fs::read(r_out), std::fs::read(c_out)) {
                (Ok(a), Ok(b)) => {
                    let c: Comparison = compare(&a, &b);
                    if c.identical {
                        Outcome::Same {
                            byte_identical: c.byte_identical,
                        }
                    } else {
                        Outcome::Different {
                            places: c.count,
                            first: c
                                .differences
                                .iter()
                                .take(3)
                                .map(|d| {
                                    format!("{}: {:?} → {:?}", d.path, d.baseline, d.candidate)
                                })
                                .collect(),
                            reference: r_out.clone(),
                            candidate: c_out.clone(),
                        }
                    }
                }
                (Err(e), _) => Outcome::ReferenceFailed {
                    error: format!("output unreadable: {e}"),
                },
                (_, Err(e)) => Outcome::CandidateFailed {
                    error: format!("output unreadable: {e}"),
                },
            },
        };
        if outcome.agrees() {
            let _ = std::fs::remove_file(r_out);
            let _ = std::fs::remove_file(c_out);
        }
        cases.push(Case {
            stylesheet: stylesheet.to_path_buf(),
            input: input.clone(),
            params: params.clone(),
            outcome,
            reference_ms: reference.jobs.get(i).filter(|j| j.ok).map(|j| j.ms),
            candidate_ms: candidate.jobs.get(i).filter(|j| j.ok).map(|j| j.ms),
        });
    }
    // Leave nothing behind for a unit that agreed everywhere.
    let _ = std::fs::remove_dir(dir.join("reference"));
    let _ = std::fs::remove_dir(dir.join("candidate"));
    let _ = std::fs::remove_dir(&dir);
    if let Some(parent) = dir.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    cases
}

/// Totals for a summary.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Totals {
    pub cases: usize,
    pub same: usize,
    pub byte_identical: usize,
    pub both_failed: usize,
    pub different: usize,
    pub reference_failed: usize,
    pub candidate_failed: usize,
    pub reference_ms: u64,
    pub candidate_ms: u64,
    /// Cases where both engines produced output (the times compare).
    pub timed: usize,
}

pub fn totals(cases: &[Case]) -> Totals {
    let mut t = Totals {
        cases: cases.len(),
        ..Totals::default()
    };
    for c in cases {
        match &c.outcome {
            Outcome::Same { byte_identical } => {
                t.same += 1;
                if *byte_identical {
                    t.byte_identical += 1;
                }
            }
            Outcome::Different { .. } => t.different += 1,
            Outcome::BothFailed { .. } => t.both_failed += 1,
            Outcome::ReferenceFailed { .. } => t.reference_failed += 1,
            Outcome::CandidateFailed { .. } => t.candidate_failed += 1,
        }
        if let (Some(r), Some(c)) = (c.reference_ms, c.candidate_ms) {
            t.reference_ms += r;
            t.candidate_ms += c;
            t.timed += 1;
        }
    }
    t
}

/// The results as a Markdown report: totals, times, and every case where
/// the engines disagree, grouped by stylesheet.
pub fn markdown(cases: &[Case], reference: &Path, candidate: &Path) -> String {
    let t = totals(cases);
    let mut md = format!(
        "# Differential run\n\nReference: `{}`  \nCandidate: `{}`\n\n\
         | Cases | Same | (byte-identical) | Both failed | Different | Reference failed | Candidate failed |\n\
         |---:|---:|---:|---:|---:|---:|---:|\n\
         | {} | {} | {} | {} | {} | {} | {} |\n\n",
        reference.display(),
        candidate.display(),
        t.cases,
        t.same,
        t.byte_identical,
        t.both_failed,
        t.different,
        t.reference_failed,
        t.candidate_failed
    );
    if t.timed > 0 {
        md.push_str(&format!(
            "Transform time where both produced output ({} cases): reference {:.1} s, candidate {:.1} s.\n\n",
            t.timed,
            t.reference_ms as f64 / 1000.0,
            t.candidate_ms as f64 / 1000.0
        ));
    }
    let mut problems: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for c in cases {
        let what = match &c.outcome {
            Outcome::Same { .. } | Outcome::BothFailed { .. } => continue,
            Outcome::Different { places, first, .. } => {
                format!("different in {places} places: {}", first.join(" | "))
            }
            Outcome::ReferenceFailed { error } => format!("only the reference failed: {error}"),
            Outcome::CandidateFailed { error } => format!("only the candidate failed: {error}"),
        };
        problems
            .entry(c.stylesheet.display().to_string())
            .or_default()
            .push(format!(
                "`{}` ({}): {}",
                c.input.display(),
                params_key(&c.params),
                what.chars().take(400).collect::<String>()
            ));
    }
    if !problems.is_empty() {
        md.push_str("## Disagreements\n\n");
        for (s, lines) in &problems {
            md.push_str(&format!("### `{s}`\n\n"));
            for l in lines {
                md.push_str(&format!("- {l}\n"));
            }
            md.push('\n');
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_round_trip_through_the_results_file() {
        let case = Case {
            stylesheet: "a.xsl".into(),
            input: "in.xml".into(),
            params: BTreeMap::from([("mode".into(), "final".into())]),
            outcome: Outcome::Different {
                places: 2,
                first: vec!["x".into()],
                reference: "r".into(),
                candidate: "c".into(),
            },
            reference_ms: Some(5),
            candidate_ms: None,
        };
        let line = serde_json::to_string(&case).unwrap();
        let back: Case = serde_json::from_str(&line).unwrap();
        assert_eq!(back.outcome, case.outcome);
        assert_eq!(back.params["mode"], "final");
        let t = totals(&[back]);
        assert_eq!((t.cases, t.different, t.timed), (1, 1, 0));
    }

    #[test]
    fn two_engines_are_compared_on_every_input() {
        // The built-in engine against itself: everything agrees, and the
        // agreeing outputs are removed.
        let dir = std::env::temp_dir().join(format!("xylem-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let xsl = dir.join("t.xsl");
        std::fs::write(
            &xsl,
            r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:param name="mode"/>
<xsl:template match="/"><out mode="{$mode}"><xsl:value-of select="count(//*)"/></out></xsl:template>
</xsl:stylesheet>"#,
        )
        .unwrap();
        let bad = dir.join("bad.xsl");
        std::fs::write(&bad, "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\"><xsl:bogus/></xsl:stylesheet>").unwrap();
        let a = dir.join("a.xml");
        let b = dir.join("b.xml");
        std::fs::write(&a, "<r><x/></r>").unwrap();
        std::fs::write(&b, "<r/>").unwrap();
        let host = Host::builtin();
        let work = dir.join("work");
        let sheets = [xsl.clone(), bad];
        let inputs = [a, b];
        let sets = [
            BTreeMap::from([("mode".to_string(), "final".to_string())]),
            BTreeMap::from([("mode".to_string(), "html".to_string())]),
        ];
        let plan = Plan {
            stylesheets: &sheets,
            inputs: &inputs,
            param_sets: &sets,
            reference: &host,
            candidate: &host,
            work: &work,
            threads: 2,
        };
        let cases = run(&plan, &|_| {}, &AtomicBool::new(false)).unwrap();
        assert_eq!(cases.len(), 8);
        let t = totals(&cases);
        assert_eq!((t.same, t.both_failed), (4, 4), "{cases:#?}");
        // Nothing left but the results file.
        let left: Vec<_> = std::fs::read_dir(&work).unwrap().flatten().collect();
        assert_eq!(left.len(), 1, "{left:?}");
        // A second run finds everything done.
        let again = run(
            &plan,
            &|_| panic!("nothing to run"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(again.len(), 8);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
