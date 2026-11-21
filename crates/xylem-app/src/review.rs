//! Track changes and review comments in the app: who the author is, the time
//! stamps, revision colours and the Review panel. The markup itself and the
//! editing rules live in `xylem_edit::revision`.

use crate::doc::DocState;
use egui::{Color32, RichText, Ui};
use xylem_dom::NodeId;
use xylem_edit::revision::{self, Change, ChangeKind};

/// The user's login name, the default review author.
pub fn os_user_name() -> String {
    ["USERNAME", "USER", "LOGNAME"]
        .iter()
        .find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()))
        .unwrap_or_else(|| "Anonymous".to_string())
}

/// The current time in XMetaL's stamp format. The standard library has no
/// time zone support, so stamps are written in UTC with a `+0000` offset;
/// they are exact, just not in local time.
pub fn now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    revision::format_time(secs, 0)
}

/// A colour per author, so reviewers can tell each other's changes apart.
pub fn author_color(author: &str, dark: bool) -> Color32 {
    const LIGHT: [Color32; 6] = [
        Color32::from_rgb(0x1f, 0x5f, 0xc8),
        Color32::from_rgb(0xb0, 0x30, 0x90),
        Color32::from_rgb(0x1e, 0x80, 0x40),
        Color32::from_rgb(0xc0, 0x50, 0x10),
        Color32::from_rgb(0x70, 0x40, 0xc0),
        Color32::from_rgb(0x10, 0x80, 0x90),
    ];
    const DARK: [Color32; 6] = [
        Color32::from_rgb(0x70, 0xa8, 0xff),
        Color32::from_rgb(0xf0, 0x80, 0xd8),
        Color32::from_rgb(0x70, 0xd0, 0x90),
        Color32::from_rgb(0xff, 0xa0, 0x60),
        Color32::from_rgb(0xb8, 0x98, 0xff),
        Color32::from_rgb(0x60, 0xd0, 0xe0),
    ];
    let h = author
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let i = h as usize % LIGHT.len();
    if dark { DARK[i] } else { LIGHT[i] }
}

/// What a deletion shows in the document: its text with runs of whitespace
/// collapsed (keeping one at either end, since it separated words), or, for
/// deleted markup with no text, what it was in brackets.
pub fn deleted_label(xml: &str) -> String {
    let frag = xylem_dom::parse_fragment(xml).ok();
    let text = match &frag {
        Some(f) => f.text_content(f.root()),
        None => xml.to_string(),
    };
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !flat.is_empty() {
        let lead = if text.starts_with(char::is_whitespace) {
            " "
        } else {
            ""
        };
        let tail = if text.ends_with(char::is_whitespace) {
            " "
        } else {
            ""
        };
        return format!("{lead}{flat}{tail}");
    }
    if !text.is_empty() {
        // A deleted space: still worth seeing struck through.
        return " ".to_string();
    }
    let what = frag.as_ref().and_then(|f| {
        f.children(f.root()).iter().find_map(|&c| match f.kind(c) {
            xylem_dom::NodeKind::Element(e) => Some(e.name.clone()),
            xylem_dom::NodeKind::Comment(_) => Some("comment".to_string()),
            xylem_dom::NodeKind::Pi { target, .. } => Some(target.clone()),
            _ => None,
        })
    });
    format!("[{}]", what.as_deref().unwrap_or("deleted"))
}

/// "Ann Reviewer" -> "AR", "bob" -> "B", "" -> "?".
pub fn initials(author: &str) -> String {
    let s: String = author
        .split(|c: char| c.is_whitespace() || c == '.' || c == '_')
        .filter_map(|w| w.chars().next())
        .flat_map(char::to_uppercase)
        .take(3)
        .collect();
    if s.is_empty() { "?".into() } else { s }
}

/// Background of commented text and comment markers.
pub fn comment_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(0x5a, 0x4c, 0x10)
    } else {
        Color32::from_rgb(0xff, 0xec, 0x90)
    }
}

/// One-line description of a change, used in tooltips.
pub fn describe(c: &Change) -> String {
    let what = match c.kind {
        ChangeKind::Insertion => "Inserted",
        ChangeKind::Deletion => "Deleted",
        ChangeKind::Comment => "Comment",
    };
    let who = if c.author.is_empty() {
        "unknown author"
    } else {
        c.author.as_str()
    };
    format!("{what} by {who}, {}", revision::display_time(&c.time))
}

pub enum ReviewAction {
    Goto(NodeId),
    Accept(NodeId),
    Reject(NodeId),
    SaveComment(NodeId, String),
    DeleteComment(NodeId),
    ResolveAll(bool),
}

#[derive(Default)]
pub struct ReviewState {
    /// The Review panel is shown (not remembered across runs).
    pub open: bool,
    /// The comment being edited in the panel and its text so far.
    pub editing: Option<(NodeId, String)>,
    /// Changes of the active document, by document revision.
    cache: Option<(u64, u64, Vec<Change>)>,
}

impl ReviewState {
    fn changes(&mut self, ds: &DocState) -> &[Change] {
        let key = (ds.id, ds.editor.doc.revision());
        if self.cache.as_ref().is_none_or(|c| (c.0, c.1) != key) {
            self.cache = Some((key.0, key.1, revision::changes(&ds.editor.doc)));
        }
        &self.cache.as_ref().unwrap().2
    }
}

fn excerpt(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        let cut: String = flat.chars().take(max).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// The Review panel: every change and comment in document order.
pub fn panel(ui: &mut Ui, ds: &DocState, state: &mut ReviewState, out: &mut Vec<ReviewAction>) {
    let dark = ui.visuals().dark_mode;
    let tracking = ds.editor.is_tracking();
    ui.horizontal(|ui| {
        ui.label(if tracking {
            RichText::new("Tracking changes").color(author_color("", dark))
        } else {
            RichText::new("Not tracking changes").weak()
        });
    });
    ui.horizontal(|ui| {
        if ui.button("Accept all").clicked() {
            out.push(ReviewAction::ResolveAll(true));
        }
        if ui.button("Reject all").clicked() {
            out.push(ReviewAction::ResolveAll(false));
        }
    });
    ui.separator();
    let caret_change = ds.editor.change_at(ds.editor.caret()).map(|c| c.node);
    let changes = state.changes(ds).to_vec();
    if changes.is_empty() {
        ui.weak("No changes or comments.");
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .id_salt("review")
        .show(ui, |ui| {
            for c in &changes {
                let color = author_color(&c.author, dark);
                let current = caret_change == Some(c.node);
                egui::Frame::group(ui.style())
                    .stroke(if current {
                        egui::Stroke::new(1.5_f32, color)
                    } else {
                        ui.visuals().widgets.noninteractive.bg_stroke
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let head = RichText::new(describe(c)).color(color).small();
                        if ui
                            .add(egui::Label::new(head).sense(egui::Sense::click()))
                            .clicked()
                        {
                            out.push(ReviewAction::Goto(c.node));
                        }
                        let body = match c.kind {
                            ChangeKind::Insertion => {
                                RichText::new(excerpt(&c.excerpt, 120)).underline()
                            }
                            ChangeKind::Deletion => {
                                RichText::new(excerpt(&c.excerpt, 120)).strikethrough()
                            }
                            ChangeKind::Comment => RichText::new(excerpt(&c.comment, 400)),
                        };
                        let editing = matches!(&state.editing, Some((n, _)) if *n == c.node);
                        if editing {
                            let mut done = None;
                            if let Some((_, text)) = &mut state.editing {
                                ui.add(
                                    egui::TextEdit::multiline(text)
                                        .desired_rows(3)
                                        .desired_width(f32::INFINITY),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button("Save").clicked() {
                                        done = Some(Some(text.clone()));
                                    }
                                    if ui.button("Cancel").clicked() {
                                        done = Some(None);
                                    }
                                });
                            }
                            match done {
                                Some(Some(text)) => {
                                    out.push(ReviewAction::SaveComment(c.node, text));
                                    state.editing = None;
                                }
                                Some(None) => state.editing = None,
                                None => {}
                            }
                        } else if ui
                            .add(egui::Label::new(body).sense(egui::Sense::click()))
                            .clicked()
                        {
                            out.push(ReviewAction::Goto(c.node));
                        }
                        if c.kind == ChangeKind::Comment && !c.excerpt.is_empty() {
                            ui.label(
                                RichText::new(format!(
                                    "on \u{201c}{}\u{201d}",
                                    excerpt(&c.excerpt, 60)
                                ))
                                .weak()
                                .small(),
                            );
                        }
                        ui.horizontal(|ui| match c.kind {
                            ChangeKind::Comment => {
                                if !editing && ui.small_button("Edit").clicked() {
                                    state.editing = Some((c.node, c.comment.clone()));
                                }
                                if ui.small_button("Delete").clicked() {
                                    out.push(ReviewAction::DeleteComment(c.node));
                                }
                            }
                            _ => {
                                if ui.small_button("Accept").clicked() {
                                    out.push(ReviewAction::Accept(c.node));
                                }
                                if ui.small_button("Reject").clicked() {
                                    out.push(ReviewAction::Reject(c.node));
                                }
                            }
                        });
                    });
            }
        });
}
