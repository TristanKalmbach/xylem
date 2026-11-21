//! A pure-Rust XPath 1.0 engine over the [`xylem_dom`] document model.
//!
//! ```
//! use xylem_dom::Document;
//! use xylem_xpath::{select, XNode};
//!
//! let doc = Document::from_str("<doc><para id='x'>hi</para></doc>").unwrap();
//! let hits = select(&doc, doc.root(), "descendant::para[@id='x']").unwrap();
//! assert_eq!(hits.len(), 1);
//! assert!(matches!(hits[0], XNode::Node(_)));
//! ```
//!
//! Attributes are not DOM nodes, so they are addressed as [`XNode::Attr`].
//! Node-sets are always returned in document order without duplicates.
//! Namespace nodes are not supported; the `namespace` axis is empty.

mod ast;
mod context;
mod error;
mod eval;
mod lexer;
mod parser;
mod value;

use xylem_dom::{Document, NodeId};

pub use context::{Context, CustomFunction, FunctionContext};
pub use error::XPathError;
pub use value::{Value, XNode, string_value};

/// A compiled XPath expression.
#[derive(Debug, Clone)]
pub struct XPath {
    source: String,
    expr: ast::Expr,
}

/// Compiles an expression, reporting syntax errors with their position.
pub fn compile(expr: &str) -> Result<XPath, XPathError> {
    Ok(XPath {
        source: expr.to_string(),
        expr: parser::parse(expr)?,
    })
}

impl XPath {
    /// The text this expression was compiled from.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Evaluates with an empty [`Context`].
    pub fn evaluate(&self, doc: &Document, context: XNode) -> Result<Value, XPathError> {
        self.evaluate_with(doc, context, &Context::default())
    }

    /// Evaluates with variables, namespace prefixes and custom functions.
    pub fn evaluate_with(
        &self,
        doc: &Document,
        context: XNode,
        ctx: &Context,
    ) -> Result<Value, XPathError> {
        eval::evaluate(&self.expr, doc, context, ctx)
    }

    /// Evaluates and requires a node-set result.
    pub fn select(&self, doc: &Document, context: XNode) -> Result<Vec<XNode>, XPathError> {
        self.select_with(doc, context, &Context::default())
    }

    /// Like [`XPath::select`] with an explicit [`Context`].
    pub fn select_with(
        &self,
        doc: &Document,
        context: XNode,
        ctx: &Context,
    ) -> Result<Vec<XNode>, XPathError> {
        match self.evaluate_with(doc, context, ctx)? {
            Value::NodeSet(nodes) => Ok(nodes),
            other => Err(XPathError::eval(format!(
                "expression '{}' yields a {}, not a node-set",
                self.source,
                other.type_name()
            ))),
        }
    }
}

/// Compiles and evaluates `expr` against `context`.
pub fn evaluate(doc: &Document, context: NodeId, expr: &str) -> Result<Value, XPathError> {
    compile(expr)?.evaluate(doc, XNode::Node(context))
}

/// Compiles `expr` and returns the selected nodes in document order.
pub fn select(doc: &Document, context: NodeId, expr: &str) -> Result<Vec<XNode>, XPathError> {
    compile(expr)?.select(doc, XNode::Node(context))
}

#[cfg(test)]
mod tests;
