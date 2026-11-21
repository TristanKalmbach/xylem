//! The interactive document canvas: painting, hit testing, caret, selection
//! and keyboard/clipboard handling for the tags-on and normal views.

use super::layout::{
    Decor, DocLayout, LayoutCache, LayoutParams, Layouter, Loc, SegKind, ViewMode, tag_color,
};
use super::style::StyleLookup;
use egui::{
    Color32, CursorIcon, Event, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2,
};
use std::collections::HashMap;
use xylem_dom::NodeId;
use xylem_edit::pos::{self, cmp_pos, container};
use xylem_edit::{EditError, Editor, Pos, Rules, Selection};

/// Requests from the canvas to the application.
#[derive(Debug, Clone)]
pub enum CanvasAction {
    Message(String),
    ContextMenu {
        node: NodeId,
    },
    #[allow(dead_code)] // carries the element for future targeted editing
    EditAttributes(NodeId),
    EditNode(NodeId),
    OpenLink(NodeId),
    /// The selection or document changed (refresh panels).
    Changed,
}

/// Shared clipboard: plain text plus the XML it came from.
#[derive(Default, Clone)]
pub struct XmlClipboard {
    pub text: String,
    pub xml: String,
}

#[derive(Default)]
pub struct Canvas {
    pub layout: DocLayout,
    key: Option<(u64, u64, u64, u32, ViewMode, u32, bool)>,
    cache: LayoutCache,
    /// Desired x for vertical caret movement.
    goal_x: Option<f32>,
    pub scroll_to_caret: bool,
    pub scroll_to_y: Option<f32>,
    drag_anchor: Option<Pos>,
    last_click: Option<(f64, Pos2, u32)>,
    pub has_focus: bool,
    pub request_focus: bool,
    /// Find highlights: (text node, byte range).
    pub highlights: Vec<(NodeId, std::ops::Range<usize>)>,
    /// Nodes flagged by validation (painted with a squiggle).
    pub flagged: Vec<NodeId>,
    pub misspelled: Vec<(NodeId, std::ops::Range<usize>)>,
    misspelled_rev: Option<u64>,
    /// Where the caret was last placed on screen. A position between two
    /// blocks shows up twice (end of one line, start of the next); this keeps
    /// the caret on the line the user put it on.
    caret_hint: Option<(Pos, Loc)>,
    /// The caret's rectangle on screen in the last frame (for popups).
    pub caret_screen: Option<Rect>,
    /// Textures for CSS generated-content images, by file.
    icons: std::collections::HashMap<std::path::PathBuf, Option<egui::TextureHandle>>,
    /// Elements collapsed to a single line in this document.
    pub collapsed: std::collections::HashSet<NodeId>,
    collapsed_rev: u64,
}

pub struct CanvasCtx<'a> {
    pub rules: &'a dyn Rules,
    pub styles: &'a dyn StyleLookup,
    pub style_revision: u64,
    pub view: ViewMode,
    pub zoom: f32,
    pub fonts: &'a super::layout::FontSet,
    pub tag_colors: &'a HashMap<String, Color32>,
    pub clipboard: &'a mut XmlClipboard,
    pub image_size: &'a dyn Fn(NodeId) -> Option<(String, egui::Vec2)>,
    pub image_texture: &'a dyn Fn(&egui::Context, NodeId) -> Option<egui::TextureHandle>,
    pub read_only: bool,
    /// Identifies the document for per-document UI state (scroll position).
    pub doc_key: u64,
}

impl Canvas {
    pub fn misspelled_revision(&self) -> Option<u64> {
        self.misspelled_rev
    }

    pub fn set_misspelled(&mut self, words: Vec<(NodeId, std::ops::Range<usize>)>, rev: u64) {
        self.misspelled = words;
        self.misspelled_rev = Some(rev);
    }

    /// Collapses or expands `el` in the document view.
    pub fn set_collapsed(&mut self, el: NodeId, collapsed: bool) {
        let changed = if collapsed { self.collapsed.insert(el) } else { self.collapsed.remove(&el) };
        if changed {
            self.collapsed_rev += 1;
        }
    }

    pub fn expand_all(&mut self) {
        if !self.collapsed.is_empty() {
            self.collapsed.clear();
            self.collapsed_rev += 1;
        }
    }

    pub fn invalidate(&mut self) {
        self.key = None;
    }

    fn ensure_layout(&mut self, ui: &Ui, ed: &Editor, cx: &CanvasCtx, width: f32) {
        let key = (
            ed.doc.revision(),
            self.collapsed_rev,
            cx.style_revision,
            width.round() as u32,
            cx.view,
            (cx.zoom * 100.0) as u32,
            ui.visuals().dark_mode,
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        let params = LayoutParams {
            view: cx.view,
            width,
            zoom: cx.zoom,
            fonts: cx.fonts,
            tag_colors: cx.tag_colors,
            dark: ui.visuals().dark_mode,
            image_size: cx.image_size,
            collapsed: &self.collapsed,
        };
        let t = std::time::Instant::now();
        self.caret_hint = None;
        self.layout = Layouter::run(
            ui.ctx(),
            &ed.doc,
            cx.styles,
            cx.rules,
            params,
            &mut self.cache,
        );
        log::debug!(
            "layout: {} paras in {:?}",
            self.layout.paras.len(),
            t.elapsed()
        );
    }

    /// Location of the caret (focus) in the layout.
    pub fn caret_loc(&self, ed: &Editor, rules: &dyn Rules) -> Option<Loc> {
        if let Some((p, loc)) = self.caret_hint {
            if p == ed.sel.focus && loc.para < self.layout.paras.len() {
                return Some(loc);
            }
        }
        self.layout.locate(&ed.doc, rules, ed.sel.focus)
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        ed: &mut Editor,
        cx: &mut CanvasCtx,
        actions: &mut Vec<CanvasAction>,
    ) {
        let width = ui.available_width().max(200.0);
        // Content the caret moved into must be visible.
        if !self.collapsed.is_empty() && ed.doc.contains(ed.sel.focus.node) {
            let hidden: Vec<NodeId> = ed.doc.ancestors(ed.sel.focus.node).filter(|a| self.collapsed.contains(a)).collect();
            for a in hidden {
                if a != ed.sel.focus.node {
                    self.set_collapsed(a, false);
                }
            }
        }
        self.ensure_layout(ui, ed, cx, width);
        let mut scroll = egui::ScrollArea::both()
            .auto_shrink([false, false])
            .id_salt(("doc-canvas", cx.doc_key));
        if let Some(y) = self.scroll_to_y.take() {
            scroll = scroll.vertical_scroll_offset((y - 60.0).max(0.0));
        }
        scroll.show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(
                self.layout.size.max(vec2(width, ui.available_height())),
                Sense::click_and_drag(),
            );
            let origin = rect.min;
            if self.request_focus {
                resp.request_focus();
                self.request_focus = false;
            }
            if resp.clicked() || resp.drag_started() {
                resp.request_focus();
            }
            self.has_focus = resp.has_focus();
            if resp.hovered() {
                ui.ctx().set_cursor_icon(CursorIcon::Text);
            }
            ui.memory_mut(|m| {
                m.set_focus_lock_filter(
                    resp.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: false,
                    },
                )
            });

            self.handle_pointer(ui, &resp, origin, ed, cx, actions);
            if resp.has_focus() {
                let rev = ed.doc.revision();
                self.handle_keys(ui, ed, cx, actions);
                if ed.doc.revision() != rev {
                    // Styles are recomputed next frame; repaint immediately.
                    ui.ctx().request_repaint();
                }
            }
            self.paint(ui, origin, ed, cx);

            if self.scroll_to_caret {
                self.scroll_to_caret = false;
                if let Some(loc) = self.caret_loc(ed, cx.rules) {
                    let r = self
                        .layout
                        .caret_rect(loc)
                        .translate(origin.to_vec2())
                        .expand(20.0);
                    ui.scroll_to_rect(r, None);
                }
            }
            // IME cursor area.
            if resp.has_focus()
                && let Some(loc) = self.caret_loc(ed, cx.rules)
            {
                let r = self.layout.caret_rect(loc).translate(origin.to_vec2());
                ui.ctx().output_mut(|o| {
                    o.ime = Some(egui::output::IMEOutput {
                        rect: resp.rect,
                        cursor_rect: r,
                    });
                });
            }
        });
    }

    fn pos_at_point(&self, p: Pos2) -> Option<(Loc, Pos)> {
        let loc = self.layout.hit(p)?;
        let pos = self.layout.position_at(loc)?;
        Some((loc, pos))
    }

    fn handle_pointer(
        &mut self,
        ui: &Ui,
        resp: &egui::Response,
        origin: Pos2,
        ed: &mut Editor,
        cx: &mut CanvasCtx,
        actions: &mut Vec<CanvasAction>,
    ) {
        let Some(pointer) = resp.interact_pointer_pos() else {
            if !ui.input(|i| i.pointer.any_down()) {
                self.drag_anchor = None;
            }
            return;
        };
        let local = pointer - origin.to_vec2();
        let shift = ui.input(|i| i.modifiers.shift);
        if resp.secondary_clicked() {
            if let Some((_, p)) = self.pos_at_point(local) {
                let inside = !ed.sel.is_caret() && {
                    let (a, b) = ed.sel.ordered(&ed.doc);
                    cmp_pos(&ed.doc, a, p).is_le() && cmp_pos(&ed.doc, p, b).is_le()
                };
                if !inside {
                    ed.set_caret(p);
                }
                actions.push(CanvasAction::ContextMenu {
                    node: container(&ed.doc, ed.caret()),
                });
            }
            return;
        }
        if resp.drag_started() || resp.clicked() || (resp.dragged() && self.drag_anchor.is_some()) {
            let Some((loc, p)) = self.pos_at_point(local) else {
                return;
            };
            let now = ui.input(|i| i.time);
            if resp.drag_started() || resp.clicked() {
                let count = match self.last_click {
                    Some((t, at, n)) if now - t < 0.45 && at.distance(local) < 6.0 => n + 1,
                    _ => 1,
                };
                if resp.clicked() || resp.drag_started() {
                    self.last_click = Some((now, local, count));
                }
                // Clicking a tag chip selects the element.
                if let Some(seg) = self.layout.seg_at(loc) {
                    if let SegKind::StartTag(el) | SegKind::EndTag(el) | SegKind::EmptyTag(el) =
                        seg.kind
                    {
                        let r = self.layout.tag_rect(loc.para, seg);
                        if r.expand(1.0).contains(local) && resp.clicked() {
                            if count >= 2 {
                                actions.push(CanvasAction::EditAttributes(el));
                            } else {
                                ed.select_element(el);
                            }
                            self.goal_x = None;
                            actions.push(CanvasAction::Changed);
                            return;
                        }
                    }
                    if let SegKind::Generated(el) = seg.kind
                        && self.collapsed.contains(&el)
                        && resp.clicked()
                    {
                        self.set_collapsed(el, false);
                        actions.push(CanvasAction::Changed);
                        return;
                    }
                    if let SegKind::Atom(n) = seg.kind
                        && resp.clicked()
                        && count >= 2
                    {
                        actions.push(CanvasAction::EditNode(n));
                        return;
                    }
                }
                if resp.clicked() && ui.input(|i| i.modifiers.command) {
                    actions.push(CanvasAction::OpenLink(container(&ed.doc, p)));
                }
                match count {
                    2 => self.select_word(ed, cx.rules, p),
                    n if n >= 3 => {
                        let c = container(&ed.doc, p);
                        ed.select_contents(cx.rules, c);
                    }
                    _ => {
                        if shift {
                            ed.sel.focus = p;
                        } else {
                            ed.set_caret(p);
                        }
                        self.caret_hint = Some((p, loc));
                        self.drag_anchor = Some(ed.sel.anchor);
                    }
                }
                self.goal_x = None;
                actions.push(CanvasAction::Changed);
            } else if let Some(anchor) = self.drag_anchor {
                ed.sel = Selection { anchor, focus: p };
                actions.push(CanvasAction::Changed);
                // Auto-scroll while dragging near the edges.
                let clip = ui.clip_rect();
                if pointer.y > clip.max.y - 10.0 || pointer.y < clip.min.y + 10.0 {
                    self.scroll_to_caret = true;
                }
            }
        }
    }

    fn select_word(&mut self, ed: &mut Editor, rules: &dyn Rules, p: Pos) {
        if let Some(t) = ed.doc.text(p.node) {
            let is_w = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
            let mut s = p.offset.min(t.len());
            while s > 0 {
                let c = t[..s].chars().next_back().unwrap();
                if !is_w(c) {
                    break;
                }
                s -= c.len_utf8();
            }
            let mut e = p.offset.min(t.len());
            while e < t.len() {
                let c = t[e..].chars().next().unwrap();
                if !is_w(c) {
                    break;
                }
                e += c.len_utf8();
            }
            ed.sel = Selection {
                anchor: Pos::new(p.node, s),
                focus: Pos::new(p.node, e),
            };
        } else {
            ed.set_caret(p);
        }
        let _ = rules;
    }

    fn report(actions: &mut Vec<CanvasAction>, r: Result<(), EditError>) {
        match r {
            Ok(()) => actions.push(CanvasAction::Changed),
            Err(EditError::Nothing) => {}
            Err(e) => actions.push(CanvasAction::Message(e.to_string())),
        }
    }

    fn handle_keys(
        &mut self,
        ui: &Ui,
        ed: &mut Editor,
        cx: &mut CanvasCtx,
        actions: &mut Vec<CanvasAction>,
    ) {
        let events = ui.input(|i| i.events.clone());
        let rules = cx.rules;
        let skip_tags = cx.view == ViewMode::Normal;
        for ev in events {
            match ev {
                Event::Text(t) | Event::Ime(egui::ImeEvent::Commit(t)) => {
                    if cx.read_only {
                        continue;
                    }
                    if t.chars().all(|c| !c.is_control()) && !t.is_empty() {
                        let r = ed.insert_text(rules, &t);
                        Self::report(actions, r);
                        self.after_edit();
                    }
                }
                Event::Copy => self.copy(ui, ed, cx),
                Event::Cut => {
                    self.copy(ui, ed, cx);
                    if !cx.read_only {
                        let r = ed.delete_selection(rules);
                        Self::report(actions, r);
                        self.after_edit();
                    }
                }
                Event::Paste(text) => {
                    if cx.read_only {
                        continue;
                    }
                    let r = if !cx.clipboard.xml.is_empty()
                        && normalize_nl(&text) == normalize_nl(&cx.clipboard.text)
                    {
                        ed.paste(rules, &cx.clipboard.xml.clone(), true)
                    } else {
                        let looks_xml = text.trim_start().starts_with('<');
                        ed.paste(rules, &text, looks_xml)
                    };
                    Self::report(actions, r);
                    self.after_edit();
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    self.key(ui, key, modifiers, ed, cx, actions, skip_tags);
                }
                _ => {}
            }
        }
    }

    fn after_edit(&mut self) {
        self.goal_x = None;
        self.scroll_to_caret = true;
    }

    fn copy(&mut self, ui: &Ui, ed: &Editor, cx: &mut CanvasCtx) {
        if let (Some(xml), Some(text)) = (ed.selection_xml(), ed.selection_text()) {
            cx.clipboard.xml = xml;
            cx.clipboard.text = text.clone();
            ui.ctx().copy_text(text);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn key(
        &mut self,
        _ui: &Ui,
        key: Key,
        m: Modifiers,
        ed: &mut Editor,
        cx: &mut CanvasCtx,
        actions: &mut Vec<CanvasAction>,
        skip_tags: bool,
    ) {
        let rules = cx.rules;
        let doc = &ed.doc;
        let moved = |canvas: &mut Self,
                     ed: &mut Editor,
                     p: Pos,
                     keep_goal: bool,
                     actions: &mut Vec<CanvasAction>| {
            if m.shift {
                ed.sel.focus = p;
            } else {
                ed.set_caret(p);
            }
            if !keep_goal {
                canvas.goal_x = None;
            }
            canvas.scroll_to_caret = true;
            actions.push(CanvasAction::Changed);
        };
        match key {
            Key::ArrowLeft | Key::ArrowRight => {
                let fwd = key == Key::ArrowRight;
                let from = if !m.shift && !ed.sel.is_caret() {
                    let (a, b) = ed.sel.ordered(doc);
                    let p = if fwd { b } else { a };
                    moved(self, ed, p, false, actions);
                    return;
                } else {
                    ed.sel.focus
                };
                let p = match (fwd, m.command || m.alt) {
                    (true, true) => pos::next_word(doc, rules, from),
                    (false, true) => pos::prev_word(doc, rules, from),
                    (true, false) => pos::next_pos(doc, rules, from, skip_tags),
                    (false, false) => pos::prev_pos(doc, rules, from, skip_tags),
                };
                moved(self, ed, p, false, actions);
            }
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown => {
                let Some(loc) = self.caret_loc(ed, rules) else {
                    return;
                };
                let r = self.layout.caret_rect(loc);
                let x = *self.goal_x.get_or_insert(r.center().x);
                let dy = match key {
                    Key::ArrowUp => -(r.height() * 0.5 + 3.0),
                    Key::ArrowDown => r.height() * 0.5 + 3.0,
                    Key::PageUp => -500.0,
                    _ => 500.0,
                };
                let y = if dy < 0.0 { r.min.y + dy } else { r.max.y + dy };
                let target = pos2(x, y);
                if let Some(nl) = self.layout.hit(target)
                    && let Some(p) = self.layout.position_at(nl)
                {
                    moved(self, ed, p, true, actions);
                    self.caret_hint = Some((p, nl));
                }
            }
            Key::Home | Key::End => {
                if m.command {
                    let root = doc.document_element();
                    if let Some(root) = root {
                        let p = if key == Key::Home {
                            pos::first_editable(doc, rules, root)
                        } else {
                            pos::end_of(doc, rules, root)
                        };
                        moved(self, ed, p, false, actions);
                    }
                    return;
                }
                let Some(loc) = self.caret_loc(ed, rules) else {
                    return;
                };
                let para = &self.layout.paras[loc.para];
                let mut start = 0;
                for row in &para.galley.rows {
                    let end = start + row.char_count_excluding_newline();
                    if loc.ch >= start && loc.ch <= end {
                        // Stop at the first/last text on the row rather than
                        // beyond leading start-tag or trailing end-tag chips.
                        let is_chrome = |c: usize| {
                            para.segs.iter().any(|sg| {
                                sg.chars.contains(&c) && matches!(sg.kind, SegKind::StartTag(_) | SegKind::EndTag(_) | SegKind::EmptyTag(_) | SegKind::Generated(_))
                            })
                        };
                        let ch = if key == Key::Home {
                            let mut c = start;
                            while c < end && is_chrome(c) {
                                c += 1;
                            }
                            if c == end { start } else { c }
                        } else {
                            let mut c = end;
                            while c > start && is_chrome(c - 1) {
                                c -= 1;
                            }
                            if c == start { end } else { c }
                        };
                        let target = Loc { para: loc.para, ch };
                        if let Some(p) = self.layout.position_at(target) {
                            moved(self, ed, p, false, actions);
                            self.caret_hint = Some((p, target));
                        }
                        return;
                    }
                    start += row.char_count_including_newline();
                }
            }
            Key::A if m.command => {
                ed.select_all(rules);
                actions.push(CanvasAction::Changed);
            }
            _ if cx.read_only => {}
            Key::Backspace => {
                let r = if m.command && ed.sel.is_caret() {
                    let p = pos::prev_word(doc, rules, ed.sel.focus);
                    ed.sel.anchor = p;
                    std::mem::swap(&mut ed.sel.anchor, &mut ed.sel.focus);
                    ed.delete_selection(rules)
                } else {
                    ed.delete_backward(rules)
                };
                Self::report(actions, r);
                self.after_edit();
            }
            Key::Delete => {
                let r = if m.command && ed.sel.is_caret() {
                    let p = pos::next_word(doc, rules, ed.sel.focus);
                    ed.sel.focus = p;
                    ed.delete_selection(rules)
                } else {
                    ed.delete_forward(rules)
                };
                Self::report(actions, r);
                self.after_edit();
            }
            Key::Enter => {
                let pre = cx.styles.get(container(doc, ed.caret())).white_space;
                let r = if m.shift
                    || matches!(
                        pre,
                        super::style::WhiteSpace::Pre | super::style::WhiteSpace::PreWrap
                    ) {
                    ed.insert_text(rules, "\n")
                } else {
                    ed.enter(rules)
                };
                Self::report(actions, r);
                self.after_edit();
            }
            Key::Tab => {
                if ed.table_next_cell(rules, !m.shift) {
                    actions.push(CanvasAction::Changed);
                    self.after_edit();
                }
            }
            Key::Z if m.command => {
                if if m.shift { ed.redo() } else { ed.undo() } {
                    actions.push(CanvasAction::Changed);
                    self.after_edit();
                }
            }
            Key::Y if m.command && ed.redo() => {
                actions.push(CanvasAction::Changed);
                self.after_edit();
            }
            _ => {}
        }
    }

    fn paint(&mut self, ui: &mut Ui, origin: Pos2, ed: &Editor, cx: &CanvasCtx) {
        let painter = ui.painter().clone();
        let clip = ui.clip_rect().translate(-origin.to_vec2()).expand(50.0);
        let off = origin.to_vec2();
        let dark = ui.visuals().dark_mode;
        // Background decorations.
        for d in &self.layout.decor {
            match d {
                Decor::Fill(r, c) if r.intersects(clip) => {
                    painter.rect_filled(r.translate(off), 0.0, *c);
                }
                Decor::Border {
                    rect,
                    widths,
                    colors,
                } if rect.intersects(clip) => {
                    let r = rect.translate(off);
                    let lines = [
                        (r.left_top(), r.right_top(), widths[0], colors[0]),
                        (r.right_top(), r.right_bottom(), widths[1], colors[1]),
                        (r.left_bottom(), r.right_bottom(), widths[2], colors[2]),
                        (r.left_top(), r.left_bottom(), widths[3], colors[3]),
                    ];
                    for (a, b, w, c) in lines {
                        if w > 0.0 {
                            painter.line_segment([a, b], Stroke::new(w, c));
                        }
                    }
                }
                Decor::Marker {
                    pos,
                    text,
                    font,
                    color,
                } if clip.contains(*pos) => {
                    painter.text(
                        *pos + off,
                        egui::Align2::RIGHT_CENTER,
                        text,
                        font.clone(),
                        *color,
                    );
                }
                Decor::Grid(r, c) if r.intersects(clip) => {
                    painter.rect_stroke(
                        r.translate(off),
                        0.0,
                        Stroke::new(1.0_f32, *c),
                        egui::StrokeKind::Inside,
                    );
                }
                _ => {}
            }
        }
        // Selection.
        let sel_color = if dark {
            Color32::from_rgba_unmultiplied(60, 110, 200, 110)
        } else {
            Color32::from_rgba_unmultiplied(120, 170, 255, 110)
        };
        if !ed.sel.is_caret() {
            let (a, b) = ed.sel.ordered(&ed.doc);
            if let (Some(la), Some(lb)) = (
                self.layout.locate(&ed.doc, cx.rules, a),
                self.layout.locate(&ed.doc, cx.rules, b),
            ) {
                for pi in la.para..=lb.para {
                    let p = &self.layout.paras[pi];
                    if !p.rect.intersects(clip) {
                        continue;
                    }
                    let s = if pi == la.para { la.ch } else { 0 };
                    let e = if pi == lb.para { lb.ch } else { usize::MAX / 2 };
                    let e = e.min(p.galley.job.text.chars().count() + 1);
                    for r in self.layout.range_rects(pi, s, e) {
                        painter.rect_filled(r.translate(off), 0.0, sel_color);
                    }
                }
            }
        }
        // Find / spelling highlights.
        let hl = Color32::from_rgba_unmultiplied(255, 210, 0, 120);
        for (node, range) in &self.highlights {
            self.paint_text_range(&painter, off, ed, cx, *node, range.clone(), |r, p| {
                p.rect_filled(r, 1.0, hl);
            });
        }
        for (node, range) in &self.misspelled {
            self.paint_text_range(&painter, off, ed, cx, *node, range.clone(), |r, p| {
                squiggle(p, r, Color32::from_rgb(220, 40, 40));
            });
        }
        // Tag chips, then text.
        for (pi, p) in self.layout.paras.iter().enumerate() {
            if !p.rect.intersects(clip) {
                continue;
            }
            for seg in p.segs.iter() {
                let (el, kind) = match seg.kind {
                    SegKind::StartTag(e) => (e, 0),
                    SegKind::EndTag(e) => (e, 1),
                    SegKind::EmptyTag(e) => (e, 2),
                    _ => continue,
                };
                let name = ed.doc.name(el).unwrap_or("");
                let fill = tag_color(name, cx.tag_colors, dark);
                let flagged = self.flagged.contains(&el);
                for r in self.layout.range_rects(pi, seg.chars.start, seg.chars.end) {
                    let r = r.translate(off).shrink2(vec2(0.5, 1.5));
                    chip(&painter, r, kind, fill, flagged, dark);
                }
            }
            painter.galley(p.anchor + off, p.galley.clone(), Color32::GRAY);
            for (range, path) in p.icons.iter() {
                let Some(r) = self.layout.range_rects(pi, range.start, range.end).into_iter().next() else { continue };
                let tex = self.icons.entry(path.clone()).or_insert_with(|| load_icon(ui.ctx(), path)).clone();
                if let Some(tex) = tex {
                    // Fit the image into the reserved box, keeping its aspect.
                    let r = r.translate(off).shrink(1.0);
                    let [w, h] = tex.size().map(|v| v as f32);
                    let scale = (r.width() / w).min(r.height() / h);
                    let dst = Rect::from_center_size(r.center(), vec2(w * scale, h * scale));
                    painter.image(tex.id(), dst, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
        }
        // Images.
        for img in &self.layout.images {
            if !img.rect.intersects(clip) {
                continue;
            }
            let r = img.rect.translate(off);
            match (cx.image_texture)(ui.ctx(), img.node) {
                Some(tex) => {
                    painter.image(
                        tex.id(),
                        r,
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    painter.rect_stroke(
                        r,
                        2.0,
                        Stroke::new(1.0_f32, Color32::GRAY),
                        egui::StrokeKind::Inside,
                    );
                    let name = img.src.rsplit(['/', '\\']).next().unwrap_or(&img.src);
                    painter.text(
                        r.center(),
                        egui::Align2::CENTER_CENTER,
                        format!("🖼 {name}"),
                        egui::FontId::proportional(13.0),
                        Color32::GRAY,
                    );
                }
            }
        }
        // Caret.
        self.caret_screen = self.caret_loc(ed, cx.rules).map(|loc| self.layout.caret_rect(loc).translate(off));
        if self.has_focus
            && let Some(loc) = self.caret_loc(ed, cx.rules)
        {
            let r = self.layout.caret_rect(loc).translate(off);
            let t = ui.input(|i| i.time);
            let visible = (t * 1.6).fract() < 0.6 || ui.input(|i| i.time_since_last_scroll() < 0.5);
            if visible {
                let c = if dark { Color32::WHITE } else { Color32::BLACK };
                painter.line_segment([r.center_top(), r.center_bottom()], Stroke::new(1.6_f32, c));
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    fn paint_text_range(
        &self,
        painter: &egui::Painter,
        off: egui::Vec2,
        ed: &Editor,
        cx: &CanvasCtx,
        node: NodeId,
        range: std::ops::Range<usize>,
        f: impl Fn(Rect, &egui::Painter),
    ) {
        if !ed.doc.contains(node) || !ed.doc.is_text(node) {
            return;
        }
        let (Some(a), Some(b)) = (
            self.layout
                .locate(&ed.doc, cx.rules, Pos::new(node, range.start)),
            self.layout
                .locate(&ed.doc, cx.rules, Pos::new(node, range.end)),
        ) else {
            return;
        };
        if a.para != b.para {
            return;
        }
        for r in self.layout.range_rects(a.para, a.ch, b.ch) {
            f(r.translate(off), painter);
        }
    }

    /// Scrolls so that `node` is visible.
    pub fn reveal(&mut self, node: NodeId) {
        if let Some(&(p, _)) = self.layout.element_paras.get(&node) {
            if let Some(para) = self.layout.paras.get(p) {
                self.scroll_to_y = Some(para.rect.min.y);
            }
        } else {
            self.scroll_to_caret = true;
        }
    }
}

fn normalize_nl(s: &str) -> String {
    s.replace("\r\n", "\n")
}

/// Draws a tag chip: start tags point right, end tags point left.
fn chip(p: &egui::Painter, r: Rect, kind: u8, fill: Color32, flagged: bool, dark: bool) {
    let stroke = Stroke::new(
        if flagged { 1.5 } else { 0.8_f32 },
        if flagged {
            Color32::from_rgb(220, 50, 50)
        } else if dark {
            Color32::from_gray(140)
        } else {
            Color32::from_gray(120)
        },
    );
    let tip = (r.height() * 0.4).min(7.0);
    let pts = match kind {
        0 => vec![
            r.left_top(),
            pos2(r.right() - tip, r.top()),
            r.right_center(),
            pos2(r.right() - tip, r.bottom()),
            r.left_bottom(),
        ],
        1 => vec![
            r.left_center(),
            pos2(r.left() + tip, r.top()),
            r.right_top(),
            r.right_bottom(),
            pos2(r.left() + tip, r.bottom()),
        ],
        _ => {
            p.rect(r, 3.0, fill, stroke, egui::StrokeKind::Inside);
            return;
        }
    };
    p.add(Shape::convex_polygon(pts, fill, stroke));
}

fn squiggle(p: &egui::Painter, r: Rect, c: Color32) {
    let y = r.bottom() - 1.0;
    let mut pts = Vec::new();
    let mut x = r.left();
    let mut up = true;
    while x <= r.right() {
        pts.push(pos2(x, if up { y - 1.5 } else { y + 0.5 }));
        x += 2.5;
        up = !up;
    }
    if pts.len() > 1 {
        p.add(Shape::line(pts, Stroke::new(1.0_f32, c)));
    }
}

fn load_icon(ctx: &egui::Context, path: &std::path::Path) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(path.display().to_string(), ci, egui::TextureOptions::LINEAR))
}
