//! Caret positions and the logical token stream used for caret movement.
//!
//! A [`Pos`] is a DOM boundary point: inside a text node it is a byte offset,
//! inside an element (or the document node) it is a child index.

use crate::Rules;
use std::cmp::Ordering;
use xylem_dom::{Document, NodeId, NodeKind};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pos {
    pub node: NodeId,
    pub offset: usize,
}

impl Pos {
    pub fn new(node: NodeId, offset: usize) -> Self {
        Self { node, offset }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Pos,
    pub focus: Pos,
}

impl Selection {
    pub fn caret(p: Pos) -> Self {
        Self {
            anchor: p,
            focus: p,
        }
    }
    pub fn is_caret(&self) -> bool {
        self.anchor == self.focus
    }
    /// Returns (start, end) in document order.
    pub fn ordered(&self, doc: &Document) -> (Pos, Pos) {
        if cmp_pos(doc, self.anchor, self.focus) == Ordering::Greater {
            (self.focus, self.anchor)
        } else {
            (self.anchor, self.focus)
        }
    }
}

/// One step of the logical content stream.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Start(NodeId),
    End(NodeId),
    /// A character in a text node: (node, byte offset, byte length).
    Char(NodeId, usize, usize),
    /// An atomic node: comment, PI, entity ref, CDATA (edited via dialogs).
    Atom(NodeId),
}

/// Whitespace-only text that is formatting rather than content.
pub fn is_ignorable(doc: &Document, rules: &dyn Rules, n: NodeId) -> bool {
    match doc.kind(n) {
        NodeKind::Text(t) if t.chars().all(char::is_whitespace) => {
            let Some(p) = doc.parent(n) else { return true };
            if !doc.is_element(p) {
                return true;
            }
            if rules.is_preformatted(doc, p) {
                return false;
            }
            rules.is_element_only(doc, p) || t.contains('\n') || t.is_empty()
        }
        _ => false,
    }
}

fn is_caret_text(doc: &Document, rules: &dyn Rules, n: NodeId) -> bool {
    matches!(doc.kind(n), NodeKind::Text(_)) && !is_ignorable(doc, rules, n)
}

/// Puts a position in canonical form (text positions preferred).
pub fn canonical(doc: &Document, rules: &dyn Rules, p: Pos) -> Pos {
    if doc.is_text(p.node) {
        return p;
    }
    let ch = doc.children(p.node);
    if let Some(&c) = ch.get(p.offset)
        && is_caret_text(doc, rules, c)
    {
        return Pos::new(c, 0);
    }
    if p.offset > 0
        && let Some(&c) = ch.get(p.offset - 1)
        && is_caret_text(doc, rules, c)
    {
        return Pos::new(c, doc.text(c).unwrap().len());
    }
    p
}

/// Converts a position to an element-level boundary point (never inside text
/// unless strictly in the middle of it).
pub fn boundary(doc: &Document, p: Pos) -> Pos {
    if let Some(t) = doc.text(p.node) {
        let parent = doc.parent(p.node).unwrap();
        let i = doc.index_in_parent(p.node).unwrap();
        if p.offset == 0 {
            return Pos::new(parent, i);
        }
        if p.offset >= t.len() {
            return Pos::new(parent, i + 1);
        }
    }
    p
}

fn first_token(doc: &Document, rules: &dyn Rules, n: NodeId) -> Option<Token> {
    match doc.kind(n) {
        NodeKind::Element(_) => Some(Token::Start(n)),
        NodeKind::Text(t) => {
            if is_ignorable(doc, rules, n) || t.is_empty() {
                token_after_node(doc, rules, n)
            } else {
                let l = t.chars().next().unwrap().len_utf8();
                Some(Token::Char(n, 0, l))
            }
        }
        _ => Some(Token::Atom(n)),
    }
}

fn last_token(doc: &Document, rules: &dyn Rules, n: NodeId) -> Option<Token> {
    match doc.kind(n) {
        NodeKind::Element(_) => Some(Token::End(n)),
        NodeKind::Text(t) => {
            if is_ignorable(doc, rules, n) || t.is_empty() {
                token_before_node(doc, rules, n)
            } else {
                let c = t.chars().next_back().unwrap();
                Some(Token::Char(n, t.len() - c.len_utf8(), c.len_utf8()))
            }
        }
        _ => Some(Token::Atom(n)),
    }
}

fn token_after_node(doc: &Document, rules: &dyn Rules, n: NodeId) -> Option<Token> {
    if let Some(s) = doc.next_sibling(n) {
        return first_token(doc, rules, s);
    }
    let p = doc.parent(n)?;
    if doc.is_element(p) {
        Some(Token::End(p))
    } else {
        None
    }
}

fn token_before_node(doc: &Document, rules: &dyn Rules, n: NodeId) -> Option<Token> {
    if let Some(s) = doc.prev_sibling(n) {
        return last_token(doc, rules, s);
    }
    let p = doc.parent(n)?;
    if doc.is_element(p) {
        Some(Token::Start(p))
    } else {
        None
    }
}

/// Token immediately after a position.
pub fn token_after(doc: &Document, rules: &dyn Rules, p: Pos) -> Option<Token> {
    if let Some(t) = doc.text(p.node) {
        if p.offset < t.len() {
            let l = t[p.offset..].chars().next().unwrap().len_utf8();
            return Some(Token::Char(p.node, p.offset, l));
        }
        return token_after_node(doc, rules, p.node);
    }
    match doc.children(p.node).get(p.offset) {
        Some(&c) => first_token(doc, rules, c),
        None if doc.is_element(p.node) => Some(Token::End(p.node)),
        None => None,
    }
}

/// Token immediately before a position.
pub fn token_before(doc: &Document, rules: &dyn Rules, p: Pos) -> Option<Token> {
    if let Some(t) = doc.text(p.node) {
        if p.offset > 0 {
            let c = t[..p.offset].chars().next_back().unwrap();
            return Some(Token::Char(p.node, p.offset - c.len_utf8(), c.len_utf8()));
        }
        return token_before_node(doc, rules, p.node);
    }
    if p.offset == 0 {
        return if doc.is_element(p.node) {
            Some(Token::Start(p.node))
        } else {
            None
        };
    }
    let c = doc.children(p.node)[p.offset - 1];
    last_token(doc, rules, c)
}

pub fn pos_after_token(doc: &Document, rules: &dyn Rules, t: Token) -> Pos {
    let p = match t {
        Token::Char(n, off, len) => Pos::new(n, off + len),
        Token::Start(e) => Pos::new(e, 0),
        Token::End(e) | Token::Atom(e) => {
            let parent = doc.parent(e).unwrap();
            Pos::new(parent, doc.index_in_parent(e).unwrap() + 1)
        }
    };
    canonical(doc, rules, p)
}

pub fn pos_before_token(doc: &Document, rules: &dyn Rules, t: Token) -> Pos {
    let p = match t {
        Token::Char(n, off, _) => Pos::new(n, off),
        Token::End(e) => Pos::new(e, doc.children(e).len()),
        Token::Start(e) | Token::Atom(e) => {
            let parent = doc.parent(e).unwrap();
            Pos::new(parent, doc.index_in_parent(e).unwrap())
        }
    };
    canonical_left(doc, rules, p)
}

/// Like [`canonical`] but prefers the text on the left.
pub fn canonical_left(doc: &Document, rules: &dyn Rules, p: Pos) -> Pos {
    if doc.is_text(p.node) {
        return p;
    }
    let ch = doc.children(p.node);
    if p.offset > 0
        && let Some(&c) = ch.get(p.offset - 1)
        && is_caret_text(doc, rules, c)
    {
        return Pos::new(c, doc.text(c).unwrap().len());
    }
    canonical(doc, rules, p)
}

/// The document element bounds caret movement.
fn within_root(doc: &Document, p: Pos) -> bool {
    let Some(root) = doc.document_element() else {
        return false;
    };
    doc.is_ancestor_or_self(root, p.node) && !(p.node == doc.root())
}

/// Next caret stop. With `skip_tags`, tag-only moves are merged (Normal view).
pub fn next_pos(doc: &Document, rules: &dyn Rules, p: Pos, skip_tags: bool) -> Pos {
    step(doc, rules, p, skip_tags, true)
}

pub fn prev_pos(doc: &Document, rules: &dyn Rules, p: Pos, skip_tags: bool) -> Pos {
    step(doc, rules, p, skip_tags, false)
}

/// One caret step. With `skip_tags` (Normal view) a step must consume visible
/// content, or cross into a different block, so it never looks like a no-op.
fn step(doc: &Document, rules: &dyn Rules, p: Pos, skip_tags: bool, forward: bool) -> Pos {
    let mut cur = p;
    let mut crossed_block = false;
    loop {
        let tok = if forward {
            token_after(doc, rules, cur)
        } else {
            token_before(doc, rules, cur)
        };
        let Some(t) = tok else { return cur };
        let n = if forward {
            pos_after_token(doc, rules, t)
        } else {
            pos_before_token(doc, rules, t)
        };
        if !within_root(doc, n) {
            return cur;
        }
        cur = n;
        match t {
            Token::Char(..) | Token::Atom(_) => return cur,
            _ if !skip_tags => return cur,
            Token::Start(e) | Token::End(e) => {
                if rules.is_block(doc, e) {
                    crossed_block = true;
                }
                let ahead = if forward {
                    token_after(doc, rules, cur)
                } else {
                    token_before(doc, rules, cur)
                };
                if crossed_block && matches!(ahead, Some(Token::Char(..) | Token::Atom(_))) {
                    return cur;
                }
                if ahead.is_none() {
                    return cur;
                }
            }
        }
    }
}

/// Moves to the next word boundary.
pub fn next_word(doc: &Document, rules: &dyn Rules, p: Pos) -> Pos {
    let mut cur = next_pos(doc, rules, p, true);
    let mut seen_word = false;
    for _ in 0..10_000 {
        match token_after(doc, rules, cur) {
            Some(Token::Char(n, off, len)) => {
                let c = doc.text(n).unwrap()[off..off + len].chars().next().unwrap();
                if c.is_alphanumeric() {
                    seen_word = true;
                } else if seen_word {
                    return cur;
                }
            }
            _ => return cur,
        }
        cur = next_pos(doc, rules, cur, true);
    }
    cur
}

pub fn prev_word(doc: &Document, rules: &dyn Rules, p: Pos) -> Pos {
    let mut cur = prev_pos(doc, rules, p, true);
    for _ in 0..10_000 {
        match token_before(doc, rules, cur) {
            Some(Token::Char(n, off, len)) => {
                let c = doc.text(n).unwrap()[off..off + len].chars().next().unwrap();
                if !c.is_alphanumeric() {
                    return cur;
                }
            }
            _ => return cur,
        }
        cur = prev_pos(doc, rules, cur, true);
    }
    cur
}

/// First caret position inside a node's content.
pub fn start_of(doc: &Document, rules: &dyn Rules, n: NodeId) -> Pos {
    if doc.is_text(n) {
        return Pos::new(n, 0);
    }
    canonical(doc, rules, Pos::new(n, 0))
}

pub fn end_of(doc: &Document, rules: &dyn Rules, n: NodeId) -> Pos {
    if let Some(t) = doc.text(n) {
        return Pos::new(n, t.len());
    }
    canonical_left(doc, rules, Pos::new(n, doc.children(n).len()))
}

/// Finds the first "deepest" caret position inside an element, descending into
/// leading child elements (where typing should happen after an insert).
pub fn first_editable(doc: &Document, rules: &dyn Rules, n: NodeId) -> Pos {
    first_editable_in(doc, rules, n).unwrap_or_else(|| canonical(doc, rules, Pos::new(n, 0)))
}

/// Depth-first search for the first place text can go, skipping empty
/// elements that cannot hold text (e.g. CALS `colspec`).
fn first_editable_in(doc: &Document, rules: &dyn Rules, n: NodeId) -> Option<Pos> {
    for &c in doc.children(n) {
        if is_ignorable(doc, rules, c) {
            continue;
        }
        match doc.kind(c) {
            NodeKind::Element(_) => {
                if let Some(p) = first_editable_in(doc, rules, c) {
                    return Some(p);
                }
                let empty_holder = doc.children(c).is_empty()
                    && rules.can_contain_text(doc, c)
                    && !rules.is_element_only(doc, c);
                if empty_holder {
                    return Some(Pos::new(c, 0));
                }
            }
            NodeKind::Text(_) => return Some(Pos::new(c, 0)),
            _ => return Some(Pos::new(n, doc.index_in_parent(c).unwrap())),
        }
    }
    if doc.children(n).iter().all(|&c| is_ignorable(doc, rules, c))
        && rules.can_contain_text(doc, n)
        && !rules.is_element_only(doc, n)
    {
        return Some(canonical(doc, rules, Pos::new(n, 0)));
    }
    None
}

/// The element containing a position.
pub fn container(doc: &Document, p: Pos) -> NodeId {
    if doc.is_text(p.node) {
        doc.parent(p.node).unwrap()
    } else {
        p.node
    }
}

/// Compares boundary points in document order (DOM Range algorithm).
pub fn cmp_pos(doc: &Document, a: Pos, b: Pos) -> Ordering {
    if a.node == b.node {
        return a.offset.cmp(&b.offset);
    }
    // Is b.node a descendant of a.node?
    if doc.is_ancestor_or_self(a.node, b.node) {
        let child = child_toward(doc, a.node, b.node);
        let idx = doc.index_in_parent(child).unwrap();
        return if idx < a.offset {
            Ordering::Greater
        } else {
            Ordering::Less
        };
    }
    if doc.is_ancestor_or_self(b.node, a.node) {
        return cmp_pos(doc, b, a).reverse();
    }
    doc.cmp_order(a.node, b.node)
}

/// The child of `anc` on the path to `desc`.
pub fn child_toward(doc: &Document, anc: NodeId, desc: NodeId) -> NodeId {
    let mut cur = desc;
    while let Some(p) = doc.parent(cur) {
        if p == anc {
            return cur;
        }
        cur = p;
    }
    desc
}

pub fn common_ancestor(doc: &Document, a: NodeId, b: NodeId) -> NodeId {
    let mut cur = Some(a);
    while let Some(c) = cur {
        if doc.is_ancestor_or_self(c, b) {
            return c;
        }
        cur = doc.parent(c);
    }
    doc.root()
}
