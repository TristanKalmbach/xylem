//! Lossless, editable XML document model.
//!
//! Nodes live in an arena and are addressed by [`NodeId`]. Every node
//! remembers the exact source text it was parsed from; serialisation emits
//! that text verbatim until the node is modified, so untouched regions of a
//! document round-trip byte for byte.

mod parse;
mod serialize;

pub use parse::{ParseError, parse, parse_fragment};

use std::fmt;

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct NodeId(u32);

impl NodeId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Attr {
    pub name: String,
    pub value: String,
    /// Exact source text of ` name="value"` including leading whitespace.
    pub(crate) raw: Option<String>,
}

impl Attr {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            raw: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<Attr>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.value.as_str())
    }
    pub fn local_name(&self) -> &str {
        local_name(&self.name)
    }
    pub fn prefix(&self) -> Option<&str> {
        self.name.split_once(':').map(|(p, _)| p)
    }
}

pub fn local_name(qname: &str) -> &str {
    qname.rsplit_once(':').map(|(_, l)| l).unwrap_or(qname)
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Doctype {
    pub name: String,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    /// Text between `[` and `]`, if any.
    pub internal_subset: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Document,
    Element(Element),
    Text(String),
    CData(String),
    Comment(String),
    Pi {
        target: String,
        data: String,
    },
    Doctype(Doctype),
    /// A general entity reference that is not predefined (`&foo;`).
    EntityRef(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: NodeKind,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    /// Source text for leaf nodes / start tag for elements.
    pub(crate) raw: Option<String>,
    /// Source text of the end tag (elements only).
    pub(crate) raw_end: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Latin1,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub(crate) nodes: Vec<Node>,
    root: NodeId,
    pub encoding: Encoding,
    /// Incremented on every mutation; cheap change detection for views.
    revision: u64,
    /// Incremented on every change except text edits (styles depend only on
    /// structure and attributes).
    structure_revision: u64,
    /// Active change journal (see [`Document::begin_journal`]).
    journal: Option<Vec<Op>>,
    /// Per node: the revision at which the node or anything inside it last
    /// changed. Views use this to reuse work for untouched subtrees.
    stamps: Vec<u64>,
}

/// One recorded primitive change, used to revert edits without snapshots.
#[derive(Clone, Debug)]
pub enum Op {
    /// `node` was attached to a parent.
    Inserted { node: NodeId },
    /// `node` was removed from `parent` at `index`.
    Detached {
        node: NodeId,
        parent: NodeId,
        index: usize,
    },
    /// `node`'s content (name, attributes, text) changed; holds the old state.
    Changed {
        node: NodeId,
        kind: NodeKind,
        raw: Option<String>,
        raw_end: Option<String>,
    },
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        // Every document gets its own range of revision numbers, so a
        // revision (or subtree stamp) never means the same thing for two
        // different documents. Views key their caches on these numbers and
        // rely on that when a document is replaced wholesale.
        static NEXT_BASE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let base = NEXT_BASE.fetch_add(1 << 32, std::sync::atomic::Ordering::Relaxed);
        let root = Node {
            kind: NodeKind::Document,
            parent: None,
            children: Vec::new(),
            raw: None,
            raw_end: None,
        };
        Self {
            nodes: vec![root],
            root: NodeId(0),
            encoding: Encoding::Utf8,
            revision: base,
            structure_revision: base,
            journal: None,
            stamps: vec![base],
        }
    }

    /// Parses a complete document from bytes, detecting the encoding.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let (text, enc) = decode(bytes);
        let mut doc = parse(&text)?;
        doc.encoding = enc;
        Ok(doc)
    }

    /// Parses a complete document from a string. (Also available through
    /// [`std::str::FromStr`], i.e. `text.parse::<Document>()`.)
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Result<Self, ParseError> {
        parse(text)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The revision at which `id` or any of its descendants last changed.
    pub fn subtree_stamp(&self, id: NodeId) -> u64 {
        self.stamps.get(id.index()).copied().unwrap_or(u64::MAX)
    }

    /// Records a change at `id`: it and all its ancestors get the current
    /// revision as their stamp.
    fn mark_changed(&mut self, id: NodeId) {
        let rev = self.revision;
        let mut cur = Some(id);
        while let Some(n) = cur {
            self.stamps[n.index()] = rev;
            cur = self.nodes[n.index()].parent;
        }
    }

    /// Changes only when structure, names or attributes change.
    pub fn structure_revision(&self) -> u64 {
        self.structure_revision
    }

    fn touch(&mut self) {
        self.revision += 1;
        self.structure_revision += 1;
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    /// The single top-level element, if present.
    pub fn document_element(&self) -> Option<NodeId> {
        self.children(self.root)
            .iter()
            .copied()
            .find(|&c| self.is_element(c))
    }

    pub fn doctype(&self) -> Option<&Doctype> {
        self.children(self.root)
            .iter()
            .find_map(|&c| match self.kind(c) {
                NodeKind::Doctype(d) => Some(d),
                _ => None,
            })
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn contains(&self, id: NodeId) -> bool {
        id.index() < self.nodes.len()
    }

    fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn kind(&self, id: NodeId) -> &NodeKind {
        &self.node(id).kind
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).parent
    }

    pub fn children(&self, id: NodeId) -> &[NodeId] {
        &self.node(id).children
    }

    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.node(id).kind {
            NodeKind::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn is_element(&self, id: NodeId) -> bool {
        matches!(self.node(id).kind, NodeKind::Element(_))
    }

    pub fn name(&self, id: NodeId) -> Option<&str> {
        self.element(id).map(|e| e.name.as_str())
    }

    pub fn attr(&self, id: NodeId, name: &str) -> Option<&str> {
        self.element(id).and_then(|e| e.attr(name))
    }

    pub fn text(&self, id: NodeId) -> Option<&str> {
        match &self.node(id).kind {
            NodeKind::Text(t) | NodeKind::CData(t) => Some(t),
            _ => None,
        }
    }

    pub fn is_text(&self, id: NodeId) -> bool {
        matches!(self.node(id).kind, NodeKind::Text(_) | NodeKind::CData(_))
    }

    pub fn element_children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.children(id)
            .iter()
            .copied()
            .filter(move |&c| self.is_element(c))
    }

    pub fn index_in_parent(&self, id: NodeId) -> Option<usize> {
        let p = self.parent(id)?;
        self.children(p).iter().position(|&c| c == id)
    }

    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.index_in_parent(id)?;
        self.children(p).get(i + 1).copied()
    }

    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeId> {
        let p = self.parent(id)?;
        let i = self.index_in_parent(id)?;
        i.checked_sub(1).map(|j| self.children(p)[j])
    }

    pub fn ancestors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.parent(id), move |&n| self.parent(n))
    }

    /// True if `anc` is `node` or an ancestor of it.
    pub fn is_ancestor_or_self(&self, anc: NodeId, node: NodeId) -> bool {
        anc == node || self.ancestors(node).any(|a| a == anc)
    }

    pub fn depth(&self, id: NodeId) -> usize {
        self.ancestors(id).count()
    }

    /// Pre-order traversal including `id` itself.
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            stack: vec![id],
        }
    }

    pub fn text_content(&self, id: NodeId) -> String {
        let mut s = String::new();
        for n in self.descendants(id) {
            if let Some(t) = self.text(n) {
                s.push_str(t);
            }
        }
        s
    }

    /// Path like `/dmodule/content/description/levelledPara[2]`.
    pub fn path(&self, id: NodeId) -> String {
        let mut parts = Vec::new();
        let mut cur = Some(id);
        while let Some(n) = cur {
            if let Some(name) = self.name(n) {
                let p = self.parent(n);
                let same: Vec<_> = p
                    .map(|p| {
                        self.element_children(p)
                            .filter(|&c| self.name(c) == Some(name))
                            .collect()
                    })
                    .unwrap_or_default();
                if same.len() > 1 {
                    let i = same.iter().position(|&c| c == n).unwrap_or(0) + 1;
                    parts.push(format!("{name}[{i}]"));
                } else {
                    parts.push(name.to_string());
                }
            }
            cur = self.parent(n);
        }
        parts.reverse();
        format!("/{}", parts.join("/"))
    }

    /// Document order comparison.
    pub fn cmp_order(&self, a: NodeId, b: NodeId) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        if a == b {
            return Ordering::Equal;
        }
        let pa: Vec<NodeId> = {
            let mut v: Vec<_> = self.ancestors(a).collect();
            v.reverse();
            v.push(a);
            v
        };
        let pb: Vec<NodeId> = {
            let mut v: Vec<_> = self.ancestors(b).collect();
            v.reverse();
            v.push(b);
            v
        };
        let mut i = 0;
        while i < pa.len() && i < pb.len() && pa[i] == pb[i] {
            i += 1;
        }
        if i == pa.len() {
            return Ordering::Less; // a is ancestor of b
        }
        if i == pb.len() {
            return Ordering::Greater;
        }
        let parent = pa[i - 1];
        let ch = self.children(parent);
        let ia = ch.iter().position(|&c| c == pa[i]);
        let ib = ch.iter().position(|&c| c == pb[i]);
        ia.cmp(&ib)
    }

    // ---------------------------------------------------------------- creation

    fn alloc(&mut self, kind: NodeKind) -> NodeId {
        let id = NodeId(self.nodes.len() as u32);
        self.stamps.push(self.revision);
        self.nodes.push(Node {
            kind,
            parent: None,
            children: Vec::new(),
            raw: None,
            raw_end: None,
        });
        id
    }

    pub fn create_element(&mut self, name: impl Into<String>) -> NodeId {
        self.alloc(NodeKind::Element(Element {
            name: name.into(),
            attrs: Vec::new(),
        }))
    }

    pub fn create_text(&mut self, text: impl Into<String>) -> NodeId {
        self.alloc(NodeKind::Text(text.into()))
    }

    pub fn create_node(&mut self, kind: NodeKind) -> NodeId {
        self.alloc(kind)
    }

    // ---------------------------------------------------------------- journal

    /// Starts recording primitive changes. Returns false if a journal is
    /// already active (nested transactions share the outer journal).
    pub fn begin_journal(&mut self) -> bool {
        if self.journal.is_some() {
            return false;
        }
        self.journal = Some(Vec::new());
        true
    }

    /// Stops recording and returns the recorded operations.
    pub fn take_journal(&mut self) -> Vec<Op> {
        self.journal.take().unwrap_or_default()
    }

    fn record(&mut self, op: Op) {
        if let Some(j) = &mut self.journal {
            j.push(op);
        }
    }

    fn record_change(&mut self, id: NodeId) {
        if self.journal.is_some() {
            let n = &self.nodes[id.index()];
            let op = Op::Changed {
                node: id,
                kind: n.kind.clone(),
                raw: n.raw.clone(),
                raw_end: n.raw_end.clone(),
            };
            self.record(op);
        }
    }

    /// Undoes recorded operations (newest first). If a journal is active the
    /// reverting operations are recorded, which yields the matching redo.
    pub fn revert(&mut self, ops: &[Op]) {
        for op in ops.iter().rev() {
            match op {
                Op::Inserted { node } => self.detach(*node),
                Op::Detached {
                    node,
                    parent,
                    index,
                } => self.insert_child(*parent, *index, *node),
                Op::Changed {
                    node,
                    kind,
                    raw,
                    raw_end,
                } => {
                    self.record_change(*node);
                    let n = self.node_mut(*node);
                    n.kind = kind.clone();
                    n.raw = raw.clone();
                    n.raw_end = raw_end.clone();
                    self.touch();
                    self.mark_changed(*node);
                }
            }
        }
    }

    // ---------------------------------------------------------------- mutation

    /// Inserts a detached node as child `index` of `parent`.
    pub fn insert_child(&mut self, parent: NodeId, index: usize, child: NodeId) {
        self.detach(child);
        let idx = index.min(self.children(parent).len());
        self.node_mut(parent).children.insert(idx, child);
        self.node_mut(child).parent = Some(parent);
        self.record(Op::Inserted { node: child });
        self.touch();
        self.mark_changed(parent);
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let n = self.children(parent).len();
        self.insert_child(parent, n, child);
    }

    /// Removes a node from its parent (it stays in the arena, detached).
    pub fn detach(&mut self, id: NodeId) {
        if let Some(p) = self.node(id).parent {
            let index = self.children(p).iter().position(|&c| c == id).unwrap_or(0);
            self.node_mut(p).children.remove(index);
            self.node_mut(id).parent = None;
            self.record(Op::Detached {
                node: id,
                parent: p,
                index,
            });
            self.touch();
            self.mark_changed(p);
        }
    }

    /// Replaces `id` in its parent by its children.
    pub fn unwrap_node(&mut self, id: NodeId) {
        let Some(p) = self.parent(id) else { return };
        let idx = self.index_in_parent(id).unwrap();
        let kids = self.children(id).to_vec();
        self.detach(id);
        for (k, c) in kids.into_iter().enumerate() {
            self.insert_child(p, idx + k, c);
        }
        self.normalize(p);
    }

    /// Wraps children `start..end` of `parent` in a new element.
    pub fn wrap_children(
        &mut self,
        parent: NodeId,
        start: usize,
        end: usize,
        name: &str,
    ) -> NodeId {
        let el = self.create_element(name);
        let moved: Vec<NodeId> = self.children(parent)[start..end].to_vec();
        for c in &moved {
            self.detach(*c);
        }
        for c in moved {
            self.append_child(el, c);
        }
        self.insert_child(parent, start, el);
        el
    }

    pub fn set_text(&mut self, id: NodeId, text: impl Into<String>) {
        self.record_change(id);
        let n = self.node_mut(id);
        match &mut n.kind {
            NodeKind::Text(t) | NodeKind::CData(t) | NodeKind::Comment(t) => *t = text.into(),
            NodeKind::Pi { data, .. } => *data = text.into(),
            _ => return,
        }
        n.raw = None;
        // Text edits do not affect styles; PI data and comments can.
        if matches!(n.kind, NodeKind::Text(_) | NodeKind::CData(_)) {
            self.revision += 1;
        } else {
            self.touch();
        }
        self.mark_changed(id);
    }

    pub fn rename(&mut self, id: NodeId, name: impl Into<String>) {
        self.record_change(id);
        let n = self.node_mut(id);
        if let NodeKind::Element(e) = &mut n.kind {
            e.name = name.into();
            n.raw = None;
            n.raw_end = None;
        }
        self.touch();
        self.mark_changed(id);
    }

    pub fn set_attr(&mut self, id: NodeId, name: &str, value: impl Into<String>) {
        let value = value.into();
        if self.attr(id, name) == Some(value.as_str()) {
            return;
        }
        self.record_change(id);
        let n = self.node_mut(id);
        if let NodeKind::Element(e) = &mut n.kind {
            if let Some(a) = e.attrs.iter_mut().find(|a| a.name == name) {
                if a.value == value {
                    return;
                }
                a.value = value;
                a.raw = None;
            } else {
                e.attrs.push(Attr::new(name, value));
            }
            n.raw = None;
        }
        self.touch();
        self.mark_changed(id);
    }

    pub fn remove_attr(&mut self, id: NodeId, name: &str) {
        if self.attr(id, name).is_none() {
            return;
        }
        self.record_change(id);
        let n = self.node_mut(id);
        if let NodeKind::Element(e) = &mut n.kind {
            let before = e.attrs.len();
            e.attrs.retain(|a| a.name != name);
            if e.attrs.len() != before {
                n.raw = None;
            }
        }
        self.touch();
        self.mark_changed(id);
    }

    /// Merges adjacent text nodes and drops empty ones among `parent`'s children.
    pub fn normalize(&mut self, parent: NodeId) {
        let kids = self.children(parent).to_vec();
        let mut prev_text: Option<NodeId> = None;
        for c in kids {
            if let NodeKind::Text(t) = self.kind(c) {
                if t.is_empty() {
                    self.detach(c);
                    continue;
                }
                if let Some(p) = prev_text {
                    let add = t.clone();
                    let mut merged = self.text(p).unwrap().to_string();
                    merged.push_str(&add);
                    self.set_text(p, merged);
                    self.detach(c);
                    continue;
                }
                prev_text = Some(c);
            } else {
                prev_text = None;
            }
        }
    }

    /// Splits a text node at a byte offset, returning the new right-hand node.
    pub fn split_text(&mut self, id: NodeId, at: usize) -> NodeId {
        let t = self.text(id).unwrap_or("").to_string();
        let (l, r) = t.split_at(at.min(t.len()));
        let (l, r) = (l.to_string(), r.to_string());
        self.set_text(id, l);
        let right = self.create_text(r);
        let p = self.parent(id).unwrap();
        let i = self.index_in_parent(id).unwrap();
        self.insert_child(p, i + 1, right);
        right
    }

    /// Deep copies a subtree (possibly from another document) into this one, detached.
    pub fn import(&mut self, src: &Document, id: NodeId) -> NodeId {
        let sn = src.node(id);
        let new = self.alloc(sn.kind.clone());
        self.node_mut(new).raw = sn.raw.clone();
        self.node_mut(new).raw_end = sn.raw_end.clone();
        for &c in &sn.children {
            let nc = self.import(src, c);
            self.node_mut(new).children.push(nc);
            self.node_mut(nc).parent = Some(new);
        }
        new
    }

    pub fn deep_clone(&mut self, id: NodeId) -> NodeId {
        let src = self.clone();
        self.import(&src, id)
    }

    /// Parses `xml` as a fragment and returns the new detached top-level nodes.
    pub fn parse_fragment_into(&mut self, xml: &str) -> Result<Vec<NodeId>, ParseError> {
        let frag = parse_fragment(xml)?;
        let tops = frag.children(frag.root()).to_vec();
        Ok(tops.into_iter().map(|t| self.import(&frag, t)).collect())
    }

    /// A copy with the same node IDs and structure but without text content
    /// or preserved source text. Much cheaper than `clone()`, and enough for
    /// work that only looks at names, attributes and processing instructions
    /// (styling, for example).
    pub fn structure_copy(&self) -> Document {
        let nodes = self
            .nodes
            .iter()
            .map(|n| Node {
                kind: match &n.kind {
                    NodeKind::Text(_) => NodeKind::Text(String::new()),
                    NodeKind::CData(_) => NodeKind::CData(String::new()),
                    other => other.clone(),
                },
                parent: n.parent,
                children: n.children.clone(),
                raw: None,
                raw_end: None,
            })
            .collect();
        Document {
            nodes,
            root: self.root,
            encoding: self.encoding,
            revision: self.revision,
            structure_revision: self.structure_revision,
            journal: None,
            stamps: Vec::new(),
        }
    }

    /// Serialises the whole document.
    pub fn to_xml(&self) -> String {
        let mut s = String::new();
        serialize::write_node(self, self.root, &mut s);
        s
    }

    /// Serialises the document into bytes using its original encoding.
    pub fn to_bytes(&self) -> Vec<u8> {
        encode(&self.to_xml(), self.encoding)
    }

    /// Serialises a single node (and subtree).
    pub fn node_to_xml(&self, id: NodeId) -> String {
        let mut s = String::new();
        serialize::write_node(self, id, &mut s);
        s
    }

    /// Serialises a node freshly (ignoring preserved source), indented.
    pub fn pretty_xml(&self, id: NodeId) -> String {
        serialize::pretty(self, id)
    }
}

impl std::str::FromStr for Document {
    type Err = ParseError;
    fn from_str(text: &str) -> Result<Self, ParseError> {
        parse(text)
    }
}

pub struct Descendants<'a> {
    doc: &'a Document,
    stack: Vec<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let n = self.stack.pop()?;
        for &c in self.doc.children(n).iter().rev() {
            self.stack.push(c);
        }
        Some(n)
    }
}

pub fn escape_text(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

pub fn escape_attr(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\t' => out.push_str("&#9;"),
            _ => out.push(ch),
        }
    }
}

fn decode(bytes: &[u8]) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return (
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        );
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let le = bytes[0] == 0xFF;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        let s: String = char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{FFFD}'))
            .collect();
        return (
            s,
            if le {
                Encoding::Utf16Le
            } else {
                Encoding::Utf16Be
            },
        );
    }
    // Declared encoding sniffing.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]).to_ascii_lowercase();
    let latin = head.starts_with("<?xml")
        && (head.contains("iso-8859-1")
            || head.contains("latin1")
            || head.contains("windows-1252"));
    match std::str::from_utf8(bytes) {
        Ok(s) if !latin => (s.to_string(), Encoding::Utf8),
        _ if latin || std::str::from_utf8(bytes).is_err() => {
            (bytes.iter().map(|&b| b as char).collect(), Encoding::Latin1)
        }
        _ => (String::from_utf8_lossy(bytes).into_owned(), Encoding::Utf8),
    }
}

fn encode(s: &str, enc: Encoding) -> Vec<u8> {
    match enc {
        Encoding::Utf8 => s.as_bytes().to_vec(),
        Encoding::Utf8Bom => {
            let mut v = vec![0xEF, 0xBB, 0xBF];
            v.extend_from_slice(s.as_bytes());
            v
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let le = enc == Encoding::Utf16Le;
            let mut v = if le {
                vec![0xFF, 0xFE]
            } else {
                vec![0xFE, 0xFF]
            };
            for u in s.encode_utf16() {
                v.extend_from_slice(&if le { u.to_le_bytes() } else { u.to_be_bytes() });
            }
            v
        }
        Encoding::Latin1 => {
            let mut v = Vec::with_capacity(s.len());
            for ch in s.chars() {
                if (ch as u32) < 256 {
                    v.push(ch as u8);
                } else {
                    v.extend_from_slice(format!("&#{};", ch as u32).as_bytes());
                }
            }
            v
        }
    }
}

#[cfg(test)]
mod tests;
