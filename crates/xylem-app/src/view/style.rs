//! The style model consumed by the layout engine, plus adapters from the CSS
//! engine and a heuristic default for documents without a stylesheet.

use egui::Color32;
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

pub trait StyleLookup {
    fn get(&self, n: NodeId) -> Sty;
    fn before(&self, n: NodeId) -> Option<(String, Sty)>;
    fn after(&self, n: NodeId) -> Option<(String, Sty)>;
    fn list_marker(&self, n: NodeId) -> Option<String>;
}

/// Styles computed by the CSS engine, converted once per document revision.
pub struct CssStyles {
    styles: Vec<Option<Sty>>,
    before: std::collections::HashMap<NodeId, (String, Sty)>,
    after: std::collections::HashMap<NodeId, (String, Sty)>,
    markers: std::collections::HashMap<NodeId, String>,
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
    pub fn compute(doc: &Document, engine: &xylem_style::StyleEngine) -> Self {
        let map = engine.compute(doc);
        let mut styles = vec![None; doc.node_count()];
        let mut before = std::collections::HashMap::new();
        let mut after = std::collections::HashMap::new();
        let mut markers = std::collections::HashMap::new();
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
        Self {
            styles,
            before,
            after,
            markers,
            fallback: Sty::default(),
        }
    }
}

impl StyleLookup for CssStyles {
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
}

/// Styling for documents with no stylesheet: elements are blocks, except those
/// in mixed content which flow inline; nesting is shown by indentation.
pub struct DefaultStyles {
    styles: Vec<Option<Sty>>,
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
        Self { styles }
    }
}

impl StyleLookup for DefaultStyles {
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
