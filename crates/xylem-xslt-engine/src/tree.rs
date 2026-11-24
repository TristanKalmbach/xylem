//! The engine's read-only XDM tree.
//!
//! `xylem-dom` is an editing model: attributes are not nodes, names are raw
//! QName strings and there are no namespace bindings. XPath needs all three,
//! plus several documents at once (source, `document()`, temporary trees)
//! with a stable document order between them. So each document is copied
//! once into a [`Tree`]: an arena in document order where attributes are
//! nodes and every name is resolved to its namespace.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use xylem_dom::{Document, NodeId, NodeKind};

pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// An expanded name. `prefix` is kept only to serialize it again.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QName {
    pub uri: Rc<str>,
    pub local: Rc<str>,
    pub prefix: Rc<str>,
}

impl QName {
    pub fn new(uri: &str, local: &str, prefix: &str) -> QName {
        QName {
            uri: uri.into(),
            local: local.into(),
            prefix: prefix.into(),
        }
    }
    pub fn local(local: &str) -> QName {
        QName::new("", local, "")
    }
    /// Same expanded name, ignoring the prefix.
    pub fn same(&self, other: &QName) -> bool {
        self.local == other.local && self.uri == other.uri
    }
    pub fn lexical(&self) -> String {
        if self.prefix.is_empty() {
            self.local.to_string()
        } else {
            format!("{}:{}", self.prefix, self.local)
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Document,
    Element,
    Attribute,
    Text,
    Comment,
    Pi,
}

#[derive(Clone, Debug)]
pub struct TNode {
    pub kind: Kind,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    pub attrs: Vec<u32>,
    /// Element, attribute or PI name (the PI target is `local`).
    pub name: Option<QName>,
    /// Text, comment, PI data or attribute value.
    pub value: String,
    /// Namespace bindings declared on (elements) or required by this node.
    pub ns: Vec<(Rc<str>, Rc<str>)>,
}

/// One document or temporary tree.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<TNode>,
    /// Base URI (folder or file) used to resolve `document()`.
    pub base: String,
    /// Per node, its number among the tree's non-attribute nodes in
    /// document order (see `generate-id` in the design notes).
    numbers: std::cell::OnceCell<Vec<u32>>,
    /// For stylesheet modules: per node, the line where an element's start
    /// tag ends (the line the reference processor reports), else empty.
    pub lines: Vec<u32>,
}

/// A node: which tree (index in the engine's tree list) and which node.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeRef {
    pub tree: u32,
    pub idx: u32,
}

impl Tree {
    pub fn new_document() -> Tree {
        let mut t = Tree::default();
        t.push(TNode::new(Kind::Document, None));
        t
    }

    pub fn push(&mut self, n: TNode) -> u32 {
        self.nodes.push(n);
        (self.nodes.len() - 1) as u32
    }

    pub fn node(&self, i: u32) -> &TNode {
        &self.nodes[i as usize]
    }

    /// The node's number for `generate-id()`: its position in document
    /// order counting every node except attributes, the document node
    /// being 0, with the reference processor's skipped numbers. An
    /// attribute gets its element's number.
    pub fn node_number(&self, i: u32) -> u32 {
        let nums = self.numbers.get_or_init(|| {
            // Spec note 1: under each parent, once eleven children have
            // been numbered since the last skip, the next element child
            // skips one number.
            let mut skip = vec![false; self.nodes.len()];
            for n in &self.nodes {
                let mut since = 0;
                for &c in &n.children {
                    if since >= 11 && self.node(c).kind == Kind::Element {
                        skip[c as usize] = true;
                        since = 0;
                    }
                    since += 1;
                }
            }
            let mut v = vec![0; self.nodes.len()];
            let mut next = 0;
            let mut stack = vec![0u32];
            while let Some(x) = stack.pop() {
                if skip[x as usize] {
                    next += 1;
                }
                v[x as usize] = next;
                next += 1;
                let n = self.node(x);
                for &a in &n.attrs {
                    v[a as usize] = v[x as usize];
                }
                stack.extend(n.children.iter().rev().copied());
            }
            v
        });
        nums.get(i as usize).copied().unwrap_or(i)
    }

    /// The string value (XPath `string()` of a node).
    pub fn string_value(&self, i: u32) -> String {
        let n = self.node(i);
        match n.kind {
            Kind::Document | Kind::Element => {
                let mut s = String::new();
                self.collect_text(i, &mut s);
                s
            }
            _ => n.value.clone(),
        }
    }

    fn collect_text(&self, i: u32, out: &mut String) {
        for &c in &self.node(i).children {
            let n = self.node(c);
            match n.kind {
                Kind::Text => out.push_str(&n.value),
                Kind::Element => self.collect_text(c, out),
                _ => {}
            }
        }
    }

    /// In-scope namespaces of an element, innermost binding first.
    pub fn in_scope(&self, i: u32) -> Vec<(Rc<str>, Rc<str>)> {
        let mut out: Vec<(Rc<str>, Rc<str>)> = Vec::new();
        let mut cur = Some(i);
        while let Some(c) = cur {
            let n = self.node(c);
            for (p, u) in &n.ns {
                if !out.iter().any(|(q, _)| q == p) {
                    out.push((p.clone(), u.clone()));
                }
            }
            cur = n.parent;
        }
        out
    }

    /// Resolves a prefix in scope at element `i` ("" = default namespace).
    pub fn resolve(&self, i: u32, prefix: &str) -> Option<Rc<str>> {
        if prefix == "xml" {
            return Some(XML_NS.into());
        }
        self.in_scope(i)
            .into_iter()
            .find(|(p, _)| &**p == prefix)
            .map(|(_, u)| u)
            .filter(|u| !u.is_empty() || prefix.is_empty())
    }
}

impl TNode {
    pub fn new(kind: Kind, parent: Option<u32>) -> TNode {
        TNode {
            kind,
            parent,
            children: Vec::new(),
            attrs: Vec::new(),
            name: None,
            value: String::new(),
            ns: Vec::new(),
        }
    }
}

/// Which whitespace-only text nodes of a source document to drop.
pub trait StripSpace {
    fn strip(&self, element: &QName) -> bool;
}

/// Copies a parsed `xylem-dom` document into a [`Tree`], resolving names.
pub fn from_dom(doc: &Document, base: &str, strip: &dyn StripSpace) -> Result<Tree, String> {
    from_dom_lines(doc, base, strip, None)
}

/// Like [`from_dom`], also recording each element's line from `lines`
/// (indexed by `xylem-dom` node).
pub fn from_dom_lines(
    doc: &Document,
    base: &str,
    strip: &dyn StripSpace,
    lines: Option<&[u32]>,
) -> Result<Tree, String> {
    let mut t = Tree::new_document();
    t.base = base.to_string();
    let b = std::path::Path::new(base);
    let ents = Entities {
        decls: doc
            .doctype()
            .map(|d| parse_entities(d.internal_subset.as_deref().unwrap_or("")))
            .unwrap_or_default(),
        dir: if b.is_dir() {
            b.to_path_buf()
        } else {
            b.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        },
        depth: 0,
    };
    let strip = &Strip {
        s: strip,
        ents: &ents,
        lines,
    };
    for &c in doc.children(doc.root()) {
        // Whitespace outside the document element is not part of the XDM.
        if matches!(doc.kind(c), NodeKind::Text(_) | NodeKind::CData(_)) {
            continue;
        }
        // An XML declaration after a byte order mark can surface as a PI.
        if matches!(doc.kind(c), NodeKind::Pi { target, .. } if target.eq_ignore_ascii_case("xml"))
        {
            continue;
        }
        copy(doc, c, &mut t, 0, strip, false)?;
    }
    Ok(t)
}

/// General entities declared in a document's internal subset.
struct Entities {
    decls: HashMap<String, Entity>,
    dir: PathBuf,
    depth: usize,
}

enum Entity {
    Internal(String),
    External(String),
}

struct Strip<'a> {
    s: &'a dyn StripSpace,
    ents: &'a Entities,
    lines: Option<&'a [u32]>,
}

fn parse_entities(subset: &str) -> HashMap<String, Entity> {
    let mut out = HashMap::new();
    let mut rest = subset;
    let quoted = |s: &str| -> Option<String> {
        let s = s.trim_start();
        let q = s.chars().next()?;
        if q != '"' && q != '\'' {
            return None;
        }
        let body = &s[1..];
        Some(body[..body.find(q)?].to_string())
    };
    while let Some(at) = rest.find("<!ENTITY") {
        rest = &rest[at + 8..];
        // The declaration ends at the first `>` outside a quoted value.
        let mut quote = None;
        let end = rest
            .char_indices()
            .find(|&(_, c)| match quote {
                Some(q) if c == q => {
                    quote = None;
                    false
                }
                Some(_) => false,
                None if c == '"' || c == '\'' => {
                    quote = Some(c);
                    false
                }
                None => c == '>',
            })
            .map_or(rest.len(), |(i, _)| i);
        let decl = rest[..end].trim_start();
        rest = &rest[end..];
        let Some(name) = decl.split_whitespace().next() else {
            continue;
        };
        if name == "%" {
            continue;
        }
        let after = decl[name.len()..].trim_start();
        if let Some(sys) = after.strip_prefix("SYSTEM") {
            if let Some(uri) = quoted(sys) {
                out.insert(name.to_string(), Entity::External(uri));
            }
        } else if let Some(v) = quoted(after) {
            out.insert(name.to_string(), Entity::Internal(v));
        }
    }
    out
}

/// Parses an entity's replacement text in the namespace scope of `parent`
/// and copies its nodes there. References inside it resolve against the
/// same declarations.
fn expand(
    name: &str,
    t: &mut Tree,
    parent: u32,
    strip: &Strip,
    preserve: bool,
) -> Result<(), String> {
    let ents = strip.ents;
    if ents.depth > 16 {
        return Err(format!("entity '{name}' nests too deeply"));
    }
    let (text, _dir) = match ents.decls.get(name) {
        None => return Err(format!("undeclared entity '&{name};'")),
        Some(Entity::Internal(v)) => (v.clone(), ents.dir.clone()),
        Some(Entity::External(uri)) => {
            let path = ents.dir.join(uri.strip_prefix("file:///").unwrap_or(uri));
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("entity '{name}' ({}): {e}", path.display()))?;
            let mut s = String::from_utf8_lossy(&bytes)
                .trim_start_matches('\u{feff}')
                .to_string();
            // Drop the text declaration.
            if s.starts_with("<?xml")
                && let Some(e) = s.find("?>")
            {
                s = s[e + 2..].to_string();
            }
            (
                s,
                path.parent().map(|p| p.to_path_buf()).unwrap_or_default(),
            )
        }
    };
    let mut wrapper = String::from("<w");
    if t.node(parent).kind == Kind::Element {
        for (p, u) in t.in_scope(parent) {
            if p.is_empty() {
                wrapper.push_str(&format!(" xmlns=\"{u}\""));
            } else {
                wrapper.push_str(&format!(" xmlns:{p}=\"{u}\""));
            }
        }
    }
    wrapper.push('>');
    wrapper.push_str(&text);
    wrapper.push_str("</w>");
    let frag = Document::from_str(&wrapper).map_err(|e| format!("entity '{name}': {e}"))?;
    let w = frag.document_element().ok_or("empty entity")?;
    for &c in frag.children(w) {
        copy(&frag, c, t, parent, strip, preserve)?;
    }
    Ok(())
}

fn copy(
    doc: &Document,
    id: NodeId,
    t: &mut Tree,
    parent: u32,
    strip: &Strip,
    preserve: bool,
) -> Result<(), String> {
    match doc.kind(id) {
        NodeKind::Element(el) => {
            let me = t.push(TNode::new(Kind::Element, Some(parent)));
            t.nodes[parent as usize].children.push(me);
            let mut preserve = preserve;
            for a in &el.attrs {
                if a.name == "xmlns" {
                    t.nodes[me as usize]
                        .ns
                        .push(("".into(), a.value.as_str().into()));
                } else if let Some(p) = a.name.strip_prefix("xmlns:") {
                    t.nodes[me as usize]
                        .ns
                        .push((p.into(), a.value.as_str().into()));
                }
            }
            let (prefix, local) = split(&el.name);
            let uri = t.resolve(me, prefix).unwrap_or_else(|| "".into());
            if !prefix.is_empty() && uri.is_empty() {
                return Err(format!("undeclared namespace prefix '{prefix}'"));
            }
            let name = QName::new(&uri, local, prefix);
            for a in &el.attrs {
                if a.name == "xmlns" || a.name.starts_with("xmlns:") {
                    continue;
                }
                let (p, l) = split(&a.name);
                let u = if p.is_empty() {
                    "".into()
                } else {
                    t.resolve(me, p)
                        .ok_or_else(|| format!("undeclared namespace prefix '{p}'"))?
                };
                if a.name == "xml:space" {
                    preserve = a.value == "preserve";
                }
                let mut an = TNode::new(Kind::Attribute, Some(me));
                an.name = Some(QName::new(&u, l, p));
                an.value = a.value.clone();
                let ai = t.push(an);
                t.nodes[me as usize].attrs.push(ai);
            }
            let stripping = !preserve && strip.s.strip(&name);
            t.nodes[me as usize].name = Some(name);
            if let Some(l) = strip.lines {
                if t.lines.len() <= me as usize {
                    t.lines.resize(me as usize + 1, 0);
                }
                t.lines[me as usize] = l.get(id.index()).copied().unwrap_or(0);
            }
            for &c in doc.children(id) {
                if stripping
                    && let NodeKind::Text(s) = doc.kind(c)
                    && s.chars().all(is_xml_space)
                {
                    continue;
                }
                copy(doc, c, t, me, strip, preserve)?;
            }
        }
        // `xylem-dom` keeps line ends as written; XML parsers report CR LF
        // as LF. A lone CR can only come from `&#13;` and is kept.
        NodeKind::Text(s) | NodeKind::CData(s) => {
            if s.contains("\r\n") {
                append_text(t, parent, &s.replace("\r\n", "\n"))
            } else {
                append_text(t, parent, s)
            }
        }
        NodeKind::EntityRef(name) => expand(name, t, parent, strip, preserve)?,
        NodeKind::Comment(s) => {
            let mut n = TNode::new(Kind::Comment, Some(parent));
            n.value = s.clone();
            let i = t.push(n);
            t.nodes[parent as usize].children.push(i);
        }
        NodeKind::Pi { target, data } => {
            let mut n = TNode::new(Kind::Pi, Some(parent));
            n.name = Some(QName::local(target));
            n.value = data.clone();
            let i = t.push(n);
            t.nodes[parent as usize].children.push(i);
        }
        NodeKind::Document | NodeKind::Doctype(_) => {}
    }
    Ok(())
}

/// Appends text, merging with a preceding text node.
pub fn append_text(t: &mut Tree, parent: u32, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(&last) = t.nodes[parent as usize].children.last()
        && t.nodes[last as usize].kind == Kind::Text
    {
        t.nodes[last as usize].value.push_str(s);
        return;
    }
    let mut n = TNode::new(Kind::Text, Some(parent));
    n.value = s.to_string();
    let i = t.push(n);
    t.nodes[parent as usize].children.push(i);
}

pub fn split(qname: &str) -> (&str, &str) {
    qname.split_once(':').unwrap_or(("", qname))
}

pub fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}
