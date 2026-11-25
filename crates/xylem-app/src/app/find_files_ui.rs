//! The Find in files window: collects the search, shows results as they
//! stream in from the worker, opens a result in the editor and runs Replace
//! all. The searching itself lives in `crate::find_files`.

use super::XylemApp;
use crate::doc::{SourceState, View};
use crate::find::FindAction;
use crate::find_files::{
    FileHits, Place, ReplaceReport, SearchEvent, SearchOptions, SearchSpec, path_key,
    spawn_replace, spawn_search,
};
use egui::{Color32, RichText, TextFormat, text::LayoutJob};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use xylem_dom::Document;
use xylem_edit::{Pos, Selection};

#[derive(Default)]
pub struct FindFilesState {
    pub open: bool,
    focus: bool,
    folder: String,
    pattern: String,
    options: SearchOptions,
    replacement: String,
    /// Search subfolders too (on unless turned off).
    top_level_only: bool,
    search: Option<(Receiver<SearchEvent>, Arc<AtomicBool>)>,
    /// The search the results belong to. Replace all uses this, never the
    /// fields as they are now, so editing the fields after a search cannot
    /// change what gets replaced.
    searched: Option<SearchSpec>,
    results: Vec<FileHits>,
    unreadable: Vec<(PathBuf, String)>,
    scanned: usize,
    finished: Option<bool>,
    error: Option<String>,
    /// Waiting for the user to confirm Replace all.
    confirm: bool,
    replacing: Option<Receiver<Result<ReplaceReport, String>>>,
    report: Option<String>,
}

enum Action {
    Search,
    Stop,
    Browse,
    AskReplace,
    Replace,
    Open(PathBuf, Place),
}

impl FindFilesState {
    fn spec(&self) -> SearchSpec {
        SearchSpec {
            root: PathBuf::from(self.folder.trim()),
            pattern: self.pattern.clone(),
            subfolders: !self.top_level_only,
            options: self.options.clone(),
        }
    }

    fn match_count(&self) -> usize {
        self.results.iter().map(|f| f.hits.len()).sum()
    }

    /// Results are current: they came from a finished search with exactly
    /// the settings now in the window.
    fn results_current(&self) -> bool {
        self.finished == Some(false) && self.searched.as_ref() == Some(&self.spec())
    }
}

impl XylemApp {
    pub(super) fn open_find_in_files(&mut self) {
        let st = &mut self.find_files;
        st.open = true;
        st.focus = true;
        if st.folder.is_empty() {
            let folder = self
                .settings
                .find_files_folder
                .clone()
                .or_else(|| {
                    self.docs
                        .get(self.active)
                        .and_then(|d| d.dir().map(Path::to_path_buf))
                })
                .or_else(|| std::env::current_dir().ok());
            st.folder = folder.map(|p| p.display().to_string()).unwrap_or_default();
        }
        if st.pattern.is_empty() {
            st.pattern = self.settings.find_files_pattern.clone();
        }
        // Start from what the single-document find was looking for.
        if st.options.text.is_empty() && self.find.has_query() {
            st.options.text = self.find.query.clone();
        }
    }

    pub(super) fn find_files_window(&mut self, ctx: &egui::Context) {
        self.poll_find_files();
        if !self.find_files.open {
            return;
        }
        let mut actions = Vec::new();
        let (close, _) = crate::ui::Dialog::new("Find in files")
            .width(660.0)
            .modeless()
            .show(ctx, |ui| self.find_files_ui(ui, &mut actions));
        if close {
            self.find_files.open = false;
        }
        if self.find_files.confirm {
            self.confirm_replace_ui(ctx, &mut actions);
        }
        for a in actions {
            match a {
                Action::Search => self.start_find_files(),
                Action::Stop => {
                    if let Some((_, cancel)) = &self.find_files.search {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                Action::Browse => {
                    let start = PathBuf::from(self.find_files.folder.trim());
                    let picked = crate::script::file_dialog(|| {
                        let mut d = rfd::FileDialog::new();
                        if start.is_dir() {
                            d = d.set_directory(&start);
                        }
                        d.pick_folder()
                    });
                    if let Some(p) = picked {
                        self.find_files.folder = p.display().to_string();
                    }
                }
                Action::AskReplace => self.find_files.confirm = true,
                Action::Replace => self.start_replace_in_files(),
                Action::Open(path, place) => self.open_find_result(&path, &place),
            }
        }
    }

    fn find_files_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let st = &mut self.find_files;
        let busy = st.search.is_some() || st.replacing.is_some();
        let p = crate::ui::palette(ui.ctx());
        let field_w = 500.0;
        crate::ui::form(ui, "find-files-form", |f| {
            f.row("Find", |ui| {
                let r = crate::ui::text_field(
                    ui,
                    "Find in files text",
                    &mut st.options.text,
                    "Text to find",
                    field_w,
                );
                if std::mem::take(&mut st.focus) {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !busy {
                    actions.push(Action::Search);
                }
            });
            f.row("Replace with", |ui| {
                crate::ui::text_field(ui, "Replace with", &mut st.replacement, "", field_w);
            });
            f.row("Folder", |ui| {
                ui.horizontal(|ui| {
                    crate::ui::text_field(
                        ui,
                        "Folder",
                        &mut st.folder,
                        "Folder to search",
                        field_w - 90.0,
                    );
                    if crate::ui::button(ui, "Browse…").clicked() {
                        actions.push(Action::Browse);
                    }
                });
            });
            f.row("Files", |ui| {
                crate::ui::text_field(ui, "File pattern", &mut st.pattern, "*.xml", field_w);
            });
            f.row("", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(&mut st.options.match_case, "Match case");
                    ui.checkbox(&mut st.options.whole_word, "Whole word");
                    ui.checkbox(&mut st.options.regex, "Regex");
                    ui.checkbox(&mut st.options.attributes, "Attribute values")
                        .on_hover_text(
                            "Also search and replace in attribute values, not only text",
                        );
                    let mut subfolders = !st.top_level_only;
                    ui.checkbox(&mut subfolders, "Subfolders");
                    st.top_level_only = !subfolders;
                });
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if st.search.is_some() {
                if crate::ui::button(ui, "Stop").clicked() {
                    actions.push(Action::Stop);
                }
                ui.spinner();
            } else if ui
                .add_enabled_ui(!busy && !st.options.text.is_empty(), |ui| {
                    crate::ui::primary_button(ui, "Find all")
                })
                .inner
                .clicked()
            {
                actions.push(Action::Search);
            }
            let can_replace = !busy && st.results_current() && !st.results.is_empty();
            let r = ui
                .add_enabled_ui(can_replace, |ui| crate::ui::button(ui, "Replace all…"))
                .inner
                .on_disabled_hover_text("Run Find all with the current settings first");
            if r.clicked() {
                actions.push(Action::AskReplace);
            }
            if st.replacing.is_some() {
                ui.spinner();
                crate::ui::muted(ui, "Replacing…");
            }
            let status = match (st.finished, st.search.is_some()) {
                (_, true) => format!(
                    "{} matches in {} files · {} files searched",
                    st.match_count(),
                    st.results.len(),
                    st.scanned
                ),
                (Some(cancelled), false) => format!(
                    "{} matches in {} files · {} files searched{}",
                    st.match_count(),
                    st.results.len(),
                    st.scanned,
                    if cancelled { " (stopped)" } else { "" }
                ),
                (None, false) => String::new(),
            };
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                crate::ui::muted(ui, status);
            });
        });
        let _ = p;
        if let Some(e) = &st.error {
            crate::ui::notice(ui, crate::ui::Tone::Danger, e.clone());
        }
        if st.finished.is_some() && !st.results_current() && st.search.is_none() {
            crate::ui::notice(
                ui,
                crate::ui::Tone::Warning,
                "The settings changed since this search. Run Find all again to replace.",
            );
        }
        if let Some(report) = &st.report {
            crate::ui::divider(ui);
            egui::ScrollArea::vertical()
                .id_salt("find-files-report")
                .max_height(120.0)
                .show(ui, |ui| ui.label(report.as_str()));
        }
        crate::ui::divider(ui);
        // One row per file heading and per match, so long result lists only
        // lay out what is visible.
        let rows: Vec<(usize, Option<usize>)> = st
            .results
            .iter()
            .enumerate()
            .flat_map(|(f, fh)| {
                std::iter::once((f, None)).chain((0..fh.hits.len()).map(move |h| (f, Some(h))))
            })
            .collect();
        let root = PathBuf::from(st.folder.trim());
        let row_h = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let pal = crate::ui::palette(ui.ctx());
        let highlight = pal.warning_soft;
        let weak = ui.visuals().weak_text_color();
        let text = ui.visuals().text_color();
        let font = egui::TextStyle::Body.resolve(ui.style());
        egui::ScrollArea::both()
            .id_salt("find-files-results")
            .auto_shrink([false, true])
            .max_height(380.0)
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for &(f, h) in &rows[range] {
                    let fh = &st.results[f];
                    match h {
                        None => {
                            let shown = fh.path.strip_prefix(&root).unwrap_or(&fh.path);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(crate::ui::icon::FILE_TEXT).color(pal.text_muted),
                                );
                                ui.label(RichText::new(shown.display().to_string()).strong());
                                crate::ui::badge(
                                    ui,
                                    fh.hits.len().to_string(),
                                    crate::ui::Tone::Neutral,
                                );
                            });
                        }
                        Some(h) => {
                            let hit = &fh.hits[h];
                            let mut job = LayoutJob::default();
                            let fmt = |color, background| TextFormat {
                                font_id: font.clone(),
                                color,
                                background,
                                ..Default::default()
                            };
                            job.append(
                                &format!("    {:>5}  ", hit.line),
                                0.0,
                                fmt(weak, Color32::TRANSPARENT),
                            );
                            if let Place::Attribute { name, .. } = &hit.place {
                                job.append(
                                    &format!("@{name}  "),
                                    0.0,
                                    fmt(weak, Color32::TRANSPARENT),
                                );
                            }
                            let c = &hit.context;
                            let hl = hit.highlight.clone();
                            job.append(&c[..hl.start], 0.0, fmt(text, Color32::TRANSPARENT));
                            job.append(&c[hl.clone()], 0.0, fmt(text, highlight));
                            job.append(&c[hl.end..], 0.0, fmt(text, Color32::TRANSPARENT));
                            job.wrap.max_rows = 1;
                            let r = ui.add(
                                egui::Label::new(job)
                                    .sense(egui::Sense::click())
                                    .selectable(false),
                            );
                            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                actions.push(Action::Open(fh.path.clone(), hit.place.clone()));
                            }
                        }
                    }
                }
            });
        if !st.unreadable.is_empty() {
            let n = st.unreadable.len();
            ui.collapsing(
                format!(
                    "{n} {} could not be searched",
                    if n == 1 { "file" } else { "files" }
                ),
                |ui| {
                    for (p, why) in &st.unreadable {
                        let shown = p.strip_prefix(&root).unwrap_or(p);
                        ui.label(format!("{}: {why}", shown.display()));
                    }
                },
            );
        }
    }

    fn confirm_replace_ui(&mut self, ctx: &egui::Context, actions: &mut Vec<Action>) {
        let unsaved = self.unsaved_result_files();
        let st = &mut self.find_files;
        let (matches, files) = (st.match_count(), st.results.len());
        let mut keep = true;
        let (x, res) = crate::ui::Dialog::new("Replace in files")
            .width(440.0)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Replace {matches} {} in {files} {} with \u{201c}{}\u{201d}?",
                    if matches == 1 { "match" } else { "matches" },
                    if files == 1 { "file" } else { "files" },
                    st.replacement
                ));
                if !unsaved.is_empty() {
                    ui.add_space(4.0);
                    crate::ui::notice(
                        ui,
                        crate::ui::Tone::Warning,
                        format!(
                            "{} of these {} open with unsaved changes and will be skipped.",
                            unsaved.len(),
                            if unsaved.len() == 1 { "is" } else { "are" }
                        ),
                    );
                }
                ui.add_space(4.0);
                crate::ui::muted(
                    ui,
                    "Files are changed on disk. This cannot be undone from Xylem.",
                );
                crate::ui::dialog_footer(ui, Some(("Replace", true)), Some("Cancel"), |_| {})
            });
        let res = res.unwrap_or_default();
        if res.confirmed {
            actions.push(Action::Replace);
            keep = false;
        }
        if x || res.closed {
            keep = false;
        }
        st.confirm = keep;
    }

    /// Result files that are open with unsaved changes, as path keys.
    fn unsaved_result_files(&self) -> HashSet<PathBuf> {
        let dirty: HashSet<PathBuf> = self
            .docs
            .iter()
            .filter(|d| d.is_dirty())
            .filter_map(|d| d.path.as_deref().map(path_key))
            .collect();
        self.find_files
            .results
            .iter()
            .map(|f| path_key(&f.path))
            .filter(|k| dirty.contains(k))
            .collect()
    }

    fn start_find_files(&mut self) {
        let spec = self.find_files.spec();
        let st = &mut self.find_files;
        st.error = None;
        st.report = None;
        if !spec.root.is_dir() {
            st.error = Some(format!("{} is not a folder.", spec.root.display()));
            return;
        }
        let spec = SearchSpec {
            root: std::path::absolute(&spec.root).unwrap_or(spec.root),
            ..spec
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let ctx = self.ctx.clone();
        match spawn_search(
            spec.clone(),
            cancel.clone(),
            Box::new(move || ctx.request_repaint()),
        ) {
            Ok(rx) => {
                st.results.clear();
                st.unreadable.clear();
                st.scanned = 0;
                st.finished = None;
                st.search = Some((rx, cancel));
                // Keep the field as typed; the snapshot is what matters.
                st.searched = Some(SearchSpec {
                    root: PathBuf::from(st.folder.trim()),
                    ..spec.clone()
                });
                self.settings.find_files_folder = Some(spec.root);
                self.settings.find_files_pattern = spec.pattern;
            }
            Err(e) => st.error = Some(e.to_string()),
        }
    }

    fn start_replace_in_files(&mut self) {
        let Some(spec) = self.find_files.searched.clone() else {
            return;
        };
        let skip = self.unsaved_result_files();
        let st = &mut self.find_files;
        let files = st.results.iter().map(|f| f.path.clone()).collect();
        let ctx = self.ctx.clone();
        st.replacing = Some(spawn_replace(
            files,
            spec.options,
            st.replacement.clone(),
            skip,
            Box::new(move || ctx.request_repaint()),
        ));
    }

    /// Takes whatever the workers have sent since the last frame.
    fn poll_find_files(&mut self) {
        let st = &mut self.find_files;
        if let Some((rx, _)) = &st.search {
            let mut done = false;
            while let Ok(e) = rx.try_recv() {
                match e {
                    SearchEvent::File(f) => st.results.push(f),
                    SearchEvent::Skipped(p, why) => st.unreadable.push((p, why)),
                    SearchEvent::Progress(n) => st.scanned = n,
                    SearchEvent::Done { files, cancelled } => {
                        st.scanned = files;
                        st.finished = Some(cancelled);
                        done = true;
                    }
                }
            }
            if done {
                st.search = None;
            }
        }
        let report = match &st.replacing {
            Some(rx) => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("the replace stopped unexpectedly".to_string()))
                }
            },
            None => return,
        };
        st.replacing = None;
        match report {
            Some(Ok(r)) => self.finish_replace(r),
            Some(Err(e)) => self.find_files.error = Some(e),
            None => {}
        }
    }

    fn finish_replace(&mut self, report: ReplaceReport) {
        // Reload open documents that were rewritten, unless they picked up
        // edits while the replace ran.
        let mut not_reloaded = Vec::new();
        for (path, _) in &report.changed {
            let key = path_key(path);
            let Some(i) = self
                .docs
                .iter()
                .position(|d| d.path.as_deref().is_some_and(|p| path_key(p) == key))
            else {
                continue;
            };
            if self.docs[i].is_dirty() || !self.reload_document(i) {
                not_reloaded.push(path.clone());
            }
        }
        let root = PathBuf::from(self.find_files.folder.trim());
        let name = |p: &Path| p.strip_prefix(&root).unwrap_or(p).display().to_string();
        let mut lines = vec![format!(
            "Replaced {} {} in {} {}.",
            report.replacements(),
            if report.replacements() == 1 {
                "match"
            } else {
                "matches"
            },
            report.changed.len(),
            if report.changed.len() == 1 {
                "file"
            } else {
                "files"
            }
        )];
        for (p, n) in &report.changed {
            lines.push(format!("  {}: {n}", name(p)));
        }
        if !report.skipped_unsaved.is_empty() {
            lines.push("Skipped, open with unsaved changes:".into());
            lines.extend(
                report
                    .skipped_unsaved
                    .iter()
                    .map(|p| format!("  {}", name(p))),
            );
        }
        for (p, e) in &report.failed {
            lines.push(format!("Failed: {}: {e}", name(p)));
        }
        if !not_reloaded.is_empty() {
            lines.push(
                "Changed on disk but not reloaded, because they were edited meanwhile:".into(),
            );
            lines.extend(not_reloaded.iter().map(|p| format!("  {}", name(p))));
        }
        let summary = lines[0].clone();
        let problems = !report.failed.is_empty() || !report.skipped_unsaved.is_empty();
        self.toast(
            if report.skipped_unsaved.is_empty() {
                summary
            } else {
                format!(
                    "{summary} Skipped {} with unsaved changes.",
                    report.skipped_unsaved.len()
                )
            },
            problems,
        );
        let st = &mut self.find_files;
        st.report = Some(lines.join("\n"));
        // The old results point at text that has changed.
        st.results.clear();
        st.finished = None;
        st.searched = None;
    }

    /// Replaces an open, unmodified document with a fresh copy from disk.
    pub(super) fn reload_document(&mut self, i: usize) -> bool {
        let Some(path) = self.docs[i].path.clone() else {
            return false;
        };
        let Ok(doc) = std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|b| Document::from_bytes(&b).map_err(|e| e.to_string()))
        else {
            return false;
        };
        let (view, active) = (self.docs[i].view, self.active);
        self.add_doc(doc, Some(path));
        let mut fresh = self.docs.pop().expect("add_doc pushes the document");
        fresh.view = view;
        if view == View::Source {
            fresh.source = Some(SourceState {
                text: fresh.editor.doc.to_xml(),
                error: None,
                revision: fresh.editor.doc.revision(),
                goto: None,
            });
        }
        self.docs[i] = fresh;
        self.active = active;
        // Anything holding node ids of the old document is now meaningless.
        self.context_menu = None;
        self.palette = None;
        self.dialogs.node_edit = None;
        true
    }

    /// Opens a result's file and selects the match.
    fn open_find_result(&mut self, path: &Path, place: &Place) {
        let key = path_key(path);
        let find = |docs: &[crate::doc::DocState]| {
            docs.iter()
                .position(|d| d.path.as_deref().is_some_and(|p| path_key(p) == key))
        };
        let i = match find(&self.docs) {
            Some(i) => i,
            None => {
                self.open_path(path.to_path_buf());
                match find(&self.docs) {
                    Some(i) => i,
                    None => return,
                }
            }
        };
        self.active = i;
        if self.docs[i].view == View::Source {
            // Matches are selected in the document views only.
            let v = match self.settings.default_view {
                View::Source => View::TagsOn,
                v => v,
            };
            self.set_view(v);
            if self.docs[i].view == View::Source {
                return;
            }
        }
        let options = self
            .find_files
            .searched
            .as_ref()
            .map(|s| s.options.clone())
            .unwrap_or_else(|| self.find_files.options.clone());
        let ds = &mut self.docs[i];
        match place {
            Place::Text { nth } => {
                self.find.adopt(&options);
                if let Some(FindAction::Goto(node, range)) = self.find.goto_nth(ds, *nth) {
                    ds.editor.sel = Selection {
                        anchor: Pos::new(node, range.start),
                        focus: Pos::new(node, range.end),
                    };
                }
            }
            Place::Attribute { element, .. } => {
                let doc = &ds.editor.doc;
                if let Some(el) = doc
                    .descendants(doc.root())
                    .filter(|&n| doc.is_element(n))
                    .nth(*element)
                {
                    let p = xylem_edit::pos::first_editable(doc, &xylem_edit::NoRules, el);
                    ds.editor.set_caret(p);
                    self.settings.show_attributes = true;
                }
            }
        }
        ds.canvas.scroll_to_caret = true;
        ds.canvas.request_focus = true;
    }
}
