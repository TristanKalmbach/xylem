//! Tools > Stylesheet references (F12): what the element at the caret in a
//! stylesheet refers to or defines (templates, functions, keys, modes,
//! variables), with every definition and usage across the stylesheets that
//! use the file. Click one to go there.

use super::XylemApp;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use xylem_xslt::navigate::{Index, Place, Reference, tops_using};
use xylem_xslt::stylesheet::path_key;

/// Usages listed before "and N more".
const MAX_LISTED: usize = 200;

#[derive(Default)]
pub struct ReferencesState {
    pub open: bool,
    job: Option<Receiver<Result<Found, String>>>,
    found: Option<Found>,
    error: Option<String>,
    /// For each file, the stylesheets that use it (finding them means
    /// reading every stylesheet in the folder, so it is done once).
    tops: Arc<Mutex<HashMap<String, Vec<PathBuf>>>>,
    /// The last index built, reused while its files are unchanged.
    index: Arc<Mutex<Option<CachedIndex>>>,
    /// References whose usages are listed.
    expanded: HashSet<usize>,
}

/// An index and what it was built from.
struct CachedIndex {
    tops: Vec<PathBuf>,
    unsaved: HashMap<String, Vec<u8>>,
    /// Every module's size and modification time when it was read.
    files: Vec<(PathBuf, Option<(u64, std::time::SystemTime)>)>,
    index: Arc<Index>,
}

fn file_stamp(p: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

impl CachedIndex {
    /// Still right for these stylesheets and unsaved texts.
    fn fits(&self, tops: &[PathBuf], unsaved: &HashMap<String, Vec<u8>>) -> bool {
        self.tops == tops
            && self.unsaved == *unsaved
            && self.files.iter().all(|(p, stamp)| file_stamp(p) == *stamp)
    }
}

struct Found {
    /// The element at the caret, as `<xsl:call-template>`.
    element: String,
    refs: Vec<Reference>,
    stylesheets: usize,
}

fn is_stylesheet(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xsl") || e.eq_ignore_ascii_case("xslt"))
}

/// `folder/file.xsl:12`, short enough for a list.
fn short_place(p: &Place) -> String {
    let mut parts: Vec<String> = p
        .file
        .components()
        .rev()
        .take(2)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    parts.reverse();
    format!("{}:{}", parts.join("/"), p.line)
}

impl XylemApp {
    pub(super) fn show_stylesheet_references(&mut self) {
        let Some(ds) = self.docs.get(self.active) else {
            return;
        };
        let Some(path) = ds.path.clone().filter(|p| is_stylesheet(p)) else {
            return self.toast(
                "Stylesheet references works in a stylesheet (.xsl or .xslt file).",
                false,
            );
        };
        let doc = &ds.editor.doc;
        let caret = ds.editor.caret().node;
        let (xml, offsets) = doc.to_xml_with_offsets();
        // The element at the caret and those around it, innermost first: the
        // first that defines or uses a name is the one shown, so the caret
        // can be anywhere inside a template.
        let candidates: Vec<(usize, String)> = std::iter::once(caret)
            .chain(doc.ancestors(caret))
            .filter(|&n| doc.is_element(n))
            .filter_map(|n| {
                let at = *offsets.get(n.index())?;
                Some((at, format!("<{}>", doc.name(n).unwrap_or("?"))))
            })
            .collect();
        let Some(element) = candidates.first().map(|c| c.1.clone()) else {
            return;
        };
        // Unsaved stylesheets are read as they are in the editor.
        let mut unsaved: HashMap<String, Vec<u8>> = self
            .docs
            .iter()
            .filter(|d| d.editor.is_dirty())
            .filter_map(|d| {
                let p = d.path.as_ref().filter(|p| is_stylesheet(p))?;
                Some((path_key(p), super::stylesheet_issues::editor_bytes(d)))
            })
            .collect();
        // In the document's own encoding; the index reads it back to the
        // same text the offsets above were taken from.
        drop(xml);
        unsaved.insert(path_key(&path), doc.to_bytes());

        let st = &mut self.references;
        st.open = true;
        st.error = None;
        st.found = None;
        st.expanded.clear();
        let (tx, rx) = channel();
        st.job = Some(rx);
        let cache = st.tops.clone();
        let index_cache = st.index.clone();
        std::thread::spawn(move || {
            let key = path_key(&path);
            let cached = cache.lock().ok().and_then(|c| c.get(&key).cloned());
            let tops = cached.unwrap_or_else(|| {
                let t = tops_using(&path);
                if let Ok(mut c) = cache.lock() {
                    c.insert(key, t.clone());
                }
                t
            });
            let reuse = index_cache.lock().ok().and_then(|c| {
                c.as_ref()
                    .filter(|c| c.fits(&tops, &unsaved))
                    .map(|c| c.index.clone())
            });
            let built = match reuse {
                Some(index) => Ok(index),
                None => {
                    let read = |p: &Path| unsaved.get(&path_key(p)).cloned();
                    Index::build(&tops, &read).map(|index| {
                        let index = Arc::new(index);
                        let files = index
                            .stylesheet
                            .modules
                            .iter()
                            .map(|m| (m.path.clone(), file_stamp(&m.path)))
                            .collect();
                        if let Ok(mut c) = index_cache.lock() {
                            *c = Some(CachedIndex {
                                tops: tops.clone(),
                                unsaved: unsaved.clone(),
                                files,
                                index: index.clone(),
                            });
                        }
                        index
                    })
                }
            };
            let result = built.map(|index| {
                let hit = candidates
                    .iter()
                    .map(|(at, name)| (name, index.references_at(&path, *at)))
                    .find(|(_, refs)| !refs.is_empty());
                match hit {
                    Some((name, refs)) => Found {
                        element: name.clone(),
                        refs,
                        stylesheets: tops.len(),
                    },
                    None => Found {
                        element,
                        refs: Vec::new(),
                        stylesheets: tops.len(),
                    },
                }
            });
            let _ = tx.send(result);
        });
    }

    pub(super) fn stylesheet_references_window(&mut self, ctx: &egui::Context) {
        let st = &mut self.references;
        if let Some(rx) = &st.job {
            match rx.try_recv() {
                Ok(Ok(found)) => {
                    // A single reference shows its usages straight away.
                    if found.refs.len() == 1 {
                        st.expanded.insert(0);
                    }
                    st.found = Some(found);
                    st.job = None;
                }
                Ok(Err(e)) => {
                    st.error = Some(e);
                    st.job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    st.error = Some("Finding the references stopped unexpectedly.".into());
                    st.job = None;
                }
            }
        }
        if !st.open {
            return;
        }
        let mut go: Option<(PathBuf, u32)> = None;
        let (close, _) = crate::ui::Dialog::new("Stylesheet references")
            .width(640.0)
            .modeless()
            .grow()
            .show(ctx, |ui| {
                if st.job.is_some() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        crate::ui::muted(ui, "Reading the stylesheets that use this file…");
                    });
                    return;
                }
                if let Some(e) = &st.error {
                    crate::ui::notice(ui, crate::ui::Tone::Danger, e.clone());
                    return;
                }
                let Some(found) = &st.found else { return };
                if found.refs.is_empty() {
                    crate::ui::muted(
                        ui,
                        format!(
                            "{} does not define or use a named template, function, key, mode or variable. Put the caret in an element that does, and press F12.",
                            found.element
                        ),
                    );
                    return;
                }
                crate::ui::muted(
                    ui,
                    format!(
                        "In {}, across {} stylesheet{} that use this file:",
                        found.element,
                        found.stylesheets,
                        if found.stylesheets == 1 { "" } else { "s" }
                    ),
                );
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .max_height(480.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, r) in found.refs.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.strong(format!("{} {}", r.kind.label(), r.name));
                            });
                            if r.definitions.is_empty() {
                                crate::ui::muted(ui, "    Not defined in these stylesheets.");
                            }
                            for d in &r.definitions {
                                let text = match &d.within {
                                    Some(w) if r.kind == xylem_xslt::navigate::Kind::Variable => {
                                        format!("Defined at {}   in {w}", short_place(d))
                                    }
                                    _ => format!("Defined at {}", short_place(d)),
                                };
                                if ui
                                    .link(text)
                                    .on_hover_text(format!("{}\n{}", d.file.display(), d.excerpt))
                                    .clicked()
                                {
                                    go = Some((d.file.clone(), d.line));
                                }
                            }
                            let n = r.usages.len();
                            let open = st.expanded.contains(&i);
                            let label = match (n, open) {
                                (0, _) => "No usages".to_string(),
                                (n, true) => format!("▾ {n} usage{}", if n == 1 { "" } else { "s" }),
                                (n, false) => format!("▸ {n} usage{}", if n == 1 { "" } else { "s" }),
                            };
                            if n == 0 {
                                crate::ui::muted(ui, label);
                            } else if crate::ui::link_button(ui, &label).clicked() {
                                if open {
                                    st.expanded.remove(&i);
                                } else {
                                    st.expanded.insert(i);
                                }
                            }
                            if open {
                                ui.indent(("usages", i), |ui| {
                                    for u in r.usages.iter().take(MAX_LISTED) {
                                        let text = match &u.within {
                                            Some(w) => format!("{}   {w}", short_place(u)),
                                            None => short_place(u),
                                        };
                                        if ui
                                            .link(text)
                                            .on_hover_text(format!("{}\n{}", u.file.display(), u.excerpt))
                                            .clicked()
                                        {
                                            go = Some((u.file.clone(), u.line));
                                        }
                                    }
                                    if n > MAX_LISTED {
                                        crate::ui::muted(ui, format!("and {} more", n - MAX_LISTED));
                                    }
                                });
                            }
                            ui.add_space(8.0);
                        }
                    });
            });
        if close {
            self.references.open = false;
        }
        if let Some((file, line)) = go {
            self.open_stylesheet_at(&file, line);
        }
    }
}
