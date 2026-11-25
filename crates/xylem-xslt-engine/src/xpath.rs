//! XPath 2.0 syntax: tokens, AST and a recursive-descent parser.
//!
//! Prefixes are resolved while parsing, so the AST holds expanded names.

use crate::tree::QName;
use crate::value::{Atomic, Decimal};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum Axis {
    Child,
    Descendant,
    DescendantOrSelf,
    Parent,
    Ancestor,
    AncestorOrSelf,
    FollowingSibling,
    PrecedingSibling,
    Following,
    Preceding,
    Attribute,
    SelfAxis,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeTest {
    /// `name`, `p:*` (local None), `*:l` (uri None), `*`.
    Name {
        uri: Option<String>,
        local: Option<String>,
    },
    AnyKind,
    Text,
    Comment,
    Pi(Option<String>),
    Document,
    /// `element()` / `element(name)` and `attribute()` / `attribute(name)`.
    Element(Option<QName>),
    Attribute(Option<QName>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    pub axis: Axis,
    pub test: NodeTest,
    pub preds: Vec<Expr>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BinOp {
    Or,
    And,
    GenEq,
    GenNe,
    GenLt,
    GenLe,
    GenGt,
    GenGe,
    ValEq,
    ValNe,
    ValLt,
    ValLe,
    ValGt,
    ValGe,
    Is,
    Precedes,
    Follows,
    To,
    Add,
    Sub,
    Mul,
    Div,
    IDiv,
    Mod,
    Union,
    Intersect,
    Except,
}

/// `xs:string?`, `item()*`, `element()+` …
#[derive(Clone, Debug, PartialEq)]
pub struct SeqType {
    pub item: ItemType,
    /// '?', '*', '+' or ' ' for exactly one.
    pub occ: char,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ItemType {
    Item,
    Empty,
    Node(NodeTest),
    Atomic(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Literal(Atomic),
    Var(QName),
    Context,
    Root,
    /// `E/step/step…`; `start` None means relative to the context item.
    Path {
        start: Option<Box<Expr>>,
        steps: Vec<PathStep>,
    },
    Filter(Box<Expr>, Vec<Expr>),
    Seq(Vec<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Neg(Box<Expr>),
    If(Box<Expr>, Box<Expr>, Box<Expr>),
    For(Vec<(QName, Expr)>, Box<Expr>),
    Quant(bool, Vec<(QName, Expr)>, Box<Expr>),
    InstanceOf(Box<Expr>, SeqType),
    Cast(Box<Expr>, String, bool),
    Castable(Box<Expr>, String, bool),
    Call(QName, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathStep {
    Axis(Step),
    /// A general primary expression used as a step (`a/string()`, `a/(b|c)`).
    Expr(Expr, Vec<Expr>),
}

pub const FN_NS: &str = "http://www.w3.org/2005/xpath-functions";
pub const XS_NS: &str = "http://www.w3.org/2001/XMLSchema";

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Name(String), // NCName or QName, `p:*`, `*:l`
    Var(String),
    Str(String),
    Int(String),
    Dec(String),
    Dbl(String),
    Sym(&'static str),
    Star,
    AxisName(String),
    Eof,
}

fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    let name_start = |ch: char| ch.is_alphabetic() || ch == '_';
    let name_char = |ch: char| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '\u{B7}');
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }
        if ch == '(' && c.get(i + 1) == Some(&':') {
            let mut depth = 0;
            while i < c.len() {
                if c[i] == '(' && c.get(i + 1) == Some(&':') {
                    depth += 1;
                    i += 2;
                } else if c[i] == ':' && c.get(i + 1) == Some(&')') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            let mut v = String::new();
            i += 1;
            loop {
                match c.get(i) {
                    None => return Err("unterminated string literal".into()),
                    Some(&q) if q == ch => {
                        if c.get(i + 1) == Some(&ch) {
                            v.push(ch);
                            i += 2;
                        } else {
                            i += 1;
                            break;
                        }
                    }
                    Some(&o) => {
                        v.push(o);
                        i += 1;
                    }
                }
            }
            out.push(Tok::Str(v));
            continue;
        }
        if ch.is_ascii_digit() || (ch == '.' && c.get(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let st = i;
            while i < c.len() && c[i].is_ascii_digit() {
                i += 1;
            }
            let mut dec = false;
            if i < c.len() && c[i] == '.' {
                dec = true;
                i += 1;
                while i < c.len() && c[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < c.len() && (c[i] == 'e' || c[i] == 'E') {
                i += 1;
                if i < c.len() && (c[i] == '+' || c[i] == '-') {
                    i += 1;
                }
                while i < c.len() && c[i].is_ascii_digit() {
                    i += 1;
                }
                out.push(Tok::Dbl(c[st..i].iter().collect()));
            } else if dec {
                out.push(Tok::Dec(c[st..i].iter().collect()));
            } else {
                out.push(Tok::Int(c[st..i].iter().collect()));
            }
            continue;
        }
        if ch == '$' {
            i += 1;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            let st = i;
            while i < c.len()
                && (name_char(c[i])
                    || (c[i] == ':' && c.get(i + 1).is_some_and(|&n| name_start(n))))
            {
                i += 1;
            }
            out.push(Tok::Var(c[st..i].iter().collect()));
            continue;
        }
        if name_start(ch) || (ch == '*' && c.get(i + 1) == Some(&':')) {
            let st = i;
            if ch == '*' {
                i += 2;
            }
            while i < c.len() && name_char(c[i]) {
                i += 1;
            }
            if ch != '*' && c.get(i) == Some(&':') {
                if c.get(i + 1) == Some(&':') {
                    out.push(Tok::AxisName(c[st..i].iter().collect()));
                    i += 2;
                    continue;
                } else if c.get(i + 1) == Some(&'*') {
                    i += 2;
                } else if c.get(i + 1).is_some_and(|&n| name_start(n)) {
                    i += 1;
                    while i < c.len() && name_char(c[i]) {
                        i += 1;
                    }
                }
            }
            // `name ::` with whitespace before the axis separator.
            let name: String = c[st..i].iter().collect();
            let mut j = i;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if c.get(j) == Some(&':') && c.get(j + 1) == Some(&':') {
                out.push(Tok::AxisName(name));
                i = j + 2;
                continue;
            }
            out.push(Tok::Name(name));
            continue;
        }
        let two: String = c[i..(i + 2).min(c.len())].iter().collect();
        let sym2 = ["//", "!=", "<=", ">=", "..", "<<", ">>", "::"];
        if let Some(s) = sym2.iter().find(|s| **s == two) {
            out.push(Tok::Sym(s));
            i += 2;
            continue;
        }
        let sym1 = [
            "/", "(", ")", "[", "]", "@", ",", "|", "+", "-", "=", "<", ">", ".", "?",
        ];
        let one = ch.to_string();
        if ch == '*' {
            out.push(Tok::Star);
            i += 1;
            continue;
        }
        if let Some(s) = sym1.iter().find(|s| **s == one) {
            out.push(Tok::Sym(s));
            i += 1;
            continue;
        }
        return Err(format!("unexpected character '{ch}'"));
    }
    out.push(Tok::Eof);
    Ok(out)
}

/// Resolves a namespace prefix ("" for the default element namespace).
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The key a [`Resolver`] answers with the static base URI of the
/// expression (its stylesheet module), when it knows one. No namespace
/// prefix can contain a NUL, so it never collides with one.
pub const STATIC_BASE_KEY: &str = "\0static-base-uri";

struct Parser<'a> {
    t: Vec<Tok>,
    i: usize,
    ns: Resolver<'a>,
}

pub fn parse(src: &str, ns: Resolver) -> Result<Expr, String> {
    let mut p = Parser {
        t: lex(src).map_err(|e| format!("{e} in '{src}'"))?,
        i: 0,
        ns,
    };
    let e = p.expr().map_err(|e| format!("{e} in '{src}'"))?;
    if p.peek() != &Tok::Eof {
        return Err(format!("unexpected {:?} in '{src}'", p.peek()));
    }
    Ok(e)
}

impl Parser<'_> {
    fn peek(&self) -> &Tok {
        &self.t[self.i]
    }
    fn peek_at(&self, n: usize) -> &Tok {
        self.t.get(self.i + n).unwrap_or(&Tok::Eof)
    }
    fn next(&mut self) -> Tok {
        let t = self.t[self.i].clone();
        if self.i < self.t.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn sym(&mut self, s: &str) -> bool {
        if matches!(self.peek(), Tok::Sym(x) if *x == s) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.sym(s) {
            Ok(())
        } else {
            Err(format!("expected '{s}', found {:?}", self.peek()))
        }
    }
    fn kw(&mut self, k: &str) -> bool {
        if matches!(self.peek(), Tok::Name(x) if x == k) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn is_kw(&self, k: &str) -> bool {
        matches!(self.peek(), Tok::Name(x) if x == k)
    }

    fn qname(&self, lexical: &str, default_ns: bool) -> Result<QName, String> {
        let (p, l) = lexical.split_once(':').unwrap_or(("", lexical));
        if p.is_empty() && !default_ns {
            return Ok(QName::local(l));
        }
        let uri = (self.ns)(p).ok_or_else(|| {
            if p.is_empty() {
                String::new()
            } else {
                format!("undeclared prefix '{p}'")
            }
        });
        match uri {
            Ok(u) => Ok(QName::new(&u, l, p)),
            Err(e) if e.is_empty() => Ok(QName::local(l)),
            Err(e) => Err(e),
        }
    }

    fn expr(&mut self) -> Result<Expr, String> {
        let first = self.single()?;
        if !matches!(self.peek(), Tok::Sym(",")) {
            return Ok(first);
        }
        let mut v = vec![first];
        while self.sym(",") {
            v.push(self.single()?);
        }
        Ok(Expr::Seq(v))
    }

    fn bindings(&mut self, sep: &str) -> Result<Vec<(QName, Expr)>, String> {
        let mut v = Vec::new();
        loop {
            let Tok::Var(n) = self.next() else {
                return Err("expected variable".into());
            };
            if !self.kw(sep) {
                return Err(format!("expected '{sep}'"));
            }
            let e = self.single()?;
            v.push((self.qname(&n, false)?, e));
            if !self.sym(",") {
                break;
            }
        }
        Ok(v)
    }

    fn single(&mut self) -> Result<Expr, String> {
        let open = matches!(self.peek_at(1), Tok::Sym("("));
        let var = matches!(self.peek_at(1), Tok::Var(_));
        if self.is_kw("if") && open {
            self.next();
            self.expect("(")?;
            let c = self.expr()?;
            self.expect(")")?;
            if !self.kw("then") {
                return Err("expected 'then'".into());
            }
            let a = self.single()?;
            if !self.kw("else") {
                return Err("expected 'else'".into());
            }
            let b = self.single()?;
            return Ok(Expr::If(Box::new(c), Box::new(a), Box::new(b)));
        }
        if self.is_kw("for") && var {
            self.next();
            let b = self.bindings("in")?;
            if !self.kw("return") {
                return Err("expected 'return'".into());
            }
            return Ok(Expr::For(b, Box::new(self.single()?)));
        }
        if (self.is_kw("some") || self.is_kw("every")) && var {
            let every = self.is_kw("every");
            self.next();
            let b = self.bindings("in")?;
            if !self.kw("satisfies") {
                return Err("expected 'satisfies'".into());
            }
            return Ok(Expr::Quant(every, b, Box::new(self.single()?)));
        }
        self.or()
    }

    fn or(&mut self) -> Result<Expr, String> {
        let mut l = self.and()?;
        while self.kw("or") {
            l = Expr::Bin(BinOp::Or, Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Expr, String> {
        let mut l = self.cmp()?;
        while self.kw("and") {
            l = Expr::Bin(BinOp::And, Box::new(l), Box::new(self.cmp()?));
        }
        Ok(l)
    }
    fn cmp(&mut self) -> Result<Expr, String> {
        let l = self.range()?;
        let op = match self.peek() {
            Tok::Sym("=") => BinOp::GenEq,
            Tok::Sym("!=") => BinOp::GenNe,
            Tok::Sym("<") => BinOp::GenLt,
            Tok::Sym("<=") => BinOp::GenLe,
            Tok::Sym(">") => BinOp::GenGt,
            Tok::Sym(">=") => BinOp::GenGe,
            Tok::Sym("<<") => BinOp::Precedes,
            Tok::Sym(">>") => BinOp::Follows,
            Tok::Name(n) => match n.as_str() {
                "eq" => BinOp::ValEq,
                "ne" => BinOp::ValNe,
                "lt" => BinOp::ValLt,
                "le" => BinOp::ValLe,
                "gt" => BinOp::ValGt,
                "ge" => BinOp::ValGe,
                "is" => BinOp::Is,
                _ => return Ok(l),
            },
            _ => return Ok(l),
        };
        self.next();
        Ok(Expr::Bin(op, Box::new(l), Box::new(self.range()?)))
    }
    fn range(&mut self) -> Result<Expr, String> {
        let l = self.additive()?;
        if self.kw("to") {
            return Ok(Expr::Bin(
                BinOp::To,
                Box::new(l),
                Box::new(self.additive()?),
            ));
        }
        Ok(l)
    }
    fn additive(&mut self) -> Result<Expr, String> {
        let mut l = self.mult()?;
        loop {
            let op = if self.sym("+") {
                BinOp::Add
            } else if self.sym("-") {
                BinOp::Sub
            } else {
                return Ok(l);
            };
            l = Expr::Bin(op, Box::new(l), Box::new(self.mult()?));
        }
    }
    fn mult(&mut self) -> Result<Expr, String> {
        let mut l = self.union()?;
        loop {
            let op = if matches!(self.peek(), Tok::Star) {
                self.next();
                BinOp::Mul
            } else if self.kw("div") {
                BinOp::Div
            } else if self.kw("idiv") {
                BinOp::IDiv
            } else if self.kw("mod") {
                BinOp::Mod
            } else {
                return Ok(l);
            };
            l = Expr::Bin(op, Box::new(l), Box::new(self.union()?));
        }
    }
    fn union(&mut self) -> Result<Expr, String> {
        let mut l = self.intersect()?;
        while self.sym("|") || self.kw("union") {
            l = Expr::Bin(BinOp::Union, Box::new(l), Box::new(self.intersect()?));
        }
        Ok(l)
    }
    fn intersect(&mut self) -> Result<Expr, String> {
        let mut l = self.instance()?;
        loop {
            let op = if self.kw("intersect") {
                BinOp::Intersect
            } else if self.kw("except") {
                BinOp::Except
            } else {
                return Ok(l);
            };
            l = Expr::Bin(op, Box::new(l), Box::new(self.instance()?));
        }
    }
    fn instance(&mut self) -> Result<Expr, String> {
        let l = self.castable()?;
        if self.is_kw("instance") && matches!(self.peek_at(1), Tok::Name(n) if n == "of") {
            self.i += 2;
            return Ok(Expr::InstanceOf(Box::new(l), self.seq_type()?));
        }
        Ok(l)
    }
    fn castable(&mut self) -> Result<Expr, String> {
        let l = self.cast()?;
        if self.is_kw("castable") && matches!(self.peek_at(1), Tok::Name(n) if n == "as") {
            self.i += 2;
            let (t, opt) = self.single_type()?;
            if t == "QName"
                && let Expr::Literal(Atomic::Str(s)) = &l
            {
                return Ok(Expr::Literal(Atomic::Bool(self.qname_value(s).is_ok())));
            }
            return Ok(Expr::Castable(Box::new(l), t, opt));
        }
        Ok(l)
    }
    fn cast(&mut self) -> Result<Expr, String> {
        let l = self.unary()?;
        if self.is_kw("cast") && matches!(self.peek_at(1), Tok::Name(n) if n == "as") {
            self.i += 2;
            let (t, opt) = self.single_type()?;
            if t == "QName"
                && let Expr::Literal(Atomic::Str(s)) = &l
            {
                return Ok(Expr::Literal(self.qname_value(s)?));
            }
            return Ok(Expr::Cast(Box::new(l), t, opt));
        }
        Ok(l)
    }
    fn single_type(&mut self) -> Result<(String, bool), String> {
        let Tok::Name(n) = self.next() else {
            return Err("expected a type name".into());
        };
        let q = self.qname(&n, false)?;
        if &*q.uri == XS_NS && matches!(&*q.local, "NOTATION" | "anyAtomicType") {
            return Err(format!(
                "XPST0080: cannot cast to the abstract type xs:{}",
                q.local
            ));
        }
        Ok((q.local.to_string(), self.sym("?")))
    }

    /// The `xs:QName` value of a string literal cast to `xs:QName`, its
    /// prefix resolved against the static namespaces.
    fn qname_value(&self, s: &str) -> Result<Atomic, String> {
        let lex = s.trim_matches(|c: char| c.is_ascii_whitespace());
        let (p, l) = lex.split_once(':').unwrap_or(("", lex));
        let ncname = |x: &str| {
            let mut c = x.chars();
            c.next().is_some_and(|c| c.is_alphabetic() || c == '_')
                && c.all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | '·'))
        };
        if !ncname(l) || (!p.is_empty() && !ncname(p)) {
            return Err(format!("FORG0001: invalid QName '{lex}'"));
        }
        let uri = match (self.ns)(p) {
            Some(u) => u,
            None if p.is_empty() => String::new(),
            None => return Err(format!("FONS0004: undeclared prefix '{p}'")),
        };
        Ok(Atomic::QName(std::rc::Rc::new((
            uri.into(),
            p.into(),
            l.into(),
        ))))
    }
    pub fn seq_type(&mut self) -> Result<SeqType, String> {
        let item = match self.next() {
            Tok::Name(n) if matches!(self.peek(), Tok::Sym("(")) => {
                self.next();
                let it = match n.as_str() {
                    "item" => ItemType::Item,
                    "empty-sequence" => ItemType::Empty,
                    _ => {
                        self.i -= 2;
                        let test = self.kind_test()?.ok_or("unknown item type")?;
                        return Ok(SeqType {
                            item: ItemType::Node(test),
                            occ: self.occ(),
                        });
                    }
                };
                self.expect(")")?;
                it
            }
            Tok::Name(n) => ItemType::Atomic(self.qname(&n, false)?.local.to_string()),
            t => return Err(format!("expected a sequence type, found {t:?}")),
        };
        Ok(SeqType {
            item,
            occ: self.occ(),
        })
    }
    fn occ(&mut self) -> char {
        if self.sym("?") {
            '?'
        } else if matches!(self.peek(), Tok::Star) {
            self.next();
            '*'
        } else if self.sym("+") {
            '+'
        } else {
            ' '
        }
    }
    fn unary(&mut self) -> Result<Expr, String> {
        if self.sym("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.sym("+") {
            return self.unary();
        }
        self.path()
    }

    fn path(&mut self) -> Result<Expr, String> {
        if self.sym("/") {
            if self.starts_step() {
                let steps = self.rel_steps()?;
                return Ok(Expr::Path {
                    start: Some(Box::new(Expr::Root)),
                    steps,
                });
            }
            return Ok(Expr::Root);
        }
        if self.sym("//") {
            let mut steps = vec![PathStep::Axis(dos())];
            steps.extend(self.rel_steps()?);
            return Ok(Expr::Path {
                start: Some(Box::new(Expr::Root)),
                steps,
            });
        }
        // A primary expression followed by optional predicates and steps.
        if !self.starts_axis_step() {
            let prim = self.primary()?;
            let preds = self.preds()?;
            let base = if preds.is_empty() {
                prim
            } else {
                Expr::Filter(Box::new(prim), preds)
            };
            if matches!(self.peek(), Tok::Sym("/") | Tok::Sym("//")) {
                let steps = self.more_steps(Vec::new())?;
                return Ok(Expr::Path {
                    start: Some(Box::new(base)),
                    steps,
                });
            }
            return Ok(base);
        }
        let steps = self.rel_steps()?;
        Ok(Expr::Path { start: None, steps })
    }

    fn starts_step(&self) -> bool {
        match self.peek() {
            Tok::Name(_) | Tok::Star | Tok::AxisName(_) | Tok::Var(_) | Tok::Str(_) => true,
            Tok::Int(_) | Tok::Dec(_) | Tok::Dbl(_) => true,
            Tok::Sym(s) => matches!(*s, "@" | "." | ".." | "("),
            _ => false,
        }
    }

    /// Whether the next token starts an axis step rather than a primary.
    fn starts_axis_step(&self) -> bool {
        match self.peek() {
            Tok::Star | Tok::AxisName(_) => true,
            Tok::Sym("@") | Tok::Sym("..") => true,
            Tok::Name(n) => {
                if matches!(self.peek_at(1), Tok::Sym("(")) {
                    is_kind_test(n)
                } else {
                    true
                }
            }
            _ => false,
        }
    }

    fn rel_steps(&mut self) -> Result<Vec<PathStep>, String> {
        let first = self.step()?;
        self.more_steps(vec![first])
    }

    fn more_steps(&mut self, mut v: Vec<PathStep>) -> Result<Vec<PathStep>, String> {
        loop {
            if self.sym("/") {
                v.push(self.step()?);
            } else if self.sym("//") {
                v.push(PathStep::Axis(dos()));
                v.push(self.step()?);
            } else {
                return Ok(v);
            }
        }
    }

    fn step(&mut self) -> Result<PathStep, String> {
        if self.sym("..") {
            return Ok(PathStep::Axis(Step {
                axis: Axis::Parent,
                test: NodeTest::AnyKind,
                preds: vec![],
            }));
        }
        if !self.starts_axis_step() {
            let prim = self.primary()?;
            let preds = self.preds()?;
            return Ok(PathStep::Expr(prim, preds));
        }
        let axis = if self.sym("@") {
            Axis::Attribute
        } else if let Tok::AxisName(a) = self.peek().clone() {
            self.next();
            match a.as_str() {
                "child" => Axis::Child,
                "descendant" => Axis::Descendant,
                "descendant-or-self" => Axis::DescendantOrSelf,
                "parent" => Axis::Parent,
                "ancestor" => Axis::Ancestor,
                "ancestor-or-self" => Axis::AncestorOrSelf,
                "following-sibling" => Axis::FollowingSibling,
                "preceding-sibling" => Axis::PrecedingSibling,
                "following" => Axis::Following,
                "preceding" => Axis::Preceding,
                "attribute" => Axis::Attribute,
                "self" => Axis::SelfAxis,
                _ => return Err(format!("unknown axis '{a}'")),
            }
        } else {
            Axis::Child
        };
        let test = self.node_test(&axis)?;
        let preds = self.preds()?;
        Ok(PathStep::Axis(Step { axis, test, preds }))
    }

    fn preds(&mut self) -> Result<Vec<Expr>, String> {
        let mut v = Vec::new();
        while self.sym("[") {
            v.push(self.expr()?);
            self.expect("]")?;
        }
        Ok(v)
    }

    fn kind_test(&mut self) -> Result<Option<NodeTest>, String> {
        let Tok::Name(n) = self.peek().clone() else {
            return Ok(None);
        };
        if !is_kind_test(&n) || !matches!(self.peek_at(1), Tok::Sym("(")) {
            return Ok(None);
        }
        self.i += 2;
        let arg = match self.peek().clone() {
            Tok::Name(a) => {
                self.next();
                Some(a)
            }
            Tok::Str(a) => {
                self.next();
                Some(a)
            }
            Tok::Star => {
                self.next();
                None
            }
            _ => None,
        };
        self.expect(")")?;
        Ok(Some(match n.as_str() {
            "node" => NodeTest::AnyKind,
            "text" => NodeTest::Text,
            "comment" => NodeTest::Comment,
            "processing-instruction" => NodeTest::Pi(arg),
            "document-node" => NodeTest::Document,
            "element" => NodeTest::Element(arg.map(|a| self.qname(&a, true)).transpose()?),
            "attribute" => NodeTest::Attribute(arg.map(|a| self.qname(&a, false)).transpose()?),
            _ => unreachable!(),
        }))
    }

    fn node_test(&mut self, axis: &Axis) -> Result<NodeTest, String> {
        if let Some(k) = self.kind_test()? {
            return Ok(k);
        }
        match self.next() {
            Tok::Star => Ok(NodeTest::Name {
                uri: None,
                local: None,
            }),
            Tok::Name(n) => {
                if let Some(l) = n.strip_prefix("*:") {
                    return Ok(NodeTest::Name {
                        uri: None,
                        local: Some(l.into()),
                    });
                }
                if let Some(p) = n.strip_suffix(":*") {
                    let u = (self.ns)(p).ok_or(format!("undeclared prefix '{p}'"))?;
                    return Ok(NodeTest::Name {
                        uri: Some(u),
                        local: None,
                    });
                }
                let q = self.qname(&n, *axis != Axis::Attribute)?;
                Ok(NodeTest::Name {
                    uri: Some(q.uri.to_string()),
                    local: Some(q.local.to_string()),
                })
            }
            t => Err(format!("expected a node test, found {t:?}")),
        }
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.next() {
            Tok::Str(s) => Ok(Expr::Literal(Atomic::str(s))),
            Tok::Int(s) => Ok(match s.parse::<i64>() {
                Ok(i) => Expr::Literal(Atomic::Int(i)),
                Err(_) => Expr::Literal(Atomic::Dec(Decimal::parse(&s).ok_or("bad number")?)),
            }),
            Tok::Dec(s) => Ok(Expr::Literal(Atomic::Dec(
                Decimal::parse(&s).ok_or("bad decimal")?,
            ))),
            Tok::Dbl(s) => Ok(Expr::Literal(Atomic::Dbl(
                s.parse().map_err(|_| "bad double")?,
            ))),
            Tok::Var(v) => Ok(Expr::Var(self.qname(&v, false)?)),
            Tok::Sym("(") => {
                if self.sym(")") {
                    return Ok(Expr::Seq(vec![]));
                }
                let e = self.expr()?;
                self.expect(")")?;
                Ok(match e {
                    Expr::Seq(v) => Expr::Seq(v),
                    e => Expr::Seq(vec![e]),
                })
            }
            Tok::Sym(".") => Ok(Expr::Context),
            Tok::Name(n) if matches!(self.peek(), Tok::Sym("(")) => {
                self.next();
                let mut args = Vec::new();
                if !self.sym(")") {
                    loop {
                        args.push(self.single()?);
                        if self.sym(")") {
                            break;
                        }
                        self.expect(",")?;
                    }
                }
                let q = if n.contains(':') {
                    self.qname(&n, false)?
                } else {
                    QName::new(FN_NS, &n, "")
                };
                if &*q.uri == XS_NS {
                    if matches!(&*q.local, "NOTATION" | "anyAtomicType") {
                        return Err(format!("XPST0017: no constructor function xs:{}", q.local));
                    }
                    if let ("QName", [Expr::Literal(Atomic::Str(s))]) = (&*q.local, args.as_slice())
                    {
                        return Ok(Expr::Literal(self.qname_value(s)?));
                    }
                }
                // The static base URI is known when parsing: bind it here
                // (`static-base-uri()`, and the default base of
                // `resolve-uri($relative)`).
                if &*q.uri == FN_NS
                    && let Some(base) = (self.ns)(STATIC_BASE_KEY)
                {
                    let base =
                        Expr::Literal(Atomic::Sub("anyURI", Rc::new(Atomic::str(base.as_str()))));
                    match (&*q.local, args.len()) {
                        ("static-base-uri", 0) => return Ok(base),
                        ("resolve-uri", 1) => args.push(base),
                        _ => {}
                    }
                }
                Ok(Expr::Call(q, args))
            }
            t => Err(format!("unexpected {t:?}")),
        }
    }
}

fn is_kind_test(n: &str) -> bool {
    matches!(
        n,
        "node"
            | "text"
            | "comment"
            | "processing-instruction"
            | "document-node"
            | "element"
            | "attribute"
    )
}

fn dos() -> Step {
    Step {
        axis: Axis::DescendantOrSelf,
        test: NodeTest::AnyKind,
        preds: vec![],
    }
}

/// Parses a sequence type such as `xs:string?` (the `as` attribute).
pub fn parse_seq_type(src: &str, ns: Resolver) -> Result<SeqType, String> {
    let mut p = Parser {
        t: lex(src)?,
        i: 0,
        ns,
    };
    let t = p.seq_type()?;
    if p.peek() != &Tok::Eof {
        return Err(format!("unexpected text in type '{src}'"));
    }
    Ok(t)
}
