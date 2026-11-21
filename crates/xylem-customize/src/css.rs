//! Finding file references (`@import`, `url()`) in CSS text.

use std::ops::Range;

/// A reference to another file, with the byte range of the target text.
#[derive(Debug, PartialEq, Eq)]
pub struct CssRef {
    pub range: Range<usize>,
    pub target: String,
}

impl CssRef {
    /// True when the target names a stylesheet.
    pub fn is_stylesheet(&self) -> bool {
        strip_suffix_ci(self.target.split(['?', '#']).next().unwrap_or(""), ".css")
    }
}

fn strip_suffix_ci(s: &str, suffix: &str) -> bool {
    s.len() >= suffix.len()
        && s.is_char_boundary(s.len() - suffix.len())
        && s[s.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

fn starts_with_ci(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

/// Targets that point outside the local file set.
fn is_external(target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    t.is_empty()
        || t.starts_with('#')
        || t.starts_with("//")
        || ["data:", "http:", "https:", "about:", "var(", "file:"]
            .iter()
            .any(|p| t.starts_with(p))
}

/// Lists local file references in `text`, in source order. Comments are skipped.
pub fn references(text: &str) -> Vec<CssRef> {
    let bytes = text.as_bytes();
    let mut refs = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &bytes[i..];
        if rest.starts_with(b"/*") {
            i = text[i + 2..]
                .find("*/")
                .map_or(bytes.len(), |p| i + 2 + p + 2);
        } else if starts_with_ci(rest, b"url(") {
            i = target_at(text, i + 4, true, &mut refs);
        } else if starts_with_ci(rest, b"@import") {
            // `@import url(...)` is picked up by the url( branch; only the
            // bare-string form needs handling here.
            let mut j = i + 7;
            while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
                j += 1;
            }
            i = if matches!(bytes.get(j), Some(b'"' | b'\'')) {
                target_at(text, j, false, &mut refs)
            } else {
                j
            };
        } else {
            i += 1;
        }
    }
    refs
}

/// Parses a target starting at `start` (after `url(` or at the opening
/// quote) and returns the index to continue scanning from.
fn target_at(text: &str, start: usize, allow_unquoted: bool, refs: &mut Vec<CssRef>) -> usize {
    let bytes = text.as_bytes();
    let mut j = start;
    while bytes.get(j).is_some_and(u8::is_ascii_whitespace) {
        j += 1;
    }
    let (from, to) = match bytes.get(j) {
        Some(&q @ (b'"' | b'\'')) => {
            let Some(len) = text[j + 1..].find(q as char) else {
                return bytes.len();
            };
            (j + 1, j + 1 + len)
        }
        Some(_) if allow_unquoted => {
            let Some(len) = text[j..].find(')') else {
                return bytes.len();
            };
            (j, j + len)
        }
        _ => return j,
    };
    let raw = &text[from..to];
    let trimmed = raw.trim();
    if !is_external(trimmed) {
        let lead = raw.len() - raw.trim_start().len();
        refs.push(CssRef {
            range: from + lead..from + lead + trimmed.len(),
            target: trimmed.to_string(),
        });
    }
    to + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(text: &str) -> Vec<String> {
        references(text).into_iter().map(|r| r.target).collect()
    }

    #[test]
    fn finds_imports_and_urls() {
        let css = r#"@import url("common.css");
@import 'other.css';
@namespace xs url(http://www.w3.org/2001/XMLSchema);
a { content: url( checked_box.gif ); background: URL(data:image/png;base64,AAA=) }
/* url(commented.png) */
b { x: url(../img/p.png?v=1) }"#;
        assert_eq!(
            targets(css),
            [
                "common.css",
                "other.css",
                "checked_box.gif",
                "../img/p.png?v=1"
            ]
        );
    }

    #[test]
    fn ranges_cover_the_target_only() {
        let css = "x{y:url( 'a b.png' )}";
        let r = &references(css)[0];
        assert_eq!(&css[r.range.clone()], "a b.png");
        assert!(!r.is_stylesheet());
        assert!(references("@import \"X.CSS\";")[0].is_stylesheet());
    }

    #[test]
    fn tolerates_truncated_input() {
        assert!(references("a{b:url(").is_empty());
        assert!(references("@import \"x").is_empty());
        assert!(references("é{b:url(é.png)}").len() == 1);
    }
}
