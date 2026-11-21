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
    // Tuned to sit with the theme: the accent marks element names, and every
    // colour keeps at least 4.5:1 contrast on the source background.
    let rgb = |hex: u32| Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    let pal = crate::ui::Palette::for_dark_mode(dark);
    match (t, dark) {
        (Tok::Text, _) => pal.text,
        (Tok::Tag, _) => pal.text_faint,
        (Tok::Name, _) => pal.accent,
        (Tok::AttrName, true) => rgb(0xE2AC45),
        (Tok::AttrName, false) => rgb(0x8F5A00),
        (Tok::AttrValue, true) => rgb(0x86B6F2),
        (Tok::AttrValue, false) => rgb(0x2A5CA6),
        (Tok::Comment, _) => pal.text_muted,
        (Tok::Pi, true) => rgb(0xC99AE6),
        (Tok::Pi, false) => rgb(0x7A3E9D),
        (Tok::Entity, _) => pal.danger,
        (Tok::CData, true) => rgb(0xCFC783),
        (Tok::CData, false) => rgb(0x6A651C),
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
    let pal = crate::ui::palette(ui.ctx());
    let font = FontId::new(13.0 * zoom, egui::FontFamily::Name("mono".into()));
    let mut changed = false;
    if let Some((msg, _)) = error {
        crate::ui::notice(
            ui,
            crate::ui::Tone::Danger,
            format!("Not well-formed: {msg}. Press F4 to go to the problem."),
        );
        ui.add_space(4.0);
    }
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .id_salt("source")
        .show(ui, |ui| {
            // Lines don't wrap, so the gutter's numbers line up with them.
            let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
                let mut job = highlight(buf.as_str(), font.clone(), dark);
                job.wrap.max_width = f32::INFINITY;
                ui.fonts_mut(|f| f.layout_job(job))
            };
            let lines = text.lines().count().max(1) + usize::from(text.ends_with('\n'));
            let digits = lines.to_string().len().max(3);
            let numbers: String = (1..=lines).map(|n| format!("{n:>digits$}\n")).collect();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let gutter_font = font.clone();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(numbers)
                            .font(gutter_font)
                            .color(pal.text_faint),
                    )
                    .selectable(false)
                    .extend(),
                );
                ui.add_space(4.0);
                let line_x = ui.cursor().left();
                ui.painter().vline(
                    line_x,
                    ui.clip_rect().y_range(),
                    egui::Stroke::new(1.0_f32, pal.border),
                );
                ui.add_space(8.0);
                let out = TextEdit::multiline(text)
                    .code_editor()
                    .frame(false)
                    .margin(egui::Margin::ZERO)
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
        });
    changed
}
