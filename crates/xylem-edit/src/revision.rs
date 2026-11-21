//! Tracked changes and review comments, stored the way XMetaL stores them.
//!
//! XMetaL keeps revisions as processing instructions inside the document, so
//! a reviewed file is still plain XML that validates (processing instructions
//! are outside the content model) and round-trips with XMetaL users:
//!
//! ```text
//! <?xm-insertion_mark_start author="ann" time="20261121T101500+0000"?>new text<?xm-insertion_mark_end?>
//! <?xm-deletion_mark author="ann" time="20261121T101500+0000" data="old &amp;lt;b&amp;gt;words&amp;lt;/b&amp;gt;"?>
//! <?xm-annotation author="ann" time="20261121T101500+0000" data="Check this value"?>
//! <?xm-annotation_start author="ann" time="..." data="Check this"?>commented text<?xm-annotation_end?>
//! ```
//!
//! There was no file written by XMetaL, and no XMetaL documentation, to
//! check against when this was built. The instruction names and attributes
//! come from a description of XMetaL's markup given with the task, so treat
//! them as unconfirmed. Reading is lenient: either quote style, any
//! attribute order, and attributes we don't know are kept when a mark is
//! rewritten. Writing always uses the form above.
//!
//! The `data` pseudo-attribute of a deletion mark holds the deleted content
//! as an XML fragment, escaped once more to fit in the attribute. That keeps
//! deleted markup (a `<b>` inside a deleted phrase) so that rejecting the
//! deletion restores it exactly. Times are `YYYYMMDDThhmmss±hhmm`.
//!
//! Only text is tracked. Splitting or joining paragraphs and inserting or
//! removing whole elements are ordinary, untracked edits.

use crate::pos::{
    Pos, Selection, boundary, canonical, canonical_left, cmp_pos, container, is_ignorable,
};
use crate::{EditError, EditResult, Editor, Rules};
use std::cmp::Ordering;
use xylem_dom::{Document, NodeId, NodeKind};

/// Start of a tracked insertion.
pub const INSERTION_START: &str = "xm-insertion_mark_start";
/// End of a tracked insertion.
pub const INSERTION_END: &str = "xm-insertion_mark_end";
/// A tracked deletion; the deleted content is in its `data`.
pub const DELETION: &str = "xm-deletion_mark";
/// A comment attached to a point in the text.
pub const COMMENT: &str = "xm-annotation";
/// Start of a comment attached to a range of text.
pub const COMMENT_START: &str = "xm-annotation_start";
/// End of a commented range.
pub const COMMENT_END: &str = "xm-annotation_end";

/// The revision markers this module understands.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MarkKind {
    InsertionStart,
    InsertionEnd,
    Deletion,
    Comment,
    CommentStart,
    CommentEnd,
}

impl MarkKind {
    fn from_target(target: &str) -> Option<Self> {
        Some(match target {
            INSERTION_START => Self::InsertionStart,
            INSERTION_END => Self::InsertionEnd,
            DELETION => Self::Deletion,
            COMMENT => Self::Comment,
            COMMENT_START => Self::CommentStart,
            COMMENT_END => Self::CommentEnd,
            _ => return None,
        })
    }

    /// Marks that only bound a range and have no appearance of their own.
    /// The caret passes over them as if they were not there.
    pub fn is_hidden(self) -> bool {
        matches!(
            self,
            Self::InsertionStart | Self::InsertionEnd | Self::CommentEnd
        )
    }

    /// Marks that come in start/end pairs.
    pub fn is_paired(self) -> bool {
        matches!(
            self,
            Self::InsertionStart | Self::InsertionEnd | Self::CommentStart | Self::CommentEnd
        )
    }
}

/// The kind of revision marker `n` is, if it is one.
pub fn mark_kind(doc: &Document, n: NodeId) -> Option<MarkKind> {
    match doc.kind(n) {
        NodeKind::Pi { target, .. } => MarkKind::from_target(target),
        _ => None,
    }
}

/// True for a marker the caret should not stop at (see [`MarkKind::is_hidden`]).
pub fn is_hidden_mark(doc: &Document, n: NodeId) -> bool {
    mark_kind(doc, n).is_some_and(MarkKind::is_hidden)
}

/// True for any revision or comment marker.
pub fn is_mark(doc: &Document, n: NodeId) -> bool {
    mark_kind(doc, n).is_some()
}

// ------------------------------------------------------------ pseudo-attributes

/// The `name="value"` pairs in a processing instruction's data.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PiAttrs(pub Vec<(String, String)>);

impl PiAttrs {
    /// Parses pseudo-attributes, stopping quietly at the first thing that
    /// does not look like one (XMetaL never writes anything else there).
    pub fn parse(data: &str) -> Self {
        let mut out = Vec::new();
        let mut rest = data;
        loop {
            rest = rest.trim_start();
            let Some(eq) = rest.find('=') else { break };
            let name = rest[..eq].trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                break;
            }
            let after = rest[eq + 1..].trim_start();
            let Some(quote) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else {
                break;
            };
            let body = &after[1..];
            let Some(close) = body.find(quote) else { break };
            out.push((name.to_string(), unescape(&body[..close])));
            rest = &body[close + 1..];
        }
        Self(out)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some((_, v)) => *v = value,
            None => self.0.push((name.to_string(), value)),
        }
    }

    /// Serialises the pairs with double quotes. `>` is escaped as well so the
    /// data can never contain the `?>` that would end the instruction.
    pub fn to_data(&self) -> String {
        let mut s = String::new();
        for (i, (k, v)) in self.0.iter().enumerate() {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(k);
            s.push_str("=\"");
            escape_value(v, &mut s);
            s.push('"');
        }
        s
    }
}

fn escape_value(v: &str, out: &mut String) {
    for ch in v.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(ch),
        }
    }
}

fn unescape(v: &str) -> String {
    let mut out = String::with_capacity(v.len());
    let mut rest = v;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let Some(semi) = rest.find(';') else { break };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The pseudo-attributes of a marker (empty for other nodes).
pub fn mark_attrs(doc: &Document, n: NodeId) -> PiAttrs {
    match doc.kind(n) {
        NodeKind::Pi { data, .. } => PiAttrs::parse(data),
        _ => PiAttrs::default(),
    }
}

/// Who made the change a marker belongs to. An end marker answers for its start.
pub fn mark_author(doc: &Document, n: NodeId) -> Option<String> {
    let owner = match mark_kind(doc, n)? {
        MarkKind::InsertionEnd | MarkKind::CommentEnd => partner(doc, n)?,
        _ => n,
    };
    mark_attrs(doc, owner).get("author").map(str::to_string)
}

fn new_mark(doc: &mut Document, target: &str, author: &str, time: &str) -> NodeId {
    let attrs = PiAttrs(vec![
        ("author".into(), author.into()),
        ("time".into(), time.into()),
    ]);
    doc.create_node(NodeKind::Pi {
        target: target.into(),
        data: attrs.to_data(),
    })
}

fn new_end(doc: &mut Document, target: &str) -> NodeId {
    doc.create_node(NodeKind::Pi {
        target: target.into(),
        data: String::new(),
    })
}

// ------------------------------------------------------------ document walking

/// The next node in document order (pre-order), not descending into `n`.
fn next_in_order(doc: &Document, n: NodeId) -> Option<NodeId> {
    let mut cur = n;
    loop {
        if let Some(s) = doc.next_sibling(cur) {
            return Some(s);
        }
        cur = doc.parent(cur)?;
    }
}

/// The next node in document order, descending into `n` first.
fn next_node(doc: &Document, n: NodeId) -> Option<NodeId> {
    doc.children(n)
        .first()
        .copied()
        .or_else(|| next_in_order(doc, n))
}

/// The previous node in document order.
fn prev_node(doc: &Document, n: NodeId) -> Option<NodeId> {
    match doc.prev_sibling(n) {
        Some(mut s) => {
            while let Some(&last) = doc.children(s).last() {
                s = last;
            }
            Some(s)
        }
        None => doc.parent(n),
    }
}

/// The other half of a paired marker: the end for a start and the start for
/// an end. Pairs don't nest, so it is the nearest matching marker; another
/// marker of the same half in between means this one is unmatched.
pub fn partner(doc: &Document, n: NodeId) -> Option<NodeId> {
    let kind = mark_kind(doc, n)?;
    let (want, same, forward) = match kind {
        MarkKind::InsertionStart => (MarkKind::InsertionEnd, kind, true),
        MarkKind::InsertionEnd => (MarkKind::InsertionStart, kind, false),
        MarkKind::CommentStart => (MarkKind::CommentEnd, kind, true),
        MarkKind::CommentEnd => (MarkKind::CommentStart, kind, false),
        _ => return None,
    };
    let mut cur = n;
    loop {
        cur = if forward {
            next_node(doc, cur)?
        } else {
            prev_node(doc, cur)?
        };
        match mark_kind(doc, cur) {
            Some(k) if k == want => return Some(cur),
            Some(k) if k == same => return None,
            _ => {}
        }
    }
}

/// The insertion start marker whose range contains `p`, if any: the nearest
/// insertion mark before `p` in document order is a start. This agrees with
/// [`partner`], so a range that crosses element boundaries (after a
/// paragraph split, or in a file from elsewhere) still counts as one.
pub fn enclosing_insertion(doc: &Document, p: Pos) -> Option<NodeId> {
    let (mut parent, mut idx) = match doc.parent(p.node) {
        Some(par) if doc.is_text(p.node) => (par, doc.index_in_parent(p.node)?),
        _ => (p.node, p.offset),
    };
    loop {
        let kids = doc.children(parent);
        for &c in kids[..idx.min(kids.len())].iter().rev() {
            if let Some(m) = last_insertion_mark(doc, c) {
                return (mark_kind(doc, m) == Some(MarkKind::InsertionStart)).then_some(m);
            }
        }
        let gp = doc.parent(parent)?;
        idx = doc.index_in_parent(parent)?;
        parent = gp;
    }
}

/// The last insertion start or end mark in `n`'s subtree, in document order.
fn last_insertion_mark(doc: &Document, n: NodeId) -> Option<NodeId> {
    if matches!(
        mark_kind(doc, n),
        Some(MarkKind::InsertionStart | MarkKind::InsertionEnd)
    ) {
        return Some(n);
    }
    doc.children(n)
        .iter()
        .rev()
        .find_map(|&c| last_insertion_mark(doc, c))
}

/// True if `author` has any insertion in the document. Bulk deletions use it
/// to skip looking for one around every node they visit.
fn has_insertion_by(doc: &Document, author: &str) -> bool {
    doc.descendants(doc.root()).any(|n| {
        mark_kind(doc, n) == Some(MarkKind::InsertionStart)
            && mark_attrs(doc, n).get("author") == Some(author)
    })
}

fn is_empty_text(doc: &Document, n: NodeId) -> bool {
    doc.text(n).is_some_and(str::is_empty)
}

/// The sibling just before `parent[idx]`, ignoring empty text nodes.
fn sibling_before(doc: &Document, parent: NodeId, idx: usize) -> Option<NodeId> {
    let kids = doc.children(parent);
    kids[..idx.min(kids.len())]
        .iter()
        .rev()
        .copied()
        .find(|&c| !is_empty_text(doc, c))
}

/// The sibling at or after `parent[idx]`, ignoring empty text nodes.
fn sibling_after(doc: &Document, parent: NodeId, idx: usize) -> Option<NodeId> {
    let kids = doc.children(parent);
    kids[idx.min(kids.len())..]
        .iter()
        .copied()
        .find(|&c| !is_empty_text(doc, c))
}

fn contains_mark(doc: &Document, n: NodeId) -> bool {
    doc.descendants(n).any(|d| is_mark(doc, d))
}

// ------------------------------------------------------------ listing

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Insertion,
    Deletion,
    Comment,
}

/// One tracked change or comment, in document order.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub kind: ChangeKind,
    /// The start marker (or the only one).
    pub node: NodeId,
    /// The end marker of a range, when it has one.
    pub end: Option<NodeId>,
    pub author: String,
    /// As stored: `YYYYMMDDThhmmss±hhmm`.
    pub time: String,
    /// Inserted or deleted text, or the text a comment is attached to.
    pub excerpt: String,
    /// The comment itself (empty for insertions and deletions).
    pub comment: String,
}

const EXCERPT_CHARS: usize = 200;

/// All changes and comments in the document, in document order. Unmatched
/// start markers are listed with no end so they can still be found and fixed.
pub fn changes(doc: &Document) -> Vec<Change> {
    doc.descendants(doc.root())
        .filter_map(|n| change_of(doc, n))
        .collect()
}

/// The change or comment that marker `n` starts (None for end markers and
/// other nodes).
pub fn change_of(doc: &Document, n: NodeId) -> Option<Change> {
    let kind = mark_kind(doc, n)?;
    let attrs = mark_attrs(doc, n);
    let data = attrs.get("data").unwrap_or_default();
    let (kind, end, excerpt, comment) = match kind {
        MarkKind::InsertionStart => {
            let end = partner(doc, n);
            (ChangeKind::Insertion, end, text_between(doc, n, end), "")
        }
        MarkKind::Deletion => (ChangeKind::Deletion, None, deleted_text(data), ""),
        MarkKind::Comment => (ChangeKind::Comment, None, String::new(), data),
        MarkKind::CommentStart => {
            let end = partner(doc, n);
            (ChangeKind::Comment, end, text_between(doc, n, end), data)
        }
        MarkKind::InsertionEnd | MarkKind::CommentEnd => return None,
    };
    Some(Change {
        kind,
        node: n,
        end,
        author: attrs.get("author").unwrap_or_default().to_string(),
        time: attrs.get("time").unwrap_or_default().to_string(),
        excerpt,
        comment: comment.to_string(),
    })
}

/// The text between two markers, cut short for display.
fn text_between(doc: &Document, start: NodeId, end: Option<NodeId>) -> String {
    let mut s = String::new();
    let mut cur = start;
    while let Some(n) = next_node(doc, cur) {
        if Some(n) == end || s.chars().count() >= EXCERPT_CHARS {
            break;
        }
        if let NodeKind::Text(t) | NodeKind::CData(t) = doc.kind(n) {
            s.push_str(t);
        }
        cur = n;
    }
    s
}

/// The readable text of a deletion's `data` (markup stripped).
pub fn deleted_text(xml: &str) -> String {
    match xylem_dom::parse_fragment(xml) {
        Ok(frag) => frag.text_content(frag.root()),
        Err(_) => xml.to_string(),
    }
}

// ------------------------------------------------------------ time stamps

/// Formats a time as XMetaL writes it: `YYYYMMDDThhmmss±hhmm`, for
/// `unix_seconds` seen from a zone `offset_minutes` east of UTC.
pub fn format_time(unix_seconds: i64, offset_minutes: i32) -> String {
    let local = unix_seconds + i64::from(offset_minutes) * 60;
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let off = offset_minutes.unsigned_abs();
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}{sign}{:02}{:02}",
        secs / 3600,
        secs / 60 % 60,
        secs % 60,
        off / 60,
        off % 60
    )
}

/// Proleptic Gregorian date for a day count since 1970-01-01 (the
/// well-known algorithm from Howard Hinnant's date library).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `20261121T101500+0000` as `2026-11-21 10:15`; anything else unchanged.
pub fn display_time(t: &str) -> String {
    let b = t.as_bytes();
    if t.len() >= 13 && t.is_ascii() && b[8] == b'T' && b[..8].iter().all(u8::is_ascii_digit) {
        format!(
            "{}-{}-{} {}:{}",
            &t[0..4],
            &t[4..6],
            &t[6..8],
            &t[9..11],
            &t[11..13]
        )
    } else {
        t.to_string()
    }
}

// ------------------------------------------------------------ editing

/// Who is editing while changes are tracked, and how to read the clock.
#[derive(Clone)]
pub struct Tracking {
    pub author: String,
    /// Returns the current time in the format of [`format_time`]. Injected
    /// so the editor stays free of clocks and tests stay deterministic.
    pub clock: fn() -> String,
}

impl Editor {
    pub fn is_tracking(&self) -> bool {
        self.tracking.is_some()
    }

    pub(crate) fn stamp(&self) -> (String, String) {
        let t = self.tracking.as_ref().expect("tracking is on");
        (t.author.clone(), (t.clock)())
    }

    fn is_own(&self, mark: NodeId, author: &str) -> bool {
        mark_author(&self.doc, mark).as_deref() == Some(author)
    }

    /// Inserts `text` at `p` as a tracked insertion: into the author's own
    /// insertion when `p` is inside or right next to one, else into a new one.
    pub(crate) fn tracked_insert(
        &mut self,
        rules: &dyn Rules,
        p: Pos,
        text: &str,
    ) -> EditResult<Pos> {
        let (author, time) = self.stamp();
        let p = self.text_insert_target(rules, p)?;
        if let Some(start) = enclosing_insertion(&self.doc, p)
            && self.is_own(start, &author)
        {
            return self.insert_text_at(rules, p, text);
        }
        // Text typed just before deleted text goes after it, so a change
        // reads as old then new (Backspace leaves the caret in front).
        let mut p = p;
        let b = boundary(&self.doc, p);
        if !self.doc.is_text(b.node) {
            let mut idx = b.offset;
            while let Some(n) = sibling_after(&self.doc, b.node, idx)
                && mark_kind(&self.doc, n) == Some(MarkKind::Deletion)
            {
                idx = self.doc.index_in_parent(n).unwrap() + 1;
            }
            if idx != b.offset {
                p = canonical(&self.doc, rules, Pos::new(b.node, idx));
            }
        }
        // Right at the outer edge of an insertion of ours: extend it.
        let b = boundary(&self.doc, p);
        if !self.doc.is_text(b.node) {
            if let Some(prev) = sibling_before(&self.doc, b.node, b.offset)
                && mark_kind(&self.doc, prev) == Some(MarkKind::InsertionEnd)
                && self.is_own(prev, &author)
            {
                let at = Pos::new(b.node, self.doc.index_in_parent(prev).unwrap());
                let at = canonical_left(&self.doc, rules, at);
                return self.insert_text_at(rules, at, text);
            }
            if let Some(next) = sibling_after(&self.doc, b.node, b.offset)
                && mark_kind(&self.doc, next) == Some(MarkKind::InsertionStart)
                && self.is_own(next, &author)
            {
                let at = Pos::new(b.node, self.doc.index_in_parent(next).unwrap() + 1);
                let at = canonical(&self.doc, rules, at);
                return self.insert_text_at(rules, at, text);
            }
        }
        let b = self.split_at_boundary(p);
        let t = self.doc.create_text(text);
        self.doc.insert_child(b.node, b.offset, t);
        self.wrap_tracked(b.node, b.offset, b.offset + 1, &author, &time);
        let caret = Pos::new(t, text.len());
        Ok(self.tidy_keep(rules, b.node, caret))
    }

    /// Marks `parent[start..end]` as inserted by `author`. Inside someone
    /// else's insertion, theirs is closed before and reopened after, since
    /// insertions cannot nest.
    pub(crate) fn wrap_tracked(
        &mut self,
        parent: NodeId,
        start: usize,
        end: usize,
        author: &str,
        time: &str,
    ) {
        let enclosing = enclosing_insertion(&self.doc, Pos::new(parent, start));
        if let Some(s) = enclosing
            && self.is_own(s, author)
        {
            return;
        }
        if let Some(theirs) = enclosing {
            let reopen = self.doc.create_node(self.doc.kind(theirs).clone());
            self.doc.insert_child(parent, end, reopen);
        }
        let close = new_end(&mut self.doc, INSERTION_END);
        self.doc.insert_child(parent, end, close);
        let open = new_mark(&mut self.doc, INSERTION_START, author, time);
        self.doc.insert_child(parent, start, open);
        if enclosing.is_some() {
            let close_theirs = new_end(&mut self.doc, INSERTION_END);
            self.doc.insert_child(parent, start, close_theirs);
        }
    }

    /// Marks a newly inserted element as a tracked insertion (insertion
    /// marks on either side of it, which XML allows anywhere). Rejecting it
    /// removes the element.
    pub(crate) fn track_inserted_element(&mut self, el: NodeId) {
        let (Some(parent), Some(idx)) = (self.doc.parent(el), self.doc.index_in_parent(el)) else {
            return;
        };
        let (author, time) = self.stamp();
        self.wrap_tracked(parent, idx, idx + 1, &author, &time);
    }

    /// Removes element `el` with tracking on: it is replaced by a deletion
    /// mark holding its markup, so rejecting puts it back. An element the
    /// author inserted themselves, or one with no content at all (such as
    /// the empty paragraph Enter leaves behind), is simply removed.
    pub(crate) fn tracked_remove_element(&mut self, rules: &dyn Rules, el: NodeId) -> EditResult {
        let parent = self.doc.parent(el).ok_or(EditError::Nothing)?;
        let idx = self.doc.index_in_parent(el).unwrap();
        let (author, time) = self.stamp();
        let own = enclosing_insertion(&self.doc, Pos::new(parent, idx))
            .is_some_and(|s| self.is_own(s, &author));
        let empty = self.doc.element_children(el).next().is_none()
            && self
                .doc
                .descendants(el)
                .filter_map(|n| self.doc.text(n))
                .all(|t| t.trim().is_empty());
        if own || empty {
            self.doc.detach(el);
            self.sel = Selection::caret(self.normalize_keep(rules, Pos::new(parent, idx)));
            return Ok(());
        }
        let xml = self.doc.node_to_xml(el);
        self.doc.detach(el);
        let mark = self.add_deletion(parent, idx, &xml, &author, &time);
        let after = self.doc.index_in_parent(mark).unwrap() + 1;
        self.sel = Selection::caret(self.normalize_keep(rules, Pos::new(parent, after)));
        Ok(())
    }

    /// Before an element is split at `p`: an insertion running through `p`
    /// is ended there, and a copy of its start mark begins the other side,
    /// so each part keeps a complete pair. Returns the split point, which is
    /// between the two marks. This holds with tracking on or off.
    pub(crate) fn break_insertion_at(&mut self, p: Pos) -> Pos {
        let Some(start) = enclosing_insertion(&self.doc, p) else {
            return p;
        };
        let b = self.split_at_boundary(p);
        let reopen = self.doc.create_node(self.doc.kind(start).clone());
        self.doc.insert_child(b.node, b.offset, reopen);
        let end = new_end(&mut self.doc, INSERTION_END);
        self.doc.insert_child(b.node, b.offset, end);
        Pos::new(b.node, b.offset + 1)
    }

    /// Deletes bytes `s..e` of text node `n` with tracking on. Text the
    /// author inserted is simply removed; anything else is replaced by a
    /// deletion mark, merged into an adjacent one of the same author. The
    /// caret ends up after the mark when `caret_after` (the Delete key), else
    /// before it (Backspace).
    pub(crate) fn tracked_delete_text(
        &mut self,
        rules: &dyn Rules,
        n: NodeId,
        s: usize,
        e: usize,
        caret_after: bool,
    ) -> EditResult<Pos> {
        self.check_writable(rules, n)?;
        let (author, time) = self.stamp();
        if let Some(start) = enclosing_insertion(&self.doc, Pos::new(n, s))
            && self.is_own(start, &author)
        {
            let mut t = self.doc.text(n).unwrap().to_string();
            t.replace_range(s..e, "");
            self.doc.set_text(n, t);
            let caret = self.remove_if_emptied(rules, n, Pos::new(n, s));
            return Ok(caret);
        }
        let parent = self.doc.parent(n).unwrap();
        let deleted = self.doc.text(n).unwrap()[s..e].to_string();
        let len = self.doc.text(n).unwrap().len();
        if e < len {
            self.doc.split_text(n, e);
        }
        let mid = if s > 0 { self.doc.split_text(n, s) } else { n };
        let idx = self.doc.index_in_parent(mid).unwrap();
        self.doc.detach(mid);
        let mut xml = String::new();
        xylem_dom::escape_text(&deleted, &mut xml);
        let mark = self.add_deletion(parent, idx, &xml, &author, &time);
        let mi = self.doc.index_in_parent(mark).unwrap();
        Ok(if caret_after {
            canonical(&self.doc, rules, Pos::new(parent, mi + 1))
        } else {
            canonical_left(&self.doc, rules, Pos::new(parent, mi))
        })
    }

    /// Records deleted XML at `parent[idx]`, extending a neighbouring
    /// deletion mark by the same author when there is one.
    fn add_deletion(
        &mut self,
        parent: NodeId,
        idx: usize,
        xml: &str,
        author: &str,
        time: &str,
    ) -> NodeId {
        let own_deletion = |ed: &Self, m: Option<NodeId>| {
            m.filter(|&m| mark_kind(&ed.doc, m) == Some(MarkKind::Deletion) && ed.is_own(m, author))
        };
        let extend = |ed: &mut Self, m: NodeId, before: bool| {
            let mut attrs = mark_attrs(&ed.doc, m);
            let old = attrs.get("data").unwrap_or_default().to_string();
            let data = if before {
                format!("{xml}{old}")
            } else {
                format!("{old}{xml}")
            };
            attrs.set("data", data);
            ed.doc.set_text(m, attrs.to_data());
            m
        };
        if let Some(m) = own_deletion(self, sibling_before(&self.doc, parent, idx)) {
            return extend(self, m, false);
        }
        if let Some(m) = own_deletion(self, sibling_after(&self.doc, parent, idx)) {
            return extend(self, m, true);
        }
        let mut attrs = PiAttrs(vec![
            ("author".into(), author.into()),
            ("time".into(), time.into()),
        ]);
        attrs.set("data", xml);
        let mark = self.doc.create_node(NodeKind::Pi {
            target: DELETION.into(),
            data: attrs.to_data(),
        });
        self.doc.insert_child(parent, idx, mark);
        mark
    }

    /// Deletes the range `a..b` with tracking on (a selection, cut, or the
    /// text find-and-replace replaces). Returns the caret, after the marks.
    ///
    /// Block elements in the range keep their tags and only their content
    /// is marked deleted; inline elements that lie wholly inside go into the
    /// deletion mark as markup. Where text isn't allowed (between table rows
    /// or list items) nothing is deleted, only the content further in.
    pub(crate) fn tracked_delete_range(
        &mut self,
        rules: &dyn Rules,
        a: Pos,
        b: Pos,
    ) -> EditResult<Pos> {
        if cmp_pos(&self.doc, a, b) != Ordering::Less {
            return Ok(a);
        }
        if a.node == b.node && self.doc.is_text(a.node) {
            return self.tracked_delete_text(rules, a.node, a.offset, b.offset, true);
        }
        let (author, time) = self.stamp();
        let (a, b) = self.split_pair(a, b);
        let after = self.doc.children(b.node).get(b.offset).copied();
        let mut doomed = self.range_nodes(a, b);
        doomed.sort_by(|&x, &y| self.doc.cmp_order(x, y));
        for &d in &doomed {
            self.check_writable(rules, d)?;
            if rules.is_protected(&self.doc, d) {
                return Err(EditError::Protected);
            }
        }
        let own = has_insertion_by(&self.doc, &author);
        let mut touched = vec![a.node, b.node];
        for d in doomed {
            self.track_delete_node(rules, d, (&author, &time, own), &mut touched)?;
        }
        let mut caret = match after {
            Some(n) if self.doc.parent(n) == Some(b.node) => {
                let i = self.doc.index_in_parent(n).unwrap();
                canonical_left(&self.doc, rules, Pos::new(b.node, i))
            }
            _ => {
                let len = self.doc.children(b.node).len();
                canonical_left(&self.doc, rules, Pos::new(b.node, len))
            }
        };
        touched.sort();
        touched.dedup();
        for t in touched {
            if self.doc.parent(t).is_some() {
                caret = self.tidy_keep(rules, t, caret);
            }
        }
        Ok(caret)
    }

    /// One node of a tracked range deletion. `who` is (author, time, whether
    /// the author has any insertion that the node could be part of).
    fn track_delete_node(
        &mut self,
        rules: &dyn Rules,
        d: NodeId,
        who: (&str, &str, bool),
        touched: &mut Vec<NodeId>,
    ) -> EditResult {
        let (author, time, has_own) = who;
        if is_mark(&self.doc, d) || is_ignorable(&self.doc, rules, d) {
            return Ok(());
        }
        let parent = self.doc.parent(d).unwrap();
        let idx = self.doc.index_in_parent(d).unwrap();
        if has_own
            && let Some(s) = enclosing_insertion(&self.doc, Pos::new(parent, idx))
            && self.is_own(s, author)
        {
            self.doc.detach(d);
            touched.push(parent);
            return Ok(());
        }
        // Where text isn't allowed (between table rows, list items, colspecs)
        // everything is structure: keep it and look inside the elements.
        let in_text = rules.can_contain_text(&self.doc, parent);
        if self.doc.is_element(d) {
            if rules.is_read_only(&self.doc, d) || rules.is_non_removable(&self.doc, d) {
                return Err(EditError::ReadOnly);
            }
            if !in_text || rules.is_block(&self.doc, d) || contains_mark(&self.doc, d) {
                for c in self.doc.children(d).to_vec() {
                    self.track_delete_node(rules, c, who, touched)?;
                }
                return Ok(());
            }
        } else if !in_text {
            return Ok(());
        }
        let xml = match self.doc.text(d) {
            Some(t) => {
                let mut s = String::new();
                xylem_dom::escape_text(t, &mut s);
                s
            }
            None => self.doc.node_to_xml(d),
        };
        self.doc.detach(d);
        self.add_deletion(parent, idx, &xml, author, time);
        Ok(())
    }

    /// Cleans up insertion marks among `parent`'s children: drops empty
    /// insertions and joins two insertions by the same author that touch.
    /// Returns `caret` adjusted for the removed nodes.
    pub(crate) fn tidy_keep(&mut self, rules: &dyn Rules, parent: NodeId, caret: Pos) -> Pos {
        let gap = (caret.node == parent).then_some(caret.offset);
        let Some(gap) = self.tidy_marks(parent, gap) else {
            return caret;
        };
        let caret = match gap {
            Some(offset) => Pos::new(parent, offset),
            None => caret,
        };
        if container(&self.doc, caret) == parent {
            self.normalize_keep(rules, caret)
        } else {
            self.doc.normalize(parent);
            caret
        }
    }

    /// The node work of [`Self::tidy_keep`]: returns None when nothing
    /// changed, else the child index `gap` (a position in `parent`) moved
    /// to account for the removed marks. Text is not merged here.
    pub(crate) fn tidy_marks(
        &mut self,
        parent: NodeId,
        gap: Option<usize>,
    ) -> Option<Option<usize>> {
        let kids: Vec<NodeId> = self
            .doc
            .children(parent)
            .iter()
            .copied()
            .filter(|&c| !is_empty_text(&self.doc, c))
            .collect();
        let mut doomed = Vec::new();
        let mut i = 0;
        while i + 1 < kids.len() {
            let (x, y) = (kids[i], kids[i + 1]);
            let pair = match (mark_kind(&self.doc, x), mark_kind(&self.doc, y)) {
                (Some(MarkKind::InsertionStart), Some(MarkKind::InsertionEnd)) => true,
                (Some(MarkKind::InsertionEnd), Some(MarkKind::InsertionStart)) => {
                    let a = mark_author(&self.doc, x);
                    a.is_some() && a == mark_author(&self.doc, y)
                }
                _ => false,
            };
            if pair {
                doomed.extend([x, y]);
                i += 2;
            } else {
                i += 1;
            }
        }
        if doomed.is_empty() {
            return None;
        }
        let mut gap = gap;
        for d in doomed {
            if let Some(g) = gap.as_mut()
                && self.doc.index_in_parent(d).is_some_and(|k| k < *g)
            {
                *g -= 1;
            }
            self.doc.detach(d);
        }
        Some(gap)
    }

    /// Before an untracked deletion of `doomed` (whole nodes): a start or
    /// end mark whose partner survives is kept, so no edit leaves half an
    /// insertion or comment behind. Marks lying directly in the range are
    /// dropped from `doomed`; ones inside a doomed element are moved out in
    /// front of it.
    pub(crate) fn keep_lone_marks(&mut self, doomed: &mut Vec<NodeId>) {
        let inside = |doc: &Document, doomed: &[NodeId], n: NodeId| {
            doomed.iter().any(|&d| doc.is_ancestor_or_self(d, n))
        };
        let lone = |doc: &Document, doomed: &[NodeId], n: NodeId| {
            mark_kind(doc, n).is_some_and(MarkKind::is_paired)
                && !partner(doc, n).is_some_and(|p| inside(doc, doomed, p))
        };
        let mut rescue = Vec::new();
        for &d in doomed.iter() {
            if self.doc.is_element(d) {
                for n in self.doc.descendants(d) {
                    if lone(&self.doc, doomed, n) {
                        rescue.push((d, n));
                    }
                }
            }
        }
        let snapshot = doomed.clone();
        doomed.retain(|&d| !lone(&self.doc, &snapshot, d));
        for (holder, mark) in rescue {
            let parent = self.doc.parent(holder).unwrap();
            let idx = self.doc.index_in_parent(holder).unwrap();
            self.doc.insert_child(parent, idx, mark);
        }
    }

    // -------------------------------------------------------- review

    /// The change or comment at `p`: a deletion or comment marker right
    /// next to it, else an insertion or commented range around it.
    pub fn change_at(&self, p: Pos) -> Option<Change> {
        let doc = &self.doc;
        let p = boundary(doc, p);
        let before = |n: NodeId| {
            let par = doc.parent(n).unwrap();
            Pos::new(par, doc.index_in_parent(n).unwrap())
        };
        let after = |n: NodeId| {
            let mut q = before(n);
            q.offset += 1;
            q
        };
        let within = |lo: Pos, hi: Pos| cmp_pos(doc, lo, p).is_le() && cmp_pos(doc, p, hi).is_le();
        let all = changes(doc);
        let point = all
            .iter()
            .find(|c| c.end.is_none() && within(before(c.node), after(c.node)));
        if let Some(c) = point {
            return Some(c.clone());
        }
        all.into_iter()
            .filter(|c| c.end.is_some())
            .rfind(|c| within(before(c.node), after(c.end.unwrap())))
    }

    /// Accepts the change whose marker is `node` (one undo step).
    pub fn accept_change(&mut self, rules: &dyn Rules, node: NodeId) -> EditResult {
        self.transact(|ed| ed.resolve(rules, node, true))
    }

    /// Rejects the change whose marker is `node` (one undo step).
    pub fn reject_change(&mut self, rules: &dyn Rules, node: NodeId) -> EditResult {
        self.transact(|ed| ed.resolve(rules, node, false))
    }

    /// Accepts (or rejects) every insertion and deletion; comments stay.
    pub fn resolve_all(&mut self, rules: &dyn Rules, accept: bool) -> EditResult {
        let nodes: Vec<NodeId> = changes(&self.doc)
            .into_iter()
            .filter(|c| c.kind != ChangeKind::Comment)
            .map(|c| c.node)
            .collect();
        if nodes.is_empty() {
            return Err(EditError::Nothing);
        }
        self.transact(|ed| {
            // Later changes first: rejecting an insertion takes any deletion
            // inside it along, and that deletion has been dealt with by then.
            let mut nodes = nodes;
            // Rejecting a deleted element brings it back with the changes it
            // held, which then need resolving too; repeat until none are
            // left (bounded, in case a change can't be resolved).
            for _ in 0..32 {
                for n in nodes.into_iter().rev() {
                    if ed.attached(n) {
                        ed.resolve(rules, n, accept)?;
                    }
                }
                nodes = changes(&ed.doc)
                    .into_iter()
                    .filter(|c| c.kind != ChangeKind::Comment)
                    .map(|c| c.node)
                    .collect();
                if nodes.is_empty() {
                    break;
                }
            }
            Ok(())
        })
    }

    fn attached(&self, n: NodeId) -> bool {
        self.doc.ancestors(n).any(|a| a == self.doc.root())
    }

    fn resolve(&mut self, rules: &dyn Rules, node: NodeId, accept: bool) -> EditResult {
        let kind = mark_kind(&self.doc, node).ok_or(EditError::Nothing)?;
        let parent = self.doc.parent(node).ok_or(EditError::Nothing)?;
        self.check_writable(rules, parent)?;
        let idx = self.doc.index_in_parent(node).unwrap();
        match kind {
            MarkKind::InsertionStart | MarkKind::InsertionEnd => {
                let other = partner(&self.doc, node);
                let (start, end) = if kind == MarkKind::InsertionStart {
                    (Some(node), other)
                } else {
                    (other, Some(node))
                };
                if !accept && let (Some(s), Some(e)) = (start, end) {
                    self.remove_between(rules, s, e)?;
                }
                for m in [start, end].into_iter().flatten() {
                    self.doc.detach(m);
                }
            }
            MarkKind::Deletion => {
                if !accept {
                    let xml = mark_attrs(&self.doc, node)
                        .get("data")
                        .unwrap_or_default()
                        .to_string();
                    let nodes = match self.doc.parse_fragment_into(&xml) {
                        Ok(nodes) => nodes,
                        Err(_) => vec![self.doc.create_text(xml)],
                    };
                    for (k, n) in nodes.into_iter().enumerate() {
                        self.doc.insert_child(parent, idx + 1 + k, n);
                    }
                }
                self.doc.detach(node);
            }
            _ => return Err(EditError::Nothing),
        }
        if self.attached(parent) {
            let at = Pos::new(parent, idx.min(self.doc.children(parent).len()));
            let caret = self.normalize_keep(rules, at);
            self.sel = Selection::caret(caret);
        }
        self.fix_selection(rules);
        Ok(())
    }

    /// Removes everything between two markers (not the markers themselves).
    fn remove_between(&mut self, rules: &dyn Rules, s: NodeId, e: NodeId) -> EditResult {
        let ps = self.doc.parent(s).unwrap();
        let pe = self.doc.parent(e).unwrap();
        let is = self.doc.index_in_parent(s).unwrap();
        let ie = self.doc.index_in_parent(e).unwrap();
        if ps == pe {
            for c in Vec::from(&self.doc.children(ps)[is + 1..ie]) {
                self.check_writable(rules, c)?;
                self.doc.detach(c);
            }
            return Ok(());
        }
        self.delete_range(rules, Pos::new(ps, is + 1), Pos::new(pe, ie))
            .map(|_| ())
    }

    /// Adds a comment at the caret, or around the selection.
    pub fn add_comment(
        &mut self,
        rules: &dyn Rules,
        author: &str,
        time: &str,
        text: &str,
    ) -> EditResult<NodeId> {
        self.transact(|ed| {
            let make = |ed: &mut Self, target: &str| {
                let n = new_mark(&mut ed.doc, target, author, time);
                let mut attrs = mark_attrs(&ed.doc, n);
                attrs.set("data", text);
                ed.doc.set_text(n, attrs.to_data());
                n
            };
            if ed.sel.is_caret() {
                let b = ed.split_at_boundary(ed.caret());
                ed.check_writable(rules, b.node)?;
                let n = make(ed, COMMENT);
                ed.doc.insert_child(b.node, b.offset, n);
                ed.sel =
                    Selection::caret(canonical(&ed.doc, rules, Pos::new(b.node, b.offset + 1)));
                return Ok(n);
            }
            let (a, b) = ed.sel.ordered(&ed.doc);
            let (a, b) = ed.split_pair(a, b);
            ed.check_writable(rules, a.node)?;
            ed.check_writable(rules, b.node)?;
            let end = new_end(&mut ed.doc, COMMENT_END);
            ed.doc.insert_child(b.node, b.offset, end);
            let n = make(ed, COMMENT_START);
            ed.doc.insert_child(a.node, a.offset, n);
            let i = ed.doc.index_in_parent(end).unwrap();
            ed.sel = Selection::caret(canonical_left(&ed.doc, rules, Pos::new(b.node, i)));
            Ok(n)
        })
    }

    /// Replaces a comment's text.
    pub fn set_comment_text(&mut self, rules: &dyn Rules, node: NodeId, text: &str) -> EditResult {
        if !matches!(
            mark_kind(&self.doc, node),
            Some(MarkKind::Comment | MarkKind::CommentStart)
        ) {
            return Err(EditError::Nothing);
        }
        self.transact(|ed| {
            ed.check_writable(rules, node)?;
            let mut attrs = mark_attrs(&ed.doc, node);
            attrs.set("data", text);
            ed.doc.set_text(node, attrs.to_data());
            Ok(())
        })
    }

    /// Removes a comment (both markers of a commented range).
    pub fn delete_comment(&mut self, rules: &dyn Rules, node: NodeId) -> EditResult {
        let kind = mark_kind(&self.doc, node);
        if !matches!(kind, Some(MarkKind::Comment | MarkKind::CommentStart)) {
            return Err(EditError::Nothing);
        }
        self.transact(|ed| {
            ed.check_writable(rules, node)?;
            let end = partner(&ed.doc, node);
            let parent = ed.doc.parent(node).unwrap();
            let idx = ed.doc.index_in_parent(node).unwrap();
            for m in std::iter::once(node).chain(end) {
                let p = ed.doc.parent(m);
                ed.doc.detach(m);
                if let Some(p) = p {
                    ed.doc.normalize(p);
                }
            }
            let at = Pos::new(parent, idx.min(ed.doc.children(parent).len()));
            ed.sel = Selection::caret(canonical(&ed.doc, rules, at));
            ed.fix_selection(rules);
            Ok(())
        })
    }
}

/// Prepares a fragment about to be pasted. With tracking on, its own
/// revision marks are dropped (the paste becomes one new insertion and the
/// text it had deleted stays deleted); either way, half pairs are dropped.
pub fn clean_pasted_fragment(frag: &mut Document, tracking: bool) {
    let marks: Vec<NodeId> = frag
        .descendants(frag.root())
        .filter(|&n| is_mark(frag, n))
        .collect();
    for m in marks {
        let kind = mark_kind(frag, m).unwrap();
        let drop = if tracking
            && matches!(
                kind,
                MarkKind::InsertionStart | MarkKind::InsertionEnd | MarkKind::Deletion
            ) {
            true
        } else {
            kind.is_paired() && partner(frag, m).is_none()
        };
        if drop {
            frag.detach(m);
        }
    }
}

#[cfg(test)]
#[path = "revision_tests.rs"]
mod tests;
