//! Editing queries: what may be inserted, wrapped or renamed, and minimal
//! valid skeletons for new elements.
//!
//! Every query ranks the parent's child-element sequence the same way: the
//! number of misplaced elements (with error recovery), then whether the
//! sequence is complete. An edit "keeps the document valid" when it does not
//! make that rank worse. Text, comments and processing instructions are
//! ignored when computing element sequences.

use xylem_dom::{Document, NodeId, NodeKind};

use crate::model::{InstName, Term};
use crate::{ContentType, DeclId, Grammar, NsScope};

/// An element that may be inserted at a position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Insertable {
    /// Qualified name to create, using a prefix bound in the document when possible.
    pub name: String,
    pub decl: DeclId,
    /// Inserting keeps the parent's children as valid as they are now; when
    /// false the insertion is only a viable prefix (more edits are needed).
    pub keeps_valid: bool,
}

/// A minimal valid subtree for a new element.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skeleton {
    pub name: String,
    /// Required attributes with their fixed, default or first enumerated value.
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Skeleton>,
    /// The element accepts text; the editor may place the caret inside.
    pub wants_text: bool,
}

/// Recursion bound for [`Grammar::required_skeleton`].
const MAX_SKELETON_DEPTH: usize = 12;

fn is_ws(s: &str) -> bool {
    s.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
}

impl Grammar {
    fn decl_inst_name(&self, d: DeclId) -> InstName<'_> {
        let sym = self.decl(d).sym;
        InstName {
            sym: Some(sym),
            ns: self.names.get(sym).ns.as_deref(),
        }
    }

    /// Resolved names of the element children of `parent`, with their child-list indices.
    fn child_names<'d>(&self, doc: &'d Document, parent: NodeId) -> Vec<(usize, InstName<'d>)> {
        let mut scope = NsScope::at(doc, parent);
        let mut out = Vec::new();
        for (i, &c) in doc.children(parent).iter().enumerate() {
            if let Some(el) = doc.element(c) {
                let mark = scope.push(el);
                out.push((i, self.inst_name(&el.name, &scope)));
                scope.pop(mark);
            }
        }
        out
    }

    /// Insertion name for a declaration, honouring prefixes in scope at `node`.
    fn insertion_name(&self, doc: &Document, node: NodeId, d: DeclId) -> String {
        let q = self.names.get(self.decl(d).sym);
        let Some(ns) = q.ns.as_deref().filter(|_| self.namespace_aware) else {
            return q.display.clone();
        };
        match NsScope::at(doc, node).prefix_for(ns) {
            Some("") => q.local.clone(),
            Some(p) => format!("{p}:{}", q.local),
            None => q.display.clone(),
        }
    }

    /// Global declarations usable where a wildcard or `ANY` content appears.
    fn wildcard_candidates(&self, term: Term) -> Vec<DeclId> {
        let mut v: Vec<DeclId> = self
            .globals
            .values()
            .copied()
            .filter(|&d| {
                let decl = self.decl(d);
                let ok_ns = match term {
                    Term::Any(w) => self.wildcards[w.0 as usize]
                        .matches_ns(self.names.get(decl.sym).ns.as_deref()),
                    Term::Elem(_) => true,
                };
                decl.declared && !decl.is_abstract && ok_ns
            })
            .collect();
        v.sort_by(|a, b| self.decl_name(*a).cmp(self.decl_name(*b)));
        v
    }

    fn expand_terms(&self, terms: &[Term]) -> Vec<DeclId> {
        let mut out = Vec::new();
        for &t in terms {
            match t {
                Term::Elem(d) => out.push(d),
                Term::Any(_) => out.extend(self.wildcard_candidates(t)),
            }
        }
        out
    }

    /// Elements that may be inserted between `children[child_index - 1]` and
    /// `children[child_index]` of `parent` (indices into all child nodes).
    /// Sorted by name.
    pub fn insertable_at(
        &self,
        doc: &Document,
        parent: NodeId,
        child_index: usize,
    ) -> Vec<Insertable> {
        if matches!(doc.kind(parent), NodeKind::Document) {
            if doc.document_element().is_some() {
                return Vec::new();
            }
            return self
                .root_candidates()
                .into_iter()
                .filter_map(|n| {
                    Some(Insertable {
                        decl: self.global_decl(&n)?,
                        name: n,
                        keeps_valid: true,
                    })
                })
                .collect();
        }
        let Some(decl) = self.element_decl_for(doc, parent) else {
            return Vec::new();
        };
        let ty = self.ty(decl);
        let candidates: Vec<(DeclId, bool)> = match ty.content {
            ContentType::Empty | ContentType::TextOnly => return Vec::new(),
            ContentType::Any => self
                .wildcard_candidates(Term::Elem(decl))
                .into_iter()
                .map(|d| (d, true))
                .collect(),
            ContentType::ElementOnly | ContentType::Mixed => {
                let names = self.child_names(doc, parent);
                let split = names
                    .iter()
                    .position(|(i, _)| *i >= child_index)
                    .unwrap_or(names.len());
                let before: Vec<InstName<'_>> = names[..split].iter().map(|n| n.1).collect();
                let after: Vec<InstName<'_>> = names[split..].iter().map(|n| n.1).collect();
                let base = self
                    .run_model(decl, before.iter().chain(&after).copied())
                    .rank();
                let state = self.run_model(decl, before.iter().copied()).state;
                let allowed = ty.model.allowed(&state);
                self.expand_terms(&allowed)
                    .into_iter()
                    .map(|d| {
                        let n = self.decl_inst_name(d);
                        let seq = before
                            .iter()
                            .copied()
                            .chain(std::iter::once(n))
                            .chain(after.iter().copied());
                        (d, self.run_model(decl, seq).rank() <= base)
                    })
                    .collect()
            }
        };
        let mut out: Vec<Insertable> = Vec::new();
        for (d, keeps_valid) in candidates {
            let name = self.insertion_name(doc, parent, d);
            match out.iter_mut().find(|x| x.name == name) {
                Some(x) => x.keeps_valid |= keeps_valid,
                None => out.push(Insertable {
                    name,
                    decl: d,
                    keeps_valid,
                }),
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    /// Whether an element called `name` may be inserted at `index` (a viable
    /// position, not necessarily one that leaves the parent complete).
    pub fn can_insert(&self, doc: &Document, parent: NodeId, index: usize, name: &str) -> bool {
        if matches!(doc.kind(parent), NodeKind::Document) {
            return doc.document_element().is_none()
                && self.root_candidates().iter().any(|r| r == name);
        }
        let Some(decl) = self.element_decl_for(doc, parent) else {
            return false;
        };
        let ty = self.ty(decl);
        let scope = NsScope::at(doc, parent);
        let inst = self.inst_name(name, &scope);
        match ty.content {
            ContentType::Empty | ContentType::TextOnly => false,
            ContentType::Any => self.global_for_sym(inst.sym).is_some(),
            ContentType::ElementOnly | ContentType::Mixed => {
                let names = self.child_names(doc, parent);
                let before = names.iter().filter(|(i, _)| *i < index).map(|n| n.1);
                let state = self.run_model(decl, before).state;
                ty.model.step(&state, inst, self).is_some()
            }
        }
    }

    /// Whether text may be typed directly inside `parent`. Elements the
    /// grammar does not know are treated permissively.
    pub fn can_contain_text(&self, doc: &Document, parent: NodeId) -> bool {
        if !doc.is_element(parent) {
            return false;
        }
        match self.element_decl_for(doc, parent) {
            Some(d) => matches!(
                self.content_type(d),
                ContentType::Mixed | ContentType::TextOnly | ContentType::Any
            ),
            None => true,
        }
    }

    /// Whether `children[start..end]` of `parent` may be wrapped in a new
    /// element `name`: the wrapper must be allowed in `parent` (without making
    /// it less valid) and the wrapped nodes must be acceptable inside it.
    pub fn can_surround(
        &self,
        doc: &Document,
        parent: NodeId,
        start: usize,
        end: usize,
        name: &str,
    ) -> bool {
        let kids = doc.children(parent);
        if start > end || end > kids.len() {
            return false;
        }
        let scope = NsScope::at(doc, parent);
        let inst = self.inst_name(name, &scope);
        let new_decl = if matches!(doc.kind(parent), NodeKind::Document) {
            let wraps_root = kids[start..end].iter().any(|&c| doc.is_element(c));
            if !wraps_root || !self.root_candidates().iter().any(|r| r == name) {
                return false;
            }
            self.global_for_sym(inst.sym)
        } else {
            let Some(decl) = self.element_decl_for(doc, parent) else {
                return false;
            };
            let ty = self.ty(decl);
            match ty.content {
                ContentType::Empty | ContentType::TextOnly => return false,
                ContentType::Any => self.global_for_sym(inst.sym),
                ContentType::ElementOnly | ContentType::Mixed => {
                    let names = self.child_names(doc, parent);
                    let before: Vec<InstName<'_>> = names
                        .iter()
                        .filter(|(i, _)| *i < start)
                        .map(|n| n.1)
                        .collect();
                    let after: Vec<InstName<'_>> = names
                        .iter()
                        .filter(|(i, _)| *i >= end)
                        .map(|n| n.1)
                        .collect();
                    let state = self.run_model(decl, before.iter().copied()).state;
                    let Some((_, term)) = ty.model.step(&state, inst, self) else {
                        return false;
                    };
                    let old = self.run_model(decl, names.iter().map(|n| n.1)).rank();
                    let seq = before
                        .iter()
                        .copied()
                        .chain(std::iter::once(inst))
                        .chain(after.iter().copied());
                    if self.run_model(decl, seq).rank() > old {
                        return false;
                    }
                    self.term_decl(term, inst)
                }
            }
        };
        let Some(new_decl) = new_decl else {
            return true;
        };
        // The wrapped nodes must fit inside the new element.
        let wrapped: Vec<(usize, InstName<'_>)> = self
            .child_names(doc, parent)
            .into_iter()
            .filter(|(i, _)| (start..end).contains(i))
            .collect();
        let has_text = kids[start..end]
            .iter()
            .any(|&c| doc.text(c).is_some_and(|t| !is_ws(t)));
        match self.content_type(new_decl) {
            ContentType::Any => true,
            ContentType::Empty => wrapped.is_empty() && !has_text,
            ContentType::TextOnly => wrapped.is_empty(),
            ct => {
                (ct == ContentType::Mixed || !has_text)
                    && self.run_model(new_decl, wrapped.iter().map(|w| w.1)).errors == 0
            }
        }
    }

    /// A minimal valid instance of `decl`: required attributes and the
    /// shortest valid sequence of required children, recursively (bounded
    /// depth; recursive types stop at the first repetition).
    pub fn required_skeleton(&self, decl: DeclId) -> Skeleton {
        let mut stack = Vec::new();
        self.skeleton(decl, &mut stack)
    }

    fn skeleton(&self, decl: DeclId, stack: &mut Vec<DeclId>) -> Skeleton {
        let ty = self.ty(decl);
        let attrs = ty
            .attrs
            .iter()
            .filter(|a| a.required)
            .map(|a| (a.name.clone(), a.initial_value()))
            .collect();
        let mut children = Vec::new();
        if stack.len() < MAX_SKELETON_DEPTH && !stack.contains(&decl) {
            stack.push(decl);
            for term in ty.model.shortest_word() {
                if let Term::Elem(d) = term {
                    children.push(self.skeleton(d, stack));
                }
            }
            stack.pop();
        }
        Skeleton {
            name: self.decl_name(decl).to_string(),
            attrs,
            children,
            wants_text: matches!(ty.content, ContentType::Mixed | ContentType::TextOnly),
        }
    }

    /// How well the current children of `node` fit `decl` (lower is better).
    fn content_rank(&self, doc: &Document, node: NodeId, decl: DeclId) -> (usize, bool) {
        let names = self.child_names(doc, node);
        let text = doc
            .children(node)
            .iter()
            .any(|&c| doc.text(c).is_some_and(|t| !is_ws(t)));
        match self.content_type(decl) {
            ContentType::Any => (0, false),
            ContentType::Empty => (names.len() + usize::from(text), false),
            ContentType::TextOnly => (names.len(), false),
            ct => {
                let r = self.run_model(decl, names.iter().map(|n| n.1)).rank();
                (
                    r.0 + usize::from(text && ct == ContentType::ElementOnly),
                    r.1,
                )
            }
        }
    }

    /// Names `node` could be renamed to ("change element") without making its
    /// parent's children or its own content less valid. Sorted; excludes the
    /// current name.
    pub fn valid_renames(&self, doc: &Document, node: NodeId) -> Vec<String> {
        let Some(el) = doc.element(node) else {
            return Vec::new();
        };
        let Some(parent) = doc.parent(node) else {
            return Vec::new();
        };
        let current = self.element_decl_for(doc, node);
        let current_rank = current
            .map(|d| self.content_rank(doc, node, d))
            .unwrap_or((usize::MAX, true));
        let candidates: Vec<DeclId> = if matches!(doc.kind(parent), NodeKind::Document) {
            self.root_candidates()
                .iter()
                .filter_map(|n| self.global_decl(n))
                .collect()
        } else {
            let Some(pdecl) = self.element_decl_for(doc, parent) else {
                return Vec::new();
            };
            let ty = self.ty(pdecl);
            match ty.content {
                ContentType::Empty | ContentType::TextOnly => return Vec::new(),
                ContentType::Any => self.wildcard_candidates(Term::Elem(pdecl)),
                _ => {
                    let names = self.child_names(doc, parent);
                    let Some(pos) = doc
                        .index_in_parent(node)
                        .and_then(|i| names.iter().position(|n| n.0 == i))
                    else {
                        return Vec::new();
                    };
                    let base = self.run_model(pdecl, names.iter().map(|n| n.1)).rank();
                    self.expand_terms(&ty.model.all_terms())
                        .into_iter()
                        .filter(|&d| {
                            let replaced = names.iter().enumerate().map(|(i, n)| {
                                if i == pos {
                                    self.decl_inst_name(d)
                                } else {
                                    n.1
                                }
                            });
                            self.run_model(pdecl, replaced).rank() <= base
                        })
                        .collect()
                }
            }
        };
        let mut out: Vec<String> = candidates
            .into_iter()
            .filter(|&d| self.content_rank(doc, node, d) <= current_rank)
            .map(|d| self.insertion_name(doc, node, d))
            .filter(|n| n != &el.name)
            .collect();
        out.sort();
        out.dedup();
        out
    }
}
