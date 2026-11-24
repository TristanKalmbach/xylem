//! The function library: F&O core functions, XSLT additions and
//! constructor functions for the built-in types.

use crate::eval::{R, cast};
use crate::exec::{Dyn, Engine};
use crate::tree::{Kind, NodeRef, QName};
use crate::value::{Atomic, Decimal, Item, Seq};
use crate::xpath::{Expr, FN_NS, XS_NS};

/// `round()` semantics: halves round towards positive infinity.
pub fn round_half_up(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    (x + 0.5).floor()
}

impl Engine<'_> {
    pub fn call(&self, name: &QName, args: &[Expr], d: &Dyn) -> R<Seq> {
        let n = args.len();
        if &*name.uri == XS_NS {
            if n != 1 {
                return Err(format!("xs:{}() takes one argument", name.local));
            }
            let v = self.atomize(&self.eval(&args[0], d)?);
            return match v.as_slice() {
                [] => Ok(vec![]),
                [a] => Ok(vec![Item::Atomic(cast(a, &name.local)?)]),
                _ => Err(format!("xs:{}() needs a single value", name.local)),
            };
        }
        if &*name.uri != FN_NS {
            let mut vals = Vec::with_capacity(n);
            for a in args {
                vals.push(self.eval(a, d)?);
            }
            return self
                .call_user(name, vals)?
                .ok_or_else(|| format!("unknown function {}#{n}", name.lexical()));
        }
        let arg = |i: usize| self.eval(&args[i], d);
        let s = |i: usize| -> R<String> {
            let v = arg(i)?;
            Ok(v.first().map(|x| self.string_of(x)).unwrap_or_default())
        };
        // The context item's string, for one-argument functions called bare.
        let s_or_ctx = |i: usize| -> R<String> {
            if n > i {
                s(i)
            } else {
                Ok(self.string_of(d.item()?))
            }
        };
        let str_ = |x: String| Ok(vec![Item::Atomic(Atomic::str(x))]);
        let bool_ = |b: bool| Ok(vec![Item::Atomic(Atomic::Bool(b))]);
        let int_ = |i: i64| Ok(vec![Item::Atomic(Atomic::Int(i))]);
        let node_arg = |i: usize| -> R<Option<NodeRef>> {
            if n <= i {
                return match d.item()? {
                    Item::Node(x) => Ok(Some(*x)),
                    _ => Err(format!("{}() needs a node", name.local)),
                };
            }
            match arg(i)?.as_slice() {
                [] => Ok(None),
                [Item::Node(x)] => Ok(Some(*x)),
                _ => Err(format!("{}() needs a single node", name.local)),
            }
        };
        match (&*name.local, n) {
            ("true", 0) => bool_(true),
            ("false", 0) => bool_(false),
            ("not", 1) => bool_(!self.ebv(&arg(0)?)?),
            ("boolean", 1) => bool_(self.ebv(&arg(0)?)?),
            ("position", 0) => int_(d.pos as i64),
            ("last", 0) => int_(d.size as i64),
            ("count", 1) => int_(arg(0)?.len() as i64),
            ("empty", 1) => bool_(arg(0)?.is_empty()),
            ("exists", 1) => bool_(!arg(0)?.is_empty()),
            ("current", 0) => Ok(d.current.clone().into_iter().collect()),
            ("string", 0 | 1) => str_(s_or_ctx(0)?),
            ("data", 1) => Ok(self
                .atomize(&arg(0)?)
                .into_iter()
                .map(Item::Atomic)
                .collect()),
            ("concat", _) => {
                let mut out = String::new();
                for i in 0..n {
                    out.push_str(&s(i)?);
                }
                str_(out)
            }
            ("string-length", 0 | 1) => int_(s_or_ctx(0)?.chars().count() as i64),
            ("normalize-space", 0 | 1) => str_(
                s_or_ctx(0)?
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            ("contains", 2) => bool_(s(0)?.contains(&s(1)?)),
            ("starts-with", 2) => bool_(s(0)?.starts_with(&s(1)?)),
            ("ends-with", 2) => bool_(s(0)?.ends_with(&s(1)?)),
            ("substring-before", 2) => {
                let (a, b) = (s(0)?, s(1)?);
                str_(a.find(&b).map(|i| a[..i].to_string()).unwrap_or_default())
            }
            ("substring-after", 2) => {
                let (a, b) = (s(0)?, s(1)?);
                str_(
                    a.find(&b)
                        .map(|i| a[i + b.len()..].to_string())
                        .unwrap_or_default(),
                )
            }
            ("substring", 2 | 3) => {
                let src: Vec<char> = s(0)?.chars().collect();
                let num = |i: usize| -> R<f64> {
                    Ok(self
                        .atomize(&arg(i)?)
                        .first()
                        .map(|a| a.to_f64())
                        .unwrap_or(f64::NAN))
                };
                let start = round_half_up(num(1)?);
                let end = if n == 3 {
                    start + round_half_up(num(2)?)
                } else {
                    f64::INFINITY
                };
                let out: String = src
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        let p = (*i + 1) as f64;
                        p >= start && p < end
                    })
                    .map(|(_, c)| *c)
                    .collect();
                str_(out)
            }
            ("translate", 3) => {
                let from: Vec<char> = s(1)?.chars().collect();
                let to: Vec<char> = s(2)?.chars().collect();
                let out: String = s(0)?
                    .chars()
                    .filter_map(|c| match from.iter().position(|&f| f == c) {
                        Some(i) => to.get(i).copied(),
                        None => Some(c),
                    })
                    .collect();
                str_(out)
            }
            ("upper-case", 1) => str_(s(0)?.to_uppercase()),
            ("lower-case", 1) => str_(s(0)?.to_lowercase()),
            ("string-join", 2) => {
                let parts: Vec<String> = self
                    .atomize(&arg(0)?)
                    .iter()
                    .map(Atomic::to_string)
                    .collect();
                str_(parts.join(&s(1)?))
            }
            ("matches", 2 | 3) => {
                let re = regex(&s(1)?, if n == 3 { s(2)? } else { String::new() })?;
                bool_(re.is_match(&s(0)?))
            }
            ("replace", 3 | 4) => {
                let re = regex(&s(1)?, if n == 4 { s(3)? } else { String::new() })?;
                let rep = replacement(&s(2)?)?;
                str_(re.replace_all(&s(0)?, rep.as_str()).into_owned())
            }
            ("tokenize", 2 | 3) => {
                let src = s(0)?;
                if src.is_empty() {
                    return Ok(vec![]);
                }
                let re = regex(&s(1)?, if n == 3 { s(2)? } else { String::new() })?;
                Ok(re
                    .split(&src)
                    .map(|p| Item::Atomic(Atomic::str(p)))
                    .collect())
            }
            ("name" | "local-name", 0 | 1) => {
                let Some(x) = node_arg(0)? else {
                    return str_(String::new());
                };
                let t = self.tree(x.tree);
                let q = t.node(x.idx).name.clone();
                str_(
                    q.map(|q| {
                        if &*name.local == "name" {
                            q.lexical()
                        } else {
                            q.local.to_string()
                        }
                    })
                    .unwrap_or_default(),
                )
            }
            ("namespace-uri", 0 | 1) => {
                let Some(x) = node_arg(0)? else {
                    return str_(String::new());
                };
                let t = self.tree(x.tree);
                str_(
                    t.node(x.idx)
                        .name
                        .as_ref()
                        .map(|q| q.uri.to_string())
                        .unwrap_or_default(),
                )
            }
            ("root", 0 | 1) => Ok(node_arg(0)?
                .map(|x| {
                    Item::Node(NodeRef {
                        tree: x.tree,
                        idx: 0,
                    })
                })
                .into_iter()
                .collect()),
            ("generate-id", 0 | 1) => {
                let Some(x) = node_arg(0)? else {
                    return str_(String::new());
                };
                // Document number then node number, like `d1e23`.
                let t = self.tree(x.tree);
                let kind = match t.node(x.idx).kind {
                    Kind::Text => 't',
                    Kind::Comment => 'c',
                    Kind::Pi => 'p',
                    _ => 'e',
                };
                let number = t.node_number(x.idx);
                str_(format!("d{}{kind}{number}", x.tree + 1))
            }
            ("number", 0 | 1) => {
                let v = if n == 0 {
                    vec![d.item()?.clone()]
                } else {
                    arg(0)?
                };
                let f = self
                    .atomize(&v)
                    .first()
                    .map(|a| a.to_f64())
                    .unwrap_or(f64::NAN);
                Ok(vec![Item::Atomic(Atomic::Dbl(f))])
            }
            ("sum", 1 | 2) => {
                let v = self.atomize(&arg(0)?);
                if v.is_empty() {
                    return if n == 2 { arg(1) } else { int_(0) };
                }
                let mut acc = numeric_of(&v[0]);
                for a in &v[1..] {
                    acc = crate::eval::arith(crate::xpath::BinOp::Add, &acc, &numeric_of(a))?;
                }
                Ok(vec![Item::Atomic(acc)])
            }
            ("max" | "min", 1) => {
                let v = self.atomize(&arg(0)?);
                let mut best: Option<Atomic> = None;
                for a in v {
                    let a = match a {
                        Atomic::Untyped(s) => Atomic::Dbl(crate::value::parse_double(&s)),
                        o => o,
                    };
                    best = Some(match best {
                        None => a,
                        Some(b) => {
                            let o = crate::eval::compare(&a, &b)?;
                            let better = if &*name.local == "max" {
                                o == Some(std::cmp::Ordering::Greater)
                            } else {
                                o == Some(std::cmp::Ordering::Less)
                            };
                            if better { a } else { b }
                        }
                    });
                }
                Ok(best.map(Item::Atomic).into_iter().collect())
            }
            ("avg", 1) => {
                let v = self.atomize(&arg(0)?);
                if v.is_empty() {
                    return Ok(vec![]);
                }
                let mut acc = numeric_of(&v[0]);
                for a in &v[1..] {
                    acc = crate::eval::arith(crate::xpath::BinOp::Add, &acc, &numeric_of(a))?;
                }
                let r = crate::eval::arith(
                    crate::xpath::BinOp::Div,
                    &acc,
                    &Atomic::Int(v.len() as i64),
                )?;
                Ok(vec![Item::Atomic(r)])
            }
            ("round" | "floor" | "ceiling" | "abs", 1) => {
                let v = self.atomize(&arg(0)?);
                let Some(a) = v.first() else {
                    return Ok(vec![]);
                };
                let a = numeric_of(a);
                let f = |x: f64| match &*name.local {
                    "round" => round_half_up(x),
                    "floor" => x.floor(),
                    "ceiling" => x.ceil(),
                    _ => x.abs(),
                };
                Ok(vec![Item::Atomic(match a {
                    Atomic::Int(i) => Atomic::Int(if &*name.local == "abs" { i.abs() } else { i }),
                    Atomic::Dec(x) => Atomic::Dec(Decimal::from_f64(f(x.to_f64())).unwrap_or(x)),
                    o => Atomic::Dbl(f(o.to_f64())),
                })])
            }
            ("distinct-values", 1) => {
                let mut out: Vec<Atomic> = Vec::new();
                for a in self.atomize(&arg(0)?) {
                    let a = match a {
                        Atomic::Untyped(s) => Atomic::Str(s),
                        o => o,
                    };
                    let dup = out.iter().any(|b| {
                        crate::eval::compare(&a, b).ok().flatten()
                            == Some(std::cmp::Ordering::Equal)
                    });
                    if !dup {
                        out.push(a);
                    }
                }
                Ok(out.into_iter().map(Item::Atomic).collect())
            }
            ("reverse", 1) => {
                let mut v = arg(0)?;
                v.reverse();
                Ok(v)
            }
            ("subsequence", 2 | 3) => {
                let v = arg(0)?;
                let st = round_half_up(
                    self.atomize(&arg(1)?)
                        .first()
                        .map(|a| a.to_f64())
                        .unwrap_or(f64::NAN),
                );
                let len = if n == 3 {
                    round_half_up(
                        self.atomize(&arg(2)?)
                            .first()
                            .map(|a| a.to_f64())
                            .unwrap_or(f64::NAN),
                    )
                } else {
                    f64::INFINITY
                };
                Ok(v.into_iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        let p = (*i + 1) as f64;
                        p >= st && p < st + len
                    })
                    .map(|(_, x)| x)
                    .collect())
            }
            ("index-of", 2) => {
                let v = self.atomize(&arg(0)?);
                let k = self.atomize(&arg(1)?);
                let Some(k) = k.first() else {
                    return Ok(vec![]);
                };
                Ok(v.iter()
                    .enumerate()
                    .filter(|(_, a)| {
                        crate::eval::compare(a, k).ok().flatten() == Some(std::cmp::Ordering::Equal)
                    })
                    .map(|(i, _)| Item::Atomic(Atomic::Int(i as i64 + 1)))
                    .collect())
            }
            ("exactly-one" | "one-or-more" | "zero-or-one", 1) => {
                let v = arg(0)?;
                let ok = match &*name.local {
                    "exactly-one" => v.len() == 1,
                    "one-or-more" => !v.is_empty(),
                    _ => v.len() <= 1,
                };
                if ok {
                    Ok(v)
                } else {
                    Err(format!("{}() got {} items", name.local, v.len()))
                }
            }
            ("key", 2 | 3) => {
                let kn = s(0)?;
                let (p, l) = crate::tree::split(kn.trim());
                let q = if p.is_empty() {
                    QName::local(l)
                } else {
                    // Prefixed key names: match by local name and prefix.
                    self.ss
                        .keys
                        .keys()
                        .find(|k| &*k.local == l && &*k.prefix == p)
                        .cloned()
                        .unwrap_or(QName::local(l))
                };
                let vals = self.atomize(&arg(1)?);
                let tree = match node_arg(2)? {
                    Some(x) if n == 3 => x.tree,
                    _ => match d.item()? {
                        Item::Node(x) => x.tree,
                        _ => return Err("key() needs a node as the context item".into()),
                    },
                };
                Ok(self
                    .key(&q, &vals, tree)?
                    .into_iter()
                    .map(Item::Node)
                    .collect())
            }
            ("document" | "doc", 1 | 2) => {
                let v = arg(0)?;
                let base = self.ss.base.to_string_lossy().to_string();
                let mut out = Vec::new();
                for it in &v {
                    let (href, b) = match it {
                        Item::Node(x) => (self.string_of(it), self.base_of(*x)),
                        Item::Atomic(a) => (a.to_string(), base.clone()),
                    };
                    let href = href.trim().to_string();
                    let t = if href.is_empty() {
                        None
                    } else {
                        self.document(&href, &b)?
                    };
                    if let Some(t) = t {
                        out.push(Item::Node(NodeRef { tree: t, idx: 0 }));
                    }
                }
                Ok(out)
            }
            ("format-number", 2 | 3) => {
                let v = self.atomize(&arg(0)?);
                let x = v.first().map(|a| a.to_f64()).unwrap_or(f64::NAN);
                str_(crate::number::format_number(x, &s(1)?))
            }
            ("system-property", 1) => str_(match s(0)?.trim() {
                "xsl:version" => "2.0".into(),
                "xsl:vendor" => "Xylem".into(),
                _ => String::new(),
            }),
            ("function-available" | "element-available", 1 | 2) => bool_(false),
            ("error", _) => Err(format!(
                "fn:error() called{}",
                if n > 1 {
                    format!(": {}", s(1)?)
                } else {
                    String::new()
                }
            )),
            ("lang", 1) => {
                let want = s(0)?.to_lowercase();
                let Item::Node(x) = d.item()? else {
                    return bool_(false);
                };
                let t = self.tree(x.tree);
                let mut cur = Some(x.idx);
                while let Some(c) = cur {
                    for &a in &t.node(c).attrs {
                        let an = t.node(a);
                        let q = an.name.as_ref().unwrap();
                        if &*q.uri == crate::tree::XML_NS && &*q.local == "lang" {
                            let l = an.value.to_lowercase();
                            return bool_(l == want || l.starts_with(&format!("{want}-")));
                        }
                    }
                    cur = t.node(c).parent;
                }
                bool_(false)
            }
            ("node-name", 1) => {
                let Some(x) = node_arg(0)? else {
                    return Ok(vec![]);
                };
                let t = self.tree(x.tree);
                let k = t.node(x.idx).kind;
                if matches!(k, Kind::Element | Kind::Attribute) {
                    str_(t.node(x.idx).name.as_ref().unwrap().lexical())
                } else {
                    Ok(vec![])
                }
            }
            _ => Err(format!("function {}#{n} is not supported yet", name.local)),
        }
    }
}

/// Converts an XPath replacement string (`$1`, `\$`, `\\`) to the `regex`
/// crate's syntax (`${1}`, `$$`, `\`).
fn replacement(r: &str) -> R<String> {
    let c: Vec<char> = r.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let bad = || format!("invalid replacement string '{r}'");
    while i < c.len() {
        match c[i] {
            '\\' => {
                match c.get(i + 1) {
                    Some('$') => out.push_str("$$"),
                    Some('\\') => out.push('\\'),
                    _ => return Err(bad()),
                }
                i += 2;
            }
            '$' => {
                let st = i + 1;
                let mut j = st;
                while j < c.len() && c[j].is_ascii_digit() {
                    j += 1;
                }
                if j == st {
                    return Err(bad());
                }
                out.push_str("${");
                out.extend(&c[st..j]);
                out.push('}');
                i = j;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn numeric_of(a: &Atomic) -> Atomic {
    match a {
        Atomic::Untyped(s) | Atomic::Str(s) => Atomic::Dbl(crate::value::parse_double(s)),
        o => o.clone(),
    }
}

/// Builds a regex from an XPath pattern and flags (s, m, i, x).
///
/// XPath regexes are XML Schema regexes plus anchors and back-references;
/// the common subset maps directly onto the `regex` crate's syntax.
fn regex(pattern: &str, flags: String) -> R<regex::Regex> {
    let mut b = regex::RegexBuilder::new(pattern);
    for f in flags.chars() {
        match f {
            's' => {
                b.dot_matches_new_line(true);
            }
            'm' => {
                b.multi_line(true);
            }
            'i' => {
                b.case_insensitive(true);
            }
            'x' => {
                b.ignore_whitespace(true);
            }
            _ => return Err(format!("unknown regex flag '{f}'")),
        }
    }
    b.build()
        .map_err(|e| format!("invalid regular expression '{pattern}': {e}"))
}
