//! Find / Replace over the document's text content.

use crate::doc::DocState;
use crate::find_files::{SearchOptions, text_matches};
use egui::RichText;
use std::ops::Range;
use xylem_dom::NodeId;
use xylem_edit::cmp_pos;

pub enum FindAction {
    Goto(NodeId, Range<usize>),
    Replace(NodeId, Range<usize>, String),
    ReplaceAll(Vec<(NodeId, Range<usize>, String)>),
}

#[derive(Default)]
pub struct FindState {
    pub open: bool,
    pub query: String,
    pub replacement: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Restrict matches to text inside elements with this name.
    pub in_element: String,
    matches: Vec<(NodeId, Range<usize>)>,
    current: Option<usize>,
    key: Option<(String, bool, bool, bool, String, u64)>,
    pub error: Option<String>,
    /// Put the keyboard in the Find field on the next frame.
    pub focus_query: bool,
}

impl FindState {
    pub fn highlights(&self) -> Vec<(NodeId, Range<usize>)> {
        if self.open {
            self.matches.iter().take(5000).cloned().collect()
        } else {
            Vec::new()
        }
    }

    fn refresh(&mut self, ds: &DocState) {
        let key = (
            self.query.clone(),
            self.match_case,
            self.whole_word,
            self.regex,
            self.in_element.clone(),
            ds.editor.doc.revision(),
        );
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.matches.clear();
        self.error = None;
        if self.query.is_empty() {
            self.current = None;
            return;
        }
        let re = match self.options().build() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        let doc = &ds.editor.doc;
        let within = self.in_element.as_str();
        self.matches = text_matches(doc, &re, |n| {
            within.is_empty() || doc.ancestors(n).any(|a| doc.name(a) == Some(within))
        });
        self.current = None;
    }

    /// The current settings in the form the shared matcher takes.
    fn options(&self) -> SearchOptions {
        SearchOptions {
            text: self.query.clone(),
            match_case: self.match_case,
            whole_word: self.whole_word,
            regex: self.regex,
            attributes: false,
        }
    }

    /// Takes over a search from find in files, so F3 continues through the
    /// same matches in the document that was opened.
    pub fn adopt(&mut self, opts: &SearchOptions) {
        self.query = opts.text.clone();
        self.match_case = opts.match_case;
        self.whole_word = opts.whole_word;
        self.regex = opts.regex;
        self.in_element.clear();
        self.key = None;
    }

    /// Selects the `n`th match in document order (the numbering find in
    /// files reports), or the last one if the document has fewer.
    pub fn goto_nth(&mut self, ds: &DocState, n: usize) -> Option<FindAction> {
        self.refresh(ds);
        let i = n.min(self.matches.len().checked_sub(1)?);
        self.current = Some(i);
        let m = self.matches[i].clone();
        Some(FindAction::Goto(m.0, m.1))
    }

    fn expand(&self, ds: &DocState, m: &(NodeId, Range<usize>)) -> String {
        let t = ds.editor.doc.text(m.0).unwrap_or("");
        let opts = self.options();
        match opts.build() {
            Ok(re) => opts.expand(&re, t, m.1.clone(), &self.replacement),
            Err(_) => self.replacement.clone(),
        }
    }

    /// Index of the first match after the caret (wrapping).
    /// Find next/previous with the current settings (F3 / Shift+F3), even
    /// while the dialog is closed.
    pub fn step(&mut self, ds: &DocState, forward: bool) -> Option<FindAction> {
        if self.query.is_empty() {
            return None;
        }
        self.refresh(ds);
        let n = self.matches.len();
        let mut i = self.next_from_caret(ds, forward)?;
        if Some(i) == self.current && n > 0 {
            i = if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            };
        }
        self.current = Some(i);
        let m = self.matches[i].clone();
        Some(FindAction::Goto(m.0, m.1))
    }

    pub fn has_query(&self) -> bool {
        !self.query.is_empty()
    }

    fn next_from_caret(&self, ds: &DocState, forward: bool) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        let doc = &ds.editor.doc;
        let caret = if forward {
            ds.editor.sel.ordered(doc).1
        } else {
            ds.editor.sel.ordered(doc).0
        };
        let pos_of = |m: &(NodeId, Range<usize>), end: bool| {
            xylem_edit::Pos::new(m.0, if end { m.1.end } else { m.1.start })
        };
        if forward {
            self.matches
                .iter()
                .position(|m| cmp_pos(doc, pos_of(m, false), caret).is_ge())
                .or(Some(0))
        } else {
            self.matches
                .iter()
                .rposition(|m| cmp_pos(doc, pos_of(m, true), caret).is_le())
                .or(Some(self.matches.len() - 1))
        }
    }
}

pub fn window(ctx: &egui::Context, st: &mut FindState, ds: &DocState, out: &mut Vec<FindAction>) {
    use crate::ui::{Tone, form, icon, icon_button, notice, palette, text_field};
    let p = palette(ctx);
    let (close, _) = crate::ui::Dialog::new("Find and replace")
        .width(420.0)
        .modeless()
        .show(ctx, |ui| {
            form(ui, "find-form", |f| {
                f.row("Find", |ui| {
                    let r = text_field(ui, "Find text", &mut st.query, "Text to find", 280.0);
                    if std::mem::take(&mut st.focus_query) {
                        r.request_focus();
                        // Select what is there, so typing replaces it.
                        let mut state =
                            egui::TextEdit::load_state(ui.ctx(), r.id).unwrap_or_default();
                        let n = st.query.chars().count();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(n),
                            )));
                        state.store(ui.ctx(), r.id);
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        st.refresh(ds);
                        if let Some(i) = st.next_from_caret(ds, true) {
                            st.current = Some(i);
                            let m = st.matches[i].clone();
                            out.push(FindAction::Goto(m.0, m.1));
                        }
                        r.request_focus();
                    }
                });
                f.row("Replace with", |ui| {
                    text_field(ui, "Replace text", &mut st.replacement, "", 280.0);
                });
                f.row("Only inside", |ui| {
                    text_field(ui, "In element", &mut st.in_element, "Any element", 280.0);
                });
                f.row("", |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut st.match_case, "Match case");
                        ui.checkbox(&mut st.whole_word, "Whole word");
                        ui.checkbox(&mut st.regex, "Regex");
                    });
                });
            });
            st.refresh(ds);
            if let Some(e) = &st.error {
                notice(ui, Tone::Danger, e.clone());
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let n = st.matches.len();
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_enabled_ui(n > 0, |ui| {
                    if icon_button(ui, icon::CARET_LEFT, "Previous match", Some("Shift+F3"))
                        .clicked()
                        && let Some(i) = st.next_from_caret(ds, false)
                    {
                        let i = if Some(i) == st.current {
                            (i + n - 1) % n.max(1)
                        } else {
                            i
                        };
                        st.current = Some(i);
                        let m = st.matches[i].clone();
                        out.push(FindAction::Goto(m.0, m.1));
                    }
                    if icon_button(ui, icon::CARET_RIGHT, "Next match", Some("F3")).clicked()
                        && let Some(i) = st.next_from_caret(ds, true)
                    {
                        st.current = Some(i);
                        let m = st.matches[i].clone();
                        out.push(FindAction::Goto(m.0, m.1));
                    }
                });
                ui.add_space(6.0);
                ui.label(
                    RichText::new(match (st.current, n) {
                        (_, 0) if !st.query.is_empty() => "No matches".to_string(),
                        (Some(i), n) => format!("{} of {n}", i + 1),
                        (None, n) if n > 0 => format!("{n} matches"),
                        _ => String::new(),
                    })
                    .color(if n == 0 && !st.query.is_empty() {
                        p.danger
                    } else {
                        p.text_muted
                    }),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui
                        .add_enabled_ui(n > 0, |ui| crate::ui::button(ui, "Replace all"))
                        .inner
                        .clicked()
                    {
                        let items = st
                            .matches
                            .iter()
                            .map(|m| (m.0, m.1.clone(), st.expand(ds, m)))
                            .collect();
                        out.push(FindAction::ReplaceAll(items));
                    }
                    if ui
                        .add_enabled_ui(st.current.is_some(), |ui| crate::ui::button(ui, "Replace"))
                        .inner
                        .clicked()
                        && let Some(i) = st.current
                    {
                        let m = st.matches[i].clone();
                        let with = st.expand(ds, &m);
                        out.push(FindAction::Replace(m.0, m.1, with));
                        st.current = None;
                    }
                });
            });
        });
    if close {
        st.open = false;
    }
}
