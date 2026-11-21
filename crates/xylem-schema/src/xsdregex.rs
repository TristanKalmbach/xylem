//! Translation of XML Schema regular expressions (XSD 1.0, Appendix F) into
//! the syntax of the `regex` crate.
//!
//! Differences handled here:
//! * XSD patterns are implicitly anchored at both ends.
//! * `^` and `$` are ordinary characters outside character classes.
//! * `.` excludes both `\n` and `\r`; `\s` is exactly `[ \t\n\r]`; `\w` is
//!   "not punctuation, separator or other".
//! * `\i`, `\I`, `\c`, `\C` (XML name characters) have no counterpart.
//! * `\p{IsBlock}` names Unicode blocks, which `regex` does not know.
//! * Character class subtraction `[a-z-[aeiou]]`.

#[cfg(test)]
use regex::Regex;

const NAME_START: &str = r"\p{L}\p{Nl}_:";
const NAME_CHAR: &str = r"\p{L}\p{Nl}\p{Nd}\p{Mn}\p{Mc}\p{Lm}._:\-\x{B7}";
const WORD_NOT: &str = r"\p{P}\p{Z}\p{C}";

/// Compiles an XSD pattern into an anchored [`Regex`].
#[cfg(test)]
pub(crate) fn compile(pattern: &str) -> Result<Regex, String> {
    let body = translate(pattern)?;
    Regex::new(&format!(r"\A(?:{body})\z")).map_err(|e| e.to_string())
}

/// Translates the body of an XSD pattern (without anchors).
pub(crate) fn translate(pattern: &str) -> Result<String, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 8);
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' => {
                let (text, next) = escape(&chars, i, false)?;
                out.push_str(&text);
                i = next;
            }
            '[' => {
                let (text, next) = class(&chars, i)?;
                out.push_str(&text);
                i = next;
            }
            '.' => {
                out.push_str(r"[^\n\r]");
                i += 1;
            }
            '^' | '$' => {
                out.push('\\');
                out.push(c);
                i += 1;
            }
            '(' if chars.get(i + 1) == Some(&'?') => {
                return Err("`(?` is not XSD regex syntax".into());
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// Translates the escape starting at `chars[i] == '\\'`. Inside a class the
/// result is a class fragment rather than a stand-alone atom.
fn escape(chars: &[char], i: usize, in_class: bool) -> Result<(String, usize), String> {
    let c = *chars.get(i + 1).ok_or("trailing backslash")?;
    let wrap = |inner: &str, negate: bool| -> String {
        match (in_class, negate) {
            (true, false) => inner.to_string(),
            (_, true) => format!("[^{inner}]"),
            (false, false) => format!("[{inner}]"),
        }
    };
    let text = match c {
        'i' => wrap(NAME_START, false),
        'I' => wrap(NAME_START, true),
        'c' => wrap(NAME_CHAR, false),
        'C' => wrap(NAME_CHAR, true),
        's' => wrap(r" \t\n\r", false),
        'S' => wrap(r" \t\n\r", true),
        'w' => wrap(WORD_NOT, true),
        'W' => wrap(WORD_NOT, false),
        'd' => r"\d".to_string(),
        'D' => r"\D".to_string(),
        'n' | 'r' | 't' => format!("\\{c}"),
        'p' | 'P' => {
            let open = i + 2;
            if chars.get(open) != Some(&'{') {
                return Err("expected `{` after \\p".into());
            }
            let close = chars[open..]
                .iter()
                .position(|&c| c == '}')
                .ok_or("unterminated \\p{")?
                + open;
            let name: String = chars[open + 1..close].iter().collect();
            let negate = c == 'P';
            let text = property(&name, negate, in_class)?;
            return Ok((text, close + 1));
        }
        c if c.is_ascii_punctuation() => format!("\\{c}"),
        c => return Err(format!("unknown escape `\\{c}`")),
    };
    Ok((text, i + 2))
}

fn property(name: &str, negate: bool, in_class: bool) -> Result<String, String> {
    if let Some(block) = name.strip_prefix("Is") {
        let (lo, hi) =
            block_range(block).ok_or_else(|| format!("unknown Unicode block `{block}`"))?;
        let range = format!(r"\x{{{lo:X}}}-\x{{{hi:X}}}");
        return Ok(match (negate, in_class) {
            (false, true) => range,
            (false, false) => format!("[{range}]"),
            (true, _) => format!("[^{range}]"),
        });
    }
    let p = if negate { 'P' } else { 'p' };
    Ok(format!("\\{p}{{{name}}}"))
}

/// Translates a character class starting at `chars[i] == '['`.
fn class(chars: &[char], start: usize) -> Result<(String, usize), String> {
    let mut i = start + 1;
    let negate = chars.get(i) == Some(&'^');
    if negate {
        i += 1;
    }
    let mut items = String::new();
    let mut first = true;
    loop {
        let c = *chars.get(i).ok_or("unterminated character class")?;
        match c {
            ']' if !first => {
                i += 1;
                break;
            }
            '-' if chars.get(i + 1) == Some(&'[') => {
                // Subtraction: must be the last thing in the class.
                let (sub, next) = class(chars, i + 1)?;
                if chars.get(next) != Some(&']') {
                    return Err("class subtraction must end the class".into());
                }
                let base = if negate {
                    format!("[^{items}]")
                } else {
                    format!("[{items}]")
                };
                return Ok((format!("[{base}--{sub}]"), next + 1));
            }
            '\\' => {
                let (text, next) = escape(chars, i, true)?;
                items.push_str(&text);
                i = next;
            }
            _ => {
                // A literal, possibly the start of a range.
                let lo = literal_in_class(c);
                if chars.get(i + 1) == Some(&'-')
                    && chars.get(i + 2).is_some_and(|&n| n != ']' && n != '[')
                {
                    let hi_c = chars[i + 2];
                    let (hi, next) = if hi_c == '\\' {
                        let e = *chars.get(i + 3).ok_or("trailing backslash")?;
                        let lit = match e {
                            'n' => "\\n".to_string(),
                            'r' => "\\r".to_string(),
                            't' => "\\t".to_string(),
                            e if e.is_ascii_punctuation() => format!("\\{e}"),
                            e => return Err(format!("invalid range end `\\{e}`")),
                        };
                        (lit, i + 4)
                    } else {
                        (literal_in_class(hi_c), i + 3)
                    };
                    items.push_str(&lo);
                    items.push('-');
                    items.push_str(&hi);
                    i = next;
                } else {
                    items.push_str(&lo);
                    i += 1;
                }
            }
        }
        first = false;
    }
    let text = if negate {
        format!("[^{items}]")
    } else {
        format!("[{items}]")
    };
    Ok((text, i))
}

fn literal_in_class(c: char) -> String {
    match c {
        '[' | ']' | '\\' | '-' | '^' | '&' | '~' => format!("\\{c}"),
        c => c.to_string(),
    }
}

/// Code point ranges of the Unicode blocks most often used in schemas.
fn block_range(name: &str) -> Option<(u32, u32)> {
    let key: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    Some(match key.as_str() {
        "basiclatin" => (0x0000, 0x007F),
        "latin1supplement" => (0x0080, 0x00FF),
        "latinextendeda" => (0x0100, 0x017F),
        "latinextendedb" => (0x0180, 0x024F),
        "ipaextensions" => (0x0250, 0x02AF),
        "spacingmodifierletters" => (0x02B0, 0x02FF),
        "combiningdiacriticalmarks" => (0x0300, 0x036F),
        "greek" | "greekandcoptic" => (0x0370, 0x03FF),
        "cyrillic" => (0x0400, 0x04FF),
        "armenian" => (0x0530, 0x058F),
        "hebrew" => (0x0590, 0x05FF),
        "arabic" => (0x0600, 0x06FF),
        "devanagari" => (0x0900, 0x097F),
        "thai" => (0x0E00, 0x0E7F),
        "latinextendedadditional" => (0x1E00, 0x1EFF),
        "greekextended" => (0x1F00, 0x1FFF),
        "generalpunctuation" => (0x2000, 0x206F),
        "superscriptsandsubscripts" => (0x2070, 0x209F),
        "currencysymbols" => (0x20A0, 0x20CF),
        "letterlikesymbols" => (0x2100, 0x214F),
        "numberforms" => (0x2150, 0x218F),
        "arrows" => (0x2190, 0x21FF),
        "mathematicaloperators" => (0x2200, 0x22FF),
        "miscellaneoustechnical" => (0x2300, 0x23FF),
        "enclosedalphanumerics" => (0x2460, 0x24FF),
        "boxdrawing" => (0x2500, 0x257F),
        "blockelements" => (0x2580, 0x259F),
        "geometricshapes" => (0x25A0, 0x25FF),
        "miscellaneoussymbols" => (0x2600, 0x26FF),
        "dingbats" => (0x2700, 0x27BF),
        "cjksymbolsandpunctuation" => (0x3000, 0x303F),
        "hiragana" => (0x3040, 0x309F),
        "katakana" => (0x30A0, 0x30FF),
        "cjkunifiedideographs" => (0x4E00, 0x9FFF),
        "hangulsyllables" => (0xAC00, 0xD7AF),
        "privateuse" | "privateusearea" => (0xE000, 0xF8FF),
        "alphabeticpresentationforms" => (0xFB00, 0xFB4F),
        "halfwidthandfullwidthforms" => (0xFF00, 0xFFEF),
        "specials" => (0xFFF0, 0xFFFF),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, s: &str) -> bool {
        compile(p)
            .unwrap_or_else(|e| panic!("{p}: {e}"))
            .is_match(s)
    }

    #[test]
    fn anchored_and_literal_metachars() {
        assert!(m(r"\d{2}", "12"));
        assert!(!m(r"\d{2}", "123"));
        assert!(m(r"a$b^", "a$b^"));
        assert!(m(r"rgb\(\d{1,3},\d{1,3},\d{1,3}\)", "rgb(1,22,255)"));
        assert!(m(r"\d+(\.\d+)?(|px|pt)", "12"));
        assert!(m(r"\d+(\.\d+)?(|px|pt)", "1.5pt"));
        assert!(!m(".", "\r"));
    }

    #[test]
    fn classes_and_escapes() {
        assert!(m(r"[A-Za-z]{2}", "eN"));
        assert!(m(r"[a-z-[aeiou]]+", "xyz"));
        assert!(!m(r"[a-z-[aeiou]]+", "xaz"));
        assert!(m(r"[^a-c-[x]]", "d"));
        assert!(!m(r"[^a-c-[x]]", "x"));
        assert!(m(r"\i\c*", "my-name.1"));
        assert!(!m(r"\i\c*", "1abc"));
        assert!(m(r"[\i-[:]][\c-[:]]*", "ab"));
        assert!(m(r"\s\S", " x"));
        assert!(m(r"\w+", "abc123"));
        assert!(!m(r"\w", "!"));
        assert!(m(r"[-a]", "-"));
        assert!(m(r"[a\-z]", "-"));
        assert!(m(r"[&~]", "~"));
        assert!(m(r"\p{Lu}\P{Lu}", "Ab"));
        assert!(m(r"\p{IsBasicLatin}+", "abc"));
        assert!(!m(r"\p{IsBasicLatin}", "é"));
        assert!(m(r"[\p{IsLatin-1Supplement}]", "é"));
    }

    #[test]
    fn rejects_bad_patterns() {
        assert!(compile("[abc").is_err());
        assert!(compile(r"\p{IsNoSuchBlock}").is_err());
        assert!(compile("(?i)x").is_err());
    }
}
