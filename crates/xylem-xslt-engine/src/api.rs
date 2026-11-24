//! Evaluating XPath expressions on their own, outside a stylesheet (for
//! tools and conformance tests).

use crate::compile::Stylesheet;
use crate::exec::Engine;
use crate::tree::{self, Kind, QName};
use crate::value::{Item, Seq};
use crate::xpath;
use std::collections::HashMap;
use std::path::Path;

/// A value an expression returned.
#[derive(Clone, Debug, PartialEq)]
pub enum ResultItem {
    /// A node: its kind (`element`, `attribute`, `text`, `document`, ...),
    /// its XML and its string value.
    Node {
        kind: &'static str,
        xml: String,
        string: String,
    },
    /// An atomic value: its type (`xs:integer`, ...) and its string form.
    Atomic {
        type_name: &'static str,
        value: String,
    },
}

/// Parses XML text (a document or a fragment) and writes it back the
/// engine's way, so two results can be compared regardless of quoting,
/// empty-element style or where namespaces are declared.
pub fn canonical_xml(text: &str) -> Result<String, String> {
    let mut t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<?xml") {
        t = t.find("?>").map(|i| &t[i + 2..]).unwrap_or(t);
    }
    let wrapped = format!("<w>{t}</w>");
    let doc = xylem_dom::Document::from_str(&wrapped).map_err(|e| e.to_string())?;
    struct Keep;
    impl tree::StripSpace for Keep {
        fn strip(&self, _: &QName) -> bool {
            false
        }
    }
    let tr = tree::from_dom(&doc, "", &Keep)?;
    let w = tr.node(0).children[0];
    Ok(tr
        .node(w)
        .children
        .iter()
        .map(|&c| crate::serialize::node_xml(&tr, c))
        .collect())
}

/// The namespaces every standalone expression may use.
pub const DEFAULT_NAMESPACES: &[(&str, &str)] = &[
    ("xs", xpath::XS_NS),
    ("fn", xpath::FN_NS),
    ("xsi", "http://www.w3.org/2001/XMLSchema-instance"),
    ("xml", tree::XML_NS),
    ("local", "http://www.w3.org/2005/xquery-local-functions"),
    ("err", "http://www.w3.org/2005/xqt-errors"),
];

/// An evaluation session over one context document: expressions evaluated
/// in the same session can pass their results to each other by name.
pub struct Session<'s> {
    engine: Engine<'s>,
    has_context: bool,
    ns: Vec<(String, String)>,
    results: HashMap<String, Seq>,
}

/// Runs `f` with a session over `context` (XML bytes; `None` for no
/// context item). Relative URIs (`doc()`) resolve against `base`.
pub fn with_session<T>(
    context: Option<&[u8]>,
    base: &Path,
    namespaces: &[(String, String)],
    f: impl FnOnce(&mut Session) -> T,
) -> Result<T, String> {
    let ss = Stylesheet {
        version: "2.0".into(),
        base: base.to_path_buf(),
        ..Default::default()
    };
    let source = match context {
        Some(bytes) => {
            let doc =
                xylem_dom::Document::from_bytes(bytes).map_err(|e| format!("context: {e}"))?;
            tree::from_dom(&doc, &base.to_string_lossy(), &ss)?
        }
        None => tree::Tree::new_document(),
    };
    let mut ns: Vec<(String, String)> = DEFAULT_NAMESPACES
        .iter()
        .map(|(p, u)| (p.to_string(), u.to_string()))
        .collect();
    ns.extend(namespaces.iter().cloned());
    let engine = Engine::new(&ss, source, HashMap::new(), false);
    let mut s = Session {
        engine,
        has_context: context.is_some(),
        ns,
        results: HashMap::new(),
    };
    Ok(f(&mut s))
}

impl Session<'_> {
    /// Evaluates `expr`; earlier results kept with [`Session::keep`] are
    /// visible as variables (`$result`).
    pub fn eval(&mut self, expr: &str) -> Result<Vec<ResultItem>, String> {
        let seq = self.eval_seq(expr)?;
        Ok(self.describe(&seq))
    }

    /// Evaluates `expr` and keeps its value under `name` for later
    /// expressions.
    pub fn keep(&mut self, name: &str, expr: &str) -> Result<Vec<ResultItem>, String> {
        let seq = self.eval_seq(expr)?;
        let d = self.describe(&seq);
        self.results.insert(name.to_string(), seq);
        Ok(d)
    }

    fn eval_seq(&self, expr: &str) -> Result<Seq, String> {
        let ns = &self.ns;
        let resolver = |p: &str| -> Option<String> {
            if p.is_empty() {
                return None;
            }
            ns.iter()
                .rev()
                .find(|(q, _)| q == p)
                .map(|(_, u)| u.clone())
        };
        let e = xpath::parse(expr, &resolver)?;
        let vars = self
            .results
            .iter()
            .map(|(n, v)| (QName::local(n), v.clone()))
            .collect();
        let d = self.engine.standalone_dyn(self.has_context, vars);
        crate::value::with_compat(false, || self.engine.eval(&e, &d))
    }

    fn describe(&self, seq: &Seq) -> Vec<ResultItem> {
        seq.iter()
            .map(|it| match it {
                Item::Atomic(a) => ResultItem::Atomic {
                    type_name: a.type_name(),
                    value: a.to_string(),
                },
                Item::Node(n) => {
                    let t = self.engine.tree(n.tree);
                    let kind = match t.node(n.idx).kind {
                        Kind::Document => "document",
                        Kind::Element => "element",
                        Kind::Attribute => "attribute",
                        Kind::Text => "text",
                        Kind::Comment => "comment",
                        Kind::Pi => "processing-instruction",
                    };
                    ResultItem::Node {
                        kind,
                        xml: crate::serialize::node_xml(&t, n.idx),
                        string: t.string_value(n.idx),
                    }
                }
            })
            .collect()
    }
}
