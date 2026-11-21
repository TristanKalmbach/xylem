//! Simple types: XSD built-in datatypes, derived types with facets, and the
//! DTD attribute types mapped onto them.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use regex::Regex;

use crate::xsdregex;

/// Index of a simple type in a [`crate::Grammar`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TypeId(pub(crate) u32);

/// Whitespace normalisation applied before a value is checked.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WhiteSpace {
    Preserve,
    Replace,
    Collapse,
}

impl WhiteSpace {
    pub fn apply<'a>(self, value: &'a str) -> std::borrow::Cow<'a, str> {
        use std::borrow::Cow;
        match self {
            WhiteSpace::Preserve => Cow::Borrowed(value),
            WhiteSpace::Replace => {
                if value.contains(['\t', '\n', '\r']) {
                    Cow::Owned(value.replace(['\t', '\n', '\r'], " "))
                } else {
                    Cow::Borrowed(value)
                }
            }
            WhiteSpace::Collapse => {
                let needs = value.starts_with(is_xml_ws)
                    || value.ends_with(is_xml_ws)
                    || value.contains(['\t', '\n', '\r'])
                    || value.contains("  ");
                if needs {
                    Cow::Owned(
                        value
                            .split(is_xml_ws)
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" "),
                    )
                } else {
                    Cow::Borrowed(value)
                }
            }
        }
    }
}

fn is_xml_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// The primitive (or name-like) built-in a simple type ultimately derives from.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Builtin {
    AnySimpleType,
    String,
    NormalizedString,
    Token,
    Language,
    Name,
    NCName,
    Id,
    IdRef,
    Entity,
    NmToken,
    QName,
    Notation,
    AnyUri,
    Boolean,
    Decimal,
    Integer,
    Float,
    Double,
    Duration,
    DateTime,
    Date,
    Time,
    GYearMonth,
    GYear,
    GMonthDay,
    GDay,
    GMonth,
    HexBinary,
    Base64Binary,
}

/// How a simple type is constructed.
#[derive(Clone, Debug, PartialEq)]
pub enum Variety {
    Atomic,
    /// Whitespace-separated list of the item type.
    List(TypeId),
    /// Valid if valid for any member type.
    Union(Vec<TypeId>),
}

/// One `xs:pattern` derivation step: the value must match one of the patterns.
#[derive(Debug)]
pub struct PatternSet {
    pub sources: Vec<String>,
    compiled: OnceLock<Result<Regex, String>>,
}

impl PatternSet {
    pub(crate) fn new(sources: Vec<String>) -> Self {
        Self {
            sources,
            compiled: OnceLock::new(),
        }
    }

    /// Compiles (once) the alternation of all patterns of this step.
    pub fn regex(&self) -> Result<&Regex, &str> {
        self.compiled
            .get_or_init(|| {
                let parts: Result<Vec<String>, String> = self
                    .sources
                    .iter()
                    .map(|s| xsdregex::translate(s))
                    .collect();
                let alt = parts?
                    .iter()
                    .map(|p| format!("(?:{p})"))
                    .collect::<Vec<_>>()
                    .join("|");
                Regex::new(&format!(r"\A(?:{alt})\z")).map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(|e| e.as_str())
    }
}

/// Constraining facets accumulated along a derivation chain.
#[derive(Clone, Debug, Default)]
pub struct Facets {
    /// Allowed values (after whitespace normalisation), in declaration order.
    pub enumeration: Option<Vec<String>>,
    enum_set: Option<Arc<HashSet<String>>>,
    /// Every set must match (sets are ANDed across derivation steps).
    pub patterns: Vec<Arc<PatternSet>>,
    pub length: Option<usize>,
    pub min_length: Option<usize>,
    pub max_length: Option<usize>,
    pub min_inclusive: Option<String>,
    pub max_inclusive: Option<String>,
    pub min_exclusive: Option<String>,
    pub max_exclusive: Option<String>,
    pub total_digits: Option<u32>,
    pub fraction_digits: Option<u32>,
}

impl Facets {
    pub(crate) fn set_enumeration(&mut self, values: Vec<String>) {
        self.enum_set = (values.len() > 8).then(|| Arc::new(values.iter().cloned().collect()));
        self.enumeration = Some(values);
    }

    fn enum_contains(&self, v: &str) -> bool {
        match (&self.enum_set, &self.enumeration) {
            (Some(set), _) => set.contains(v),
            (None, Some(list)) => list.iter().any(|e| e == v),
            (None, None) => true,
        }
    }
}

/// A simple type definition.
#[derive(Clone, Debug)]
pub struct SimpleType {
    /// Name for display, e.g. `xs:token` or `CDATA`; `None` for anonymous types.
    pub name: Option<String>,
    pub builtin: Builtin,
    pub variety: Variety,
    pub whitespace: WhiteSpace,
    pub facets: Facets,
}

impl SimpleType {
    fn atomic(name: &str, builtin: Builtin, whitespace: WhiteSpace) -> Self {
        Self {
            name: Some(name.to_string()),
            builtin,
            variety: Variety::Atomic,
            whitespace,
            facets: Facets::default(),
        }
    }
}

/// Built-in type names, in a fixed order; their indices are their [`TypeId`]s.
const BUILTINS: &[(&str, Builtin, WhiteSpace)] = {
    use Builtin as B;
    use WhiteSpace::*;
    &[
        ("anySimpleType", B::AnySimpleType, Preserve),
        ("string", B::String, Preserve),
        ("normalizedString", B::NormalizedString, Replace),
        ("token", B::Token, Collapse),
        ("language", B::Language, Collapse),
        ("Name", B::Name, Collapse),
        ("NCName", B::NCName, Collapse),
        ("ID", B::Id, Collapse),
        ("IDREF", B::IdRef, Collapse),
        ("ENTITY", B::Entity, Collapse),
        ("NMTOKEN", B::NmToken, Collapse),
        ("QName", B::QName, Collapse),
        ("NOTATION", B::Notation, Collapse),
        ("anyURI", B::AnyUri, Collapse),
        ("boolean", B::Boolean, Collapse),
        ("decimal", B::Decimal, Collapse),
        ("integer", B::Integer, Collapse),
        ("float", B::Float, Collapse),
        ("double", B::Double, Collapse),
        ("duration", B::Duration, Collapse),
        ("dateTime", B::DateTime, Collapse),
        ("date", B::Date, Collapse),
        ("time", B::Time, Collapse),
        ("gYearMonth", B::GYearMonth, Collapse),
        ("gYear", B::GYear, Collapse),
        ("gMonthDay", B::GMonthDay, Collapse),
        ("gDay", B::GDay, Collapse),
        ("gMonth", B::GMonth, Collapse),
        ("hexBinary", B::HexBinary, Collapse),
        ("base64Binary", B::Base64Binary, Collapse),
    ]
};

/// Integer subtypes: name, minInclusive, maxInclusive.
const INTEGER_SUBTYPES: &[(&str, Option<&str>, Option<&str>)] = &[
    ("nonPositiveInteger", None, Some("0")),
    ("negativeInteger", None, Some("-1")),
    (
        "long",
        Some("-9223372036854775808"),
        Some("9223372036854775807"),
    ),
    ("int", Some("-2147483648"), Some("2147483647")),
    ("short", Some("-32768"), Some("32767")),
    ("byte", Some("-128"), Some("127")),
    ("nonNegativeInteger", Some("0"), None),
    ("unsignedLong", Some("0"), Some("18446744073709551615")),
    ("unsignedInt", Some("0"), Some("4294967295")),
    ("unsignedShort", Some("0"), Some("65535")),
    ("unsignedByte", Some("0"), Some("255")),
    ("positiveInteger", Some("1"), None),
];

/// Built-in list types: name and item type.
const LIST_BUILTINS: &[(&str, &str)] = &[
    ("IDREFS", "IDREF"),
    ("ENTITIES", "ENTITY"),
    ("NMTOKENS", "NMTOKEN"),
];

/// All simple types of a grammar.
#[derive(Debug, Clone)]
pub(crate) struct TypeTable {
    pub(crate) types: Vec<SimpleType>,
    builtin_index: HashMap<&'static str, TypeId>,
}

impl Default for TypeTable {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeTable {
    pub(crate) fn new() -> Self {
        let mut t = TypeTable {
            types: Vec::new(),
            builtin_index: HashMap::new(),
        };
        for &(name, b, ws) in BUILTINS {
            let id = t.add(SimpleType::atomic(name, b, ws));
            t.builtin_index.insert(name, id);
        }
        for &(name, min, max) in INTEGER_SUBTYPES {
            let mut st = SimpleType::atomic(name, Builtin::Integer, WhiteSpace::Collapse);
            st.facets.min_inclusive = min.map(str::to_string);
            st.facets.max_inclusive = max.map(str::to_string);
            let id = t.add(st);
            t.builtin_index.insert(name, id);
        }
        for &(name, item) in LIST_BUILTINS {
            let item_id = t.builtin_index[item];
            let item_b = t.get(item_id).builtin;
            let mut st = SimpleType::atomic(name, item_b, WhiteSpace::Collapse);
            st.variety = Variety::List(item_id);
            st.facets.min_length = Some(1);
            let id = t.add(st);
            t.builtin_index.insert(name, id);
        }
        // `anyType` used as a simple type behaves like anySimpleType.
        t.builtin_index.insert("anyType", TypeId(0));
        t
    }

    pub(crate) fn add(&mut self, st: SimpleType) -> TypeId {
        self.types.push(st);
        TypeId(self.types.len() as u32 - 1)
    }

    pub(crate) fn get(&self, id: TypeId) -> &SimpleType {
        &self.types[id.0 as usize]
    }

    /// Looks up a built-in by its local name (`string`, `IDREFS`, ...).
    pub(crate) fn builtin(&self, name: &str) -> Option<TypeId> {
        self.builtin_index.get(name).copied()
    }

    pub(crate) fn string(&self) -> TypeId {
        self.builtin_index["string"]
    }

    /// Creates a type derived by restriction from `base` (facets added later).
    pub(crate) fn restrict(&self, base: TypeId) -> SimpleType {
        let mut st = self.get(base).clone();
        st.name = None;
        st
    }

    /// Validates a value against a type. The error is a short reason suitable
    /// for parenthesising after "Value 'x' is not valid ...".
    pub(crate) fn validate(&self, id: TypeId, value: &str) -> Result<(), String> {
        self.validate_depth(id, value, 0)
    }

    fn validate_depth(&self, id: TypeId, value: &str, depth: u32) -> Result<(), String> {
        let st = self.get(id);
        if depth > 32 {
            return Ok(());
        }
        let norm = st.whitespace.apply(value);
        let v: &str = &norm;
        let len = match &st.variety {
            Variety::Atomic => {
                check_lexical(st.builtin, v)?;
                value_length(st.builtin, v)
            }
            Variety::List(item) => {
                let mut n = 0;
                for tok in v.split(' ').filter(|s| !s.is_empty()) {
                    self.validate_depth(*item, tok, depth + 1)?;
                    n += 1;
                }
                n
            }
            Variety::Union(members) => {
                if !members.is_empty()
                    && !members
                        .iter()
                        .any(|m| self.validate_depth(*m, v, depth + 1).is_ok())
                {
                    let names: Vec<String> = members.iter().map(|m| self.describe(*m)).collect();
                    return Err(format!("must be one of the types {}", names.join(" | ")));
                }
                v.chars().count()
            }
        };
        let f = &st.facets;
        if let Some(list) = &f.enumeration
            && !f.enum_contains(v)
        {
            return Err(format!("allowed: {}", list.join(", ")));
        }
        for set in &f.patterns {
            if let Ok(re) = set.regex()
                && !re.is_match(v)
            {
                return Err(format!(
                    "must match pattern '{}'",
                    set.sources.join("' or '")
                ));
            }
        }
        if let Some(n) = f.length
            && len != n
        {
            return Err(format!("length must be {n}"));
        }
        if let Some(n) = f.min_length
            && len < n
        {
            return Err(format!("length must be at least {n}"));
        }
        if let Some(n) = f.max_length
            && len > n
        {
            return Err(format!("length must be at most {n}"));
        }
        if matches!(st.variety, Variety::Atomic) {
            check_bounds(st.builtin, f, v)?;
        }
        Ok(())
    }

    /// A short human-readable description of a type.
    pub(crate) fn describe(&self, id: TypeId) -> String {
        let st = self.get(id);
        if let Some(n) = &st.name {
            return n.clone();
        }
        match &st.variety {
            Variety::List(item) => format!("list of {}", self.describe(*item)),
            _ => format!("{:?}", st.builtin).to_ascii_lowercase(),
        }
    }
}

fn value_length(b: Builtin, v: &str) -> usize {
    match b {
        Builtin::HexBinary => v.len() / 2,
        Builtin::Base64Binary => {
            let n = v.chars().filter(|c| !c.is_whitespace()).count();
            let pad = v.chars().rev().take_while(|&c| c == '=').count();
            (n / 4 * 3).saturating_sub(pad)
        }
        _ => v.chars().count(),
    }
}

fn check_bounds(b: Builtin, f: &Facets, v: &str) -> Result<(), String> {
    let checks = [
        (
            &f.min_inclusive,
            [Ordering::Greater, Ordering::Equal],
            "at least",
        ),
        (
            &f.max_inclusive,
            [Ordering::Less, Ordering::Equal],
            "at most",
        ),
        (
            &f.min_exclusive,
            [Ordering::Greater, Ordering::Greater],
            "greater than",
        ),
        (
            &f.max_exclusive,
            [Ordering::Less, Ordering::Less],
            "less than",
        ),
    ];
    for (bound, ok, word) in checks {
        if let Some(bound) = bound
            && let Some(ord) = compare_values(b, v, bound)
            && !ok.contains(&ord)
        {
            return Err(format!("must be {word} {bound}"));
        }
    }
    if let Some(n) = f.total_digits
        && matches!(b, Builtin::Decimal | Builtin::Integer)
        && let Some((int, frac)) = decimal_parts(v)
    {
        let digits = int.len() + frac.len();
        if digits as u32 > n.max(1) && !(int.is_empty() && frac.is_empty()) {
            return Err(format!("must have at most {n} digits"));
        }
    }
    if let Some(n) = f.fraction_digits
        && matches!(b, Builtin::Decimal)
        && let Some((_, frac)) = decimal_parts(v)
        && frac.len() as u32 > n
    {
        return Err(format!("must have at most {n} fraction digits"));
    }
    Ok(())
}

/// Compares two lexical values in the value space of `b`. `None` when the
/// values are not comparable (or the type has no supported ordering).
pub(crate) fn compare_values(b: Builtin, a: &str, c: &str) -> Option<Ordering> {
    match b {
        Builtin::Decimal | Builtin::Integer => compare_decimal(a, c),
        Builtin::Float | Builtin::Double => {
            let x = parse_float(a)?;
            let y = parse_float(c)?;
            x.partial_cmp(&y)
        }
        Builtin::DateTime
        | Builtin::Date
        | Builtin::Time
        | Builtin::GYear
        | Builtin::GYearMonth
        | Builtin::GMonthDay
        | Builtin::GDay
        | Builtin::GMonth => {
            // Lexical comparison of the timezone-less parts (practical subset).
            let strip = |s: &str| -> String {
                let s = s.strip_suffix('Z').unwrap_or(s);
                let tz = s.len().checked_sub(6).filter(|&i| {
                    let t = &s.as_bytes()[i..];
                    (t[0] == b'+' || t[0] == b'-') && t[3] == b':'
                });
                tz.map(|i| s[..i].to_string())
                    .unwrap_or_else(|| s.to_string())
            };
            let (x, y) = (strip(a), strip(c));
            if b == Builtin::GYear {
                return compare_decimal(&x, &y);
            }
            Some(x.cmp(&y))
        }
        _ => None,
    }
}

fn parse_float(s: &str) -> Option<f64> {
    match s {
        "INF" | "+INF" => Some(f64::INFINITY),
        "-INF" => Some(f64::NEG_INFINITY),
        "NaN" => Some(f64::NAN),
        _ => s.parse().ok(),
    }
}

/// Splits a decimal into (integer digits, fraction digits) without leading
/// and trailing zeros. Returns `None` if it is not a decimal.
fn decimal_parts(s: &str) -> Option<(&str, &str)> {
    let s = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    if (int.is_empty() && frac.is_empty())
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    Some((int.trim_start_matches('0'), frac.trim_end_matches('0')))
}

/// Exact comparison of decimal lexical values of arbitrary size.
pub(crate) fn compare_decimal(a: &str, b: &str) -> Option<Ordering> {
    let neg = |s: &str| s.starts_with('-');
    let (ai, af) = decimal_parts(a)?;
    let (bi, bf) = decimal_parts(b)?;
    let a_zero = ai.is_empty() && af.is_empty();
    let b_zero = bi.is_empty() && bf.is_empty();
    let a_neg = neg(a) && !a_zero;
    let b_neg = neg(b) && !b_zero;
    let magnitude = ai
        .len()
        .cmp(&bi.len())
        .then_with(|| ai.cmp(bi))
        .then_with(|| af.cmp(bf));
    Some(match (a_neg, b_neg) {
        (false, false) => magnitude,
        (true, true) => magnitude.reverse(),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
    })
}

pub(crate) fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':' || (c as u32 > 0x7F && !c.is_whitespace())
}

pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_ascii_digit() || c.is_numeric() || matches!(c, '-' | '.' | '\u{B7}')
}

pub(crate) fn is_name(s: &str) -> bool {
    let mut it = s.chars();
    it.next().is_some_and(is_name_start) && it.all(is_name_char)
}

pub(crate) fn is_ncname(s: &str) -> bool {
    is_name(s) && !s.contains(':')
}

pub(crate) fn is_nmtoken(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_name_char)
}

fn digits(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| b.is_ascii_digit())
}

fn in_range(s: &str, lo: u32, hi: u32) -> bool {
    s.parse::<u32>().is_ok_and(|v| (lo..=hi).contains(&v))
}

/// Strips an optional timezone (`Z` or `±hh:mm`).
fn strip_tz(s: &str) -> Option<&str> {
    if let Some(r) = s.strip_suffix('Z') {
        return Some(r);
    }
    let cut = s.len().wrapping_sub(6);
    if s.len() >= 6 && s.is_char_boundary(cut) {
        let (head, tz) = s.split_at(cut);
        let b = tz.as_bytes();
        if (b[0] == b'+' || b[0] == b'-') && b[3] == b':' && tz.is_ascii() {
            return (in_range(&tz[1..3], 0, 14) && in_range(&tz[4..6], 0, 59)).then_some(head);
        }
    }
    Some(s)
}

fn year(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    s.len() >= 4 && s.bytes().all(|b| b.is_ascii_digit()) && (s.len() == 4 || !s.starts_with('0'))
}

fn date_part(s: &str) -> bool {
    // [-]YYYY-MM-DD
    let Some(i) = s.get(1..).and_then(|r| r.find('-')).map(|i| i + 1) else {
        return false;
    };
    let (y, rest) = s.split_at(i);
    let parts: Vec<&str> = rest[1..].split('-').collect();
    year(y)
        && parts.len() == 2
        && digits(parts[0], 2)
        && in_range(parts[0], 1, 12)
        && digits(parts[1], 2)
        && in_range(parts[1], 1, 31)
}

fn time_part(s: &str) -> bool {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 || !digits(parts[0], 2) || !digits(parts[1], 2) {
        return false;
    }
    let (sec, frac) = parts[2].split_once('.').unwrap_or((parts[2], "0"));
    let ok = in_range(parts[0], 0, 23)
        && in_range(parts[1], 0, 59)
        && digits(sec, 2)
        && in_range(sec, 0, 59);
    ok && !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit()) || s == "24:00:00"
}

fn duration(s: &str) -> bool {
    let s = s.strip_prefix('-').unwrap_or(s);
    let Some(s) = s.strip_prefix('P') else {
        return false;
    };
    let (date, time) = match s.split_once('T') {
        Some((d, t)) => {
            if t.is_empty() {
                return false;
            }
            (d, Some(t))
        }
        None => (s, None),
    };
    fn units(mut s: &str, allowed: &[char], frac_last: bool) -> Option<usize> {
        let mut count = 0;
        let mut idx = 0;
        while !s.is_empty() {
            let end = s.find(|c: char| !c.is_ascii_digit() && c != '.')?;
            let (num, rest) = s.split_at(end);
            let unit = rest.chars().next()?;
            let pos = allowed[idx..].iter().position(|&u| u == unit)? + idx;
            let is_last = pos == allowed.len() - 1;
            if num.is_empty() || (num.contains('.') && !(frac_last && is_last)) {
                return None;
            }
            idx = pos + 1;
            count += 1;
            s = &rest[1..];
        }
        Some(count)
    }
    let Some(d) = units(date, &['Y', 'M', 'D'], false) else {
        return false;
    };
    let t = match time {
        Some(t) => match units(t, &['H', 'M', 'S'], true) {
            Some(n) if n > 0 => n,
            _ => return false,
        },
        None => 0,
    };
    d + t > 0
}

fn is_base64(s: &str) -> bool {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !compact.len().is_multiple_of(4) {
        return false;
    }
    let body = compact.trim_end_matches('=');
    compact.len() - body.len() <= 2
        && body
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/')
}

fn is_decimal(s: &str) -> bool {
    decimal_parts(s).is_some()
}

fn is_integer(s: &str) -> bool {
    let d = s.strip_prefix(['+', '-']).unwrap_or(s);
    !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())
}

fn is_float(s: &str) -> bool {
    if matches!(s, "INF" | "-INF" | "+INF" | "NaN") {
        return true;
    }
    let (mantissa, exp) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    is_decimal(mantissa) && exp.is_none_or(is_integer)
}

fn check_lexical(b: Builtin, v: &str) -> Result<(), String> {
    let ok = match b {
        Builtin::AnySimpleType
        | Builtin::String
        | Builtin::NormalizedString
        | Builtin::Token
        | Builtin::AnyUri => true,
        Builtin::Language => {
            let mut parts = v.split('-');
            let first = parts.next().unwrap_or("");
            (1..=8).contains(&first.len())
                && first.bytes().all(|b| b.is_ascii_alphabetic())
                && parts.all(|p| {
                    (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric())
                })
        }
        Builtin::Name => is_name(v),
        Builtin::NCName | Builtin::Id | Builtin::IdRef | Builtin::Entity => is_ncname(v),
        Builtin::NmToken => is_nmtoken(v),
        Builtin::QName | Builtin::Notation => match v.split_once(':') {
            Some((p, l)) => is_ncname(p) && is_ncname(l),
            None => is_ncname(v),
        },
        Builtin::Boolean => matches!(v, "true" | "false" | "1" | "0"),
        Builtin::Decimal => is_decimal(v),
        Builtin::Integer => is_integer(v),
        Builtin::Float | Builtin::Double => is_float(v),
        Builtin::Duration => duration(v),
        Builtin::DateTime => strip_tz(v)
            .and_then(|s| s.split_once('T'))
            .is_some_and(|(d, t)| date_part(d) && time_part(t)),
        Builtin::Date => strip_tz(v).is_some_and(date_part),
        Builtin::Time => strip_tz(v).is_some_and(time_part),
        Builtin::GYearMonth => strip_tz(v).is_some_and(|s| {
            s.rsplit_once('-')
                .is_some_and(|(y, m)| year(y) && digits(m, 2) && in_range(m, 1, 12))
        }),
        Builtin::GYear => strip_tz(v).is_some_and(year),
        Builtin::GMonthDay => strip_tz(v)
            .and_then(|s| s.strip_prefix("--"))
            .is_some_and(|s| {
                s.split_once('-').is_some_and(|(m, d)| {
                    digits(m, 2) && in_range(m, 1, 12) && digits(d, 2) && in_range(d, 1, 31)
                })
            }),
        Builtin::GDay => strip_tz(v)
            .and_then(|s| s.strip_prefix("---"))
            .is_some_and(|d| digits(d, 2) && in_range(d, 1, 31)),
        Builtin::GMonth => strip_tz(v)
            .and_then(|s| s.strip_prefix("--"))
            .is_some_and(|m| digits(m, 2) && in_range(m, 1, 12)),
        Builtin::HexBinary => v.len().is_multiple_of(2) && v.bytes().all(|b| b.is_ascii_hexdigit()),
        Builtin::Base64Binary => is_base64(v),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("must be a valid {}", builtin_label(b)))
    }
}

fn builtin_label(b: Builtin) -> &'static str {
    match b {
        Builtin::AnySimpleType | Builtin::String => "string",
        Builtin::NormalizedString => "normalizedString",
        Builtin::Token => "token",
        Builtin::Language => "language code",
        Builtin::Name => "XML name",
        Builtin::NCName => "NCName",
        Builtin::Id => "ID",
        Builtin::IdRef => "IDREF",
        Builtin::Entity => "ENTITY name",
        Builtin::NmToken => "NMTOKEN",
        Builtin::QName => "QName",
        Builtin::Notation => "NOTATION name",
        Builtin::AnyUri => "URI",
        Builtin::Boolean => "boolean (true, false, 1, 0)",
        Builtin::Decimal => "decimal number",
        Builtin::Integer => "integer",
        Builtin::Float => "float",
        Builtin::Double => "double",
        Builtin::Duration => "duration (e.g. P1DT2H)",
        Builtin::DateTime => "dateTime (YYYY-MM-DDThh:mm:ss)",
        Builtin::Date => "date (YYYY-MM-DD)",
        Builtin::Time => "time (hh:mm:ss)",
        Builtin::GYearMonth => "gYearMonth (YYYY-MM)",
        Builtin::GYear => "year (YYYY)",
        Builtin::GMonthDay => "gMonthDay (--MM-DD)",
        Builtin::GDay => "gDay (---DD)",
        Builtin::GMonth => "gMonth (--MM)",
        Builtin::HexBinary => "hexBinary",
        Builtin::Base64Binary => "base64Binary",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(t: &TypeTable, ty: &str, v: &str) -> bool {
        t.validate(t.builtin(ty).unwrap(), v).is_ok()
    }

    #[test]
    fn builtin_lexical_spaces() {
        let t = TypeTable::new();
        assert!(ok(&t, "boolean", " true "));
        assert!(!ok(&t, "boolean", "yes"));
        assert!(ok(&t, "integer", "-0042"));
        assert!(!ok(&t, "integer", "4.2"));
        assert!(ok(&t, "decimal", "4.2"));
        assert!(ok(&t, "decimal", ".5"));
        assert!(!ok(&t, "decimal", "."));
        assert!(ok(&t, "double", "1.5E-3"));
        assert!(ok(&t, "float", "INF"));
        assert!(ok(&t, "positiveInteger", "1"));
        assert!(!ok(&t, "positiveInteger", "0"));
        assert!(!ok(&t, "byte", "128"));
        assert!(ok(&t, "unsignedLong", "18446744073709551615"));
        assert!(!ok(&t, "unsignedLong", "18446744073709551616"));
        assert!(ok(&t, "date", "2024-02-29"));
        assert!(ok(&t, "date", "2024-02-29Z"));
        assert!(!ok(&t, "date", "2024-13-01"));
        assert!(ok(&t, "dateTime", "2024-02-29T10:20:30.5+01:00"));
        assert!(!ok(&t, "dateTime", "2024-02-29 10:20:30"));
        assert!(ok(&t, "time", "23:59:59"));
        assert!(ok(&t, "gYear", "2024"));
        assert!(!ok(&t, "gYear", "24"));
        assert!(ok(&t, "gYearMonth", "2024-05"));
        assert!(ok(&t, "gMonthDay", "--05-31"));
        assert!(ok(&t, "gDay", "---31"));
        assert!(ok(&t, "gMonth", "--12"));
        assert!(ok(&t, "duration", "P1Y2M3DT4H5M6.7S"));
        assert!(ok(&t, "duration", "-PT5M"));
        assert!(!ok(&t, "duration", "P"));
        assert!(!ok(&t, "duration", "PT"));
        assert!(!ok(&t, "duration", "P1S"));
        assert!(ok(&t, "hexBinary", "0aFF"));
        assert!(!ok(&t, "hexBinary", "0aF"));
        assert!(ok(&t, "base64Binary", "SGVsbG8="));
        assert!(ok(&t, "ID", " a-1 "));
        assert!(!ok(&t, "ID", "1a"));
        assert!(!ok(&t, "NCName", "a:b"));
        assert!(ok(&t, "QName", "a:b"));
        assert!(ok(&t, "NMTOKEN", "1a"));
        assert!(!ok(&t, "NMTOKEN", "a b"));
        assert!(ok(&t, "NMTOKENS", " a  b "));
        assert!(!ok(&t, "IDREFS", "  "));
        assert!(ok(&t, "language", "en-US"));
        assert!(ok(&t, "string", "  anything "));
    }

    #[test]
    fn facets() {
        let mut t = TypeTable::new();
        let mut st = t.restrict(t.builtin("NMTOKEN").unwrap());
        st.facets.set_enumeration(vec!["a".into(), "b".into()]);
        let e = t.add(st);
        assert!(t.validate(e, " a ").is_ok());
        assert_eq!(t.validate(e, "c").unwrap_err(), "allowed: a, b");

        let mut st = t.restrict(t.builtin("string").unwrap());
        st.facets
            .patterns
            .push(Arc::new(PatternSet::new(vec![r"\d{2}".into(), "x".into()])));
        st.facets.max_length = Some(2);
        let p = t.add(st);
        assert!(t.validate(p, "12").is_ok());
        assert!(t.validate(p, "x").is_ok());
        assert!(t.validate(p, "123").is_err());

        let mut st = t.restrict(t.builtin("decimal").unwrap());
        st.facets.min_exclusive = Some("-1.5".into());
        st.facets.max_inclusive = Some("100".into());
        st.facets.fraction_digits = Some(2);
        let d = t.add(st);
        assert!(t.validate(d, "-1.25").is_ok());
        assert!(t.validate(d, "-1.5").is_err());
        assert!(t.validate(d, "100.00").is_ok());
        assert!(t.validate(d, "100.01").is_err());
        assert!(t.validate(d, "1.234").is_err());

        let mut st = t.restrict(t.builtin("gYear").unwrap());
        st.facets.max_inclusive = Some("2030".into());
        let y = t.add(st);
        assert!(t.validate(y, "2029").is_ok());
        assert!(t.validate(y, "2031").is_err());
    }

    #[test]
    fn decimal_ordering() {
        assert_eq!(compare_decimal("10", "9"), Some(Ordering::Greater));
        assert_eq!(compare_decimal("-10", "-9"), Some(Ordering::Less));
        assert_eq!(compare_decimal("-0", "0.000"), Some(Ordering::Equal));
        assert_eq!(compare_decimal("1.50", "1.5"), Some(Ordering::Equal));
        assert_eq!(compare_decimal("x", "1"), None);
    }

    #[test]
    fn whitespace() {
        assert_eq!(WhiteSpace::Collapse.apply("  a \n b  "), "a b");
        assert_eq!(WhiteSpace::Replace.apply("a\tb"), "a b");
        assert_eq!(WhiteSpace::Preserve.apply(" a "), " a ");
    }

    #[test]
    fn union_and_list() {
        let mut t = TypeTable::new();
        let union = SimpleType {
            name: None,
            builtin: Builtin::AnySimpleType,
            variety: Variety::Union(vec![
                t.builtin("integer").unwrap(),
                t.builtin("boolean").unwrap(),
            ]),
            whitespace: WhiteSpace::Collapse,
            facets: Facets::default(),
        };
        let u = t.add(union);
        assert!(t.validate(u, "12").is_ok());
        assert!(t.validate(u, "true").is_ok());
        assert!(t.validate(u, "x").is_err());
        let list = SimpleType {
            name: None,
            builtin: Builtin::Integer,
            variety: Variety::List(t.builtin("integer").unwrap()),
            whitespace: WhiteSpace::Collapse,
            facets: Facets {
                max_length: Some(2),
                ..Facets::default()
            },
        };
        let l = t.add(list);
        assert!(t.validate(l, "1 2").is_ok());
        assert!(t.validate(l, "1 2 3").is_err());
        assert!(t.validate(l, "1 x").is_err());
    }
}
