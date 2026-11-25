//! The function library: F&O core functions, XSLT additions and
//! constructor functions for the built-in types.

use crate::eval::{R, cast};
use crate::exec::{Dyn, Engine};
use crate::tree::{Kind, NodeRef, QName};
use crate::value::{Atomic, Decimal, Item, Seq};
use crate::xpath::{Expr, FN_NS, XS_NS};
use crate::xsd_regex::XRegex;
use std::sync::Arc;

/// `round()` semantics: halves round towards positive infinity.
pub fn round_half_up(x: f64) -> f64 {
    if !x.is_finite() {
        return x;
    }
    // Halves round up; a negative value that rounds to zero stays -0.
    let r = (x + 0.5).floor();
    if r == 0.0 && x.is_sign_negative() {
        -0.0
    } else {
        r
    }
}

impl Engine<'_> {
    /// The folder or file a relative URI in `document()`, `doc()` or
    /// `doc-available()` resolves against: the calling module, when the
    /// parser bound its URI as an extra `xs:anyURI` argument, otherwise the
    /// top module's folder.
    fn module_base(&self, args: &[Expr], n: usize) -> R<String> {
        if n == 2
            && let Expr::Literal(Atomic::Sub("anyURI", u)) = &args[1]
            && let Some(p) = crate::uri::to_path(&u.to_string())
        {
            return Ok(p.to_string_lossy().to_string());
        }
        Ok(self.ss.base.to_string_lossy().to_string())
    }

    /// XPTY0004 for a sequence given where a function takes one item,
    /// worded as the reference processor words it.
    fn too_many_items(&self, v: &Seq, arg: usize, function: &str) -> String {
        const ORDINALS: [&str; 6] = ["first", "second", "third", "fourth", "fifth", "sixth"];
        let which = ORDINALS.get(arg).copied().unwrap_or("next");
        let mut shown: Vec<String> = v
            .iter()
            .take(2)
            .map(|it| match it {
                Item::Atomic(a) if !a.is_stringlike() => a.to_string(),
                it => format!("\"{}\"", self.string_of(it)),
            })
            .collect();
        if v.len() > 2 {
            shown.push("...".into());
        }
        format!(
            "XPTY0004: A sequence of more than one item is not allowed as the {which} argument of {function}() ({})",
            shown.join(", ")
        )
    }

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
        // A single-item string argument. XSLT 1.0 rules take the first item
        // of a longer sequence; XSLT 2.0 refuses it, with the reference
        // processor's message.
        let s = |i: usize| -> R<String> {
            let v = arg(i)?;
            if v.len() > 1 && !crate::value::compat() {
                return Err(self.too_many_items(&v, i, &name.local));
            }
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
            ("current-group", 0) => Ok(d.group.as_ref().map(|g| g.0.clone()).unwrap_or_default()),
            ("current-grouping-key", 0) => Ok(d
                .group
                .as_ref()
                .and_then(|g| g.1.clone())
                .map(Item::Atomic)
                .into_iter()
                .collect()),
            ("regex-group", 1) => {
                let k = match self.atomize(&arg(0)?).first() {
                    Some(Atomic::Int(i)) => *i,
                    Some(o) => o.to_string().trim().parse().unwrap_or(-1),
                    None => -1,
                };
                let g = usize::try_from(k).ok().and_then(|k| d.groups.get(k));
                str_(g.cloned().unwrap_or_default())
            }
            ("string-to-codepoints", 1) => Ok(s(0)?
                .chars()
                .map(|c| Item::Atomic(Atomic::Int(c as i64)))
                .collect()),
            ("codepoints-to-string", 1) => {
                let mut out = String::new();
                for a in self.atomize(&arg(0)?) {
                    let cp = match a {
                        Atomic::Int(i) => i,
                        o => o.to_string().trim().parse().map_err(|_| {
                            format!(
                                "XPTY0004: codepoints-to-string needs integers, got '{}'",
                                o.to_string()
                            )
                        })?,
                    };
                    let c = u32::try_from(cp)
                        .ok()
                        .and_then(char::from_u32)
                        .filter(|&c| crate::xsd_regex::is_xml_char(c))
                        .ok_or_else(|| format!("FOCH0001: {cp} is not a valid XML character"))?;
                    out.push(c);
                }
                str_(out)
            }
            ("string-length", 0 | 1) => int_(s_or_ctx(0)?.chars().count() as i64),
            ("normalize-space", 0 | 1) => str_(
                // XML whitespace only: a no-break space is kept.
                s_or_ctx(0)?
                    .split(crate::tree::is_xml_space)
                    .filter(|w| !w.is_empty())
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
                let re = regex(
                    self.xpath30(),
                    &s(1)?,
                    if n == 3 { s(2)? } else { String::new() },
                )?;
                bool_(re.is_match(&s(0)?)?)
            }
            ("replace", 3 | 4) => {
                let re = regex(
                    self.xpath30(),
                    &s(1)?,
                    if n == 4 { s(3)? } else { String::new() },
                )?;
                no_empty_match(&re)?;
                let rep = replacement(&s(2)?, re.groups())?;
                str_(re.replace_all(&s(0)?, rep.as_str()).into_owned())
            }
            ("tokenize", 2 | 3) => {
                let src = s(0)?;
                if src.is_empty() {
                    return Ok(vec![]);
                }
                let re = regex(
                    self.xpath30(),
                    &s(1)?,
                    if n == 3 { s(2)? } else { String::new() },
                )?;
                no_empty_match(&re)?;
                Ok(re
                    .split(&src)?
                    .into_iter()
                    .map(|p| Item::Atomic(Atomic::str(p)))
                    .collect())
            }
            ("lang", 1 | 2) => {
                // The nearest xml:lang on the node or its ancestors, equal to
                // the argument or a sub-language of it, ignoring case.
                let want = s(0)?.to_ascii_lowercase();
                let Some(x) = node_arg(1)? else {
                    return bool_(false);
                };
                let t = self.tree(x.tree);
                let mut cur = Some(x.idx);
                while let Some(i) = cur {
                    let found = t.node(i).attrs.iter().map(|&a| t.node(a)).find(|a| {
                        a.name
                            .as_ref()
                            .is_some_and(|q| &*q.uri == crate::tree::XML_NS && &*q.local == "lang")
                    });
                    if let Some(a) = found {
                        let have = a.value.trim().to_ascii_lowercase();
                        return bool_(
                            have == want
                                || have.strip_prefix(&want).is_some_and(|r| r.starts_with('-')),
                        );
                    }
                    cur = t.node(i).parent;
                }
                bool_(false)
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
                let doc = x.tree + 1 + self.ss.extra_modules;
                if t.node(x.idx).kind == Kind::Document {
                    return str_(format!("d{doc}"));
                }
                str_(format!("d{doc}{kind}{number}"))
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
                // The numeric type all values promote to (0 none, 1 decimal,
                // 2 float, 3 double), and whether a NaN was seen.
                let mut widest = 0;
                let mut nan = false;
                for a in v {
                    let a = match a {
                        Atomic::Untyped(_) => cast(&a, "double")?,
                        o => o,
                    };
                    if a.is_numeric() {
                        let rank = match &a {
                            Atomic::Dbl(_) => 3,
                            Atomic::Sub("float", _) => 2,
                            Atomic::Dec(_) => 1,
                            _ => 0,
                        };
                        widest = widest.max(rank);
                        nan |= a.to_f64().is_nan();
                    }
                    if nan {
                        best = Some(a);
                        continue;
                    }
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
                let best = match best {
                    Some(b) if b.is_numeric() => Some(match widest {
                        3 if nan => Atomic::Dbl(f64::NAN),
                        2 if nan => cast(&Atomic::Dbl(f64::NAN), "float")?,
                        3 => cast(&b, "double")?,
                        2 => cast(&b, "float")?,
                        1 if !matches!(b, Atomic::Dec(_)) => cast(&b, "decimal")?,
                        _ => b,
                    }),
                    o => o,
                };
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
                if !a.is_numeric() && !matches!(a, Atomic::Untyped(_)) {
                    return Err(format!(
                        "XPTY0004: {}() needs a numeric value, not {}",
                        name.local,
                        a.type_name()
                    ));
                }
                let a = numeric_of(a);
                let op = &*name.local;
                let f = |x: f64| match op {
                    "round" => round_half_up(x),
                    "floor" => x.floor(),
                    "ceiling" => x.ceil(),
                    _ => x.abs(),
                };
                const S: i128 = crate::value::DEC_SCALE;
                Ok(vec![Item::Atomic(match a {
                    Atomic::Int(i) if op == "abs" => i
                        .checked_abs()
                        .map(Atomic::Int)
                        .unwrap_or_else(|| Atomic::Dec(Decimal(-(i as i128) * S))),
                    Atomic::Int(i) => Atomic::Int(i),
                    Atomic::Dec(x) => Atomic::Dec(Decimal(match op {
                        "round" => (x.0 + S / 2).div_euclid(S) * S,
                        "floor" => x.0.div_euclid(S) * S,
                        "ceiling" => -(-x.0).div_euclid(S) * S,
                        _ => x.0.abs(),
                    })),
                    Atomic::Sub("float", v) => {
                        Atomic::Sub("float", Arc::new(Atomic::Dbl(f(v.to_f64()) as f32 as f64)))
                    }
                    o => Atomic::Dbl(f(o.to_f64())),
                })])
            }
            ("current-dateTime" | "current-date" | "current-time", 0) => {
                let now = crate::datetime::now();
                let kind = match &*name.local {
                    "current-date" => crate::datetime::DtKind::Date,
                    "current-time" => crate::datetime::DtKind::Time,
                    _ => crate::datetime::DtKind::DateTime,
                };
                let v = now.convert(kind).unwrap_or(now);
                Ok(vec![Item::Atomic(Atomic::Time(Arc::new(v)))])
            }
            ("format-date" | "format-dateTime" | "format-time", 2 | 5) => {
                let v = self.atomize(&arg(0)?);
                let Some(a) = v.first() else {
                    return Ok(vec![]);
                };
                let want = match &*name.local {
                    "format-date" => crate::datetime::DtKind::Date,
                    "format-time" => crate::datetime::DtKind::Time,
                    _ => crate::datetime::DtKind::DateTime,
                };
                let value = match a {
                    Atomic::Time(t) if t.kind == want => (**t).clone(),
                    Atomic::Untyped(_) | Atomic::Str(_) => {
                        return Err(format!(
                            "XPTY0004: {}() needs an xs:{}, not a string",
                            name.local,
                            want.name()
                        ));
                    }
                    other => {
                        return Err(format!(
                            "XPTY0004: {}() needs an xs:{}, not an xs:{}",
                            name.local,
                            want.name(),
                            other.type_name()
                        ));
                    }
                };
                let picture = s(1)?;
                let (lang, cal) = if n == 5 {
                    (Some(s(2)?), Some(s(3)?))
                } else {
                    (None, None)
                };
                str_(crate::format_date::format(
                    &value,
                    &picture,
                    lang.as_deref(),
                    cal.as_deref(),
                )?)
            }
            ("implicit-timezone", 0) => Ok(vec![Item::Atomic(crate::datetime::tz_duration(
                crate::datetime::IMPLICIT_TZ,
            ))]),
            ("dateTime", 2) => {
                let (x, y) = (self.atomize(&arg(0)?), self.atomize(&arg(1)?));
                let (Some(x), Some(y)) = (x.first(), y.first()) else {
                    return Ok(vec![]);
                };
                let d = cast(x, "date")?;
                let t = cast(y, "time")?;
                let (Atomic::Time(d), Atomic::Time(t)) = (&d, &t) else {
                    return Err("XPTY0004: dateTime() needs a date and a time".into());
                };
                let v = crate::datetime::combine(d, t)?;
                Ok(vec![Item::Atomic(Atomic::Time(Arc::new(v)))])
            }
            (
                "adjust-dateTime-to-timezone"
                | "adjust-date-to-timezone"
                | "adjust-time-to-timezone",
                1 | 2,
            ) => {
                let v = self.atomize(&arg(0)?);
                let Some(Atomic::Time(t)) = v.first() else {
                    return if v.is_empty() {
                        Ok(vec![])
                    } else {
                        Err("XPTY0004: adjust-*-to-timezone needs a date/time".into())
                    };
                };
                let tz = if n == 2 {
                    match self.atomize(&arg(1)?).first() {
                        None => None,
                        Some(Atomic::Dur(d)) => Some(crate::datetime::duration_tz(d)?),
                        Some(_) => {
                            return Err("XPTY0004: the timezone must be a dayTimeDuration".into());
                        }
                    }
                } else {
                    Some(crate::datetime::IMPLICIT_TZ)
                };
                Ok(vec![Item::Atomic(Atomic::Time(Arc::new(t.adjust(tz))))])
            }
            (f, 1) if component_fn(f).is_some() => {
                let v = self.atomize(&arg(0)?);
                let Some(a) = v.first() else {
                    return Ok(vec![]);
                };
                let (which, want) = component_fn(f).unwrap_or_default();
                if !crate::types::instance(a, want) {
                    return Err(format!(
                        "XPTY0004: {f}() needs an xs:{want}, got {}",
                        a.type_name()
                    ));
                }
                Ok(crate::datetime::component(a, which)
                    .map(Item::Atomic)
                    .into_iter()
                    .collect())
            }
            ("QName", 2) => {
                let uri = s(0)?;
                let lex = s(1)?;
                let (prefix, local) = lex.split_once(':').unwrap_or(("", &lex));
                let names_ok = crate::types::is_ncname(local)
                    && (!lex.contains(':') || crate::types::is_ncname(prefix));
                if !names_ok || (!prefix.is_empty() && uri.is_empty()) {
                    return Err(format!("FOCA0002: invalid QName '{lex}'"));
                }
                Ok(vec![Item::Atomic(Atomic::QName(Arc::new((
                    uri.as_str().into(),
                    prefix.into(),
                    local.into(),
                ))))])
            }
            ("local-name-from-QName" | "prefix-from-QName" | "namespace-uri-from-QName", 1) => {
                let v = self.atomize(&arg(0)?);
                let Some(a) = v.first() else {
                    return Ok(vec![]);
                };
                let Atomic::QName(q) = a else {
                    return Err(format!("XPTY0004: {}() needs an xs:QName", name.local));
                };
                match &*name.local {
                    "local-name-from-QName" => str_(q.2.to_string()),
                    "namespace-uri-from-QName" => Ok(vec![Item::Atomic(Atomic::Sub(
                        "anyURI",
                        Arc::new(Atomic::str(q.0.clone())),
                    ))]),
                    _ if q.1.is_empty() => Ok(vec![]),
                    _ => str_(q.1.to_string()),
                }
            }
            ("round-half-to-even", 1 | 2) => {
                let v = self.atomize(&arg(0)?);
                let Some(a) = v.first() else {
                    return Ok(vec![]);
                };
                let p = if n == 2 {
                    match self.atomize(&arg(1)?).first() {
                        Some(Atomic::Int(i)) => *i,
                        Some(o) => o.to_f64() as i64,
                        None => 0,
                    }
                } else {
                    0
                };
                if !a.is_numeric() && !matches!(a, Atomic::Untyped(_)) {
                    return Err(format!(
                        "XPTY0004: round-half-to-even() needs a numeric value, not {}",
                        a.type_name()
                    ));
                }
                let r = match numeric_of(a) {
                    Atomic::Sub("float", v) => {
                        let r = round_half_even(&Atomic::Dbl(v.to_f64() as f32 as f64), p);
                        Atomic::Sub("float", Arc::new(Atomic::Dbl(r.to_f64() as f32 as f64)))
                    }
                    o => round_half_even(&o, p),
                };
                Ok(vec![Item::Atomic(r)])
            }
            ("remove", 2) => {
                let mut v = arg(0)?;
                let k = self.atomize(&arg(1)?).first().map_or(0.0, Atomic::to_f64);
                if k >= 1.0 && (k as usize) <= v.len() {
                    v.remove(k as usize - 1);
                }
                Ok(v)
            }
            ("insert-before", 3) => {
                let mut v = arg(0)?;
                let k = self.atomize(&arg(1)?).first().map_or(0.0, Atomic::to_f64);
                let at = (k.max(1.0) as usize - 1).min(v.len());
                let ins = arg(2)?;
                v.splice(at..at, ins);
                Ok(v)
            }
            ("unordered", 1) => arg(0),
            ("compare", 2 | 3) | ("codepoint-equal", 2) => {
                let (x, y) = (self.atomize(&arg(0)?), self.atomize(&arg(1)?));
                let (Some(x), Some(y)) = (x.first(), y.first()) else {
                    return Ok(vec![]);
                };
                let o = x.to_string().cmp(&y.to_string());
                if &*name.local == "codepoint-equal" {
                    bool_(o == std::cmp::Ordering::Equal)
                } else {
                    int_(o as i64)
                }
            }
            ("encode-for-uri" | "iri-to-uri" | "escape-html-uri", 1) => {
                let keep: fn(char) -> bool = match &*name.local {
                    "encode-for-uri" => |c| c.is_ascii_alphanumeric() || "-_.~".contains(c),
                    "iri-to-uri" => {
                        |c| c.is_ascii() && !c.is_ascii_control() && !" <>\"{}|\\^`".contains(c)
                    }
                    _ => |c| c.is_ascii() && (' '..='~').contains(&c),
                };
                let mut out = String::new();
                for c in s(0)?.chars() {
                    if keep(c) {
                        out.push(c);
                    } else {
                        let mut b = [0; 4];
                        for x in c.encode_utf8(&mut b).bytes() {
                            out.push_str(&format!("%{x:02X}"));
                        }
                    }
                }
                str_(out)
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
                let base = self.module_base(args, n)?;
                let mut out = Vec::new();
                for it in &v {
                    let (href, b) = match it {
                        Item::Node(x) => (self.string_of(it), self.base_of(*x)),
                        Item::Atomic(a) => (a.to_string(), base.clone()),
                    };
                    let href = href.trim().to_string();
                    match self.document(&href, &b)? {
                        Ok(t) => out.push(Item::Node(NodeRef { tree: t, idx: 0 })),
                        // `doc()` cannot recover from a missing document.
                        Err(why) if &*name.local == "doc" => {
                            return Err(format!("FODC0002: doc('{href}'): {why}"));
                        }
                        // The XSLT 1.0 processor fails the transformation, as
                        // the reference host shows it.
                        Err(why) if crate::value::compat() => {
                            return Err(format!(
                                "An error occurred while loading document '{href}'. See InnerException for a complete description of the error. / {why}"
                            ));
                        }
                        // XSLT 2.0 recovers with an empty sequence (FODC0002 is
                        // recoverable for document()), as the reference
                        // processor does, and warns.
                        Err(why) => self.warnings.borrow_mut().push(format!(
                            "FODC0002: document('{href}') could not be read, so it gave an empty sequence: {why}"
                        )),
                    }
                }
                Ok(out)
            }
            ("base-uri", 0 | 1) => {
                let Some(x) = node_arg(0)? else {
                    return Ok(vec![]);
                };
                let u = self.base_uri(x)?;
                Ok(vec![Item::Atomic(Atomic::Sub(
                    "anyURI",
                    Arc::new(Atomic::str(u)),
                ))])
            }
            ("document-uri", 1) => {
                let Some(x) = node_arg(0)? else {
                    return Ok(vec![]);
                };
                Ok(self
                    .document_uri(x)
                    .map(|u| Item::Atomic(Atomic::Sub("anyURI", Arc::new(Atomic::str(u)))))
                    .into_iter()
                    .collect())
            }
            ("static-base-uri", 0) => {
                // Bound when parsing inside a stylesheet; standalone XPath
                // falls back to the top module.
                let u = crate::uri::from_path(&self.ss.path);
                Ok(vec![Item::Atomic(Atomic::Sub(
                    "anyURI",
                    Arc::new(Atomic::str(u)),
                ))])
            }
            ("resolve-uri", 1 | 2) => {
                let rel = arg(0)?;
                let Some(rel) = rel.first() else {
                    return Ok(vec![]);
                };
                let rel = self.string_of(rel);
                let base = if n == 2 {
                    s(1)?
                } else {
                    crate::uri::from_path(&self.ss.path)
                };
                let u = crate::uri::resolve(rel.trim(), base.trim())?;
                Ok(vec![Item::Atomic(Atomic::Sub(
                    "anyURI",
                    Arc::new(Atomic::str(u)),
                ))])
            }
            ("deep-equal", 2 | 3) => {
                let (x, y) = (arg(0)?, arg(1)?);
                bool_(self.deep_equal(&x, &y))
            }
            ("doc-available", 1 | 2) => {
                let href = s(0)?.trim().to_string();
                let base = self.module_base(args, n)?;
                bool_(!href.is_empty() && matches!(self.document(&href, &base), Ok(Ok(_))))
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

impl Engine<'_> {
    /// Whether XPath 3.0 regex syntax (`(?:...)`, the `q` flag) is allowed:
    /// only for stylesheets of version 3.0 or later.
    pub fn xpath30(&self) -> bool {
        !self.ss.version.starts_with(['1', '2'])
    }

    /// `fn:deep-equal` with the codepoint collation.
    pub fn deep_equal(&self, x: &Seq, y: &Seq) -> bool {
        x.len() == y.len()
            && x.iter().zip(y).all(|(a, b)| match (a, b) {
                (Item::Atomic(p), Item::Atomic(q)) => atomic_equal(p, q),
                (Item::Node(p), Item::Node(q)) => self.node_equal(*p, *q),
                _ => false,
            })
    }

    fn node_equal(&self, p: NodeRef, q: NodeRef) -> bool {
        let (tp, tq) = (self.tree(p.tree), self.tree(q.tree));
        let (a, b) = (tp.node(p.idx), tq.node(q.idx));
        if a.kind != b.kind {
            return false;
        }
        let names = match (&a.name, &b.name) {
            (Some(m), Some(n)) => m.same(n),
            (None, None) => true,
            _ => false,
        };
        match a.kind {
            Kind::Text | Kind::Comment => a.value == b.value,
            Kind::Attribute | Kind::Pi => names && a.value == b.value,
            Kind::Document | Kind::Element => {
                if !names || a.attrs.len() != b.attrs.len() {
                    return false;
                }
                let attrs_match = a.attrs.iter().all(|&i| {
                    let x = tp.node(i);
                    b.attrs.iter().any(|&j| {
                        let y = tq.node(j);
                        matches!((&x.name, &y.name), (Some(m), Some(n)) if m.same(n))
                            && x.value == y.value
                    })
                });
                // Comments and processing instructions are not compared.
                let kids =
                    |t: &crate::tree::Tree, n: &crate::tree::TNode, tree: u32| -> Vec<NodeRef> {
                        n.children
                            .iter()
                            .filter(|&&c| !matches!(t.node(c).kind, Kind::Comment | Kind::Pi))
                            .map(|&idx| NodeRef { tree, idx })
                            .collect()
                    };
                let (ka, kb) = (kids(&tp, a, p.tree), kids(&tq, b, q.tree));
                drop((tp, tq));
                attrs_match
                    && ka.len() == kb.len()
                    && ka.iter().zip(&kb).all(|(&m, &n)| self.node_equal(m, n))
            }
        }
    }
}

/// `eq` for `deep-equal`: NaN equals NaN; values that cannot be compared
/// are not equal.
fn atomic_equal(p: &Atomic, q: &Atomic) -> bool {
    if p.is_numeric() && q.is_numeric() && p.to_f64().is_nan() && q.to_f64().is_nan() {
        return true;
    }
    matches!(
        crate::eval::compare(p, q),
        Ok(Some(std::cmp::Ordering::Equal))
    )
}

/// Converts an XPath replacement string (`$1`, `\$`, `\\`) to the `regex`
/// crate's syntax (`${1}`, `$$`, `\`).
///
/// A reference `$NN` uses as many digits as name an existing group; a
/// reference to a group that does not exist is replaced by nothing.
fn replacement(r: &str, groups: usize) -> R<String> {
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
                // The first digit always counts; further ones only while
                // they still name an existing group.
                let mut n = c[st].to_digit(10).unwrap_or(0) as usize;
                let mut k = st + 1;
                while k < j {
                    let longer = n * 10 + c[k].to_digit(10).unwrap_or(0) as usize;
                    if longer > groups {
                        break;
                    }
                    n = longer;
                    k += 1;
                }
                if n <= groups {
                    out.push_str(&format!("${{{n}}}"));
                }
                i = k;
            }
            ch => {
                out.push(ch);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// The component accessors: function name to (component, argument type).
fn component_fn(f: &str) -> Option<(&'static str, &'static str)> {
    Some(match f {
        "year-from-dateTime" => ("year", "dateTime"),
        "month-from-dateTime" => ("month", "dateTime"),
        "day-from-dateTime" => ("day", "dateTime"),
        "hours-from-dateTime" => ("hours", "dateTime"),
        "minutes-from-dateTime" => ("minutes", "dateTime"),
        "seconds-from-dateTime" => ("seconds", "dateTime"),
        "timezone-from-dateTime" => ("timezone", "dateTime"),
        "year-from-date" => ("year", "date"),
        "month-from-date" => ("month", "date"),
        "day-from-date" => ("day", "date"),
        "timezone-from-date" => ("timezone", "date"),
        "hours-from-time" => ("hours", "time"),
        "minutes-from-time" => ("minutes", "time"),
        "seconds-from-time" => ("seconds", "time"),
        "timezone-from-time" => ("timezone", "time"),
        "years-from-duration" => ("years", "duration"),
        "months-from-duration" => ("months", "duration"),
        "days-from-duration" => ("days", "duration"),
        "hours-from-duration" => ("hours", "duration"),
        "minutes-from-duration" => ("minutes", "duration"),
        "seconds-from-duration" => ("seconds", "duration"),
        _ => return None,
    })
}

/// `fn:round-half-to-even` at `p` decimal places (negative: tens, ...).
fn round_half_even(a: &Atomic, p: i64) -> Atomic {
    // Rounds an integer count of units to a multiple of `unit`, ties to even.
    fn to_even(x: i128, unit: i128) -> i128 {
        let (q, r) = (x.div_euclid(unit), x.rem_euclid(unit));
        let q = match (2 * r).cmp(&unit) {
            std::cmp::Ordering::Greater => q + 1,
            std::cmp::Ordering::Equal if q % 2 != 0 => q + 1,
            _ => q,
        };
        q * unit
    }
    match a {
        Atomic::Int(i) if p < 0 => {
            let unit = 10i128.checked_pow((-p) as u32).unwrap_or(i128::MAX);
            Atomic::Int(i64::try_from(to_even(*i as i128, unit)).unwrap_or(0))
        }
        Atomic::Int(i) => Atomic::Int(*i),
        Atomic::Dec(d) => {
            let digits = 18 - p;
            if digits <= 0 {
                return Atomic::Dec(*d);
            }
            match 10i128.checked_pow(digits as u32) {
                Some(unit) => Atomic::Dec(Decimal(to_even(d.0, unit))),
                None => Atomic::Dec(Decimal(0)),
            }
        }
        o => {
            let x = o.to_f64();
            if !x.is_finite() || x == 0.0 {
                return Atomic::Dbl(x);
            }
            // Round the decimal expansion, not the binary value.
            let Some(d) = Decimal::from_f64(x) else {
                let f = 10f64.powi(p as i32);
                return Atomic::Dbl((x * f).round_ties_even() / f);
            };
            match round_half_even(&Atomic::Dec(d), p) {
                Atomic::Dec(r) => {
                    let v = r.to_f64();
                    Atomic::Dbl(if v == 0.0 { 0.0f64.copysign(x) } else { v })
                }
                other => other,
            }
        }
    }
}

fn numeric_of(a: &Atomic) -> Atomic {
    match a {
        Atomic::Sub("float", _) => a.clone(),
        Atomic::Sub(..) => numeric_of(&a.prim()),
        Atomic::Untyped(s) | Atomic::Str(s) => Atomic::Dbl(crate::value::parse_double(s)),
        o => o.clone(),
    }
}

/// Builds a regex from an XPath pattern and flags (s, m, i, x, q).
fn regex(xpath30: bool, pattern: &str, flags: String) -> R<XRegex> {
    XRegex::with_version(pattern, &flags, xpath30)
}

/// `fn:replace` and `fn:tokenize` reject patterns that match "".
fn no_empty_match(re: &XRegex) -> R<()> {
    if re.is_match("")? {
        return Err("FORX0003: the regular expression matches a zero-length string".into());
    }
    Ok(())
}
