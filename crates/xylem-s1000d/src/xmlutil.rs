//! Small DOM navigation helpers shared by the S1000D modules.

use xylem_dom::{Document, NodeId};

pub(crate) fn child(doc: &Document, parent: NodeId, name: &str) -> Option<NodeId> {
    doc.element_children(parent).find(|&c| doc.name(c) == Some(name))
}

pub(crate) fn path(doc: &Document, start: NodeId, names: &[&str]) -> Option<NodeId> {
    names.iter().try_fold(start, |cur, name| child(doc, cur, name))
}

/// First descendant element called `name` in document order (not `start` itself).
pub(crate) fn descendant(doc: &Document, start: NodeId, name: &str) -> Option<NodeId> {
    doc.descendants(start).skip(1).find(|&n| doc.name(n) == Some(name))
}

/// Trimmed text content of an element.
pub(crate) fn text(doc: &Document, el: NodeId) -> String {
    doc.text_content(el).trim().to_string()
}

pub(crate) fn attr(doc: &Document, el: NodeId, name: &str) -> Option<String> {
    doc.attr(el, name).map(str::to_string)
}

/// Replaces the content of `el` with a single text node.
pub(crate) fn set_text(doc: &mut Document, el: NodeId, value: &str) {
    for c in doc.children(el).to_vec() {
        doc.detach(c);
    }
    let t = doc.create_text(value);
    doc.append_child(el, t);
}

/// Returns the existing child called `name`, or appends a new one.
pub(crate) fn ensure_child(doc: &mut Document, parent: NodeId, name: &str) -> NodeId {
    if let Some(c) = child(doc, parent, name) {
        return c;
    }
    let c = doc.create_element(name);
    doc.append_child(parent, c);
    c
}

/// Case-insensitive ASCII `starts_with` that returns the remainder.
pub(crate) fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    let head = s.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &s[prefix.len()..])
}
