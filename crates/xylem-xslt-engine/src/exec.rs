//! Running a compiled stylesheet: focus and variables, template rules,
//! instructions and the result-tree builder.

use crate::compile::{Avt, AvtPart, Instr, ModeRef, Number, Param, Sort, Stylesheet, Template};
use crate::eval::R;
use crate::tree::{self, Kind, NodeRef, QName, TNode, Tree};
use crate::value::{Atomic, Item, Seq};
use crate::xpath::{Axis, Expr, PathStep};
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// A linked list of variable bindings; cloning is cheap.
#[derive(Clone, Default)]
pub struct Env(Option<Rc<(QName, Rc<Seq>, Env)>>);

type Tunnel = Rc<HashMap<QName, Rc<Seq>>>;
/// Template rules of one mode, best first: (template, pattern alternative).
type RuleList = Vec<(usize, usize)>;
/// One key's index over one tree: key value to nodes in document order.
type KeyIndex = HashMap<String, Vec<NodeRef>>;

/// The dynamic context of an expression or instruction.
#[derive(Clone)]
pub struct Dyn {
    pub item: Option<Item>,
    pub pos: usize,
    pub size: usize,
    pub env: Env,
    /// `current()`: the item the innermost instruction is processing.
    pub current: Option<Item>,
    pub mode: Option<QName>,
    pub tunnel: Tunnel,
}

impl Dyn {
    pub fn item(&self) -> R<&Item> {
        self.item
            .as_ref()
            .ok_or_else(|| "the context item is undefined".to_string())
    }
    pub fn focus(&self, item: Item, pos: usize, size: usize) -> Dyn {
        Dyn {
            item: Some(item),
            pos,
            size,
            ..self.clone()
        }
    }
    pub fn bind(&self, name: QName, v: Seq) -> Dyn {
        let mut d = self.clone();
        d.env = Env(Some(Rc::new((name, Rc::new(v), self.env.clone()))));
        d
    }
    fn lookup(&self, n: &QName) -> Option<Rc<Seq>> {
        let mut cur = &self.env;
        while let Some(b) = &cur.0 {
            if b.0.same(n) {
                return Some(b.1.clone());
            }
            cur = &b.2;
        }
        None
    }
}

/// Calls and time of one template or function, for "Time templates".
///
/// Own time excludes the templates and functions it called; total time
/// includes them, counting a recursive call once (the outermost).
#[derive(Default, Clone, Debug)]
pub struct TemplateTime {
    pub calls: u64,
    pub own: Duration,
    pub total: Duration,
}

/// Timing state: one entry per template, then one per function.
pub struct Profiler {
    pub times: Vec<TemplateTime>,
    /// Running calls: (entry, started, time spent in calls it made).
    stack: Vec<(usize, Instant, Duration)>,
}

impl Profiler {
    fn enter(&mut self, id: usize) {
        self.stack.push((id, Instant::now(), Duration::ZERO));
    }
    fn leave(&mut self) {
        let Some((id, started, inner)) = self.stack.pop() else {
            return;
        };
        let spent = started.elapsed();
        let t = &mut self.times[id];
        t.calls += 1;
        t.own += spent.saturating_sub(inner);
        if !self.stack.iter().any(|(i, _, _)| *i == id) {
            t.total += spent;
        }
        if let Some(parent) = self.stack.last_mut() {
            parent.2 += spent;
        }
    }
}

pub struct Engine<'s> {
    pub ss: &'s Stylesheet,
    trees: RefCell<Vec<Rc<Tree>>>,
    globals: RefCell<HashMap<QName, Rc<Seq>>>,
    evaluating: RefCell<Vec<QName>>,
    params: HashMap<String, String>,
    rules: RefCell<HashMap<Option<QName>, Rc<RuleList>>>,
    keys: RefCell<HashMap<(QName, u32), Rc<KeyIndex>>>,
    docs: RefCell<HashMap<String, u32>>,
    pub messages: RefCell<Vec<String>>,
    /// Variables visible to patterns: those in scope at an `xsl:number`
    /// whose `count` / `from` patterns are being matched.
    pattern_env: RefCell<Env>,
    pub timing: Option<RefCell<Profiler>>,
    source: u32,
}

impl<'s> Engine<'s> {
    pub fn new(
        ss: &'s Stylesheet,
        source: Tree,
        params: HashMap<String, String>,
        time: bool,
    ) -> Engine<'s> {
        Engine {
            ss,
            trees: RefCell::new(vec![Rc::new(source)]),
            globals: RefCell::default(),
            evaluating: RefCell::default(),
            params: params.into_iter().collect(),
            rules: RefCell::default(),
            keys: RefCell::default(),
            docs: RefCell::default(),
            messages: RefCell::default(),
            pattern_env: RefCell::default(),
            timing: time.then(|| {
                RefCell::new(Profiler {
                    times: vec![TemplateTime::default(); ss.templates.len() + ss.functions.len()],
                    stack: Vec::new(),
                })
            }),
            source: 0,
        }
    }

    pub fn tree(&self, i: u32) -> Ref<'_, Tree> {
        Ref::map(self.trees.borrow(), |t| &*t[i as usize])
    }
    pub fn tree_rc(&self, i: u32) -> Rc<Tree> {
        self.trees.borrow()[i as usize].clone()
    }
    pub fn add_tree(&self, t: Tree) -> u32 {
        let mut v = self.trees.borrow_mut();
        v.push(Rc::new(t));
        (v.len() - 1) as u32
    }

    fn root_dyn(&self) -> Dyn {
        let item = Item::Node(NodeRef {
            tree: self.source,
            idx: 0,
        });
        Dyn {
            item: Some(item.clone()),
            pos: 1,
            size: 1,
            env: Env::default(),
            current: Some(item),
            mode: None,
            tunnel: Rc::default(),
        }
    }

    /// Runs the transformation from the source document node and returns
    /// the principal result tree.
    pub fn run(&self) -> R<Tree> {
        let d = self.root_dyn();
        let mut out = Out::document();
        let root = vec![Item::Node(NodeRef {
            tree: self.source,
            idx: 0,
        })];
        self.apply(&root, &d, None, &[], &mut out)?;
        Ok(out.finish_tree())
    }

    /// Runs the transformation from a named template (`initial-template`),
    /// with the source document, if any, as the context item.
    pub fn run_named(&self, name: &QName, with_context: bool) -> R<Tree> {
        let t = self
            .ss
            .named
            .iter()
            .find(|(n, _)| n.same(name))
            .map(|(_, &t)| t)
            .ok_or_else(|| format!("no template named '{}'", name.lexical()))?;
        let mut d = self.root_dyn();
        if !with_context {
            d.item = None;
            d.current = None;
        }
        let mut out = Out::document();
        self.invoke(t, &d, &[], &mut out)?;
        Ok(out.finish_tree())
    }

    /// Runs the transformation from the source document in `mode`.
    pub fn run_mode(&self, mode: &QName) -> R<Tree> {
        let d = self.root_dyn();
        let mut out = Out::document();
        let root = vec![Item::Node(NodeRef {
            tree: self.source,
            idx: 0,
        })];
        self.apply(&root, &d, Some(mode.clone()), &[], &mut out)?;
        Ok(out.finish_tree())
    }

    /// The dynamic context for evaluating a standalone expression: the
    /// source document as context item (or none), with `vars` bound.
    pub fn standalone_dyn(&self, with_context: bool, vars: Vec<(QName, Seq)>) -> Dyn {
        let mut d = self.root_dyn();
        if !with_context {
            d.item = None;
            d.current = None;
        }
        for (n, v) in vars {
            d = d.bind(n, v);
        }
        d
    }

    pub fn var(&self, n: &QName, d: &Dyn) -> R<Seq> {
        if let Some(v) = d.lookup(n) {
            return Ok((*v).clone());
        }
        if let Some(v) = self.globals.borrow().get(n) {
            return Ok((**v).clone());
        }
        let Some((p, is_param)) = self.ss.globals.iter().find(|(g, _)| g.name.same(n)) else {
            return Err(format!("variable ${} is not declared", n.lexical()));
        };
        if self.evaluating.borrow().iter().any(|e| e.same(n)) {
            return Err(format!(
                "global variable ${} depends on itself",
                n.lexical()
            ));
        }
        self.evaluating.borrow_mut().push(n.clone());
        let supplied = is_param.then(|| self.params.get(&*n.local)).flatten();
        let v = match supplied {
            // Spec note 2: the host passes a value as the declared atomic
            // type (boolean, integer, decimal, double) or else as a string,
            // under exactly the name given.
            Some(s) => {
                let v = vec![Item::Atomic(Atomic::str(s.as_str()))];
                let numeric = |t: &crate::xpath::SeqType| {
                    matches!(&t.item, crate::xpath::ItemType::Atomic(a)
                        if matches!(a.as_str(), "boolean" | "integer" | "decimal" | "double" | "float"))
                };
                match &p.as_ {
                    Some(t) if numeric(t) => {
                        let v = vec![Item::Atomic(Atomic::Untyped(s.as_str().into()))];
                        self.convert(v, t, &format!("parameter ${}", n.lexical()))?
                    }
                    _ => v,
                }
            }
            None => self.binding(p, &self.root_dyn())?,
        };
        self.evaluating.borrow_mut().pop();
        self.globals
            .borrow_mut()
            .insert(n.clone(), Rc::new(v.clone()));
        Ok(v)
    }

    /// The value of a variable, parameter default or with-param.
    fn binding(&self, p: &Param, d: &Dyn) -> R<Seq> {
        let what = || format!("${}", p.name.lexical());
        let v = if let Some(s) = &p.select {
            self.eval(s, d)?
        } else if p.body.is_empty() {
            if p.as_.is_some() {
                vec![]
            } else {
                vec![Item::Atomic(Atomic::str(""))]
            }
        } else if p.as_.is_some() {
            let mut out = Out::sequence();
            self.body(&p.body, d, &mut out)?;
            out.finish_seq(self)
        } else {
            let mut out = Out::document();
            self.body(&p.body, d, &mut out)?;
            let t = self.add_tree(out.finish_tree());
            vec![Item::Node(NodeRef { tree: t, idx: 0 })]
        };
        match &p.as_ {
            Some(t) => self.convert(v, t, &what()),
            None => Ok(v),
        }
    }

    pub fn avt(&self, a: &Avt, d: &Dyn) -> R<String> {
        match a {
            Avt::Lit(s) => Ok(s.clone()),
            Avt::Parts(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        AvtPart::Lit(l) => s.push_str(l),
                        AvtPart::Expr(e) => s.push_str(&self.joined(&self.eval(e, d)?, " ")),
                    }
                }
                Ok(s)
            }
        }
    }

    // ---- template rules -------------------------------------------------

    fn rules_for(&self, mode: &Option<QName>) -> Rc<Vec<(usize, usize)>> {
        if let Some(r) = self.rules.borrow().get(mode) {
            return r.clone();
        }
        let mut v: Vec<(usize, usize)> = Vec::new();
        for t in &self.ss.templates {
            let in_mode = t.all_modes
                || t.modes.iter().any(|m| match (m, mode) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same(b),
                    _ => false,
                });
            if in_mode {
                for k in 0..t.matches.len() {
                    v.push((t.id, k));
                }
            }
        }
        // Highest precedence, then priority, then last in the stylesheet.
        let key = |&(t, k): &(usize, usize)| {
            let tt: &Template = &self.ss.templates[t];
            (tt.prec, tt.matches[k].1, t)
        };
        v.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap());
        let r = Rc::new(v);
        self.rules.borrow_mut().insert(mode.clone(), r.clone());
        r
    }

    fn find_rule(&self, n: NodeRef, mode: &Option<QName>) -> R<Option<usize>> {
        for &(t, k) in self.rules_for(mode).iter() {
            if self.matches(n, &self.ss.templates[t].matches[k].0)? {
                return Ok(Some(t));
            }
        }
        Ok(None)
    }

    /// Whether node `n` matches one pattern alternative.
    pub fn matches(&self, n: NodeRef, p: &Expr) -> R<bool> {
        if !self.quick_match(n, p) {
            return Ok(false);
        }
        let d = Dyn {
            item: None,
            pos: 1,
            size: 1,
            env: self.pattern_env.borrow().clone(),
            ..self.root_dyn()
        };
        let holds = |ctx: NodeRef| -> R<bool> {
            let v = self.eval(p, &d.focus(Item::Node(ctx), 1, 1))?;
            Ok(v.contains(&Item::Node(n)))
        };
        match p {
            Expr::Root => Ok(self.tree(n.tree).node(n.idx).kind == Kind::Document),
            Expr::Path { start: Some(_), .. } | Expr::Call(..) => holds(NodeRef {
                tree: n.tree,
                idx: 0,
            }),
            _ => {
                // A relative pattern matches if some ancestor reaches `n`.
                let mut cur = self.tree(n.tree).node(n.idx).parent;
                while let Some(a) = cur {
                    if holds(NodeRef {
                        tree: n.tree,
                        idx: a,
                    })? {
                        return Ok(true);
                    }
                    cur = self.tree(n.tree).node(a).parent;
                }
                Ok(false)
            }
        }
    }

    /// A cheap test of the pattern's last step before evaluating it.
    fn quick_match(&self, n: NodeRef, p: &Expr) -> bool {
        let Expr::Path { steps, .. } = p else {
            return true;
        };
        let Some(PathStep::Axis(s)) = steps.last() else {
            return true;
        };
        let t = self.tree(n.tree);
        let node = t.node(n.idx);
        let principal = if s.axis == Axis::Attribute {
            Kind::Attribute
        } else {
            Kind::Element
        };
        if s.axis == Axis::Attribute && node.kind != Kind::Attribute {
            return false;
        }
        if s.axis == Axis::Child && node.kind == Kind::Attribute {
            return false;
        }
        crate::eval::test(node, &s.test, principal)
    }

    fn apply(
        &self,
        items: &Seq,
        d: &Dyn,
        mode: Option<QName>,
        params: &[(QName, Rc<Seq>, bool)],
        out: &mut Out,
    ) -> R<()> {
        let size = items.len();
        for (k, it) in items.iter().enumerate() {
            let Item::Node(n) = it else {
                return Err("xsl:apply-templates selected an atomic value".into());
            };
            let mut nd = d.focus(it.clone(), k + 1, size);
            nd.current = Some(it.clone());
            nd.mode = mode.clone();
            match self.find_rule(*n, &mode)? {
                Some(t) => self.invoke(t, &nd, params, out)?,
                None => self.builtin(*n, &nd, params, out)?,
            }
        }
        Ok(())
    }

    fn builtin(
        &self,
        n: NodeRef,
        d: &Dyn,
        params: &[(QName, Rc<Seq>, bool)],
        out: &mut Out,
    ) -> R<()> {
        let kind = self.tree(n.tree).node(n.idx).kind;
        match kind {
            Kind::Document | Kind::Element => {
                let kids: Seq = self
                    .tree(n.tree)
                    .node(n.idx)
                    .children
                    .iter()
                    .map(|&c| {
                        Item::Node(NodeRef {
                            tree: n.tree,
                            idx: c,
                        })
                    })
                    .collect();
                self.apply(&kids, d, d.mode.clone(), params, out)
            }
            Kind::Text | Kind::Attribute => {
                let s = self.tree(n.tree).node(n.idx).value.clone();
                out.text(&s);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Calls template `t` with `params` (name, value, tunnel).
    fn invoke(&self, t: usize, d: &Dyn, params: &[(QName, Rc<Seq>, bool)], out: &mut Out) -> R<()> {
        if let Some(p) = &self.timing {
            p.borrow_mut().enter(t);
        }
        let tpl = &self.ss.templates[t];
        let mut tunnel: HashMap<QName, Rc<Seq>> = (*d.tunnel).clone();
        for (n, v, tun) in params {
            if *tun {
                tunnel.insert(n.clone(), v.clone());
            }
        }
        let mut nd = d.clone();
        nd.env = Env::default();
        nd.tunnel = Rc::new(tunnel);
        for p in &tpl.params {
            let supplied = if p.tunnel {
                nd.tunnel.get(&p.name).cloned()
            } else {
                params
                    .iter()
                    .find(|(n, _, tun)| !*tun && n.same(&p.name))
                    .map(|(_, v, _)| v.clone())
            };
            let v = match supplied {
                Some(v) => match &p.as_ {
                    Some(ty) => self.convert(
                        (*v).clone(),
                        ty,
                        &format!("parameter ${}", p.name.lexical()),
                    )?,
                    None => (*v).clone(),
                },
                None => self.binding(p, &nd)?,
            };
            nd = nd.bind(p.name.clone(), v);
        }
        let r = self.body(&tpl.body, &nd, out);
        if let Some(p) = &self.timing {
            p.borrow_mut().leave();
        }
        r.map_err(|e| format!("{e}\n  in template {}", tpl.location))
    }

    fn with_params(&self, ps: &[Param], d: &Dyn) -> R<Vec<(QName, Rc<Seq>, bool)>> {
        ps.iter()
            .map(|p| Ok((p.name.clone(), Rc::new(self.binding(p, d)?), p.tunnel)))
            .collect()
    }

    /// Calls a stylesheet function.
    pub fn call_user(&self, name: &QName, args: Vec<Seq>) -> R<Option<Seq>> {
        let Some(f) = self
            .ss
            .functions
            .iter()
            .find(|((n, a), _)| n.same(name) && *a == args.len())
            .map(|(_, f)| f)
        else {
            return Ok(None);
        };
        let mut d = Dyn {
            item: None,
            current: None,
            ..self.root_dyn()
        };
        for (p, v) in f.params.iter().zip(args) {
            let v = match &p.as_ {
                Some(t) => self.convert(
                    v,
                    t,
                    &format!("argument ${} of {}()", p.name.lexical(), f.name.lexical()),
                )?,
                None => v,
            };
            d = d.bind(p.name.clone(), v);
        }
        let mut out = Out::sequence();
        if let Some(p) = &self.timing {
            p.borrow_mut().enter(self.ss.templates.len() + f.id);
        }
        let r = self.body(&f.body, &d, &mut out);
        if let Some(p) = &self.timing {
            p.borrow_mut().leave();
        }
        r.map_err(|e| format!("{e}\n  in function {}()", f.name.lexical()))?;
        let v = out.finish_seq(self);
        Ok(Some(match &f.as_ {
            Some(t) => self.convert(v, t, &format!("result of {}()", f.name.lexical()))?,
            None => v,
        }))
    }

    // ---- keys and documents --------------------------------------------

    pub fn key(&self, name: &QName, values: &[Atomic], tree: u32) -> R<Vec<NodeRef>> {
        let id = (name.clone(), tree);
        let index = self.keys.borrow().get(&id).cloned();
        let index = match index {
            Some(i) => i,
            None => {
                let defs = self
                    .ss
                    .keys
                    .iter()
                    .find(|(n, _)| n.same(name))
                    .map(|(_, k)| k);
                let defs =
                    defs.ok_or_else(|| format!("key '{}' is not declared", name.lexical()))?;
                let mut map: HashMap<String, Vec<NodeRef>> = HashMap::new();
                let count = self.tree(tree).nodes.len() as u32;
                let d = self.root_dyn();
                for idx in 0..count {
                    let n = NodeRef { tree, idx };
                    for k in defs {
                        let mut hit = false;
                        for m in &k.matches {
                            if self.matches(n, m)? {
                                hit = true;
                                break;
                            }
                        }
                        if hit {
                            let v = self.eval(&k.use_, &d.focus(Item::Node(n), 1, 1))?;
                            for a in self.atomize(&v) {
                                let e = map.entry(a.to_string()).or_default();
                                if e.last() != Some(&n) {
                                    e.push(n);
                                }
                            }
                        }
                    }
                }
                let i = Rc::new(map);
                self.keys.borrow_mut().insert(id, i.clone());
                i
            }
        };
        let mut out: Vec<NodeRef> = values
            .iter()
            .flat_map(|v| index.get(&v.to_string()).cloned().unwrap_or_default())
            .collect();
        out.sort();
        out.dedup();
        Ok(out)
    }

    pub fn document(&self, href: &str, base: &str) -> R<Option<u32>> {
        if !crate::value::file_access() {
            return Err(format!("document('{href}'): file access is turned off"));
        }
        let base = std::path::Path::new(base);
        let dir = if base.is_dir() {
            base.to_path_buf()
        } else {
            base.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        };
        let href = href.strip_prefix("file:///").unwrap_or(href);
        let path = dir.join(href);
        let key = path.to_string_lossy().to_string();
        if let Some(&t) = self.docs.borrow().get(&key) {
            return Ok(Some(t));
        }
        let Ok(bytes) = std::fs::read(&path) else {
            return Ok(None);
        };
        let doc = xylem_dom::Document::from_bytes(&bytes).map_err(|e| format!("{key}: {e}"))?;
        let t = tree::from_dom(&doc, &key, self.ss)?;
        let i = self.add_tree(t);
        self.docs.borrow_mut().insert(key, i);
        Ok(Some(i))
    }

    pub fn base_of(&self, n: NodeRef) -> String {
        self.tree(n.tree).base.clone()
    }

    // ---- instructions ---------------------------------------------------

    pub fn body(&self, body: &[Instr], d: &Dyn, out: &mut Out) -> R<()> {
        let mut d = d.clone();
        for i in body {
            if let Instr::Variable(p) = i {
                let v = self.binding(p, &d)?;
                d = d.bind(p.name.clone(), v);
                continue;
            }
            self.instr(i, &d, out)?;
        }
        Ok(())
    }

    fn instr(&self, i: &Instr, d: &Dyn, out: &mut Out) -> R<()> {
        match i {
            Instr::Variable(_) => unreachable!(),
            Instr::Text(s) => out.text(s),
            Instr::Lre {
                name,
                ns,
                attrs,
                sets,
                body,
            } => {
                out.start(name.clone(), ns.clone());
                self.attr_sets(sets, d, out)?;
                for (n, v) in attrs {
                    let v = self.avt(v, d)?;
                    out.attr(n.clone(), v)?;
                }
                self.body(body, d, out)?;
                out.end(self);
            }
            Instr::ValueOf { select, sep, body } => {
                let s = match select {
                    Some(e) => {
                        let sep = match sep {
                            Some(a) => self.avt(a, d)?,
                            None => " ".into(),
                        };
                        let v = self.eval(e, d)?;
                        if self.ss.version.starts_with('1') {
                            v.first().map(|i| self.string_of(i)).unwrap_or_default()
                        } else {
                            self.joined(&v, &sep)
                        }
                    }
                    None => self.content_string(body, d, sep.as_ref())?,
                };
                out.text(&s);
            }
            Instr::ApplyTemplates {
                select,
                mode,
                params,
                sorts,
            } => {
                let items = match select {
                    Some(e) => self.eval(e, d)?,
                    None => {
                        let Item::Node(n) = d.item()? else {
                            return Err("xsl:apply-templates without select needs a node".into());
                        };
                        let t = self.tree(n.tree);
                        t.node(n.idx)
                            .children
                            .iter()
                            .map(|&c| {
                                Item::Node(NodeRef {
                                    tree: n.tree,
                                    idx: c,
                                })
                            })
                            .collect()
                    }
                };
                let items = self.sort(items, sorts, d)?;
                let mode = match mode {
                    ModeRef::Default => None,
                    ModeRef::Current => d.mode.clone(),
                    ModeRef::Named(m) => Some(m.clone()),
                };
                let ps = self.with_params(params, d)?;
                self.apply(&items, d, mode, &ps, out)?;
            }
            Instr::CallTemplate { name, params } => {
                let t = self
                    .ss
                    .named
                    .iter()
                    .find(|(n, _)| n.same(name))
                    .map(|(_, &t)| t);
                let t = t.ok_or_else(|| format!("no template named '{}'", name.lexical()))?;
                let ps = self.with_params(params, d)?;
                self.invoke(t, d, &ps, out)?;
            }
            Instr::Choose(whens, other) => {
                for (test, body) in whens {
                    if self.ebv(&self.eval(test, d)?)? {
                        return self.body(body, d, out);
                    }
                }
                self.body(other, d, out)?;
            }
            Instr::If(test, body) => {
                if self.ebv(&self.eval(test, d)?)? {
                    self.body(body, d, out)?;
                }
            }
            Instr::ForEach {
                select,
                sorts,
                body,
            } => {
                let items = self.sort(self.eval(select, d)?, sorts, d)?;
                let size = items.len();
                for (k, it) in items.into_iter().enumerate() {
                    let mut nd = d.focus(it.clone(), k + 1, size);
                    nd.current = Some(it);
                    self.body(body, &nd, out)?;
                }
            }
            Instr::Attribute {
                name,
                namespace,
                scope,
                select,
                sep,
                body,
            } => {
                let lexical = self.avt(name, d)?;
                let q = self.computed_name(&lexical, namespace.as_ref(), scope, d, false)?;
                let v = match select {
                    Some(e) => {
                        let sep = match sep {
                            Some(a) => self.avt(a, d)?,
                            None => " ".into(),
                        };
                        self.joined(&self.eval(e, d)?, &sep)
                    }
                    None => self.content_string(body, d, sep.as_ref())?,
                };
                out.attr(q, v)?;
            }
            Instr::Element {
                name,
                namespace,
                scope,
                sets,
                body,
            } => {
                let lexical = self.avt(name, d)?;
                let q = self.computed_name(&lexical, namespace.as_ref(), scope, d, true)?;
                out.start(q, Vec::new());
                self.attr_sets(sets, d, out)?;
                self.body(body, d, out)?;
                out.end(self);
            }
            Instr::CopyOf(e) => {
                for it in self.eval(e, d)? {
                    out.item(self, it, true)?;
                }
            }
            Instr::Sequence(e) => {
                for it in self.eval(e, d)? {
                    out.item(self, it, false)?;
                }
            }
            Instr::Copy { sets, body } => {
                let it = d.item()?.clone();
                match &it {
                    Item::Node(n) => {
                        let (kind, name, ns) = {
                            let t = self.tree(n.tree);
                            let node = t.node(n.idx);
                            let ns = if node.kind == Kind::Element {
                                t.in_scope(n.idx)
                            } else {
                                vec![]
                            };
                            (node.kind, node.name.clone(), ns)
                        };
                        match kind {
                            Kind::Element => {
                                out.start(name.unwrap(), ns);
                                self.attr_sets(sets, d, out)?;
                                self.body(body, d, out)?;
                                out.end(self);
                            }
                            Kind::Document => self.body(body, d, out)?,
                            _ => out.item(self, it, true)?,
                        }
                    }
                    Item::Atomic(_) => out.item(self, it, false)?,
                }
            }
            Instr::Comment { select, body } => {
                let s = match select {
                    Some(e) => self.joined(&self.eval(e, d)?, " "),
                    None => self.content_string(body, d, None)?,
                };
                out.leaf(Kind::Comment, None, s);
            }
            Instr::Pi { name, select, body } => {
                let n = self.avt(name, d)?;
                let s = match select {
                    Some(e) => self.joined(&self.eval(e, d)?, " "),
                    None => self.content_string(body, d, None)?,
                };
                out.leaf(Kind::Pi, Some(QName::local(&n)), s.trim_start().to_string());
            }
            Instr::Message {
                select,
                body,
                terminate,
            } => {
                let mut s = match select {
                    Some(e) => self.joined(&self.eval(e, d)?, " "),
                    None => String::new(),
                };
                s.push_str(&self.content_string(body, d, None)?);
                self.messages.borrow_mut().push(s.clone());
                if *terminate {
                    return Err(format!("xsl:message terminated the transformation: {s}"));
                }
            }
            Instr::Number(n) => {
                let s = self.number(n, d)?;
                out.text(&s);
            }
        }
        Ok(())
    }

    /// The string value of a sequence constructor's result (for
    /// `xsl:value-of`, `xsl:attribute` and friends without `select`).
    fn content_string(&self, body: &[Instr], d: &Dyn, sep: Option<&Avt>) -> R<String> {
        let mut out = Out::sequence();
        self.body(body, d, &mut out)?;
        let items = out.finish_seq(self);
        let sep = match sep {
            Some(a) => self.avt(a, d)?,
            None => String::new(),
        };
        // Adjacent text nodes are merged before joining.
        let mut parts: Vec<String> = Vec::new();
        let mut prev_text = false;
        for it in &items {
            let is_text =
                matches!(it, Item::Node(n) if self.tree(n.tree).node(n.idx).kind == Kind::Text);
            let s = self.string_of(it);
            if is_text && prev_text {
                parts.last_mut().unwrap().push_str(&s);
            } else {
                parts.push(s);
            }
            prev_text = is_text;
        }
        Ok(parts.join(&sep))
    }

    fn computed_name(
        &self,
        lexical: &str,
        ns: Option<&Avt>,
        scope: &[(Rc<str>, Rc<str>)],
        d: &Dyn,
        element: bool,
    ) -> R<QName> {
        let (p, l) = tree::split(lexical.trim());
        if let Some(ns) = ns {
            let uri = self.avt(ns, d)?;
            return Ok(QName::new(&uri, l, p));
        }
        if p.is_empty() && !element {
            return Ok(QName::local(l));
        }
        if p == "xml" {
            return Ok(QName::new(tree::XML_NS, l, p));
        }
        let uri = scope
            .iter()
            .find(|(q, _)| &**q == p)
            .map(|(_, u)| u.clone());
        match uri {
            Some(u) => Ok(QName::new(&u, l, p)),
            None if p.is_empty() => Ok(QName::local(l)),
            None => Err(format!(
                "undeclared prefix '{p}' in computed name '{lexical}'"
            )),
        }
    }

    fn attr_sets(&self, sets: &[QName], d: &Dyn, out: &mut Out) -> R<()> {
        for s in sets {
            let defs = self
                .ss
                .attr_sets
                .iter()
                .find(|(n, _)| n.same(s))
                .map(|(_, v)| v);
            let defs =
                defs.ok_or_else(|| format!("attribute set '{}' is not declared", s.lexical()))?;
            let base = Dyn {
                env: Env::default(),
                ..d.clone()
            };
            for def in defs {
                self.attr_sets(&def.uses, &base, out)?;
                self.body(&def.body, &base, out)?;
            }
        }
        Ok(())
    }

    fn sort(&self, items: Seq, sorts: &[Sort], d: &Dyn) -> R<Seq> {
        if sorts.is_empty() {
            return Ok(items);
        }
        let size = items.len();
        let mut keyed: Vec<(Vec<Option<Atomic>>, Item)> = Vec::with_capacity(size);
        let mut specs = Vec::new();
        for s in sorts {
            let desc = s
                .order
                .as_ref()
                .map(|a| self.avt(a, d))
                .transpose()?
                .is_some_and(|o| o.trim() == "descending");
            let num = s
                .data_type
                .as_ref()
                .map(|a| self.avt(a, d))
                .transpose()?
                .is_some_and(|o| o.trim() == "number");
            let _ = &s.case_order;
            specs.push((desc, num));
        }
        for (k, it) in items.into_iter().enumerate() {
            let mut nd = d.focus(it.clone(), k + 1, size);
            nd.current = Some(it.clone());
            let mut keys = Vec::new();
            for (s, (_, num)) in sorts.iter().zip(&specs) {
                let v = match &s.select {
                    Some(e) => self.atomize(&self.eval(e, &nd)?),
                    None => self.atomize(&vec![it.clone()]),
                };
                let a = v.into_iter().next().map(|a| {
                    if *num {
                        Atomic::Dbl(a.to_f64())
                    } else if let Atomic::Untyped(s) = a {
                        Atomic::Str(s)
                    } else {
                        a
                    }
                });
                keys.push(a);
            }
            keyed.push((keys, it));
        }
        let mut err = None;
        keyed.sort_by(|a, b| {
            for (k, (desc, _)) in specs.iter().enumerate() {
                let o = match (&a.0[k], &b.0[k]) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, _) => std::cmp::Ordering::Less,
                    (_, None) => std::cmp::Ordering::Greater,
                    (Some(x), Some(y)) => {
                        let xn = matches!(x, Atomic::Dbl(f) if f.is_nan());
                        let yn = matches!(y, Atomic::Dbl(f) if f.is_nan());
                        match (xn, yn) {
                            (true, true) => std::cmp::Ordering::Equal,
                            (true, false) => std::cmp::Ordering::Less,
                            (false, true) => std::cmp::Ordering::Greater,
                            _ => match crate::eval::compare(x, y) {
                                Ok(o) => o.unwrap_or(std::cmp::Ordering::Equal),
                                Err(e) => {
                                    err.get_or_insert(e);
                                    std::cmp::Ordering::Equal
                                }
                            },
                        }
                    }
                };
                let o = if *desc { o.reverse() } else { o };
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            std::cmp::Ordering::Equal
        });
        if let Some(e) = err {
            return Err(e);
        }
        Ok(keyed.into_iter().map(|(_, i)| i).collect())
    }

    fn number(&self, n: &Number, d: &Dyn) -> R<String> {
        let nums: Vec<i64> = if let Some(v) = &n.value {
            let v = self.atomize(&self.eval(v, d)?);
            v.iter()
                .map(|a| crate::funcs::round_half_up(a.to_f64()) as i64)
                .collect()
        } else {
            let node = match &n.select {
                Some(e) => match self.eval(e, d)?.as_slice() {
                    [Item::Node(x)] => *x,
                    _ => return Err("xsl:number select must give one node".into()),
                },
                None => match d.item()? {
                    Item::Node(x) => *x,
                    _ => return Err("xsl:number needs a node".into()),
                },
            };
            let outer = self.pattern_env.replace(d.env.clone());
            let r = self.number_of(node, n);
            self.pattern_env.replace(outer);
            r?
        };
        let fmt = self.avt(&n.format, d)?;
        Ok(crate::number::format_numbers(&nums, &fmt))
    }

    fn count_matches(&self, x: NodeRef, n: &Number, of: NodeRef) -> R<bool> {
        match &n.count {
            Some(alts) => {
                for a in alts {
                    if self.matches(x, a)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            None => {
                let t = self.tree(x.tree);
                let (a, b) = (t.node(x.idx), t.node(of.idx));
                Ok(a.kind == b.kind
                    && match (&a.name, &b.name) {
                        (Some(p), Some(q)) => p.same(q),
                        (None, None) => true,
                        _ => false,
                    })
            }
        }
    }

    fn matches_from(&self, x: NodeRef, n: &Number) -> R<bool> {
        if let Some(alts) = &n.from {
            for a in alts {
                if self.matches(x, a)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// The numbers for `xsl:number` without `value` (XSLT 2.0 §12.2). The
    /// `from` node is inclusive: counting happens within its subtree.
    fn number_of(&self, node: NodeRef, n: &Number) -> R<Vec<i64>> {
        let parent = |x: NodeRef| {
            self.tree(x.tree).node(x.idx).parent.map(|p| NodeRef {
                tree: x.tree,
                idx: p,
            })
        };
        let ancestors_or_self = |x: NodeRef| {
            let mut v = vec![x];
            let mut c = parent(x);
            while let Some(p) = c {
                v.push(p);
                c = parent(p);
            }
            v
        };
        let within = |from: Option<NodeRef>, x: NodeRef| match from {
            None => true,
            Some(f) => ancestors_or_self(x).contains(&f),
        };
        let siblings_before = |x: NodeRef| -> R<i64> {
            let mut c = 1;
            if let Some(p) = parent(x) {
                let kids = self.tree(x.tree).node(p.idx).children.clone();
                for k in kids {
                    if k == x.idx {
                        break;
                    }
                    if self.count_matches(
                        NodeRef {
                            tree: x.tree,
                            idx: k,
                        },
                        n,
                        node,
                    )? {
                        c += 1;
                    }
                }
            }
            Ok(c)
        };
        // The nearest ancestor-or-self matching `from` (single, multiple).
        let from_ancestor = || -> R<Option<Option<NodeRef>>> {
            if n.from.is_none() {
                return Ok(Some(None));
            }
            for a in ancestors_or_self(node) {
                if self.matches_from(a, n)? {
                    return Ok(Some(Some(a)));
                }
            }
            Ok(None)
        };
        match n.level.as_str() {
            "any" => {
                // Nodes before `node` (preceding or ancestor-or-self).
                let before: Vec<NodeRef> = (0..=node.idx)
                    .map(|i| NodeRef {
                        tree: node.tree,
                        idx: i,
                    })
                    .filter(|x| {
                        x.idx == node.idx || self.tree(x.tree).node(x.idx).kind != Kind::Attribute
                    })
                    .collect();
                let mut from = None;
                if n.from.is_some() {
                    for &x in before.iter().rev() {
                        if self.matches_from(x, n)? {
                            from = Some(x);
                            break;
                        }
                    }
                    if from.is_none() {
                        return Ok(vec![]);
                    }
                }
                // The reference processor counts every matching node from
                // the `from` node onwards in document order (the XSLT 1.0
                // rule), not only those inside the `from` node's subtree.
                let mut c = 0;
                for &x in &before {
                    if from.is_none_or(|f| x >= f) && self.count_matches(x, n, node)? {
                        c += 1;
                    }
                }
                Ok(if c == 0 { vec![] } else { vec![c] })
            }
            "multiple" => {
                let Some(from) = from_ancestor()? else {
                    return Ok(vec![]);
                };
                let mut v = Vec::new();
                for x in ancestors_or_self(node) {
                    if within(from, x) && self.count_matches(x, n, node)? {
                        v.push(siblings_before(x)?);
                    }
                }
                v.reverse();
                Ok(v)
            }
            _ => {
                let Some(from) = from_ancestor()? else {
                    return Ok(vec![]);
                };
                for x in ancestors_or_self(node) {
                    if self.count_matches(x, n, node)? {
                        return Ok(if within(from, x) {
                            vec![siblings_before(x)?]
                        } else {
                            vec![]
                        });
                    }
                }
                Ok(vec![])
            }
        }
    }
}

// ---- result builder -----------------------------------------------------

/// Where instructions write: a tree (document node at the bottom of the
/// stack) or a sequence of items (`as` bodies and functions).
pub struct Out {
    seq: bool,
    items: Seq,
    tree: Tree,
    stack: Vec<u32>,
    /// The previous thing written was an atomic value (separated by spaces).
    after_atomic: bool,
    /// Finished parentless trees waiting to be registered (sequence mode).
    pending: Vec<Tree>,
}

impl Out {
    pub fn document() -> Out {
        Out {
            seq: false,
            items: vec![],
            tree: Tree::new_document(),
            stack: vec![0],
            after_atomic: false,
            pending: vec![],
        }
    }
    pub fn sequence() -> Out {
        Out {
            seq: true,
            items: vec![],
            tree: Tree::default(),
            stack: vec![],
            after_atomic: false,
            pending: vec![],
        }
    }

    pub fn finish_tree(self) -> Tree {
        self.tree
    }

    pub fn finish_seq(self, e: &Engine) -> Seq {
        let mut pending = self.pending.into_iter();
        self.items
            .into_iter()
            .map(|it| match it {
                // Placeholder for a parentless node built here: register it.
                Item::Node(NodeRef {
                    tree: u32::MAX,
                    idx,
                }) => {
                    let t = pending.next().unwrap();
                    Item::Node(NodeRef {
                        tree: e.add_tree(t),
                        idx,
                    })
                }
                other => other,
            })
            .collect()
    }

    fn top_level(&self) -> bool {
        self.seq && self.stack.is_empty()
    }

    fn new_node(&mut self, mut n: TNode) -> u32 {
        if self.top_level() {
            self.tree = Tree::default();
        }
        n.parent = self.stack.last().copied();
        let i = self.tree.push(n);
        if let Some(&p) = self.stack.last() {
            self.tree.nodes[p as usize].children.push(i);
        }
        i
    }

    fn close_top(&mut self, i: u32) {
        let t = std::mem::take(&mut self.tree);
        self.pending.push(t);
        self.items.push(Item::Node(NodeRef {
            tree: u32::MAX,
            idx: i,
        }));
    }

    pub fn start(&mut self, name: QName, ns: Vec<(Rc<str>, Rc<str>)>) {
        self.after_atomic = false;
        let mut n = TNode::new(Kind::Element, None);
        n.name = Some(name);
        n.ns = ns;
        let i = self.new_node(n);
        self.stack.push(i);
    }

    pub fn end(&mut self, _e: &Engine) {
        self.after_atomic = false;
        let i = self.stack.pop().unwrap();
        if self.top_level() {
            self.close_top(i);
        }
    }

    pub fn attr(&mut self, name: QName, value: String) -> R<()> {
        self.after_atomic = false;
        if self.top_level() {
            let mut n = TNode::new(Kind::Attribute, None);
            n.name = Some(name);
            n.value = value;
            let i = self.new_node(n);
            self.close_top(i);
            return Ok(());
        }
        let p = *self.stack.last().unwrap();
        let pn = &self.tree.nodes[p as usize];
        if pn.kind != Kind::Element {
            return Err(format!(
                "attribute '{}' written outside an element",
                name.lexical()
            ));
        }
        if !pn.children.is_empty() {
            return Err(format!(
                "attribute '{}' written after the element's children",
                name.lexical()
            ));
        }
        let existing = pn.attrs.iter().copied().find(|&a| {
            self.tree.nodes[a as usize]
                .name
                .as_ref()
                .unwrap()
                .same(&name)
        });
        if let Some(a) = existing {
            self.tree.nodes[a as usize].value = value;
            // Spec note 12: the XSLT 1.0 host moves a replaced attribute
            // to the end; the 2.0 host keeps its place.
            if crate::value::compat() {
                let attrs = &mut self.tree.nodes[p as usize].attrs;
                attrs.retain(|&x| x != a);
                attrs.push(a);
            }
            return Ok(());
        }
        let mut n = TNode::new(Kind::Attribute, Some(p));
        n.name = Some(name);
        n.value = value;
        let i = self.tree.push(n);
        self.tree.nodes[p as usize].attrs.push(i);
        Ok(())
    }

    pub fn text(&mut self, s: &str) {
        self.after_atomic = false;
        // A zero-length text node is dropped when added to a tree, but is
        // still an item in a sequence (`xsl:variable as="xs:string"`).
        if self.top_level() {
            let mut n = TNode::new(Kind::Text, None);
            n.value = s.to_string();
            let i = self.new_node(n);
            self.close_top(i);
            return;
        }
        let p = *self.stack.last().unwrap();
        tree::append_text(&mut self.tree, p, s);
    }

    pub fn leaf(&mut self, kind: Kind, name: Option<QName>, value: String) {
        self.after_atomic = false;
        let mut n = TNode::new(kind, None);
        n.name = name;
        n.value = value;
        let i = self.new_node(n);
        if self.top_level() {
            self.close_top(i);
        }
    }

    /// Adds an item from `xsl:sequence` / `xsl:copy-of`.
    pub fn item(&mut self, e: &Engine, it: Item, _copy: bool) -> R<()> {
        if self.top_level() {
            self.items.push(it);
            return Ok(());
        }
        match it {
            Item::Atomic(a) => {
                let s = a.to_string();
                if self.after_atomic {
                    let p = *self.stack.last().unwrap();
                    tree::append_text(&mut self.tree, p, " ");
                }
                let p = *self.stack.last().unwrap();
                tree::append_text(&mut self.tree, p, &s);
                self.after_atomic = true;
            }
            Item::Node(n) => {
                self.after_atomic = false;
                let src = e.tree_rc(n.tree);
                self.copy_node(&src, n.idx);
            }
        }
        Ok(())
    }

    fn copy_node(&mut self, src: &Tree, i: u32) {
        let n = src.node(i);
        match n.kind {
            Kind::Document => {
                for &c in &n.children {
                    self.copy_node(src, c);
                }
            }
            Kind::Element => {
                self.start(n.name.clone().unwrap(), src.in_scope(i));
                for &a in &n.attrs {
                    let an = src.node(a);
                    let _ = self.attr(an.name.clone().unwrap(), an.value.clone());
                }
                for &c in &n.children {
                    self.copy_node(src, c);
                }
                let i = self.stack.pop().unwrap();
                let _ = i;
            }
            Kind::Attribute => {
                let _ = self.attr(n.name.clone().unwrap(), n.value.clone());
            }
            Kind::Text => {
                let p = *self.stack.last().unwrap();
                tree::append_text(&mut self.tree, p, &n.value);
            }
            Kind::Comment | Kind::Pi => self.leaf(n.kind, n.name.clone(), n.value.clone()),
        }
    }
}
