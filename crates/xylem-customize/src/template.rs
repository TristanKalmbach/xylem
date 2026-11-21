//! Element templates: XML snippets inserted when the user adds an element.

use serde::{Deserialize, Serialize};
use xylem_dom::{Document, NodeId, NodeKind};

use crate::error::{Error, Result};

/// Processing-instruction target XMetaL uses for "type here" placeholders.
pub const PLACEHOLDER_TARGET: &str = "xm-replace_text";

/// A snippet of XML (possibly several top-level nodes) for one element.
///
/// Serialised as `[templates.<element>]` with a single `xml` key.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub xml: String,
}

impl Template {
    pub fn new(xml: impl Into<String>) -> Self {
        Self { xml: xml.into() }
    }

    /// Parses the snippet into `doc` and returns the new, detached top-level
    /// nodes. Placeholder processing instructions are preserved so the editor
    /// can show their prompt text (see [`placeholder_text`]).
    pub fn instantiate_into(&self, doc: &mut Document) -> Result<Vec<NodeId>> {
        doc.parse_fragment_into(&self.xml)
            .map_err(|e| Error::xml("template", e))
    }

    /// Checks that the snippet is a well-formed fragment.
    pub fn validate(&self) -> Result<()> {
        xylem_dom::parse_fragment(&self.xml)
            .map(drop)
            .map_err(|e| Error::xml("template", e))
    }

    /// Prompt texts of every placeholder in the snippet, in document order.
    pub fn placeholders(&self) -> Vec<String> {
        let Ok(frag) = xylem_dom::parse_fragment(&self.xml) else {
            return Vec::new();
        };
        frag.descendants(frag.root())
            .filter_map(|n| match frag.kind(n) {
                NodeKind::Pi { target, data } if target == PLACEHOLDER_TARGET => {
                    Some(placeholder_text(data))
                }
                _ => None,
            })
            .collect()
    }
}

/// Extracts the user-visible prompt from the data of an `xm-replace_text`
/// processing instruction.
///
/// The real corpus stores plain text with a trailing space
/// (`Type new para here `); XMetaL's documented brace/quote forms
/// (`{Type here}`, `"Type here"`) are unwrapped as well.
pub fn placeholder_text(pi_data: &str) -> String {
    let mut text = pi_data.trim();
    if let Some(inner) = text.strip_prefix('{') {
        text = inner.strip_suffix('}').unwrap_or(inner).trim();
    }
    for quote in ['"', '\''] {
        if let Some(inner) = text.strip_prefix(quote) {
            text = inner.strip_suffix(quote).unwrap_or(inner).trim();
            break;
        }
    }
    text.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_forms() {
        assert_eq!(
            placeholder_text("Type new para here "),
            "Type new para here"
        );
        assert_eq!(placeholder_text("{Type here}"), "Type here");
        assert_eq!(placeholder_text(" \"Type here\" "), "Type here");
        assert_eq!(placeholder_text("{ 'Quoted' "), "Quoted");
        assert_eq!(placeholder_text(""), "");
    }

    #[test]
    fn instantiates_multiple_top_level_nodes() {
        let t = Template::new(
            "<listSubTitle><?xm-replace_text Type new listSubTitle here ?></listSubTitle><listItem/>",
        );
        let mut doc = Document::new();
        let nodes = t.instantiate_into(&mut doc).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(doc.name(nodes[0]), Some("listSubTitle"));
        assert_eq!(doc.name(nodes[1]), Some("listItem"));
        assert!(doc.parent(nodes[0]).is_none());
        assert_eq!(t.placeholders(), vec!["Type new listSubTitle here"]);
    }

    #[test]
    fn malformed_snippet_is_an_error() {
        let t = Template::new("<cover><title1>x</title><date/></cover>");
        assert!(t.validate().is_err());
        assert!(t.instantiate_into(&mut Document::new()).is_err());
        assert!(t.placeholders().is_empty());
    }
}
