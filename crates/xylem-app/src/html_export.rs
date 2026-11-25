//! Standalone HTML from a document, styled the way the Normal (tags off)
//! view shows it.
//!
//! The export walks the document with the same computed styles the editor
//! lays out with ([`StyleLookup`]), so counters, generated text, list
//! markers and hidden elements come out as the author sees them. Each
//! element becomes a `div` or `span` with inline CSS for the properties that
//! differ from its parent; tables become real HTML tables, and images are
//! embedded as data URIs so the file can be mailed or archived on its own.
//! Comments and processing instructions are never printed.

use crate::view::layout::parse_colwidth;
use crate::view::style::{
    Display, Sty, StyleLookup, TextAlign, TextTransform, VerticalAlign, WhiteSpace,
};
use egui::Color32;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId, NodeKind};
use xylem_edit::Rules;
use xylem_edit::pos::is_ignorable;

/// Widest an image is shown, matching the editor's document view.
const MAX_IMAGE_WIDTH: f32 = 480.0;
/// Text colour the editor uses when the stylesheet sets none.
const DEFAULT_COLOR: Color32 = Color32::from_rgb(20, 20, 20);
/// Table grid colour in the editor (light theme).
const GRID_COLOR: &str = "#aaaaaa";

/// An embedded image: its data URI and, when known, the size it is shown at.
type Embedded = (String, Option<(f32, f32)>);

/// An image element: its source attribute and the file it resolved to.
pub struct ImageRef {
    pub src: String,
    pub path: Option<PathBuf>,
}

pub struct ExportInput<'a> {
    pub doc: &'a Document,
    pub styles: &'a dyn StyleLookup,
    /// Decides which whitespace is insignificant, as in the editor.
    pub rules: &'a dyn Rules,
    /// Elements that display an image.
    pub images: &'a HashMap<NodeId, ImageRef>,
    pub title: &'a str,
}

/// The whole HTML page.
pub fn to_html(input: &ExportInput) -> String {
    let mut w = Writer {
        input,
        out: String::new(),
        uris: HashMap::new(),
    };
    let base = Sty::default();
    if let Some(root) = input.doc.document_element() {
        w.node(root, &base);
    }
    let body = std::mem::take(&mut w.out);
    let mut page = String::with_capacity(body.len() + 2048);
    page.push_str("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>");
    escape(input.title, &mut page);
    page.push_str("</title>\n<style>\n");
    page.push_str(&page_css(&base));
    page.push_str("</style>\n</head>\n<body>\n");
    page.push_str(&body);
    page.push_str("\n</body>\n</html>\n");
    page
}

/// Page-wide CSS: the editor's default font, print margins, and keeping
/// headings, table rows and images in one piece across page breaks.
fn page_css(base: &Sty) -> String {
    format!(
        "html {{ -webkit-print-color-adjust: exact; print-color-adjust: exact; }}
body {{ margin: 8px; font-family: {}; font-size: {}px; color: {}; background: #ffffff; }}
@page {{ margin: 18mm 16mm; }}
@media print {{ body {{ margin: 0; }} }}
table {{ border-collapse: collapse; table-layout: fixed; width: 100%; }}
thead {{ display: table-header-group; }}
tr, img, .x-keep {{ break-inside: avoid; page-break-inside: avoid; }}
.x-keep {{ break-after: avoid; page-break-after: avoid; }}
td {{ border: 1px solid {GRID_COLOR}; padding: 3px 4px; vertical-align: top; }}
div, td {{ unicode-bidi: plaintext; }}
.x-item {{ position: relative; }}
.x-marker {{ position: absolute; white-space: nowrap; }}
.x-noimage {{ display: inline-block; border: 1px dashed #888888; color: #406040; padding: 12px 16px; }}
",
        family(base),
        num(base.font_size),
        hex(DEFAULT_COLOR)
    )
}

struct Writer<'a> {
    input: &'a ExportInput<'a>,
    out: String,
    /// Data URIs already made, with the size the image is shown at.
    uris: HashMap<PathBuf, Option<Embedded>>,
}

impl Writer<'_> {
    fn doc(&self) -> &Document {
        self.input.doc
    }

    fn style(&self, n: NodeId) -> Sty {
        self.input.styles.get(n)
    }

    /// Any child node, given its parent's style.
    fn node(&mut self, n: NodeId, parent: &Sty) {
        let doc = self.input.doc;
        match doc.kind(n) {
            NodeKind::Element(_) => {
                let s = self.style(n);
                match s.display {
                    Display::None => {}
                    Display::Table => self.table(n, &s, parent),
                    d if d.is_block() => self.block(n, &s, parent),
                    _ => self.inline(n, &s, parent),
                }
            }
            NodeKind::Text(t) => {
                if !is_ignorable(doc, self.input.rules, n) {
                    escape(t, &mut self.out);
                }
            }
            NodeKind::CData(t) => escape(t, &mut self.out),
            // Shown as written, as in the editor: an unresolved entity has
            // no replacement text to print.
            NodeKind::EntityRef(name) => {
                let _ = write!(
                    self.out,
                    "<span style=\"color:#707070\">&amp;{}</span>",
                    esc(name)
                );
                self.out.push(';');
            }
            // Comments, processing instructions and placeholders are notes
            // for authors, not part of the printed document.
            NodeKind::Comment(_) | NodeKind::Pi { .. } => {}
            NodeKind::Doctype(_) | NodeKind::Document => {}
        }
    }

    fn children(&mut self, n: NodeId, s: &Sty) {
        let kids = self.doc().children(n).to_vec();
        for c in kids {
            self.node(c, s);
        }
    }

    fn block(&mut self, n: NodeId, s: &Sty, parent: &Sty) {
        let mut css = css(s, parent, true);
        let marker = (s.display == Display::ListItem)
            .then(|| self.input.styles.list_marker(n))
            .flatten();
        let mut class = Vec::new();
        if marker.is_some() {
            class.push("x-item");
        }
        if is_heading(self.doc(), n, s, parent) {
            class.push("x-keep");
        }
        self.out.push_str("<div");
        if !class.is_empty() {
            let _ = write!(self.out, " class=\"{}\"", class.join(" "));
        }
        if s.display == Display::InlineBlock {
            css.push_str("display:inline-block;");
        }
        self.style_attr(&css);
        self.out.push('>');
        if let Some(m) = marker {
            // The editor ends the marker 4px left of the content's first
            // line. Offsets are from the padding box, hence the padding.
            let _ = write!(
                self.out,
                "<span class=\"x-marker\" style=\"right:calc(100% + {}px);top:{}px\">",
                num(4.0 - s.padding[3]),
                num(s.padding[0])
            );
            escape(m.trim_end(), &mut self.out);
            self.out.push_str("</span>");
        }
        let start = self.out.len();
        self.content(n, s);
        if let Some(img) = self.input.images.get(&n) {
            self.out.push_str("<div>");
            self.image(img);
            self.out.push_str("</div>");
        } else if self.out.len() == start {
            // The editor gives an empty block one line, which also keeps a
            // list marker from running into what follows.
            self.out.push_str("<br>");
        }
        self.out.push_str("</div>\n");
    }

    fn inline(&mut self, n: NodeId, s: &Sty, parent: &Sty) {
        let mut css = css(s, parent, false);
        if s.display == Display::InlineBlock {
            css.push_str("display:inline-block;");
        }
        self.out.push_str("<span");
        self.style_attr(&css);
        self.out.push('>');
        self.content(n, s);
        if let Some(img) = self.input.images.get(&n) {
            self.image(img);
        }
        self.out.push_str("</span>");
    }

    /// Generated content around the element's children.
    fn content(&mut self, n: NodeId, s: &Sty) {
        self.generated_image(n, false);
        if let Some((text, gs)) = self.input.styles.before(n) {
            self.generated(&text, &gs, s);
        }
        self.children(n, s);
        self.generated_image(n, true);
        if let Some((text, gs)) = self.input.styles.after(n) {
            self.generated(&text, &gs, s);
        }
    }

    fn generated(&mut self, text: &str, gs: &Sty, s: &Sty) {
        let style = css(gs, s, false);
        if style.is_empty() {
            escape(text, &mut self.out);
            return;
        }
        self.out.push_str("<span");
        self.style_attr(&style);
        self.out.push('>');
        escape(text, &mut self.out);
        self.out.push_str("</span>");
    }

    fn generated_image(&mut self, n: NodeId, after: bool) {
        let Some(path) = self.input.styles.generated_image(n, after) else {
            return;
        };
        if let Some((uri, _)) = self.data_uri(&path) {
            // A square about the height of the line, as in the editor.
            let _ = write!(
                self.out,
                "<img src=\"{uri}\" alt=\"\" style=\"height:1.15em;vertical-align:-0.2em\">"
            );
        }
    }

    fn image(&mut self, img: &ImageRef) {
        let name = img
            .src
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(&img.src)
            .to_string();
        let embedded = img.path.as_deref().and_then(|p| self.data_uri(p));
        match embedded {
            Some((uri, size)) => {
                let size = match size {
                    Some((w, h)) => format!("width:{}px;height:{}px;", num(w), num(h)),
                    None => format!("max-width:{}px;", num(MAX_IMAGE_WIDTH)),
                };
                let _ = write!(
                    self.out,
                    "<img src=\"{uri}\" alt=\"{}\" style=\"{size}max-width:100%;object-fit:contain\">",
                    esc(&name)
                );
            }
            // CGM and missing files: a labelled box, like the editor.
            None => {
                let _ = write!(
                    self.out,
                    "<span class=\"x-noimage\">image: {}</span>",
                    esc(&name)
                );
            }
        }
    }

    /// The file as a data URI, with its display size when known.
    fn data_uri(&mut self, path: &Path) -> Option<Embedded> {
        if let Some(cached) = self.uris.get(path) {
            return cached.clone();
        }
        let made = make_data_uri(path);
        self.uris.insert(path.to_path_buf(), made.clone());
        made
    }

    fn style_attr(&mut self, css: &str) {
        if !css.is_empty() {
            self.out.push_str(" style=\"");
            escape(css, &mut self.out);
            self.out.push('"');
        }
    }

    /// Tables follow the editor's layout: other block content (a title)
    /// first, then the rows on a grid sized by CALS `colwidth`, with cells
    /// placed by `namest`/`nameend`/`colname` or `colspan`, and spanning
    /// rows by `morerows` or `rowspan`.
    fn table(&mut self, n: NodeId, s: &Sty, parent: &Sty) {
        let mut rows = Vec::new();
        let mut widths = Vec::new();
        let mut prelude = Vec::new();
        self.collect_rows(n, false, &mut rows, &mut widths, &mut prelude);
        let ncols = rows
            .iter()
            .map(|r: &Row| self.colspan_total(&r.cells))
            .max()
            .unwrap_or(1)
            .max(widths.len())
            .max(1);
        let weights: Vec<f32> = (0..ncols)
            .map(|i| widths.get(i).copied().unwrap_or(1.0))
            .collect();
        let total: f32 = weights.iter().sum::<f32>().max(0.001);

        // The editor keeps a table's margins but draws only the grid, not
        // the table's own padding, border or background.
        let mut outer = s.clone();
        outer.padding = [0.0; 4];
        outer.border_width = [0.0; 4];
        outer.background = None;
        let wrapper = css(&outer, parent, true);
        self.out.push_str("<div");
        self.style_attr(&wrapper);
        self.out.push_str(">\n");
        for b in prelude {
            self.node(b, s);
        }
        self.out.push_str("<table>\n<colgroup>");
        for w in &weights {
            let _ = write!(
                self.out,
                "<col style=\"width:{}%\">",
                num(w / total * 100.0)
            );
        }
        self.out.push_str("</colgroup>\n");
        let mut occupied = vec![0usize; ncols];
        let mut section = None;
        for row in rows {
            if section != Some(row.header) {
                if let Some(h) = section {
                    self.out
                        .push_str(if h { "</thead>\n" } else { "</tbody>\n" });
                }
                self.out
                    .push_str(if row.header { "<thead>\n" } else { "<tbody>\n" });
                section = Some(row.header);
            }
            self.row(&row, ncols, &mut occupied, s);
        }
        if let Some(h) = section {
            self.out
                .push_str(if h { "</thead>\n" } else { "</tbody>\n" });
        }
        self.out.push_str("</table>\n</div>\n");
    }

    fn row(&mut self, row: &Row, ncols: usize, occupied: &mut [usize], table: &Sty) {
        self.out.push_str("<tr>");
        let explicit = self.explicit_columns(row.node, &row.cells, ncols);
        // The next column the browser would fill: it skips columns taken by
        // cells spanning down from earlier rows, as the editor does.
        let mut cursor = 0;
        let free_from = |c: usize, occupied: &[usize]| {
            let mut c = c;
            while c < ncols && occupied[c] > 0 {
                c += 1;
            }
            c
        };
        let mut spans = Vec::new();
        for (k, &cell) in row.cells.iter().enumerate() {
            let at = free_from(cursor, occupied);
            let (start, end) = match explicit.get(k).copied().flatten() {
                Some((s, e)) if s >= at => (s, e),
                Some((_, e)) => (at.min(ncols - 1), e.max(at).min(ncols - 1)),
                None => {
                    let span = self.attr_usize(cell, "colspan").unwrap_or(1).max(1);
                    let s = at.min(ncols - 1);
                    (s, (s + span - 1).min(ncols - 1))
                }
            };
            // Empty cells for skipped columns, so the cell lands where its
            // column names put it.
            let mut c = at;
            while c < start {
                if occupied[c] == 0 {
                    self.out.push_str("<td></td>");
                }
                c += 1;
            }
            let rows_spanned = self
                .attr_usize(cell, "morerows")
                .map(|m| m + 1)
                .or_else(|| self.attr_usize(cell, "rowspan"))
                .unwrap_or(1)
                .max(1);
            self.cell(cell, end - start + 1, rows_spanned, table);
            if rows_spanned > 1 {
                spans.push((start, end, rows_spanned));
            }
            cursor = end + 1;
        }
        for (start, end, n) in spans {
            for o in &mut occupied[start..=end] {
                *o = n;
            }
        }
        for o in occupied.iter_mut() {
            *o = o.saturating_sub(1);
        }
        self.out.push_str("</tr>\n");
    }

    fn cell(&mut self, cell: NodeId, colspan: usize, rowspan: usize, table: &Sty) {
        let cs = self.style(cell);
        // Cells draw only their background; the grid replaces their box.
        let mut boxless = cs.clone();
        boxless.margin = [0.0; 4];
        boxless.padding = [0.0; 4];
        boxless.border_width = [0.0; 4];
        let style = css(&boxless, table, true);
        self.out.push_str("<td");
        if colspan > 1 {
            let _ = write!(self.out, " colspan=\"{colspan}\"");
        }
        if rowspan > 1 {
            let _ = write!(self.out, " rowspan=\"{rowspan}\"");
        }
        self.style_attr(&style);
        self.out.push('>');
        self.content(cell, &cs);
        self.out.push_str("</td>");
    }

    fn collect_rows(
        &self,
        n: NodeId,
        header: bool,
        rows: &mut Vec<Row>,
        widths: &mut Vec<f32>,
        prelude: &mut Vec<NodeId>,
    ) {
        let doc = self.doc();
        for c in doc.element_children(n) {
            let cs = self.style(c);
            let lname = doc.element(c).map(|e| e.local_name()).unwrap_or_default();
            if lname == "colspec" {
                widths.push(
                    doc.attr(c, "colwidth")
                        .and_then(parse_colwidth)
                        .unwrap_or(1.0),
                );
            } else if cs.display == Display::TableRow || matches!(lname, "row" | "tr") {
                let cells = doc
                    .element_children(c)
                    .filter(|&x| self.style(x).display != Display::None)
                    .collect();
                rows.push(Row {
                    node: c,
                    cells,
                    header,
                });
            } else if cs.display == Display::None {
            } else if matches!(
                cs.display,
                Display::TableRowGroup | Display::TableHeaderGroup | Display::Table
            ) || matches!(lname, "tgroup" | "thead" | "tbody" | "tfoot")
            {
                let head = header
                    || lname == "thead"
                    || (cs.display == Display::TableHeaderGroup && lname != "tfoot");
                self.collect_rows(c, head, rows, widths, prelude);
            } else {
                prelude.push(c);
            }
        }
    }

    fn colspan_total(&self, cells: &[NodeId]) -> usize {
        cells
            .iter()
            .map(|&c| self.attr_usize(c, "colspan").unwrap_or(1).max(1))
            .sum()
    }

    fn attr_usize(&self, n: NodeId, name: &str) -> Option<usize> {
        self.doc().attr(n, name).and_then(|v| v.trim().parse().ok())
    }

    /// Column ranges given by CALS `namest`/`nameend` or `colname`.
    fn explicit_columns(
        &self,
        row: NodeId,
        cells: &[NodeId],
        ncols: usize,
    ) -> Vec<Option<(usize, usize)>> {
        let doc = self.doc();
        let group = doc
            .ancestors(row)
            .find(|&a| doc.element(a).is_some_and(|e| e.local_name() == "tgroup"));
        let names: Vec<String> = group
            .map(|g| {
                doc.element_children(g)
                    .filter(|&c| doc.element(c).is_some_and(|e| e.local_name() == "colspec"))
                    .enumerate()
                    .map(|(i, c)| {
                        doc.attr(c, "colname")
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("col{}", i + 1))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let col = |name: Option<&str>| name.and_then(|n| names.iter().position(|c| c == n));
        cells
            .iter()
            .map(|&c| {
                let start = col(doc.attr(c, "namest")).or_else(|| col(doc.attr(c, "colname")))?;
                let end = col(doc.attr(c, "nameend")).unwrap_or(start).max(start);
                Some((start.min(ncols - 1), end.min(ncols - 1)))
            })
            .collect()
    }
}

struct Row {
    node: NodeId,
    cells: Vec<NodeId>,
    header: bool,
}

/// A title-like block that should stay with what follows it.
fn is_heading(doc: &Document, n: NodeId, s: &Sty, parent: &Sty) -> bool {
    let name = doc
        .element(n)
        .map(|e| e.local_name().to_ascii_lowercase())
        .unwrap_or_default();
    matches!(
        name.as_str(),
        "title"
            | "heading"
            | "head"
            | "techname"
            | "infoname"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
    ) || (s.bold && s.font_size > parent.font_size * 1.1)
}

fn family(s: &Sty) -> &'static str {
    if s.monospace {
        "Consolas, \"DejaVu Sans Mono\", \"Liberation Mono\", monospace"
    } else if s.serif {
        "\"Times New Roman\", \"DejaVu Serif\", \"Liberation Serif\", serif"
    } else {
        "\"Segoe UI\", Arial, \"DejaVu Sans\", \"Liberation Sans\", sans-serif"
    }
}

/// CSS declarations for `s` inside an element styled `parent`. Inherited
/// properties are written only where they change; box properties always.
fn css(s: &Sty, parent: &Sty, block: bool) -> String {
    let mut c = String::new();
    if family(s) != family(parent) {
        let _ = write!(c, "font-family:{};", family(s));
    }
    if (s.font_size - parent.font_size).abs() > 0.01 {
        let _ = write!(c, "font-size:{}px;", num(s.font_size));
    }
    if s.bold != parent.bold {
        c.push_str(if s.bold {
            "font-weight:bold;"
        } else {
            "font-weight:normal;"
        });
    }
    if s.italic != parent.italic {
        c.push_str(if s.italic {
            "font-style:italic;"
        } else {
            "font-style:normal;"
        });
    }
    let color = s.color.unwrap_or(DEFAULT_COLOR);
    if color != parent.color.unwrap_or(DEFAULT_COLOR) {
        let _ = write!(c, "color:{};", hex(color));
    }
    let lines = match (s.underline, s.line_through) {
        (true, true) => Some("underline line-through"),
        (true, false) => Some("underline"),
        (false, true) => Some("line-through"),
        (false, false) => None,
    };
    if (s.underline, s.line_through) != (parent.underline, parent.line_through)
        && let Some(l) = lines
    {
        let _ = write!(c, "text-decoration:{l};");
    }
    if s.text_transform != parent.text_transform {
        let t = match s.text_transform {
            TextTransform::None => "none",
            TextTransform::Uppercase => "uppercase",
            TextTransform::Lowercase => "lowercase",
            TextTransform::Capitalize => "capitalize",
        };
        let _ = write!(c, "text-transform:{t};");
    }
    if s.white_space != parent.white_space {
        let w = match s.white_space {
            WhiteSpace::Normal => "normal",
            WhiteSpace::Pre => "pre",
            WhiteSpace::NoWrap => "nowrap",
            WhiteSpace::PreWrap => "pre-wrap",
        };
        let _ = write!(c, "white-space:{w};");
    }
    if s.line_height != parent.line_height {
        match s.line_height {
            Some(l) => {
                let _ = write!(c, "line-height:{}px;", num(l));
            }
            None => c.push_str("line-height:normal;"),
        }
    }
    match s.vertical_align {
        VerticalAlign::Super => c.push_str("vertical-align:super;"),
        VerticalAlign::Sub => c.push_str("vertical-align:sub;"),
        VerticalAlign::Baseline => {}
    }
    if block {
        if s.text_align != parent.text_align {
            let a = match s.text_align {
                TextAlign::Left => "left",
                TextAlign::Center => "center",
                TextAlign::Right => "right",
                TextAlign::Justify => "justify",
            };
            let _ = write!(c, "text-align:{a};");
        }
        if (s.text_indent - parent.text_indent).abs() > 0.01 {
            let _ = write!(c, "text-indent:{}px;", num(s.text_indent));
        }
        if let Some(bg) = s.background {
            let _ = write!(c, "background-color:{};", hex(bg));
        }
        if s.margin.iter().any(|&v| v != 0.0) {
            let _ = write!(c, "margin:{};", sides(&s.margin));
        }
        if s.padding.iter().any(|&v| v != 0.0) {
            let _ = write!(c, "padding:{};", sides(&s.padding));
        }
        for (i, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
            if s.border_width[i] > 0.0 {
                // Unset border colours are grey in the editor.
                let color = s.border_color[i].unwrap_or(Color32::GRAY);
                let _ = write!(
                    c,
                    "border-{side}:{}px solid {};",
                    num(s.border_width[i]),
                    hex(color)
                );
            }
        }
    } else if let Some(bg) = s.background_inline {
        let _ = write!(c, "background-color:{};", hex(bg));
    }
    c
}

fn sides(v: &[f32; 4]) -> String {
    v.iter()
        .map(|&x| format!("{}px", num(x)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A number without needless decimals.
fn num(v: f32) -> String {
    let r = (v * 100.0).round() / 100.0;
    if r == r.trunc() {
        format!("{}", r as i64)
    } else {
        format!("{r}")
    }
}

fn hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("rgba({r},{g},{b},{})", num(a as f32 / 255.0))
    }
}

fn escape(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape(s, &mut out);
    out
}

/// Embeds an image file. TIFF, which browsers cannot show, is converted to
/// PNG; formats a browser cannot show and we cannot convert (CGM) give
/// `None`. The size is the editor's: natural size, at most 480 px wide.
fn make_data_uri(path: &Path) -> Option<Embedded> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let size = image::image_dimensions(path).ok().map(|(w, h)| {
        let scale = (MAX_IMAGE_WIDTH / w as f32).min(1.0);
        (w as f32 * scale, h as f32 * scale)
    });
    let (mime, bytes) = match ext.as_str() {
        "png" => ("image/png", std::fs::read(path).ok()?),
        "jpg" | "jpeg" => ("image/jpeg", std::fs::read(path).ok()?),
        "gif" => ("image/gif", std::fs::read(path).ok()?),
        "bmp" => ("image/bmp", std::fs::read(path).ok()?),
        "webp" => ("image/webp", std::fs::read(path).ok()?),
        "svg" => ("image/svg+xml", std::fs::read(path).ok()?),
        "tif" | "tiff" => {
            let img = image::open(path).ok()?;
            let mut png = std::io::Cursor::new(Vec::new());
            img.write_to(&mut png, image::ImageFormat::Png).ok()?;
            ("image/png", png.into_inner())
        }
        _ => return None,
    };
    Some((format!("data:{mime};base64,{}", base64(&bytes)), size))
}

/// Standard base64 with padding.
fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A file name for a document title that is safe to hand to a shell:
/// letters, digits, `-` and `_` only.
pub fn safe_file_stem(title: &str) -> String {
    let s: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(80)
        .collect();
    if s.is_empty() { "document".into() } else { s }
}

/// Opens a file with the system's default program (the web browser for
/// HTML), without waiting for it.
pub fn open_in_browser(path: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        // raw_arg: cmd.exe parses its own command line and does not
        // understand the escaping Rust applies to quoted arguments. The
        // empty title keeps `start` from taking the path as a window title.
        std::process::Command::new("cmd")
            .arg("/c")
            .raw_arg(format!("start \"\" \"{}\"", path.display()))
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::style::CssStyles;

    const CSS: &str = r#"
        doc { display: block; font-size: 12pt; counter-reset: sec; }
        title { display: block; font-size: 18pt; font-weight: bold; color: #1b3a6b; margin-bottom: 6px; }
        sec { display: block; counter-increment: sec; }
        sec > title:before { content: counter(sec) ". "; }
        p { display: block; margin: 4px 0; }
        b { font-weight: bold; }
        hidden { display: none; }
        list { display: block; margin-left: 20px; }
        item { display: list-item; list-style-type: decimal; }
        note { display: block; border: 2px solid #c0392b; background-color: #fdecea; padding: 6px; }
        note:after { content: " (end of note)"; color: gray; }
        table { display: table; }
        table > title { display: block; }
        tgroup, tbody, thead { display: table-row-group; }
        colspec { display: none; }
        row { display: table-row; }
        entry { display: table-cell; }
        thead entry { font-weight: bold; background-color: #e8eef7; }
    "#;

    const DOC: &str = r#"<?xml version="1.0"?>
<doc>
  <!-- a reviewer's comment -->
  <title>Lift &amp; Pump</title>
  <sec><title>Safety</title>
    <p>Use the <b>red</b> button.<?xm-replace_text Type here?></p>
    <hidden>secret</hidden>
    <note><p>Mind the gap.</p></note>
    <list><item><p>One</p></item><item><p>Two</p></item></list>
    <table><title>Intervals</title>
      <tgroup cols="3">
        <colspec colname="c1" colwidth="2*"/><colspec colname="c2"/><colspec colname="c3"/>
        <thead><row><entry>Item</entry><entry>When</entry><entry>Do</entry></row></thead>
        <tbody>
          <row><entry morerows="1">Arms</entry><entry>Weekly</entry><entry>Oil</entry></row>
          <row><entry colname="c3">Replace pins</entry></row>
          <row><entry namest="c1" nameend="c3">Log it.</entry></row>
        </tbody>
      </tgroup>
    </table>
  </sec>
</doc>
"#;

    fn export(doc_src: &str, images: &HashMap<NodeId, ImageRef>) -> String {
        let doc = Document::from_str(doc_src).unwrap();
        let sheet = xylem_style::Stylesheet::parse(CSS);
        let engine = xylem_style::StyleEngine::new(sheet);
        let styles = CssStyles::compute(&doc, &engine, None);
        to_html(&ExportInput {
            doc: &doc,
            styles: &styles,
            rules: &xylem_edit::NoRules,
            images,
            title: "Test <manual>",
        })
    }

    #[test]
    fn page_has_the_styled_text_and_nothing_hidden() {
        let html = export(DOC, &HashMap::new());
        assert!(html.starts_with("<!DOCTYPE html>"));
        assert!(html.contains("<title>Test &lt;manual&gt;</title>"));
        assert!(html.contains("@page"));
        assert!(html.contains("Lift &amp; Pump"));
        assert!(html.contains("font-weight:bold"));
        assert!(html.contains("color:#1b3a6b"));
        assert!(html.contains("font-size:24px"), "18pt");
        // Counter from ::before.
        assert!(html.contains(">1. Safety</div>"));
        assert!(html.contains("Use the <span style=\"font-weight:bold;\">red</span> button."));
        assert!(!html.contains("secret"), "display: none");
        assert!(!html.contains("reviewer"), "comments are not printed");
        assert!(
            !html.contains("Type here"),
            "nor are processing instructions"
        );
        assert!(html.contains("(end of note)"));
        assert!(html.contains("border-top:2px solid #c0392b"));
        assert!(html.contains("background-color:#fdecea"));
        assert!(html.contains("padding:6px 6px 6px 6px"));
    }

    #[test]
    fn list_items_get_their_markers() {
        let html = export(DOC, &HashMap::new());
        assert!(html.contains("class=\"x-item\""));
        assert!(html.contains(">1.</span>"));
        assert!(html.contains(">2.</span>"));
    }

    #[test]
    fn cals_tables_keep_spans_and_widths() {
        let html = export(DOC, &HashMap::new());
        let table = &html[html.find("<table>").unwrap()..html.find("</table>").unwrap()];
        assert!(html.find("Intervals").unwrap() < html.find("<table>").unwrap());
        assert!(table.contains(
            "<col style=\"width:50%\"><col style=\"width:25%\"><col style=\"width:25%\">"
        ));
        assert!(table.contains("<thead>"));
        assert!(table.contains("background-color:#e8eef7"));
        assert!(table.contains("<td rowspan=\"2\">Arms</td>"));
        // Column 1 is covered by "Arms", column 2 is skipped: one filler.
        assert!(
            table.contains("<tr><td></td><td>Replace pins</td></tr>"),
            "{table}"
        );
        assert!(table.contains("<td colspan=\"3\">Log it.</td>"));
    }

    #[test]
    fn html_tables_use_colspan_and_rowspan() {
        let src = "<doc><table><tr><td colspan=\"2\">a</td></tr><tr><td rowspan=\"2\">b</td><td>c</td></tr><tr><td>d</td></tr></table></doc>";
        let html = export(src, &HashMap::new());
        assert!(html.contains("<td colspan=\"2\">a</td>"));
        assert!(html.contains("<td rowspan=\"2\">b</td><td>c</td>"));
        assert!(html.contains("<tr><td>d</td></tr>"));
    }

    #[test]
    fn images_are_embedded_or_labelled() {
        let dir = std::env::temp_dir().join(format!("xylem-html-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("dot.png");
        image::RgbaImage::from_pixel(600, 300, image::Rgba([200, 0, 0, 255]))
            .save(&png)
            .unwrap();
        let src = "<doc><p>x</p><p>y</p></doc>";
        let doc = Document::from_str(src).unwrap();
        let root = doc.document_element().unwrap();
        let mut ps = doc.element_children(root);
        let (a, b) = (ps.next().unwrap(), ps.next().unwrap());
        let mut images = HashMap::new();
        images.insert(
            a,
            ImageRef {
                src: "dot.png".into(),
                path: Some(png.clone()),
            },
        );
        images.insert(
            b,
            ImageRef {
                src: "fig/ICN-1.cgm".into(),
                path: None,
            },
        );
        let html = export(src, &images);
        assert!(html.contains("<img src=\"data:image/png;base64,iVBORw0KGgo"));
        assert!(
            html.contains("width:480px;height:240px;"),
            "scaled like the editor"
        );
        assert!(html.contains("<span class=\"x-noimage\">image: ICN-1.cgm</span>"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn file_stems_are_shell_safe() {
        assert_eq!(safe_file_stem("a b&c\"d.xml"), "a_b_c_d_xml");
        assert_eq!(safe_file_stem(""), "document");
    }
}
