//! Structure-aware editing engine (UI independent).
//!
//! [`Editor`] owns a [`Document`] plus a selection and implements the editing
//! commands of an XMetaL-style tags-on editor: typing, deletion that respects
//! markup, splitting and joining containers, inserting/surrounding/removing
//! elements, clipboard fragments, CALS/HTML table operations, and undo.
//! Schema and customisation knowledge comes in through the [`Rules`] trait.

pub mod pos;
pub mod table;

pub use pos::{Pos, Selection, Token, cmp_pos};

use pos::*;
use std::cmp::Ordering;
use xylem_dom::{Document, NodeId, NodeKind};

/// Knowledge the editor needs from schema, style, and customisation.
/// All methods have permissive defaults so the editor works on plain XML.
pub trait Rules {
    fn can_contain_text(&self, _doc: &Document, _el: NodeId) -> bool {
        true
    }
    fn can_insert(&self, _doc: &Document, _parent: NodeId, _index: usize, _name: &str) -> bool {
        true
    }
    fn is_element_only(&self, _doc: &Document, _el: NodeId) -> bool {
        false
    }
    fn is_preformatted(&self, _doc: &Document, _el: NodeId) -> bool {
        false
    }
    /// Block-level (paragraph-like) element, as decided by the stylesheet.
    fn is_block(&self, _doc: &Document, _el: NodeId) -> bool {
        false
    }
    fn is_read_only(&self, _doc: &Document, _el: NodeId) -> bool {
        false
    }
    fn is_non_removable(&self, _doc: &Document, _el: NodeId) -> bool {
        false
    }
    /// Protected non-element nodes (marker PIs, entity references). These
    /// can't be deleted on their own, only as part of a removed ancestor.
    fn is_protected(&self, _doc: &Document, _node: NodeId) -> bool {
        false
    }
    /// Element to create when pressing Enter at the end of `name`.
    fn followed_by(&self, _name: &str) -> Option<String> {
        None
    }
    /// XML used to create a new `name` element at `parent[index]`.
    fn new_element_xml(
        &self,
        _doc: &Document,
        _parent: NodeId,
        _index: usize,
        name: &str,
    ) -> String {
        format!("<{name}/>")
    }
    /// Attributes that must not be copied when splitting (IDs).
    fn is_id_attr(&self, _element: &str, attr: &str) -> bool {
        matches!(attr, "id" | "ID" | "Id" | "xml:id")
    }
}

/// Permissive rules for documents without a schema.
pub struct NoRules;
impl Rules for NoRules {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    ReadOnly,
    /// A protected marker (PI or entity reference) would be removed.
    Protected,
    NotAllowed(String),
    Nothing,
    Parse(String),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::ReadOnly => write!(f, "This part of the document is read-only."),
            EditError::Protected => write!(f, "This marker is protected; it can only be removed with its parent element."),
            EditError::NotAllowed(m) => write!(f, "{m}"),
            EditError::Nothing => write!(f, "Nothing to do here."),
            EditError::Parse(m) => write!(f, "Not well-formed XML: {m}"),
        }
    }
}

pub type EditResult<T = ()> = Result<T, EditError>;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum EditKind {
    Typing,
    Other,
}

/// One undo step: the journaled primitive operations of an edit (cheap), or a
/// whole document (only for wholesale replacement from the source view).
enum UndoChange {
    Ops(Vec<xylem_dom::Op>),
    Doc(Document),
}

struct UndoEntry {
    change: UndoChange,
    /// Selection before the change.
    sel: Selection,
    /// Document state id before the change.
    state: u64,
}

pub struct Editor {
    pub doc: Document,
    pub sel: Selection,
    undo: Vec<UndoEntry>,
    redo: Vec<UndoEntry>,
    last_kind: Option<EditKind>,
    /// Identifies the current document state; every edit gets a fresh id and
    /// undo/redo restore earlier ids, so `state != saved_state` is exact.
    state: u64,
    next_state: u64,
    saved_state: u64,
    /// Set by the view when an edit should start a fresh undo step.
    pub break_undo_group: bool,
    pub undo_limit: usize,
}

impl Editor {
    pub fn new(doc: Document) -> Self {
        let start = doc
            .document_element()
            .map(|r| first_editable(&doc, &NoRules, r))
            .unwrap_or(Pos::new(doc.root(), 0));
        Self {
            doc,
            sel: Selection::caret(start),
            undo: Vec::new(),
            redo: Vec::new(),
            last_kind: None,
            state: 0,
            next_state: 1,
            saved_state: 0,
            break_undo_group: false,
            undo_limit: 1000,
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.state != self.saved_state
    }

    pub fn mark_saved(&mut self) {
        self.saved_state = self.state;
        // Further typing must not merge into an already-saved undo step.
        self.break_undo_group = true;
    }

    pub fn caret(&self) -> Pos {
        self.sel.focus
    }

    pub fn set_caret(&mut self, p: Pos) {
        self.sel = Selection::caret(p);
        self.break_undo_group = true;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn fresh_state(&mut self) -> u64 {
        let s = self.next_state;
        self.next_state += 1;
        s
    }

    fn commit(&mut self, ops: Vec<xylem_dom::Op>, sel_before: Selection, kind: EditKind) {
        if ops.is_empty() {
            return;
        }
        let coalesce = kind == EditKind::Typing
            && self.last_kind == Some(EditKind::Typing)
            && !self.break_undo_group;
        match self.undo.last_mut() {
            Some(UndoEntry { change: UndoChange::Ops(prev), .. }) if coalesce => prev.extend(ops),
            _ => {
                self.undo.push(UndoEntry { change: UndoChange::Ops(ops), sel: sel_before, state: self.state });
                if self.undo.len() > self.undo_limit {
                    self.undo.remove(0);
                }
            }
        }
        self.state = self.fresh_state();
        self.redo.clear();
        self.last_kind = Some(kind);
        self.break_undo_group = false;
    }

    /// Runs `f` as one undoable step; rolls back on error.
    pub fn transact<T>(&mut self, f: impl FnOnce(&mut Self) -> EditResult<T>) -> EditResult<T> {
        self.transact_kind(EditKind::Other, f)
    }

    fn transact_kind<T>(
        &mut self,
        kind: EditKind,
        f: impl FnOnce(&mut Self) -> EditResult<T>,
    ) -> EditResult<T> {
        if !self.doc.begin_journal() {
            // Nested: the outer transaction records and rolls back.
            return f(self);
        }
        let sel_before = self.sel;
        let result = f(self);
        let ops = self.doc.take_journal();
        match result {
            Ok(v) => {
                self.commit(ops, sel_before, kind);
                Ok(v)
            }
            Err(e) => {
                self.doc.revert(&ops);
                // `Nothing` means only the caret moved; keep that movement.
                if e != EditError::Nothing {
                    self.sel = sel_before;
                }
                self.fix_selection(&NoRules);
                Err(e)
            }
        }
    }

    /// Applies an undo/redo entry and returns the inverse entry.
    fn apply_entry(&mut self, entry: UndoEntry) -> UndoEntry {
        let change = match entry.change {
            UndoChange::Ops(ops) => {
                self.doc.begin_journal();
                self.doc.revert(&ops);
                UndoChange::Ops(self.doc.take_journal())
            }
            UndoChange::Doc(doc) => UndoChange::Doc(std::mem::replace(&mut self.doc, doc)),
        };
        let inverse = UndoEntry { change, sel: self.sel, state: self.state };
        self.sel = entry.sel;
        self.state = entry.state;
        self.last_kind = None;
        self.fix_selection(&NoRules);
        inverse
    }

    pub fn undo(&mut self) -> bool {
        let Some(e) = self.undo.pop() else { return false };
        let inv = self.apply_entry(e);
        self.redo.push(inv);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(e) = self.redo.pop() else { return false };
        let inv = self.apply_entry(e);
        self.undo.push(inv);
        true
    }

    pub fn clear_undo(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Replaces the document wholesale (e.g. after editing in source view).
    pub fn replace_document(&mut self, doc: Document) {
        let old = std::mem::replace(&mut self.doc, doc);
        let state = self.state;
        self.undo.push(UndoEntry { change: UndoChange::Doc(old), sel: self.sel, state });
        self.state = self.fresh_state();
        self.redo.clear();
        let start = self
            .doc
            .document_element()
            .map(|r| first_editable(&self.doc, &NoRules, r))
            .unwrap_or(Pos::new(self.doc.root(), 0));
        self.sel = Selection::caret(start);
    }

    // ----------------------------------------------------------- validation of positions

    /// Repairs a selection that refers to detached or out-of-range nodes.
    pub fn fix_selection(&mut self, rules: &dyn Rules) {
        let fix = |doc: &Document, p: Pos| -> Pos {
            let attached = doc.contains(p.node)
                && (p.node == doc.root() || doc.ancestors(p.node).any(|a| a == doc.root()));
            if !attached {
                return doc
                    .document_element()
                    .map(|r| first_editable(doc, rules, r))
                    .unwrap_or(Pos::new(doc.root(), 0));
            }
            let max = doc
                .text(p.node)
                .map(|t| t.len())
                .unwrap_or(doc.children(p.node).len());
            let mut off = p.offset.min(max);
            if let Some(t) = doc.text(p.node) {
                while !t.is_char_boundary(off) {
                    off -= 1;
                }
            }
            Pos::new(p.node, off)
        };
        self.sel.anchor = fix(&self.doc, self.sel.anchor);
        self.sel.focus = fix(&self.doc, self.sel.focus);
    }

    fn check_writable(&self, rules: &dyn Rules, node: NodeId) -> EditResult {
        let el = if self.doc.is_element(node) {
            Some(node)
        } else {
            self.doc.parent(node)
        };
        if let Some(el) = el
            && self.doc.is_ancestor_or_self(self.doc.root(), el)
            && std::iter::once(el)
                .chain(self.doc.ancestors(el))
                .any(|a| self.doc.is_element(a) && rules.is_read_only(&self.doc, a))
        {
            return Err(EditError::ReadOnly);
        }
        Ok(())
    }

    // ----------------------------------------------------------- text input

    /// Inserts text at the caret, replacing the selection.
    pub fn insert_text(&mut self, rules: &dyn Rules, text: &str) -> EditResult {
        if text.is_empty() {
            return Ok(());
        }
        let kind = if self.sel.is_caret() && !text.contains('\n') {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.transact_kind(kind, |ed| {
            if !ed.sel.is_caret() {
                ed.delete_selection_inner(rules)?;
            }
            let p = ed.caret();
            ed.check_writable(rules, p.node)?;
            // Typing onto an `xm-replace_text` placeholder replaces it.
            let p = ed.consume_placeholder(rules, p);
            let p = ed.insert_text_at(rules, p, text)?;
            ed.sel = Selection::caret(p);
            Ok(())
        })
    }

    fn consume_placeholder(&mut self, rules: &dyn Rules, p: Pos) -> Pos {
        for tok in [
            token_after(&self.doc, rules, p),
            token_before(&self.doc, rules, p),
        ] {
            if let Some(Token::Atom(n)) = tok
                && matches!(self.doc.kind(n), NodeKind::Pi { target, .. } if target == "xm-replace_text")
            {
                let parent = self.doc.parent(n).unwrap();
                let idx = self.doc.index_in_parent(n).unwrap();
                self.doc.detach(n);
                return canonical(&self.doc, rules, Pos::new(parent, idx));
            }
        }
        p
    }

    fn insert_text_at(&mut self, rules: &dyn Rules, p: Pos, text: &str) -> EditResult<Pos> {
        if let Some(t) = self.doc.text(p.node) {
            let mut s = t.to_string();
            s.insert_str(p.offset, text);
            self.doc.set_text(p.node, s);
            return Ok(Pos::new(p.node, p.offset + text.len()));
        }
        let parent = p.node;
        if !self.doc.is_element(parent) {
            return Err(EditError::NotAllowed(
                "Text cannot be entered outside the root element.".into(),
            ));
        }
        if !rules.can_contain_text(&self.doc, parent) {
            let name = self.doc.name(parent).unwrap_or("?");
            return Err(EditError::NotAllowed(format!(
                "Text is not allowed directly in <{name}>."
            )));
        }
        // Reuse an adjacent (ignorable) whitespace text node if present.
        let ch = self.doc.children(parent).to_vec();
        if let Some(&c) = ch.get(p.offset)
            && self.doc.is_text(c)
        {
            let mut s = self.doc.text(c).unwrap().to_string();
            s.insert_str(0, text);
            self.doc.set_text(c, s);
            return Ok(Pos::new(c, text.len()));
        }
        let t = self.doc.create_text(text);
        self.doc.insert_child(parent, p.offset, t);
        Ok(Pos::new(t, text.len()))
    }

    // ----------------------------------------------------------- deletion

    pub fn delete_selection(&mut self, rules: &dyn Rules) -> EditResult {
        if self.sel.is_caret() {
            return Err(EditError::Nothing);
        }
        self.transact(|ed| ed.delete_selection_inner(rules))
    }

    fn delete_selection_inner(&mut self, rules: &dyn Rules) -> EditResult {
        let (a, b) = self.sel.ordered(&self.doc);
        let p = self.delete_range(rules, a, b)?;
        self.sel = Selection::caret(p);
        Ok(())
    }

    /// Deletes content between two positions (DOM Range `deleteContents`,
    /// without merging partially selected elements). Returns the new caret.
    pub fn delete_range(&mut self, rules: &dyn Rules, a: Pos, b: Pos) -> EditResult<Pos> {
        if cmp_pos(&self.doc, a, b) != Ordering::Less {
            return Ok(a);
        }
        // Same text node: simple.
        if a.node == b.node && self.doc.is_text(a.node) {
            self.check_writable(rules, a.node)?;
            let mut s = self.doc.text(a.node).unwrap().to_string();
            s.replace_range(a.offset..b.offset, "");
            self.doc.set_text(a.node, s);
            return Ok(a);
        }
        let (a, b) = self.split_pair(a, b);
        let (na, ia) = (a.node, a.offset);
        let (nb, ib) = (b.node, b.offset);
        let c = common_ancestor(&self.doc, na, nb);
        let mut doomed: Vec<NodeId> = Vec::new();

        let start_idx = if na == c {
            ia
        } else {
            let (mut cur, mut idx) = (na, ia);
            while cur != c {
                doomed.extend_from_slice(&self.doc.children(cur)[idx..]);
                idx = self.doc.index_in_parent(cur).unwrap() + 1;
                cur = self.doc.parent(cur).unwrap();
            }
            idx
        };

        let end_idx = if nb == c {
            ib
        } else {
            let (mut cur, mut idx) = (nb, ib);
            while cur != c {
                doomed.extend_from_slice(&self.doc.children(cur)[..idx]);
                idx = self.doc.index_in_parent(cur).unwrap();
                cur = self.doc.parent(cur).unwrap();
            }
            idx
        };
        if start_idx < end_idx {
            doomed.extend_from_slice(&self.doc.children(c)[start_idx..end_idx]);
        }
        for &d in &doomed {
            self.check_writable(rules, d)?;
            if rules.is_protected(&self.doc, d) {
                return Err(EditError::Protected);
            }
            for n in self.doc.descendants(d) {
                if self.doc.is_element(n)
                    && (rules.is_read_only(&self.doc, n) || rules.is_non_removable(&self.doc, n))
                {
                    return Err(EditError::ReadOnly);
                }
            }
        }
        for d in doomed {
            self.doc.detach(d);
        }
        // Caret at the start boundary, preferring text on the left.
        let caret = canonical_left(
            &self.doc,
            rules,
            Pos::new(na, ia.min(self.doc.children(na).len())),
        );
        let caret = self.normalize_keep(rules, caret);
        Ok(caret)
    }

    /// Splits text at both ends of an ordered range, returning element-level
    /// boundaries that remain consistent with each other.
    fn split_pair(&mut self, a: Pos, b: Pos) -> (Pos, Pos) {
        let mut b = self.split_at_boundary(b);
        let a_splits = self
            .doc
            .text(a.node)
            .is_some_and(|t| a.offset > 0 && a.offset < t.len());
        let a_parent = if self.doc.is_text(a.node) {
            self.doc.parent(a.node)
        } else {
            None
        };
        let a2 = self.split_at_boundary(a);
        if a_splits && a_parent == Some(b.node) && a2.offset <= b.offset {
            b.offset += 1;
        }
        (a2, b)
    }

    /// Converts a text-interior position into an element boundary by splitting.
    fn split_at_boundary(&mut self, p: Pos) -> Pos {
        if let Some(t) = self.doc.text(p.node) {
            let len = t.len();
            let parent = self.doc.parent(p.node).unwrap();
            let i = self.doc.index_in_parent(p.node).unwrap();
            if p.offset == 0 {
                return Pos::new(parent, i);
            }
            if p.offset >= len {
                return Pos::new(parent, i + 1);
            }
            self.doc.split_text(p.node, p.offset);
            return Pos::new(parent, i + 1);
        }
        p
    }

    /// Normalises the container of `p` and returns an equivalent caret.
    fn normalize_keep(&mut self, rules: &dyn Rules, p: Pos) -> Pos {
        let c = container(&self.doc, p);
        // Express as (text node, offset) of a left text which survives merging.
        let p = canonical_left(&self.doc, rules, p);
        if let Some(t) = self.doc.text(p.node) {
            // Offset from the start of the merged run: sum lengths of preceding
            // adjacent text siblings.
            let mut offset = p.offset;
            let mut first = p.node;
            while let Some(prev) = self.doc.prev_sibling(first) {
                if matches!(self.doc.kind(prev), NodeKind::Text(_)) {
                    offset += self.doc.text(prev).unwrap().len();
                    first = prev;
                } else {
                    break;
                }
            }
            let _ = t;
            self.doc.normalize(c);
            if self.doc.parent(first).is_some() {
                return Pos::new(first, offset);
            }
            return canonical(&self.doc, rules, Pos::new(c, 0));
        }
        let idx = p.offset;
        let before: Vec<NodeId> =
            self.doc.children(c)[..idx.min(self.doc.children(c).len())].to_vec();
        self.doc.normalize(c);
        let new_idx = before
            .iter()
            .filter(|n| self.doc.parent(**n) == Some(c))
            .count();
        canonical(&self.doc, rules, Pos::new(c, new_idx))
    }

    /// Backspace.
    pub fn delete_backward(&mut self, rules: &dyn Rules) -> EditResult {
        if !self.sel.is_caret() {
            return self.delete_selection(rules);
        }
        let kind = EditKind::Typing;
        self.transact_kind(kind, |ed| {
            let p = ed.caret();
            match token_before(&ed.doc, rules, p) {
                Some(Token::Char(n, off, len)) => {
                    ed.check_writable(rules, n)?;
                    let mut s = ed.doc.text(n).unwrap().to_string();
                    s.replace_range(off..off + len, "");
                    ed.doc.set_text(n, s);
                    let caret = ed.remove_if_emptied(rules, n, Pos::new(n, off));
                    ed.sel = Selection::caret(caret);
                    Ok(())
                }
                Some(Token::Atom(n)) => {
                    ed.check_writable(rules, n)?;
                    if rules.is_protected(&ed.doc, n) {
                        return Err(EditError::Protected);
                    }
                    let parent = ed.doc.parent(n).unwrap();
                    let idx = ed.doc.index_in_parent(n).unwrap();
                    ed.doc.detach(n);
                    let c = ed.normalize_keep(rules, Pos::new(parent, idx));
                    ed.sel = Selection::caret(c);
                    Ok(())
                }
                Some(Token::End(e)) => {
                    // Step into the element and delete its last character.
                    ed.sel = Selection::caret(pos_before_token(&ed.doc, rules, Token::End(e)));
                    if ed.doc.children(e).is_empty() {
                        return ed.remove_element_keep_caret(rules, e);
                    }
                    ed.delete_backward_inner(rules)
                }
                Some(Token::Start(e)) => {
                    if ed
                        .doc
                        .children(e)
                        .iter()
                        .all(|&c| is_ignorable(&ed.doc, rules, c))
                        && !rules.is_non_removable(&ed.doc, e)
                    {
                        return ed.remove_element_keep_caret(rules, e);
                    }
                    if rules.is_block(&ed.doc, e)
                        && let Some(prev) = ed.prev_element_sibling(rules, e)
                        && ed.doc.name(prev) == ed.doc.name(e)
                    {
                        return ed.join(rules, prev, e);
                    }
                    // Move out of the element.
                    let np = prev_pos(&ed.doc, rules, p, false);
                    ed.sel = Selection::caret(np);
                    Err(EditError::Nothing)
                }
                None => Err(EditError::Nothing),
            }
        })
        .or_else(|e| {
            if e == EditError::Nothing {
                Ok(())
            } else {
                Err(e)
            }
        })
    }

    fn delete_backward_inner(&mut self, rules: &dyn Rules) -> EditResult {
        let p = self.caret();
        match token_before(&self.doc, rules, p) {
            Some(Token::Char(n, off, len)) => {
                self.check_writable(rules, n)?;
                let mut s = self.doc.text(n).unwrap().to_string();
                s.replace_range(off..off + len, "");
                self.doc.set_text(n, s);
                let caret = self.remove_if_emptied(rules, n, Pos::new(n, off));
                self.sel = Selection::caret(caret);
                Ok(())
            }
            Some(Token::End(e)) => {
                self.sel = Selection::caret(pos_before_token(&self.doc, rules, Token::End(e)));
                self.delete_backward_inner(rules)
            }
            Some(Token::Atom(n)) => {
                if rules.is_protected(&self.doc, n) {
                    return Err(EditError::Protected);
                }
                let parent = self.doc.parent(n).unwrap();
                let idx = self.doc.index_in_parent(n).unwrap();
                self.doc.detach(n);
                self.sel = Selection::caret(self.normalize_keep(rules, Pos::new(parent, idx)));
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Delete key.
    pub fn delete_forward(&mut self, rules: &dyn Rules) -> EditResult {
        if !self.sel.is_caret() {
            return self.delete_selection(rules);
        }
        self.transact_kind(EditKind::Typing, |ed| {
            let p = ed.caret();
            match token_after(&ed.doc, rules, p) {
                Some(Token::Char(n, off, len)) => {
                    ed.check_writable(rules, n)?;
                    let mut s = ed.doc.text(n).unwrap().to_string();
                    s.replace_range(off..off + len, "");
                    ed.doc.set_text(n, s);
                    let caret = ed.remove_if_emptied(rules, n, Pos::new(n, off));
                    ed.sel = Selection::caret(caret);
                    Ok(())
                }
                Some(Token::Atom(n)) => {
                    ed.check_writable(rules, n)?;
                    if rules.is_protected(&ed.doc, n) {
                        return Err(EditError::Protected);
                    }
                    let parent = ed.doc.parent(n).unwrap();
                    let idx = ed.doc.index_in_parent(n).unwrap();
                    ed.doc.detach(n);
                    ed.sel = Selection::caret(ed.normalize_keep(rules, Pos::new(parent, idx)));
                    Ok(())
                }
                Some(Token::Start(e)) => {
                    if ed
                        .doc
                        .children(e)
                        .iter()
                        .all(|&c| is_ignorable(&ed.doc, rules, c))
                        && !rules.is_non_removable(&ed.doc, e)
                    {
                        return ed.remove_element_keep_caret(rules, e);
                    }
                    ed.sel = Selection::caret(pos_after_token(&ed.doc, rules, Token::Start(e)));
                    Err(EditError::Nothing)
                }
                Some(Token::End(e)) => {
                    if ed
                        .doc
                        .children(e)
                        .iter()
                        .all(|&c| is_ignorable(&ed.doc, rules, c))
                        && !rules.is_non_removable(&ed.doc, e)
                    {
                        return ed.remove_element_keep_caret(rules, e);
                    }
                    if rules.is_block(&ed.doc, e)
                        && let Some(next) = ed.next_element_sibling(rules, e)
                        && ed.doc.name(next) == ed.doc.name(e)
                    {
                        return ed.join(rules, e, next);
                    }
                    ed.sel = Selection::caret(next_pos(&ed.doc, rules, p, false));
                    Err(EditError::Nothing)
                }
                None => Err(EditError::Nothing),
            }
        })
        .or_else(|e| {
            if e == EditError::Nothing {
                Ok(())
            } else {
                Err(e)
            }
        })
    }

    fn prev_element_sibling(&self, rules: &dyn Rules, e: NodeId) -> Option<NodeId> {
        let mut cur = self.doc.prev_sibling(e);
        while let Some(c) = cur {
            if self.doc.is_element(c) {
                return Some(c);
            }
            if !is_ignorable(&self.doc, rules, c) {
                return None;
            }
            cur = self.doc.prev_sibling(c);
        }
        None
    }

    fn next_element_sibling(&self, rules: &dyn Rules, e: NodeId) -> Option<NodeId> {
        let mut cur = self.doc.next_sibling(e);
        while let Some(c) = cur {
            if self.doc.is_element(c) {
                return Some(c);
            }
            if !is_ignorable(&self.doc, rules, c) {
                return None;
            }
            cur = self.doc.next_sibling(c);
        }
        None
    }

    /// If deleting emptied a text node inside an inline element, remove the
    /// emptied element(s) too. Returns a valid caret.
    fn remove_if_emptied(&mut self, rules: &dyn Rules, text: NodeId, caret: Pos) -> Pos {
        if !self.doc.text(text).map(|t| t.is_empty()).unwrap_or(false) {
            return caret;
        }
        let mut parent = self.doc.parent(text).unwrap();
        let mut idx = self.doc.index_in_parent(text).unwrap();
        self.doc.detach(text);
        // Remove empty inline ancestors.
        while self.doc.is_element(parent)
            && self.doc.children(parent).is_empty()
            && !rules.is_block(&self.doc, parent)
            && !rules.is_non_removable(&self.doc, parent)
            && Some(parent) != self.doc.document_element()
        {
            let gp = self.doc.parent(parent).unwrap();
            idx = self.doc.index_in_parent(parent).unwrap();
            self.doc.detach(parent);
            parent = gp;
        }
        self.normalize_keep(rules, Pos::new(parent, idx))
    }

    fn remove_element_keep_caret(&mut self, rules: &dyn Rules, e: NodeId) -> EditResult {
        if Some(e) == self.doc.document_element() {
            return Err(EditError::NotAllowed(
                "The root element cannot be removed.".into(),
            ));
        }
        self.check_writable(rules, self.doc.parent(e).unwrap())?;
        if rules.is_non_removable(&self.doc, e) {
            return Err(EditError::ReadOnly);
        }
        let parent = self.doc.parent(e).unwrap();
        let idx = self.doc.index_in_parent(e).unwrap();
        self.doc.detach(e);
        self.sel = Selection::caret(self.normalize_keep(rules, Pos::new(parent, idx)));
        Ok(())
    }

    /// Moves `b`'s children to the end of `a` and removes `b`.
    fn join(&mut self, rules: &dyn Rules, a: NodeId, b: NodeId) -> EditResult {
        self.check_writable(rules, a)?;
        self.check_writable(rules, b)?;
        let caret_idx = self.doc.children(a).len();
        let kids = self.doc.children(b).to_vec();
        for k in kids {
            self.doc.append_child(a, k);
        }
        self.doc.detach(b);
        self.sel = Selection::caret(self.normalize_keep(rules, Pos::new(a, caret_idx)));
        Ok(())
    }

    /// XMAX `JoinElementToPreceding`.
    pub fn join_to_preceding(&mut self, rules: &dyn Rules, e: NodeId) -> EditResult {
        let prev = self
            .prev_element_sibling(rules, e)
            .ok_or(EditError::Nothing)?;
        if self.doc.name(prev) != self.doc.name(e) {
            return Err(EditError::NotAllowed(
                "Only elements of the same type can be joined.".into(),
            ));
        }
        self.transact(|ed| ed.join(rules, prev, e))
    }

    pub fn can_join_to_preceding(&self, rules: &dyn Rules, e: NodeId) -> bool {
        self.prev_element_sibling(rules, e)
            .is_some_and(|p| self.doc.name(p) == self.doc.name(e))
    }

    // ----------------------------------------------------------- split / enter

    /// The element Enter would split: the nearest block ancestor of the caret.
    pub fn split_target(&self, rules: &dyn Rules) -> Option<NodeId> {
        let c = container(&self.doc, self.caret());
        std::iter::once(c)
            .chain(self.doc.ancestors(c))
            .take_while(|&n| self.doc.is_element(n))
            .find(|&n| rules.is_block(&self.doc, n) && Some(n) != self.doc.document_element())
    }

    /// Enter key: split the current block, or insert its "followed by" element.
    pub fn enter(&mut self, rules: &dyn Rules) -> EditResult {
        let Some(target) = self.split_target(rules) else {
            return Err(EditError::NotAllowed(
                "There is no paragraph to split here.".into(),
            ));
        };
        self.transact(|ed| {
            if !ed.sel.is_caret() {
                ed.delete_selection_inner(rules)?;
            }
            let p = ed.caret();
            let at_end = token_after_skipping_tags_within(&ed.doc, rules, p, target).is_none();
            let name = ed.doc.name(target).unwrap().to_string();
            if at_end
                && let Some(next) = rules.followed_by(&name)
                && next != name
            {
                let parent = ed.doc.parent(target).unwrap();
                let idx = ed.doc.index_in_parent(target).unwrap() + 1;
                if rules.can_insert(&ed.doc, parent, idx, &next) {
                    let n = ed.insert_new_element(rules, parent, idx, &next, None)?;
                    ed.sel = Selection::caret(first_editable(&ed.doc, rules, n));
                    return Ok(());
                }
            }
            let parent = ed.doc.parent(target).unwrap();
            let idx = ed.doc.index_in_parent(target).unwrap() + 1;
            if !rules.can_insert(&ed.doc, parent, idx, &name) {
                return Err(EditError::NotAllowed(format!(
                    "Another <{name}> is not allowed here."
                )));
            }
            ed.check_writable(rules, target)?;
            let right = ed.split_element(rules, target, p);
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, right));
            Ok(())
        })
    }

    /// Splits `target` (an ancestor of `p`) at `p`; returns the new right part.
    pub fn split_element(&mut self, rules: &dyn Rules, target: NodeId, p: Pos) -> NodeId {
        let mut b = self.split_at_boundary(p);
        loop {
            let cur = b.node;
            let el = self.doc.element(cur).unwrap().clone();
            let copy = self.doc.create_element(el.name.clone());
            for a in &el.attrs {
                if !rules.is_id_attr(&el.name, &a.name) {
                    self.doc.set_attr(copy, &a.name, a.value.clone());
                }
            }
            let moved: Vec<NodeId> = self.doc.children(cur)[b.offset..].to_vec();
            for m in moved {
                self.doc.append_child(copy, m);
            }
            let parent = self.doc.parent(cur).unwrap();
            let idx = self.doc.index_in_parent(cur).unwrap() + 1;
            self.doc.insert_child(parent, idx, copy);
            if cur == target {
                return copy;
            }
            b = Pos::new(parent, idx);
        }
    }

    /// XMAX `SplitContainer`: split the innermost element at the caret.
    pub fn split_container(&mut self, rules: &dyn Rules) -> EditResult {
        let c = container(&self.doc, self.caret());
        if Some(c) == self.doc.document_element() {
            return Err(EditError::NotAllowed(
                "The root element cannot be split.".into(),
            ));
        }
        let name = self.doc.name(c).unwrap_or("").to_string();
        let parent = self.doc.parent(c).unwrap();
        let idx = self.doc.index_in_parent(c).unwrap() + 1;
        if !rules.can_insert(&self.doc, parent, idx, &name) {
            return Err(EditError::NotAllowed(format!(
                "<{name}> cannot be split here."
            )));
        }
        self.transact(|ed| {
            let p = ed.caret();
            let r = ed.split_element(rules, c, p);
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, r));
            Ok(())
        })
    }

    // ----------------------------------------------------------- element insertion

    /// Inserts a new element built from `rules.new_element_xml` at `parent[index]`.
    fn insert_new_element(
        &mut self,
        rules: &dyn Rules,
        parent: NodeId,
        index: usize,
        name: &str,
        xml: Option<&str>,
    ) -> EditResult<NodeId> {
        self.check_writable(rules, parent)?;
        let xml = match xml {
            Some(x) => x.to_string(),
            None => rules.new_element_xml(&self.doc, parent, index, name),
        };
        let nodes = self
            .doc
            .parse_fragment_into(&xml)
            .map_err(|e| EditError::Parse(e.to_string()))?;
        let mut first = None;
        for (k, n) in nodes.into_iter().enumerate() {
            self.doc.insert_child(parent, index + k, n);
            if first.is_none() && self.doc.is_element(n) {
                first = Some(n);
            }
        }
        first.ok_or_else(|| EditError::Parse("template produced no element".into()))
    }

    /// Where an element would be inserted at the caret: (parent, index).
    pub fn insertion_point(&self) -> (NodeId, usize) {
        let p = self.caret();
        if let Some(t) = self.doc.text(p.node) {
            let parent = self.doc.parent(p.node).unwrap();
            let i = self.doc.index_in_parent(p.node).unwrap();
            if p.offset == 0 {
                return (parent, i);
            }
            if p.offset >= t.len() {
                return (parent, i + 1);
            }
            return (parent, i + 1); // text will be split
        }
        (p.node, p.offset)
    }

    /// Inserts element `name` at the caret (XMAX `InsertWithTemplate`).
    pub fn insert_element(&mut self, rules: &dyn Rules, name: &str) -> EditResult<NodeId> {
        self.insert_element_impl(rules, name, None)
    }

    /// Inserts prepared markup for element `name` (e.g. a new table) at the
    /// caret, or after the nearest ancestor where it is allowed.
    pub fn insert_element_xml(&mut self, rules: &dyn Rules, name: &str, xml: &str) -> EditResult<NodeId> {
        self.insert_element_impl(rules, name, Some(xml))
    }

    fn insert_element_impl(&mut self, rules: &dyn Rules, name: &str, xml: Option<&str>) -> EditResult<NodeId> {
        self.transact(|ed| {
            if !ed.sel.is_caret() {
                ed.delete_selection_inner(rules)?;
            }
            let p = ed.caret();
            let b = ed.split_at_boundary(p);
            if !rules.can_insert(&ed.doc, b.node, b.offset, name) {
                // Try after the containing elements (XMetaL "insert after").
                let mut cur = b.node;
                while let (Some(parent), true) = (ed.doc.parent(cur), ed.doc.is_element(cur)) {
                    let idx = ed.doc.index_in_parent(cur).unwrap() + 1;
                    if ed.doc.is_element(parent) && rules.can_insert(&ed.doc, parent, idx, name) {
                        let n = ed.insert_new_element(rules, parent, idx, name, xml)?;
                        ed.sel = Selection::caret(first_editable(&ed.doc, rules, n));
                        return Ok(n);
                    }
                    cur = parent;
                }
                return Err(EditError::NotAllowed(format!(
                    "<{name}> is not allowed here."
                )));
            }
            let n = ed.insert_new_element(rules, b.node, b.offset, name, xml)?;
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, n));
            Ok(n)
        })
    }

    /// Inserts a comment, PI or CDATA section at the caret.
    pub fn insert_special(&mut self, rules: &dyn Rules, kind: NodeKind) -> EditResult<NodeId> {
        self.transact(|ed| {
            let b = ed.split_at_boundary(ed.caret());
            ed.check_writable(rules, b.node)?;
            let n = ed.doc.create_node(kind);
            ed.doc.insert_child(b.node, b.offset, n);
            ed.sel = Selection::caret(canonical(&ed.doc, rules, Pos::new(b.node, b.offset + 1)));
            Ok(n)
        })
    }

    /// Selection boundaries lifted to a common parent: (parent, start, end).
    /// Text at the edges is split. Returns None if the selection crosses
    /// element boundaries unevenly.
    fn selection_as_sibling_range(&mut self) -> Option<(NodeId, usize, usize)> {
        let (a, b) = self.sel.ordered(&self.doc);
        let (mut a, mut b) = self.split_pair(a, b);
        // Lift boundaries that sit at the very start/end of an element.
        while a.node != b.node {
            if self.doc.is_ancestor_or_self(a.node, b.node) {
                // b deeper: lift b if at end of its node
                if b.offset == self.doc.children(b.node).len() || b.offset == 0 {
                    let i = self.doc.index_in_parent(b.node)?;
                    let add = if b.offset == 0 { 0 } else { 1 };
                    b = Pos::new(self.doc.parent(b.node)?, i + add);
                    continue;
                }
                return None;
            } else if self.doc.is_ancestor_or_self(b.node, a.node) {
                if a.offset == 0 || a.offset == self.doc.children(a.node).len() {
                    let i = self.doc.index_in_parent(a.node)?;
                    let add = if a.offset == 0 { 0 } else { 1 };
                    a = Pos::new(self.doc.parent(a.node)?, i + add);
                    continue;
                }
                return None;
            } else {
                // Lift the deeper one.
                if self.doc.depth(a.node) >= self.doc.depth(b.node) {
                    if a.offset != 0 && a.offset != self.doc.children(a.node).len() {
                        return None;
                    }
                    let i = self.doc.index_in_parent(a.node)?;
                    let add = if a.offset == 0 { 0 } else { 1 };
                    a = Pos::new(self.doc.parent(a.node)?, i + add);
                } else {
                    if b.offset != 0 && b.offset != self.doc.children(b.node).len() {
                        return None;
                    }
                    let i = self.doc.index_in_parent(b.node)?;
                    let add = if b.offset == 0 { 0 } else { 1 };
                    b = Pos::new(self.doc.parent(b.node)?, i + add);
                }
            }
        }
        if a.offset > b.offset {
            return None;
        }
        Some((a.node, a.offset, b.offset))
    }

    /// Wraps the selection in a new element (XMAX `Surround`).
    pub fn surround(
        &mut self,
        rules: &dyn Rules,
        name: &str,
        can_surround: &dyn Fn(&Document, NodeId, usize, usize) -> bool,
    ) -> EditResult<NodeId> {
        self.transact(|ed| {
            let (parent, s, e) = ed.selection_as_sibling_range().ok_or_else(|| {
                EditError::NotAllowed("The selection crosses element boundaries.".into())
            })?;
            ed.check_writable(rules, parent)?;
            if !can_surround(&ed.doc, parent, s, e) {
                return Err(EditError::NotAllowed(format!(
                    "<{name}> cannot surround the selection here."
                )));
            }
            let w = ed.doc.wrap_children(parent, s, e, name);
            let start = start_of(&ed.doc, rules, w);
            let end = end_of(&ed.doc, rules, w);
            ed.sel = Selection {
                anchor: start,
                focus: end,
            };
            Ok(w)
        })
    }

    /// Removes an element's tags, keeping its content (XMAX `RemoveContainerTags`).
    pub fn remove_tags(&mut self, rules: &dyn Rules, el: NodeId) -> EditResult {
        if Some(el) == self.doc.document_element() {
            return Err(EditError::NotAllowed(
                "The root element's tags cannot be removed.".into(),
            ));
        }
        if rules.is_non_removable(&self.doc, el) {
            return Err(EditError::ReadOnly);
        }
        self.transact(|ed| {
            ed.check_writable(rules, el)?;
            let parent = ed.doc.parent(el).unwrap();
            let idx = ed.doc.index_in_parent(el).unwrap();
            let n = ed.doc.children(el).len();
            let kids: Vec<NodeId> = ed.doc.children(el).to_vec();
            ed.doc.unwrap_node(el);
            let first_alive = kids
                .iter()
                .find(|k| ed.doc.parent(**k) == Some(parent))
                .copied();
            let p = match first_alive {
                Some(k) => start_of(&ed.doc, rules, k),
                None => canonical(
                    &ed.doc,
                    rules,
                    Pos::new(parent, idx.min(ed.doc.children(parent).len())),
                ),
            };
            let _ = n;
            ed.sel = Selection::caret(p);
            Ok(())
        })
    }

    /// Deletes an element and its content.
    pub fn delete_element(&mut self, rules: &dyn Rules, el: NodeId) -> EditResult {
        self.transact(|ed| ed.remove_element_keep_caret(rules, el))
    }

    pub fn rename_element(&mut self, rules: &dyn Rules, el: NodeId, name: &str) -> EditResult {
        self.transact(|ed| {
            ed.check_writable(rules, el)?;
            ed.doc.rename(el, name);
            Ok(())
        })
    }

    pub fn set_attribute(
        &mut self,
        rules: &dyn Rules,
        el: NodeId,
        name: &str,
        value: &str,
    ) -> EditResult {
        self.transact(|ed| {
            ed.check_writable(rules, el)?;
            ed.doc.set_attr(el, name, value);
            Ok(())
        })
    }

    pub fn remove_attribute(&mut self, rules: &dyn Rules, el: NodeId, name: &str) -> EditResult {
        self.transact(|ed| {
            ed.check_writable(rules, el)?;
            ed.doc.remove_attr(el, name);
            Ok(())
        })
    }

    /// Toggles an inline element (bold, italic…): removes it if the caret is
    /// inside one, otherwise surrounds the selection or inserts it.
    pub fn toggle_inline(
        &mut self,
        rules: &dyn Rules,
        name: &str,
        can_surround: &dyn Fn(&Document, NodeId, usize, usize) -> bool,
    ) -> EditResult {
        let c = container(&self.doc, self.caret());
        let found = std::iter::once(c)
            .chain(self.doc.ancestors(c))
            .find(|&n| self.doc.name(n) == Some(name));
        if let Some(e) = found
            && self.sel.is_caret()
        {
            return self.remove_tags(rules, e);
        }
        if self.sel.is_caret() {
            self.insert_element(rules, name).map(|_| ())
        } else {
            self.surround(rules, name, can_surround).map(|_| ())
        }
    }

    // ----------------------------------------------------------- clipboard

    /// Serialises the selection as an XML fragment (keeps partial ancestors).
    pub fn selection_xml(&self) -> Option<String> {
        if self.sel.is_caret() {
            return None;
        }
        let (a, b) = self.sel.ordered(&self.doc);
        let c = common_ancestor(&self.doc, a.node, b.node);
        if self.doc.is_text(c) {
            let t = self.doc.text(c).unwrap();
            let mut s = String::new();
            xylem_dom::escape_text(&t[a.offset..b.offset], &mut s);
            return Some(s);
        }
        let mut clone = Editor::new(self.doc.clone());
        let end_c = Pos::new(c, clone.doc.children(c).len());
        clone.delete_range_raw(b, end_c);
        clone.delete_range_raw(Pos::new(c, 0), a);
        let mut out = String::new();
        for &ch in clone.doc.children(c) {
            out.push_str(&clone.doc.node_to_xml(ch));
        }
        Some(out)
    }

    fn delete_range_raw(&mut self, a: Pos, b: Pos) {
        let _ = self.delete_range(&NoRules, a, b);
    }

    pub fn selection_text(&self) -> Option<String> {
        let xml = self.selection_xml()?;
        let frag = xylem_dom::parse_fragment(&xml).ok()?;
        Some(frag.text_content(frag.root()))
    }

    /// Pastes an XML fragment (falls back to plain text when it does not parse
    /// or is not allowed).
    pub fn paste(&mut self, rules: &dyn Rules, xml_or_text: &str, as_xml: bool) -> EditResult {
        if as_xml && let Ok(frag) = xylem_dom::parse_fragment(xml_or_text) {
            let has_markup = frag.children(frag.root()).iter().any(|&c| !frag.is_text(c));
            if has_markup {
                let r = self.transact(|ed| {
                    if !ed.sel.is_caret() {
                        ed.delete_selection_inner(rules)?;
                    }
                    ed.paste_fragment(rules, &frag)
                });
                if r.is_ok() {
                    return r;
                }
            }
            return self.insert_text(rules, &frag.text_content(frag.root()));
        }
        self.insert_text(rules, xml_or_text)
    }

    fn paste_fragment(&mut self, rules: &dyn Rules, frag: &Document) -> EditResult {
        let p = self.caret();
        let b = self.split_at_boundary(p);
        self.check_writable(rules, b.node)?;
        let tops: Vec<NodeId> = frag.children(frag.root()).to_vec();
        // Find a level where every top-level element is allowed.
        let mut target = b;
        loop {
            let ok = tops.iter().enumerate().all(|(k, &t)| match frag.kind(t) {
                NodeKind::Element(e) => {
                    rules.can_insert(&self.doc, target.node, target.offset + k, &e.name)
                }
                NodeKind::Text(s) => {
                    s.trim().is_empty() || rules.can_contain_text(&self.doc, target.node)
                }
                _ => true,
            });
            if ok {
                break;
            }
            let at_end = target.offset >= self.doc.children(target.node).len();
            let parent = self.doc.parent(target.node);
            match parent {
                Some(par) if at_end && self.doc.is_element(par) => {
                    target = Pos::new(par, self.doc.index_in_parent(target.node).unwrap() + 1);
                }
                _ => {
                    return Err(EditError::NotAllowed(
                        "The clipboard content is not allowed here.".into(),
                    ));
                }
            }
        }
        let mut last = None;
        for (k, &t) in tops.iter().enumerate() {
            let n = self.doc.import(frag, t);
            self.doc.insert_child(target.node, target.offset + k, n);
            last = Some(n);
        }
        let end = last
            .map(|l| self.doc.index_in_parent(l).unwrap() + 1)
            .unwrap_or(target.offset);
        let caret = self.normalize_keep(rules, Pos::new(target.node, end));
        self.sel = Selection::caret(caret);
        Ok(())
    }

    // ----------------------------------------------------------- structure moves

    /// Moves an element before its previous / after its next element sibling.
    pub fn move_element(&mut self, rules: &dyn Rules, el: NodeId, up: bool) -> EditResult {
        let sib = if up {
            self.prev_element_sibling(rules, el)
        } else {
            self.next_element_sibling(rules, el)
        };
        let sib = sib.ok_or(EditError::Nothing)?;
        self.transact(|ed| {
            let parent = ed.doc.parent(el).unwrap();
            ed.check_writable(rules, parent)?;
            let idx = ed.doc.index_in_parent(sib).unwrap();
            ed.doc.detach(el);
            let idx = if up {
                idx
            } else {
                ed.doc.index_in_parent(sib).unwrap() + 1
            };
            ed.doc.insert_child(parent, idx, el);
            ed.sel = Selection::caret(first_editable(&ed.doc, rules, el));
            Ok(())
        })
    }

    /// Selects the whole element (XMAX `SelectElement`).
    pub fn select_element(&mut self, el: NodeId) {
        let parent = self.doc.parent(el).unwrap();
        let i = self.doc.index_in_parent(el).unwrap();
        self.sel = Selection {
            anchor: Pos::new(parent, i),
            focus: Pos::new(parent, i + 1),
        };
    }

    pub fn select_contents(&mut self, rules: &dyn Rules, el: NodeId) {
        self.sel = Selection {
            anchor: start_of(&self.doc, rules, el),
            focus: end_of(&self.doc, rules, el),
        };
    }

    pub fn select_all(&mut self, rules: &dyn Rules) {
        if let Some(r) = self.doc.document_element() {
            self.select_contents(rules, r);
        }
    }

    /// Replaces the text in [a, b) (same text node) — used by find/replace and spell check.
    pub fn replace_text_range(
        &mut self,
        rules: &dyn Rules,
        node: NodeId,
        start: usize,
        end: usize,
        with: &str,
    ) -> EditResult {
        self.transact(|ed| {
            ed.check_writable(rules, node)?;
            let mut s = ed.doc.text(node).ok_or(EditError::Nothing)?.to_string();
            s.replace_range(start..end, with);
            ed.doc.set_text(node, s);
            ed.sel = Selection::caret(Pos::new(node, start + with.len()));
            Ok(())
        })
    }
}

/// The next content token after `p` that is still inside `scope`, skipping tags.
fn token_after_skipping_tags_within(
    doc: &Document,
    rules: &dyn Rules,
    p: Pos,
    scope: NodeId,
) -> Option<Token> {
    let mut cur = p;
    for _ in 0..100_000 {
        let t = token_after(doc, rules, cur)?;
        match t {
            Token::End(e) if e == scope => return None,
            Token::Char(..) | Token::Atom(_) => return Some(t),
            _ => cur = pos_after_token(doc, rules, t),
        }
    }
    None
}

#[cfg(test)]
mod tests;
