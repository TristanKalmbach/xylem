//! W3C XML Schema loader.
//!
//! Loading happens in two phases:
//! 1. every schema document reachable through `include`/`import`/`redefine`
//!    is parsed and its top-level components are indexed by expanded name;
//! 2. global element declarations are compiled, pulling in the types,
//!    groups and attribute groups they need. Each named type is compiled
//!    once and shared by every declaration that uses it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use xylem_dom::{Document, NodeId, local_name};

use crate::catalog::Catalog;
use crate::datatype::{Facets, PatternSet, SimpleType, TypeId, Variety, WhiteSpace};
use crate::error::{Result, SchemaError, read_file};
use crate::model::{Model, NsConstraint, Particle, ProcessContents, Term, Wildcard, WildcardId};
use crate::resolve::resolve_location;
use crate::{
    AttrDecl, ContentType, DeclId, ElemType, ElementDecl, Grammar, GrammarKind, NotationDecl,
    NsScope, XML_NS, XSD_NS, attr_kind,
};

/// Expanded name of a schema component.
type Key = (Option<String>, String);

/// Location of a component's defining element.
#[derive(Copy, Clone, Debug)]
struct Src {
    doc: usize,
    node: NodeId,
}

struct SchemaDoc {
    doc: Document,
    path: PathBuf,
    tns: Option<String>,
    /// Included without a target namespace into one that has one.
    chameleon: bool,
    elem_qualified: bool,
    attr_qualified: bool,
}

#[derive(Default)]
struct Tables {
    elements: HashMap<Key, Src>,
    element_order: Vec<Key>,
    types: HashMap<Key, Src>,
    groups: HashMap<Key, Src>,
    attr_groups: HashMap<Key, Src>,
    attributes: HashMap<Key, Src>,
    /// Original definitions replaced by `xs:redefine`.
    redefined: HashMap<(&'static str, Key), Src>,
}

struct Loader<'c> {
    catalog: &'c Catalog,
    docs: Vec<SchemaDoc>,
    loaded: HashMap<(PathBuf, Option<String>), usize>,
    t: Tables,
    warnings: Vec<String>,
    notations: Vec<NotationDecl>,
}

pub(crate) fn load(paths: &[PathBuf], catalog: &Catalog) -> Result<Grammar> {
    let mut loader = Loader {
        catalog,
        docs: Vec::new(),
        loaded: HashMap::new(),
        t: Tables::default(),
        warnings: Vec::new(),
        notations: Vec::new(),
    };
    for p in paths {
        loader.load_doc(p, None, true)?;
    }
    loader.report_unsupported();
    Compiler::new(&loader).run()
}

/// Concatenated, trimmed text of `xs:annotation/xs:documentation` children.
fn documentation(doc: &Document, node: NodeId) -> Option<String> {
    let mut parts = Vec::new();
    for a in doc.element_children(node).filter(|&c| {
        doc.element(c)
            .is_some_and(|e| e.local_name() == "annotation")
    }) {
        for d in doc.element_children(a) {
            if doc
                .element(d)
                .is_some_and(|e| e.local_name() == "documentation")
            {
                let text = doc.text_content(d);
                let lines: Vec<&str> = text.lines().map(str::trim).collect();
                let joined = lines.join("\n");
                let trimmed = joined.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn children_named<'d>(
    doc: &'d Document,
    node: NodeId,
    names: &'d [&str],
) -> impl Iterator<Item = NodeId> + 'd {
    doc.element_children(node).filter(move |&c| {
        doc.element(c)
            .is_some_and(|e| names.contains(&e.local_name()))
    })
}

fn lname(doc: &Document, node: NodeId) -> &str {
    doc.element(node).map(|e| e.local_name()).unwrap_or("")
}

fn parse_occurs(doc: &Document, node: NodeId) -> (u32, Option<u32>) {
    let min = doc
        .attr(node, "minOccurs")
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(1);
    let max = match doc.attr(node, "maxOccurs").map(str::trim) {
        Some("unbounded") => None,
        Some(v) => Some(v.parse().unwrap_or(1)),
        None => Some(1),
    };
    (min, max)
}

impl<'c> Loader<'c> {
    fn load_doc(
        &mut self,
        path: &Path,
        chameleon_tns: Option<&str>,
        required: bool,
    ) -> Result<Option<usize>> {
        let canon = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let key = (canon, chameleon_tns.map(str::to_string));
        if let Some(&i) = self.loaded.get(&key) {
            return Ok(Some(i));
        }
        let bytes = match read_file(path) {
            Ok(b) => b,
            Err(e) if !required => {
                self.warnings.push(e.to_string());
                return Ok(None);
            }
            Err(e) => return Err(e),
        };
        let doc = Document::from_bytes(&bytes).map_err(|e| SchemaError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let root = doc
            .document_element()
            .filter(|&r| lname(&doc, r) == "schema")
            .ok_or_else(|| {
                SchemaError::Invalid(format!("'{}' is not an XML Schema", path.display()))
            })?;
        let own_tns = doc
            .attr(root, "targetNamespace")
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let chameleon = own_tns.is_none() && chameleon_tns.is_some();
        let tns = own_tns.or_else(|| chameleon_tns.map(str::to_string));
        let elem_qualified = doc.attr(root, "elementFormDefault") == Some("qualified");
        let attr_qualified = doc.attr(root, "attributeFormDefault") == Some("qualified");
        let idx = self.docs.len();
        self.loaded.insert(key, idx);
        self.docs.push(SchemaDoc {
            doc,
            path: path.to_path_buf(),
            tns,
            chameleon,
            elem_qualified,
            attr_qualified,
        });

        let base = path.parent().map(Path::to_path_buf);
        let children: Vec<NodeId> = self.docs[idx].doc.element_children(root).collect();
        for c in children {
            let d = &self.docs[idx];
            let kind = lname(&d.doc, c).to_string();
            match kind.as_str() {
                "include" | "redefine" | "import" => {
                    let Some(loc) = d.doc.attr(c, "schemaLocation").map(str::to_string) else {
                        let ns = d.doc.attr(c, "namespace").unwrap_or("");
                        if kind == "import" && ns != XML_NS {
                            self.warnings.push(format!(
                                "import of namespace '{ns}' without schemaLocation ignored"
                            ));
                        }
                        continue;
                    };
                    let tns = if kind == "import" {
                        None
                    } else {
                        self.docs[idx].tns.clone()
                    };
                    let target = match resolve_location(&loc, base.as_deref(), self.catalog) {
                        Ok(p) => p,
                        Err(e) => {
                            self.warnings
                                .push(format!("{kind} '{loc}' in '{}': {e}", path.display()));
                            continue;
                        }
                    };
                    if kind == "import"
                        && d.doc.attr(c, "namespace") == Some(XML_NS)
                        && !target.exists()
                    {
                        continue;
                    }
                    self.load_doc(&target, tns.as_deref(), false)?;
                    if kind == "redefine" {
                        self.register_redefinitions(idx, c);
                    }
                }
                "notation" => {
                    if let Some(name) = d.doc.attr(c, "name") {
                        self.notations.push(NotationDecl {
                            name: name.to_string(),
                            public_id: d.doc.attr(c, "public").map(str::to_string),
                            system_id: d.doc.attr(c, "system").map(str::to_string),
                        });
                    }
                }
                _ => self.register(idx, c, &kind),
            }
        }
        Ok(Some(idx))
    }

    fn register(&mut self, doc: usize, node: NodeId, kind: &str) {
        let d = &self.docs[doc];
        let Some(name) = d.doc.attr(node, "name") else {
            return;
        };
        let key = (d.tns.clone(), name.to_string());
        let src = Src { doc, node };
        let table = match kind {
            "element" => {
                if !self.t.elements.contains_key(&key) {
                    self.t.element_order.push(key.clone());
                }
                &mut self.t.elements
            }
            "complexType" | "simpleType" => &mut self.t.types,
            "group" => &mut self.t.groups,
            "attributeGroup" => &mut self.t.attr_groups,
            "attribute" => &mut self.t.attributes,
            _ => return,
        };
        table.entry(key).or_insert(src);
    }

    fn register_redefinitions(&mut self, doc: usize, redefine: NodeId) {
        let kids: Vec<NodeId> = self.docs[doc].doc.element_children(redefine).collect();
        for c in kids {
            let d = &self.docs[doc];
            let kind = lname(&d.doc, c);
            let Some(name) = d.doc.attr(c, "name") else {
                continue;
            };
            let key = (d.tns.clone(), name.to_string());
            let (tag, table) = match kind {
                "complexType" | "simpleType" => ("type", &mut self.t.types),
                "group" => ("group", &mut self.t.groups),
                "attributeGroup" => ("attributeGroup", &mut self.t.attr_groups),
                _ => continue,
            };
            if let Some(old) = table.insert(key.clone(), Src { doc, node: c }) {
                self.t.redefined.insert((tag, key), old);
            }
        }
    }

    /// Records schema constructs this engine ignores.
    fn report_unsupported(&mut self) {
        const KNOWN: &[&str] = &[
            "schema",
            "include",
            "import",
            "redefine",
            "annotation",
            "documentation",
            "appinfo",
            "element",
            "complexType",
            "simpleType",
            "group",
            "attributeGroup",
            "attribute",
            "sequence",
            "choice",
            "all",
            "any",
            "anyAttribute",
            "complexContent",
            "simpleContent",
            "extension",
            "restriction",
            "list",
            "union",
            "enumeration",
            "pattern",
            "length",
            "minLength",
            "maxLength",
            "minInclusive",
            "maxInclusive",
            "minExclusive",
            "maxExclusive",
            "totalDigits",
            "fractionDigits",
            "whiteSpace",
            "notation",
        ];
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for d in &self.docs {
            let Some(root) = d.doc.document_element() else {
                continue;
            };
            let mut stack = vec![root];
            while let Some(n) = stack.pop() {
                let name = lname(&d.doc, n);
                if name == "annotation" {
                    continue;
                }
                if !KNOWN.contains(&name) {
                    *counts.entry(name.to_string()).or_default() += 1;
                    continue;
                }
                stack.extend(d.doc.element_children(n));
            }
        }
        for (name, n) in counts {
            self.warnings.push(format!(
                "unsupported XSD construct 'xs:{name}' ignored ({n} occurrence(s))"
            ));
        }
    }
}

/// Per-type compilation state.
#[derive(Default)]
struct Slot {
    src: Option<Src>,
    state: SlotState,
    particle: Particle,
    mixed: bool,
    any_content: bool,
    simple: Option<TypeId>,
    attrs: Vec<AttrDecl>,
    any_attr: bool,
    doc: Option<String>,
}

#[derive(Default, PartialEq, Eq, Copy, Clone)]
enum SlotState {
    #[default]
    Pending,
    InProgress,
    Done,
}

/// Attribute uses gathered from a complex type or attribute group.
#[derive(Default, Clone)]
struct AttrUses {
    attrs: Vec<AttrDecl>,
    any: bool,
    prohibited: Vec<Key>,
}

struct Compiler<'l> {
    l: &'l Loader<'l>,
    g: Grammar,
    slots: Vec<Slot>,
    global_decls: HashMap<Key, DeclId>,
    named_complex: HashMap<Key, usize>,
    simple_slots: HashMap<TypeId, usize>,
    any_slot: Option<usize>,
    named_simple: HashMap<Key, TypeId>,
    simple_in_progress: HashSet<Key>,
    groups: HashMap<Key, Particle>,
    groups_in_progress: HashSet<Key>,
    attr_groups: HashMap<Key, AttrUses>,
    attr_groups_in_progress: HashSet<Key>,
    local_decls: HashMap<(usize, NodeId), DeclId>,
    wildcards: HashMap<(usize, NodeId), WildcardId>,
    patterns: HashMap<Vec<String>, Arc<PatternSet>>,
    subst_members: HashMap<Key, Vec<Key>>,
    head_in_progress: HashSet<Key>,
    queue: Vec<usize>,
}

impl<'l> Compiler<'l> {
    fn new(l: &'l Loader<'l>) -> Self {
        let mut subst_members: HashMap<Key, Vec<Key>> = HashMap::new();
        for key in &l.t.element_order {
            let src = l.t.elements[key];
            let d = &l.docs[src.doc];
            if let Some(head) = d.doc.attr(src.node, "substitutionGroup") {
                let hk = resolve_qname(d, src.node, head);
                subst_members.entry(hk).or_default().push(key.clone());
            }
        }
        let mut g = Grammar::new(GrammarKind::Xsd);
        g.warnings = l.warnings.clone();
        g.notations = l.notations.clone();
        Compiler {
            l,
            g,
            slots: Vec::new(),
            global_decls: HashMap::new(),
            named_complex: HashMap::new(),
            simple_slots: HashMap::new(),
            any_slot: None,
            named_simple: HashMap::new(),
            simple_in_progress: HashSet::new(),
            groups: HashMap::new(),
            groups_in_progress: HashSet::new(),
            attr_groups: HashMap::new(),
            attr_groups_in_progress: HashSet::new(),
            local_decls: HashMap::new(),
            wildcards: HashMap::new(),
            patterns: HashMap::new(),
            subst_members,
            head_in_progress: HashSet::new(),
            queue: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Grammar> {
        // Allocate all global declarations first so references resolve.
        for key in &self.l.t.element_order {
            let src = self.l.t.elements[key];
            let d = &self.l.docs[src.doc];
            let display = self.display_name(src.doc, key.0.as_deref(), &key.1);
            let sym = self.g.names.intern(key.0.as_deref(), &key.1, &display);
            let id = DeclId(self.g.decls.len() as u32);
            self.g.decls.push(ElementDecl {
                sym,
                ty: usize::MAX,
                global: true,
                is_abstract: d.doc.attr(src.node, "abstract") == Some("true"),
                declared: true,
                documentation: documentation(&d.doc, src.node),
            });
            self.global_decls.insert(key.clone(), id);
            self.g.globals.insert(sym, id);
        }
        let keys = self.l.t.element_order.clone();
        for key in &keys {
            self.global_type(key);
        }
        while let Some(idx) = self.queue.pop() {
            self.ensure_complex(idx);
        }
        // Compile one content model per type.
        let slots = std::mem::take(&mut self.slots);
        let models: Vec<Model> = slots
            .iter()
            .map(|s| Model::compile(&s.particle, &self.g))
            .collect();
        for (slot, model) in slots.into_iter().zip(models) {
            let content = if slot.simple.is_some() {
                ContentType::TextOnly
            } else if slot.any_content {
                ContentType::Any
            } else if !model.has_elements() {
                if slot.mixed {
                    ContentType::Mixed
                } else {
                    ContentType::Empty
                }
            } else if slot.mixed {
                ContentType::Mixed
            } else {
                ContentType::ElementOnly
            };
            self.g.types.push(ElemType {
                content,
                model,
                attrs: slot.attrs,
                any_attribute: slot.any_attr,
                simple: slot.simple,
                documentation: slot.doc,
            });
        }
        Ok(self.g)
    }

    fn doc(&self, i: usize) -> &'l SchemaDoc {
        &self.l.docs[i]
    }

    fn warn(&mut self, msg: String) {
        if !self.g.warnings.contains(&msg) {
            self.g.warnings.push(msg);
        }
    }

    /// Qualified display name using the schema's own prefix for `ns`.
    fn display_name(&self, doc: usize, ns: Option<&str>, local: &str) -> String {
        let Some(ns) = ns else {
            return local.to_string();
        };
        if ns == XML_NS {
            return format!("xml:{local}");
        }
        let d = self.doc(doc);
        // Prefer the prefix declared on the schema element of the defining document,
        // then of any loaded document.
        let docs = std::iter::once(d).chain(self.l.docs.iter());
        for sd in docs {
            if let Some(root) = sd.doc.document_element()
                && let Some(e) = sd.doc.element(root)
            {
                for a in &e.attrs {
                    if let Some(p) = a.name.strip_prefix("xmlns:")
                        && a.value == ns
                    {
                        return format!("{p}:{local}");
                    }
                }
            }
        }
        local.to_string()
    }

    // ----------------------------------------------------------- slots

    fn new_slot(&mut self, slot: Slot) -> usize {
        self.slots.push(slot);
        self.slots.len() - 1
    }

    fn any_slot(&mut self) -> usize {
        if let Some(i) = self.any_slot {
            return i;
        }
        let i = self.new_slot(Slot {
            state: SlotState::Done,
            any_content: true,
            any_attr: true,
            ..Slot::default()
        });
        self.any_slot = Some(i);
        i
    }

    fn simple_slot(&mut self, t: TypeId) -> usize {
        if let Some(&i) = self.simple_slots.get(&t) {
            return i;
        }
        let i = self.new_slot(Slot {
            state: SlotState::Done,
            simple: Some(t),
            ..Slot::default()
        });
        self.simple_slots.insert(t, i);
        i
    }

    fn complex_slot(&mut self, src: Src) -> usize {
        let i = self.new_slot(Slot {
            src: Some(src),
            ..Slot::default()
        });
        self.queue.push(i);
        i
    }

    // ----------------------------------------------------------- element types

    fn global_type(&mut self, key: &Key) -> usize {
        let id = self.global_decls[key];
        let ty = self.g.decls[id.0 as usize].ty;
        if ty != usize::MAX {
            return ty;
        }
        if !self.head_in_progress.insert(key.clone()) {
            return self.any_slot();
        }
        let src = self.l.t.elements[key];
        let ty = self.element_type(src);
        self.head_in_progress.remove(key);
        self.g.decls[id.0 as usize].ty = ty;
        ty
    }

    /// The type slot of an `xs:element` declaration.
    fn element_type(&mut self, src: Src) -> usize {
        let d = self.doc(src.doc);
        if let Some(t) = d.doc.attr(src.node, "type") {
            let key = resolve_qname(d, src.node, t);
            return self.type_slot_for(&key);
        }
        if let Some(ct) = children_named(&d.doc, src.node, &["complexType"]).next() {
            return self.complex_slot(Src {
                doc: src.doc,
                node: ct,
            });
        }
        if let Some(st) = children_named(&d.doc, src.node, &["simpleType"]).next() {
            let t = self.compile_simple(src.doc, st, None);
            return self.simple_slot(t);
        }
        if let Some(head) = d.doc.attr(src.node, "substitutionGroup") {
            let hk = resolve_qname(d, src.node, head);
            if self.global_decls.contains_key(&hk) {
                return self.global_type(&hk);
            }
        }
        self.any_slot()
    }

    fn type_slot_for(&mut self, key: &Key) -> usize {
        if key.0.as_deref() == Some(XSD_NS) {
            if key.1 == "anyType" {
                return self.any_slot();
            }
            let t = self.builtin_or_string(&key.1);
            return self.simple_slot(t);
        }
        if let Some(&i) = self.named_complex.get(key) {
            return i;
        }
        match self.l.t.types.get(key) {
            Some(&src) if lname(&self.doc(src.doc).doc, src.node) == "complexType" => {
                let i = self.complex_slot(src);
                self.named_complex.insert(key.clone(), i);
                i
            }
            Some(_) => {
                let t = self.named_simple_type(key);
                self.simple_slot(t)
            }
            None => {
                self.warn(format!("unknown type '{}'", fmt_key(key)));
                self.any_slot()
            }
        }
    }

    fn builtin_or_string(&mut self, local: &str) -> TypeId {
        match self.g.simple.builtin(local) {
            Some(t) => t,
            None => {
                self.warn(format!("unknown built-in type 'xs:{local}'"));
                self.g.simple.string()
            }
        }
    }

    /// Makes sure slot `idx` (a complex type) is compiled.
    fn ensure_complex(&mut self, idx: usize) {
        if self.slots[idx].state != SlotState::Pending {
            return;
        }
        let Some(src) = self.slots[idx].src else {
            self.slots[idx].state = SlotState::Done;
            return;
        };
        self.slots[idx].state = SlotState::InProgress;
        let d = self.doc(src.doc);
        let doc = &d.doc;
        let node = src.node;
        let mut slot = Slot {
            src: Some(src),
            state: SlotState::Done,
            mixed: doc.attr(node, "mixed") == Some("true"),
            doc: documentation(doc, node),
            ..Slot::default()
        };
        let content = doc
            .element_children(node)
            .find(|&c| matches!(lname(doc, c), "simpleContent" | "complexContent"));
        match content.map(|c| (c, lname(doc, c))) {
            Some((sc, "simpleContent")) => self.simple_content(src.doc, sc, &mut slot),
            Some((cc, _)) => {
                if let Some(m) = doc.attr(cc, "mixed") {
                    slot.mixed = m == "true";
                }
                self.complex_content(src.doc, cc, &mut slot);
            }
            None => {
                slot.particle = self.content_particle(src.doc, node);
                let uses = self.attr_uses(src.doc, node);
                slot.attrs = uses.attrs;
                slot.any_attr = uses.any;
            }
        }
        self.slots[idx] = slot;
    }

    /// The single model group child (`sequence`, `choice`, `all`, `group`) of a node.
    fn content_particle(&mut self, doc: usize, node: NodeId) -> Particle {
        let d = self.doc(doc);
        match children_named(&d.doc, node, &["sequence", "choice", "all", "group"]).next() {
            Some(g) => self.particle(doc, g),
            None => Particle::Empty,
        }
    }

    fn base_complex(&mut self, doc: usize, node: NodeId) -> Option<BaseKind> {
        let d = self.doc(doc);
        let base = d.doc.attr(node, "base")?;
        let key = resolve_qname(d, node, base);
        if key.0.as_deref() == Some(XSD_NS) {
            if key.1 == "anyType" {
                return Some(BaseKind::AnyType);
            }
            return Some(BaseKind::Simple(self.builtin_or_string(&key.1)));
        }
        // Inside a redefinition, a self-reference names the original definition.
        let redefined = self.redefine_original("type", doc, node, &key);
        let src = redefined.or_else(|| self.l.t.types.get(&key).copied());
        match src {
            Some(s) if lname(&self.doc(s.doc).doc, s.node) == "complexType" => {
                let idx = match (redefined, self.named_complex.get(&key)) {
                    (None, Some(&i)) => i,
                    _ => {
                        let i = self.complex_slot(s);
                        if redefined.is_none() {
                            self.named_complex.insert(key.clone(), i);
                        }
                        i
                    }
                };
                if self.slots[idx].state == SlotState::InProgress {
                    self.warn(format!("circular derivation of type '{}'", fmt_key(&key)));
                    return Some(BaseKind::AnyType);
                }
                self.ensure_complex(idx);
                Some(BaseKind::Complex(idx))
            }
            Some(_) => Some(BaseKind::Simple(self.named_simple_type(&key))),
            None => {
                self.warn(format!("unknown base type '{}'", fmt_key(&key)));
                Some(BaseKind::AnyType)
            }
        }
    }

    fn simple_content(&mut self, doc: usize, sc: NodeId, slot: &mut Slot) {
        let d = self.doc(doc);
        let Some(der) = children_named(&d.doc, sc, &["extension", "restriction"]).next() else {
            return;
        };
        let is_ext = lname(&d.doc, der) == "extension";
        let base = self.base_complex(doc, der);
        let (base_simple, base_attrs, base_any) = match base {
            Some(BaseKind::Simple(t)) => (t, Vec::new(), false),
            Some(BaseKind::Complex(i)) => {
                let s = &self.slots[i];
                (
                    s.simple.unwrap_or_else(|| self.g.simple.string()),
                    s.attrs.clone(),
                    s.any_attr,
                )
            }
            _ => (self.g.simple.string(), Vec::new(), false),
        };
        let uses = self.attr_uses(doc, der);
        slot.any_attr = base_any || uses.any;
        if is_ext {
            slot.simple = Some(base_simple);
            slot.attrs = extend_attrs(base_attrs, uses.attrs);
        } else {
            let d = self.doc(doc);
            let base_simple = match children_named(&d.doc, der, &["simpleType"]).next() {
                Some(st) => self.compile_simple(doc, st, None),
                None => base_simple,
            };
            let mut st = self.g.simple.restrict(base_simple);
            self.apply_facets(doc, der, &mut st);
            slot.simple = Some(self.g.simple.add(st));
            slot.attrs = restrict_attrs(base_attrs, uses);
        }
    }

    fn complex_content(&mut self, doc: usize, cc: NodeId, slot: &mut Slot) {
        let d = self.doc(doc);
        let Some(der) = children_named(&d.doc, cc, &["extension", "restriction"]).next() else {
            return;
        };
        let is_ext = lname(&d.doc, der) == "extension";
        let base = self.base_complex(doc, der);
        let own = self.content_particle(doc, der);
        let uses = self.attr_uses(doc, der);
        let (base_particle, base_attrs, base_any, base_mixed) = match base {
            Some(BaseKind::Complex(i)) => {
                let s = &self.slots[i];
                (s.particle.clone(), s.attrs.clone(), s.any_attr, s.mixed)
            }
            _ => (Particle::Empty, Vec::new(), false, false),
        };
        if is_ext {
            slot.particle = match (base_particle.is_empty(), own.is_empty()) {
                (true, _) => own,
                (false, true) => base_particle,
                (false, false) => Particle::Seq(vec![base_particle, own]),
            };
            slot.attrs = extend_attrs(base_attrs, uses.attrs);
            slot.any_attr = base_any || uses.any;
            slot.mixed |= base_mixed;
        } else {
            slot.particle = own;
            slot.any_attr = uses.any;
            slot.attrs = restrict_attrs(base_attrs, uses);
        }
    }

    fn redefine_original(
        &self,
        tag: &'static str,
        doc: usize,
        node: NodeId,
        key: &Key,
    ) -> Option<Src> {
        let d = self.doc(doc);
        // Find the top-level component containing `node`, directly under xs:redefine.
        let mut cur = node;
        let mut top = None;
        for a in d.doc.ancestors(node) {
            if lname(&d.doc, a) == "redefine" {
                top = Some(cur);
                break;
            }
            cur = a;
        }
        let top = top?;
        (d.doc.attr(top, "name") == Some(key.1.as_str()))
            .then(|| self.l.t.redefined.get(&(tag, key.clone())).copied())?
    }

    // ----------------------------------------------------------- particles

    fn particle(&mut self, doc: usize, node: NodeId) -> Particle {
        let d = self.doc(doc);
        let (min, max) = parse_occurs(&d.doc, node);
        let kind = lname(&d.doc, node);
        let p = match kind {
            "sequence" | "choice" | "all" => {
                let kids: Vec<NodeId> = children_named(
                    &d.doc,
                    node,
                    &["element", "group", "sequence", "choice", "any", "all"],
                )
                .collect();
                let items: Vec<Particle> =
                    kids.into_iter().map(|k| self.particle(doc, k)).collect();
                match kind {
                    "sequence" => Particle::Seq(items),
                    "choice" => Particle::Choice(items),
                    _ => Particle::All(items),
                }
            }
            "group" => match d.doc.attr(node, "ref") {
                Some(r) => {
                    let key = resolve_qname(d, node, r);
                    self.group_particle(doc, node, &key)
                }
                None => self.content_particle(doc, node),
            },
            "element" => self.element_particle(doc, node),
            "any" => Particle::Term(Term::Any(self.wildcard(doc, node))),
            _ => Particle::Empty,
        };
        p.repeat(min, max)
    }

    fn group_particle(&mut self, doc: usize, node: NodeId, key: &Key) -> Particle {
        if let Some(orig) = self.redefine_original("group", doc, node, key) {
            return self.content_particle(orig.doc, orig.node);
        }
        if let Some(p) = self.groups.get(key) {
            return p.clone();
        }
        let Some(&src) = self.l.t.groups.get(key) else {
            self.warn(format!("unknown group '{}'", fmt_key(key)));
            return Particle::Empty;
        };
        if !self.groups_in_progress.insert(key.clone()) {
            self.warn(format!("circular group '{}'", fmt_key(key)));
            return Particle::Empty;
        }
        let p = self.content_particle(src.doc, src.node);
        self.groups_in_progress.remove(key);
        self.groups.insert(key.clone(), p.clone());
        p
    }

    fn element_particle(&mut self, doc: usize, node: NodeId) -> Particle {
        let d = self.doc(doc);
        if let Some(r) = d.doc.attr(node, "ref") {
            let key = resolve_qname(d, node, r);
            return self.element_ref(&key);
        }
        if let Some(&id) = self.local_decls.get(&(doc, node)) {
            return Particle::Term(Term::Elem(id));
        }
        let Some(name) = d.doc.attr(node, "name") else {
            return Particle::Empty;
        };
        let qualified = match d.doc.attr(node, "form") {
            Some(f) => f == "qualified",
            None => d.elem_qualified,
        };
        let ns = if qualified { d.tns.clone() } else { None };
        let display = self.display_name(doc, ns.as_deref(), name);
        let sym = self.g.names.intern(ns.as_deref(), name, &display);
        let id = DeclId(self.g.decls.len() as u32);
        self.g.decls.push(ElementDecl {
            sym,
            ty: usize::MAX,
            global: false,
            is_abstract: false,
            declared: true,
            documentation: documentation(&d.doc, node),
        });
        self.local_decls.insert((doc, node), id);
        let ty = self.element_type(Src { doc, node });
        self.g.decls[id.0 as usize].ty = ty;
        Particle::Term(Term::Elem(id))
    }

    /// A reference to a global element, expanded over its substitution group.
    fn element_ref(&mut self, key: &Key) -> Particle {
        if !self.global_decls.contains_key(key) {
            self.warn(format!("unknown element '{}'", fmt_key(key)));
            return Particle::Empty;
        }
        let mut members = vec![key.clone()];
        let mut i = 0;
        while i < members.len() {
            if let Some(m) = self.subst_members.get(&members[i]) {
                for k in m {
                    if !members.contains(k) {
                        members.push(k.clone());
                    }
                }
            }
            i += 1;
        }
        let mut alts: Vec<Particle> = Vec::new();
        for k in &members {
            let Some(&id) = self.global_decls.get(k) else {
                continue;
            };
            if !self.g.decls[id.0 as usize].is_abstract {
                alts.push(Particle::Term(Term::Elem(id)));
            }
        }
        match alts.len() {
            1 => alts.pop().unwrap_or(Particle::Empty),
            _ => Particle::Choice(alts),
        }
    }

    fn wildcard(&mut self, doc: usize, node: NodeId) -> WildcardId {
        if let Some(&w) = self.wildcards.get(&(doc, node)) {
            return w;
        }
        let d = self.doc(doc);
        let process = match d.doc.attr(node, "processContents") {
            Some("lax") => ProcessContents::Lax,
            Some("skip") => ProcessContents::Skip,
            _ => ProcessContents::Strict,
        };
        let ns = wildcard_ns(d.doc.attr(node, "namespace"), d.tns.as_deref());
        let id = WildcardId(self.g.wildcards.len() as u32);
        self.g.wildcards.push(Wildcard { process, ns });
        self.wildcards.insert((doc, node), id);
        id
    }

    // ----------------------------------------------------------- attributes

    fn attr_uses(&mut self, doc: usize, node: NodeId) -> AttrUses {
        let mut uses = AttrUses::default();
        let d = self.doc(doc);
        let kids: Vec<NodeId> = children_named(
            &d.doc,
            node,
            &["attribute", "attributeGroup", "anyAttribute"],
        )
        .collect();
        for k in kids {
            match lname(&d.doc, k) {
                "anyAttribute" => uses.any = true,
                "attributeGroup" => {
                    let Some(r) = d.doc.attr(k, "ref") else {
                        continue;
                    };
                    let key = resolve_qname(d, k, r);
                    let g = self.attr_group(doc, k, &key);
                    uses.any |= g.any;
                    uses.prohibited.extend(g.prohibited);
                    for a in g.attrs {
                        add_attr(&mut uses.attrs, a);
                    }
                }
                _ => {
                    if d.doc.attr(k, "use") == Some("prohibited") {
                        let key = match d.doc.attr(k, "ref") {
                            Some(r) => resolve_qname(d, k, r),
                            None => (None, d.doc.attr(k, "name").unwrap_or("").to_string()),
                        };
                        uses.prohibited.push(key);
                        continue;
                    }
                    if let Some(a) = self.attribute(doc, k) {
                        add_attr(&mut uses.attrs, a);
                    }
                }
            }
        }
        uses
    }

    fn attr_group(&mut self, doc: usize, node: NodeId, key: &Key) -> AttrUses {
        if let Some(orig) = self.redefine_original("attributeGroup", doc, node, key) {
            return self.attr_uses(orig.doc, orig.node);
        }
        if let Some(g) = self.attr_groups.get(key) {
            return g.clone();
        }
        let Some(&src) = self.l.t.attr_groups.get(key) else {
            self.warn(format!("unknown attribute group '{}'", fmt_key(key)));
            return AttrUses::default();
        };
        if !self.attr_groups_in_progress.insert(key.clone()) {
            return AttrUses::default();
        }
        let g = self.attr_uses(src.doc, src.node);
        self.attr_groups_in_progress.remove(key);
        self.attr_groups.insert(key.clone(), g.clone());
        g
    }

    /// Builds an attribute declaration from an `xs:attribute` use.
    fn attribute(&mut self, doc: usize, node: NodeId) -> Option<AttrDecl> {
        let d = self.doc(doc);
        let use_site = |name: &str| d.doc.attr(node, name).map(str::to_string);
        let required = d.doc.attr(node, "use") == Some("required");
        let (decl_doc, decl_node, ns, local) = match d.doc.attr(node, "ref") {
            Some(r) => {
                let key = resolve_qname(d, node, r);
                match self.l.t.attributes.get(&key) {
                    Some(&src) => (src.doc, src.node, key.0, key.1),
                    None if key.0.as_deref() == Some(XML_NS) => {
                        let mut a = self.make_attr(
                            doc,
                            Some(XML_NS.to_string()),
                            key.1,
                            self.g.simple.string(),
                        );
                        a.required = required;
                        return Some(a);
                    }
                    None => {
                        self.warn(format!("unknown attribute '{}'", fmt_key(&key)));
                        return None;
                    }
                }
            }
            None => {
                let name = d.doc.attr(node, "name")?.to_string();
                let qualified = match d.doc.attr(node, "form") {
                    Some(f) => f == "qualified",
                    None => d.attr_qualified,
                };
                (
                    doc,
                    node,
                    if qualified { d.tns.clone() } else { None },
                    name,
                )
            }
        };
        let dd = self.doc(decl_doc);
        let datatype = match dd.doc.attr(decl_node, "type") {
            Some(t) => {
                let key = resolve_qname(dd, decl_node, t);
                if key.0.as_deref() == Some(XSD_NS) {
                    self.builtin_or_string(&key.1)
                } else {
                    self.named_simple_type(&key)
                }
            }
            None => match children_named(&dd.doc, decl_node, &["simpleType"]).next() {
                Some(st) => self.compile_simple(decl_doc, st, None),
                None => self
                    .g
                    .simple
                    .builtin("anySimpleType")
                    .unwrap_or(self.g.simple.string()),
            },
        };
        let mut a = self.make_attr(decl_doc, ns, local, datatype);
        a.required = required;
        a.default =
            use_site("default").or_else(|| dd.doc.attr(decl_node, "default").map(str::to_string));
        a.fixed = use_site("fixed").or_else(|| dd.doc.attr(decl_node, "fixed").map(str::to_string));
        a.documentation = documentation(&d.doc, node).or_else(|| documentation(&dd.doc, decl_node));
        Some(a)
    }

    fn make_attr(
        &self,
        doc: usize,
        ns: Option<String>,
        local: String,
        datatype: TypeId,
    ) -> AttrDecl {
        let st = self.g.simple.get(datatype);
        let type_name = self.g.simple.describe(datatype);
        AttrDecl {
            name: self.display_name(doc, ns.as_deref(), &local),
            local_name: local,
            namespace: ns,
            kind: attr_kind(&self.g.simple, datatype),
            datatype,
            type_name,
            required: false,
            default: None,
            fixed: None,
            enum_values: st.facets.enumeration.clone().unwrap_or_default(),
            documentation: None,
        }
    }

    // ----------------------------------------------------------- simple types

    fn named_simple_type(&mut self, key: &Key) -> TypeId {
        if key.0.as_deref() == Some(XSD_NS) {
            return self.builtin_or_string(&key.1);
        }
        if let Some(&t) = self.named_simple.get(key) {
            return t;
        }
        let Some(&src) = self.l.t.types.get(key) else {
            self.warn(format!("unknown simple type '{}'", fmt_key(key)));
            return self.g.simple.string();
        };
        if lname(&self.doc(src.doc).doc, src.node) != "simpleType"
            || !self.simple_in_progress.insert(key.clone())
        {
            return self.g.simple.string();
        }
        let display = self.display_name(src.doc, key.0.as_deref(), &key.1);
        let t = self.compile_simple(src.doc, src.node, Some(display));
        self.simple_in_progress.remove(key);
        self.named_simple.insert(key.clone(), t);
        t
    }

    fn simple_ref(&mut self, doc: usize, node: NodeId, attr: &str) -> Option<TypeId> {
        let d = self.doc(doc);
        let q = d.doc.attr(node, attr)?;
        let key = resolve_qname(d, node, q);
        Some(self.named_simple_type(&key))
    }

    fn compile_simple(&mut self, doc: usize, node: NodeId, name: Option<String>) -> TypeId {
        let d = self.doc(doc);
        let Some(def) = children_named(&d.doc, node, &["restriction", "list", "union"]).next()
        else {
            return self.g.simple.string();
        };
        let mut st = match lname(&d.doc, def) {
            "restriction" => {
                let base = match self.simple_ref(doc, def, "base") {
                    Some(t) => t,
                    None => match children_named(&d.doc, def, &["simpleType"]).next() {
                        Some(inner) => self.compile_simple(doc, inner, None),
                        None => self.g.simple.string(),
                    },
                };
                let mut st = self.g.simple.restrict(base);
                self.apply_facets(doc, def, &mut st);
                st
            }
            "list" => {
                let item = match self.simple_ref(doc, def, "itemType") {
                    Some(t) => t,
                    None => match children_named(&d.doc, def, &["simpleType"]).next() {
                        Some(inner) => self.compile_simple(doc, inner, None),
                        None => self.g.simple.string(),
                    },
                };
                SimpleType {
                    name: None,
                    builtin: self.g.simple.get(item).builtin,
                    variety: Variety::List(item),
                    whitespace: WhiteSpace::Collapse,
                    facets: Facets::default(),
                }
            }
            _ => {
                let mut members = Vec::new();
                if let Some(list) = d.doc.attr(def, "memberTypes") {
                    for q in list.split_whitespace() {
                        let key = resolve_qname(d, def, q);
                        members.push(self.named_simple_type(&key));
                    }
                }
                let inner: Vec<NodeId> = children_named(&d.doc, def, &["simpleType"]).collect();
                for i in inner {
                    members.push(self.compile_simple(doc, i, None));
                }
                SimpleType {
                    name: None,
                    builtin: crate::Builtin::AnySimpleType,
                    variety: Variety::Union(members),
                    whitespace: WhiteSpace::Collapse,
                    facets: Facets::default(),
                }
            }
        };
        st.name = name;
        self.g.simple.add(st)
    }

    fn apply_facets(&mut self, doc: usize, restriction: NodeId, st: &mut SimpleType) {
        let d = self.doc(doc);
        let mut enums = Vec::new();
        let mut patterns = Vec::new();
        for f in d.doc.element_children(restriction) {
            let Some(value) = d.doc.attr(f, "value") else {
                continue;
            };
            let num = || value.trim().parse::<usize>().ok();
            match lname(&d.doc, f) {
                "enumeration" => enums.push(st.whitespace.apply(value).into_owned()),
                "pattern" => patterns.push(value.to_string()),
                "length" => st.facets.length = num(),
                "minLength" => st.facets.min_length = num(),
                "maxLength" => st.facets.max_length = num(),
                "minInclusive" => st.facets.min_inclusive = Some(value.trim().to_string()),
                "maxInclusive" => st.facets.max_inclusive = Some(value.trim().to_string()),
                "minExclusive" => st.facets.min_exclusive = Some(value.trim().to_string()),
                "maxExclusive" => st.facets.max_exclusive = Some(value.trim().to_string()),
                "totalDigits" => st.facets.total_digits = value.trim().parse().ok(),
                "fractionDigits" => st.facets.fraction_digits = value.trim().parse().ok(),
                "whiteSpace" => {
                    st.whitespace = match value.trim() {
                        "preserve" => WhiteSpace::Preserve,
                        "replace" => WhiteSpace::Replace,
                        _ => WhiteSpace::Collapse,
                    }
                }
                _ => {}
            }
        }
        if !enums.is_empty() {
            st.facets.set_enumeration(enums);
        }
        if !patterns.is_empty() {
            for p in &patterns {
                if let Err(e) = crate::xsdregex::translate(p) {
                    let msg = format!(
                        "pattern '{p}' in '{}' is not supported ({e}); it is not checked",
                        d.path.display()
                    );
                    self.warn(msg);
                }
            }
            let set = self
                .patterns
                .entry(patterns.clone())
                .or_insert_with(|| Arc::new(PatternSet::new(patterns)))
                .clone();
            st.facets.patterns.push(set);
        }
    }
}

enum BaseKind {
    AnyType,
    Simple(TypeId),
    Complex(usize),
}

fn fmt_key(key: &Key) -> String {
    match &key.0 {
        Some(ns) => format!("{{{ns}}}{}", key.1),
        None => key.1.clone(),
    }
}

fn same_attr(a: &AttrDecl, ns: Option<&str>, local: &str) -> bool {
    a.namespace.as_deref() == ns && a.local_name == local
}

fn add_attr(list: &mut Vec<AttrDecl>, a: AttrDecl) {
    match list
        .iter_mut()
        .find(|x| same_attr(x, a.namespace.as_deref(), &a.local_name))
    {
        Some(existing) => *existing = a,
        None => list.push(a),
    }
}

fn extend_attrs(mut base: Vec<AttrDecl>, own: Vec<AttrDecl>) -> Vec<AttrDecl> {
    for a in own {
        add_attr(&mut base, a);
    }
    base
}

fn restrict_attrs(base: Vec<AttrDecl>, uses: AttrUses) -> Vec<AttrDecl> {
    let mut out: Vec<AttrDecl> = base
        .into_iter()
        .filter(|a| {
            !uses
                .prohibited
                .iter()
                .any(|(ns, l)| same_attr(a, ns.as_deref(), l))
        })
        .collect();
    for a in uses.attrs {
        add_attr(&mut out, a);
    }
    out
}

fn wildcard_ns(spec: Option<&str>, tns: Option<&str>) -> NsConstraint {
    match spec.map(str::trim) {
        None | Some("##any") => NsConstraint::Any,
        Some("##other") => NsConstraint::Other(tns.map(str::to_string)),
        Some(list) => NsConstraint::List(
            list.split_whitespace()
                .map(|t| match t {
                    "##local" => None,
                    "##targetNamespace" => tns.map(str::to_string),
                    uri => Some(uri.to_string()),
                })
                .collect(),
        ),
    }
}

/// Resolves a QName-valued schema attribute in the scope of `node`.
fn resolve_qname(d: &SchemaDoc, node: NodeId, qname: &str) -> Key {
    let qname = qname.trim();
    let scope = NsScope::at(&d.doc, node);
    let (ns, local) = match qname.split_once(':') {
        Some((p, l)) => (scope.resolve(p).map(str::to_string), l),
        None => (scope.resolve("").map(str::to_string), qname),
    };
    let ns = match ns {
        None if d.chameleon => d.tns.clone(),
        other => other,
    };
    (ns, local_name(local).to_string())
}
