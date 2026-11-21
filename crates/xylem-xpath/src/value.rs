//! XPath data model: nodes and values, plus the conversions between them.

use xylem_dom::{Document, NodeId, NodeKind};

/// A node in the XPath data model.
///
/// Attributes are not nodes in the DOM, so they are addressed by their owning
/// element and their index in the element's attribute list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum XNode {
    /// Document, element, text, comment or processing-instruction node.
    Node(NodeId),
    /// The `n`th attribute of an element.
    Attr(NodeId, usize),
}

impl XNode {
    /// The DOM node, if this is not an attribute.
    pub fn node(self) -> Option<NodeId> {
        match self {
            XNode::Node(id) => Some(id),
            XNode::Attr(..) => None,
        }
    }

    /// The element owning an attribute, or the node itself.
    pub fn owner(self) -> NodeId {
        match self {
            XNode::Node(id) | XNode::Attr(id, _) => id,
        }
    }

    /// True for attribute nodes.
    pub fn is_attr(self) -> bool {
        matches!(self, XNode::Attr(..))
    }
}

impl From<NodeId> for XNode {
    fn from(id: NodeId) -> Self {
        XNode::Node(id)
    }
}

/// The result of evaluating an XPath expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// Nodes in document order, without duplicates.
    NodeSet(Vec<XNode>),
    Boolean(bool),
    Number(f64),
    String(String),
}

impl Value {
    /// The XPath `boolean()` conversion.
    pub fn to_boolean(&self) -> bool {
        match self {
            Value::NodeSet(n) => !n.is_empty(),
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
        }
    }

    /// The XPath `number()` conversion.
    pub fn to_number(&self, doc: &Document) -> f64 {
        match self {
            Value::Number(n) => *n,
            Value::Boolean(b) => f64::from(u8::from(*b)),
            Value::String(s) => string_to_number(s),
            Value::NodeSet(_) => string_to_number(&self.to_string_value(doc)),
        }
    }

    /// The XPath `string()` conversion.
    pub fn to_string_value(&self, doc: &Document) -> String {
        match self {
            Value::String(s) => s.clone(),
            Value::Boolean(b) => b.to_string(),
            Value::Number(n) => number_to_string(*n),
            Value::NodeSet(nodes) => nodes
                .first()
                .map(|&n| string_value(doc, n))
                .unwrap_or_default(),
        }
    }

    /// The nodes of a node-set value.
    pub fn as_node_set(&self) -> Option<&[XNode]> {
        match self {
            Value::NodeSet(n) => Some(n),
            _ => None,
        }
    }

    /// Consumes the value, returning the nodes of a node-set.
    pub fn into_node_set(self) -> Option<Vec<XNode>> {
        match self {
            Value::NodeSet(n) => Some(n),
            _ => None,
        }
    }

    pub(crate) fn type_name(&self) -> &'static str {
        match self {
            Value::NodeSet(_) => "node-set",
            Value::Boolean(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Value::String(s)
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Value::String(s.to_string())
    }
}

impl From<Vec<XNode>> for Value {
    fn from(n: Vec<XNode>) -> Self {
        Value::NodeSet(n)
    }
}

/// True for DOM nodes that exist in the XPath data model (excludes DOCTYPE, entity
/// references and the XML declaration).
pub(crate) fn is_xpath_node(doc: &Document, id: NodeId) -> bool {
    match doc.kind(id) {
        NodeKind::Doctype(_) | NodeKind::EntityRef(_) => false,
        // The XML declaration is not a processing instruction in the data model.
        NodeKind::Pi { target, .. } => !target.eq_ignore_ascii_case("xml"),
        _ => true,
    }
}

/// Attributes that are namespace declarations are not attribute nodes.
pub(crate) fn is_namespace_decl(name: &str) -> bool {
    name == "xmlns" || name.starts_with("xmlns:")
}

/// The XPath string-value of a node.
pub fn string_value(doc: &Document, node: XNode) -> String {
    match node {
        XNode::Attr(id, i) => doc
            .element(id)
            .and_then(|e| e.attrs.get(i))
            .map(|a| a.value.clone())
            .unwrap_or_default(),
        XNode::Node(id) => match doc.kind(id) {
            NodeKind::Document | NodeKind::Element(_) => {
                let mut s = String::new();
                for n in doc.descendants(id) {
                    if let Some(t) = doc.text(n) {
                        s.push_str(t);
                    }
                }
                s
            }
            NodeKind::Text(t) | NodeKind::CData(t) | NodeKind::Comment(t) => t.clone(),
            NodeKind::Pi { data, .. } => data.clone(),
            NodeKind::Doctype(_) | NodeKind::EntityRef(_) => String::new(),
        },
    }
}

fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

/// XPath string-to-number conversion: optional minus, digits with optional fraction.
pub(crate) fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_xml_space);
    let body = t.strip_prefix('-').unwrap_or(t);
    let mut digits = 0;
    let mut dots = 0;
    for c in body.chars() {
        match c {
            '0'..='9' => digits += 1,
            '.' => dots += 1,
            _ => return f64::NAN,
        }
    }
    if digits == 0 || dots > 1 {
        return f64::NAN;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// XPath number-to-string conversion (never uses exponent notation).
pub(crate) fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        "NaN".to_string()
    } else if n.is_infinite() {
        if n > 0.0 {
            "Infinity".to_string()
        } else {
            "-Infinity".to_string()
        }
    } else if n == 0.0 {
        "0".to_string()
    } else {
        format!("{n}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_formatting() {
        assert_eq!(number_to_string(3.0), "3");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(0.5), "0.5");
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(number_to_string(1e21), "1000000000000000000000");
        assert_eq!(number_to_string(0.00001), "0.00001");
    }

    #[test]
    fn number_parsing() {
        assert_eq!(string_to_number("  12 "), 12.0);
        assert_eq!(string_to_number("-1.5"), -1.5);
        assert_eq!(string_to_number(".5"), 0.5);
        assert_eq!(string_to_number("5."), 5.0);
        assert!(string_to_number("1e3").is_nan());
        assert!(string_to_number("+1").is_nan());
        assert!(string_to_number("").is_nan());
        assert!(string_to_number("-").is_nan());
        assert!(string_to_number("1.2.3").is_nan());
    }
}
