//! XPath 2.0 evaluation over the engine's trees.

use crate::exec::{Dyn, Engine};
use crate::tree::{Kind, NodeRef, QName};
use crate::value::{Atomic, Decimal, Item, Seq, cmp_numeric, parse_double, to_dec};
use crate::xpath::{Axis, BinOp, Expr, ItemType, NodeTest, PathStep, SeqType, Step};
use std::cmp::Ordering;

pub type R<T> = Result<T, String>;

impl Engine<'_> {
    pub fn eval(&self, e: &Expr, d: &Dyn) -> R<Seq> {
        match e {
            Expr::Literal(a) => Ok(vec![Item::Atomic(a.clone())]),
            Expr::Var(n) => self.var(n, d),
            Expr::Context => Ok(vec![d.item()?.clone()]),
            Expr::Root => {
                let Item::Node(n) = d.item()? else {
                    return Err("'/' needs a node as the context item".into());
                };
                Ok(vec![Item::Node(NodeRef {
                    tree: n.tree,
                    idx: 0,
                })])
            }
            Expr::Seq(v) => {
                let mut out = Vec::new();
                for x in v {
                    out.extend(self.eval(x, d)?);
                }
                Ok(out)
            }
            Expr::Path { start, steps } => {
                let mut cur = match start {
                    Some(s) => self.eval(s, d)?,
                    None => vec![d.item()?.clone()],
                };
                for (k, st) in steps.iter().enumerate() {
                    cur = self.step(&cur, st, d, k + 1 == steps.len())?;
                }
                Ok(cur)
            }
            Expr::Filter(base, preds) => {
                let mut v = self.eval(base, d)?;
                for p in preds {
                    v = self.filter(v, p, d)?;
                }
                Ok(v)
            }
            Expr::Bin(op, a, b) => self.binary(*op, a, b, d),
            Expr::Neg(a) => {
                let v = self.atomize(&self.eval(a, d)?);
                match v.as_slice() {
                    [] => Ok(vec![]),
                    [x] => Ok(vec![Item::Atomic(match numeric(x)? {
                        Atomic::Int(i) => Atomic::Int(-i),
                        Atomic::Dec(x) => Atomic::Dec(Decimal(-x.0)),
                        x => Atomic::Dbl(-x.to_f64()),
                    })]),
                    _ => Err("unary minus on a sequence".into()),
                }
            }
            Expr::If(c, a, b) => {
                if self.ebv(&self.eval(c, d)?)? {
                    self.eval(a, d)
                } else {
                    self.eval(b, d)
                }
            }
            Expr::For(binds, ret) => {
                let mut out = Vec::new();
                self.for_loop(binds, 0, ret, d, &mut |v| {
                    out.extend(v);
                    Ok(false)
                })?;
                Ok(out)
            }
            Expr::Quant(every, binds, sat) => {
                let mut result = *every;
                self.for_loop(binds, 0, sat, d, &mut |v| {
                    let b = self.ebv(&v)?;
                    if b != *every {
                        result = b;
                        return Ok(true);
                    }
                    Ok(false)
                })?;
                Ok(vec![Item::Atomic(Atomic::Bool(result))])
            }
            Expr::InstanceOf(a, t) => {
                let v = self.eval(a, d)?;
                Ok(vec![Item::Atomic(Atomic::Bool(self.instance_of(&v, t)))])
            }
            Expr::Cast(a, t, opt) => {
                let v = self.atomize(&self.eval(a, d)?);
                match v.as_slice() {
                    [] if *opt => Ok(vec![]),
                    [x] => Ok(vec![Item::Atomic(cast(x, t)?)]),
                    _ => Err(format!("cast as xs:{t} needs exactly one item")),
                }
            }
            Expr::Castable(a, t, opt) => {
                let v = self.atomize(&self.eval(a, d)?);
                let ok = match v.as_slice() {
                    [] => *opt,
                    [x] => cast(x, t).is_ok(),
                    _ => false,
                };
                Ok(vec![Item::Atomic(Atomic::Bool(ok))])
            }
            Expr::Call(name, args) => self.call(name, args, d),
        }
    }

    fn for_loop(
        &self,
        binds: &[(QName, Expr)],
        k: usize,
        ret: &Expr,
        d: &Dyn,
        f: &mut dyn FnMut(Seq) -> R<bool>,
    ) -> R<bool> {
        if k == binds.len() {
            return f(self.eval(ret, d)?);
        }
        let (name, e) = &binds[k];
        for it in self.eval(e, d)? {
            let inner = d.bind(name.clone(), vec![it]);
            if self.for_loop(binds, k + 1, ret, &inner, f)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn step(&self, input: &Seq, st: &PathStep, d: &Dyn, last: bool) -> R<Seq> {
        let mut out: Seq = Vec::new();
        let mut all_nodes = true;
        let size = input.len();
        for (pos, it) in input.iter().enumerate() {
            let Item::Node(n) = it else {
                return Err("a path step needs nodes, not atomic values".into());
            };
            match st {
                PathStep::Axis(s) => {
                    let v = self.axis_step(*n, s, d)?;
                    out.extend(v.into_iter().map(Item::Node));
                }
                PathStep::Expr(e, preds) => {
                    let inner = d.focus(it.clone(), pos + 1, size);
                    let mut v = self.eval(e, &inner)?;
                    for p in preds {
                        v = self.filter(v, p, &inner)?;
                    }
                    for x in &v {
                        if !matches!(x, Item::Node(_)) {
                            all_nodes = false;
                        }
                    }
                    out.extend(v);
                }
            }
        }
        if all_nodes {
            let mut nodes: Vec<NodeRef> = out
                .into_iter()
                .map(|i| match i {
                    Item::Node(n) => n,
                    _ => unreachable!(),
                })
                .collect();
            if size > 1 || matches!(st, PathStep::Expr(..)) {
                nodes.sort();
                nodes.dedup();
            }
            Ok(nodes.into_iter().map(Item::Node).collect())
        } else if last {
            Ok(out)
        } else {
            Err("only the last step of a path may return atomic values".into())
        }
    }

    /// The nodes of one axis step, in document order.
    pub fn axis_step(&self, n: NodeRef, s: &Step, d: &Dyn) -> R<Vec<NodeRef>> {
        let t = self.tree(n.tree);
        let mut v: Vec<u32> = Vec::new();
        let node = t.node(n.idx);
        let push_desc = |v: &mut Vec<u32>, i: u32| {
            let mut stack = vec![i];
            while let Some(x) = stack.pop() {
                v.push(x);
                stack.extend(t.node(x).children.iter().rev().copied());
            }
        };
        match s.axis {
            Axis::Child => v.extend(&node.children),
            Axis::Attribute => v.extend(&node.attrs),
            Axis::SelfAxis => v.push(n.idx),
            Axis::Parent => v.extend(node.parent),
            Axis::Descendant | Axis::DescendantOrSelf => {
                if s.axis == Axis::DescendantOrSelf {
                    v.push(n.idx);
                }
                for &c in &node.children {
                    push_desc(&mut v, c);
                }
            }
            Axis::Ancestor | Axis::AncestorOrSelf => {
                if s.axis == Axis::AncestorOrSelf {
                    v.push(n.idx);
                }
                let mut p = node.parent;
                while let Some(x) = p {
                    v.push(x);
                    p = t.node(x).parent;
                }
            }
            Axis::FollowingSibling | Axis::PrecedingSibling => {
                if node.kind != Kind::Attribute
                    && let Some(p) = node.parent
                {
                    let sib = &t.node(p).children;
                    let at = sib.iter().position(|&c| c == n.idx).unwrap_or(0);
                    if s.axis == Axis::FollowingSibling {
                        v.extend(&sib[at + 1..]);
                    } else {
                        v.extend(sib[..at].iter().rev());
                    }
                }
            }
            Axis::Following => {
                let mut cur = n.idx;
                if node.kind == Kind::Attribute {
                    cur = node.parent.unwrap_or(cur);
                    for &c in &t.node(cur).children {
                        push_desc(&mut v, c);
                    }
                }
                loop {
                    let cn = t.node(cur);
                    let Some(p) = cn.parent else { break };
                    let sib = &t.node(p).children;
                    if let Some(at) = sib.iter().position(|&c| c == cur) {
                        for &c in &sib[at + 1..] {
                            push_desc(&mut v, c);
                        }
                    }
                    cur = p;
                }
            }
            Axis::Preceding => {
                let mut cur = if node.kind == Kind::Attribute {
                    node.parent.unwrap_or(n.idx)
                } else {
                    n.idx
                };
                let mut acc = Vec::new();
                while let Some(p) = t.node(cur).parent {
                    let sib = &t.node(p).children;
                    if let Some(at) = sib.iter().position(|&c| c == cur) {
                        for &c in sib[..at].iter().rev() {
                            let mut sub = Vec::new();
                            push_desc(&mut sub, c);
                            acc.extend(sub.into_iter().rev());
                        }
                    }
                    cur = p;
                }
                v = acc;
            }
        }
        let principal = if s.axis == Axis::Attribute {
            Kind::Attribute
        } else {
            Kind::Element
        };
        let mut nodes: Vec<NodeRef> = v
            .into_iter()
            .filter(|&i| test(t.node(i), &s.test, principal))
            .map(|idx| NodeRef { tree: n.tree, idx })
            .collect();
        drop(t);
        for p in &s.preds {
            let items: Seq = nodes.into_iter().map(Item::Node).collect();
            nodes = self
                .filter(items, p, d)?
                .into_iter()
                .map(|i| match i {
                    Item::Node(n) => n,
                    _ => unreachable!(),
                })
                .collect();
        }
        if is_reverse(&s.axis) {
            nodes.reverse();
        }
        Ok(nodes)
    }

    pub fn filter(&self, v: Seq, p: &Expr, d: &Dyn) -> R<Seq> {
        // A literal number predicate selects by position without evaluating.
        if let Expr::Literal(Atomic::Int(i)) = p {
            let i = *i;
            return Ok(if i >= 1 {
                v.into_iter().nth(i as usize - 1).into_iter().collect()
            } else {
                vec![]
            });
        }
        let size = v.len();
        let mut out = Vec::new();
        for (k, it) in v.into_iter().enumerate() {
            let inner = d.focus(it.clone(), k + 1, size);
            let r = self.eval(p, &inner)?;
            let keep = match r.as_slice() {
                [Item::Atomic(a)] if a.is_numeric() => a.to_f64() == (k + 1) as f64,
                _ => self.ebv(&r)?,
            };
            if keep {
                out.push(it);
            }
        }
        Ok(out)
    }

    pub fn string_of(&self, it: &Item) -> String {
        match it {
            Item::Node(n) => self.tree(n.tree).string_value(n.idx),
            Item::Atomic(a) => a.to_string(),
        }
    }

    pub fn atomize(&self, v: &Seq) -> Vec<Atomic> {
        v.iter()
            .map(|i| match i {
                Item::Node(n) => Atomic::Untyped(self.tree(n.tree).string_value(n.idx).into()),
                Item::Atomic(a) => a.clone(),
            })
            .collect()
    }

    pub fn ebv(&self, v: &Seq) -> R<bool> {
        match v.as_slice() {
            [] => Ok(false),
            [Item::Node(_), ..] => Ok(true),
            [Item::Atomic(a)] => Ok(match a {
                Atomic::Bool(b) => *b,
                Atomic::Str(s) | Atomic::Untyped(s) => !s.is_empty(),
                Atomic::Int(i) => *i != 0,
                Atomic::Dec(x) => x.0 != 0,
                Atomic::Dbl(x) => *x != 0.0 && !x.is_nan(),
            }),
            _ => Err(
                "the effective boolean value of a sequence of several atomic values is not defined"
                    .into(),
            ),
        }
    }

    /// The string value of a sequence joined by `sep` (`xsl:value-of`).
    pub fn joined(&self, v: &Seq, sep: &str) -> String {
        let parts: Vec<String> = self.atomize(v).iter().map(Atomic::to_string).collect();
        parts.join(sep)
    }

    fn binary(&self, op: BinOp, a: &Expr, b: &Expr, d: &Dyn) -> R<Seq> {
        let bool_ = |b: bool| Ok(vec![Item::Atomic(Atomic::Bool(b))]);
        match op {
            BinOp::Or => {
                if self.ebv(&self.eval(a, d)?)? {
                    return bool_(true);
                }
                bool_(self.ebv(&self.eval(b, d)?)?)
            }
            BinOp::And => {
                if !self.ebv(&self.eval(a, d)?)? {
                    return bool_(false);
                }
                bool_(self.ebv(&self.eval(b, d)?)?)
            }
            BinOp::GenEq
            | BinOp::GenNe
            | BinOp::GenLt
            | BinOp::GenLe
            | BinOp::GenGt
            | BinOp::GenGe
                if crate::value::compat() =>
            {
                let x = self.eval(a, d)?;
                let y = self.eval(b, d)?;
                bool_(self.compare_1(op, &x, &y)?)
            }
            BinOp::GenEq
            | BinOp::GenNe
            | BinOp::GenLt
            | BinOp::GenLe
            | BinOp::GenGt
            | BinOp::GenGe => {
                let x = self.atomize(&self.eval(a, d)?);
                let y = self.atomize(&self.eval(b, d)?);
                for p in &x {
                    for q in &y {
                        if general_cmp(op, p, q)? {
                            return bool_(true);
                        }
                    }
                }
                bool_(false)
            }
            BinOp::ValEq
            | BinOp::ValNe
            | BinOp::ValLt
            | BinOp::ValLe
            | BinOp::ValGt
            | BinOp::ValGe => {
                let x = self.atomize(&self.eval(a, d)?);
                let y = self.atomize(&self.eval(b, d)?);
                match (x.as_slice(), y.as_slice()) {
                    ([], _) | (_, []) => Ok(vec![]),
                    ([p], [q]) => {
                        let s = |a: &Atomic| match a {
                            Atomic::Untyped(s) => Atomic::Str(s.clone()),
                            o => o.clone(),
                        };
                        let o = compare(&s(p), &s(q))?;
                        bool_(holds(op, o))
                    }
                    _ => Err("a value comparison needs single values".into()),
                }
            }
            BinOp::Is | BinOp::Precedes | BinOp::Follows => {
                let x = self.eval(a, d)?;
                let y = self.eval(b, d)?;
                match (x.as_slice(), y.as_slice()) {
                    ([Item::Node(p)], [Item::Node(q)]) => bool_(match op {
                        BinOp::Is => p == q,
                        BinOp::Precedes => p < q,
                        _ => p > q,
                    }),
                    ([], _) | (_, []) => Ok(vec![]),
                    _ => Err("node comparison needs single nodes".into()),
                }
            }
            BinOp::To => {
                let x = self.atomize(&self.eval(a, d)?);
                let y = self.atomize(&self.eval(b, d)?);
                match (x.as_slice(), y.as_slice()) {
                    ([p], [q]) => {
                        let (p, q) = (to_int(p)?, to_int(q)?);
                        Ok((p..=q).map(|i| Item::Atomic(Atomic::Int(i))).collect())
                    }
                    _ => Ok(vec![]),
                }
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::IDiv | BinOp::Mod => {
                let x = self.atomize(&self.eval(a, d)?);
                let y = self.atomize(&self.eval(b, d)?);
                match (x.as_slice(), y.as_slice()) {
                    ([], _) | (_, []) => Ok(vec![]),
                    // XPath 1.0: every operand is converted with number().
                    ([p], [q]) if crate::value::compat() => Ok(vec![Item::Atomic(arith(
                        op,
                        &Atomic::Dbl(p.to_f64()),
                        &Atomic::Dbl(q.to_f64()),
                    )?)]),
                    ([p], [q]) => Ok(vec![Item::Atomic(arith(op, &numeric(p)?, &numeric(q)?)?)]),
                    ([p, ..], [q, ..]) if crate::value::compat() => Ok(vec![Item::Atomic(arith(
                        op,
                        &Atomic::Dbl(p.to_f64()),
                        &Atomic::Dbl(q.to_f64()),
                    )?)]),
                    _ => Err("arithmetic needs single values".into()),
                }
            }
            BinOp::Union | BinOp::Intersect | BinOp::Except => {
                let nodes = |v: Seq| -> R<Vec<NodeRef>> {
                    v.into_iter()
                        .map(|i| match i {
                            Item::Node(n) => Ok(n),
                            _ => Err("union, intersect and except need nodes".to_string()),
                        })
                        .collect()
                };
                let x = nodes(self.eval(a, d)?)?;
                let y = nodes(self.eval(b, d)?)?;
                let mut r: Vec<NodeRef> = match op {
                    BinOp::Union => x.into_iter().chain(y).collect(),
                    BinOp::Intersect => x.into_iter().filter(|n| y.contains(n)).collect(),
                    _ => x.into_iter().filter(|n| !y.contains(n)).collect(),
                };
                r.sort();
                r.dedup();
                Ok(r.into_iter().map(Item::Node).collect())
            }
        }
    }

    /// XPath 1.0 comparison (§3.4): a boolean on either side compares
    /// booleans; otherwise each pair of values compares as numbers when
    /// either is a number or the operator is an ordering, else as strings.
    fn compare_1(&self, op: BinOp, x: &Seq, y: &Seq) -> R<bool> {
        let is_bool = |v: &Seq| matches!(v.as_slice(), [Item::Atomic(Atomic::Bool(_))]);
        let ordering = !matches!(op, BinOp::GenEq | BinOp::GenNe);
        let holds_f = |p: f64, q: f64| match op {
            BinOp::GenEq => p == q,
            BinOp::GenNe => p != q,
            BinOp::GenLt => p < q,
            BinOp::GenLe => p <= q,
            BinOp::GenGt => p > q,
            _ => p >= q,
        };
        if is_bool(x) || is_bool(y) {
            let (p, q) = (self.ebv(x)? as u8 as f64, self.ebv(y)? as u8 as f64);
            return Ok(holds_f(p, q));
        }
        let xa = self.atomize(x);
        let ya = self.atomize(y);
        for p in &xa {
            for q in &ya {
                let r = if ordering || p.is_numeric() || q.is_numeric() {
                    holds_f(p.to_f64(), q.to_f64())
                } else {
                    let (s, t) = (p.to_string(), q.to_string());
                    if op == BinOp::GenEq { s == t } else { s != t }
                };
                if r {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    pub fn instance_of(&self, v: &Seq, t: &SeqType) -> bool {
        let n = v.len();
        let card = match t.occ {
            '?' => n <= 1,
            '*' => true,
            '+' => n >= 1,
            _ => n == 1,
        };
        if !card {
            return false;
        }
        v.iter().all(|i| self.item_matches(i, &t.item))
    }

    fn item_matches(&self, i: &Item, t: &ItemType) -> bool {
        match (t, i) {
            (ItemType::Item, _) => true,
            (ItemType::Empty, _) => false,
            (ItemType::Node(nt), Item::Node(n)) => {
                let tr = self.tree(n.tree);
                test(tr.node(n.idx), nt, Kind::Element)
            }
            (ItemType::Atomic(name), Item::Atomic(a)) => match name.as_str() {
                "anyAtomicType" => true,
                "string" => matches!(a, Atomic::Str(_)),
                "untypedAtomic" => matches!(a, Atomic::Untyped(_)),
                "boolean" => matches!(a, Atomic::Bool(_)),
                "integer" => matches!(a, Atomic::Int(_)),
                "decimal" => matches!(a, Atomic::Int(_) | Atomic::Dec(_)),
                "double" => matches!(a, Atomic::Dbl(_)),
                _ => false,
            },
            _ => false,
        }
    }

    /// Function conversion rules (XPath 2.0 §3.1.5) for `as` attributes.
    pub fn convert(&self, v: Seq, t: &SeqType, what: &str) -> R<Seq> {
        let v = match &t.item {
            ItemType::Atomic(name) => {
                let mut out = Vec::new();
                for a in self.atomize(&v) {
                    let a = match (&a, name.as_str()) {
                        (Atomic::Untyped(_), "anyAtomicType") => a,
                        (Atomic::Untyped(_), n) => cast(&a, n)?,
                        (Atomic::Int(_) | Atomic::Dec(_), "double") => Atomic::Dbl(a.to_f64()),
                        (Atomic::Int(i), "decimal") => Atomic::Dec(Decimal::from_int(*i)),
                        _ => a,
                    };
                    out.push(Item::Atomic(a));
                }
                out
            }
            _ => v,
        };
        if !self.instance_of(&v, t) {
            let got: Vec<&str> = v
                .iter()
                .take(3)
                .map(|i| match i {
                    Item::Node(_) => "node()",
                    Item::Atomic(a) => a.type_name(),
                })
                .collect();
            return Err(format!(
                "{what}: required type {t:?}, supplied {} item(s) [{}]",
                v.len(),
                got.join(", ")
            ));
        }
        Ok(v)
    }
}

fn is_reverse(a: &Axis) -> bool {
    matches!(
        a,
        Axis::Ancestor | Axis::AncestorOrSelf | Axis::Preceding | Axis::PrecedingSibling
    )
}

pub fn test(n: &crate::tree::TNode, t: &NodeTest, principal: Kind) -> bool {
    match t {
        NodeTest::AnyKind => true,
        NodeTest::Text => n.kind == Kind::Text,
        NodeTest::Comment => n.kind == Kind::Comment,
        NodeTest::Document => n.kind == Kind::Document,
        NodeTest::Pi(name) => {
            n.kind == Kind::Pi
                && name
                    .as_ref()
                    .is_none_or(|p| *n.name.as_ref().unwrap().local == **p)
        }
        NodeTest::Element(q) => {
            n.kind == Kind::Element && q.as_ref().is_none_or(|q| q.same(n.name.as_ref().unwrap()))
        }
        NodeTest::Attribute(q) => {
            n.kind == Kind::Attribute && q.as_ref().is_none_or(|q| q.same(n.name.as_ref().unwrap()))
        }
        NodeTest::Name { uri, local } => {
            if n.kind != principal {
                return false;
            }
            let q = n.name.as_ref().unwrap();
            uri.as_ref().is_none_or(|u| **u == *q.uri)
                && local.as_ref().is_none_or(|l| **l == *q.local)
        }
    }
}

fn numeric(a: &Atomic) -> R<Atomic> {
    match a {
        Atomic::Untyped(s) => Ok(Atomic::Dbl(parse_double(s))),
        Atomic::Int(_) | Atomic::Dec(_) | Atomic::Dbl(_) => Ok(a.clone()),
        other => Err(format!(
            "arithmetic on {} '{}'",
            other.type_name(),
            other.to_string()
        )),
    }
}

fn to_int(a: &Atomic) -> R<i64> {
    match a {
        Atomic::Int(i) => Ok(*i),
        Atomic::Untyped(s) => s
            .trim()
            .parse()
            .map_err(|_| format!("'{s}' is not an integer")),
        other => Err(format!("{} is not an integer", other.type_name())),
    }
}

pub fn arith(op: BinOp, x: &Atomic, y: &Atomic) -> R<Atomic> {
    use Atomic::*;
    if let (Int(a), Int(b)) = (x, y) {
        let r = match op {
            BinOp::Add => a.checked_add(*b),
            BinOp::Sub => a.checked_sub(*b),
            BinOp::Mul => a.checked_mul(*b),
            BinOp::IDiv => {
                if *b == 0 {
                    return Err("integer division by zero".into());
                }
                Some(a / b)
            }
            BinOp::Mod => {
                if *b == 0 {
                    return Err("modulus by zero".into());
                }
                Some(a % b)
            }
            _ => None,
        };
        if let Some(r) = r {
            return Ok(Int(r));
        }
    }
    if matches!(x, Dbl(_)) || matches!(y, Dbl(_)) {
        let (a, b) = (x.to_f64(), y.to_f64());
        return Ok(match op {
            BinOp::Add => Dbl(a + b),
            BinOp::Sub => Dbl(a - b),
            BinOp::Mul => Dbl(a * b),
            BinOp::Div => Dbl(a / b),
            BinOp::Mod => Dbl(a % b),
            BinOp::IDiv => {
                if b == 0.0 || a.is_nan() || b.is_nan() || a.is_infinite() {
                    return Err("invalid idiv operands".into());
                }
                Int((a / b).trunc() as i64)
            }
            _ => unreachable!(),
        });
    }
    let (a, b) = (to_dec(x).unwrap(), to_dec(y).unwrap());
    let fallback = || arith(op, &Dbl(x.to_f64()), &Dbl(y.to_f64()));
    match op {
        BinOp::Add => a.0.checked_add(b.0).map(|v| Dec(Decimal(v))),
        BinOp::Sub => a.0.checked_sub(b.0).map(|v| Dec(Decimal(v))),
        BinOp::Mul => a.mul(b).map(Dec),
        BinOp::Div => {
            if b.0 == 0 {
                return Err("decimal division by zero".into());
            }
            a.div(b).map(Dec)
        }
        BinOp::IDiv => {
            if b.0 == 0 {
                return Err("integer division by zero".into());
            }
            Some(Int((a.0 / b.0) as i64))
        }
        BinOp::Mod => {
            if b.0 == 0 {
                return Err("modulus by zero".into());
            }
            Some(Dec(Decimal(a.0 % b.0)))
        }
        _ => unreachable!(),
    }
    .map_or_else(fallback, Ok)
}

fn general_cmp(op: BinOp, p: &Atomic, q: &Atomic) -> R<bool> {
    use Atomic::*;
    let (p, q) = match (p, q) {
        (Untyped(s), o) | (o, Untyped(s)) if o.is_numeric() => {
            let d = Dbl(parse_double(s));
            if matches!(p, Untyped(_)) {
                (d, Dbl(q.to_f64()))
            } else {
                (Dbl(p.to_f64()), d)
            }
        }
        (Untyped(a), Untyped(b)) => (Str(a.clone()), Str(b.clone())),
        (Untyped(a), o) | (o, Untyped(a)) if matches!(o, Str(_)) => {
            if matches!(p, Untyped(_)) {
                (Str(a.clone()), o.clone())
            } else {
                (o.clone(), Str(a.clone()))
            }
        }
        (Untyped(a), Bool(_)) => (cast(&Untyped(a.clone()), "boolean")?, q.clone()),
        (Bool(_), Untyped(a)) => (p.clone(), cast(&Untyped(a.clone()), "boolean")?),
        _ => (p.clone(), q.clone()),
    };
    let o = compare(&p, &q)?;
    let op = match op {
        BinOp::GenEq => BinOp::ValEq,
        BinOp::GenNe => BinOp::ValNe,
        BinOp::GenLt => BinOp::ValLt,
        BinOp::GenLe => BinOp::ValLe,
        BinOp::GenGt => BinOp::ValGt,
        _ => BinOp::ValGe,
    };
    Ok(holds(op, o))
}

/// Compares two atomic values of comparable types; None for NaN.
pub fn compare(p: &Atomic, q: &Atomic) -> R<Option<Ordering>> {
    if p.is_numeric() && q.is_numeric() {
        return Ok(cmp_numeric(p, q));
    }
    match (p, q) {
        (a, b) if a.is_stringlike() && b.is_stringlike() => {
            Ok(Some(a.to_string().cmp(&b.to_string())))
        }
        (Atomic::Bool(a), Atomic::Bool(b)) => Ok(Some(a.cmp(b))),
        _ => Err(format!(
            "cannot compare {} '{}' with {} '{}'",
            p.type_name(),
            p.to_string(),
            q.type_name(),
            q.to_string()
        )),
    }
}

fn holds(op: BinOp, o: Option<Ordering>) -> bool {
    let Some(o) = o else {
        return op == BinOp::ValNe;
    };
    match op {
        BinOp::ValEq => o == Ordering::Equal,
        BinOp::ValNe => o != Ordering::Equal,
        BinOp::ValLt => o == Ordering::Less,
        BinOp::ValLe => o != Ordering::Greater,
        BinOp::ValGt => o == Ordering::Greater,
        _ => o != Ordering::Less,
    }
}

/// Casts an atomic value to the named built-in type (local name).
pub fn cast(a: &Atomic, t: &str) -> R<Atomic> {
    let s = a.to_string();
    let bad = || format!("cannot cast '{s}' to xs:{t}");
    Ok(match t {
        "string" => Atomic::str(s),
        "untypedAtomic" => Atomic::Untyped(s.into()),
        "boolean" => match a {
            Atomic::Bool(b) => Atomic::Bool(*b),
            x if x.is_numeric() => Atomic::Bool(x.to_f64() != 0.0 && !x.to_f64().is_nan()),
            _ => match s.trim() {
                "true" | "1" => Atomic::Bool(true),
                "false" | "0" => Atomic::Bool(false),
                _ => return Err(bad()),
            },
        },
        "integer" | "int" | "long" | "short" | "nonNegativeInteger" | "positiveInteger" => {
            match a {
                Atomic::Int(i) => Atomic::Int(*i),
                Atomic::Dec(d) => Atomic::Int((d.0 / crate::value::DEC_SCALE) as i64),
                Atomic::Dbl(d) if d.is_finite() => Atomic::Int(d.trunc() as i64),
                Atomic::Bool(b) => Atomic::Int(*b as i64),
                _ => Atomic::Int(s.trim().parse().map_err(|_| bad())?),
            }
        }
        "decimal" => match a {
            Atomic::Bool(b) => Atomic::Int(*b as i64),
            _ => Atomic::Dec(to_dec(a).ok_or_else(bad)?),
        },
        "double" | "float" => match a {
            Atomic::Str(_) | Atomic::Untyped(_) => {
                let d = parse_double(&s);
                if d.is_nan() && s.trim() != "NaN" {
                    return Err(bad());
                }
                Atomic::Dbl(d)
            }
            _ => Atomic::Dbl(a.to_f64()),
        },
        _ => return Err(format!("xs:{t} is not supported yet")),
    })
}
