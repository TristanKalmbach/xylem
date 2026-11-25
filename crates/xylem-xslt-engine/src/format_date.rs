//! `format-date()`, `format-dateTime()` and `format-time()` (XSLT 2.0
//! §16.5), in English with the Gregorian calendar.
//!
//! A picture is literal text with variable markers in square brackets
//! (`[Y0001]-[M01]-[D01]`, `[FNn], [D1o] [MNn]`; `[[` and `]]` stand for
//! brackets). A marker names a component and may give a presentation
//! (`1`, `01`, `i`, `I`, `a`, `A`, `w`, `W`, `Ww`, `n`, `N`, `Nn`), an
//! ordinal or traditional modifier (`o`, `t`), and widths (`,min-max`).

use crate::datetime::{DateTime, DtKind, days_in_month};

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

/// Formats `v` with `picture`. `lang` other than English (or empty) is
/// reported in the result, as the specification asks of an unsupported
/// language; so is a calendar other than the Gregorian one.
pub fn format(
    v: &DateTime,
    picture: &str,
    lang: Option<&str>,
    calendar: Option<&str>,
) -> Result<String, String> {
    let mut out = String::new();
    if let Some(l) = lang.map(str::trim).filter(|l| !l.is_empty())
        && !l.to_ascii_lowercase().starts_with("en")
    {
        out.push_str("[Language: en]");
    }
    if let Some(c) = calendar.map(str::trim).filter(|c| !c.is_empty())
        && !matches!(c, "AD" | "ISO")
    {
        out.push_str("[Calendar: AD]");
    }
    let chars: Vec<char> = picture.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '[' if chars.get(i + 1) == Some(&'[') => {
                out.push('[');
                i += 2;
            }
            ']' if chars.get(i + 1) == Some(&']') => {
                out.push(']');
                i += 2;
            }
            '[' => {
                let end = chars[i + 1..]
                    .iter()
                    .position(|&c| c == ']')
                    .map(|p| i + 1 + p)
                    .ok_or_else(|| {
                        format!("XTDE1340: picture '{picture}' has a '[' without a ']'")
                    })?;
                let marker: String = chars[i + 1..end]
                    .iter()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                out.push_str(&component(v, &marker, picture)?);
                i = end + 1;
            }
            ']' => {
                return Err(format!(
                    "XTDE1340: picture '{picture}' has a ']' without a '['"
                ));
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Monday = 1 ... Sunday = 7.
fn weekday(y: i64, m: u8, d: u8) -> u8 {
    // Days from 0001-01-01 (a Monday) in the proleptic Gregorian calendar.
    let days = day_number(y, m, d);
    (days.rem_euclid(7) + 1) as u8
}

fn day_number(y: i64, m: u8, d: u8) -> i64 {
    let y0 = y - 1;
    let mut n = y0 * 365 + y0.div_euclid(4) - y0.div_euclid(100) + y0.div_euclid(400);
    for mm in 1..m {
        n += i64::from(days_in_month(y, mm));
    }
    n + i64::from(d) - 1
}

fn day_of_year(y: i64, m: u8, d: u8) -> i64 {
    day_number(y, m, d) - day_number(y, 1, 1) + 1
}

/// Week of the year, weeks starting on Monday, week 1 the one that holds
/// the year's first Thursday; days before it belong to the previous
/// year's last week. A day at the end of December stays in its own year
/// (week 53), as the reference processor counts.
fn iso_week(y: i64, m: u8, d: u8) -> i64 {
    let doy = day_of_year(y, m, d);
    let wd = i64::from(weekday(y, m, d));
    let week = (doy - wd + 10).div_euclid(7);
    if week < 1 {
        return iso_week(y - 1, 12, 31);
    }
    week
}

/// Week of the month, counted as the ISO week rule counts weeks of a year.
fn week_in_month(y: i64, m: u8, d: u8) -> i64 {
    let first_wd = i64::from(weekday(y, m, 1));
    // Days of the first week that fall in this month.
    let offset = (first_wd - 1) % 7;
    let w = (i64::from(d) + offset - 1) / 7 + 1;
    // A first week with fewer than four days belongs to the previous month.
    if first_wd > 4 {
        if w == 1 {
            let (py, pm) = if m == 1 { (y - 1, 12) } else { (y, m - 1) };
            return week_in_month(py, pm, days_in_month(py, pm));
        }
        return w - 1;
    }
    w
}

struct Marker {
    comp: char,
    first: String,
    second: Option<char>,
    min: Option<usize>,
    max: Option<usize>,
}

fn parse_marker(m: &str, picture: &str) -> Result<Marker, String> {
    let mut chars = m.chars();
    let comp = chars
        .next()
        .ok_or_else(|| format!("XTDE1340: empty marker in picture '{picture}'"))?;
    let rest: String = chars.collect();
    let (pres, width) = match rest.rfind(',') {
        Some(p) => (&rest[..p], Some(&rest[p + 1..])),
        None => (rest.as_str(), None),
    };
    let (first, second) = match pres.chars().last() {
        Some(c @ ('o' | 't')) if pres.len() > 1 => (pres[..pres.len() - 1].to_string(), Some(c)),
        _ => (pres.to_string(), None),
    };
    let (mut min, mut max) = (None, None);
    if let Some(w) = width {
        let (a, b) = match w.split_once('-') {
            Some((a, b)) => (a, Some(b)),
            None => (w, None),
        };
        let num = |s: &str| -> Result<Option<usize>, String> {
            match s {
                "*" | "" => Ok(None),
                s => s
                    .parse::<usize>()
                    .ok()
                    .filter(|&n| n > 0)
                    .map(Some)
                    .ok_or_else(|| format!("XTDE1340: bad width '{w}' in picture '{picture}'")),
            }
        };
        min = num(a)?;
        max = match b {
            Some(b) => num(b)?,
            None => None,
        };
    }
    Ok(Marker {
        comp,
        first,
        second,
        min,
        max,
    })
}

fn component(v: &DateTime, m: &str, picture: &str) -> Result<String, String> {
    let mk = parse_marker(m, picture)?;
    let has_date = matches!(v.kind, DtKind::Date | DtKind::DateTime);
    let has_time = matches!(v.kind, DtKind::Time | DtKind::DateTime);
    let need = |ok: bool| -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err(format!(
                "XTDE1350: component [{}] is not available in an xs:{}",
                mk.comp,
                v.kind.name()
            ))
        }
    };
    let number = |n: i64, default: &str| fmt_number(n, &mk, default);
    match mk.comp {
        'Y' => {
            need(has_date)?;
            // A maximum width (`[Y,2-2]`), or a two-digit picture (`[Y01]`),
            // keeps the year's last digits; the picture's digits then set
            // the minimum width (`[Y0001,2-2]` gives `0002`).
            let digits = mk.first.chars().filter(|c| c.is_ascii_digit()).count();
            let decimal = mk.first.is_empty() || mk.first.chars().all(|c| c.is_ascii_digit());
            let keep = mk.max.or((decimal && digits == 2).then_some(2));
            let mut y = v.year.abs();
            if decimal && let Some(k) = keep {
                y %= 10i64.pow(k.min(18) as u32);
            }
            let shown = Marker {
                min: if digits > 1 { None } else { mk.min },
                max: None,
                ..copy(&mk)
            };
            fmt_number(y, &shown, "1")
        }
        'M' => {
            need(has_date)?;
            if is_name(&mk.first) {
                Ok(name(MONTHS[usize::from(v.month) - 1], &mk))
            } else {
                number(i64::from(v.month), "1")
            }
        }
        'D' => {
            need(has_date)?;
            number(i64::from(v.day), "1")
        }
        'd' => {
            need(has_date)?;
            number(day_of_year(v.year, v.month, v.day), "1")
        }
        'F' => {
            need(has_date)?;
            let wd = weekday(v.year, v.month, v.day);
            if mk.first.is_empty() || is_name(&mk.first) {
                let m2 = Marker {
                    first: if mk.first.is_empty() {
                        "Nn".into()
                    } else {
                        mk.first.clone()
                    },
                    ..copy(&mk)
                };
                Ok(name(DAYS[usize::from(wd) - 1], &m2))
            } else {
                number(i64::from(wd), "1")
            }
        }
        'W' => {
            need(has_date)?;
            number(iso_week(v.year, v.month, v.day), "1")
        }
        'w' => {
            need(has_date)?;
            number(week_in_month(v.year, v.month, v.day), "1")
        }
        'H' => {
            need(has_time)?;
            number(i64::from(v.hour), "1")
        }
        'h' => {
            need(has_time)?;
            let h = match v.hour % 12 {
                0 => 12,
                h => h,
            };
            number(i64::from(h), "1")
        }
        'P' => {
            need(has_time)?;
            // `a.m.` / `p.m.`, or `am` / `pm` when the width allows less
            // than four characters.
            let short = mk.max.is_some_and(|m| m < 4);
            let s = match (v.hour < 12, short) {
                (true, false) => "a.m.",
                (false, false) => "p.m.",
                (true, true) => "am",
                (false, true) => "pm",
            };
            let m2 = Marker {
                first: if mk.first.is_empty() {
                    "n".into()
                } else {
                    mk.first.clone()
                },
                ..copy(&mk)
            };
            Ok(name(s, &m2))
        }
        'm' => {
            need(has_time)?;
            number(i64::from(v.minute), "01")
        }
        's' => {
            need(has_time)?;
            number((v.second.0 / crate::value::DEC_SCALE) as i64, "01")
        }
        'f' => {
            need(has_time)?;
            Ok(fraction(v, &mk))
        }
        'Z' | 'z' => Ok(timezone(v.tz, &mk)),
        'C' => Ok(if mk.first.starts_with('N') {
            "GREGORIAN".into()
        } else {
            "Gregorian".into()
        }),
        'E' => Ok(if v.year > 0 { "AD" } else { "BC" }.into()),
        c => Err(format!(
            "XTDE1340: unknown component [{c}] in picture '{picture}'"
        )),
    }
}

fn copy(m: &Marker) -> Marker {
    Marker {
        comp: m.comp,
        first: m.first.clone(),
        second: m.second,
        min: m.min,
        max: m.max,
    }
}

fn is_name(p: &str) -> bool {
    matches!(p, "n" | "N" | "Nn")
}

/// A name in the marker's case, cut to its maximum width.
fn name(full: &str, mk: &Marker) -> String {
    let mut s = match mk.first.as_str() {
        "N" => full.to_uppercase(),
        "n" => full.to_lowercase(),
        _ => full.to_string(),
    };
    if let Some(mx) = mk.max
        && s.chars().count() > mx
    {
        s = s.chars().take(mx).collect();
    }
    if let Some(mn) = mk.min {
        while s.chars().count() < mn {
            s.push(' ');
        }
    }
    s
}

fn fmt_number(n: i64, mk: &Marker, default: &str) -> Result<String, String> {
    let pres = if mk.first.is_empty() {
        default
    } else {
        mk.first.as_str()
    };
    let ordinal = mk.second == Some('o');
    let mut s = match pres {
        "i" => crate::number::roman(n).to_lowercase(),
        "I" => crate::number::roman(n),
        "a" => crate::number::alpha(n, b'a'),
        "A" => crate::number::alpha(n, b'A'),
        "w" => words(n, ordinal),
        "W" => words(n, ordinal).to_uppercase(),
        "Ww" => title_case(&words(n, ordinal)),
        "n" | "N" | "Nn" => n.to_string(),
        p => {
            // A decimal-digit picture: its digits give the minimum width;
            // `#` marks optional digits.
            let digits = p.chars().filter(|c| c.is_ascii_digit()).count().max(1);
            let width = mk
                .min
                .unwrap_or(digits)
                .max(if mk.min.is_some() { 1 } else { digits });
            let mut d = format!("{:0width$}", n.abs(), width = width);
            if n < 0 {
                d.insert(0, '-');
            }
            if ordinal {
                d.push_str(ordinal_suffix(n));
            }
            return Ok(d);
        }
    };
    if let Some(mn) = mk.min {
        while s.chars().count() < mn {
            s.push(' ');
        }
    }
    if ordinal && matches!(pres, "i" | "I" | "a" | "A") {
        s.push_str(ordinal_suffix(n));
    }
    Ok(s)
}

fn ordinal_suffix(n: i64) -> &'static str {
    let n = n.abs();
    match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    }
}

fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A number in English words (`twenty one`), or its ordinal (`twenty
/// first`).
fn words(n: i64, ordinal: bool) -> String {
    const ONES: [&str; 20] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
    ];
    const TENS: [&str; 10] = [
        "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
    ];
    fn cardinal(n: i64) -> String {
        if n < 0 {
            return format!("minus {}", cardinal(-n));
        }
        if n < 20 {
            return ONES[n as usize].into();
        }
        if n < 100 {
            let t = TENS[(n / 10) as usize];
            return if n % 10 == 0 {
                t.into()
            } else {
                format!("{t} {}", ONES[(n % 10) as usize])
            };
        }
        for (unit, name) in [
            (1_000_000_000_000, "trillion"),
            (1_000_000_000, "billion"),
            (1_000_000, "million"),
            (1_000, "thousand"),
            (100, "hundred"),
        ] {
            if n >= unit {
                let head = format!("{} {name}", cardinal(n / unit));
                let rest = n % unit;
                return if rest == 0 {
                    head
                } else if unit == 100 || rest < 100 {
                    format!("{head} and {}", cardinal(rest))
                } else {
                    format!("{head} {}", cardinal(rest))
                };
            }
        }
        n.to_string()
    }
    let c = cardinal(n);
    if !ordinal {
        return c;
    }
    // Ordinals join tens and units with a hyphen (`thirty-first`);
    // cardinals do not (`thirty one`), as the reference processor writes.
    let (head, last) = match c.rfind(' ') {
        Some(p) => {
            let before = &c[..p];
            let tens = before.rsplit(' ').next().unwrap_or("");
            let sep = if TENS.contains(&tens) && !tens.is_empty() {
                "-"
            } else {
                " "
            };
            (format!("{before}{sep}"), c[p + 1..].to_string())
        }
        None => (String::new(), c.clone()),
    };
    let last = match last.as_str() {
        "one" => "first".into(),
        "two" => "second".into(),
        "three" => "third".into(),
        "five" => "fifth".into(),
        "eight" => "eighth".into(),
        "nine" => "ninth".into(),
        "twelve" => "twelfth".into(),
        l if l.ends_with('y') => format!("{}ieth", &l[..l.len() - 1]),
        l => format!("{l}th"),
    };
    head + &last
}

/// Fractional seconds: a one-digit picture (`[f]`, `[f1]`) gives every
/// significant digit (at least one); a longer one exactly its number of
/// digits, truncated or padded with zeros.
fn fraction(v: &DateTime, mk: &Marker) -> String {
    let s = v.second.to_canonical();
    let frac = s.split_once('.').map(|(_, f)| f).unwrap_or("");
    let digits = mk.first.chars().filter(|c| c.is_ascii_digit()).count();
    let (min, max) = if digits > 1 {
        (digits, digits)
    } else {
        (mk.min.unwrap_or(1), mk.max.unwrap_or(usize::MAX))
    };
    let mut d: String = frac.chars().take(max).collect();
    while d.len() < min {
        d.push('0');
    }
    d
}

/// `[Z]` as `+01:00` (`Z` for `[Z0]` at UTC is not used); `[z]` adds `GMT`.
fn timezone(tz: Option<i32>, mk: &Marker) -> String {
    let Some(tz) = tz else {
        return String::new();
    };
    let sign = if tz < 0 { '-' } else { '+' };
    let (h, m) = (tz.abs() / 60, tz.abs() % 60);
    let body = match mk.first.as_str() {
        "" | "01:01" => format!("{sign}{h:02}:{m:02}"),
        "0" | "1" => {
            if m == 0 {
                format!("{sign}{h}")
            } else {
                format!("{sign}{h}:{m:02}")
            }
        }
        "01" if m == 0 => format!("{sign}{h:02}"),
        "0101" => format!("{sign}{h:02}{m:02}"),
        "Z" => {
            if tz == 0 {
                "Z".into()
            } else {
                format!("{sign}{h:02}:{m:02}")
            }
        }
        _ => format!("{sign}{h:02}:{m:02}"),
    };
    if mk.comp == 'z' {
        format!("GMT{body}")
    } else {
        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> DateTime {
        DateTime::parse(DtKind::DateTime, s).unwrap()
    }

    #[test]
    fn pictures() {
        let d = dt("2002-12-31T15:58:45.762+02:00");
        let f = |p: &str| format(&d, p, None, None).unwrap();
        assert_eq!(f("[Y0001]-[M01]-[D01]"), "2002-12-31");
        assert_eq!(f("[M]/[D]/[Y]"), "12/31/2002");
        assert_eq!(f("[D1o] [MNn], [Y]"), "31st December, 2002");
        assert_eq!(f("[FNn], [D] [MNn,*-3] [Y01]"), "Tuesday, 31 Dec 02");
        assert_eq!(f("[H01]:[m01]:[s01].[f001] [Z]"), "15:58:45.762 +02:00");
        assert_eq!(f("[h].[m01][Pn]"), "3.58p.m.");
        assert_eq!(f("[Dwo] of [MNn]"), "thirty-first of December");
        assert_eq!(f("[YI]"), "MMII");
        assert_eq!(f("[d] [W] [[x]]"), "365 53 [x]");
        assert!(
            format(
                &DateTime::parse(DtKind::Date, "2002-12-31").unwrap(),
                "[H]",
                None,
                None
            )
            .is_err()
        );
        assert_eq!(f("[z]"), "GMT+02:00");
    }

    #[test]
    fn weeks() {
        assert_eq!(iso_week(2005, 1, 1), 53);
        assert_eq!(iso_week(2008, 12, 29), 53);
        assert_eq!(iso_week(2026, 11, 25), 48);
        assert_eq!(weekday(2026, 11, 25), 3);
    }
}
