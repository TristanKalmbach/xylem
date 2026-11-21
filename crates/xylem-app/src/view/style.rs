//! The style model consumed by the layout engine, plus adapters from the CSS
//! engine and a heuristic default for documents without a stylesheet.

use egui::Color32;
use std::rc::Rc;
use xylem_dom::{Document, NodeId, NodeKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    InlineBlock,
    ListItem,
    None,
    Table,
    TableRow,
    TableCell,
    TableRowGroup,
    TableHeaderGroup,
}

impl Display {
    pub fn is_block(self) -> bool {
        matches!(
            self,
            Display::Block
                | Display::ListItem
                | Display::Table
                | Display::TableRow
                | Display::TableCell
                | Display::TableRowGroup
                | Display::TableHeaderGroup
        )
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TextTransform {
    None,
    Uppercase,
    Lowercase,
    Capitalize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WhiteSpace {
    Normal,
    Pre,
    NoWrap,
    PreWrap,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VerticalAlign {
    Baseline,
    Super,
    Sub,
}

#[derive(Clone, Debug)]
pub struct Sty {
    pub display: Display,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub line_through: bool,
    pub monospace: bool,
    pub serif: bool,
    pub color: Option<Color32>,
    /// Box background (block-level elements).
    pub background: Option<Color32>,
    /// Text background (inline elements).
    pub background_inline: Option<Color32>,
    pub text_align: TextAlign,
    pub text_transform: TextTransform,
    pub white_space: WhiteSpace,
    pub vertical_align: VerticalAlign,
    /// top, right, bottom, left
    pub margin: [f32; 4],
    pub padding: [f32; 4],
    pub border_width: [f32; 4],
    pub border_color: [Option<Color32>; 4],
    pub line_height: Option<f32>,
    pub text_indent: f32,
    /// A stylesheet rule matched this node (used to decide PI visibility).
    pub has_rule: bool,
}

impl Default for Sty {
    fn default() -> Self {
        Self {
            display: Display::Inline,
            font_size: 15.0,
            bold: false,
            italic: false,
            underline: false,
            line_through: false,
            monospace: false,
            serif: false,
            color: None,
            background: None,
            background_inline: None,
            text_align: TextAlign::Left,
            text_transform: TextTransform::None,
            white_space: WhiteSpace::Normal,
            vertical_align: VerticalAlign::Baseline,
            margin: [0.0; 4],
            padding: [0.0; 4],
            border_width: [0.0; 4],
            border_color: [None; 4],
            line_height: None,
            text_indent: 0.0,
            has_rule: false,
        }
    }
}

/// Fingerprint of everything in a style that affects layout.
fn hash_sty(s: &Sty, h: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    std::mem::discriminant(&s.display).hash(h);
    s.font_size.to_bits().hash(h);
    (
        s.bold,
        s.italic,
        s.underline,
        s.line_through,
        s.monospace,
        s.serif,
        s.has_rule,
    )
        .hash(h);
    s.color.map(|c| c.to_array()).hash(h);
    s.background.map(|c| c.to_array()).hash(h);
    s.background_inline.map(|c| c.to_array()).hash(h);
    std::mem::discriminant(&s.text_align).hash(h);
    std::mem::discriminant(&s.text_transform).hash(h);
    std::mem::discriminant(&s.white_space).hash(h);
    std::mem::discriminant(&s.vertical_align).hash(h);
    for v in s.margin.iter().chain(&s.padding).chain(&s.border_width) {
        v.to_bits().hash(h);
    }
    for c in &s.border_color {
        c.map(|c| c.to_array()).hash(h);
    }
    s.line_height.map(f32::to_bits).hash(h);
    s.text_indent.to_bits().hash(h);
}

/// Bottom-up subtree fingerprints: a node's value covers its own style,
/// generated content and every descendant's. Indexed by `NodeId::index`.
fn subtree_hashes(doc: &Document, node_hash: impl Fn(NodeId) -> u64) -> Vec<u64> {
    use std::hash::{Hash, Hasher};
    let order: Vec<NodeId> = doc.descendants(doc.root()).collect();
    let mut out = vec![0u64; doc.node_count()];
    for &n in order.iter().rev() {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        node_hash(n).hash(&mut h);
        for &c in doc.children(n) {
            out[c.index()].hash(&mut h);
        }
        out[n.index()] = h.finish();
    }
    out
}

pub trait StyleLookup {
    /// Changes whenever the styling of `n` or anything inside it changes.
    fn subtree_hash(&self, n: NodeId) -> u64;
    fn get(&self, n: NodeId) -> Sty;
    fn before(&self, n: NodeId) -> Option<(String, Sty)>;
    fn after(&self, n: NodeId) -> Option<(String, Sty)>;
    /// Image from `content: url(...)` in `::before` (false) or `::after` (true).
    fn generated_image(&self, _n: NodeId, _after: bool) -> Option<std::path::PathBuf> {
        None
    }
    fn list_marker(&self, n: NodeId) -> Option<String>;
}

/// Styles computed by the CSS engine, converted once per document revision.
pub struct CssStyles {
    styles: Vec<Option<Sty>>,
    before: std::collections::HashMap<NodeId, (String, Sty)>,
    after: std::collections::HashMap<NodeId, (String, Sty)>,
    markers: std::collections::HashMap<NodeId, String>,
    images: std::collections::HashMap<(NodeId, bool), std::path::PathBuf>,
    subtree: Vec<u64>,
    /// The document instance these styles were computed for (see
    /// `Document::new`); node IDs mean nothing for any other instance.
    instance: u64,
    fallback: Sty,
}

fn color(c: Option<[u8; 4]>) -> Option<Color32> {
    c.map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
}

fn convert(c: &xylem_style::ComputedStyle) -> Sty {
    use xylem_style as xs;
    let display = match c.display {
        xs::Display::Block => Display::Block,
        xs::Display::Inline => Display::Inline,
        xs::Display::InlineBlock => Display::InlineBlock,
        xs::Display::ListItem => Display::ListItem,
        xs::Display::None => Display::None,
        xs::Display::Table => Display::Table,
        xs::Display::TableRow => Display::TableRow,
        xs::Display::TableCell => Display::TableCell,
        xs::Display::TableRowGroup => Display::TableRowGroup,
        xs::Display::TableHeaderGroup | xs::Display::TableFooterGroup => Display::TableHeaderGroup,
        xs::Display::InlineTable => Display::Table,
        xs::Display::TableColumn | xs::Display::TableColumnGroup => Display::None,
        xs::Display::TableCaption => Display::Block,
    };
    let fam = c.font_family.as_deref().unwrap_or("").to_ascii_lowercase();
    let bg = color(c.background);
    let inline = matches!(display, Display::Inline | Display::InlineBlock);
    let b = &c.border;
    let bw = |x: &xs::Border| {
        if matches!(x.style, xs::BorderStyle::None) {
            0.0
        } else {
            x.width
        }
    };
    Sty {
        display,
        font_size: c.font_size,
        bold: c.bold,
        italic: c.italic,
        underline: c.underline,
        line_through: c.line_through,
        monospace: c.monospace,
        serif: (fam.contains("times") || fam.contains("serif") || fam.contains("georgia"))
            && !fam.contains("sans"),
        color: color(c.color),
        background: if inline { None } else { bg },
        background_inline: if inline { bg } else { None },
        text_align: match c.text_align {
            xs::TextAlign::Center => TextAlign::Center,
            xs::TextAlign::Right => TextAlign::Right,
            xs::TextAlign::Justify => TextAlign::Justify,
            _ => TextAlign::Left,
        },
        text_transform: match c.text_transform {
            xs::TextTransform::Uppercase => TextTransform::Uppercase,
            xs::TextTransform::Lowercase => TextTransform::Lowercase,
            xs::TextTransform::Capitalize => TextTransform::Capitalize,
            _ => TextTransform::None,
        },
        white_space: match c.white_space {
            xs::WhiteSpace::Pre => WhiteSpace::Pre,
            xs::WhiteSpace::Nowrap => WhiteSpace::NoWrap,
            xs::WhiteSpace::PreWrap | xs::WhiteSpace::PreLine => WhiteSpace::PreWrap,
            _ => WhiteSpace::Normal,
        },
        vertical_align: match c.vertical_align {
            xs::VerticalAlign::Super => VerticalAlign::Super,
            xs::VerticalAlign::Sub => VerticalAlign::Sub,
            _ => VerticalAlign::Baseline,
        },
        margin: [c.margin.top, c.margin.right, c.margin.bottom, c.margin.left],
        padding: [
            c.padding.top,
            c.padding.right,
            c.padding.bottom,
            c.padding.left,
        ],
        border_width: [bw(&b.top), bw(&b.right), bw(&b.bottom), bw(&b.left)],
        border_color: [
            color(b.top.color),
            color(b.right.color),
            color(b.bottom.color),
            color(b.left.color),
        ],
        line_height: c.line_height,
        text_indent: c.text_indent,
        has_rule: true,
    }
}

impl CssStyles {
    /// `css_dir` resolves `url(...)` references in generated content.
    pub fn compute(
        doc: &Document,
        engine: &xylem_style::StyleEngine,
        css_dir: Option<&std::path::Path>,
    ) -> Self {
        let t0 = std::time::Instant::now();
        let map = engine.compute(doc);
        let t_engine = t0.elapsed();
        let mut styles = vec![None; doc.node_count()];
        let mut before = std::collections::HashMap::new();
        let mut after = std::collections::HashMap::new();
        let mut markers = std::collections::HashMap::new();
        let mut images = std::collections::HashMap::new();
        for n in doc.descendants(doc.root()) {
            if doc.is_text(n) {
                continue;
            }
            let cs = map.get(n);
            styles[n.index()] = Some(convert(cs));
            if cs.display == xylem_style::Display::ListItem
                && cs.list_style != xylem_style::ListStyle::None
                && let Some(i) = map.list_index(n)
            {
                let m = xylem_style::format_counter(i, cs.list_style);
                let bullet = matches!(
                    cs.list_style,
                    xylem_style::ListStyle::Disc
                        | xylem_style::ListStyle::Circle
                        | xylem_style::ListStyle::Square
                );
                markers.insert(
                    n,
                    if bullet {
                        format!("{m} ")
                    } else {
                        format!("{m}. ")
                    },
                );
            }
            for (after, g) in [(false, map.before(n)), (true, map.after(n))] {
                let Some(src) = g.and_then(|g| g.image.as_deref()) else {
                    continue;
                };
                let path = match css_dir {
                    Some(dir) => dir.join(src),
                    None => std::path::PathBuf::from(src),
                };
                if path.is_file() {
                    images.insert((n, after), path);
                }
            }
            if let Some(g) = map.before(n)
                && !g.text.is_empty()
            {
                before.insert(n, (g.text.clone(), convert(&g.style)));
            }
            if let Some(g) = map.after(n)
                && !g.text.is_empty()
            {
                after.insert(n, (g.text.clone(), convert(&g.style)));
            }
        }
        let t_convert = t0.elapsed() - t_engine;
        let subtree = subtree_hashes(doc, |n| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some(Some(st)) = styles.get(n.index()) {
                hash_sty(st, &mut h);
            }
            for g in [before.get(&n), after.get(&n)].into_iter().flatten() {
                let g: &(String, Sty) = g;
                g.0.hash(&mut h);
                hash_sty(&g.1, &mut h);
            }
            markers.get(&n).hash(&mut h);
            h.finish()
        });
        log::debug!(
            "styles: engine {t_engine:?}, convert {t_convert:?}, hashes {:?}",
            t0.elapsed() - t_engine - t_convert
        );
        Self {
            styles,
            before,
            after,
            markers,
            images,
            subtree,
            instance: doc.structure_revision() >> 32,
            fallback: Sty::default(),
        }
    }
}

impl StyleLookup for CssStyles {
    fn subtree_hash(&self, n: NodeId) -> u64 {
        self.subtree.get(n.index()).copied().unwrap_or(u64::MAX)
    }
    fn get(&self, n: NodeId) -> Sty {
        self.styles
            .get(n.index())
            .and_then(|s| s.clone())
            .unwrap_or_else(|| self.fallback.clone())
    }
    fn before(&self, n: NodeId) -> Option<(String, Sty)> {
        self.before.get(&n).cloned()
    }
    fn after(&self, n: NodeId) -> Option<(String, Sty)> {
        self.after.get(&n).cloned()
    }
    fn list_marker(&self, n: NodeId) -> Option<String> {
        self.markers.get(&n).cloned()
    }
    fn generated_image(&self, n: NodeId, after: bool) -> Option<std::path::PathBuf> {
        self.images.get(&(n, after)).cloned()
    }
}

/// Styling for documents with no stylesheet: elements are blocks, except those
/// in mixed content which flow inline; nesting is shown by indentation.
pub struct DefaultStyles {
    styles: Vec<Option<Sty>>,
    subtree: Vec<u64>,
}

impl DefaultStyles {
    pub fn compute(doc: &Document) -> Self {
        let mut styles = vec![None; doc.node_count()];
        for n in doc.descendants(doc.root()) {
            match doc.kind(n) {
                NodeKind::Element(_) => {
                    let parent_mixed = doc.parent(n).is_some_and(|p| {
                        doc.children(p)
                            .iter()
                            .any(|&c| doc.text(c).is_some_and(|t| !t.trim().is_empty()))
                    });
                    let lname = doc.element(n).unwrap().local_name().to_ascii_lowercase();
                    let mut s = Sty::default();
                    if parent_mixed {
                        s.display = Display::Inline;
                        match lname.as_str() {
                            "b" | "bold" | "strong" => s.bold = true,
                            "i" | "italic" | "em" | "emphasis" => s.italic = true,
                            "u" | "underline" => s.underline = true,
                            "sup" | "superscript" => s.vertical_align = VerticalAlign::Super,
                            "sub" | "subscript" => s.vertical_align = VerticalAlign::Sub,
                            "code" | "tt" | "codeph" => s.monospace = true,
                            _ => {}
                        }
                    } else {
                        s.display = Display::Block;
                        let depth = doc.depth(n);
                        s.margin = [2.0, 0.0, 2.0, if depth > 1 { 14.0 } else { 0.0 }];
                        if matches!(
                            lname.as_str(),
                            "title" | "techname" | "infoname" | "head" | "heading"
                        ) {
                            s.bold = true;
                            s.font_size = 17.0;
                        }
                        match lname.as_str() {
                            "table" | "informaltable" => s.display = Display::Table,
                            "row" | "tr" => s.display = Display::TableRow,
                            "entry" | "td" | "th" => s.display = Display::TableCell,
                            "colspec" | "spanspec" => s.display = Display::None,
                            "pre" | "verbatimtext" | "programlisting" | "screen" => {
                                s.white_space = WhiteSpace::Pre;
                                s.monospace = true;
                            }
                            _ => {}
                        }
                    }
                    styles[n.index()] = Some(s);
                }
                NodeKind::Comment(_) | NodeKind::Pi { .. } => {
                    styles[n.index()] = Some(Sty {
                        has_rule: false,
                        ..Sty::default()
                    });
                }
                _ => {}
            }
        }
        let subtree = subtree_hashes(doc, |n| {
            use std::hash::Hasher;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            if let Some(Some(st)) = styles.get(n.index()) {
                hash_sty(st, &mut h);
            }
            h.finish()
        });
        Self { styles, subtree }
    }
}

impl StyleLookup for DefaultStyles {
    fn subtree_hash(&self, n: NodeId) -> u64 {
        self.subtree.get(n.index()).copied().unwrap_or(u64::MAX)
    }
    fn get(&self, n: NodeId) -> Sty {
        self.styles
            .get(n.index())
            .and_then(|s| s.clone())
            .unwrap_or_default()
    }
    fn before(&self, _n: NodeId) -> Option<(String, Sty)> {
        None
    }
    fn after(&self, _n: NodeId) -> Option<(String, Sty)> {
        None
    }
    fn list_marker(&self, _n: NodeId) -> Option<String> {
        None
    }
}

/// Styles for a document that has just changed structurally, served while the
/// exact styles are recomputed in the background. Existing nodes keep their
/// previous styles; new elements borrow the style of an element with the same
/// name (usually a sibling), which is right in all but unusual cases (counter
/// values and sibling-dependent rules catch up when the exact styles arrive).
pub struct PatchedStyles {
    base: Rc<CssStyles>,
    borrowed: std::collections::HashMap<NodeId, NodeId>,
}

impl PatchedStyles {
    /// `None` when `base` belongs to a different document instance.
    pub fn new(base: Rc<CssStyles>, doc: &Document) -> Option<Self> {
        if base.instance != doc.structure_revision() >> 32 {
            return None;
        }
        let known = |n: NodeId| base.styles.get(n.index()).is_some_and(Option::is_some);
        // Nodes are never reused, so everything created since `base` was
        // computed has a higher index than any node it covers.
        let attached = |n: NodeId| doc.ancestors(n).any(|a| a == doc.root());
        let mut borrowed = std::collections::HashMap::new();
        let mut by_name: Option<std::collections::HashMap<String, NodeId>> = None;
        for n in new_element_ids(doc, base.styles.len()) {
            if !attached(n) {
                continue;
            }
            let name = doc.name(n).unwrap_or("").to_string();
            // Prefer a same-named sibling, then any element with that name.
            let sibling = doc.parent(n).and_then(|p| {
                doc.element_children(p)
                    .find(|&c| c != n && known(c) && doc.name(c) == Some(name.as_str()))
            });
            let src = sibling.or_else(|| {
                let map = by_name.get_or_insert_with(|| {
                    let mut m = std::collections::HashMap::new();
                    for e in doc.descendants(doc.root()).filter(|&e| known(e)) {
                        m.entry(doc.name(e).unwrap_or("").to_string()).or_insert(e);
                    }
                    m
                });
                map.get(&name).copied()
            });
            if let Some(src) = src {
                borrowed.insert(n, src);
            }
        }
        Some(Self { base, borrowed })
    }

    fn source(&self, n: NodeId) -> NodeId {
        self.borrowed.get(&n).copied().unwrap_or(n)
    }
}

impl StyleLookup for PatchedStyles {
    fn subtree_hash(&self, n: NodeId) -> u64 {
        // New nodes get a value no cached layout can have; changed ancestors
        // are caught by the document's subtree stamps.
        if self.borrowed.contains_key(&n)
            || self.base.styles.get(n.index()).is_none_or(Option::is_none)
        {
            return u64::MAX ^ n.index() as u64;
        }
        self.base.subtree_hash(n)
    }
    fn get(&self, n: NodeId) -> Sty {
        self.base.get(self.source(n))
    }
    fn before(&self, n: NodeId) -> Option<(String, Sty)> {
        self.base.before(self.source(n))
    }
    fn after(&self, n: NodeId) -> Option<(String, Sty)> {
        self.base.after(self.source(n))
    }
    fn list_marker(&self, n: NodeId) -> Option<String> {
        self.base.list_marker(self.source(n))
    }
    fn generated_image(&self, n: NodeId, after: bool) -> Option<std::path::PathBuf> {
        self.base.generated_image(self.source(n), after)
    }
}

/// How elements that do not apply under the current applicability filter are shown.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
pub enum ApplicDisplay {
    /// Drawn in a faded colour.
    #[default]
    Dim,
    /// Left out of the view.
    Hide,
}

/// Wraps the document's styles to fade or hide inapplicable elements.
pub struct ApplicStyles {
    base: Rc<dyn StyleLookup>,
    /// Inapplicable elements and everything inside them.
    excluded: Rc<std::collections::HashSet<NodeId>>,
    display: ApplicDisplay,
    dim: Color32,
    /// Fingerprint of the excluded set and display mode. Mixed into every
    /// subtree hash, so cached blocks are laid out again when the filter
    /// changes even though their own content did not.
    salt: u64,
}

impl ApplicStyles {
    pub fn new(
        base: Rc<dyn StyleLookup>,
        excluded: Rc<std::collections::HashSet<NodeId>>,
        display: ApplicDisplay,
        dim: Color32,
        salt: u64,
    ) -> Self {
        Self {
            base,
            excluded,
            display,
            dim,
            salt,
        }
    }

    fn adjust(&self, n: NodeId, mut s: Sty) -> Sty {
        if self.excluded.contains(&n) {
            match self.display {
                ApplicDisplay::Dim => {
                    s.color = Some(self.dim);
                    s.border_color = s.border_color.map(|c| c.map(|_| self.dim));
                }
                ApplicDisplay::Hide => s.display = Display::None,
            }
        }
        s
    }
}

impl StyleLookup for ApplicStyles {
    fn subtree_hash(&self, n: NodeId) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (self.base.subtree_hash(n), self.salt).hash(&mut h);
        h.finish()
    }
    fn get(&self, n: NodeId) -> Sty {
        self.adjust(n, self.base.get(n))
    }
    fn before(&self, n: NodeId) -> Option<(String, Sty)> {
        self.base.before(n).map(|(t, s)| (t, self.adjust(n, s)))
    }
    fn after(&self, n: NodeId) -> Option<(String, Sty)> {
        self.base.after(n).map(|(t, s)| (t, self.adjust(n, s)))
    }
    fn list_marker(&self, n: NodeId) -> Option<String> {
        self.base.list_marker(n)
    }
    fn generated_image(&self, n: NodeId, after: bool) -> Option<std::path::PathBuf> {
        self.base.generated_image(n, after)
    }
}

/// Attached element nodes created after the first `from` arena slots.
fn new_element_ids(doc: &Document, from: usize) -> Vec<NodeId> {
    doc.descendants(doc.root())
        .filter(|n| n.index() >= from && doc.is_element(*n))
        .collect()
}
