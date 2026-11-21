//! Box layout for the styled document view.
//!
//! The document is laid out into absolutely positioned paragraphs. Each
//! paragraph is one egui [`Galley`] built from a [`LayoutJob`] whose sections
//! are text runs, tag chips and generated content. A per-paragraph segment
//! table maps galley character indices back to DOM positions, which gives hit
//! testing, caret placement and selection painting for free.

use egui::text::{CCursor, LayoutJob, TextFormat};
use egui::{Align, Color32, FontFamily, FontId, Galley, Pos2, Rect, Stroke, Vec2};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use xylem_dom::{Document, NodeId, NodeKind};
use xylem_edit::pos::{canonical, is_ignorable};
use xylem_edit::{Pos, Rules};

use super::style::{
    Display, Sty, StyleLookup, TextAlign, TextTransform, VerticalAlign, WhiteSpace,
};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ViewMode {
    TagsOn,
    Normal,
}

#[derive(Clone, Debug)]
pub enum SegKind {
    /// Characters from a text node; `bytes[i]` is the source byte offset of
    /// displayed char `i` (one extra entry for the end).
    Text {
        node: NodeId,
        bytes: Vec<usize>,
    },
    StartTag(NodeId),
    EndTag(NodeId),
    EmptyTag(NodeId),
    /// Generated content / list markers / atom labels (not editable).
    #[allow(dead_code)] // owning element, kept for hit-test diagnostics
    Generated(NodeId),
    /// Comment, PI, CDATA or entity reference shown as one unit.
    Atom(NodeId),
}

#[derive(Clone, Debug)]
pub struct Seg {
    pub chars: Range<usize>,
    pub kind: SegKind,
}

pub struct Para {
    /// Bounding box in layout coordinates.
    pub rect: Rect,
    /// Galley origin (differs from `rect.min` for centred/right text).
    pub anchor: Pos2,
    pub galley: Arc<Galley>,
    /// Key of the galley in [`LayoutCache`], so reused paragraphs keep it alive.
    galley_key: u64,
    pub segs: Arc<[Seg]>,
    /// Caret stops between segments: (char index, position).
    pub stops: Arc<[(usize, Pos)]>,
    /// Images from CSS generated content, drawn over placeholder characters.
    pub icons: Arc<[(Range<usize>, std::path::PathBuf)]>,
    #[allow(dead_code)]
    pub owner: NodeId,
}

impl Para {
    fn shifted(&self, dy: f32) -> Para {
        Para {
            rect: self.rect.translate(Vec2::new(0.0, dy)),
            anchor: self.anchor + Vec2::new(0.0, dy),
            galley: self.galley.clone(),
            galley_key: self.galley_key,
            segs: self.segs.clone(),
            stops: self.stops.clone(),
            icons: self.icons.clone(),
            owner: self.owner,
        }
    }
}

#[derive(Clone)]
pub enum Decor {
    Fill(Rect, Color32),
    /// List marker text painted in the left margin, right-aligned at `pos`.
    Marker {
        pos: Pos2,
        text: String,
        font: FontId,
        color: Color32,
    },
    Border {
        rect: Rect,
        widths: [f32; 4],
        colors: [Color32; 4],
    },
    Grid(Rect, Color32),
}

#[derive(Clone)]
pub struct ImageBox {
    pub rect: Rect,
    pub node: NodeId,
    pub src: String,
}

#[derive(Default)]
pub struct DocLayout {
    pub paras: Vec<Para>,
    pub decor: Vec<Decor>,
    pub images: Vec<ImageBox>,
    pub size: Vec2,
    /// For each node (by `NodeId::index`), the first paragraph that shows
    /// its text or has a caret stop inside it; `u32::MAX` if none. A flat
    /// array is much cheaper to rebuild than a hash map on every edit.
    first_para: Vec<u32>,
    /// Element -> (para of start tag/first content, para of end).
    pub element_paras: HashMap<NodeId, (usize, usize)>,
}

/// A located caret position in the layout.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Loc {
    pub para: usize,
    pub ch: usize,
}

impl Decor {
    fn shifted(&self, dy: f32) -> Decor {
        let d = Vec2::new(0.0, dy);
        match self {
            Decor::Fill(r, c) => Decor::Fill(r.translate(d), *c),
            Decor::Marker { pos, text, font, color } => Decor::Marker {
                pos: *pos + d,
                text: text.clone(),
                font: font.clone(),
                color: *color,
            },
            Decor::Border { rect, widths, colors } => Decor::Border {
                rect: rect.translate(d),
                widths: *widths,
                colors: *colors,
            },
            Decor::Grid(r, c) => Decor::Grid(r.translate(d), *c),
        }
    }
}

impl DocLayout {
    /// Maps a DOM position to a (para, char) location.
    pub fn locate(&self, doc: &Document, rules: &dyn Rules, p: Pos) -> Option<Loc> {
        let p = if doc.contains(p.node) {
            canonical(doc, rules, p)
        } else {
            p
        };
        let first = *self.first_para.get(p.node.index())? as usize;
        if first == u32::MAX as usize {
            return self.element_paras.get(&p.node).map(|&(a, _)| Loc { para: a, ch: 0 });
        }
        if doc.is_text(p.node) {
            // A text node's characters sit in one paragraph, or a few
            // consecutive ones when a block interrupts its inline parent.
            let mut best = None;
            for (pi, para) in self.paras.iter().enumerate().skip(first) {
                let mut seen = false;
                for seg in para.segs.iter() {
                    let SegKind::Text { node, bytes } = &seg.kind else { continue };
                    if *node != p.node {
                        continue;
                    }
                    seen = true;
                    let (lo, hi) = (bytes[0], *bytes.last().unwrap());
                    if p.offset >= lo && p.offset <= hi {
                        // Position of the first displayed char at or after offset.
                        let i = bytes.partition_point(|&b| b < p.offset);
                        return Some(Loc { para: pi, ch: seg.chars.start + i.min(seg.chars.len()) });
                    }
                    if p.offset < lo && best.is_none() {
                        best = Some(Loc { para: pi, ch: seg.chars.start });
                    }
                    if p.offset > hi {
                        best = Some(Loc { para: pi, ch: seg.chars.end });
                    }
                }
                let stop_here = para.stops.iter().any(|(_, s)| s.node == p.node);
                if !seen && !stop_here && pi > first {
                    break;
                }
                if !seen && stop_here && best.is_none() {
                    // Collapsed (all-whitespace) text keeps only a stop.
                    if let Some(&(ch, _)) = para.stops.iter().find(|(_, s)| s.node == p.node) {
                        best = Some(Loc { para: pi, ch });
                    }
                }
            }
            return best;
        }
        // Element gap: the exact stop, else the nearest stop in the element.
        let last = self.element_paras.get(&p.node).map_or(self.paras.len() - 1, |&(_, b)| b);
        let mut nearest: Option<(usize, Loc)> = None;
        for (pi, para) in self.paras.iter().enumerate().take(last + 1).skip(first) {
            for &(ch, s) in para.stops.iter() {
                if s.node != p.node {
                    continue;
                }
                if s.offset == p.offset {
                    return Some(Loc { para: pi, ch });
                }
                let d = s.offset.abs_diff(p.offset);
                if nearest.is_none_or(|(bd, _)| d < bd) {
                    nearest = Some((d, Loc { para: pi, ch }));
                }
            }
        }
        if let Some((_, loc)) = nearest {
            return Some(loc);
        }
        self.element_paras
            .get(&p.node)
            .map(|&(a, _)| Loc { para: a, ch: 0 })
    }

    /// Maps a (para, char) location to the best DOM position.
    pub fn position_at(&self, loc: Loc) -> Option<Pos> {
        let para = self.paras.get(loc.para)?;
        let c = loc.ch;
        // Inside or at the edge of a text segment.
        let mut text_hit = None;
        for seg in para.segs.iter() {
            if let SegKind::Text { node, bytes } = &seg.kind {
                if c > seg.chars.start && c < seg.chars.end {
                    return Some(Pos::new(*node, bytes[c - seg.chars.start]));
                }
                if c == seg.chars.start || c == seg.chars.end {
                    text_hit.get_or_insert(Pos::new(*node, bytes[c - seg.chars.start]));
                }
            }
        }
        if let Some(&(_, p)) = para.stops.iter().find(|(ch, _)| *ch == c) {
            // Prefer text positions at the same spot.
            return Some(text_hit.unwrap_or(p));
        }
        if let Some(t) = text_hit {
            return Some(t);
        }
        // Inside an atom/generated/tag segment: snap to the nearer stop.
        let mut best: Option<(usize, Pos)> = None;
        for &(ch, p) in para.stops.iter() {
            let d = ch.abs_diff(c);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, p));
            }
        }
        best.map(|(_, p)| p)
    }

    /// Hit test in layout coordinates.
    pub fn hit(&self, pt: Pos2) -> Option<Loc> {
        // Nearest paragraph vertically (and containing horizontally if several).
        let mut best: Option<(f32, usize)> = None;
        for (i, p) in self.paras.iter().enumerate() {
            let r = p.rect;
            let dy = if pt.y < r.min.y {
                r.min.y - pt.y
            } else if pt.y > r.max.y {
                pt.y - r.max.y
            } else {
                0.0
            };
            let dx = if pt.x < r.min.x {
                r.min.x - pt.x
            } else if pt.x > r.max.x {
                pt.x - r.max.x
            } else {
                0.0
            };
            let d = dy * 4.0 + dx;
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        let (_, i) = best?;
        let p = &self.paras[i];
        let cur = p.galley.cursor_from_pos(pt - p.anchor);
        Some(Loc {
            para: i,
            ch: cur.index,
        })
    }

    pub fn caret_rect(&self, loc: Loc) -> Rect {
        let p = &self.paras[loc.para];
        p.galley
            .pos_from_cursor(CCursor::new(loc.ch))
            .translate(p.anchor.to_vec2())
    }

    /// Rectangles covering chars [a, b) of a paragraph.
    pub fn range_rects(&self, para: usize, a: usize, b: usize) -> Vec<Rect> {
        let p = &self.paras[para];
        let mut out = Vec::new();
        let mut start = 0;
        for row in &p.galley.rows {
            let n = row.char_count_including_newline();
            let (rs, re) = (start, start + row.char_count_excluding_newline());
            let s = a.max(rs);
            let e = b.min(re);
            if s < e || (s == e && a <= rs && b > re) {
                let x0 = row.pos.x + row.x_offset(s - rs);
                let x1 = row.pos.x + row.x_offset(e - rs);
                let x1 = if b > re && x1 - x0 < 4.0 {
                    x0 + 4.0
                } else {
                    x1
                };
                out.push(
                    Rect::from_min_max(Pos2::new(x0, row.min_y()), Pos2::new(x1, row.max_y()))
                        .translate(p.anchor.to_vec2()),
                );
            }
            start += n;
        }
        out
    }

    /// Element that a tag chip at this location belongs to.
    pub fn seg_at(&self, loc: Loc) -> Option<&Seg> {
        self.paras
            .get(loc.para)?
            .segs
            .iter()
            .find(|s| s.chars.contains(&loc.ch))
    }

    pub fn tag_rect(&self, para: usize, seg: &Seg) -> Rect {
        let rs = self.range_rects(para, seg.chars.start, seg.chars.end);
        rs.into_iter()
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::NOTHING)
    }
}

/// Fonts available for documents.
#[derive(Clone, Default)]
pub struct FontSet {
    pub has_bold: bool,
    pub has_italic: bool,
    pub has_serif: bool,
}

pub struct LayoutParams<'a> {
    pub view: ViewMode,
    pub width: f32,
    pub zoom: f32,
    pub fonts: &'a FontSet,
    #[allow(dead_code)] // chips are painted by the canvas, which reads them directly
    pub tag_colors: &'a HashMap<String, Color32>,
    pub dark: bool,
    pub image_size: &'a dyn Fn(NodeId) -> Option<(String, Vec2)>,
    /// Elements shown as a single collapsed line.
    pub collapsed: &'a HashSet<NodeId>,
}

/// What the layout keeps between runs: shaped galleys (egui only keeps
/// galleys that are used every frame) and the output of whole blocks, so that
/// typing in one paragraph doesn't re-lay-out the rest of the document.
#[derive(Default)]
pub struct LayoutCache {
    galleys: HashMap<u64, Arc<Galley>>,
    used: HashSet<u64>,
    blocks: HashMap<NodeId, CachedBlock>,
    /// Hash of the settings every cached block depends on (view, zoom, ...).
    context: u64,
    pub stats: LayoutStats,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct LayoutStats {
    pub galley_hits: u32,
    pub galley_misses: u32,
    pub blocks_reused: u32,
    pub blocks_laid_out: u32,
}

/// The laid-out output of one block element. It stays valid while the
/// element's subtree, its styles and its box (x, width) are unchanged; the
/// vertical position and the element's index among its siblings may move.
struct CachedBlock {
    stamp: u64,
    style: u64,
    x: f32,
    width: f32,
    /// The block's index in its parent when it was laid out.
    index: u32,
    y0: f32,
    y1: f32,
    paras: Vec<Para>,
    decor: Vec<Decor>,
    images: Vec<ImageBox>,
    /// `element_paras` entries for the block and its descendants, with
    /// paragraph indices relative to the block's first paragraph.
    elements: Vec<(NodeId, (usize, usize))>,
}

impl LayoutCache {
    fn galley(&mut self, ctx: &egui::Context, job: LayoutJob) -> (Arc<Galley>, u64) {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        job.hash(&mut h);
        let k = h.finish();
        self.used.insert(k);
        if let Some(g) = self.galleys.get(&k) {
            self.stats.galley_hits += 1;
            return (g.clone(), k);
        }
        self.stats.galley_misses += 1;
        let g = ctx.fonts_mut(|f| f.layout_job(job));
        self.galleys.insert(k, g.clone());
        (g, k)
    }

    fn sweep(&mut self, doc: &Document) {
        let used = std::mem::take(&mut self.used);
        self.galleys.retain(|k, _| used.contains(k));
        // Forget blocks that are no longer part of the document.
        let attached = |n: NodeId| {
            doc.contains(n) && (n == doc.root() || doc.ancestors(n).any(|a| a == doc.root()))
        };
        self.blocks.retain(|&n, _| attached(n));
    }
}

pub fn tag_color(name: &str, overrides: &HashMap<String, Color32>, dark: bool) -> Color32 {
    if let Some(c) = overrides.get(name) {
        return *c;
    }
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32 / 360.0;
    let (s, l) = if dark { (0.35, 0.32) } else { (0.45, 0.86) };
    let c = egui::ecolor::Hsva::from(egui::ecolor::HsvaGamma {
        h: hue,
        s,
        v: l,
        a: 1.0,
    });
    Color32::from(c)
}

struct ParaBuilder {
    icons: Vec<(Range<usize>, std::path::PathBuf)>,
    owner: NodeId,
    job: LayoutJob,
    segs: Vec<Seg>,
    stops: Vec<(usize, Pos)>,
    chars: usize,
    last_space: bool,
    has_content: bool,
    align: TextAlign,
    indent: f32,
}

impl ParaBuilder {
    fn new(owner: NodeId, align: TextAlign, indent: f32) -> Self {
        Self {
            owner,
            job: LayoutJob::default(),
            segs: Vec::new(),
            icons: Vec::new(),
            stops: Vec::new(),
            chars: 0,
            last_space: true,
            has_content: false,
            align,
            indent,
        }
    }

    fn push(&mut self, text: &str, fmt: TextFormat, kind: SegKind) {
        let n = text.chars().count();
        if n == 0 {
            return;
        }
        self.job.append(text, 0.0, fmt);
        self.segs.push(Seg {
            chars: self.chars..self.chars + n,
            kind,
        });
        self.chars += n;
    }

    fn stop(&mut self, p: Pos) {
        self.stops.push((self.chars, p));
    }
}

pub struct Layouter<'a> {
    ctx: &'a egui::Context,
    doc: &'a Document,
    styles: &'a dyn StyleLookup,
    rules: &'a dyn Rules,
    params: LayoutParams<'a>,
    cache: &'a mut LayoutCache,
    out: DocLayout,
    cur: Option<ParaBuilder>,
    /// `index_in_parent` for every node (avoids O(siblings) scans).
    index: Vec<u32>,
    x: f32,
    width: f32,
    y: f32,
}

impl<'a> Layouter<'a> {
    pub fn run(
        ctx: &'a egui::Context,
        doc: &'a Document,
        styles: &'a dyn StyleLookup,
        rules: &'a dyn Rules,
        params: LayoutParams<'a>,
        cache: &'a mut LayoutCache,
    ) -> DocLayout {
        let width = params.width;
        let context = {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (params.view, params.zoom.to_bits(), params.dark).hash(&mut h);
            let mut collapsed: Vec<NodeId> = params.collapsed.iter().copied().collect();
            collapsed.sort();
            collapsed.hash(&mut h);
            h.finish()
        };
        if cache.context != context {
            cache.blocks.clear();
            cache.context = context;
        }
        cache.stats = LayoutStats::default();
        let mut index = vec![0u32; doc.node_count()];
        for n in doc.descendants(doc.root()) {
            for (i, &c) in doc.children(n).iter().enumerate() {
                index[c.index()] = i as u32;
            }
        }
        let mut l = Layouter {
            index,
            ctx,
            doc,
            styles,
            rules,
            params,
            cache,
            out: DocLayout::default(),
            cur: None,
            x: 8.0,
            width: width - 16.0,
            y: 8.0,
        };
        let root = doc.root();
        // Document-level nodes: comments/PIs before the root are shown in tags-on view.
        for &c in doc.children(root) {
            match doc.kind(c) {
                NodeKind::Element(_) => l.block(c),
                NodeKind::Comment(_) | NodeKind::Pi { .. } if l.params.view == ViewMode::TagsOn => {
                    l.begin_para(c, TextAlign::Left, 0.0);
                    l.atom(c);
                    l.flush(true);
                }
                _ => {}
            }
        }
        l.flush(false);
        log::debug!("layout cache: {:?}", l.cache.stats);
        l.cache.sweep(doc);
        l.out.size = Vec2::new(width, l.y + 40.0);
        l.out
    }

    fn z(&self, v: f32) -> f32 {
        v * self.params.zoom
    }

    fn tags(&self) -> bool {
        self.params.view == ViewMode::TagsOn
    }

    fn font(&self, s: &Sty) -> FontId {
        let base = if s.monospace {
            "mono"
        } else if s.serif && self.params.fonts.has_serif {
            "serif"
        } else {
            "sans"
        };
        let b = s.bold && self.params.fonts.has_bold;
        let i = s.italic && self.params.fonts.has_italic;
        let name = match (b, i) {
            (true, true) => format!("{base}-bi"),
            (true, false) => format!("{base}-b"),
            (false, true) => format!("{base}-i"),
            _ => base.to_string(),
        };
        let mut size = self.z(s.font_size);
        if s.vertical_align != VerticalAlign::Baseline {
            size *= 0.75;
        }
        FontId::new(size.max(4.0), FontFamily::Name(name.into()))
    }

    fn fmt(&self, s: &Sty) -> TextFormat {
        let default_color = if self.params.dark {
            Color32::from_gray(225)
        } else {
            Color32::from_gray(20)
        };
        let color = s
            .color
            .map(|c| adjust_for_dark(c, self.params.dark))
            .unwrap_or(default_color);
        let mut f = TextFormat {
            font_id: self.font(s),
            color,
            italics: s.italic && !self.params.fonts.has_italic,
            line_height: s.line_height.map(|l| self.z(l)),
            ..Default::default()
        };
        if let Some(bg) = s.background_inline {
            f.background = adjust_bg_for_dark(bg, self.params.dark);
        }
        if s.underline {
            f.underline = Stroke::new(1.0_f32, color);
        }
        if s.line_through {
            f.strikethrough = Stroke::new(1.0_f32, color);
        }
        f.valign = match s.vertical_align {
            VerticalAlign::Super => Align::TOP,
            VerticalAlign::Sub => Align::BOTTOM,
            VerticalAlign::Baseline => Align::BOTTOM,
        };
        f
    }

    fn tag_fmt(&self, name: &str) -> TextFormat {
        let _ = name;
        TextFormat {
            font_id: FontId::new(self.z(10.5), FontFamily::Name("sans".into())),
            color: if self.params.dark {
                Color32::from_gray(235)
            } else {
                Color32::from_gray(30)
            },
            valign: Align::Center,
            ..Default::default()
        }
    }

    fn begin_para(&mut self, owner: NodeId, align: TextAlign, indent: f32) {
        if self.cur.is_none() {
            self.cur = Some(ParaBuilder::new(owner, align, indent));
        }
    }

    fn para(&mut self) -> &mut ParaBuilder {
        self.cur.as_mut().expect("paragraph")
    }

    /// Finishes the current paragraph. Empty paragraphs are kept only when
    /// `force` (they carry a caret stop).
    fn flush(&mut self, force: bool) {
        let Some(mut pb) = self.cur.take() else {
            return;
        };
        if !pb.has_content && !force {
            return;
        }
        let width = (self.width - pb.indent).max(40.0);
        pb.job.wrap.max_width = width;
        pb.job.halign = match pb.align {
            TextAlign::Left | TextAlign::Justify => Align::LEFT,
            TextAlign::Center => Align::Center,
            TextAlign::Right => Align::RIGHT,
        };
        if pb.job.is_empty() {
            // An empty line still needs height for the caret.
            let f = TextFormat {
                font_id: FontId::new(self.z(14.0), FontFamily::Name("sans".into())),
                ..Default::default()
            };
            pb.job.append("", 0.0, f);
        }
        let (galley, galley_key) = self.cache.galley(self.ctx, pb.job);
        let x = match pb.align {
            TextAlign::Center => self.x + pb.indent + width / 2.0,
            TextAlign::Right => self.x + pb.indent + width,
            _ => self.x + pb.indent,
        };
        let anchor = Pos2::new(x, self.y);
        let mut rect = galley.rect.translate(anchor.to_vec2());
        if rect.height() < self.z(14.0) {
            rect.max.y = rect.min.y + self.z(14.0);
        }
        if rect.width() < 2.0 {
            rect.max.x = rect.min.x + 2.0;
        }
        self.y += rect.height().max(self.z(14.0));
        let para = Para {
            rect,
            anchor,
            galley,
            galley_key,
            segs: pb.segs.into(),
            stops: pb.stops.into(),
            icons: pb.icons.into(),
            owner: pb.owner,
        };
        self.push_para(para);
    }

    /// Appends a paragraph and indexes its text segments and caret stops.
    fn push_para(&mut self, para: Para) {
        let pi = self.out.paras.len() as u32;
        let first = &mut self.out.first_para;
        let mut note = |n: NodeId| {
            let i = n.index();
            if i >= first.len() {
                first.resize(i + 1, u32::MAX);
            }
            if first[i] == u32::MAX {
                first[i] = pi;
            }
        };
        for s in para.segs.iter() {
            if let SegKind::Text { node, .. } = &s.kind {
                note(*node);
            }
        }
        for &(_, p) in para.stops.iter() {
            note(p.node);
        }
        self.out.paras.push(para);
    }

    /// Lays out a block, reusing the previous layout when nothing that affects
    /// it has changed. A block whose first line is shared with an ancestor's
    /// pending start tags is always laid out fresh.
    fn block(&mut self, el: NodeId) {
        let cacheable = self.cur.is_none();
        let stamp = self.doc.subtree_stamp(el);
        let style = self.styles.subtree_hash(el);
        let index = self.index[el.index()];
        if cacheable {
            if let Some(cb) = self.cache.blocks.get(&el) {
                if cb.stamp == stamp && cb.style == style && cb.x == self.x && cb.width == self.width {
                    self.reuse_block(el, index);
                    return;
                }
            }
        }
        self.cache.stats.blocks_laid_out += 1;
        let (p0, d0, i0, y0) = (self.out.paras.len(), self.out.decor.len(), self.out.images.len(), self.y);
        let (x, width) = (self.x, self.width);
        self.block_inner(el);
        if cacheable && self.cur.is_none() {
            let elements = self
                .doc
                .descendants(el)
                .filter_map(|n| {
                    let &(a, b) = self.out.element_paras.get(&n)?;
                    Some((n, (a.saturating_sub(p0), b.saturating_sub(p0))))
                })
                .collect();
            let cb = CachedBlock {
                stamp,
                style,
                x,
                width,
                index,
                y0,
                y1: self.y,
                paras: self.out.paras[p0..].iter().map(|p| p.shifted(0.0)).collect(),
                decor: self.out.decor[d0..].to_vec(),
                images: self.out.images[i0..].to_vec(),
                elements,
            };
            self.cache.blocks.insert(el, cb);
        }
    }

    fn reuse_block(&mut self, el: NodeId, index: u32) {
        self.cache.stats.blocks_reused += 1;
        let mut cb = self.cache.blocks.remove(&el).expect("cached block");
        let dy = self.y - cb.y0;
        let base = self.out.paras.len();
        // Caret stops just outside the block refer to its index in the parent;
        // shift them when siblings were inserted or removed before it.
        let parent = self.doc.parent(el);
        let shift = index as i64 - cb.index as i64;
        let old = cb.index as usize;
        for p in &cb.paras {
            let mut q = p.shifted(dy);
            if shift != 0 && q.stops.iter().any(|(_, s)| Some(s.node) == parent) {
                let stops: Vec<(usize, Pos)> = q
                    .stops
                    .iter()
                    .map(|&(c, s)| {
                        if Some(s.node) == parent && (s.offset == old || s.offset == old + 1) {
                            (c, Pos::new(s.node, (s.offset as i64 + shift) as usize))
                        } else {
                            (c, s)
                        }
                    })
                    .collect();
                q.stops = stops.into();
            }
            self.cache.used.insert(q.galley_key);
            self.push_para(q);
        }
        let d = Vec2::new(0.0, dy);
        self.out.decor.extend(cb.decor.iter().map(|x| x.shifted(dy)));
        self.out.images.extend(cb.images.iter().map(|i| ImageBox { rect: i.rect.translate(d), ..i.clone() }));
        for &(n, (a, b)) in &cb.elements {
            self.out.element_paras.insert(n, (a + base, b + base));
        }
        self.y = cb.y1 + dy;
        // Keep the entry in step with where the block now sits.
        if dy != 0.0 || shift != 0 {
            cb.paras = self.out.paras[base..].iter().map(|p| p.shifted(0.0)).collect();
            cb.decor = cb.decor.iter().map(|x| x.shifted(dy)).collect();
            cb.images = cb.images.iter().map(|i| ImageBox { rect: i.rect.translate(d), ..i.clone() }).collect();
            cb.y0 += dy;
            cb.y1 += dy;
            cb.index = index;
        }
        self.cache.blocks.insert(el, cb);
    }

    /// Ends the current line before a nested block — unless it holds only
    /// start tags, which then share the line with the block's first content.
    fn break_before_block(&mut self) {
        let tags_only = self.tags()
            && self.cur.as_ref().is_some_and(|p| {
                !p.segs.is_empty()
                    && p.segs
                        .iter()
                        .all(|s| matches!(s.kind, SegKind::StartTag(_)))
            });
        if !tags_only {
            self.flush(false);
        }
    }

    fn gap(&self, parent: NodeId, idx: usize) -> Pos {
        Pos::new(parent, idx)
    }

    fn before_of(&self, n: NodeId) -> Pos {
        let p = self.doc.parent(n).unwrap();
        self.gap(p, self.index[n.index()] as usize)
    }

    fn after_of(&self, n: NodeId) -> Pos {
        let p = self.doc.parent(n).unwrap();
        self.gap(p, self.index[n.index()] as usize + 1)
    }

    fn start_tag(&mut self, el: NodeId, empty: bool) {
        let name = self.doc.name(el).unwrap_or("?").to_string();
        let fmt = self.tag_fmt(&name);
        let before = self.before_of(el);
        let inside = self.gap(el, 0);
        let pb = self.para();
        pb.stop(before);
        pb.has_content = true;
        if empty {
            pb.push(&format!("  {name}  "), fmt, SegKind::EmptyTag(el));
            pb.stops.push((pb.chars, inside));
        } else {
            pb.push(&format!(" {name}  "), fmt, SegKind::StartTag(el));
            pb.stop(inside);
        }
        pb.last_space = true;
    }

    fn end_tag(&mut self, el: NodeId) {
        let name = self.doc.name(el).unwrap_or("?").to_string();
        let fmt = self.tag_fmt(&name);
        let inside = self.gap(el, self.doc.children(el).len());
        let after = self.after_of(el);
        let pb = self.para();
        pb.stop(inside);
        pb.has_content = true;
        pb.push(&format!("  {name} "), fmt, SegKind::EndTag(el));
        pb.stop(after);
        pb.last_space = true;
    }

    fn generated(&mut self, owner: NodeId, text: &str, s: &Sty) {
        if text.is_empty() {
            return;
        }
        let fmt = self.fmt(s);
        let text = apply_transform(text, s.text_transform);
        let pb = self.para();
        pb.push(&text, fmt, SegKind::Generated(owner));
        pb.has_content = true;
        pb.last_space = text.ends_with(char::is_whitespace);
    }

    /// Reserves room for a generated-content image in the current line.
    fn icon(&mut self, owner: NodeId, path: std::path::PathBuf, s: &Sty) {
        let fmt = self.fmt(s);
        // Roughly a square the height of the line: five spaces of the font.
        let pb = self.para();
        let start = pb.chars;
        pb.push("\u{2007}\u{2007}\u{2007}", fmt, SegKind::Generated(owner));
        let end = pb.chars;
        pb.icons.push((start..end, path));
        pb.has_content = true;
    }

    fn generated_images(&mut self, el: NodeId, after: bool, s: &Sty) {
        if let Some(path) = self.styles.generated_image(el, after) {
            self.icon(el, path, s);
        }
    }

    fn atom(&mut self, n: NodeId) {
        let (label, sty) = match self.doc.kind(n) {
            NodeKind::Comment(t) => {
                if !self.tags() && self.styles.get(n).display == Display::None {
                    return;
                }
                (format!("<!--{}-->", t.trim()), Some(self.styles.get(n)))
            }
            NodeKind::Pi { target, data } => {
                let s = self.styles.get(n);
                if s.display == Display::None {
                    return;
                }
                if target == "xml" {
                    return;
                }
                if target == "xm-replace_text" {
                    (xm_placeholder(data), Some(s))
                } else if self.tags() || s.has_rule {
                    (format!("<?{target} {data}?>"), Some(s))
                } else {
                    return;
                }
            }
            NodeKind::CData(t) => (t.clone(), Some(self.styles.get(n))),
            NodeKind::EntityRef(name) => (format!("&{name};"), None),
            _ => return,
        };
        let before = self.before_of(n);
        let after = self.after_of(n);
        let mut fmt = match &sty {
            Some(s) => self.fmt(s),
            None => self.fmt(&self.styles.get(self.doc.parent(n).unwrap())),
        };
        let is_placeholder =
            matches!(self.doc.kind(n), NodeKind::Pi { target, .. } if target == "xm-replace_text");
        if is_placeholder
            || matches!(
                self.doc.kind(n),
                NodeKind::Comment(_) | NodeKind::Pi { .. } | NodeKind::EntityRef(_)
            )
        {
            if sty.as_ref().is_none_or(|s| s.color.is_none()) {
                fmt.color = if is_placeholder {
                    Color32::from_rgb(0x30, 0x60, 0xc0)
                } else {
                    Color32::from_rgb(0x70, 0x70, 0x70)
                };
            }
            if is_placeholder {
                fmt.italics = true;
            }
        }
        let pb = self.para();
        pb.stop(before);
        pb.push(&label, fmt, SegKind::Atom(n));
        pb.stop(after);
        pb.has_content = true;
        pb.last_space = false;
    }

    fn text(&mut self, t: NodeId, s: &Sty) {
        let raw = self.doc.text(t).unwrap_or("");
        let fmt = self.fmt(s);
        let pre = matches!(s.white_space, WhiteSpace::Pre | WhiteSpace::PreWrap);
        let mut out = String::with_capacity(raw.len());
        let mut bytes = Vec::with_capacity(raw.len() + 1);
        let pb = self.cur.as_mut().unwrap();
        let mut last_space = pb.last_space;
        for (i, ch) in raw.char_indices() {
            let ch = if !pre && (ch == '\n' || ch == '\t' || ch == '\r') {
                ' '
            } else {
                ch
            };
            if !pre && ch == ' ' {
                if last_space {
                    continue;
                }
                last_space = true;
            } else {
                last_space = false;
            }
            out.push(ch);
            bytes.push(i);
        }
        bytes.push(raw.len());
        let out = apply_transform(&out, s.text_transform);
        // Transform can change char count (e.g. ß -> SS); keep mapping sane.
        let n = out.chars().count();
        bytes.resize(n + 1, raw.len());
        if n > 0 {
            pb.has_content = true;
        }
        pb.last_space = last_space;
        if n == 0 {
            // Keep a caret stop for the (collapsed) text node.
            pb.stop(Pos::new(t, raw.len()));
            return;
        }
        let start = pb.chars;
        pb.job.append(&out, 0.0, fmt);
        pb.segs.push(Seg {
            chars: start..start + n,
            kind: SegKind::Text { node: t, bytes },
        });
        pb.chars += n;
    }

    /// Lays out an inline element into the current paragraph.
    fn inline(&mut self, el: NodeId) {
        let s = self.styles.get(el);
        if s.display == Display::None {
            return;
        }
        let empty = self.doc.children(el).is_empty();
        if self.tags() {
            if empty && self.styles.before(el).is_none() && self.styles.after(el).is_none() {
                self.start_tag(el, true);
                return;
            }
            self.start_tag(el, false);
        } else {
            let before = self.before_of(el);
            let inside = self.gap(el, 0);
            let pb = self.para();
            pb.stop(before);
            pb.stop(inside);
        }
        // Generated images lead in ::before and trail in ::after.
        self.generated_images(el, false, &s);
        if let Some((text, gs)) = self.styles.before(el) {
            self.generated(el, &text, &gs);
        }
        if let Some((src, size)) = (self.params.image_size)(el) {
            self.inline_image_placeholder(el, &src, size, &s);
        }
        self.inline_children(el, &s);
        self.generated_images(el, true, &s);
        if let Some((text, gs)) = self.styles.after(el) {
            self.generated(el, &text, &gs);
        }
        if self.tags() {
            self.end_tag(el);
        } else {
            let inside = self.gap(el, self.doc.children(el).len());
            let after = self.after_of(el);
            let pb = self.para();
            pb.stop(inside);
            pb.stop(after);
        }
    }

    fn inline_image_placeholder(&mut self, el: NodeId, src: &str, _size: Vec2, s: &Sty) {
        let mut f = self.fmt(s);
        f.color = Color32::from_rgb(0x40, 0x70, 0x40);
        let name = src.rsplit(['/', '\\']).next().unwrap_or(src);
        let pb = self.para();
        pb.push(&format!("[image: {name}]"), f, SegKind::Generated(el));
        pb.has_content = true;
    }

    fn inline_children(&mut self, el: NodeId, s: &Sty) {
        let kids = self.doc.children(el).to_vec();
        for c in kids {
            match self.doc.kind(c) {
                NodeKind::Element(_) => {
                    let cs = self.styles.get(c);
                    if cs.display.is_block() {
                        // Block inside inline: break the line.
                        let owner = self.para().owner;
                        let align = self.para().align;
                        let indent = self.para().indent;
                        self.flush(false);
                        self.block(c);
                        self.begin_para(owner, align, indent);
                    } else {
                        self.inline(c);
                    }
                }
                NodeKind::Text(_) => {
                    if !is_ignorable(self.doc, self.rules, c) {
                        self.text(c, s);
                    }
                }
                _ => self.atom(c),
            }
        }
    }

    /// Lays out a block-level element.
    fn block_inner(&mut self, el: NodeId) {
        let s = self.styles.get(el);
        if s.display != Display::None && self.params.collapsed.contains(&el) {
            return self.collapsed_block(el);
        }
        match s.display {
            Display::None => return,
            Display::Table => return self.table(el, &s),
            _ => {}
        }
        let (outer_x, outer_w) = (self.x, self.width);
        let m = s.margin.map(|v| self.z(v));
        let p = s.padding.map(|v| self.z(v));
        let b = s.border_width.map(|v| self.z(v));
        self.break_before_block();
        self.y += m[0];
        let box_top = self.y;
        let box_x = self.x + m[3];
        let box_w = (self.width - m[3] - m[1]).max(40.0);
        self.x = box_x + b[3] + p[3];
        self.width = (box_w - b[3] - b[1] - p[3] - p[1]).max(30.0);
        self.y += b[0] + p[0];
        let first_para = self.out.paras.len();

        let indent = self.z(s.text_indent).max(0.0);
        self.begin_para(el, s.text_align, 0.0);
        let _ = indent;
        if self.tags() {
            self.start_tag(el, false);
        } else {
            let before = self.before_of(el);
            let inside = self.gap(el, 0);
            let pb = self.para();
            pb.stop(before);
            pb.stop(inside);
        }
        let marker = if s.display == Display::ListItem {
            self.styles.list_marker(el)
        } else {
            None
        };
        // Generated images lead in ::before and trail in ::after.
        self.generated_images(el, false, &s);
        if let Some((text, gs)) = self.styles.before(el) {
            self.generated(el, &text, &gs);
        }
        let image = (self.params.image_size)(el);
        let kids = self.doc.children(el).to_vec();
        let mut empty_content = kids.iter().all(|&c| is_ignorable(self.doc, self.rules, c));
        for c in kids {
            match self.doc.kind(c) {
                NodeKind::Element(_) => {
                    let cs = self.styles.get(c);
                    if cs.display == Display::None {
                        continue;
                    }
                    if cs.display.is_block() {
                        self.break_before_block();
                        self.block(c);
                        self.begin_para(el, s.text_align, 0.0);
                        empty_content = false;
                    } else {
                        self.inline(c);
                        empty_content = false;
                    }
                }
                NodeKind::Text(_) => {
                    if !is_ignorable(self.doc, self.rules, c) {
                        self.text(c, &s);
                    }
                }
                _ => self.atom(c),
            }
        }
        self.generated_images(el, true, &s);
        if let Some((text, gs)) = self.styles.after(el) {
            self.generated(el, &text, &gs);
        }
        if self.tags() {
            self.end_tag(el);
        } else {
            let inside = self.gap(el, self.doc.children(el).len());
            let after = self.after_of(el);
            let pb = self.para();
            pb.stop(inside);
            pb.stop(after);
            if empty_content {
                pb.has_content = true;
            }
        }
        self.flush(empty_content);
        if let Some((src, size)) = image {
            let w = size.x.min(self.width);
            let h = if size.x > 0.0 {
                size.y * w / size.x
            } else {
                size.y
            };
            let rect = Rect::from_min_size(Pos2::new(self.x, self.y + 2.0), Vec2::new(w, h));
            self.out.images.push(ImageBox {
                rect,
                node: el,
                src,
            });
            self.y += h + 4.0;
        }
        let last_para = self.out.paras.len().saturating_sub(1);
        self.out
            .element_paras
            .insert(el, (first_para, last_para.max(first_para)));
        if let (Some(text), Some(para)) = (marker, self.out.paras.get(first_para)) {
            // Align with the first line of content (skip a tags-only first line).
            let target = self.out.paras[first_para..]
                .iter()
                .find(|p| {
                    p.segs.iter().any(|sg| {
                        matches!(
                            sg.kind,
                            SegKind::Text { .. } | SegKind::Atom(_) | SegKind::Generated(_)
                        )
                    })
                })
                .unwrap_or(para);
            let row_h = target
                .galley
                .rows
                .first()
                .map(|r| r.height())
                .unwrap_or(16.0);
            let font = self.font(&s);
            let color = s
                .color
                .map(|c| adjust_for_dark(c, self.params.dark))
                .unwrap_or(if self.params.dark {
                    Color32::from_gray(225)
                } else {
                    Color32::from_gray(20)
                });
            let pos = Pos2::new(self.x - 4.0, target.rect.min.y + row_h / 2.0);
            self.out.decor.push(Decor::Marker {
                pos,
                text: text.trim_end().to_string(),
                font,
                color,
            });
        }
        self.y += p[2] + b[2];
        let rect = Rect::from_min_max(Pos2::new(box_x, box_top), Pos2::new(box_x + box_w, self.y));
        if let Some(bg) = s.background {
            self.out
                .decor
                .push(Decor::Fill(rect, adjust_bg_for_dark(bg, self.params.dark)));
        }
        if b.iter().any(|&w| w > 0.0) {
            let colors = s.border_color.map(|c| {
                c.map(|c| adjust_for_dark(c, self.params.dark))
                    .unwrap_or(Color32::GRAY)
            });
            self.out.decor.push(Decor::Border {
                rect,
                widths: b,
                colors,
            });
        }
        self.y += m[2];
        self.x = outer_x;
        self.width = outer_w;
    }

    /// A collapsed element: one line with its tags around a "..." marker that
    /// expands it again when clicked.
    fn collapsed_block(&mut self, el: NodeId) {
        self.break_before_block();
        let first = self.out.paras.len();
        self.begin_para(el, TextAlign::Left, 0.0);
        let name = self.doc.name(el).unwrap_or("").to_string();
        let mut fmt = self.tag_fmt(&name);
        fmt.color = if self.params.dark { Color32::from_gray(170) } else { Color32::from_gray(90) };
        let hidden = self.doc.descendants(el).filter(|&n| self.doc.is_element(n)).count().saturating_sub(1);
        let label = if self.tags() {
            format!("  ... {hidden} hidden  ")
        } else {
            format!("[+] {name} ... ({hidden} hidden) ")
        };
        if self.tags() {
            self.start_tag(el, false);
        } else {
            let before = self.before_of(el);
            self.para().stop(before);
        }
        {
            let pb = self.para();
            pb.push(&label, fmt, SegKind::Generated(el));
            pb.has_content = true;
        }
        if self.tags() {
            self.end_tag(el);
        } else {
            let after = self.after_of(el);
            self.para().stop(after);
        }
        self.flush(true);
        let last = self.out.paras.len().saturating_sub(1);
        self.out.element_paras.insert(el, (first, last.max(first)));
    }

    /// Table layout: rows of cells on an equal or `colwidth`-proportional grid.
    fn table(&mut self, el: NodeId, s: &Sty) {
        self.flush(false);
        let (outer_x, outer_w) = (self.x, self.width);
        let m = s.margin.map(|v| self.z(v));
        self.y += m[0];
        self.x += m[3];
        self.width = (self.width - m[3] - m[1]).max(60.0);
        if self.tags() {
            self.begin_para(el, TextAlign::Left, 0.0);
            self.start_tag(el, false);
            self.flush(true);
        }
        let first_para = self.out.paras.len();
        // Walk table content: rows are laid out on a grid, other blocks normally.
        let mut rows: Vec<(NodeId, Vec<NodeId>)> = Vec::new();
        let mut colwidths: Vec<f32> = Vec::new();
        self.collect_rows(el, &mut rows, &mut colwidths);
        let ncols = rows
            .iter()
            .map(|(r, cells)| self.row_span_count(*r, cells))
            .max()
            .unwrap_or(1)
            .max(colwidths.len())
            .max(1);
        let mut weights: Vec<f32> = (0..ncols)
            .map(|i| colwidths.get(i).copied().unwrap_or(1.0))
            .collect();
        let total: f32 = weights.iter().sum::<f32>().max(0.001);
        for w in &mut weights {
            *w /= total;
        }
        let table_w = self.width;
        let x0 = self.x;
        let grid = if self.params.dark {
            Color32::from_gray(90)
        } else {
            Color32::from_gray(170)
        };
        // Cells that span rows (CALS `morerows`, HTML `rowspan`) reserve
        // their columns in the following rows and are drawn as one tall cell
        // once the last spanned row is known.
        struct Spanning {
            decor: Vec<usize>,
            rows_left: usize,
            content_bottom: f32,
        }
        let mut occupied = vec![0usize; ncols];
        let mut spanning: Vec<Spanning> = Vec::new();
        for (row, cells) in rows {
            let row_top = self.y;
            let mut max_bottom = row_top;
            let mut row_decor: Vec<usize> = Vec::new();
            let explicit = self.explicit_columns(row, &cells, ncols);
            let mut next = 0;
            for (k, cell) in cells.iter().enumerate() {
                let (start, end) = match explicit.get(k).copied().flatten() {
                    Some(span) => span,
                    None => {
                        while next < ncols && occupied[next] > 0 {
                            next += 1;
                        }
                        let span = self.attr_usize(*cell, "colspan").unwrap_or(1).max(1);
                        let s = next.min(ncols - 1);
                        (s, (s + span - 1).min(ncols - 1))
                    }
                };
                next = end + 1;
                let rows_spanned = self
                    .attr_usize(*cell, "morerows")
                    .map(|m| m + 1)
                    .or_else(|| self.attr_usize(*cell, "rowspan"))
                    .unwrap_or(1)
                    .max(1);
                let cx = x0 + weights[..start].iter().sum::<f32>() * table_w;
                let cw = weights[start..=end].iter().sum::<f32>() * table_w;
                self.x = cx + 4.0;
                self.width = (cw - 8.0).max(20.0);
                self.y = row_top + 3.0;
                let cs = self.styles.get(*cell);
                self.block_contents_as_cell(*cell, &cs);
                let content_bottom = self.y + 3.0;
                let r = Rect::from_min_max(Pos2::new(cx, row_top), Pos2::new(cx + cw, 0.0));
                let mut idx = vec![self.out.decor.len()];
                self.out.decor.push(Decor::Grid(r, grid));
                if let Some(bg) = cs.background {
                    idx.push(self.out.decor.len());
                    self.out.decor.push(Decor::Fill(r, adjust_bg_for_dark(bg, self.params.dark)));
                }
                if rows_spanned > 1 {
                    for c in occupied.iter_mut().take(end + 1).skip(start) {
                        *c = rows_spanned;
                    }
                    spanning.push(Spanning { decor: idx, rows_left: rows_spanned, content_bottom });
                } else {
                    max_bottom = max_bottom.max(content_bottom);
                    row_decor.extend(idx);
                }
            }
            // A spanning cell that ends here must fit by the end of this row.
            for sp in &spanning {
                if sp.rows_left == 1 {
                    max_bottom = max_bottom.max(sp.content_bottom);
                }
            }
            if max_bottom <= row_top {
                max_bottom = row_top + self.z(18.0);
            }
            for i in row_decor {
                set_decor_bottom(&mut self.out.decor[i], max_bottom);
            }
            for sp in &mut spanning {
                sp.rows_left -= 1;
                if sp.rows_left == 0 {
                    for &i in &sp.decor {
                        set_decor_bottom(&mut self.out.decor[i], max_bottom);
                    }
                }
            }
            spanning.retain(|sp| sp.rows_left > 0);
            for c in &mut occupied {
                *c = c.saturating_sub(1);
            }
            self.y = max_bottom;
        }
        // Spans that run past the last row end with the table.
        for sp in spanning {
            for i in sp.decor {
                set_decor_bottom(&mut self.out.decor[i], self.y.max(sp.content_bottom));
            }
        }
        self.x = outer_x + m[3];
        self.width = (outer_w - m[3] - m[1]).max(60.0);
        if self.tags() {
            self.begin_para(el, TextAlign::Left, 0.0);
            self.end_tag(el);
            self.flush(true);
        }
        let last = self.out.paras.len().saturating_sub(1);
        self.out
            .element_paras
            .insert(el, (first_para, last.max(first_para)));
        self.y += m[2] + 4.0;
        self.x = outer_x;
        self.width = outer_w;
    }

    fn collect_rows(
        &mut self,
        n: NodeId,
        rows: &mut Vec<(NodeId, Vec<NodeId>)>,
        colwidths: &mut Vec<f32>,
    ) {
        for c in self.doc.element_children(n).collect::<Vec<_>>() {
            let cs = self.styles.get(c);
            let lname = self
                .doc
                .element(c)
                .map(|e| e.local_name().to_string())
                .unwrap_or_default();
            if lname == "colspec" {
                let w = self
                    .doc
                    .attr(c, "colwidth")
                    .and_then(parse_colwidth)
                    .unwrap_or(1.0);
                colwidths.push(w);
                continue;
            }
            if cs.display == Display::TableRow || matches!(lname.as_str(), "row" | "tr") {
                let cells: Vec<NodeId> = self
                    .doc
                    .element_children(c)
                    .filter(|&x| self.styles.get(x).display != Display::None)
                    .collect();
                rows.push((c, cells));
            } else if cs.display == Display::None {
                continue;
            } else if matches!(
                cs.display,
                Display::TableRowGroup | Display::TableHeaderGroup | Display::Table
            ) || matches!(lname.as_str(), "tgroup" | "thead" | "tbody" | "tfoot")
            {
                self.collect_rows(c, rows, colwidths);
            } else {
                // Title or other block content in the table.
                self.block(c);
            }
        }
    }

    fn row_span_count(&self, row: NodeId, cells: &[NodeId]) -> usize {
        let _ = row;
        cells
            .iter()
            .map(|&c| {
                self.doc
                    .attr(c, "colspan")
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(1)
                    .max(1)
            })
            .sum()
    }

    fn attr_usize(&self, n: NodeId, name: &str) -> Option<usize> {
        self.doc.attr(n, name).and_then(|v| v.trim().parse().ok())
    }

    /// Column ranges given explicitly by CALS `colname` / `namest`+`nameend`.
    /// `None` for cells that are placed in the next free column instead.
    fn explicit_columns(&self, row: NodeId, cells: &[NodeId], ncols: usize) -> Vec<Option<(usize, usize)>> {
        let group = self
            .doc
            .ancestors(row)
            .find(|&a| self.doc.element(a).is_some_and(|e| e.local_name() == "tgroup"));
        let names: Vec<String> = match group {
            Some(g) => self
                .doc
                .element_children(g)
                .filter(|&c| self.doc.element(c).is_some_and(|e| e.local_name() == "colspec"))
                .enumerate()
                .map(|(i, c)| self.doc.attr(c, "colname").map(str::to_string).unwrap_or_else(|| format!("col{}", i + 1)))
                .collect(),
            None => Vec::new(),
        };
        let col = |name: Option<&str>| name.and_then(|n| names.iter().position(|c| c == n));
        cells
            .iter()
            .map(|&c| {
                let start = col(self.doc.attr(c, "namest")).or_else(|| col(self.doc.attr(c, "colname")))?;
                let end = col(self.doc.attr(c, "nameend")).unwrap_or(start).max(start);
                Some((start.min(ncols - 1), end.min(ncols - 1)))
            })
            .collect()
    }

    fn block_contents_as_cell(&mut self, cell: NodeId, s: &Sty) {
        let mut cs = s.clone();
        cs.display = Display::Block;
        cs.margin = [0.0; 4];
        cs.border_width = [0.0; 4];
        cs.background = None;
        self.styles_override_block(cell, &cs);
    }

    fn styles_override_block(&mut self, el: NodeId, s: &Sty) {
        // Same as `block` but with an explicit style (cells ignore their own box).
        let first_para = self.out.paras.len();
        self.begin_para(el, s.text_align, 0.0);
        if self.tags() {
            self.start_tag(el, false);
        } else {
            let before = self.before_of(el);
            let inside = self.gap(el, 0);
            let pb = self.para();
            pb.stop(before);
            pb.stop(inside);
        }
        // Generated images lead in ::before and trail in ::after.
        self.generated_images(el, false, &s);
        if let Some((text, gs)) = self.styles.before(el) {
            self.generated(el, &text, &gs);
        }
        let kids = self.doc.children(el).to_vec();
        let mut empty = kids.iter().all(|&c| is_ignorable(self.doc, self.rules, c));
        for c in kids {
            match self.doc.kind(c) {
                NodeKind::Element(_) => {
                    let cs = self.styles.get(c);
                    if cs.display == Display::None {
                        continue;
                    }
                    empty = false;
                    if cs.display.is_block() {
                        self.break_before_block();
                        self.block(c);
                        self.begin_para(el, s.text_align, 0.0);
                    } else {
                        self.inline(c);
                    }
                }
                NodeKind::Text(_) => {
                    if !is_ignorable(self.doc, self.rules, c) {
                        self.text(c, s);
                    }
                }
                _ => self.atom(c),
            }
        }
        self.generated_images(el, true, &s);
        if let Some((text, gs)) = self.styles.after(el) {
            self.generated(el, &text, &gs);
        }
        if self.tags() {
            self.end_tag(el);
        } else {
            let inside = self.gap(el, self.doc.children(el).len());
            let after = self.after_of(el);
            let pb = self.para();
            pb.stop(inside);
            pb.stop(after);
            if empty {
                pb.has_content = true;
            }
        }
        self.flush(true);
        let last = self.out.paras.len().saturating_sub(1);
        self.out
            .element_paras
            .insert(el, (first_para, last.max(first_para)));
    }
}

fn set_decor_bottom(d: &mut Decor, bottom: f32) {
    if let Decor::Grid(r, _) | Decor::Fill(r, _) = d {
        r.max.y = bottom;
    }
}

fn parse_colwidth(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some(star) = v.strip_suffix('*') {
        return Some(if star.is_empty() {
            1.0
        } else {
            star.parse().ok()?
        });
    }
    // Absolute widths: treat as proportional weights.
    let num: String = v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let n: f32 = num.parse().ok()?;
    let unit = &v[num.len()..];
    Some(
        match unit.trim() {
            "in" => n * 96.0,
            "cm" => n * 37.8,
            "mm" => n * 3.78,
            "pt" => n * 1.33,
            _ => n,
        } / 50.0,
    )
}

fn apply_transform(s: &str, t: TextTransform) -> String {
    match t {
        TextTransform::None => s.to_string(),
        TextTransform::Uppercase => s.to_uppercase(),
        TextTransform::Lowercase => s.to_lowercase(),
        TextTransform::Capitalize => {
            let mut out = String::with_capacity(s.len());
            let mut start = true;
            for c in s.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else {
                    if c.is_whitespace() {
                        start = true;
                    }
                    out.push(c);
                }
            }
            out
        }
    }
}

/// Text shown for an `<?xm-replace_text ...?>` placeholder.
pub fn xm_placeholder(data: &str) -> String {
    let d = data
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim();
    d.trim_matches('"').trim_matches('\'').to_string()
}

fn luminance(c: Color32) -> f32 {
    0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32
}

/// Keeps author colours legible on a dark background.
fn adjust_for_dark(c: Color32, dark: bool) -> Color32 {
    if dark && luminance(c) < 90.0 {
        let [r, g, b, a] = c.to_array();
        Color32::from_rgba_unmultiplied(255 - r / 2, 255 - g / 2, 255 - b / 2, a)
    } else {
        c
    }
}

fn adjust_bg_for_dark(c: Color32, dark: bool) -> Color32 {
    if dark && luminance(c) > 170.0 {
        let [r, g, b, a] = c.to_array();
        Color32::from_rgba_unmultiplied(r / 4, g / 4, b / 4, a)
    } else {
        c
    }
}
