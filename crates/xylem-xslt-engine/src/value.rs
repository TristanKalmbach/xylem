//! XDM values: sequences of nodes and typed atomic values.

use crate::tree::NodeRef;
use std::cmp::Ordering;
use std::rc::Rc;

/// `xs:decimal` as a fixed-point number with 18 fractional digits.
///
/// Division keeps 18 fractional digits, which is what the reference
/// processor prints for `1 div 3` (see the spec notes in the design doc).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Decimal(pub i128);

pub const DEC_SCALE: i128 = 1_000_000_000_000_000_000;

impl Decimal {
    pub fn from_int(i: i64) -> Decimal {
        Decimal(i as i128 * DEC_SCALE)
    }
    pub fn parse(s: &str) -> Option<Decimal> {
        let s = s.trim();
        let (neg, s) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s.strip_prefix('+').unwrap_or(s)),
        };
        let (int, frac) = s.split_once('.').unwrap_or((s, ""));
        if int.is_empty() && frac.is_empty() {
            return None;
        }
        if !int.chars().chain(frac.chars()).all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mut v: i128 = 0;
        for c in int.chars() {
            v = v.checked_mul(10)?.checked_add((c as u8 - b'0') as i128)?;
        }
        v = v.checked_mul(DEC_SCALE)?;
        let mut scale = DEC_SCALE / 10;
        for c in frac.chars().take(18) {
            v += (c as u8 - b'0') as i128 * scale;
            scale /= 10;
        }
        Some(Decimal(if neg { -v } else { v }))
    }
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / DEC_SCALE as f64
    }
    pub fn from_f64(d: f64) -> Option<Decimal> {
        if !d.is_finite() {
            return None;
        }
        Decimal::parse(&format!("{d:.18}"))
    }
    pub fn mul(self, o: Decimal) -> Option<Decimal> {
        let (a, b) = (self.0, o.0);
        let (ai, af) = (a / DEC_SCALE, a % DEC_SCALE);
        // a*b/S = ai*b + af*b/S, splitting b likewise to stay in range.
        let (bi, bf) = (b / DEC_SCALE, b % DEC_SCALE);
        let t1 = ai.checked_mul(b)?;
        let t2 = af.checked_mul(bi)?;
        let t3 = af.checked_mul(bf)? / DEC_SCALE;
        Some(Decimal(t1.checked_add(t2)?.checked_add(t3)?))
    }
    pub fn div(self, o: Decimal) -> Option<Decimal> {
        if o.0 == 0 {
            return None;
        }
        // Long division to 18 fractional digits, truncating.
        let neg = (self.0 < 0) != (o.0 < 0);
        let (a, b) = (self.0.unsigned_abs(), o.0.unsigned_abs());
        let mut q = a / b;
        let mut r = a % b;
        for _ in 0..18 {
            r *= 10;
            q = q.checked_mul(10)? + r / b;
            r %= b;
        }
        let q = i128::try_from(q).ok()?;
        Some(Decimal(if neg { -q } else { q }))
    }
    /// Canonical lexical form: no trailing zeros, no trailing point.
    pub fn to_canonical(self) -> String {
        let neg = self.0 < 0;
        let a = self.0.unsigned_abs();
        let int = a / DEC_SCALE as u128;
        let frac = a % DEC_SCALE as u128;
        let mut s = String::new();
        if neg && a != 0 {
            s.push('-');
        }
        s.push_str(&int.to_string());
        if frac != 0 {
            let f = format!("{frac:018}");
            s.push('.');
            s.push_str(f.trim_end_matches('0'));
        }
        s
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Atomic {
    Str(Rc<str>),
    Untyped(Rc<str>),
    Bool(bool),
    Int(i64),
    Dec(Decimal),
    Dbl(f64),
    /// A value of a derived built-in type (or `xs:anyURI`, `xs:float`):
    /// the type's local name and the value of its primitive base.
    Sub(&'static str, Rc<Atomic>),
    /// A date/time value (`xs:dateTime`, `xs:date`, ..., `xs:gDay`).
    Time(Rc<crate::datetime::DateTime>),
    /// A duration (`xs:duration` and its two subtypes).
    Dur(Rc<crate::datetime::Duration>),
    /// An `xs:QName`: namespace URI, prefix, local name.
    QName(Rc<(Rc<str>, Rc<str>, Rc<str>)>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Node(NodeRef),
    Atomic(Atomic),
}

pub type Seq = Vec<Item>;

impl Atomic {
    pub fn str(s: impl Into<Rc<str>>) -> Atomic {
        Atomic::Str(s.into())
    }
    /// The value with any derived-type annotation removed: what the
    /// primitive operations (arithmetic, comparison) work on.
    pub fn prim(&self) -> Atomic {
        match self {
            Atomic::Sub(_, v) => v.prim(),
            o => o.clone(),
        }
    }
    #[allow(clippy::inherent_to_string)]
    pub fn to_string(&self) -> String {
        match self {
            Atomic::Sub("float", v) => format_float(v.to_f64()),
            Atomic::Sub(_, v) => v.to_string(),
            Atomic::Time(t) => t.to_canonical(),
            Atomic::Dur(d) => d.to_canonical(),
            Atomic::QName(q) if q.1.is_empty() => q.2.to_string(),
            Atomic::QName(q) => format!("{}:{}", q.1, q.2),
            Atomic::Str(s) | Atomic::Untyped(s) => s.to_string(),
            Atomic::Bool(b) => b.to_string(),
            Atomic::Int(i) => i.to_string(),
            Atomic::Dec(d) if compat() => format_double_1(d.to_f64()),
            Atomic::Dec(d) => d.to_canonical(),
            Atomic::Dbl(d) if compat() => format_double_1(*d),
            Atomic::Dbl(d) => format_double(*d),
        }
    }
    pub fn is_numeric(&self) -> bool {
        match self {
            Atomic::Sub(_, v) => v.is_numeric(),
            o => matches!(o, Atomic::Int(_) | Atomic::Dec(_) | Atomic::Dbl(_)),
        }
    }
    pub fn is_stringlike(&self) -> bool {
        match self {
            Atomic::Sub("hexBinary" | "base64Binary", _) => false,
            Atomic::Sub(_, v) => v.is_stringlike(),
            o => matches!(o, Atomic::Str(_) | Atomic::Untyped(_)),
        }
    }
    pub fn to_f64(&self) -> f64 {
        match self {
            Atomic::Int(i) => *i as f64,
            Atomic::Dec(d) => d.to_f64(),
            Atomic::Dbl(d) => *d,
            Atomic::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Atomic::Str(s) | Atomic::Untyped(s) => parse_double(s),
            Atomic::Sub(_, v) => v.to_f64(),
            Atomic::Time(_) | Atomic::Dur(_) | Atomic::QName(_) => f64::NAN,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Atomic::Str(_) => "xs:string",
            Atomic::Untyped(_) => "xs:untypedAtomic",
            Atomic::Bool(_) => "xs:boolean",
            Atomic::Int(_) => "xs:integer",
            Atomic::Dec(_) => "xs:decimal",
            Atomic::Dbl(_) => "xs:double",
            Atomic::Sub(t, _) => sub_type_name(t),
            Atomic::Time(t) => match t.kind {
                crate::datetime::DtKind::DateTime => "xs:dateTime",
                crate::datetime::DtKind::Date => "xs:date",
                crate::datetime::DtKind::Time => "xs:time",
                crate::datetime::DtKind::GYearMonth => "xs:gYearMonth",
                crate::datetime::DtKind::GYear => "xs:gYear",
                crate::datetime::DtKind::GMonthDay => "xs:gMonthDay",
                crate::datetime::DtKind::GDay => "xs:gDay",
                crate::datetime::DtKind::GMonth => "xs:gMonth",
            },
            Atomic::Dur(d) => match d.kind {
                crate::datetime::DurKind::Duration => "xs:duration",
                crate::datetime::DurKind::YearMonth => "xs:yearMonthDuration",
                crate::datetime::DurKind::DayTime => "xs:dayTimeDuration",
            },
            Atomic::QName(_) => "xs:QName",
        }
    }
}

/// `xs:` names of the derived types, for [`Atomic::type_name`].
fn sub_type_name(t: &str) -> &'static str {
    const NAMES: &[&str] = &[
        "xs:nonPositiveInteger",
        "xs:negativeInteger",
        "xs:long",
        "xs:int",
        "xs:short",
        "xs:byte",
        "xs:nonNegativeInteger",
        "xs:unsignedLong",
        "xs:unsignedInt",
        "xs:unsignedShort",
        "xs:unsignedByte",
        "xs:positiveInteger",
        "xs:normalizedString",
        "xs:token",
        "xs:language",
        "xs:NMTOKEN",
        "xs:Name",
        "xs:NCName",
        "xs:ID",
        "xs:IDREF",
        "xs:ENTITY",
        "xs:anyURI",
        "xs:float",
        "xs:hexBinary",
        "xs:base64Binary",
    ];
    NAMES
        .iter()
        .copied()
        .find(|n| &n[3..] == t)
        .unwrap_or("xs:anyAtomicType")
}

/// Canonical `xs:float` → string: like doubles, with float precision.
pub fn format_float(d: f64) -> String {
    let f = d as f32;
    if !f.is_finite() || f == 0.0 {
        return format_double(d);
    }
    let a = f.abs();
    if (1e-6..1e6).contains(&a) {
        return format!("{f}");
    }
    let s = format!("{f:e}");
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let m = if m.contains('.') {
        m.to_string()
    } else {
        format!("{m}.0")
    };
    format!("{m}E{e}")
}

/// XPath `number()` / cast to `xs:double` of a string (NaN when invalid).
pub fn parse_double(s: &str) -> f64 {
    let t = s.trim_matches(crate::tree::is_xml_space);
    match t {
        "INF" => f64::INFINITY,
        "-INF" => f64::NEG_INFINITY,
        "NaN" => f64::NAN,
        _ => {
            let ok = !t.is_empty()
                && t.chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'));
            if ok {
                t.parse().unwrap_or(f64::NAN)
            } else {
                f64::NAN
            }
        }
    }
}

thread_local! {
    /// Set while an XSLT 1.0 stylesheet runs (backwards-compatible mode:
    /// numbers are doubles written the XPath 1.0 way).
    static COMPAT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether XPath 1.0 rules apply (see [`with_compat`]).
pub fn compat() -> bool {
    COMPAT.with(|c| c.get())
}

thread_local! {
    /// Whether a transformation may read files: `document()` and external
    /// entities in the input (stylesheet modules are always read).
    static FILES: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether the running transformation may read files.
pub fn file_access() -> bool {
    FILES.with(|c| c.get())
}

/// Runs `f` with file access from inside transformations (`document()`,
/// external entities in the input) allowed or not. Hosts that run input
/// they do not trust should turn it off.
pub fn with_file_access<T>(allowed: bool, f: impl FnOnce() -> T) -> T {
    let before = FILES.with(|c| c.replace(allowed));
    let r = f();
    FILES.with(|c| c.set(before));
    r
}

/// Runs `f` with XPath 1.0 rules switched on or off.
pub fn with_compat<T>(on: bool, f: impl FnOnce() -> T) -> T {
    let before = COMPAT.with(|c| c.replace(on));
    let r = f();
    COMPAT.with(|c| c.set(before));
    r
}

/// XPath 1.0 `string()` of a number: no exponent, integers without a
/// decimal point, `Infinity`, `-Infinity`, `NaN`.
pub fn format_double_1(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 {
            "Infinity".into()
        } else {
            "-Infinity".into()
        };
    }
    if d == 0.0 {
        return "0".into();
    }
    format!("{d}")
}

/// Canonical `xs:double` → string (F&O 17.1.2): plain decimal notation for
/// magnitudes in [1e-6, 1e6), otherwise `mantissaEexponent` with at least
/// one fractional digit.
pub fn format_double(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d > 0.0 { "INF".into() } else { "-INF".into() };
    }
    if d == 0.0 {
        return if d.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let a = d.abs();
    if (1e-6..1e6).contains(&a) {
        let s = format!("{d}");
        return s;
    }
    let s = format!("{d:e}"); // e.g. 1.23456789012e11, 1e6
    let (m, e) = s.split_once('e').unwrap();
    let m = if m.contains('.') {
        m.to_string()
    } else {
        format!("{m}.0")
    };
    format!("{m}E{e}")
}

/// Numeric comparison where any operand is a double, else exact.
pub fn cmp_numeric(a: &Atomic, b: &Atomic) -> Option<Ordering> {
    // xs:float against a non-double: both promote to float precision.
    let fl = |x: &Atomic| matches!(x, Atomic::Sub("float", _));
    let dbl = |x: &Atomic| matches!(x, Atomic::Dbl(_));
    if (fl(a) && !dbl(b) && !fl(b)) || (fl(b) && !dbl(a) && !fl(a)) {
        let f = |x: &Atomic| x.to_f64() as f32 as f64;
        return f(a).partial_cmp(&f(b));
    }
    if matches!(a, Atomic::Sub(..)) || matches!(b, Atomic::Sub(..)) {
        return cmp_numeric(&a.prim(), &b.prim());
    }
    match (a, b) {
        (Atomic::Int(x), Atomic::Int(y)) => Some(x.cmp(y)),
        (Atomic::Dbl(_), _) | (_, Atomic::Dbl(_)) => a.to_f64().partial_cmp(&b.to_f64()),
        _ => Some(to_dec(a)?.cmp(&to_dec(b)?)),
    }
}

pub fn to_dec(a: &Atomic) -> Option<Decimal> {
    match a {
        Atomic::Int(i) => Some(Decimal::from_int(*i)),
        Atomic::Dec(d) => Some(*d),
        Atomic::Dbl(d) => Decimal::from_f64(*d),
        Atomic::Str(s) | Atomic::Untyped(s) => Decimal::parse(s),
        Atomic::Bool(b) => Some(Decimal::from_int(*b as i64)),
        Atomic::Sub(_, v) => to_dec(v),
        Atomic::Time(_) | Atomic::Dur(_) | Atomic::QName(_) => None,
    }
}
