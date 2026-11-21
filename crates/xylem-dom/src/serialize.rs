use crate::{Document, NodeId, NodeKind, escape_attr, escape_text};

pub(crate) fn write_node(doc: &Document, id: NodeId, out: &mut String) {
    let node = &doc.nodes[id.index()];
    match &node.kind {
        NodeKind::Document => {
            for &c in &node.children {
                write_node(doc, c, out);
            }
        }
        NodeKind::Element(e) => {
            let self_closing_raw = node
                .raw
                .as_deref()
                .map(|r| r.ends_with("/>"))
                .unwrap_or(false);
            let empty = node.children.is_empty();
            match &node.raw {
                Some(raw) if !(self_closing_raw && !empty) => out.push_str(raw),
                _ => {
                    out.push('<');
                    out.push_str(&e.name);
                    for a in &e.attrs {
                        match &a.raw {
                            Some(r) => out.push_str(r),
                            None => {
                                out.push(' ');
                                out.push_str(&a.name);
                                out.push_str("=\"");
                                escape_attr(&a.value, out);
                                out.push('"');
                            }
                        }
                    }
                    if empty && node.raw_end.is_none() {
                        out.push_str("/>");
                        return;
                    }
                    out.push('>');
                }
            }
            if self_closing_raw && empty {
                return;
            }
            for &c in &node.children {
                write_node(doc, c, out);
            }
            match &node.raw_end {
                Some(r) => out.push_str(r),
                None => {
                    out.push_str("</");
                    out.push_str(&e.name);
                    out.push('>');
                }
            }
        }
        kind => {
            if let Some(raw) = &node.raw {
                out.push_str(raw);
                return;
            }
            write_leaf(kind, out);
        }
    }
}

fn write_leaf(kind: &NodeKind, out: &mut String) {
    match kind {
        NodeKind::Text(t) => escape_text(t, out),
        NodeKind::CData(t) => {
            out.push_str("<![CDATA[");
            out.push_str(t);
            out.push_str("]]>");
        }
        NodeKind::Comment(t) => {
            out.push_str("<!--");
            out.push_str(t);
            out.push_str("-->");
        }
        NodeKind::Pi { target, data } => {
            out.push_str("<?");
            out.push_str(target);
            if !data.is_empty() {
                out.push(' ');
                out.push_str(data);
            }
            out.push_str("?>");
        }
        NodeKind::EntityRef(n) => {
            out.push('&');
            out.push_str(n);
            out.push(';');
        }
        NodeKind::Doctype(d) => {
            out.push_str("<!DOCTYPE ");
            out.push_str(&d.name);
            if let Some(p) = &d.public_id {
                out.push_str(&format!(" PUBLIC \"{p}\""));
                if let Some(s) = &d.system_id {
                    out.push_str(&format!(" \"{s}\""));
                }
            } else if let Some(s) = &d.system_id {
                out.push_str(&format!(" SYSTEM \"{s}\""));
            }
            if let Some(i) = &d.internal_subset {
                out.push_str(" [");
                out.push_str(i);
                out.push(']');
            }
            out.push('>');
        }
        NodeKind::Document | NodeKind::Element(_) => {}
    }
}

/// Fresh, indented serialisation (used by the source view's "pretty print").
pub(crate) fn pretty(doc: &Document, id: NodeId) -> String {
    let mut out = String::new();
    pretty_rec(doc, id, 0, &mut out);
    out
}

fn has_text_child(doc: &Document, id: NodeId) -> bool {
    doc.children(id).iter().any(|&c| match doc.kind(c) {
        NodeKind::Text(t) => !t.trim().is_empty(),
        NodeKind::CData(_) | NodeKind::EntityRef(_) => true,
        _ => false,
    })
}

fn pretty_rec(doc: &Document, id: NodeId, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    match doc.kind(id) {
        NodeKind::Document => {
            for &c in doc.children(id) {
                if matches!(doc.kind(c), NodeKind::Text(t) if t.trim().is_empty()) {
                    continue;
                }
                pretty_rec(doc, c, 0, out);
                out.push('\n');
            }
        }
        NodeKind::Element(e) => {
            out.push_str(&indent);
            if has_text_child(doc, id) {
                // Mixed content: emit inline, verbatim structure.
                let mut s = String::new();
                write_fresh(doc, id, &mut s);
                out.push_str(&s);
                return;
            }
            out.push('<');
            out.push_str(&e.name);
            for a in &e.attrs {
                out.push(' ');
                out.push_str(&a.name);
                out.push_str("=\"");
                escape_attr(&a.value, out);
                out.push('"');
            }
            let kids: Vec<_> = doc
                .children(id)
                .iter()
                .copied()
                .filter(|&c| !matches!(doc.kind(c), NodeKind::Text(t) if t.trim().is_empty()))
                .collect();
            if kids.is_empty() {
                out.push_str("/>");
                return;
            }
            out.push_str(">\n");
            for c in kids {
                pretty_rec(doc, c, depth + 1, out);
                out.push('\n');
            }
            out.push_str(&indent);
            out.push_str("</");
            out.push_str(&e.name);
            out.push('>');
        }
        k => {
            out.push_str(&indent);
            write_leaf(k, out);
        }
    }
}

fn write_fresh(doc: &Document, id: NodeId, out: &mut String) {
    match doc.kind(id) {
        NodeKind::Element(e) => {
            out.push('<');
            out.push_str(&e.name);
            for a in &e.attrs {
                out.push(' ');
                out.push_str(&a.name);
                out.push_str("=\"");
                escape_attr(&a.value, out);
                out.push('"');
            }
            if doc.children(id).is_empty() {
                out.push_str("/>");
                return;
            }
            out.push('>');
            for &c in doc.children(id) {
                write_fresh(doc, c, out);
            }
            out.push_str("</");
            out.push_str(&e.name);
            out.push('>');
        }
        k => write_leaf(k, out),
    }
}
