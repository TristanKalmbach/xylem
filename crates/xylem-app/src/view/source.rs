//! Plain-text (source) view: an XML code editor with syntax highlighting.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, TextEdit, Ui};

#[derive(Copy, Clone, PartialEq)]
enum Tok {
    Text,
    Tag,
    Name,
    AttrName,
    AttrValue,
    Comment,
    Pi,
    Entity,
    CData,
}

fn color(t: Tok, dark: bool) -> Color32 {
    match (t, dark) {
        (Tok::Text, true) => Color32::from_gray(220),
        (Tok::Text, false) => Color32::from_gray(25),
        (Tok::Tag, true) => Color32::from_rgb(130, 150, 170),
        (Tok::Tag, false) => Color32::from_rgb(90, 90, 120),
        (Tok::Name, true) => Color32::from_rgb(110, 180, 255),
        (Tok::Name, false) => Color32::from_rgb(20, 60, 170),
        (Tok::AttrName, true) => Color32::from_rgb(220, 170, 110),
        (Tok::AttrName, false) => Color32::from_rgb(150, 70, 10),
        (Tok::AttrValue, true) => Color32::from_rgb(150, 210, 140),
        (Tok::AttrValue, false) => Color32::from_rgb(20, 120, 40),
        (Tok::Comment, _) => Color32::from_rgb(120, 130, 120),
        (Tok::Pi, true) => Color32::from_rgb(200, 140, 220),
        (Tok::Pi, false) => Color32::from_rgb(120, 40, 140),
        (Tok::Entity, _) => Color32::from_rgb(200, 60, 120),
        (Tok::CData, true) => Color32::from_rgb(200, 200, 140),
        (Tok::CData, false) => Color32::from_rgb(110, 110, 30),
    }
}

/// Highlights XML into a layout job.
pub fn highlight(text: &str, font: FontId, dark: bool) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut push = |s: &str, t: Tok| {
        if !s.is_empty() {
            job.append(
                s,
                0.0,
                TextFormat {
                    font_id: font.clone(),
                    color: color(t, dark),
                    ..Default::default()
                },
            );
        }
    };
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            let end = rest.find("-->").map(|e| e + 3).unwrap_or(rest.len());
            push(&rest[..end], Tok::Comment);
            i += end;
        } else if rest.starts_with("<![CDATA[") {
            let end = rest.find("]]>").map(|e| e + 3).unwrap_or(rest.len());
            push(&rest[..end], Tok::CData);
            i += end;
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            push(&rest[..end], Tok::Pi);
            i += end;
        } else if rest.starts_with('<') {
            let end = rest.find('>').map(|e| e + 1).unwrap_or(rest.len());
            let tag = &rest[..end];
            // < or </
            let open = if tag.starts_with("</") { 2 } else { 1 };
            push(&tag[..open], Tok::Tag);
            let body = &tag[open..];
            let name_end = body
                .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                .unwrap_or(body.len());
            push(&body[..name_end], Tok::Name);
            let mut j = name_end;
            let bb = body.as_bytes();
            while j < body.len() {
                let c = bb[j];
                if c == b'"' || c == b'\'' {
                    let q = c as char;
                    let close = body[j + 1..]
                        .find(q)
                        .map(|e| j + e + 2)
                        .unwrap_or(body.len());
                    push(&body[j..close], Tok::AttrValue);
                    j = close;
                } else if (c as char).is_alphabetic() || c == b'_' || c == b':' {
                    let e = body[j..]
                        .find(|ch: char| ch == '=' || ch.is_whitespace() || ch == '>' || ch == '/')
                        .map(|e| j + e)
                        .unwrap_or(body.len());
                    push(&body[j..e], Tok::AttrName);
                    j = e;
                } else {
                    let ch_len = body[j..].chars().next().map(char::len_utf8).unwrap_or(1);
                    push(&body[j..j + ch_len], Tok::Tag);
                    j += ch_len;
                }
            }
            i += end;
        } else if rest.starts_with('&') {
            let end = rest
                .find(';')
                .map(|e| e + 1)
                .filter(|&e| e < 40)
                .unwrap_or(1);
            push(&rest[..end], Tok::Entity);
            i += end;
        } else {
            let end = rest.find(['<', '&']).unwrap_or(rest.len()).max(1);
            let end = (0..=end)
                .rev()
                .find(|&e| rest.is_char_boundary(e))
                .unwrap_or(end);
            push(&rest[..end], Tok::Text);
            i += end;
        }
    }
    job
}

/// Shows the source editor; returns true when the text changed.
pub fn show(
    ui: &mut Ui,
    text: &mut String,
    error: Option<&(String, usize)>,
    zoom: f32,
    goto: &mut Option<usize>,
) -> bool {
    let dark = ui.visuals().dark_mode;
    let font = FontId::new(13.0 * zoom, egui::FontFamily::Name("mono".into()));
    let mut changed = false;
    if let Some((msg, _)) = error {
        ui.colored_label(
            Color32::from_rgb(220, 60, 60),
            format!("⚠ Not well-formed: {msg}"),
        );
    }
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .id_salt("source")
        .show(ui, |ui| {
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                let mut job = highlight(buf.as_str(), font.clone(), dark);
                job.wrap.max_width = wrap;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let out = TextEdit::multiline(text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(40)
                .lock_focus(true)
                .layouter(&mut layouter)
                .show(ui);
            if out.response.changed() {
                changed = true;
            }
            if let Some(offset) = goto.take() {
                let ci = text[..offset.min(text.len())].chars().count();
                let mut state = out.state.clone();
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::one(
                        egui::text::CCursor::new(ci),
                    )));
                state.store(ui.ctx(), out.response.id);
                out.response.request_focus();
                let r = out
                    .galley
                    .pos_from_cursor(egui::text::CCursor::new(ci))
                    .translate(out.galley_pos.to_vec2());
                ui.scroll_to_rect(r.expand(40.0), Some(egui::Align::Center));
            }
        });
    changed
}
