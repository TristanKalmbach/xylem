//! Grammar engine for the Xylem XML editor.
//!
//! `xylem-schema` compiles W3C XML Schema (a practical XSD 1.0 subset) and
//! XML DTDs into a single internal [`Grammar`], then answers the questions an
//! editor asks: *is this document valid*, *what may be inserted here*, *may
//! this range be wrapped in that element*, *what is a minimal valid instance
//! of this element*.
//!
//! # Loading
//!
//! * [`Grammar::from_xsd`] follows `xs:include`, `xs:import` and
//!   `xs:redefine` relative to each schema file (and through a [`Catalog`]
//!   with [`Grammar::from_xsd_with_catalog`]).
//! * [`Grammar::from_dtd`] / [`Grammar::from_dtd_text`] expand internal and
//!   external parameter entities and `INCLUDE`/`IGNORE` conditional sections.
//! * [`Grammar::for_document`] picks the grammar a document asks for:
//!   `xsi:noNamespaceSchemaLocation`, `xsi:schemaLocation`, or the DOCTYPE
//!   public/system identifiers, and merges the DOCTYPE internal subset.
//!
//! # Content models
//!
//! Every element type's content model is compiled once into a Glushkov
//! position automaton (or an interleave matcher for a top-level `xs:all`).
//! Matching simulates sets of positions, so ambiguous (non-deterministic)
//! models are handled correctly. Bounded occurrence ranges are unfolded up to
//! 64 copies; larger bounds are approximated as unbounded.
//!
//! # Names and namespaces
//!
//! XSD grammars are namespace aware: elements and attributes are matched by
//! expanded name (namespace URI + local name), resolving instance prefixes
//! through in-scope `xmlns` declarations. DTD grammars match names literally.
//! Attributes in the XML Schema instance namespace (`xsi:*`), namespace
//! declarations, and undeclared `xml:*` attributes are always accepted.
//!
//! # Supported XSD features
//!
//! Global and local element declarations (context-sensitive), `ref`,
//! named and anonymous complex and simple types, `sequence`, `choice`, `all`,
//! `group`, `attributeGroup`, `minOccurs`/`maxOccurs`, `complexContent` and
//! `simpleContent` with `extension` (content appended) and `restriction`
//! (content replaced), `mixed`, `abstract`, `substitutionGroup`, `xs:any` and
//! `xs:anyAttribute` (namespace constraints; `processContents` strict, lax
//! and skip), `elementFormDefault`/`attributeFormDefault`/`form`,
//! chameleon includes, `xs:list`, `xs:union`, all built-in datatypes, and the
//! facets `enumeration`, `pattern`, `length`, `minLength`, `maxLength`,
//! `minInclusive`, `maxInclusive`, `minExclusive`, `maxExclusive`,
//! `totalDigits`, `fractionDigits` and `whiteSpace`. Element and attribute
//! documentation comes from `xs:annotation/xs:documentation`.
//!
//! # Unsupported XSD features
//!
//! * Identity constraints: `xs:key`, `xs:keyref`, `xs:unique` (ignored).
//! * XSD 1.1: assertions, `xs:assert`, conditional type alternatives,
//!   open content, `xs:override`.
//! * `xsi:type` and `xsi:nil` in instances (accepted, not interpreted).
//! * `block`, `final`, `fixed` on facets, and derivation-validity checks
//!   (restrictions are trusted, not verified against their base).
//! * Element `default`/`fixed` values for text content are not enforced.
//! * Date/time ordering facets compare lexically (timezones ignored);
//!   `duration` bounds are not checked.
//! * Unicode block escapes outside a common subset (`\p{IsBasicLatin}`,
//!   `\p{IsLatin-1Supplement}`, ...) make the pattern unchecked and are
//!   reported in [`Grammar::warnings`].
//! * Remote (`http:`) schema locations are not fetched; map them to local
//!   files with a [`Catalog`].
//!
//! # Approximations
//!
//! * `xs:all` is matched as an interleave only at the top of a content model;
//!   an `all` nested in another group, or extended by a derived type, is
//!   matched as `(a | b | ...)*`.
//! * Occurrence bounds above 64 are treated as unbounded (and `minOccurs`
//!   above 64 is clamped to 64).
//! * DTD `ANY` content, and `xs:anyType`, validate known children laxly.
//! * Elements are matched by expanded name only; `xs:any` namespace
//!   constraints are honoured, but `block`/`final` substitution rules are not.

mod bitset;
mod catalog;
mod datatype;
mod dtd;
mod edit;
mod error;
mod model;
mod resolve;
mod validate;
mod xsd;
mod xsdregex;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use xylem_dom::{Document, Element, NodeId};

pub use catalog::Catalog;
pub use datatype::{Builtin, Facets, PatternSet, SimpleType, TypeId, Variety, WhiteSpace};
pub use edit::{Insertable, Skeleton};
pub use error::{Result, SchemaError};
pub use model::ProcessContents;
pub use validate::{ErrorKind, Severity, ValidationError};

use datatype::TypeTable;
use model::{MState, Model, Term, TermInfo, Wildcard, WildcardId};

/// The XML Schema namespace.
pub const XSD_NS: &str = "http://www.w3.org/2001/XMLSchema";
/// The XML Schema instance namespace (`xsi:`).
pub const XSI_NS: &str = "http://www.w3.org/2001/XMLSchema-instance";
/// The namespace bound to the `xml:` prefix.
pub const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Identifies an element declaration in a [`Grammar`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeclId(pub(crate) u32);

/// Interned expanded element name.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct Sym(pub(crate) u32);

/// What an element may contain.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ContentType {
    /// Neither text nor elements.
    Empty,
    /// Text only (DTD `(#PCDATA)` or an XSD simple type / simple content).
    TextOnly,
    /// Child elements only; whitespace is ignored.
    ElementOnly,
    /// Text interleaved with the child elements of the content model.
    Mixed,
    /// Anything (`ANY` / `xs:anyType`).
    Any,
}

/// The source language a grammar was compiled from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GrammarKind {
    Xsd,
    Dtd,
}

/// The classification of an attribute's type, in DTD terms.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AttrKind {
    Cdata,
    Id,
    IdRef,
    IdRefs,
    Entity,
    Entities,
    NmToken,
    NmTokens,
    Notation,
    /// A closed list of values (DTD enumeration or XSD `xs:enumeration`).
    Enumeration,
    /// Any other XSD simple type (numbers, dates, patterns, ...).
    Typed,
}

/// An attribute declaration.
#[derive(Clone, Debug)]
pub struct AttrDecl {
    /// Qualified name for display and insertion (`id`, `xlink:href`).
    pub name: String,
    pub local_name: String,
    /// Namespace URI for qualified XSD attributes.
    pub namespace: Option<String>,
    pub kind: AttrKind,
    /// The simple type that values are checked against.
    pub datatype: TypeId,
    /// Display name of the type (`CDATA`, `xs:token`, `colorType`, ...).
    pub type_name: String,
    pub required: bool,
    pub default: Option<String>,
    pub fixed: Option<String>,
    /// Allowed values when the type is an enumeration (or NOTATION list).
    pub enum_values: Vec<String>,
    pub documentation: Option<String>,
}

impl AttrDecl {
    /// The value the editor should insert for a new instance of the attribute.
    pub fn initial_value(&self) -> String {
        self.fixed
            .clone()
            .or_else(|| self.default.clone())
            .or_else(|| self.enum_values.first().cloned())
            .unwrap_or_default()
    }

    /// Allowed values for pick lists; empty when the type is not enumerated.
    pub fn enumeration(&self) -> Vec<String> {
        self.enum_values.clone()
    }
}

/// A general entity declared in a DTD.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityDecl {
    pub name: String,
    pub kind: EntityKind,
}

/// The three kinds of general entities.
#[derive(Clone, Debug, PartialEq)]
pub enum EntityKind {
    /// `<!ENTITY name "replacement text">`
    Internal(String),
    /// `<!ENTITY name SYSTEM "file.xml">`: an external parsed entity.
    External {
        public_id: Option<String>,
        system_id: String,
        resolved: Option<PathBuf>,
    },
    /// `<!ENTITY name SYSTEM "file.cgm" NDATA cgm>`: an unparsed entity.
    Unparsed {
        public_id: Option<String>,
        system_id: String,
        notation: String,
        resolved: Option<PathBuf>,
    },
}

/// A `<!NOTATION>` declaration.
#[derive(Clone, Debug, PartialEq)]
pub struct NotationDecl {
    pub name: String,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
}

/// An element declaration.
#[derive(Clone, Debug)]
pub struct ElementDecl {
    pub(crate) sym: Sym,
    pub(crate) ty: usize,
    pub(crate) global: bool,
    pub(crate) is_abstract: bool,
    /// False for names that a DTD content model references but never declares.
    pub(crate) declared: bool,
    pub(crate) documentation: Option<String>,
}

impl ElementDecl {
    /// Whether this is a top-level (global) declaration.
    pub fn is_global(&self) -> bool {
        self.global
    }

    pub fn is_abstract(&self) -> bool {
        self.is_abstract
    }

    pub fn is_declared(&self) -> bool {
        self.declared
    }
}

/// Compiled type information shared by every declaration of the same type.
#[derive(Debug, Clone)]
pub(crate) struct ElemType {
    pub(crate) content: ContentType,
    pub(crate) model: Model,
    pub(crate) attrs: Vec<AttrDecl>,
    pub(crate) any_attribute: bool,
    pub(crate) simple: Option<TypeId>,
    pub(crate) documentation: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct QName {
    pub(crate) ns: Option<String>,
    pub(crate) local: String,
    pub(crate) display: String,
}

/// Interning table for expanded element names.
#[derive(Debug, Default)]
pub(crate) struct Names {
    syms: Vec<QName>,
    by_local: HashMap<String, Vec<Sym>>,
}

impl Names {
    pub(crate) fn intern(&mut self, ns: Option<&str>, local: &str, display: &str) -> Sym {
        if let Some(s) = self.lookup(ns, local) {
            return s;
        }
        let sym = Sym(self.syms.len() as u32);
        self.syms.push(QName {
            ns: ns.map(str::to_string),
            local: local.to_string(),
            display: display.to_string(),
        });
        self.by_local
            .entry(local.to_string())
            .or_default()
            .push(sym);
        sym
    }

    pub(crate) fn lookup(&self, ns: Option<&str>, local: &str) -> Option<Sym> {
        self.by_local
            .get(local)?
            .iter()
            .copied()
            .find(|s| self.syms[s.0 as usize].ns.as_deref() == ns)
    }

    pub(crate) fn get(&self, s: Sym) -> &QName {
        &self.syms[s.0 as usize]
    }
}

/// In-scope namespace bindings while walking an instance document.
#[derive(Default)]
pub(crate) struct NsScope<'d> {
    bindings: Vec<(&'d str, &'d str)>,
}

impl<'d> NsScope<'d> {
    /// Builds the scope in effect at `node` (including its own declarations).
    pub(crate) fn at(doc: &'d Document, node: NodeId) -> Self {
        let mut chain: Vec<NodeId> = doc.ancestors(node).collect();
        chain.reverse();
        chain.push(node);
        let mut scope = NsScope::default();
        for n in chain {
            if let Some(e) = doc.element(n) {
                scope.push(e);
            }
        }
        scope
    }

    /// Adds the declarations of `el`; returns a mark for [`NsScope::pop`].
    pub(crate) fn push(&mut self, el: &'d Element) -> usize {
        let mark = self.bindings.len();
        for a in &el.attrs {
            if a.name == "xmlns" {
                self.bindings.push(("", &a.value));
            } else if let Some(p) = a.name.strip_prefix("xmlns:") {
                self.bindings.push((p, &a.value));
            }
        }
        mark
    }

    pub(crate) fn pop(&mut self, mark: usize) {
        self.bindings.truncate(mark);
    }

    pub(crate) fn resolve(&self, prefix: &str) -> Option<&'d str> {
        match prefix {
            "xml" => Some(XML_NS),
            _ => self
                .bindings
                .iter()
                .rev()
                .find(|b| b.0 == prefix)
                .map(|b| b.1)
                .filter(|u| !u.is_empty()),
        }
    }

    /// A prefix currently bound to `ns`, if any (`Some("")` for the default namespace).
    pub(crate) fn prefix_for(&self, ns: &str) -> Option<&'d str> {
        self.bindings
            .iter()
            .rev()
            .find(|b| b.1 == ns)
            .map(|b| b.0)
            .filter(|p| self.resolve(p) == Some(ns))
    }
}

/// A compiled grammar.
#[derive(Debug)]
pub struct Grammar {
    pub(crate) kind: GrammarKind,
    pub(crate) names: Names,
    pub(crate) decls: Vec<ElementDecl>,
    pub(crate) types: Vec<ElemType>,
    pub(crate) globals: HashMap<Sym, DeclId>,
    pub(crate) wildcards: Vec<Wildcard>,
    pub(crate) simple: TypeTable,
    pub(crate) entities: Vec<EntityDecl>,
    pub(crate) notations: Vec<NotationDecl>,
    /// Root element required by a DOCTYPE declaration.
    pub(crate) doctype_root: Option<String>,
    pub(crate) warnings: Vec<String>,
    pub(crate) namespace_aware: bool,
}

impl TermInfo for Grammar {
    fn term_sym(&self, decl: DeclId) -> Sym {
        self.decls[decl.0 as usize].sym
    }
    fn wildcard(&self, id: WildcardId) -> &Wildcard {
        &self.wildcards[id.0 as usize]
    }
}

impl Grammar {
    pub(crate) fn new(kind: GrammarKind) -> Self {
        Grammar {
            kind,
            names: Names::default(),
            decls: Vec::new(),
            types: Vec::new(),
            globals: HashMap::new(),
            wildcards: Vec::new(),
            simple: TypeTable::new(),
            entities: Vec::new(),
            notations: Vec::new(),
            doctype_root: None,
            warnings: Vec::new(),
            namespace_aware: kind == GrammarKind::Xsd,
        }
    }

    // ------------------------------------------------------------- loading

    /// Loads an XML Schema and everything it includes, imports or redefines.
    pub fn from_xsd(path: impl AsRef<Path>) -> Result<Grammar> {
        Self::from_xsd_with_catalog(path, &Catalog::new())
    }

    /// Like [`Grammar::from_xsd`], resolving locations through `catalog`.
    pub fn from_xsd_with_catalog(path: impl AsRef<Path>, catalog: &Catalog) -> Result<Grammar> {
        xsd::load(&[path.as_ref().to_path_buf()], catalog)
    }

    /// Loads several schema documents (e.g. from `xsi:schemaLocation`) into one grammar.
    pub fn from_xsd_files(paths: &[PathBuf], catalog: &Catalog) -> Result<Grammar> {
        xsd::load(paths, catalog)
    }

    /// Loads a DTD file, expanding its parameter entities.
    pub fn from_dtd(path: impl AsRef<Path>) -> Result<Grammar> {
        Self::from_dtd_with_catalog(path, &Catalog::new())
    }

    /// Like [`Grammar::from_dtd`], resolving public identifiers through `catalog`.
    pub fn from_dtd_with_catalog(path: impl AsRef<Path>, catalog: &Catalog) -> Result<Grammar> {
        dtd::load_file(path.as_ref(), catalog)
    }

    /// Compiles DTD text; relative external entities resolve against `base_dir`.
    pub fn from_dtd_text(text: &str, base_dir: impl AsRef<Path>) -> Result<Grammar> {
        Self::from_dtd_text_with_catalog(text, base_dir, &Catalog::new())
    }

    /// Like [`Grammar::from_dtd_text`], resolving public identifiers through `catalog`.
    pub fn from_dtd_text_with_catalog(
        text: &str,
        base_dir: impl AsRef<Path>,
        catalog: &Catalog,
    ) -> Result<Grammar> {
        dtd::load_text(text, base_dir.as_ref(), catalog)
    }

    /// Loads the grammar a document refers to, or `None` if it names none.
    ///
    /// Checks, in order, `xsi:noNamespaceSchemaLocation`, `xsi:schemaLocation`
    /// and the DOCTYPE (internal subset, public and system identifiers).
    /// Relative locations resolve against `doc_path`'s directory; if a
    /// location does not exist there, a file of the same name next to the
    /// document is used. The internal subset of a DOCTYPE is merged into XSD
    /// grammars too (entities, notations, extra attributes).
    pub fn for_document(
        doc: &Document,
        doc_path: Option<&Path>,
        catalog: &Catalog,
    ) -> Option<Result<Grammar>> {
        resolve::for_document(doc, doc_path, catalog)
    }

    // ------------------------------------------------------------- inspection

    pub fn kind(&self) -> GrammarKind {
        self.kind
    }

    /// Non-fatal problems found while loading (unresolved imports, unsupported
    /// constructs, invalid patterns, ...).
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub fn decl(&self, id: DeclId) -> &ElementDecl {
        &self.decls[id.0 as usize]
    }

    pub(crate) fn ty(&self, id: DeclId) -> &ElemType {
        &self.types[self.decl(id).ty]
    }

    /// Every element declaration, global and local.
    pub fn decl_ids(&self) -> impl Iterator<Item = DeclId> + '_ {
        (0..self.decls.len() as u32).map(DeclId)
    }

    /// The qualified display name of a declaration (`para`, `dc:title`).
    pub fn decl_name(&self, id: DeclId) -> &str {
        &self.names.get(self.decl(id).sym).display
    }

    /// The local name and namespace URI of a declaration.
    pub fn decl_expanded_name(&self, id: DeclId) -> (Option<&str>, &str) {
        let q = self.names.get(self.decl(id).sym);
        (q.ns.as_deref(), &q.local)
    }

    /// The global declaration for a name (a qualified name for DTDs; a local
    /// name, or `{uri}local`, for XSDs).
    pub fn global_decl(&self, name: &str) -> Option<DeclId> {
        let (ns, local) = match name.strip_prefix('{').and_then(|r| r.split_once('}')) {
            Some((ns, local)) => (Some(ns), local),
            None => (None, name),
        };
        let sym = self.names.lookup(ns, local).or_else(|| {
            // Fall back to a unique global with this display name.
            self.globals
                .keys()
                .copied()
                .find(|s| self.names.get(*s).display == name)
        })?;
        self.globals.get(&sym).copied()
    }

    pub(crate) fn global_for_sym(&self, sym: Option<Sym>) -> Option<DeclId> {
        sym.and_then(|s| self.globals.get(&s).copied())
    }

    /// Distinct display names of all declared elements, sorted.
    pub fn all_element_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .decls
            .iter()
            .filter(|d| d.declared)
            .map(|d| self.names.get(d.sym).display.clone())
            .collect();
        v.sort();
        v.dedup();
        v
    }

    /// Documentation of the first declaration (global preferred) with this name.
    pub fn documentation_for_name(&self, name: &str) -> Option<&str> {
        if let Some(d) = self.global_decl(name)
            && let Some(doc) = self.documentation(d)
        {
            return Some(doc);
        }
        self.decl_ids()
            .filter(|&d| self.decl_name(d) == name)
            .find_map(|d| self.documentation(d))
    }

    pub fn content_type(&self, decl: DeclId) -> ContentType {
        self.ty(decl).content
    }

    pub fn attributes(&self, decl: DeclId) -> &[AttrDecl] {
        &self.ty(decl).attrs
    }

    /// Whether undeclared attributes are allowed (`xs:anyAttribute`).
    pub fn allows_any_attribute(&self, decl: DeclId) -> bool {
        self.ty(decl).any_attribute
    }

    /// The simple type of an element with text-only content.
    pub fn text_type(&self, decl: DeclId) -> Option<TypeId> {
        self.ty(decl).simple
    }

    /// The `xs:documentation` of the declaration (or of its type).
    pub fn documentation(&self, decl: DeclId) -> Option<&str> {
        self.decl(decl)
            .documentation
            .as_deref()
            .or(self.ty(decl).documentation.as_deref())
    }

    pub fn simple_type(&self, id: TypeId) -> &SimpleType {
        self.simple.get(id)
    }

    /// Checks a value against a simple type; the error is a short reason.
    pub fn check_value(&self, ty: TypeId, value: &str) -> std::result::Result<(), String> {
        self.simple.validate(ty, value)
    }

    pub fn entities(&self) -> &[EntityDecl] {
        &self.entities
    }

    pub fn entity(&self, name: &str) -> Option<&EntityDecl> {
        self.entities.iter().find(|e| e.name == name)
    }

    pub fn notations(&self) -> &[NotationDecl] {
        &self.notations
    }

    /// Names of the elements that may be the document element.
    ///
    /// For a DOCTYPE-based grammar this is the DOCTYPE name. Otherwise XSD
    /// grammars offer every non-abstract global element, and DTD grammars the
    /// declared elements that no content model references (or all of them if
    /// every element is referenced).
    pub fn root_candidates(&self) -> Vec<String> {
        if let Some(root) = &self.doctype_root {
            return vec![root.clone()];
        }
        let mut referenced = std::collections::HashSet::new();
        if self.kind == GrammarKind::Dtd {
            for t in &self.types {
                for term in t.model.all_terms() {
                    if let Term::Elem(d) = term {
                        referenced.insert(d);
                    }
                }
            }
        }
        let candidates = |skip_referenced: bool| -> Vec<String> {
            let mut v: Vec<String> = self
                .globals
                .values()
                .filter(|&&d| {
                    let decl = self.decl(d);
                    decl.declared
                        && !decl.is_abstract
                        && !(skip_referenced && referenced.contains(&d))
                })
                .map(|&d| self.decl_name(d).to_string())
                .collect();
            v.sort();
            v
        };
        let roots = candidates(true);
        if roots.is_empty() {
            candidates(false)
        } else {
            roots
        }
    }

    // ------------------------------------------------------------- name resolution

    /// Resolves an instance element name in `scope`.
    pub(crate) fn inst_name<'a>(&self, qname: &'a str, scope: &NsScope<'a>) -> model::InstName<'a> {
        if !self.namespace_aware {
            return model::InstName {
                sym: self.names.lookup(None, qname),
                ns: None,
            };
        }
        let (ns, local) = match qname.split_once(':') {
            Some((p, l)) => (scope.resolve(p), l),
            None => (scope.resolve(""), qname),
        };
        model::InstName {
            sym: self.names.lookup(ns, local),
            ns,
        }
    }

    /// The declaration of the document element.
    pub(crate) fn root_decl(&self, doc: &Document, root: NodeId) -> Option<DeclId> {
        let scope = NsScope::at(doc, root);
        let name = self.inst_name(doc.name(root)?, &scope);
        self.global_for_sym(name.sym)
    }

    /// Resolves the declaration of each element child of `parent` (which has
    /// declaration `decl`), using the content model so local declarations are
    /// found. `None` entries are unknown or skipped elements.
    pub(crate) fn child_decls(
        &self,
        doc: &Document,
        parent: NodeId,
        decl: DeclId,
    ) -> Vec<(NodeId, Option<DeclId>)> {
        let scope = NsScope::at(doc, parent);
        let mut scope = scope;
        let ty = self.ty(decl);
        let mut out = Vec::new();
        let mut state = ty.model.start();
        for c in doc.element_children(parent) {
            let el = doc.element(c).expect("element child");
            let mark = scope.push(el);
            let name = self.inst_name(&el.name, &scope);
            let d = match ty.content {
                ContentType::Any => self.global_for_sym(name.sym),
                _ => {
                    let matched = ty
                        .model
                        .step(&state, name, self)
                        .or_else(|| ty.model.resync(&state, name, self));
                    match matched {
                        Some((next, term)) => {
                            state = next;
                            self.term_decl(term, name)
                        }
                        None => self.global_for_sym(name.sym),
                    }
                }
            };
            scope.pop(mark);
            out.push((c, d));
        }
        out
    }

    /// The declaration an instance element gets when matched by `term`.
    pub(crate) fn term_decl(&self, term: Term, name: model::InstName<'_>) -> Option<DeclId> {
        match term {
            Term::Elem(d) => Some(d),
            Term::Any(w) => match self.wildcards[w.0 as usize].process {
                ProcessContents::Skip => None,
                _ => self.global_for_sym(name.sym),
            },
        }
    }

    /// The context-sensitive declaration of an instance element, resolved
    /// from the document element down through its ancestors.
    pub fn element_decl_for(&self, doc: &Document, node: NodeId) -> Option<DeclId> {
        if !doc.is_element(node) {
            return None;
        }
        let mut chain: Vec<NodeId> = doc.ancestors(node).filter(|&a| doc.is_element(a)).collect();
        chain.reverse();
        chain.push(node);
        let mut decl = self.root_decl(doc, chain[0])?;
        for pair in chain.windows(2) {
            let (parent, child) = (pair[0], pair[1]);
            decl = self
                .child_decls(doc, parent, decl)
                .into_iter()
                .find(|(c, _)| *c == child)?
                .1?;
        }
        Some(decl)
    }

    /// Runs the content model of `decl` over a sequence of names, with error
    /// recovery. Used by validation and by the editing queries.
    pub(crate) fn run_model<'a>(
        &self,
        decl: DeclId,
        names: impl IntoIterator<Item = model::InstName<'a>>,
    ) -> ModelRun {
        let model = &self.ty(decl).model;
        let mut state = model.start();
        let mut errors = 0;
        for name in names {
            match model.step(&state, name, self) {
                Some((s, _)) => state = s,
                None => {
                    errors += 1;
                    if let Some((s, _)) = model.resync(&state, name, self) {
                        state = s;
                    }
                }
            }
        }
        ModelRun {
            errors,
            complete: model.accepts(&state),
            state,
        }
    }
}

/// The outcome of running a content model over a child sequence.
pub(crate) struct ModelRun {
    pub(crate) errors: usize,
    pub(crate) complete: bool,
    pub(crate) state: MState,
}

impl ModelRun {
    /// Validity rank: lower is better. Errors dominate; an incomplete but
    /// error-free sequence ranks just below a valid one.
    pub(crate) fn rank(&self) -> (usize, bool) {
        (self.errors, !self.complete)
    }
}

/// Classifies an attribute type for [`AttrDecl::kind`].
pub(crate) fn attr_kind(types: &TypeTable, id: TypeId) -> AttrKind {
    let st = types.get(id);
    let list = matches!(st.variety, Variety::List(_));
    if st.facets.enumeration.is_some() && st.builtin != Builtin::Notation {
        return AttrKind::Enumeration;
    }
    match (st.builtin, list) {
        (Builtin::Id, _) => AttrKind::Id,
        (Builtin::IdRef, false) => AttrKind::IdRef,
        (Builtin::IdRef, true) => AttrKind::IdRefs,
        (Builtin::Entity, false) => AttrKind::Entity,
        (Builtin::Entity, true) => AttrKind::Entities,
        (Builtin::NmToken, false) => AttrKind::NmToken,
        (Builtin::NmToken, true) => AttrKind::NmTokens,
        (Builtin::Notation, _) => AttrKind::Notation,
        (Builtin::String | Builtin::AnySimpleType, false)
            if st.facets.patterns.is_empty() && st.facets.max_length.is_none() =>
        {
            AttrKind::Cdata
        }
        _ => AttrKind::Typed,
    }
}
