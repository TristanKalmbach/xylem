//! The text order the reference processors sort by when they compare like
//! a culture rather than by code point: .NET's XSLT 1.0 processor always
//! (for `data-type="text"`), and Saxon when `xsl:sort` gives `case-order`.
//!
//! Observed on the reference host (spec note 24), as a multi-level
//! comparison: first the letters and digits ignoring accents and case
//! (spaces and symbols before digits, digits before letters, hyphens and
//! apostrophes ignored, ligatures and `ß` as their expansions), then the
//! accents (unaccented first), then the case (lower-case first unless
//! `upper-first`), then the ignored characters and expansions (a string
//! without them first).


/// A string's sort key; compare keys with `Ord`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    primary: Vec<u32>,
    secondary: Vec<u8>,
    tertiary: Vec<u8>,
    /// Expanded characters (`ß`, ligatures) sort before written-out ones.
    expansions: Vec<u8>,
    /// Ignored characters' positions, later ones first, none first of all.
    ignored: Vec<std::cmp::Reverse<usize>>,
}

/// Base letter and accent weight of a Latin letter with a diacritic.
fn decompose(c: char) -> Option<(char, u8)> {
    const TABLE: &[(&str, char)] = &[
        ("àáâãäåāăą", 'a'),
        ("çćĉċč", 'c'),
        ("ďđ", 'd'),
        ("èéêëēĕėęě", 'e'),
        ("ĝğġģ", 'g'),
        ("ĥħ", 'h'),
        ("ìíîïĩīĭįı", 'i'),
        ("ĵ", 'j'),
        ("ķ", 'k'),
        ("ĺļľŀł", 'l'),
        ("ñńņňŉ", 'n'),
        ("òóôõöøōŏő", 'o'),
        ("ŕŗř", 'r'),
        ("śŝşš", 's'),
        ("ţťŧ", 't'),
        ("ùúûüũūŭůűų", 'u'),
        ("ŵ", 'w'),
        ("ýÿŷ", 'y'),
        ("źżž", 'z'),
    ];
    let lower: char = c.to_lowercase().next().unwrap_or(c);
    for (accented, base) in TABLE {
        if let Some(i) = accented.chars().position(|a| a == lower) {
            return Some((*base, i as u8 + 1));
        }
    }
    None
}

/// Characters that sort as two or more letters.
fn expansion(c: char) -> Option<&'static str> {
    Some(match c {
        'ß' => "ss",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'ﬀ' => "ff",
        'ﬁ' => "fi",
        'ﬂ' => "fl",
        'ﬃ' => "ffi",
        'ﬄ' => "ffl",
        _ => return None,
    })
}

fn primary(c: char) -> u32 {
    if c.is_whitespace() {
        100
    } else if c.is_ascii_digit() {
        10_000 + (c as u32 - '0' as u32)
    } else if c.is_ascii_alphabetic() {
        20_000 + (c.to_ascii_lowercase() as u32 - 'a' as u32)
    } else if c.is_alphabetic() || c.is_numeric() {
        // Other scripts: after Latin, by code point of the lower case.
        30_000 + c.to_lowercase().next().unwrap_or(c) as u32
    } else {
        // Symbols and punctuation: before digits, by code point.
        200 + c as u32
    }
}

/// The sort key of `s`; `upper_first` puts upper case before lower case.
pub fn key(s: &str, upper_first: bool) -> Key {
    let mut k = Key {
        primary: Vec::with_capacity(s.len()),
        secondary: Vec::with_capacity(s.len()),
        tertiary: Vec::with_capacity(s.len()),
        expansions: Vec::new(),
        ignored: Vec::new(),
    };
    let case = |c: char| -> u8 {
        let upper = c.is_uppercase();
        u8::from(upper != upper_first)
    };
    for (pos, c) in s.chars().enumerate() {
        if c == '-' || c == '\'' || c == '\u{2019}' {
            k.ignored.push(std::cmp::Reverse(pos));
            continue;
        }
        if let Some(exp) = expansion(c) {
            for e in exp.chars() {
                k.primary.push(primary(e));
                k.secondary.push(0);
                k.tertiary.push(case(c));
                k.expansions.push(0);
            }
            continue;
        }
        let (base, accent) = decompose(c).unwrap_or((c, 0));
        k.primary.push(primary(base));
        k.secondary.push(accent);
        k.tertiary.push(case(c));
        k.expansions.push(1);
    }
    // No ignored characters at all sorts before any.
    if k.ignored.is_empty() {
        k.ignored.push(std::cmp::Reverse(usize::MAX));
    }
    k
}

/// Compares two strings by [`key`].
#[cfg(test)]
fn compare(a: &str, b: &str, upper_first: bool) -> std::cmp::Ordering {
    key(a, upper_first).cmp(&key(b, upper_first))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_order() {
        let mut words = vec![
            "Vac", "VHMC", "apple", "Apple", "APPLE", "b", "B", "co-op", "coop", "café", "cafe",
            "a1", "a10", "a2", "_x", "Zebra", "zebra", "élan", "elan", "Élan", "x y", "xy", "Ab",
            "aB", "ab", "AB", "1", "10", "9", "-a", "a-", "résumé", "resume", "Resume", "ﬁ", "fi",
            "ß", "ss", "straße", "strasse",
        ];
        words.sort_by(|a, b| compare(a, b, false));
        assert_eq!(
            words.join("|"),
            "_x|1|10|9|a-|-a|a1|a10|a2|ab|aB|Ab|AB|apple|Apple|APPLE|b|B|cafe|café|coop|co-op|elan|élan|Élan|ﬁ|fi|resume|Resume|résumé|ß|ss|straße|strasse|Vac|VHMC|x y|xy|zebra|Zebra"
        );
        words.sort_by(|a, b| compare(a, b, true));
        assert_eq!(
            words.join("|"),
            "_x|1|10|9|a-|-a|a1|a10|a2|AB|Ab|aB|ab|APPLE|Apple|apple|B|b|cafe|café|coop|co-op|elan|Élan|élan|ﬁ|fi|Resume|resume|résumé|ß|ss|straße|strasse|Vac|VHMC|x y|xy|Zebra|zebra"
        );
    }
}
