//! Stylesheet errors as you type: while a stylesheet is open, every
//! top-level stylesheet that uses it (it included) is compiled in the
//! background with Xylem's engine, using the editor's text for stylesheets
//! with unsaved changes, and the errors go to the Issues panel at the
//! element they are on. An error in another module names its file and line,
//! and clicking it opens that file there.

use super::XylemApp;
use crate::doc::{Issue, Severity, View};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use xylem_xslt::stylesheet::path_key;

/// The `source` of these issues in the Issues panel.
pub const STYLESHEET_ISSUES: &str = "Stylesheet";

/// How long typing must pause before the stylesheet is compiled again.
const SETTLE: Duration = Duration::from_millis(700);

/// The errors reported at most, so a broken shared module does not flood
/// the panel.
const MAX_ERRORS: usize = 200;

#[derive(Default)]
pub struct StylesheetIssuesState {
    /// The text last checked for each stylesheet (by path key): a hash.
    checked: HashMap<String, u64>,
    /// A change waiting for typing to pause: path key, text hash, since.
    pending: Option<(String, u64, Instant)>,
    job: Option<Receiver<Finished>>,
    /// What the text was last time it was looked at: path key, document
    /// revision (structured views) and its hash, and when, so an unchanged
    /// document is not written out and hashed again every frame.
    seen: Option<(String, Option<u64>, u64, Instant)>,
    /// Top-level stylesheets using each file, found once per file.
    tops: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
}

/// One compile error, as reported.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub message: String,
}

struct Finished {
    key: String,
    hash: u64,
    found: Vec<Found>,
}

fn is_stylesheet(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xsl") || e.eq_ignore_ascii_case("xslt"))
}

/// A stylesheet as it is in the editor, as bytes in its own encoding (a
/// stylesheet may declare ISO-8859-1): the structured document, or the
/// source view's buffer while editing there.
pub fn editor_bytes(ds: &crate::doc::DocState) -> Vec<u8> {
    match (&ds.view, &ds.source) {
        (View::Source, Some(src)) => match xylem_dom::Document::from_str(&src.text) {
            Ok(mut d) => {
                d.encoding = ds.editor.doc.encoding;
                d.to_bytes()
            }
            // Not well-formed: compiling reports where, encoding aside.
            Err(_) => src.text.clone().into_bytes(),
        },
        _ => ds.editor.doc.to_bytes(),
    }
}

fn hash(bytes: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// The stylesheets to compile for a file, from
/// [`xylem_xslt::navigate::tops_using`] (the file first, then the top-level
/// stylesheets that use it). A module other stylesheets include is checked
/// only through them: on its own it would report the variables and
/// templates it relies on its includers for.
pub fn entry_points(tops: &[PathBuf]) -> &[PathBuf] {
    if tops.len() > 1 { &tops[1..] } else { tops }
}

/// Compiles each of `tops` (reading `unsaved` texts instead of the files
/// they name) and collects the errors, each once.
pub fn compile_errors(tops: &[PathBuf], unsaved: &HashMap<String, Vec<u8>>) -> Vec<Found> {
    let read = |p: &Path| unsaved.get(&path_key(p)).cloned();
    // The stylesheets compile independently: spread them over a few cores,
    // leaving the rest for the editor.
    let workers = std::thread::available_parallelism()
        .map_or(2, |n| (n.get() / 2).max(1))
        .min(tops.len().max(1));
    let chunk = tops.len().div_ceil(workers).max(1);
    let per_chunk: Vec<Vec<Found>> = std::thread::scope(|scope| {
        let handles: Vec<_> = tops
            .chunks(chunk)
            .map(|part| {
                let read = &read;
                scope.spawn(move || {
                    part.iter()
                        .filter_map(|top| {
                            xylem_xslt_engine::Stylesheet::compile_with(top, read)
                                .err()
                                .map(|e| Found {
                                    file: e.file.as_deref().map(PathBuf::from),
                                    line: e.line,
                                    message: e.message.clone(),
                                })
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut found: Vec<Found> = Vec::new();
    for f in per_chunk.into_iter().flatten() {
        if !found.contains(&f) {
            found.push(f);
        }
        if found.len() >= MAX_ERRORS {
            break;
        }
    }
    found
}

impl XylemApp {
    /// Called every frame: starts a check when the stylesheet being edited
    /// has changed and typing has paused, and takes in finished results.
    pub(super) fn stylesheet_issues(&mut self, ctx: &egui::Context) {
        self.take_stylesheet_issues();
        let Some(ds) = self.docs.get(self.active) else {
            return;
        };
        let Some(path) = ds.path.clone().filter(|p| is_stylesheet(p)) else {
            return;
        };
        let key = path_key(&path);
        // The structured views change only with the document's revision. The
        // source view's buffer has no revision, so it is looked at a few
        // times a second rather than every frame.
        let in_source = matches!((&ds.view, &ds.source), (View::Source, Some(_)));
        let revision = (!in_source).then(|| ds.editor.doc.revision());
        if let Some((k, rev, _, at)) = &self.stylesheet_issues.seen
            && *k == key
            && (revision.is_some() && *rev == revision
                || revision.is_none() && at.elapsed() < Duration::from_millis(250))
            && self.stylesheet_issues.pending.is_none()
        {
            return;
        }
        // The text as it is now: the source view's buffer while editing there.
        let text: Vec<u8> = editor_bytes(ds);
        let h = hash(&text);
        let st = &mut self.stylesheet_issues;
        st.seen = Some((key.clone(), revision, h, Instant::now()));
        if in_source {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if st.checked.get(&key) == Some(&h) || st.job.is_some() {
            return;
        }
        match &st.pending {
            Some((k, ph, since)) if *k == key && *ph == h => {
                let waited = since.elapsed();
                if waited < SETTLE {
                    ctx.request_repaint_after(SETTLE - waited);
                    return;
                }
            }
            _ => {
                st.pending = Some((key, h, Instant::now()));
                ctx.request_repaint_after(SETTLE);
                return;
            }
        }
        st.pending = None;
        // Other open stylesheets with unsaved changes are read as edited too.
        let mut unsaved: HashMap<String, Vec<u8>> = self
            .docs
            .iter()
            .filter(|d| d.editor.is_dirty())
            .filter_map(|d| {
                let p = d.path.as_ref().filter(|p| is_stylesheet(p))?;
                Some((path_key(p), editor_bytes(d)))
            })
            .collect();
        unsaved.insert(key.clone(), text);
        let st = &mut self.stylesheet_issues;
        let cache = st.tops.clone();
        let (tx, rx) = channel();
        st.job = Some(rx);
        std::thread::spawn(move || {
            let cached = cache.lock().ok().and_then(|c| c.get(&key).cloned());
            let tops = cached.unwrap_or_else(|| {
                let t = xylem_xslt::navigate::tops_using(&path);
                if let Ok(mut c) = cache.lock() {
                    c.insert(key.clone(), t.clone());
                }
                t
            });
            let found = compile_errors(entry_points(&tops), &unsaved);
            let _ = tx.send(Finished {
                key,
                hash: h,
                found,
            });
        });
    }

    fn take_stylesheet_issues(&mut self) {
        let st = &mut self.stylesheet_issues;
        let Some(rx) = &st.job else { return };
        let done = match rx.try_recv() {
            Ok(done) => done,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                st.job = None;
                return;
            }
        };
        st.job = None;
        st.checked.insert(done.key.clone(), done.hash);
        let Some(ds) = self
            .docs
            .iter_mut()
            .find(|d| d.path.as_deref().is_some_and(|p| path_key(p) == done.key))
        else {
            return;
        };
        let here = ds.path.clone();
        // Lines of this file are found in the structured view's text, which
        // is what was compiled unless the source view holds other edits.
        let (xml, offsets) = ds.editor.doc.to_xml_with_offsets();
        let mapped = ds.view != View::Source && hash(&ds.editor.doc.to_bytes()) == done.hash;
        let mut line_starts = vec![0usize];
        line_starts.extend(xml.match_indices('\n').map(|(i, _)| i + 1));
        let doc = &ds.editor.doc;
        let node_at_line = |line: u32| {
            let end = line_starts.get(line as usize).copied().unwrap_or(xml.len());
            doc.descendants(doc.root())
                .filter(|&n| doc.is_element(n))
                .filter(|n| offsets.get(n.index()).is_some_and(|&o| o < end))
                .last()
        };
        let issues: Vec<Issue> = done
            .found
            .iter()
            .map(|f| {
                let file = f.file.clone().or_else(|| here.clone());
                let in_this_file = file
                    .as_ref()
                    .zip(here.as_ref())
                    .is_some_and(|(a, b)| path_key(a) == path_key(b));
                let node = match (in_this_file && mapped, f.line) {
                    (true, Some(l)) => node_at_line(l),
                    _ => None,
                };
                Issue {
                    node,
                    severity: Severity::Error,
                    message: f.message.clone(),
                    source: STYLESHEET_ISSUES,
                    at: file.zip(f.line),
                }
            })
            .collect();
        ds.issues.retain(|i| i.source != STYLESHEET_ISSUES);
        ds.issues.extend(issues);
        ds.canvas.flagged = ds.issues.iter().filter_map(|i| i.node).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_come_from_the_unsaved_text_of_every_stylesheet_using_the_file() {
        let dir = std::env::temp_dir().join(format!("xylem-xsl-issues-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let top = dir.join("top.xsl");
        let module = dir.join("module.xsl");
        std::fs::write(
            &top,
            "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n<xsl:include href=\"module.xsl\"/>\n<xsl:template match=\"/\"><r/></xsl:template>\n</xsl:stylesheet>\n",
        )
        .unwrap();
        let good = "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n<xsl:template name=\"t\"><x/></xsl:template>\n</xsl:stylesheet>\n";
        std::fs::write(&module, good).unwrap();
        let tops = vec![module.clone(), top.clone()];
        assert!(compile_errors(&tops, &HashMap::new()).is_empty());
        // The editor's unsaved text has an error on line 2; the file on disk
        // does not.
        let bad = good.replace("<x/>", "<xsl:value-of select=\"1 +\"/>");
        let unsaved = HashMap::from([(path_key(&module), bad.into_bytes())]);
        let found = compile_errors(&tops, &unsaved);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].line, Some(2));
        assert_eq!(
            found[0].file.as_deref().map(path_key),
            Some(path_key(&module))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod corpus {
    use super::*;

    /// Times the check on a real shared module:
    /// `XYLEM_XSL_ISSUES_FILE=<module.xsl> cargo test -p xylem-app --release
    /// stylesheet_issues::corpus -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs XYLEM_XSL_ISSUES_FILE"]
    fn a_shared_module_is_checked_through_its_includers() {
        let Some(file) = std::env::var_os("XYLEM_XSL_ISSUES_FILE").map(PathBuf::from) else {
            return;
        };
        let started = Instant::now();
        let tops = xylem_xslt::navigate::tops_using(&file);
        let found_tops = started.elapsed();
        let entries = entry_points(&tops);
        let found = compile_errors(entries, &HashMap::new());
        println!(
            "{} stylesheets use it (found in {found_tops:?}); compiled {} in {:?}; {} errors",
            tops.len() - 1,
            entries.len(),
            started.elapsed() - found_tops,
            found.len()
        );
        for f in found.iter().take(10) {
            println!("  {:?}:{:?} {}", f.file, f.line, f.message);
        }
    }
}

#[cfg(test)]
mod editor_text_corpus {
    use super::*;

    /// Every top-level stylesheet under `XYLEM_XSL_CORPUS`, compiled once from
    /// disk and once with every module read as the editor writes it (parsed
    /// into a document and written back in its own encoding), must give the
    /// same result: what the editor checks as you type is what is on disk.
    #[test]
    #[ignore = "needs XYLEM_XSL_CORPUS"]
    fn the_editors_text_compiles_like_the_file() {
        let Some(root) = std::env::var_os("XYLEM_XSL_CORPUS").map(PathBuf::from) else {
            return;
        };
        let tops = xylem_xslt::proof::top_level(&root);
        let as_editor = |p: &Path| {
            let bytes = std::fs::read(p).ok()?;
            xylem_dom::Document::from_bytes(&bytes)
                .ok()
                .map(|d| d.to_bytes())
        };
        let mut differ = Vec::new();
        for top in &tops {
            let disk = xylem_xslt_engine::Stylesheet::compile_with(top, &|_| None)
                .err()
                .map(|e| e.message);
            let editor = xylem_xslt_engine::Stylesheet::compile_with(top, &as_editor)
                .err()
                .map(|e| e.message);
            if disk != editor {
                differ.push(format!(
                    "{}: disk {disk:?}, editor {editor:?}",
                    top.display()
                ));
            }
        }
        println!(
            "{} top-level stylesheets compiled from disk and from the editor's text",
            tops.len()
        );
        for d in &differ {
            println!("  DIFFERENT {d}");
        }
        assert!(differ.is_empty());
    }
}
