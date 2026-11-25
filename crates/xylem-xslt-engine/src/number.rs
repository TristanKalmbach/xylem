//! Number formatting: `xsl:number` format tokens and `format-number()`.

/// Formats a list of numbers with an `xsl:number` format string
/// (XSLT 2.0 §12.3): alphanumeric tokens separated by punctuation, with
/// leading and trailing punctuation as prefix and suffix.
pub fn format_numbers(nums: &[i64], format: &str) -> String {
    let (prefix, tokens, seps, suffix) = parse_format(format);
    let mut out = prefix;
    for (k, &n) in nums.iter().enumerate() {
        if k > 0 {
            let sep = if k - 1 < seps.len() {
                &seps[k - 1]
            } else {
                seps.last().map(String::as_str).unwrap_or(".")
            };
            out.push_str(sep);
        }
        let tok = tokens
            .get(k)
            .or(tokens.last())
            .map(String::as_str)
            .unwrap_or("1");
        out.push_str(&format_one(n, tok));
    }
    out.push_str(&suffix);
    out
}

fn parse_format(f: &str) -> (String, Vec<String>, Vec<String>, String) {
    let mut parts: Vec<(bool, String)> = Vec::new();
    for c in f.chars() {
        let alnum = c.is_alphanumeric();
        match parts.last_mut() {
            Some((a, s)) if *a == alnum => s.push(c),
            _ => parts.push((alnum, c.to_string())),
        }
    }
    let mut prefix = String::new();
    let mut suffix = String::new();
    if parts.first().is_some_and(|(a, _)| !a) {
        prefix = parts.remove(0).1;
    }
    if parts.last().is_some_and(|(a, _)| !a) && !parts.is_empty() {
        suffix = parts.pop().unwrap().1;
    }
    let tokens: Vec<String> = parts
        .iter()
        .filter(|(a, _)| *a)
        .map(|(_, s)| s.clone())
        .collect();
    let seps: Vec<String> = parts
        .iter()
        .filter(|(a, _)| !*a)
        .map(|(_, s)| s.clone())
        .collect();
    if tokens.is_empty() {
        return (prefix, vec!["1".into()], vec![], suffix);
    }
    (prefix, tokens, seps, suffix)
}

fn format_one(n: i64, tok: &str) -> String {
    match tok {
        "A" => alpha(n, b'A'),
        "a" => alpha(n, b'a'),
        "I" => roman(n),
        "i" => roman(n).to_lowercase(),
        t if t.chars().all(|c| c.is_ascii_digit()) => {
            let width = t.len();
            format!("{n:0width$}")
        }
        _ => n.to_string(),
    }
}

pub(crate) fn alpha(n: i64, base: u8) -> String {
    if n <= 0 {
        return n.to_string();
    }
    let mut n = n;
    let mut s = Vec::new();
    while n > 0 {
        n -= 1;
        s.push(base + (n % 26) as u8);
        n /= 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

pub(crate) fn roman(n: i64) -> String {
    if !(1..4000).contains(&n) {
        return n.to_string();
    }
    let table = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut n = n;
    let mut s = String::new();
    for (v, r) in table {
        while n >= v {
            s.push_str(r);
            n -= v;
        }
    }
    s
}

/// `format-number()` with the default decimal format: `#`, `0`, `.`, `,`
/// and a literal prefix/suffix. Percent, per-mille and exponent pictures
/// are not handled yet.
pub fn format_number(x: f64, picture: &str) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    let picture = picture.split(';').next().unwrap_or("");
    let is_pic = |c: char| matches!(c, '#' | '0' | '.' | ',');
    let start = picture.find(is_pic).unwrap_or(picture.len());
    let end = picture.rfind(is_pic).map(|i| i + 1).unwrap_or(start);
    let (prefix, body, suffix) = (&picture[..start], &picture[start..end], &picture[end..]);
    let (int_pic, frac_pic) = body.split_once('.').unwrap_or((body, ""));
    let min_int = int_pic.chars().filter(|&c| c == '0').count();
    let min_frac = frac_pic.chars().filter(|&c| c == '0').count();
    let max_frac = frac_pic.chars().filter(|&c| c == '0' || c == '#').count();
    let group = int_pic.rfind(',').map(|i| int_pic[i + 1..].len());
    let neg = x < 0.0;
    let a = x.abs();
    let s = format!("{a:.max_frac$}");
    let (mut ip, fp) = match s.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (s.clone(), String::new()),
    };
    let mut fp = fp.trim_end_matches('0').to_string();
    while fp.len() < min_frac {
        fp.push('0');
    }
    if ip == "0" && min_int == 0 {
        ip.clear();
    }
    while ip.len() < min_int {
        ip.insert(0, '0');
    }
    if let Some(g) = group.filter(|&g| g > 0) {
        let digits: Vec<char> = ip.chars().collect();
        let mut out = String::new();
        for (k, c) in digits.iter().enumerate() {
            if k > 0 && (digits.len() - k).is_multiple_of(g) {
                out.push(',');
            }
            out.push(*c);
        }
        ip = out;
    }
    let mut out = String::new();
    if neg && (ip.chars().any(|c| c != '0' && c != ',') || fp.chars().any(|c| c != '0')) {
        out.push('-');
    }
    out.push_str(prefix);
    out.push_str(&ip);
    if !fp.is_empty() {
        out.push('.');
        out.push_str(&fp);
    }
    out.push_str(suffix);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_numbers(&[3], "1."), "3.");
        assert_eq!(format_numbers(&[3], "(a)"), "(c)");
        assert_eq!(format_numbers(&[28], "A"), "AB");
        assert_eq!(format_numbers(&[14], "i)"), "xiv)");
        assert_eq!(format_numbers(&[2, 3], "1.1"), "2.3");
        assert_eq!(format_numbers(&[7], "01"), "07");
        assert_eq!(format_number(1234.5, "#,##0.00"), "1,234.50");
        assert_eq!(format_number(0.25, "0.0#"), "0.25");
    }
}
