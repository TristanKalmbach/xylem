//! Document validation: one top-down pass carrying the parent's declaration
//! and the namespace scope.

use std::collections::HashMap;

use xylem_dom::{Document, Element, NodeId, NodeKind};

use crate::model::{InstName, Term};
use crate::{
    AttrKind, ContentType, DeclId, EntityKind, Grammar, GrammarKind, NsScope, ProcessContents,
    XML_NS, XSI_NS,
};

/// How serious a validation finding is.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
}

/// The kind of a validation finding, with its structured details.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The document element is not a permitted root.
    WrongRoot {
        name: String,
        expected: Vec<String>,
    },
    /// The grammar has no declaration for this element.
    UndeclaredElement {
        name: String,
    },
    /// The element is declared but not allowed at this position.
    ElementNotAllowed {
        name: String,
        expected: Vec<String>,
    },
    /// Required child elements are missing at the end of the element.
    IncompleteContent {
        expected: Vec<String>,
    },
    /// Non-whitespace text in element-only or empty content.
    TextNotAllowed,
    /// Text content does not satisfy the element's simple type.
    InvalidText {
        value: String,
        reason: String,
    },
    MissingAttribute {
        name: String,
    },
    UndeclaredAttribute {
        name: String,
    },
    InvalidAttributeValue {
        name: String,
        value: String,
        reason: String,
    },
    DuplicateId {
        id: String,
    },
    UnresolvedIdRef {
        attribute: String,
        id: String,
    },
    UndeclaredEntity {
        name: String,
    },
}

/// A validation finding attached to a node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub node: NodeId,
    pub severity: Severity,
    pub kind: ErrorKind,
    pub message: String,
}

/// Longest expected-name list spelled out in a message.
const MAX_LISTED: usize = 25;

fn list(names: &[String]) -> String {
    if names.len() > MAX_LISTED {
        format!(
            "{}, ... ({} more)",
            names[..MAX_LISTED].join(", "),
            names.len() - MAX_LISTED
        )
    } else {
        names.join(", ")
    }
}

fn is_ws(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

struct Validator<'g, 'd> {
    g: &'g Grammar,
    doc: &'d Document,
    scope: NsScope<'d>,
    errors: Vec<ValidationError>,
    ids: HashMap<String, NodeId>,
    idrefs: Vec<(NodeId, &'d str, String)>,
}

impl Grammar {
    /// Validates a whole document against the grammar.
    pub fn validate(&self, doc: &Document) -> Vec<ValidationError> {
        let mut v = Validator {
            g: self,
            doc,
            scope: NsScope::default(),
            errors: Vec::new(),
            ids: HashMap::new(),
            idrefs: Vec::new(),
        };
        v.run();
        v.errors
    }

    /// Display names for a list of allowed terms.
    pub(crate) fn term_names(&self, terms: &[Term]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for t in terms {
            let name = match t {
                Term::Elem(d) => self.decl_name(*d).to_string(),
                Term::Any(_) => "any element".to_string(),
            };
            if !out.contains(&name) {
                out.push(name);
            }
        }
        out
    }
}

impl<'g, 'd> Validator<'g, 'd> {
    fn error(&mut self, node: NodeId, kind: ErrorKind, message: String) {
        self.errors.push(ValidationError {
            node,
            severity: Severity::Error,
            kind,
            message,
        });
    }

    fn run(&mut self) {
        let doc = self.doc;
        let Some(root) = doc.document_element() else {
            return;
        };
        let el = doc.element(root).expect("document element");
        self.scope.push(el);
        let name = self.g.inst_name(&el.name, &self.scope);
        let decl = self.g.global_for_sym(name.sym);
        let root_ok = match &self.g.doctype_root {
            Some(r) => r == &el.name,
            None => decl.is_some_and(|d| !self.g.decl(d).is_abstract && self.g.decl(d).declared),
        };
        if !root_ok {
            let expected = self.g.root_candidates();
            let message = format!(
                "Element '{}' is not allowed as the root element; expected one of: {}",
                el.name,
                list(&expected)
            );
            self.error(
                root,
                ErrorKind::WrongRoot {
                    name: el.name.clone(),
                    expected,
                },
                message,
            );
        }
        match decl {
            Some(d) => self.element(root, el, d),
            None if root_ok => self.undeclared(root, &el.name),
            None => {}
        }
        self.check_idrefs();
    }

    fn undeclared(&mut self, node: NodeId, name: &str) {
        self.error(
            node,
            ErrorKind::UndeclaredElement {
                name: name.to_string(),
            },
            format!("Element '{name}' is not declared"),
        );
    }

    fn check_idrefs(&mut self) {
        let refs = std::mem::take(&mut self.idrefs);
        for (node, attr, id) in refs {
            if !self.ids.contains_key(&id) {
                let message =
                    format!("Attribute '{attr}' refers to ID '{id}', which does not exist");
                self.error(
                    node,
                    ErrorKind::UnresolvedIdRef {
                        attribute: attr.to_string(),
                        id,
                    },
                    message,
                );
            }
        }
    }

    /// Validates an element whose own namespace declarations are already in scope.
    fn element(&mut self, node: NodeId, el: &'d Element, decl: DeclId) {
        let g = self.g;
        let d = g.decl(decl);
        if !d.declared {
            self.undeclared(node, &el.name);
            return;
        }
        self.attributes(node, el, decl);
        let ty = g.ty(decl);
        let doc = self.doc;
        let content = ty.content;
        let mut state = ty.model.start();
        let mut text_reported = false;
        let mut text: Option<String> = (content == ContentType::TextOnly).then(String::new);
        let mut text_known = true;
        for &c in doc.children(node) {
            match doc.kind(c) {
                NodeKind::Element(child) => {
                    let mark = self.scope.push(child);
                    let name = g.inst_name(&child.name, &self.scope);
                    let child_decl = match content {
                        ContentType::Any => self.lax_child(c, child, name, ProcessContents::Lax),
                        ContentType::Empty | ContentType::TextOnly => {
                            let why = if content == ContentType::Empty {
                                format!("'{}' must be empty", el.name)
                            } else {
                                format!("'{}' may contain only text", el.name)
                            };
                            self.not_allowed(c, &child.name, name, Vec::new(), why)
                        }
                        ContentType::ElementOnly | ContentType::Mixed => match ty
                            .model
                            .step(&state, name, g)
                        {
                            Some((next, term)) => {
                                state = next;
                                match term {
                                    Term::Elem(d) => Some(d),
                                    Term::Any(w) => self.lax_child(
                                        c,
                                        child,
                                        name,
                                        g.wildcards[w.0 as usize].process,
                                    ),
                                }
                            }
                            None => {
                                let expected = g.term_names(&ty.model.allowed(&state));
                                let resync = ty.model.resync(&state, name, g);
                                let why = if expected.is_empty() {
                                    format!("no more elements are allowed in '{}'", el.name)
                                } else if expected.len() == 1 {
                                    format!("expected: {}", expected[0])
                                } else {
                                    format!("expected one of: {}", list(&expected))
                                };
                                match resync {
                                    Some((next, term)) => {
                                        state = next;
                                        self.report_not_allowed(c, &child.name, expected, why);
                                        g.term_decl(term, name)
                                    }
                                    None => self.not_allowed(c, &child.name, name, expected, why),
                                }
                            }
                        },
                    };
                    if let Some(cd) = child_decl {
                        self.element(c, child, cd);
                    }
                    self.scope.pop(mark);
                }
                NodeKind::Text(t) | NodeKind::CData(t) => {
                    if let Some(buf) = text.as_mut() {
                        buf.push_str(t);
                    } else if !text_reported
                        && matches!(content, ContentType::ElementOnly | ContentType::Empty)
                        && !is_ws(t)
                    {
                        text_reported = true;
                        let message = format!("Text is not allowed in element '{}'", el.name);
                        self.error(c, ErrorKind::TextNotAllowed, message);
                    }
                }
                NodeKind::EntityRef(name) => match g.entity(name) {
                    Some(e) => {
                        if let (Some(buf), EntityKind::Internal(v)) = (text.as_mut(), &e.kind) {
                            buf.push_str(v);
                        } else {
                            text_known = false;
                        }
                    }
                    None => {
                        text_known = false;
                        if g.kind == GrammarKind::Dtd || !g.entities.is_empty() {
                            let message = format!("Entity '&{name};' is not declared");
                            self.error(
                                c,
                                ErrorKind::UndeclaredEntity { name: name.clone() },
                                message,
                            );
                        }
                    }
                },
                _ => {}
            }
        }
        if matches!(content, ContentType::ElementOnly | ContentType::Mixed)
            && !ty.model.accepts(&state)
        {
            let expected = g.term_names(&ty.model.allowed(&state));
            let message = format!(
                "Element '{}' is incomplete; expected one of: {}",
                el.name,
                list(&expected)
            );
            self.error(node, ErrorKind::IncompleteContent { expected }, message);
        }
        if let (Some(value), Some(st), true) = (text, ty.simple, text_known)
            && let Err(reason) = g.simple.validate(st, &value)
        {
            let shown = value.trim();
            let message = format!(
                "Content '{shown}' of element '{}' is not valid ({reason})",
                el.name
            );
            self.error(
                node,
                ErrorKind::InvalidText {
                    value: shown.to_string(),
                    reason,
                },
                message,
            );
        }
    }

    fn report_not_allowed(&mut self, node: NodeId, name: &str, expected: Vec<String>, why: String) {
        let message = format!("Element '{name}' is not allowed here; {why}");
        self.error(
            node,
            ErrorKind::ElementNotAllowed {
                name: name.to_string(),
                expected,
            },
            message,
        );
    }

    /// Reports a misplaced element and returns its global declaration (to
    /// keep validating its subtree), or reports it as undeclared.
    fn not_allowed(
        &mut self,
        node: NodeId,
        qname: &str,
        name: InstName<'_>,
        expected: Vec<String>,
        why: String,
    ) -> Option<DeclId> {
        match self.g.global_for_sym(name.sym) {
            Some(d) => {
                self.report_not_allowed(node, qname, expected, why);
                Some(d)
            }
            // Declared only locally, in some other context.
            None if name.sym.is_some() => {
                self.report_not_allowed(node, qname, expected, why);
                None
            }
            None => {
                self.undeclared(node, qname);
                None
            }
        }
    }

    /// Declaration for an element matched by a wildcard or `ANY` content.
    fn lax_child(
        &mut self,
        node: NodeId,
        el: &Element,
        name: InstName<'_>,
        process: ProcessContents,
    ) -> Option<DeclId> {
        match process {
            ProcessContents::Skip => None,
            ProcessContents::Lax => self.g.global_for_sym(name.sym),
            ProcessContents::Strict => {
                let d = self.g.global_for_sym(name.sym);
                if d.is_none() {
                    self.undeclared(node, &el.name);
                }
                d
            }
        }
    }

    fn attributes(&mut self, node: NodeId, el: &'d Element, decl: DeclId) {
        let g = self.g;
        let ty = g.ty(decl);
        let decls = &ty.attrs;
        let mut seen = vec![false; decls.len()];
        for a in &el.attrs {
            let name = a.name.as_str();
            if name == "xmlns" || name.starts_with("xmlns:") {
                continue;
            }
            let split = name.split_once(':');
            let ns = split.and_then(|(p, _)| self.scope.resolve(p));
            if let Some((p, _)) = split
                && (ns == Some(XSI_NS) || (ns.is_none() && p == "xsi"))
            {
                continue;
            }
            let idx = decls.iter().position(|d| d.name == name).or_else(|| {
                let (_, local) = split?;
                let ns = ns?;
                decls
                    .iter()
                    .position(|d| d.namespace.as_deref() == Some(ns) && d.local_name == local)
            });
            let Some(i) = idx else {
                if ns == Some(XML_NS) || ty.any_attribute {
                    continue;
                }
                let message = format!(
                    "Attribute '{name}' is not declared for element '{}'",
                    el.name
                );
                self.error(
                    node,
                    ErrorKind::UndeclaredAttribute {
                        name: name.to_string(),
                    },
                    message,
                );
                continue;
            };
            seen[i] = true;
            let ad = &decls[i];
            let value = a.value.as_str();
            let invalid = |reason: String| {
                let message =
                    format!("Value '{value}' is not valid for attribute '{name}' ({reason})");
                (
                    ErrorKind::InvalidAttributeValue {
                        name: name.to_string(),
                        value: value.to_string(),
                        reason,
                    },
                    message,
                )
            };
            if let Err(reason) = g.simple.validate(ad.datatype, value) {
                let (k, m) = invalid(reason);
                self.error(node, k, m);
                continue;
            }
            if let Some(fixed) = &ad.fixed {
                let ws = g.simple.get(ad.datatype).whitespace;
                if ws.apply(value) != ws.apply(fixed) {
                    let (k, m) = invalid(format!("must be '{fixed}'"));
                    self.error(node, k, m);
                    continue;
                }
            }
            match ad.kind {
                AttrKind::Id => {
                    let id = value.trim().to_string();
                    if let std::collections::hash_map::Entry::Vacant(e) = self.ids.entry(id.clone())
                    {
                        e.insert(node);
                    } else {
                        let message = format!("Duplicate ID '{id}'");
                        self.error(node, ErrorKind::DuplicateId { id }, message);
                    }
                }
                AttrKind::IdRef | AttrKind::IdRefs => {
                    for id in value.split_ascii_whitespace() {
                        self.idrefs.push((node, &a.name, id.to_string()));
                    }
                }
                AttrKind::Entity | AttrKind::Entities => {
                    for e in value.split_ascii_whitespace() {
                        let ok = matches!(
                            g.entity(e).map(|x| &x.kind),
                            Some(EntityKind::Unparsed { .. })
                        );
                        if !ok {
                            let (k, m) =
                                invalid(format!("'{e}' is not a declared unparsed entity"));
                            self.error(node, k, m);
                        }
                    }
                }
                AttrKind::Notation if !g.notations.is_empty() => {
                    let v = value.trim();
                    if !g.notations.iter().any(|n| n.name == v) {
                        let (k, m) = invalid(format!("'{v}' is not a declared notation"));
                        self.error(node, k, m);
                    }
                }
                _ => {}
            }
        }
        for (i, ad) in decls.iter().enumerate() {
            if ad.required && !seen[i] {
                let message = format!("Required attribute '{}' missing", ad.name);
                self.error(
                    node,
                    ErrorKind::MissingAttribute {
                        name: ad.name.clone(),
                    },
                    message,
                );
            }
        }
    }
}
