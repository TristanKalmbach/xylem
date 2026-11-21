//! Discovering doctype packages and picking one for a document.

use std::fs;
use std::path::{Path, PathBuf};

use glob::{MatchOptions, Pattern};
use xylem_dom::{Document, local_name};

use crate::error::Error;
use crate::package::{DoctypePackage, MANIFEST_NAME};

/// How deep below a scanned directory packages are searched for.
const MAX_SCAN_DEPTH: usize = 4;

/// A set of known packages.
#[derive(Debug, Default)]
pub struct DoctypeRegistry {
    packages: Vec<DoctypePackage>,
    errors: Vec<(PathBuf, Error)>,
}

impl DoctypeRegistry {
    /// Loads every package found under `dirs`. A directory that itself holds a
    /// `doctype.toml` is a package; otherwise its sub-directories are searched
    /// (to a bounded depth, in name order). Packages that fail to load are
    /// recorded in [`errors`](Self::errors) rather than aborting the scan.
    pub fn scan(dirs: &[PathBuf]) -> Self {
        let mut registry = Self::default();
        for dir in dirs {
            registry.scan_dir(dir, 0);
        }
        registry
    }

    fn scan_dir(&mut self, dir: &Path, depth: usize) {
        if dir.join(MANIFEST_NAME).is_file() {
            match DoctypePackage::load(dir) {
                Ok(pkg) => self.packages.push(pkg),
                Err(e) => self.errors.push((dir.to_path_buf(), e)),
            }
            return;
        }
        if depth >= MAX_SCAN_DEPTH {
            return;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut subdirs: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        subdirs.sort();
        for sub in subdirs {
            self.scan_dir(&sub, depth + 1);
        }
    }

    pub fn packages(&self) -> &[DoctypePackage] {
        &self.packages
    }

    /// Packages that were found but could not be loaded.
    pub fn errors(&self) -> &[(PathBuf, Error)] {
        &self.errors
    }

    pub fn get(&self, name: &str) -> Option<&DoctypePackage> {
        self.packages.iter().find(|p| p.doctype.name == name)
    }

    /// Picks the best package for `doc` (opened from `path`, if known).
    ///
    /// A package qualifies when it matches the document by schema location,
    /// DOCTYPE public id, file name or root element; a package that declares
    /// roots and does not match the root only qualifies through one of the
    /// stronger criteria. Qualifying packages are ranked by `priority`, then
    /// by the strongest criterion hit (schema location > public id > file
    /// name > root), then by scan order.
    pub fn match_document(&self, doc: &Document, path: Option<&Path>) -> Option<&DoctypePackage> {
        let facts = DocFacts::of(doc, path);
        let mut best: Option<(Score, &DoctypePackage)> = None;
        for pkg in &self.packages {
            let Some(score) = Score::of(pkg, &facts) else {
                continue;
            };
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, pkg));
            }
        }
        best.map(|(_, pkg)| pkg)
    }
}

/// What a document offers for matching.
struct DocFacts {
    root: Option<String>,
    schema_files: Vec<String>,
    public_id: Option<String>,
    file_name: Option<String>,
}

impl DocFacts {
    fn of(doc: &Document, path: Option<&Path>) -> Self {
        let root_el = doc.document_element();
        let mut schema_files = Vec::new();
        if let Some(el) = root_el.and_then(|r| doc.element(r)) {
            for attr in &el.attrs {
                if matches!(
                    local_name(&attr.name),
                    "noNamespaceSchemaLocation" | "schemaLocation"
                ) {
                    schema_files.extend(attr.value.split_whitespace().map(basename));
                }
            }
        }
        let dt = doc.doctype();
        if let Some(system) = dt.and_then(|d| d.system_id.as_deref()) {
            schema_files.push(basename(system));
        }
        Self {
            root: root_el.and_then(|r| doc.name(r)).map(str::to_string),
            schema_files,
            public_id: dt.and_then(|d| d.public_id.clone()),
            file_name: path
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
        }
    }
}

fn basename(location: &str) -> String {
    location
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(location)
        .to_string()
}

/// True when any candidate matches any of the (case-insensitive) globs.
fn glob_hit(globs: &[String], candidates: &[String], options: MatchOptions) -> bool {
    let patterns: Vec<Pattern> = globs.iter().filter_map(|g| Pattern::new(g).ok()).collect();
    candidates
        .iter()
        .any(|c| patterns.iter().any(|p| p.matches_with(c, options)))
}

/// Ordered so that a greater score is a better match.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Score {
    priority: i32,
    schema_location: bool,
    public_id: bool,
    file_name: bool,
    root: bool,
}

impl Score {
    fn of(pkg: &DoctypePackage, facts: &DocFacts) -> Option<Self> {
        let rules = &pkg.matching;
        let options = MatchOptions {
            case_sensitive: false,
            ..MatchOptions::default()
        };
        let schema_location = glob_hit(&rules.schema_locations, &facts.schema_files, options);
        let public_id = facts.public_id.as_deref().is_some_and(|id| {
            rules
                .public_ids
                .iter()
                .any(|needle| id.contains(needle.as_str()))
        });
        let file_name = glob_hit(&rules.file_names, facts.file_name.as_slice(), options);
        let root = facts.root.as_deref().is_some_and(|r| {
            rules
                .roots
                .iter()
                .any(|want| want == r || (!want.contains(':') && want == local_name(r)))
        });

        let strong = schema_location || public_id || file_name;
        let qualifies = if rules.roots.is_empty() {
            strong
        } else {
            strong || root
        };
        qualifies.then_some(Self {
            priority: rules.priority,
            schema_location,
            public_id,
            file_name,
            root,
        })
    }
}
