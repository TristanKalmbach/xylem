//! Expression evaluation over a [`Document`].

use std::cell::OnceCell;
use std::collections::HashSet;

use xylem_dom::{Document, NodeId, NodeKind};

use crate::ast::{Axis, BinOp, Expr, NodeTest, PathExpr, PathStart, Step};
use crate::context::{Context, FunctionContext};
use crate::error::XPathError;
use crate::value::{
    Value, XNode, is_namespace_decl, is_xpath_node, string_to_number, string_value,
};

const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// Dynamic evaluation context: context node, position and size.
#[derive(Clone, Copy)]
struct Dyn {
    node: XNode,
    position: usize,
    size: usize,
}

/// Pre-order position and sibling index of every node, built once per evaluation.
struct Index {
    pre: Vec<u32>,
    sib: Vec<u32>,
}

impl Index {
    fn build(doc: &Document) -> Index {
        let n = doc.node_count();
        let mut pre = vec![u32::MAX; n];
        let mut sib = vec![0u32; n];
        let mut counter = 0u32;
        let mut stack = vec![doc.root()];
        while let Some(id) = stack.pop() {
            pre[id.index()] = counter;
            counter += 1;
            for (i, &c) in doc.children(id).iter().enumerate().rev() {
                sib[c.index()] = i as u32;
                stack.push(c);
            }
        }
        Index { pre, sib }
    }
}

pub(crate) struct Evaluator<'a> {
    doc: &'a Document,
    ctx: &'a Context,
    index: OnceCell<Index>,
}

pub(crate) fn evaluate(
    expr: &Expr,
    doc: &Document,
    node: XNode,
    ctx: &Context,
) -> Result<Value, XPathError> {
    let ev = Evaluator {
        doc,
        ctx,
        index: OnceCell::new(),
    };
    ev.eval(
        expr,
        &Dyn {
            node,
            position: 1,
            size: 1,
        },
    )
}

impl<'a> Evaluator<'a> {
    fn index(&self) -> &Index {
        self.index.get_or_init(|| Index::build(self.doc))
    }

    fn pre(&self, id: NodeId) -> u32 {
        self.index()
            .pre
            .get(id.index())
            .copied()
            .unwrap_or(u32::MAX)
    }

    fn sibling_index(&self, id: NodeId) -> usize {
        if self.pre(id) != u32::MAX {
            self.index().sib[id.index()] as usize
        } else {
            self.doc.index_in_parent(id).unwrap_or(0)
        }
    }

    fn order_key(&self, n: XNode) -> (u32, usize, usize) {
        match n {
            XNode::Node(id) => (self.pre(id), 0, id.index()),
            XNode::Attr(id, i) => (self.pre(id), i + 1, id.index()),
        }
    }

    fn sort_dedup(&self, v: &mut Vec<XNode>) {
        v.sort_by_cached_key(|&n| self.order_key(n));
        v.dedup();
    }

    // ------------------------------------------------------------ expressions

    fn eval(&self, expr: &Expr, d: &Dyn) -> Result<Value, XPathError> {
        match expr {
            Expr::Number(n) => Ok(Value::Number(*n)),
            Expr::Literal(s) => Ok(Value::String(s.clone())),
            Expr::Variable(name) => match self.ctx.variables.get(name) {
                Some(Value::NodeSet(ns)) => {
                    let mut ns = ns.clone();
                    self.sort_dedup(&mut ns);
                    Ok(Value::NodeSet(ns))
                }
                Some(v) => Ok(v.clone()),
                None => Err(XPathError::eval(format!("undefined variable ${name}"))),
            },
            Expr::Or(a, b) => Ok(Value::Boolean(
                self.eval(a, d)?.to_boolean() || self.eval(b, d)?.to_boolean(),
            )),
            Expr::And(a, b) => Ok(Value::Boolean(
                self.eval(a, d)?.to_boolean() && self.eval(b, d)?.to_boolean(),
            )),
            Expr::Neg(e) => Ok(Value::Number(-self.eval(e, d)?.to_number(self.doc))),
            Expr::Binary(op, a, b) => {
                let l = self.eval(a, d)?;
                let r = self.eval(b, d)?;
                Ok(self.binary(*op, &l, &r))
            }
            Expr::Union(a, b) => {
                let (l, r) = (self.eval(a, d)?, self.eval(b, d)?);
                match (l, r) {
                    (Value::NodeSet(mut x), Value::NodeSet(y)) => {
                        x.extend(y);
                        self.sort_dedup(&mut x);
                        Ok(Value::NodeSet(x))
                    }
                    (l, r) => Err(XPathError::eval(format!(
                        "the union operator requires node-sets, found {} and {}",
                        l.type_name(),
                        r.type_name()
                    ))),
                }
            }
            Expr::Filter(primary, preds) => {
                let mut nodes = self.eval_node_set(primary, d, "predicate")?;
                for p in preds {
                    nodes = self.filter_predicate(nodes, p)?;
                }
                Ok(Value::NodeSet(nodes))
            }
            Expr::Path(p) => self.eval_path(p, d),
            Expr::Function(name, args) => self.call(name, args, d),
        }
    }

    fn eval_node_set(&self, expr: &Expr, d: &Dyn, what: &str) -> Result<Vec<XNode>, XPathError> {
        match self.eval(expr, d)? {
            Value::NodeSet(n) => Ok(n),
            other => Err(XPathError::eval(format!(
                "{what} requires a node-set, found a {}",
                other.type_name()
            ))),
        }
    }

    fn binary(&self, op: BinOp, l: &Value, r: &Value) -> Value {
        match op {
            BinOp::Add => Value::Number(l.to_number(self.doc) + r.to_number(self.doc)),
            BinOp::Sub => Value::Number(l.to_number(self.doc) - r.to_number(self.doc)),
            BinOp::Mul => Value::Number(l.to_number(self.doc) * r.to_number(self.doc)),
            BinOp::Div => Value::Number(l.to_number(self.doc) / r.to_number(self.doc)),
            BinOp::Mod => Value::Number(l.to_number(self.doc) % r.to_number(self.doc)),
            _ => Value::Boolean(self.compare(op, l, r)),
        }
    }

    // ------------------------------------------------------------ comparisons

    fn compare(&self, op: BinOp, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::NodeSet(x), Value::NodeSet(y)) => self.compare_sets(op, x, y),
            (Value::NodeSet(x), scalar) => self.compare_set_scalar(op, x, scalar, false),
            (scalar, Value::NodeSet(y)) => self.compare_set_scalar(op, y, scalar, true),
            _ => self.compare_scalars(op, a, b),
        }
    }

    fn compare_sets(&self, op: BinOp, x: &[XNode], y: &[XNode]) -> bool {
        match op {
            BinOp::Eq => {
                let left: HashSet<String> = x.iter().map(|&n| string_value(self.doc, n)).collect();
                y.iter().any(|&n| left.contains(&string_value(self.doc, n)))
            }
            BinOp::Neq => {
                let left: Vec<String> = x.iter().map(|&n| string_value(self.doc, n)).collect();
                let right: Vec<String> = y.iter().map(|&n| string_value(self.doc, n)).collect();
                left.iter().any(|l| right.iter().any(|r| l != r))
            }
            _ => {
                let left: Vec<f64> = x
                    .iter()
                    .map(|&n| string_to_number(&string_value(self.doc, n)))
                    .collect();
                let right: Vec<f64> = y
                    .iter()
                    .map(|&n| string_to_number(&string_value(self.doc, n)))
                    .collect();
                left.iter()
                    .any(|&l| right.iter().any(|&r| compare_numbers(op, l, r)))
            }
        }
    }

    /// `swapped` is true when the node-set was the right-hand operand.
    fn compare_set_scalar(
        &self,
        op: BinOp,
        nodes: &[XNode],
        scalar: &Value,
        swapped: bool,
    ) -> bool {
        let ordered = |l: f64, r: f64| {
            if swapped {
                compare_numbers(op, r, l)
            } else {
                compare_numbers(op, l, r)
            }
        };
        match scalar {
            Value::Boolean(b) => {
                let set_bool = !nodes.is_empty();
                if matches!(op, BinOp::Eq | BinOp::Neq) {
                    (set_bool == *b) == (op == BinOp::Eq)
                } else {
                    ordered(f64::from(u8::from(set_bool)), f64::from(u8::from(*b)))
                }
            }
            Value::Number(n) => nodes
                .iter()
                .any(|&node| ordered(string_to_number(&string_value(self.doc, node)), *n)),
            Value::String(s) => match op {
                BinOp::Eq => nodes.iter().any(|&node| string_value(self.doc, node) == *s),
                BinOp::Neq => nodes.iter().any(|&node| string_value(self.doc, node) != *s),
                _ => {
                    let n = string_to_number(s);
                    nodes
                        .iter()
                        .any(|&node| ordered(string_to_number(&string_value(self.doc, node)), n))
                }
            },
            Value::NodeSet(_) => false,
        }
    }

    fn compare_scalars(&self, op: BinOp, a: &Value, b: &Value) -> bool {
        if matches!(op, BinOp::Eq | BinOp::Neq) {
            let equal = if matches!(a, Value::Boolean(_)) || matches!(b, Value::Boolean(_)) {
                a.to_boolean() == b.to_boolean()
            } else if matches!(a, Value::Number(_)) || matches!(b, Value::Number(_)) {
                a.to_number(self.doc) == b.to_number(self.doc)
            } else {
                a.to_string_value(self.doc) == b.to_string_value(self.doc)
            };
            equal == (op == BinOp::Eq)
        } else {
            compare_numbers(op, a.to_number(self.doc), b.to_number(self.doc))
        }
    }

    // ------------------------------------------------------------ location paths

    fn eval_path(&self, path: &PathExpr, d: &Dyn) -> Result<Value, XPathError> {
        let mut current: Vec<XNode> = match &path.start {
            PathStart::Root => vec![XNode::Node(self.doc.root())],
            PathStart::Context => vec![d.node],
            PathStart::Expr(e) => {
                let mut nodes = self.eval_node_set(e, d, "a path step")?;
                self.sort_dedup(&mut nodes);
                nodes
            }
        };
        for step in &path.steps {
            current = self.eval_step(step, &current)?;
            if current.is_empty() {
                break;
            }
        }
        Ok(Value::NodeSet(current))
    }

    fn eval_step(&self, step: &Step, input: &[XNode]) -> Result<Vec<XNode>, XPathError> {
        let mut out = Vec::new();
        for &n in input {
            let mut candidates: Vec<XNode> = self
                .axis_nodes(step.axis, n)
                .into_iter()
                .filter(|&c| self.matches_test(&step.test, step.axis, c))
                .collect();
            for pred in &step.predicates {
                candidates = self.filter_predicate(candidates, pred)?;
            }
            if step.axis.is_reverse() {
                candidates.reverse();
            }
            out.extend(candidates);
        }
        if input.len() > 1 {
            self.sort_dedup(&mut out);
        }
        Ok(out)
    }

    /// Filters `nodes` (given in proximity order) by a predicate.
    fn filter_predicate(&self, nodes: Vec<XNode>, pred: &Expr) -> Result<Vec<XNode>, XPathError> {
        if let Expr::Number(n) = pred {
            let idx = *n;
            return Ok(
                if idx >= 1.0 && idx.fract() == 0.0 && (idx as usize) <= nodes.len() {
                    vec![nodes[idx as usize - 1]]
                } else {
                    Vec::new()
                },
            );
        }
        let size = nodes.len();
        let mut out = Vec::new();
        for (i, &node) in nodes.iter().enumerate() {
            let d = Dyn {
                node,
                position: i + 1,
                size,
            };
            let keep = match self.eval(pred, &d)? {
                Value::Number(n) => n == (i + 1) as f64,
                other => other.to_boolean(),
            };
            if keep {
                out.push(node);
            }
        }
        Ok(out)
    }

    fn is_xp(&self, id: NodeId) -> bool {
        is_xpath_node(self.doc, id)
    }

    fn subtree(&self, id: NodeId, out: &mut Vec<XNode>) {
        out.extend(
            self.doc
                .descendants(id)
                .filter(|&n| self.is_xp(n))
                .map(XNode::Node),
        );
    }

    /// Nodes on `axis` from `n`, in axis (proximity) order.
    fn axis_nodes(&self, axis: Axis, n: XNode) -> Vec<XNode> {
        let doc = self.doc;
        let (id, is_attr) = match n {
            XNode::Node(id) => (id, false),
            XNode::Attr(id, _) => (id, true),
        };
        let mut out = Vec::new();
        match axis {
            Axis::Current => out.push(n),
            Axis::Namespace => {}
            Axis::Parent => {
                if is_attr {
                    out.push(XNode::Node(id));
                } else if let Some(p) = doc.parent(id) {
                    out.push(XNode::Node(p));
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if axis == Axis::AncestorOrSelf {
                    out.push(n);
                }
                if is_attr {
                    out.push(XNode::Node(id));
                }
                out.extend(doc.ancestors(id).map(XNode::Node));
            }
            Axis::Attribute => {
                if !is_attr && let Some(e) = doc.element(id) {
                    out.extend(
                        e.attrs
                            .iter()
                            .enumerate()
                            .filter(|(_, a)| !is_namespace_decl(&a.name))
                            .map(|(i, _)| XNode::Attr(id, i)),
                    );
                }
            }
            Axis::Child => {
                if !is_attr {
                    out.extend(
                        doc.children(id)
                            .iter()
                            .copied()
                            .filter(|&c| self.is_xp(c))
                            .map(XNode::Node),
                    );
                }
            }
            Axis::Descendant | Axis::DescendantOrSelf => {
                if axis == Axis::DescendantOrSelf {
                    out.push(n);
                }
                if !is_attr {
                    out.extend(
                        doc.descendants(id)
                            .skip(1)
                            .filter(|&c| self.is_xp(c))
                            .map(XNode::Node),
                    );
                }
            }
            Axis::FollowingSibling | Axis::PrecedingSibling => {
                if !is_attr && let Some(p) = doc.parent(id) {
                    let kids = doc.children(p);
                    let at = self.sibling_index(id);
                    if axis == Axis::FollowingSibling {
                        out.extend(
                            kids[at + 1..]
                                .iter()
                                .copied()
                                .filter(|&c| self.is_xp(c))
                                .map(XNode::Node),
                        );
                    } else {
                        out.extend(
                            kids[..at]
                                .iter()
                                .rev()
                                .copied()
                                .filter(|&c| self.is_xp(c))
                                .map(XNode::Node),
                        );
                    }
                }
            }
            Axis::Following => {
                if is_attr {
                    // Content of the owning element follows its attributes.
                    for &c in doc.children(id) {
                        self.subtree(c, &mut out);
                    }
                }
                let mut cur = id;
                while let Some(p) = doc.parent(cur) {
                    let at = self.sibling_index(cur);
                    for &s in &doc.children(p)[at + 1..] {
                        self.subtree(s, &mut out);
                    }
                    cur = p;
                }
            }
            Axis::Preceding => {
                let mut cur = id;
                while let Some(p) = doc.parent(cur) {
                    let at = self.sibling_index(cur);
                    for &s in doc.children(p)[..at].iter().rev() {
                        let mut sub = Vec::new();
                        self.subtree(s, &mut sub);
                        out.extend(sub.into_iter().rev());
                    }
                    cur = p;
                }
            }
        }
        out
    }

    fn matches_test(&self, test: &NodeTest, axis: Axis, n: XNode) -> bool {
        let doc = self.doc;
        let principal_attr = axis == Axis::Attribute;
        match test {
            NodeTest::Node => true,
            NodeTest::Text => matches!(n, XNode::Node(id) if doc.is_text(id)),
            NodeTest::Comment => {
                matches!(n, XNode::Node(id) if matches!(doc.kind(id), NodeKind::Comment(_)))
            }
            NodeTest::Pi(target) => match n {
                XNode::Node(id) => match doc.kind(id) {
                    NodeKind::Pi { target: t, .. } => target.as_ref().is_none_or(|want| want == t),
                    _ => false,
                },
                XNode::Attr(..) => false,
            },
            NodeTest::Any => match n {
                XNode::Attr(..) => principal_attr,
                XNode::Node(id) => !principal_attr && doc.is_element(id),
            },
            NodeTest::Name(q) => match n {
                XNode::Attr(id, i) => {
                    principal_attr
                        && doc
                            .element(id)
                            .and_then(|e| e.attrs.get(i))
                            .is_some_and(|a| self.name_matches(q, &a.name, id, true))
                }
                XNode::Node(id) => {
                    !principal_attr
                        && doc
                            .name(id)
                            .is_some_and(|name| self.name_matches(q, name, id, false))
                }
            },
            NodeTest::PrefixWildcard(p) => {
                let (actual, id, is_attr) = match n {
                    XNode::Attr(id, i) => match doc.element(id).and_then(|e| e.attrs.get(i)) {
                        Some(a) => (a.name.as_str(), id, true),
                        None => return false,
                    },
                    XNode::Node(id) => match doc.name(id) {
                        Some(name) => (name, id, false),
                        None => return false,
                    },
                };
                if is_attr != principal_attr {
                    return false;
                }
                match self.ctx.namespaces.get(p) {
                    Some(uri) => {
                        self.namespace_of(actual, id, is_attr).as_deref() == Some(uri.as_str())
                    }
                    None => actual
                        .split_once(':')
                        .is_some_and(|(prefix, _)| prefix == p),
                }
            }
        }
    }

    /// Compares a name test with a node name, through the namespace map when
    /// the test prefix is mapped and literally otherwise.
    fn name_matches(&self, test: &str, actual: &str, owner: NodeId, is_attr: bool) -> bool {
        if let Some((tp, tl)) = test.split_once(':')
            && let Some(uri) = self.ctx.namespaces.get(tp)
        {
            let local = actual.rsplit_once(':').map_or(actual, |(_, l)| l);
            return local == tl
                && self.namespace_of(actual, owner, is_attr).as_deref() == Some(uri.as_str());
        }
        test == actual
    }

    /// Namespace URI of an element or attribute name, from in-scope declarations.
    fn namespace_of(&self, qname: &str, owner: NodeId, is_attr: bool) -> Option<String> {
        let prefix = qname.split_once(':').map(|(p, _)| p);
        if prefix.is_none() && is_attr {
            return None;
        }
        self.lookup_namespace(owner, prefix)
    }

    fn lookup_namespace(&self, node: NodeId, prefix: Option<&str>) -> Option<String> {
        let decl = match prefix {
            Some("xml") => return Some(XML_NAMESPACE.to_string()),
            Some(p) => format!("xmlns:{p}"),
            None => "xmlns".to_string(),
        };
        std::iter::once(node)
            .chain(self.doc.ancestors(node))
            .find_map(|n| self.doc.attr(n, &decl))
            .filter(|uri| !uri.is_empty())
            .map(str::to_string)
    }

    // ------------------------------------------------------------ functions

    fn arg_string(&self, args: &[Expr], i: usize, d: &Dyn) -> Result<String, XPathError> {
        Ok(self.eval(&args[i], d)?.to_string_value(self.doc))
    }

    fn arg_number(&self, args: &[Expr], i: usize, d: &Dyn) -> Result<f64, XPathError> {
        Ok(self.eval(&args[i], d)?.to_number(self.doc))
    }

    fn arg_nodes(
        &self,
        name: &str,
        args: &[Expr],
        i: usize,
        d: &Dyn,
    ) -> Result<Vec<XNode>, XPathError> {
        self.eval_node_set(&args[i], d, &format!("{name}()"))
    }

    /// The node a name-style function operates on: its argument's first node, or the context node.
    fn subject_node(
        &self,
        name: &str,
        args: &[Expr],
        d: &Dyn,
    ) -> Result<Option<XNode>, XPathError> {
        if args.is_empty() {
            Ok(Some(d.node))
        } else {
            Ok(self.arg_nodes(name, args, 0, d)?.first().copied())
        }
    }

    fn node_name(&self, n: XNode) -> String {
        match n {
            XNode::Attr(id, i) => self
                .doc
                .element(id)
                .and_then(|e| e.attrs.get(i))
                .map(|a| a.name.clone())
                .unwrap_or_default(),
            XNode::Node(id) => match self.doc.kind(id) {
                NodeKind::Element(e) => e.name.clone(),
                NodeKind::Pi { target, .. } => target.clone(),
                _ => String::new(),
            },
        }
    }

    fn call(&self, name: &str, args: &[Expr], d: &Dyn) -> Result<Value, XPathError> {
        if let Some(f) = self.ctx.functions.get(name) {
            let mut values = Vec::with_capacity(args.len());
            for a in args {
                values.push(self.eval(a, d)?);
            }
            let fc = FunctionContext {
                doc: self.doc,
                node: d.node,
                position: d.position,
                size: d.size,
            };
            return match f(&fc, &values)? {
                Value::NodeSet(mut ns) => {
                    self.sort_dedup(&mut ns);
                    Ok(Value::NodeSet(ns))
                }
                v => Ok(v),
            };
        }
        let arity = |min: usize, max: usize| -> Result<(), XPathError> {
            if args.len() < min || args.len() > max {
                let expected = if min == max {
                    min.to_string()
                } else if max == usize::MAX {
                    format!("at least {min}")
                } else {
                    format!("{min} to {max}")
                };
                Err(XPathError::eval(format!(
                    "{name}() takes {expected} argument(s), {} given",
                    args.len()
                )))
            } else {
                Ok(())
            }
        };
        match name {
            "last" => {
                arity(0, 0)?;
                Ok(Value::Number(d.size as f64))
            }
            "position" => {
                arity(0, 0)?;
                Ok(Value::Number(d.position as f64))
            }
            "count" => {
                arity(1, 1)?;
                Ok(Value::Number(self.arg_nodes(name, args, 0, d)?.len() as f64))
            }
            "id" => {
                arity(1, 1)?;
                self.fn_id(&args[0], d)
            }
            "local-name" => {
                arity(0, 1)?;
                let n = self.subject_node(name, args, d)?;
                let full = n.map(|n| self.node_name(n)).unwrap_or_default();
                Ok(Value::String(
                    full.rsplit_once(':')
                        .map_or(full.clone(), |(_, l)| l.to_string()),
                ))
            }
            "name" => {
                arity(0, 1)?;
                let n = self.subject_node(name, args, d)?;
                Ok(Value::String(
                    n.map(|n| self.node_name(n)).unwrap_or_default(),
                ))
            }
            "namespace-uri" => {
                arity(0, 1)?;
                let uri = self.subject_node(name, args, d)?.and_then(|n| {
                    let q = self.node_name(n);
                    if q.is_empty() {
                        return None;
                    }
                    self.namespace_of(&q, n.owner(), n.is_attr())
                });
                Ok(Value::String(uri.unwrap_or_default()))
            }
            "string" => {
                arity(0, 1)?;
                if args.is_empty() {
                    Ok(Value::String(string_value(self.doc, d.node)))
                } else {
                    Ok(Value::String(self.arg_string(args, 0, d)?))
                }
            }
            "concat" => {
                arity(2, usize::MAX)?;
                let mut s = String::new();
                for i in 0..args.len() {
                    s.push_str(&self.arg_string(args, i, d)?);
                }
                Ok(Value::String(s))
            }
            "starts-with" => {
                arity(2, 2)?;
                Ok(Value::Boolean(
                    self.arg_string(args, 0, d)?
                        .starts_with(&self.arg_string(args, 1, d)?),
                ))
            }
            "contains" => {
                arity(2, 2)?;
                Ok(Value::Boolean(
                    self.arg_string(args, 0, d)?
                        .contains(&self.arg_string(args, 1, d)?),
                ))
            }
            "substring-before" => {
                arity(2, 2)?;
                let (s, pat) = (self.arg_string(args, 0, d)?, self.arg_string(args, 1, d)?);
                Ok(Value::String(
                    s.find(&pat).map(|i| s[..i].to_string()).unwrap_or_default(),
                ))
            }
            "substring-after" => {
                arity(2, 2)?;
                let (s, pat) = (self.arg_string(args, 0, d)?, self.arg_string(args, 1, d)?);
                Ok(Value::String(
                    s.find(&pat)
                        .map(|i| s[i + pat.len()..].to_string())
                        .unwrap_or_default(),
                ))
            }
            "substring" => {
                arity(2, 3)?;
                let s = self.arg_string(args, 0, d)?;
                let start = self.arg_number(args, 1, d)?;
                let len = if args.len() == 3 {
                    Some(self.arg_number(args, 2, d)?)
                } else {
                    None
                };
                Ok(Value::String(substring(&s, start, len)))
            }
            "string-length" => {
                arity(0, 1)?;
                let s = if args.is_empty() {
                    string_value(self.doc, d.node)
                } else {
                    self.arg_string(args, 0, d)?
                };
                Ok(Value::Number(s.chars().count() as f64))
            }
            "normalize-space" => {
                arity(0, 1)?;
                let s = if args.is_empty() {
                    string_value(self.doc, d.node)
                } else {
                    self.arg_string(args, 0, d)?
                };
                Ok(Value::String(
                    s.split([' ', '\t', '\r', '\n'])
                        .filter(|w| !w.is_empty())
                        .collect::<Vec<_>>()
                        .join(" "),
                ))
            }
            "translate" => {
                arity(3, 3)?;
                let s = self.arg_string(args, 0, d)?;
                let from: Vec<char> = self.arg_string(args, 1, d)?.chars().collect();
                let to: Vec<char> = self.arg_string(args, 2, d)?.chars().collect();
                let out = s
                    .chars()
                    .filter_map(|c| match from.iter().position(|&f| f == c) {
                        Some(i) => to.get(i).copied(),
                        None => Some(c),
                    })
                    .collect();
                Ok(Value::String(out))
            }
            "boolean" => {
                arity(1, 1)?;
                Ok(Value::Boolean(self.eval(&args[0], d)?.to_boolean()))
            }
            "not" => {
                arity(1, 1)?;
                Ok(Value::Boolean(!self.eval(&args[0], d)?.to_boolean()))
            }
            "true" => {
                arity(0, 0)?;
                Ok(Value::Boolean(true))
            }
            "false" => {
                arity(0, 0)?;
                Ok(Value::Boolean(false))
            }
            "lang" => {
                arity(1, 1)?;
                let wanted = self.arg_string(args, 0, d)?;
                Ok(Value::Boolean(self.fn_lang(&wanted, d.node)))
            }
            "number" => {
                arity(0, 1)?;
                if args.is_empty() {
                    Ok(Value::Number(string_to_number(&string_value(
                        self.doc, d.node,
                    ))))
                } else {
                    Ok(Value::Number(self.eval(&args[0], d)?.to_number(self.doc)))
                }
            }
            "sum" => {
                arity(1, 1)?;
                let total = self
                    .arg_nodes(name, args, 0, d)?
                    .iter()
                    .map(|&n| string_to_number(&string_value(self.doc, n)))
                    .sum();
                Ok(Value::Number(total))
            }
            "floor" => {
                arity(1, 1)?;
                Ok(Value::Number(self.arg_number(args, 0, d)?.floor()))
            }
            "ceiling" => {
                arity(1, 1)?;
                Ok(Value::Number(self.arg_number(args, 0, d)?.ceil()))
            }
            "round" => {
                arity(1, 1)?;
                Ok(Value::Number(xpath_round(self.arg_number(args, 0, d)?)))
            }
            _ => Err(XPathError::eval(format!("unknown function {name}()"))),
        }
    }

    /// `id()`: matches `id` and `xml:id` attributes, as no DTD ID typing is available.
    fn fn_id(&self, arg: &Expr, d: &Dyn) -> Result<Value, XPathError> {
        let value = self.eval(arg, d)?;
        let tokens: HashSet<String> = match &value {
            Value::NodeSet(ns) => ns
                .iter()
                .flat_map(|&n| {
                    string_value(self.doc, n)
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                })
                .collect(),
            other => other
                .to_string_value(self.doc)
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        };
        let mut out = Vec::new();
        if !tokens.is_empty() {
            for id in self.doc.descendants(self.doc.root()) {
                let hit = self.doc.element(id).is_some_and(|e| {
                    e.attrs.iter().any(|a| {
                        (a.name == "id" || a.name == "xml:id") && tokens.contains(&a.value)
                    })
                });
                if hit {
                    out.push(XNode::Node(id));
                }
            }
        }
        Ok(Value::NodeSet(out))
    }

    fn fn_lang(&self, wanted: &str, node: XNode) -> bool {
        let start = node.owner();
        let found = std::iter::once(start)
            .chain(self.doc.ancestors(start))
            .find_map(|n| self.doc.attr(n, "xml:lang"));
        let Some(actual) = found else { return false };
        let (actual, wanted) = (actual.to_ascii_lowercase(), wanted.to_ascii_lowercase());
        actual == wanted
            || actual
                .strip_prefix(&wanted)
                .is_some_and(|rest| rest.starts_with('-'))
    }
}

fn compare_numbers(op: BinOp, l: f64, r: f64) -> bool {
    match op {
        BinOp::Eq => l == r,
        BinOp::Neq => l != r,
        BinOp::Lt => l < r,
        BinOp::Le => l <= r,
        BinOp::Gt => l > r,
        BinOp::Ge => l >= r,
        _ => false,
    }
}

/// XPath `round()`: nearest integer, ties toward positive infinity.
fn xpath_round(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 4_503_599_627_370_496.0 {
        x
    } else {
        (x + 0.5).floor()
    }
}

fn substring(s: &str, start: f64, len: Option<f64>) -> String {
    let start = xpath_round(start);
    let end = match len {
        Some(l) => start + xpath_round(l),
        None => f64::INFINITY,
    };
    s.chars()
        .enumerate()
        .filter(|(i, _)| {
            let p = (*i + 1) as f64;
            p >= start && p < end
        })
        .map(|(_, c)| c)
        .collect()
}
