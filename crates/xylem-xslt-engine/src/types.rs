//! Built-in atomic types beyond the primitives the engine stores directly:
//! the derived integer and string types, `xs:anyURI`, `xs:float`, and the
//! type hierarchy used by `instance of` and casting.
//!
//! A derived value is an [`Atomic::Sub`]: the type's name and the value of
//! its primitive base (an `Int` for the integer types, a `Str` for the
//! string types and `xs:anyURI`, a `Dbl` for `xs:float`).

use crate::value::Atomic;
use std::sync::Arc;

/// The type each built-in derived type is derived from.
pub fn parent(t: &str) -> Option<&'static str> {
    Some(match t {
        "integer" => "decimal",
        "nonPositiveInteger" | "long" | "nonNegativeInteger" => "integer",
        "negativeInteger" => "nonPositiveInteger",
        "int" => "long",
        "short" => "int",
        "byte" => "short",
        "unsignedLong" | "positiveInteger" => "nonNegativeInteger",
        "unsignedInt" => "unsignedLong",
        "unsignedShort" => "unsignedInt",
        "unsignedByte" => "unsignedShort",
        "normalizedString" => "string",
        "token" => "normalizedString",
        "language" | "NMTOKEN" | "Name" => "token",
        "NCName" => "Name",
        "ID" | "IDREF" | "ENTITY" => "NCName",
        "yearMonthDuration" | "dayTimeDuration" => "duration",
        _ => return None,
    })
}

/// Whether type `t` is `target` or derived from it.
pub fn derives(t: &str, target: &str) -> bool {
    let mut cur = t;
    loop {
        if cur == target {
            return true;
        }
        match parent(cur) {
            Some(p) => cur = p,
            None => return false,
        }
    }
}

/// The (local) type name of a value.
pub fn type_of(a: &Atomic) -> &str {
    match a {
        Atomic::Sub(t, _) => t,
        Atomic::Time(t) => t.kind.name(),
        Atomic::Dur(d) => d.kind.name(),
        Atomic::QName(_) => "QName",
        o => o.type_name().trim_start_matches("xs:"),
    }
}

/// `instance of xs:<target>` for an atomic value.
pub fn instance(a: &Atomic, target: &str) -> bool {
    target == "anyAtomicType" || derives(type_of(a), target)
}

const INT_RANGES: &[(&str, i128, i128)] = &[
    ("nonPositiveInteger", i64::MIN as i128, 0),
    ("negativeInteger", i64::MIN as i128, -1),
    ("long", i64::MIN as i128, i64::MAX as i128),
    ("int", i32::MIN as i128, i32::MAX as i128),
    ("short", i16::MIN as i128, i16::MAX as i128),
    ("byte", i8::MIN as i128, i8::MAX as i128),
    ("nonNegativeInteger", 0, i64::MAX as i128),
    ("unsignedLong", 0, i64::MAX as i128),
    ("unsignedInt", 0, u32::MAX as i128),
    ("unsignedShort", 0, u16::MAX as i128),
    ("unsignedByte", 0, u8::MAX as i128),
    ("positiveInteger", 1, i64::MAX as i128),
];

/// The interned name of a derived type (so values can hold `&'static str`).
fn intern(t: &str) -> Option<&'static str> {
    const NAMES: &[&str] = &[
        "nonPositiveInteger",
        "negativeInteger",
        "long",
        "int",
        "short",
        "byte",
        "nonNegativeInteger",
        "unsignedLong",
        "unsignedInt",
        "unsignedShort",
        "unsignedByte",
        "positiveInteger",
        "normalizedString",
        "token",
        "language",
        "NMTOKEN",
        "Name",
        "NCName",
        "ID",
        "IDREF",
        "ENTITY",
        "anyURI",
        "float",
        "hexBinary",
        "base64Binary",
    ];
    NAMES.iter().copied().find(|n| *n == t)
}

fn is_name_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == ':'
}

fn is_name_char(c: char) -> bool {
    is_name_start(c) || c.is_alphanumeric() || matches!(c, '-' | '.' | '\u{B7}')
}

/// Whether `s` is an `xs:NCName`.
pub fn is_ncname(s: &str) -> bool {
    let mut c = s.chars();
    c.next().is_some_and(|c| is_name_start(c) && c != ':') && c.all(|c| is_name_char(c) && c != ':')
}

/// Whether `s` is acceptable as an `xs:anyURI`: every `%` starts an
/// escape, and a scheme, when present, is not empty.
fn is_uri(s: &str) -> bool {
    let b = s.as_bytes();
    let escapes = b.iter().enumerate().all(|(i, c)| {
        *c != b'%'
            || (b.len() > i + 2 && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit())
    });
    escapes && !s.starts_with(':')
}

/// Checks and normalises the lexical form of a derived string type.
fn string_value(t: &str, s: &str) -> Option<String> {
    let replaced: String = s
        .chars()
        .map(|c| {
            if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            }
        })
        .collect();
    if t == "normalizedString" {
        return Some(replaced);
    }
    let collapsed = replaced
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let ok = match t {
        "token" => true,
        "language" => {
            let mut parts = collapsed.split('-');
            let first = parts.next().unwrap_or("");
            (1..=8).contains(&first.len())
                && first.chars().all(|c| c.is_ascii_alphabetic())
                && parts.all(|p| {
                    (1..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphanumeric())
                })
        }
        "NMTOKEN" => !collapsed.is_empty() && collapsed.chars().all(is_name_char),
        "Name" => {
            let mut c = collapsed.chars();
            c.next().is_some_and(is_name_start) && c.all(is_name_char)
        }
        _ => {
            // NCName and its subtypes.
            let mut c = collapsed.chars();
            c.next().is_some_and(|c| is_name_start(c) && c != ':')
                && c.all(|c| is_name_char(c) && c != ':')
        }
    };
    ok.then_some(collapsed)
}

/// Casts to a type the core `cast` does not handle; None when `t` is not a
/// known type at all.
pub fn cast(a: &Atomic, t: &str) -> Option<Result<Atomic, String>> {
    if let Some(r) = crate::datetime::cast(a, t) {
        return Some(r);
    }
    let name = intern(t)?;
    let s = a.to_string();
    let bad = || format!("FORG0001: cannot cast '{s}' to xs:{t}");
    let prim = a.prim();
    let lexical = matches!(prim, Atomic::Str(_) | Atomic::Untyped(_));
    Some((|| {
        if let Some(&(_, lo, hi)) = INT_RANGES.iter().find(|r| r.0 == t) {
            let v: i128 = match &prim {
                Atomic::Int(i) => *i as i128,
                Atomic::Dec(d) => d.0 / crate::value::DEC_SCALE,
                Atomic::Dbl(d) if d.is_finite() => d.trunc() as i128,
                Atomic::Bool(b) => *b as i128,
                _ if lexical => s
                    .trim()
                    .strip_prefix('+')
                    .unwrap_or(s.trim())
                    .parse()
                    .map_err(|_| bad())?,
                _ => return Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name())),
            };
            if v < lo || v > hi {
                return Err(format!("FORG0001: {v} is out of range for xs:{t}"));
            }
            return Ok(Atomic::Sub(name, Arc::new(Atomic::Int(v as i64))));
        }
        match t {
            "anyURI" if lexical || matches!(a, Atomic::Sub("anyURI", _)) => {
                let v = s.trim_matches(crate::tree::is_xml_space);
                if !is_uri(v) {
                    return Err(bad());
                }
                Ok(Atomic::Sub(name, Arc::new(Atomic::str(v))))
            }
            "float" => {
                let d = match &prim {
                    _ if lexical => {
                        let d = crate::value::parse_double(&s);
                        if d.is_nan() && s.trim() != "NaN" {
                            return Err(bad());
                        }
                        d
                    }
                    Atomic::Bool(b) => *b as i64 as f64,
                    o if o.is_numeric() => o.to_f64(),
                    _ => {
                        return Err(format!(
                            "XPTY0004: cannot cast {} to xs:float",
                            a.type_name()
                        ));
                    }
                };
                Ok(Atomic::Sub(name, Arc::new(Atomic::Dbl(d as f32 as f64))))
            }
            "hexBinary" | "base64Binary" => {
                let bytes = match a {
                    Atomic::Sub("hexBinary", v) => hex_decode(&v.to_string()),
                    Atomic::Sub("base64Binary", v) => base64_decode(&v.to_string()),
                    _ if lexical && t == "hexBinary" => {
                        hex_decode(s.trim_matches(crate::tree::is_xml_space))
                    }
                    _ if lexical => base64_decode(&s),
                    _ => return Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name())),
                }
                .ok_or_else(bad)?;
                let text = if t == "hexBinary" {
                    hex_encode(&bytes)
                } else {
                    base64_encode(&bytes)
                };
                Ok(Atomic::Sub(name, Arc::new(Atomic::str(text))))
            }
            "anyURI" => Err(format!(
                "XPTY0004: cannot cast {} to xs:anyURI",
                a.type_name()
            )),
            _ if matches!(a, Atomic::Sub("hexBinary" | "base64Binary", _)) => {
                Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name()))
            }
            _ if lexical || matches!(a, Atomic::Sub(..)) => string_value(t, &s)
                .map(|v| Atomic::Sub(name, Arc::new(Atomic::str(v))))
                .ok_or_else(bad),
            _ => Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name())),
        }
    })())
}

/// Decodes the lexical form of `xs:hexBinary`.
fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if !b.len().is_multiple_of(2) {
        return None;
    }
    b.chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

/// The canonical (upper-case) form of `xs:hexBinary`.
fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Decodes the lexical form of `xs:base64Binary`: spaces allowed between
/// characters, padding checked as XML Schema requires.
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let chars: Vec<u8> = s.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if !chars.len().is_multiple_of(4) {
        return None;
    }
    let pad = chars.iter().rev().take_while(|c| **c == b'=').count();
    if pad > 2 || chars[..chars.len() - pad].contains(&b'=') {
        return None;
    }
    let val = |c: u8| B64.iter().position(|x| *x == c).map(|p| p as u32);
    let mut out = Vec::new();
    for q in chars.chunks(4) {
        let mut n = 0u32;
        let mut used = 0;
        for &c in q {
            n <<= 6;
            if c != b'=' {
                n |= val(c)?;
                used += 1;
            }
        }
        let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        match used {
            4 => out.extend_from_slice(&bytes),
            3 if n & 0xFF == 0 => out.extend_from_slice(&bytes[..2]),
            2 if n & 0xFFFF == 0 => out.push(bytes[0]),
            _ => return None,
        }
    }
    Some(out)
}

/// The canonical form of `xs:base64Binary`.
fn base64_encode(b: &[u8]) -> String {
    let mut out = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
