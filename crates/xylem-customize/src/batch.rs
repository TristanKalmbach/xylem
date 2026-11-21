//! Bulk import: find every customization under a folder and import each.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::import::{ImportReport, import_xmax};
use crate::package::DoctypePackage;

/// One customization found by [`discover`], with the folder name it will get.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportJob {
    /// A folder containing a `.ctm`, or a `.xac` file.
    pub source: PathBuf,
    /// Output folder name below the output root.
    pub dir_name: String,
}

/// The result of importing one job and loading the package back.
#[derive(Debug)]
pub struct BatchEntry {
    pub job: ImportJob,
    pub out_dir: PathBuf,
    pub result: Result<ImportReport>,
}

/// Finds every folder containing a `.ctm` file and every `.xac` archive under
/// `root`, in path order, and assigns output names. Customizations whose
/// names collide all get a suffix derived from where they live.
pub fn discover(root: &Path) -> Result<Vec<ImportJob>> {
    let mut sources = Vec::new();
    walk(root, &mut sources)?;
    sources.sort();

    let stems: Vec<String> = sources.iter().map(|s| name_stem(s)).collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for stem in &stems {
        *counts.entry(stem.to_ascii_lowercase()).or_default() += 1;
    }
    let mut taken = HashSet::new();
    let mut jobs = Vec::new();
    for (source, stem) in sources.into_iter().zip(stems) {
        let mut name = if counts[&stem.to_ascii_lowercase()] > 1 {
            format!("{stem}--{}", qualifier(root, &source))
        } else {
            stem
        };
        let base = name.clone();
        let mut n = 2;
        while !taken.insert(name.to_ascii_lowercase()) {
            name = format!("{base}-{n}");
            n += 1;
        }
        jobs.push(ImportJob {
            source,
            dir_name: name,
        });
    }
    Ok(jobs)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    let mut has_ctm = false;
    let mut subdirs = Vec::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let lower = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if kind.is_dir() {
            subdirs.push(path);
        } else if kind.is_file() {
            if lower.ends_with(".ctm") {
                has_ctm = true;
            } else if lower.ends_with(".xac") {
                out.push(path);
            }
        }
    }
    if has_ctm {
        out.push(dir.to_path_buf());
    }
    for sub in subdirs {
        // Unreadable sub-folders must not abort a whole-corpus scan.
        let _ = walk(&sub, out);
    }
    Ok(())
}

/// Name a source would have on its own: the `.xac` stem or the folder's `.ctm` stem.
fn name_stem(source: &Path) -> String {
    if source.is_dir() {
        let mut ctms: Vec<String> = fs::read_dir(source)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.to_ascii_lowercase().ends_with(".ctm"))
            .collect();
        ctms.sort();
        if let Some(first) = ctms.first() {
            return sanitise(&first[..first.len() - ".ctm".len()]);
        }
    }
    sanitise(
        &source
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

/// The location of `source` relative to `root`, flattened into one path component.
fn qualifier(root: &Path, source: &Path) -> String {
    let parent = source.parent().unwrap_or(source);
    let rel = parent.strip_prefix(root).unwrap_or(parent);
    let parts: Vec<String> = rel
        .components()
        .map(|c| sanitise(&c.as_os_str().to_string_lossy()))
        .collect();
    if parts.is_empty() {
        "root".to_string()
    } else {
        parts.join("_")
    }
}

fn sanitise(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Imports every customization under `root` into `out_root/<name>` and loads
/// each result back with [`DoctypePackage::load`]; a package that does not
/// load, or whose `[files]` are missing on disk, counts as a failure.
pub fn import_all(root: &Path, out_root: &Path) -> Result<Vec<BatchEntry>> {
    fs::create_dir_all(out_root).map_err(|e| Error::io(out_root, e))?;
    Ok(discover(root)?
        .into_iter()
        .map(|job| {
            let out_dir = out_root.join(&job.dir_name);
            let result = import_xmax(&job.source, &out_dir).and_then(|mut report| {
                verify(&out_dir)?;
                report.name.clone_from(&job.dir_name);
                Ok(report)
            });
            BatchEntry {
                job,
                out_dir,
                result,
            }
        })
        .collect())
}

/// Loads an imported package back and checks that its files exist.
pub fn verify(dir: &Path) -> Result<()> {
    let package = DoctypePackage::load(dir)?;
    let missing = package.missing_files();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{}: missing files {}",
            dir.display(),
            missing.join(", ")
        )))
    }
}
