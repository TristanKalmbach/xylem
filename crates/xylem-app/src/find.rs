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
    focus_query: bool,
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
    let mut open = st.open;
    if !st.focus_query {
        st.focus_query = true;
    }
    egui::Window::new("Find / Replace")
        .open(&mut open)
        .default_width(380.0)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("find-grid").num_columns(2).show(ui, |ui| {
                ui.label("Find");
                let r = ui.add(egui::TextEdit::singleline(&mut st.query).desired_width(260.0));
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    st.refresh(ds);
                    if let Some(i) = st.next_from_caret(ds, true) {
                        st.current = Some(i);
                        let m = st.matches[i].clone();
                        out.push(FindAction::Goto(m.0, m.1));
                    }
                    r.request_focus();
                }
                ui.end_row();
                ui.label("Replace");
                ui.add(egui::TextEdit::singleline(&mut st.replacement).desired_width(260.0));
                ui.end_row();
                ui.label("In element");
                ui.add(
                    egui::TextEdit::singleline(&mut st.in_element)
                        .hint_text("(anywhere)")
                        .desired_width(260.0),
                );
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut st.match_case, "Match case");
                ui.checkbox(&mut st.whole_word, "Whole word");
                ui.checkbox(&mut st.regex, "Regex");
            });
            st.refresh(ds);
            if let Some(e) = &st.error {
                ui.colored_label(egui::Color32::from_rgb(220, 70, 70), e);
            }
            ui.horizontal(|ui| {
                let n = st.matches.len();
                if ui.button("◀ Previous").clicked()
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
                if ui.button("Next ▶").clicked()
                    && let Some(i) = st.next_from_caret(ds, true)
                {
                    st.current = Some(i);
                    let m = st.matches[i].clone();
                    out.push(FindAction::Goto(m.0, m.1));
                }
                if ui
                    .add_enabled(st.current.is_some(), egui::Button::new("Replace"))
                    .clicked()
                    && let Some(i) = st.current
                {
                    let m = st.matches[i].clone();
                    let with = st.expand(ds, &m);
                    out.push(FindAction::Replace(m.0, m.1, with));
                    st.current = None;
                }
                if ui
                    .add_enabled(n > 0, egui::Button::new("Replace all"))
                    .clicked()
                {
                    let items = st
                        .matches
                        .iter()
                        .map(|m| (m.0, m.1.clone(), st.expand(ds, m)))
                        .collect();
                    out.push(FindAction::ReplaceAll(items));
                }
                ui.label(
                    RichText::new(match (st.current, n) {
                        (_, 0) if !st.query.is_empty() => "No matches".to_string(),
                        (Some(i), n) => format!("{} of {n}", i + 1),
                        (None, n) if n > 0 => format!("{n} matches"),
                        _ => String::new(),
                    })
                    .weak(),
                );
            });
        });
    st.open = open;
}
