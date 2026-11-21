//! Pretty-printing as an editing operation.
//!
//! Only whitespace that carries no meaning is touched: the whitespace text
//! between children of element-only content. Mixed content and preformatted
//! elements are left exactly as they are, so formatting never changes what
//! the document says.

use crate::pos::is_ignorable;
use crate::{EditResult, Editor, Rules};
use xylem_dom::{Document, NodeId, NodeKind};

/// Settings for [`Editor::reformat`].
pub struct FormatOptions<'a> {
    /// Spaces per nesting level.
    pub indent: usize,
    /// Elements whose children are not indented further (XMAX's
    /// `IndentContent` switched off for that element).
    pub flat: &'a dyn Fn(&str) -> bool,
}

impl Editor {
    /// Re-indents the document. One undo step.
    pub fn reformat(&mut self, rules: &dyn Rules, opts: &FormatOptions) -> EditResult {
        self.transact(|ed| {
            if let Some(root) = ed.doc.document_element() {
                format_element(&mut ed.doc, rules, opts, root, 0);
            }
            ed.fix_selection(rules);
            Ok(())
        })
    }
}

/// Whitespace between the children of `el` is formatting, not content.
fn whitespace_is_formatting(doc: &Document, rules: &dyn Rules, el: NodeId) -> bool {
    if rules.is_preformatted(doc, el) {
        return false;
    }
    let kids = doc.children(el);
    let has_text = kids.iter().any(|&c| matches!(doc.kind(c), NodeKind::Text(t) if !t.trim().is_empty()) || matches!(doc.kind(c), NodeKind::CData(_) | NodeKind::EntityRef(_)));
    if has_text {
        return false;
    }
    if rules.is_element_only(doc, el) {
        return true;
    }
    // Without a schema: whitespace that already spans lines is layout.
    kids.iter().all(|&c| !doc.is_text(c) || is_ignorable(doc, rules, c))
}

fn format_element(doc: &mut Document, rules: &dyn Rules, opts: &FormatOptions, el: NodeId, depth: usize) {
    let name = doc.name(el).unwrap_or("").to_string();
    let inner = if (opts.flat)(&name) { depth } else { depth + 1 };
    if whitespace_is_formatting(doc, rules, el) && !doc.children(el).is_empty() {
        let kids: Vec<NodeId> = doc.children(el).to_vec();
        for &c in &kids {
            if doc.is_text(c) {
                doc.detach(c);
            }
        }
        let content: Vec<NodeId> = doc.children(el).to_vec();
        let pad = |n: usize| format!("\n{}", " ".repeat(n * opts.indent));
        for (i, &c) in content.iter().enumerate() {
            let at = doc.index_in_parent(c).unwrap_or(i * 2);
            let ws = doc.create_text(pad(inner));
            doc.insert_child(el, at, ws);
        }
        let end = doc.create_text(pad(depth));
        doc.append_child(el, end);
    }
    let children: Vec<NodeId> = doc.element_children(el).collect();
    for c in children {
        format_element(doc, rules, opts, c, inner);
    }
}
