//! `[behaviour]` and `[pretty_print]`: how the editor treats elements.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A bulleted or numbered list: container, optional header and item element.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub item: String,
}

/// A definition list: container, optional header, term and definition elements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefinitionListDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    pub term: String,
    pub definition: String,
}

/// An image element and the attributes that carry its source and size.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageDef {
    pub element: String,
    pub source_attr: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_attr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_attr: Option<String>,
}

/// Automatic ID assignment for newly inserted elements.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdGeneration {
    /// Elements that receive an ID.
    pub elements: Vec<String>,
    /// Attribute that holds the ID.
    pub attr: String,
    /// Pattern with `{name}` (element name) and `{seq}` / `{seq:N}`
    /// (counter, zero padded to N digits).
    #[serde(default = "IdGeneration::default_pattern")]
    pub pattern: String,
}

impl IdGeneration {
    fn default_pattern() -> String {
        "{name}-{seq:4}".to_string()
    }

    /// Expands the pattern for `element` using counter value `seq`.
    pub fn generate(&self, element: &str, seq: u64) -> String {
        let mut out = String::new();
        let mut rest = self.pattern.as_str();
        while let Some(start) = rest.find('{') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let Some(end) = after.find('}') else {
                out.push_str(&rest[start..]);
                return out;
            };
            let token = &after[..end];
            match token {
                "name" => out.push_str(element),
                "seq" => out.push_str(&seq.to_string()),
                _ => match token
                    .strip_prefix("seq:")
                    .and_then(|w| w.parse::<usize>().ok())
                {
                    Some(width) => out.push_str(&format!("{seq:0width$}")),
                    None => {
                        out.push('{');
                        out.push_str(token);
                        out.push('}');
                    }
                },
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }
}

/// `[behaviour]`: editing semantics beyond what the schema says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Behaviour {
    /// What Enter inserts after an element: element -> following element.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub followed_by: BTreeMap<String, String>,
    /// Paragraph-like elements.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paragraphs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bulleted_lists: Vec<ListDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numbered_lists: Vec<ListDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub definition_lists: Vec<DefinitionListDef>,
    /// Inline toggles: kind (`bold`, `italic`, `underline`, `superscript`,
    /// `subscript`, ...) -> element name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub toggles: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageDef>,
    /// Elements the user may not edit.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub read_only_elements: Vec<String>,
    /// Elements the user may not delete.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub non_removable_elements: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_generation: Option<IdGeneration>,
    /// Assign IDs automatically on insertion (needs `id_generation`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_ids: bool,
}

/// `[pretty_print]`: serialisation layout.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrettyPrint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent_amount: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_line_length: Option<u32>,
    /// Never reformat documents on save.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_pretty_printing: bool,
    /// Per-element settings: `[pretty_print.elements.<name>]`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub elements: BTreeMap<String, ElementPrettyPrint>,
}

/// Layout flags for one element.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ElementPrettyPrint {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub indent_content: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub newline_before_start_tag: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub newline_before_end_tag: bool,
    /// Tag colour as written in the source customization.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_color: Option<String>,
}

impl ElementPrettyPrint {
    /// True when no flag is set, so the entry carries no information.
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_patterns() {
        let g = IdGeneration {
            elements: vec![],
            attr: "id".into(),
            pattern: "{name}-{seq:4}".into(),
        };
        assert_eq!(g.generate("fig", 7), "fig-0007");
        let g = IdGeneration {
            pattern: "x{seq}-{other}-{name".into(),
            ..g
        };
        assert_eq!(g.generate("fig", 12), "x12-{other}-{name");
    }
}
