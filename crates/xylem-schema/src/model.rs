//! Content models: the particle tree produced by the loaders and the
//! position (Glushkov) automata compiled from it.
//!
//! A content model is compiled once per element type. Matching never assumes
//! determinism: the automaton is simulated over *sets* of positions, so
//! ambiguous DTD models and schemas that violate the Unique Particle
//! Attribution constraint still validate correctly.

use std::collections::HashMap;

use crate::bitset::BitSet;
use crate::{DeclId, Sym};

/// Bounded occurrence ranges are unfolded into copies; ranges above this cap
/// are approximated (`min` is clamped and `max` becomes unbounded).
pub(crate) const MAX_UNFOLD: u32 = 64;

/// Index of a wildcard (`xs:any`) in [`crate::Grammar`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct WildcardId(pub(crate) u32);

/// A leaf of a content model.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Term {
    Elem(DeclId),
    Any(WildcardId),
}

/// How a wildcard treats the elements it matches.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessContents {
    /// The element must have a global declaration and is validated.
    Strict,
    /// The element is validated if a global declaration exists.
    Lax,
    /// The element and its subtree are not validated.
    Skip,
}

/// The namespace constraint of a wildcard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NsConstraint {
    Any,
    /// `##other`: any namespace other than the target namespace (and not absent).
    Other(Option<String>),
    /// An explicit list; `None` stands for `##local`.
    List(Vec<Option<String>>),
}

#[derive(Clone, Debug)]
pub(crate) struct Wildcard {
    pub(crate) process: ProcessContents,
    pub(crate) ns: NsConstraint,
}

impl Wildcard {
    pub(crate) fn matches_ns(&self, ns: Option<&str>) -> bool {
        match &self.ns {
            NsConstraint::Any => true,
            NsConstraint::Other(target) => ns.is_some() && ns != target.as_deref(),
            NsConstraint::List(list) => list.iter().any(|n| n.as_deref() == ns),
        }
    }
}

/// Particle tree, as declared in the schema.
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum Particle {
    #[default]
    Empty,
    Term(Term),
    Seq(Vec<Particle>),
    Choice(Vec<Particle>),
    All(Vec<Particle>),
    Repeat(Box<Particle>, u32, Option<u32>),
}

impl Particle {
    /// Wraps `self` in an occurrence range, avoiding redundant wrappers.
    pub(crate) fn repeat(self, min: u32, max: Option<u32>) -> Particle {
        match (min, max) {
            (1, Some(1)) => self,
            (_, Some(0)) => Particle::Empty,
            _ => Particle::Repeat(Box::new(self), min, max),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        match self {
            Particle::Empty => true,
            Particle::Term(_) => false,
            Particle::Seq(v) | Particle::Choice(v) | Particle::All(v) => {
                v.iter().all(Particle::is_empty)
            }
            Particle::Repeat(p, _, _) => p.is_empty(),
        }
    }
}

/// An element name in an instance document, resolved against the grammar.
#[derive(Copy, Clone, Debug)]
pub(crate) struct InstName<'a> {
    /// The grammar symbol, if the grammar knows this expanded name at all.
    pub(crate) sym: Option<Sym>,
    pub(crate) ns: Option<&'a str>,
}

/// A compiled content model.
#[derive(Debug, Clone)]
pub(crate) enum Model {
    /// No child elements are possible.
    Empty,
    Nfa(Nfa),
    All(AllModel),
}

/// Matching state for a [`Model`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MState {
    Empty,
    Nfa { start: bool, set: BitSet },
    All(BitSet),
}

/// Glushkov position automaton.
#[derive(Debug, Clone)]
pub(crate) struct Nfa {
    terms: Vec<Term>,
    nullable: bool,
    first: BitSet,
    last: BitSet,
    follow: Vec<BitSet>,
    by_sym: HashMap<Sym, BitSet>,
    wild: BitSet,
}

/// Interleave model for a top-level `xs:all` group.
#[derive(Debug, Clone)]
pub(crate) struct AllModel {
    members: Vec<(Term, bool)>,
    /// `minOccurs="0"` on the group itself: the empty sequence is accepted.
    optional: bool,
}

/// Context needed to compile and run models: term names and wildcards.
pub(crate) trait TermInfo {
    fn term_sym(&self, decl: DeclId) -> Sym;
    fn wildcard(&self, id: WildcardId) -> &Wildcard;
}

impl Model {
    pub(crate) fn compile(particle: &Particle, info: &dyn TermInfo) -> Model {
        if particle.is_empty() {
            return Model::Empty;
        }
        if let Some(all) = AllModel::try_from_particle(particle) {
            return Model::All(all);
        }
        Model::Nfa(Nfa::build(particle, info))
    }

    pub(crate) fn has_elements(&self) -> bool {
        !matches!(self, Model::Empty)
    }

    pub(crate) fn start(&self) -> MState {
        match self {
            Model::Empty => MState::Empty,
            Model::Nfa(n) => MState::Nfa {
                start: true,
                set: BitSet::new(n.terms.len()),
            },
            Model::All(a) => MState::All(BitSet::new(a.members.len())),
        }
    }

    pub(crate) fn accepts(&self, s: &MState) -> bool {
        match (self, s) {
            (Model::Nfa(n), MState::Nfa { start, set }) => {
                (*start && n.nullable) || set.intersects(&n.last)
            }
            (Model::All(a), MState::All(seen)) => {
                (a.optional && seen.is_empty())
                    || a.members
                        .iter()
                        .enumerate()
                        .all(|(i, (_, req))| !req || seen.contains(i))
            }
            _ => true,
        }
    }

    /// Consumes one element; returns the new state and the matched term.
    pub(crate) fn step(
        &self,
        s: &MState,
        name: InstName<'_>,
        info: &dyn TermInfo,
    ) -> Option<(MState, Term)> {
        match (self, s) {
            (Model::Nfa(n), MState::Nfa { start, set }) => n.step(*start, set, name, info),
            (Model::All(a), MState::All(seen)) => {
                let i = a.find(name, info, |i| !seen.contains(i))?;
                let mut next = seen.clone();
                next.insert(i);
                Some((MState::All(next), a.members[i].0))
            }
            _ => None,
        }
    }

    /// Error recovery: jump to the positions labelled with `name`, wherever
    /// they are in the model. Returns `None` when the model never mentions it.
    pub(crate) fn resync(
        &self,
        s: &MState,
        name: InstName<'_>,
        info: &dyn TermInfo,
    ) -> Option<(MState, Term)> {
        match (self, s) {
            (Model::Nfa(n), _) => {
                let sym = name.sym?;
                let set = n.by_sym.get(&sym)?.clone();
                let term = n.terms[set.iter().next()?];
                Some((MState::Nfa { start: false, set }, term))
            }
            (Model::All(a), MState::All(seen)) => {
                let i = a.find(name, info, |_| true)?;
                let mut next = seen.clone();
                next.insert(i);
                Some((MState::All(next), a.members[i].0))
            }
            _ => None,
        }
    }

    /// Terms that may come next, in model order, without duplicates.
    pub(crate) fn allowed(&self, s: &MState) -> Vec<Term> {
        let mut out = Vec::new();
        let mut push = |t: Term| {
            if !out.contains(&t) {
                out.push(t);
            }
        };
        match (self, s) {
            (Model::Nfa(n), MState::Nfa { start, set }) => {
                for p in n.allowed_set(*start, set).iter() {
                    push(n.terms[p]);
                }
            }
            (Model::All(a), MState::All(seen)) => {
                for (i, (t, _)) in a.members.iter().enumerate() {
                    if !seen.contains(i) {
                        push(*t);
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// Every term that occurs anywhere in the model.
    pub(crate) fn all_terms(&self) -> Vec<Term> {
        let mut out: Vec<Term> = Vec::new();
        let terms: Box<dyn Iterator<Item = Term>> = match self {
            Model::Empty => Box::new(std::iter::empty()),
            Model::Nfa(n) => Box::new(n.terms.iter().copied()),
            Model::All(a) => Box::new(a.members.iter().map(|m| m.0)),
        };
        for t in terms {
            if !out.contains(&t) {
                out.push(t);
            }
        }
        out
    }

    /// The shortest accepted sequence of terms (wildcards included), preferring
    /// earlier alternatives on ties.
    pub(crate) fn shortest_word(&self) -> Vec<Term> {
        match self {
            Model::Empty => Vec::new(),
            Model::All(a) => {
                if a.optional {
                    Vec::new()
                } else {
                    a.members.iter().filter(|m| m.1).map(|m| m.0).collect()
                }
            }
            Model::Nfa(n) => n.shortest_word(),
        }
    }
}

impl AllModel {
    fn try_from_particle(p: &Particle) -> Option<AllModel> {
        let (inner, optional) = match p {
            Particle::Repeat(inner, min, Some(1)) => (&**inner, *min == 0),
            other => (other, false),
        };
        let Particle::All(children) = inner else {
            return None;
        };
        let mut members = Vec::new();
        for c in children {
            match c {
                Particle::Term(t) => members.push((*t, true)),
                Particle::Repeat(t, min, Some(1)) => match &**t {
                    Particle::Term(t) => members.push((*t, *min > 0)),
                    _ => return None,
                },
                Particle::Empty => {}
                _ => return None,
            }
        }
        Some(AllModel { members, optional })
    }

    fn find(
        &self,
        name: InstName<'_>,
        info: &dyn TermInfo,
        available: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        let mut wildcard = None;
        for (i, (t, _)) in self.members.iter().enumerate() {
            if !available(i) {
                continue;
            }
            match t {
                Term::Elem(d) if Some(info.term_sym(*d)) == name.sym => return Some(i),
                Term::Any(w) if wildcard.is_none() && info.wildcard(*w).matches_ns(name.ns) => {
                    wildcard = Some(i)
                }
                _ => {}
            }
        }
        wildcard
    }
}

/// Fragment summary used while building the position automaton.
struct Frag {
    nullable: bool,
    first: Vec<usize>,
    last: Vec<usize>,
}

struct Builder {
    terms: Vec<Term>,
    follow: Vec<Vec<usize>>,
}

impl Builder {
    fn link(&mut self, from: &[usize], to: &[usize]) {
        for &x in from {
            self.follow[x].extend_from_slice(to);
        }
    }

    fn build(&mut self, p: &Particle) -> Frag {
        match p {
            Particle::Empty => Frag {
                nullable: true,
                first: vec![],
                last: vec![],
            },
            Particle::Term(t) => {
                let pos = self.terms.len();
                self.terms.push(*t);
                self.follow.push(Vec::new());
                Frag {
                    nullable: false,
                    first: vec![pos],
                    last: vec![pos],
                }
            }
            Particle::Seq(items) => {
                let mut acc = Frag {
                    nullable: true,
                    first: vec![],
                    last: vec![],
                };
                for item in items {
                    let f = self.build(item);
                    acc = self.concat(acc, f);
                }
                acc
            }
            Particle::Choice(items) => {
                if items.is_empty() {
                    return Frag {
                        nullable: true,
                        first: vec![],
                        last: vec![],
                    };
                }
                let mut acc = Frag {
                    nullable: false,
                    first: vec![],
                    last: vec![],
                };
                for item in items {
                    let f = self.build(item);
                    acc.nullable |= f.nullable;
                    acc.first.extend(f.first);
                    acc.last.extend(f.last);
                }
                acc
            }
            // `all` nested inside other groups is not XSD 1.0; approximate it
            // as a repeated choice so documents still match.
            Particle::All(items) => {
                let rep = Particle::Repeat(Box::new(Particle::Choice(items.clone())), 0, None);
                self.build(&rep)
            }
            Particle::Repeat(inner, min, max) => self.build_repeat(inner, *min, *max),
        }
    }

    fn concat(&mut self, a: Frag, b: Frag) -> Frag {
        self.link(&a.last, &b.first);
        let mut first = a.first;
        if a.nullable {
            first.extend_from_slice(&b.first);
        }
        let mut last = b.last;
        if b.nullable {
            last.extend(a.last);
        }
        Frag {
            nullable: a.nullable && b.nullable,
            first,
            last,
        }
    }

    fn build_repeat(&mut self, inner: &Particle, min: u32, max: Option<u32>) -> Frag {
        let min_eff = min.min(MAX_UNFOLD);
        let max_eff = match max {
            Some(m) if m > MAX_UNFOLD => None,
            m => m,
        };
        let mut acc = Frag {
            nullable: true,
            first: vec![],
            last: vec![],
        };
        for i in 0..min_eff {
            let f = self.build(inner);
            if max_eff.is_none() && i + 1 == min_eff {
                // The last mandatory copy loops on itself: p{n,} = p{n-1} p+.
                self.link(&f.last.clone(), &f.first.clone());
            }
            acc = self.concat(acc, f);
        }
        match max_eff {
            None if min_eff == 0 => {
                let mut f = self.build(inner);
                self.link(&f.last.clone(), &f.first.clone());
                f.nullable = true;
                acc = self.concat(acc, f);
            }
            None => {}
            Some(m) => {
                for _ in min_eff..m {
                    let mut f = self.build(inner);
                    f.nullable = true;
                    acc = self.concat(acc, f);
                }
            }
        }
        acc
    }
}

impl Nfa {
    fn build(p: &Particle, info: &dyn TermInfo) -> Nfa {
        let mut b = Builder {
            terms: Vec::new(),
            follow: Vec::new(),
        };
        let frag = b.build(p);
        let n = b.terms.len();
        let to_set = |v: &[usize]| {
            let mut s = BitSet::new(n);
            v.iter().for_each(|&x| s.insert(x));
            s
        };
        let mut by_sym: HashMap<Sym, BitSet> = HashMap::new();
        let mut wild = BitSet::new(n);
        for (i, t) in b.terms.iter().enumerate() {
            match t {
                Term::Elem(d) => by_sym
                    .entry(info.term_sym(*d))
                    .or_insert_with(|| BitSet::new(n))
                    .insert(i),
                Term::Any(_) => wild.insert(i),
            }
        }
        Nfa {
            nullable: frag.nullable,
            first: to_set(&frag.first),
            last: to_set(&frag.last),
            follow: b.follow.iter().map(|f| to_set(f)).collect(),
            terms: b.terms,
            by_sym,
            wild,
        }
    }

    fn allowed_set(&self, start: bool, set: &BitSet) -> BitSet {
        let mut allowed = if start {
            self.first.clone()
        } else {
            BitSet::new(self.terms.len())
        };
        for p in set.iter() {
            allowed.union_with(&self.follow[p]);
        }
        allowed
    }

    fn step(
        &self,
        start: bool,
        set: &BitSet,
        name: InstName<'_>,
        info: &dyn TermInfo,
    ) -> Option<(MState, Term)> {
        let allowed = self.allowed_set(start, set);
        let mut next = BitSet::new(self.terms.len());
        if let Some(by) = name.sym.and_then(|s| self.by_sym.get(&s)) {
            next.union_with(by);
            next.intersect_with(&allowed);
        }
        // Element declarations take precedence over wildcards.
        let first_elem = next.iter().next();
        let term = match first_elem {
            Some(p) => self.terms[p],
            None => {
                let mut w = self.wild.clone();
                w.intersect_with(&allowed);
                for p in w.iter() {
                    if let Term::Any(id) = self.terms[p]
                        && info.wildcard(id).matches_ns(name.ns)
                    {
                        next.insert(p);
                    }
                }
                self.terms[next.iter().next()?]
            }
        };
        Some((
            MState::Nfa {
                start: false,
                set: next,
            },
            term,
        ))
    }

    fn shortest_word(&self) -> Vec<Term> {
        if self.nullable {
            return Vec::new();
        }
        let n = self.terms.len();
        let mut prev: Vec<Option<usize>> = vec![None; n];
        let mut seen = BitSet::new(n);
        let mut queue = std::collections::VecDeque::new();
        for p in self.first.iter() {
            seen.insert(p);
            queue.push_back(p);
        }
        while let Some(p) = queue.pop_front() {
            if self.last.contains(p) {
                let mut word = vec![self.terms[p]];
                let mut cur = p;
                while let Some(q) = prev[cur] {
                    word.push(self.terms[q]);
                    cur = q;
                }
                word.reverse();
                return word;
            }
            for q in self.follow[p].iter() {
                if !seen.contains(q) {
                    seen.insert(q);
                    prev[q] = Some(p);
                    queue.push_back(q);
                }
            }
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terms are declaration ids whose symbol equals the id.
    struct Info(Vec<Wildcard>);
    impl TermInfo for Info {
        fn term_sym(&self, decl: DeclId) -> Sym {
            Sym(decl.0)
        }
        fn wildcard(&self, id: WildcardId) -> &Wildcard {
            &self.0[id.0 as usize]
        }
    }

    fn t(i: u32) -> Particle {
        Particle::Term(Term::Elem(DeclId(i)))
    }

    fn name(i: u32) -> InstName<'static> {
        InstName {
            sym: Some(Sym(i)),
            ns: None,
        }
    }

    fn accepts(m: &Model, word: &[u32]) -> bool {
        let info = Info(vec![]);
        let mut s = m.start();
        for &w in word {
            match m.step(&s, name(w), &info) {
                Some((n, _)) => s = n,
                None => return false,
            }
        }
        m.accepts(&s)
    }

    #[test]
    fn dtd_style_regex() {
        // (a, (b | c)*, d?)+
        let p = Particle::Seq(vec![
            t(0),
            Particle::Choice(vec![t(1), t(2)]).repeat(0, None),
            t(3).repeat(0, Some(1)),
        ])
        .repeat(1, None);
        let m = Model::compile(&p, &Info(vec![]));
        assert!(accepts(&m, &[0]));
        assert!(accepts(&m, &[0, 1, 2, 1, 3, 0, 3]));
        assert!(!accepts(&m, &[]));
        assert!(!accepts(&m, &[1]));
        assert!(!accepts(&m, &[0, 3, 3]));
    }

    #[test]
    fn bounded_occurrences() {
        let m = Model::compile(&t(0).repeat(2, Some(4)), &Info(vec![]));
        assert!(!accepts(&m, &[0]));
        assert!(accepts(&m, &[0, 0]));
        assert!(accepts(&m, &[0, 0, 0, 0]));
        assert!(!accepts(&m, &[0, 0, 0, 0, 0]));
        let m = Model::compile(&t(0).repeat(3, None), &Info(vec![]));
        assert!(!accepts(&m, &[0, 0]));
        assert!(accepts(&m, &[0, 0, 0, 0, 0, 0]));
    }

    #[test]
    fn ambiguous_models_are_simulated() {
        // (a, b) | (a, c): not deterministic, still matched correctly.
        let p = Particle::Choice(vec![
            Particle::Seq(vec![t(0), t(1)]),
            Particle::Seq(vec![t(0), t(2)]),
        ]);
        let m = Model::compile(&p, &Info(vec![]));
        assert!(accepts(&m, &[0, 1]));
        assert!(accepts(&m, &[0, 2]));
        assert!(!accepts(&m, &[0]));
    }

    #[test]
    fn all_group_is_interleave() {
        let p = Particle::All(vec![t(0), t(1).repeat(0, Some(1)), t(2)]);
        let m = Model::compile(&p, &Info(vec![]));
        assert!(matches!(m, Model::All(_)));
        assert!(accepts(&m, &[2, 0]));
        assert!(accepts(&m, &[0, 1, 2]));
        assert!(accepts(&m, &[1, 2, 0]));
        assert!(!accepts(&m, &[0, 0, 2]));
        assert!(!accepts(&m, &[0]));
    }

    #[test]
    fn wildcard_matches_unknown_names() {
        let info = Info(vec![Wildcard {
            process: ProcessContents::Lax,
            ns: NsConstraint::Any,
        }]);
        let p = Particle::Seq(vec![
            t(0),
            Particle::Term(Term::Any(WildcardId(0))).repeat(0, None),
        ]);
        let m = Model::compile(&p, &info);
        let (s, _) = m.step(&m.start(), name(0), &info).unwrap();
        let (s, term) = m
            .step(
                &s,
                InstName {
                    sym: None,
                    ns: Some("urn:x"),
                },
                &info,
            )
            .unwrap();
        assert_eq!(term, Term::Any(WildcardId(0)));
        assert!(m.accepts(&s));
    }

    #[test]
    fn shortest_word_prefers_first_alternative() {
        let p = Particle::Seq(vec![
            t(0).repeat(0, Some(1)),
            Particle::Choice(vec![t(1), t(2)]),
            t(3).repeat(2, None),
        ]);
        let m = Model::compile(&p, &Info(vec![]));
        let w: Vec<_> = m.shortest_word();
        assert_eq!(
            w,
            vec![
                Term::Elem(DeclId(1)),
                Term::Elem(DeclId(3)),
                Term::Elem(DeclId(3))
            ]
        );
    }

    #[test]
    fn allowed_and_resync() {
        let p = Particle::Seq(vec![t(0).repeat(0, Some(1)), t(1).repeat(1, None)]);
        let info = Info(vec![]);
        let m = Model::compile(&p, &info);
        let s = m.start();
        assert_eq!(
            m.allowed(&s),
            vec![Term::Elem(DeclId(0)), Term::Elem(DeclId(1))]
        );
        let (s, _) = m.step(&s, name(1), &info).unwrap();
        assert!(m.step(&s, name(0), &info).is_none());
        let (s, term) = m.resync(&s, name(0), &info).unwrap();
        assert_eq!(term, Term::Elem(DeclId(0)));
        assert!(!m.accepts(&s));
    }
}
