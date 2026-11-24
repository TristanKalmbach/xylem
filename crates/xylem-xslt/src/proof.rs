//! Proving that changed stylesheet files still do what they did: every test
//! suite whose stylesheet uses a changed file is checked against a baseline,
//! and every top-level stylesheet that includes or imports one is compiled.
//! A shared module (a base stylesheet imported by many) reaches stylesheets
//! no suite covers; compiling them at least shows they still load.

use crate::check::{Baseline, CheckEnv, CheckReport, check};
use crate::host::Compiled;
use crate::stylesheet::{Stylesheet, path_key};
use crate::suite::Suite;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

#[derive(Debug, Clone)]
pub struct SuiteProof {
    pub suite: PathBuf,
    pub report: CheckReport,
}

#[derive(Debug, Clone, Default)]
pub struct Proof {
    /// Suites whose stylesheet uses a changed file, checked.
    pub suites: Vec<SuiteProof>,
    /// Suites that do not use any changed file.
    pub unaffected: Vec<PathBuf>,
    /// Top-level stylesheets using a changed file, compiled.
    pub compiled: Vec<Compiled>,
    pub errors: Vec<String>,
    pub cancelled: bool,
}

impl Proof {
    /// Every suite the same and every stylesheet compiled.
    pub fn passed(&self) -> bool {
        !self.cancelled
            && self.errors.is_empty()
            && self.suites.iter().all(|s| s.report.all_identical())
            && self.compiled.iter().all(|c| c.ok)
    }
}

fn xsl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == ".git" || n == ".claude") {
                continue;
            }
            xsl_files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("xsl") || x.eq_ignore_ascii_case("xslt"))
        {
            out.push(p);
        }
    }
}

/// The `href`s of a file's `xsl:include` and `xsl:import` elements, found
/// by scanning the text (much quicker than reading every file fully).
fn include_hrefs(file: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let mut rest: &str = &text;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let name_end = rest
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        let local = name.rsplit(':').next().unwrap_or(name);
        if (local == "include" || local == "import") && name.contains(':') {
            let tag_end = rest.find('>').unwrap_or(rest.len());
            let tag = &rest[..tag_end];
            if let Some(at) = tag.find("href") {
                let after = tag[at + 4..]
                    .trim_start()
                    .trim_start_matches('=')
                    .trim_start();
                if let Some(q) = after.chars().next().filter(|c| matches!(c, '"' | '\''))
                    && let Some(end) = after[1..].find(q)
                {
                    out.push(after[1..1 + end].to_string());
                }
            }
        }
    }
    out
}

/// Top-level stylesheets under `scope` (ones no other file includes or
/// imports) that use any of `changed`, directly or through other modules.
pub fn dependents(scope: &Path, changed: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    xsl_files(scope, &mut files);
    // Each file's includes and imports, read once.
    let mut uses: HashMap<String, Vec<String>> = HashMap::new();
    let mut referenced = HashSet::new();
    let mut by_key = HashMap::new();
    for f in &files {
        let key = path_key(f);
        by_key.insert(key.clone(), f.clone());
        let dir = f.parent().unwrap_or(Path::new("."));
        let mut out = Vec::new();
        for h in include_hrefs(f) {
            let k = path_key(&dir.join(h.replace("%20", " ")));
            referenced.insert(k.clone());
            out.push(k);
        }
        uses.insert(key, out);
    }
    let changed: HashSet<String> = changed.iter().map(|p| path_key(p)).collect();
    let reaches = |start: &String| {
        let mut seen = HashSet::new();
        let mut stack = vec![start.clone()];
        while let Some(k) = stack.pop() {
            if changed.contains(&k) {
                return true;
            }
            if seen.insert(k.clone())
                && let Some(next) = uses.get(&k)
            {
                stack.extend(next.iter().cloned());
            }
        }
        false
    };
    let mut tops: Vec<PathBuf> = uses
        .keys()
        .filter(|k| !referenced.contains(*k) && reaches(k))
        .filter_map(|k| by_key.get(k).cloned())
        .collect();
    tops.sort();
    tops
}

/// Checks every suite in `suites` that uses a changed file against
/// `baseline`, then compiles `dependents` of the changed files in `scope`.
pub fn prove(
    changed: &[PathBuf],
    suites: &[PathBuf],
    baseline: &Baseline,
    scope: Option<&Path>,
    env: CheckEnv,
) -> Proof {
    let mut proof = Proof::default();
    let changed_keys: HashSet<String> = changed.iter().map(|p| path_key(p)).collect();
    for (i, path) in suites.iter().enumerate() {
        if env.cancel.load(Ordering::Relaxed) {
            proof.cancelled = true;
            return proof;
        }
        let suite = match Suite::load(path) {
            Ok(s) => s,
            Err(e) => {
                proof.errors.push(e);
                continue;
            }
        };
        let Some(stylesheet) = suite.stylesheet.clone() else {
            proof.unaffected.push(path.clone());
            continue;
        };
        let uses_changed = Stylesheet::load(&stylesheet)
            .map(|s| !s.module_keys().is_disjoint(&changed_keys))
            .unwrap_or(true);
        if !uses_changed {
            proof.unaffected.push(path.clone());
            continue;
        }
        (env.progress)(format!(
            "Checking {} ({} of {})…",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            i + 1,
            suites.len()
        ));
        let work = env.work.join(format!("suite-{}", i + 1));
        let report = check(
            &suite,
            &stylesheet,
            baseline,
            CheckEnv {
                host: env.host,
                work: &work,
                progress: &mut |_| {},
                cancel: env.cancel,
            },
        );
        if report.cancelled {
            proof.cancelled = true;
            return proof;
        }
        proof.suites.push(SuiteProof {
            suite: path.clone(),
            report,
        });
    }
    if let Some(scope) = scope {
        (env.progress)("Finding stylesheets that use the changed files…".into());
        let tops = dependents(scope, changed);
        (env.progress)(format!("Compiling {} stylesheets…", tops.len()));
        proof.compiled = env.host.compile(&tops);
    }
    proof
}

/// The suite files in a folder (`*.toml`), sorted.
pub fn suites_in(folder: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.extension()
                        .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}
