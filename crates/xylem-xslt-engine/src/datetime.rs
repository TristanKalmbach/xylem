//! XML Schema date, time and duration values (`xs:dateTime`, `xs:date`,
//! `xs:time`, the `xs:g*` types, `xs:duration` and its two subtypes):
//! parsing, canonical forms, comparison, casting and arithmetic, following
//! XPath / XQuery Functions and Operators 2.0 (sections 10 and 17).
//!
//! Seconds are kept as a [`Decimal`] so fractional seconds stay exact.

use crate::value::{Atomic, DEC_SCALE, Decimal};
use std::cmp::Ordering;
use std::rc::Rc;

/// Which date/time type a [`DateTime`] is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DtKind {
    DateTime,
    Date,
    Time,
    GYearMonth,
    GYear,
    GMonthDay,
    GDay,
    GMonth,
}

impl DtKind {
    pub fn name(self) -> &'static str {
        match self {
            DtKind::DateTime => "dateTime",
            DtKind::Date => "date",
            DtKind::Time => "time",
            DtKind::GYearMonth => "gYearMonth",
            DtKind::GYear => "gYear",
            DtKind::GMonthDay => "gMonthDay",
            DtKind::GDay => "gDay",
            DtKind::GMonth => "gMonth",
        }
    }

    pub fn from_name(n: &str) -> Option<DtKind> {
        Some(match n {
            "dateTime" => DtKind::DateTime,
            "date" => DtKind::Date,
            "time" => DtKind::Time,
            "gYearMonth" => DtKind::GYearMonth,
            "gYear" => DtKind::GYear,
            "gMonthDay" => DtKind::GMonthDay,
            "gDay" => DtKind::GDay,
            "gMonth" => DtKind::GMonth,
            _ => return None,
        })
    }

    fn has_date(self) -> bool {
        !matches!(self, DtKind::Time)
    }
}

/// A date/time value. Fields a kind does not have hold the reference
/// values F&O uses for comparison (year 1972, month 12, day 31, midnight).
#[derive(Clone, Debug, PartialEq)]
pub struct DateTime {
    pub kind: DtKind,
    pub year: i64,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: Decimal,
    /// Timezone offset in minutes, if the value has one.
    pub tz: Option<i32>,
}

/// Which duration type a [`Duration`] is.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DurKind {
    Duration,
    YearMonth,
    DayTime,
}

impl DurKind {
    pub fn name(self) -> &'static str {
        match self {
            DurKind::Duration => "duration",
            DurKind::YearMonth => "yearMonthDuration",
            DurKind::DayTime => "dayTimeDuration",
        }
    }
}

/// A duration: whole months and (decimal) seconds, both of the same sign.
#[derive(Clone, Debug, PartialEq)]
pub struct Duration {
    pub kind: DurKind,
    pub months: i64,
    pub seconds: Decimal,
}

/// The implicit timezone (F&O: implementation-defined): UTC.
pub const IMPLICIT_TZ: i32 = 0;

const SECS_PER_DAY: i128 = 86_400;

fn dec_secs(s: i128) -> Decimal {
    Decimal(s * DEC_SCALE)
}

// ---- calendar -------------------------------------------------------------

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: u8) -> u8 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (year 0 = 1 BCE).
fn days_from_civil(y: i64, m: u8, d: u8) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u8, u8) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// XML Schema 1.0 has no year 0: lexical year -1 is 1 BCE. Internally the
/// year is astronomical (0 = 1 BCE), so lexical negative years are shifted.
fn lexical_to_astro(y: i64) -> i64 {
    if y < 0 { y + 1 } else { y }
}

fn astro_to_lexical(y: i64) -> i64 {
    if y <= 0 { y - 1 } else { y }
}

// ---- parsing --------------------------------------------------------------

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn eat(&mut self, c: u8) -> bool {
        if self.s.get(self.i) == Some(&c) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn digits(&mut self, min: usize, max: usize) -> Option<(i64, usize)> {
        let st = self.i;
        while self.i < self.s.len() && self.s[self.i].is_ascii_digit() && self.i - st < max {
            self.i += 1;
        }
        let n = self.i - st;
        if n < min {
            return None;
        }
        let v = std::str::from_utf8(&self.s[st..self.i]).ok()?.parse().ok()?;
        Some((v, n))
    }
    fn fixed(&mut self, n: usize) -> Option<i64> {
        self.digits(n, n).map(|(v, _)| v)
    }
    fn done(&self) -> bool {
        self.i == self.s.len()
    }
}

fn parse_year(c: &mut Cursor) -> Option<i64> {
    let neg = c.eat(b'-');
    let st = c.i;
    let (v, n) = c.digits(4, 12)?;
    // More than four digits may not start with 0.
    if n > 4 && c.s[st] == b'0' {
        return None;
    }
    if v == 0 {
        return None; // year 0000 is not allowed in XML Schema 1.0
    }
    Some(lexical_to_astro(if neg { -v } else { v }))
}

fn parse_tz(c: &mut Cursor) -> Option<Option<i32>> {
    if c.done() {
        return Some(None);
    }
    if c.eat(b'Z') {
        return Some(Some(0));
    }
    let sign = if c.eat(b'+') {
        1
    } else if c.eat(b'-') {
        -1
    } else {
        return None;
    };
    let h = c.fixed(2)?;
    if !c.eat(b':') {
        return None;
    }
    let m = c.fixed(2)?;
    if m > 59 || h > 14 || (h == 14 && m != 0) {
        return None;
    }
    Some(Some(sign * (h * 60 + m) as i32))
}

/// hh:mm:ss(.s+)? ; returns (hour, minute, second) with 24:00:00 allowed.
fn parse_time(c: &mut Cursor) -> Option<(u8, u8, Decimal)> {
    let h = c.fixed(2)?;
    if !c.eat(b':') {
        return None;
    }
    let m = c.fixed(2)?;
    if !c.eat(b':') {
        return None;
    }
    let st = c.i;
    c.fixed(2)?;
    if c.eat(b'.') {
        c.digits(1, usize::MAX)?;
    }
    let sec = Decimal::parse(std::str::from_utf8(&c.s[st..c.i]).ok()?)?;
    if m > 59 || sec >= dec_secs(60) {
        return None;
    }
    if h == 24 {
        if m != 0 || sec != Decimal(0) {
            return None;
        }
    } else if h > 23 {
        return None;
    }
    Some((h as u8, m as u8, sec))
}

impl DateTime {
    /// Parses the lexical form of `kind` (whitespace already collapsed).
    pub fn parse(kind: DtKind, s: &str) -> Option<DateTime> {
        let s = s.trim_matches(crate::tree::is_xml_space);
        let mut c = Cursor { s: s.as_bytes(), i: 0 };
        let mut v = DateTime {
            kind,
            year: 1972,
            month: 12,
            day: 31,
            hour: 0,
            minute: 0,
            second: Decimal(0),
            tz: None,
        };
        let mut end_of_day = false;
        match kind {
            DtKind::DateTime | DtKind::Date | DtKind::GYearMonth | DtKind::GYear => {
                v.year = parse_year(&mut c)?;
                if kind != DtKind::GYear {
                    if !c.eat(b'-') {
                        return None;
                    }
                    v.month = c.fixed(2)? as u8;
                    if kind == DtKind::GYearMonth {
                        v.day = 1;
                    } else {
                        if !c.eat(b'-') {
                            return None;
                        }
                        v.day = c.fixed(2)? as u8;
                    }
                } else {
                    v.month = 1;
                    v.day = 1;
                }
                if kind == DtKind::DateTime {
                    if !c.eat(b'T') {
                        return None;
                    }
                    let (h, m, sec) = parse_time(&mut c)?;
                    end_of_day = h == 24;
                    (v.hour, v.minute, v.second) = (if end_of_day { 0 } else { h }, m, sec);
                }
            }
            DtKind::Time => {
                let (h, m, sec) = parse_time(&mut c)?;
                (v.hour, v.minute, v.second) = (if h == 24 { 0 } else { h }, m, sec);
            }
            DtKind::GMonthDay | DtKind::GDay | DtKind::GMonth => {
                if !c.eat(b'-') || !c.eat(b'-') {
                    return None;
                }
                if kind == DtKind::GDay {
                    if !c.eat(b'-') {
                        return None;
                    }
                    v.day = c.fixed(2)? as u8;
                } else {
                    v.month = c.fixed(2)? as u8;
                    if kind == DtKind::GMonthDay {
                        if !c.eat(b'-') {
                            return None;
                        }
                        v.day = c.fixed(2)? as u8;
                    } else {
                        v.day = 1;
                    }
                }
            }
        }
        v.tz = parse_tz(&mut c)?;
        if !c.done() {
            return None;
        }
        if !(1..=12).contains(&v.month) {
            return None;
        }
        // gMonthDay allows --02-29; others must be a real day.
        let max = if kind == DtKind::GMonthDay || kind == DtKind::GDay {
            days_in_month(2000, v.month)
        } else {
            days_in_month(v.year, v.month)
        };
        if v.day < 1 || v.day > max.max(if kind == DtKind::GDay { 31 } else { 0 }) {
            return None;
        }
        if end_of_day {
            v = v.add_seconds(dec_secs(SECS_PER_DAY));
        }
        Some(v)
    }

    /// Seconds since 1970-01-01T00:00:00Z, the timezone (or `implicit`)
    /// applied.
    pub fn timeline(&self, implicit: i32) -> Decimal {
        let days = days_from_civil(self.year, self.month, self.day) as i128;
        let tz = self.tz.unwrap_or(implicit) as i128;
        let secs =
            days * SECS_PER_DAY + self.hour as i128 * 3600 + self.minute as i128 * 60 - tz * 60;
        Decimal(secs * DEC_SCALE + self.second.0)
    }

    /// The same local value moved by `s` seconds (timezone unchanged).
    fn add_seconds(&self, s: Decimal) -> DateTime {
        let local = Decimal(self.timeline(0).0 + self.tz.unwrap_or(0) as i128 * 60 * DEC_SCALE);
        let total = local.0 + s.0;
        let whole = total.div_euclid(DEC_SCALE);
        let frac = total.rem_euclid(DEC_SCALE);
        let days = whole.div_euclid(SECS_PER_DAY);
        let rem = whole.rem_euclid(SECS_PER_DAY);
        let (y, m, d) = civil_from_days(days as i64);
        DateTime {
            year: y,
            month: m,
            day: d,
            hour: (rem / 3600) as u8,
            minute: (rem % 3600 / 60) as u8,
            second: Decimal((rem % 60) * DEC_SCALE + frac),
            ..self.clone()
        }
    }

    fn add_months(&self, months: i64) -> DateTime {
        let total = self.year * 12 + (self.month as i64 - 1) + months;
        let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u8);
        DateTime {
            year: y,
            month: m,
            day: self.day.min(days_in_month(y, m)),
            ..self.clone()
        }
    }

    /// The value with its timezone changed to `tz` (`fn:adjust-*-to-timezone`).
    pub fn adjust(&self, tz: Option<i32>) -> DateTime {
        match (self.tz, tz) {
            (_, None) => DateTime { tz: None, ..self.clone() },
            (None, Some(t)) => DateTime { tz: Some(t), ..self.clone() },
            (Some(old), Some(t)) => {
                let moved = self.add_seconds(dec_secs((t - old) as i128 * 60));
                let mut r = DateTime { tz: Some(t), ..moved };
                if self.kind == DtKind::Time {
                    (r.year, r.month, r.day) = (1972, 12, 31);
                }
                r
            }
        }
    }

    /// Converts to another date/time type (casting).
    pub fn convert(&self, to: DtKind) -> Option<DateTime> {
        use DtKind::*;
        let ok = match (self.kind, to) {
            (a, b) if a == b => true,
            (DateTime, _) => true,
            (Date, DateTime | GYearMonth | GYear | GMonthDay | GDay | GMonth) => true,
            _ => false,
        };
        if !ok {
            return None;
        }
        let mut r = self.clone();
        r.kind = to;
        if to != DateTime && to != Time {
            (r.hour, r.minute, r.second) = (0, 0, Decimal(0));
        }
        match to {
            Time => (r.year, r.month, r.day) = (1972, 12, 31),
            GYear => (r.month, r.day) = (1, 1),
            GYearMonth | GMonth => r.day = 1,
            GDay | GMonthDay => r.year = 1972,
            _ => {}
        }
        if to == GMonth || to == GDay {
            r.year = 1972;
        }
        Some(r)
    }

    pub fn to_canonical(&self) -> String {
        let year = || {
            let y = astro_to_lexical(self.year);
            if y < 0 {
                format!("-{:04}", -y)
            } else {
                format!("{y:04}")
            }
        };
        let time = || {
            let sec = self.second;
            let whole = sec.0 / DEC_SCALE;
            let frac = Decimal(sec.0 % DEC_SCALE).to_canonical();
            let frac = frac.strip_prefix('0').unwrap_or("");
            format!("{:02}:{:02}:{:02}{}", self.hour, self.minute, whole, frac)
        };
        let mut s = match self.kind {
            DtKind::DateTime => {
                format!("{}-{:02}-{:02}T{}", year(), self.month, self.day, time())
            }
            DtKind::Date => format!("{}-{:02}-{:02}", year(), self.month, self.day),
            DtKind::Time => time(),
            DtKind::GYearMonth => format!("{}-{:02}", year(), self.month),
            DtKind::GYear => year(),
            DtKind::GMonthDay => format!("--{:02}-{:02}", self.month, self.day),
            DtKind::GDay => format!("---{:02}", self.day),
            DtKind::GMonth => format!("--{:02}", self.month),
        };
        if let Some(tz) = self.tz {
            s.push_str(&format_tz(tz));
        }
        s
    }
}

pub fn format_tz(tz: i32) -> String {
    if tz == 0 {
        return "Z".into();
    }
    let sign = if tz < 0 { '-' } else { '+' };
    format!("{sign}{:02}:{:02}", tz.abs() / 60, tz.abs() % 60)
}

// ---- durations ------------------------------------------------------------

impl Duration {
    pub fn zero(kind: DurKind) -> Duration {
        Duration { kind, months: 0, seconds: Decimal(0) }
    }

    /// Parses `-?PnYnMnDTnHnMnS` for `kind`; the subtypes allow only their
    /// own fields.
    pub fn parse(kind: DurKind, s: &str) -> Option<Duration> {
        let s = s.trim_matches(crate::tree::is_xml_space);
        let (neg, rest) = match s.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, s),
        };
        let rest = rest.strip_prefix('P')?;
        let (date, time) = match rest.split_once('T') {
            Some((d, t)) => {
                if t.is_empty() {
                    return None;
                }
                (d, Some(t))
            }
            None => (rest, None),
        };
        if date.is_empty() && time.is_none() {
            return None;
        }
        let mut months: i128 = 0;
        let mut secs: i128 = 0;
        let mut any = false;
        let mut take = |part: &str, units: &[(char, i128, bool)], allow_frac: char| -> Option<()> {
            let mut p = part;
            let mut last = 0usize;
            while !p.is_empty() {
                let end = p.find(|c: char| !(c.is_ascii_digit() || c == '.'))?;
                let (num, tail) = p.split_at(end);
                let unit = tail.chars().next()?;
                let k = units.iter().position(|u| u.0 == unit)?;
                if k < last || num.is_empty() {
                    return None;
                }
                last = k + 1;
                let (_, mult, is_month) = units[k];
                if num.contains('.') {
                    if unit != allow_frac || num.ends_with('.') || num.starts_with('.') {
                        return None;
                    }
                    let d = Decimal::parse(num)?;
                    secs = secs.checked_add(d.0)?;
                } else {
                    let n: i128 = num.parse().ok()?;
                    if is_month {
                        months = months.checked_add(n.checked_mul(mult)?)?;
                    } else {
                        secs = secs.checked_add(n.checked_mul(mult)?.checked_mul(DEC_SCALE)?)?;
                    }
                }
                any = true;
                p = &tail[1..];
            }
            Some(())
        };
        take(date, &[('Y', 12, true), ('M', 1, true), ('D', SECS_PER_DAY, false)], '\0')?;
        if let Some(t) = time {
            take(t, &[('H', 3600, false), ('M', 60, false), ('S', 1, false)], 'S')?;
        }
        if !any {
            return None;
        }
        match kind {
            DurKind::YearMonth if date.contains('D') || time.is_some() => return None,
            DurKind::DayTime if date.contains('Y') || date.contains('M') => return None,
            _ => {}
        }
        let sign = if neg { -1 } else { 1 };
        Some(Duration {
            kind,
            months: i64::try_from(months * sign).ok()?,
            seconds: Decimal(secs * sign),
        })
    }

    pub fn to_canonical(&self) -> String {
        let neg = self.months < 0 || self.seconds.0 < 0;
        let months = self.months.unsigned_abs();
        let secs = self.seconds.0.abs();
        let mut s = String::from(if neg { "-P" } else { "P" });
        let (y, m) = (months / 12, months % 12);
        if self.kind == DurKind::YearMonth {
            if y > 0 {
                s.push_str(&format!("{y}Y"));
            }
            if m > 0 || y == 0 {
                s.push_str(&format!("{m}M"));
            }
            return s;
        }
        if y > 0 {
            s.push_str(&format!("{y}Y"));
        }
        if m > 0 {
            s.push_str(&format!("{m}M"));
        }
        let whole = secs / DEC_SCALE;
        let frac = secs % DEC_SCALE;
        let (d, h, mi, se) = (
            whole / SECS_PER_DAY,
            whole % SECS_PER_DAY / 3600,
            whole % 3600 / 60,
            whole % 60,
        );
        if d > 0 {
            s.push_str(&format!("{d}D"));
        }
        if h > 0 || mi > 0 || se > 0 || frac > 0 {
            s.push('T');
            if h > 0 {
                s.push_str(&format!("{h}H"));
            }
            if mi > 0 {
                s.push_str(&format!("{mi}M"));
            }
            if se > 0 || frac > 0 {
                s.push_str(&Decimal(se * DEC_SCALE + frac).to_canonical());
                s.push('S');
            }
        }
        if s.ends_with('P') {
            s.push_str("T0S");
        }
        s
    }

    pub fn convert(&self, to: DurKind) -> Duration {
        match to {
            DurKind::Duration => Duration { kind: to, ..self.clone() },
            DurKind::YearMonth => Duration { kind: to, months: self.months, seconds: Decimal(0) },
            DurKind::DayTime => Duration { kind: to, months: 0, seconds: self.seconds },
        }
    }
}

// ---- atomic-level operations ---------------------------------------------

/// Casts `a` to the date/time or duration type `t`; None if `t` is not one.
pub fn cast(a: &Atomic, t: &str) -> Option<Result<Atomic, String>> {
    let bad = || format!("FORG0001: cannot cast '{}' to xs:{t}", a.to_string());
    let lexical = matches!(a, Atomic::Str(_) | Atomic::Untyped(_));
    if let Some(kind) = DtKind::from_name(t) {
        return Some(match a {
            Atomic::Time(dt) => dt
                .convert(kind)
                .map(|v| Atomic::Time(Rc::new(v)))
                .ok_or_else(|| format!("XPTY0004: cannot cast xs:{} to xs:{t}", dt.kind.name())),
            _ if lexical => DateTime::parse(kind, &a.to_string())
                .map(|v| Atomic::Time(Rc::new(v)))
                .ok_or_else(bad),
            _ => Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name())),
        });
    }
    let kind = match t {
        "duration" => DurKind::Duration,
        "yearMonthDuration" => DurKind::YearMonth,
        "dayTimeDuration" => DurKind::DayTime,
        _ => return None,
    };
    Some(match a {
        Atomic::Dur(d) => Ok(Atomic::Dur(Rc::new(d.convert(kind)))),
        _ if lexical => Duration::parse(kind, &a.to_string())
            .map(|v| Atomic::Dur(Rc::new(v)))
            .ok_or_else(bad),
        _ => Err(format!("XPTY0004: cannot cast {} to xs:{t}", a.type_name())),
    })
}

/// Compares two date/time or two duration values; None when they are not
/// of comparable types (the caller reports the error). `Some(None)` means
/// "not equal, unordered" (durations of the general type).
pub fn compare(p: &Atomic, q: &Atomic) -> Option<Option<Ordering>> {
    match (p, q) {
        (Atomic::Time(a), Atomic::Time(b)) if a.kind == b.kind => {
            Some(a.timeline(IMPLICIT_TZ).partial_cmp(&b.timeline(IMPLICIT_TZ)))
        }
        (Atomic::Dur(a), Atomic::Dur(b)) => {
            if a.months == b.months && a.seconds == b.seconds {
                return Some(Some(Ordering::Equal));
            }
            match (a.kind, b.kind) {
                (DurKind::YearMonth, DurKind::YearMonth) => Some(Some(a.months.cmp(&b.months))),
                (DurKind::DayTime, DurKind::DayTime) => Some(Some(a.seconds.cmp(&b.seconds))),
                // Unequal and not ordered: report "less" so eq is false.
                _ => Some(Some(Ordering::Less)),
            }
        }
        _ => None,
    }
}

/// Whether `p` and `q` are a date/time or duration pair `compare` handles.
pub fn involved(p: &Atomic, q: &Atomic) -> bool {
    matches!(p, Atomic::Time(_) | Atomic::Dur(_)) || matches!(q, Atomic::Time(_) | Atomic::Dur(_))
}

pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

fn dur(kind: DurKind, months: i64, seconds: Decimal) -> Atomic {
    Atomic::Dur(Rc::new(Duration { kind, months, seconds }))
}

/// Arithmetic with date/time and duration operands.
pub fn arith(op: ArithOp, x: &Atomic, y: &Atomic) -> Result<Atomic, String> {
    use Atomic::{Dur, Time};
    let bad = || {
        format!(
            "XPTY0004: arithmetic on {} and {} is not defined",
            x.type_name(),
            y.type_name()
        )
    };
    match (op, x, y) {
        (ArithOp::Sub, Time(a), Time(b)) if a.kind == b.kind && a.kind != DtKind::GYear => {
            if !matches!(a.kind, DtKind::DateTime | DtKind::Date | DtKind::Time) {
                return Err(bad());
            }
            let d = a.timeline(IMPLICIT_TZ).0 - b.timeline(IMPLICIT_TZ).0;
            Ok(dur(DurKind::DayTime, 0, Decimal(d)))
        }
        (ArithOp::Add, Time(t), Dur(d)) | (ArithOp::Add, Dur(d), Time(t)) => add_to(t, d, 1),
        (ArithOp::Sub, Time(t), Dur(d)) => add_to(t, d, -1),
        (op @ (ArithOp::Add | ArithOp::Sub), Dur(a), Dur(b)) if a.kind == b.kind => {
            let s = if matches!(op, ArithOp::Add) { 1 } else { -1 };
            match a.kind {
                DurKind::YearMonth => Ok(dur(a.kind, a.months + s * b.months, Decimal(0))),
                DurKind::DayTime => {
                    Ok(dur(a.kind, 0, Decimal(a.seconds.0 + s as i128 * b.seconds.0)))
                }
                DurKind::Duration => Err(bad()),
            }
        }
        (ArithOp::Mul, Dur(d), n) | (ArithOp::Mul, n, Dur(d)) if n.is_numeric() => {
            scale(d, n.to_f64(), crate::value::to_dec(n), false)
        }
        (ArithOp::Div, Dur(d), n) if n.is_numeric() => {
            scale(d, n.to_f64(), crate::value::to_dec(n), true)
        }
        (ArithOp::Div, Dur(a), Dur(b)) if a.kind == b.kind => match a.kind {
            DurKind::YearMonth if b.months != 0 => Ok(Atomic::Dec(
                Decimal::from_int(a.months)
                    .div(Decimal::from_int(b.months))
                    .ok_or_else(bad)?,
            )),
            DurKind::DayTime if b.seconds.0 != 0 => {
                Ok(Atomic::Dec(a.seconds.div(b.seconds).ok_or_else(bad)?))
            }
            DurKind::Duration => Err(bad()),
            _ => Err("FOAR0001: division by a zero duration".into()),
        },
        _ => Err(bad()),
    }
}

fn add_to(t: &DateTime, d: &Duration, sign: i64) -> Result<Atomic, String> {
    let ok = match (t.kind, d.kind) {
        (DtKind::DateTime | DtKind::Date, DurKind::YearMonth | DurKind::DayTime) => true,
        (DtKind::Time, DurKind::DayTime) => true,
        _ => false,
    };
    if !ok {
        return Err(format!(
            "XPTY0004: cannot add xs:{} to xs:{}",
            d.kind.name(),
            t.kind.name()
        ));
    }
    let mut r = t.add_months(sign * d.months);
    r = r.add_seconds(Decimal(sign as i128 * d.seconds.0));
    match t.kind {
        DtKind::Date => (r.hour, r.minute, r.second) = (0, 0, Decimal(0)),
        DtKind::Time => (r.year, r.month, r.day) = (1972, 12, 31),
        _ => {}
    }
    Ok(Atomic::Time(Rc::new(r)))
}

fn scale(d: &Duration, f: f64, dec: Option<Decimal>, divide: bool) -> Result<Atomic, String> {
    if f.is_nan() {
        return Err("FOCA0005: NaN in duration arithmetic".into());
    }
    if divide && f == 0.0 {
        return Err("FODT0002: duration divided by zero".into());
    }
    if f.is_infinite() && !divide {
        return Err("FODT0002: duration overflow".into());
    }
    match d.kind {
        DurKind::YearMonth => {
            let m = d.months as f64;
            let r = if divide { m / f } else { m * f };
            // Round half up (towards positive infinity).
            Ok(dur(d.kind, (r + 0.5).floor() as i64, Decimal(0)))
        }
        DurKind::DayTime => {
            let s = match dec {
                Some(k) if !divide => d.seconds.mul(k),
                Some(k) if k.0 != 0 => d.seconds.div(k),
                _ => None,
            };
            let s = match s {
                Some(s) => s,
                None => {
                    let v = d.seconds.to_f64();
                    Decimal::from_f64(if divide { v / f } else { v * f })
                        .ok_or("FODT0002: duration overflow")?
                }
            };
            Ok(dur(d.kind, 0, s))
        }
        DurKind::Duration => Err("XPTY0004: xs:duration cannot be multiplied".into()),
    }
}

/// Builds a dayTimeDuration from a timezone offset in minutes.
pub fn tz_duration(tz: i32) -> Atomic {
    dur(DurKind::DayTime, 0, dec_secs(tz as i128 * 60))
}

/// A dayTimeDuration as a timezone offset in minutes (`adjust-*`), checked
/// to lie in -PT14H..PT14H and be whole minutes.
pub fn duration_tz(d: &Duration) -> Result<i32, String> {
    let s = d.seconds.0;
    if d.months != 0 || s % (60 * DEC_SCALE) != 0 || s.abs() > 14 * 3600 * DEC_SCALE {
        return Err(format!("FODT0003: invalid timezone {}", d.to_canonical()));
    }
    Ok((s / (60 * DEC_SCALE)) as i32)
}

/// The value of a component accessor (`year-from-dateTime`, ...).
pub fn component(a: &Atomic, which: &str) -> Option<Atomic> {
    match a {
        Atomic::Time(t) => {
            let int = |v: i64| Some(Atomic::Int(v));
            match which {
                "year" if t.kind.has_date() => int(astro_to_lexical(t.year)),
                "month" if t.kind.has_date() => int(t.month as i64),
                "day" if t.kind.has_date() => int(t.day as i64),
                "hours" => int(t.hour as i64),
                "minutes" => int(t.minute as i64),
                "seconds" => Some(Atomic::Dec(t.second)),
                "timezone" => t.tz.map(tz_duration),
                _ => None,
            }
        }
        Atomic::Dur(d) => {
            let neg = d.months < 0 || d.seconds.0 < 0;
            let sign = if neg { -1 } else { 1 };
            let m = d.months.abs();
            let s = d.seconds.0.abs();
            let whole = (s / DEC_SCALE) as i64;
            Some(match which {
                "years" => Atomic::Int(sign * (m / 12)),
                "months" => Atomic::Int(sign * (m % 12)),
                "days" => Atomic::Int(sign * (whole / 86_400)),
                "hours" => Atomic::Int(sign * (whole % 86_400 / 3600)),
                "minutes" => Atomic::Int(sign * (whole % 3600 / 60)),
                "seconds" => Atomic::Dec(Decimal(sign as i128 * (s % (60 * DEC_SCALE)))),
                _ => return None,
            })
        }
        _ => None,
    }
}

/// `fn:dateTime($date, $time)`.
pub fn combine(d: &DateTime, t: &DateTime) -> Result<DateTime, String> {
    let tz = match (d.tz, t.tz) {
        (Some(a), Some(b)) if a != b => {
            return Err("FORG0008: the date and time have different timezones".into());
        }
        (a, b) => a.or(b),
    };
    Ok(DateTime {
        kind: DtKind::DateTime,
        hour: t.hour,
        minute: t.minute,
        second: t.second,
        tz,
        ..d.clone()
    })
}

/// The current dateTime, once per thread (stable within a transformation).
pub fn now() -> DateTime {
    thread_local! {
        static NOW: std::cell::OnceCell<DateTime> = const { std::cell::OnceCell::new() };
    }
    NOW.with(|n| {
        n.get_or_init(|| {
            let d = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            let ms = d.as_millis() as i128;
            let epoch = DateTime {
                kind: DtKind::DateTime,
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: Decimal(0),
                tz: Some(IMPLICIT_TZ),
            };
            epoch.add_seconds(Decimal(ms * (DEC_SCALE / 1000)))
        })
        .clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(k: DtKind, s: &str) -> DateTime {
        DateTime::parse(k, s).unwrap_or_else(|| panic!("{s}"))
    }

    #[test]
    fn round_trips() {
        for (k, s) in [
            (DtKind::DateTime, "2002-10-10T12:00:00-05:00"),
            (DtKind::DateTime, "1999-12-31T23:59:59.5Z"),
            (DtKind::Date, "-0044-03-15"),
            (DtKind::Time, "13:20:00"),
            (DtKind::GMonthDay, "--02-29"),
            (DtKind::GDay, "---31Z"),
        ] {
            assert_eq!(dt(k, s).to_canonical(), s);
        }
        assert_eq!(dt(DtKind::DateTime, "1999-12-31T24:00:00").to_canonical(), "2000-01-01T00:00:00");
        assert!(DateTime::parse(DtKind::Date, "2001-02-29").is_none());
        assert!(DateTime::parse(DtKind::Date, "0000-01-01").is_none());
    }

    #[test]
    fn durations() {
        let d = Duration::parse(DurKind::Duration, "P1Y2M3DT10H30M0.5S").unwrap();
        assert_eq!(d.to_canonical(), "P1Y2M3DT10H30M0.5S");
        assert_eq!(Duration::parse(DurKind::DayTime, "PT36H").unwrap().to_canonical(), "P1DT12H");
        assert_eq!(Duration::zero(DurKind::YearMonth).to_canonical(), "P0M");
        assert_eq!(Duration::zero(DurKind::DayTime).to_canonical(), "PT0S");
        assert!(Duration::parse(DurKind::Duration, "P").is_none());
        assert!(Duration::parse(DurKind::Duration, "P1Y2MT").is_none());
        assert!(Duration::parse(DurKind::YearMonth, "P1D").is_none());
    }

    #[test]
    fn arithmetic() {
        let t = Atomic::Time(Rc::new(dt(DtKind::Date, "2000-01-31")));
        let m = Atomic::Dur(Rc::new(Duration::parse(DurKind::YearMonth, "P1M").unwrap()));
        let r = arith(ArithOp::Add, &t, &m).unwrap();
        assert_eq!(r.to_string(), "2000-02-29");
        let a = Atomic::Time(Rc::new(dt(DtKind::DateTime, "2000-10-30T06:12:00")));
        let b = Atomic::Time(Rc::new(dt(DtKind::DateTime, "1999-11-28T09:00:00Z")));
        assert_eq!(arith(ArithOp::Sub, &a, &b).unwrap().to_string(), "P336DT21H12M");
    }
}
