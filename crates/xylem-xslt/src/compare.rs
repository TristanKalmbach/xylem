//! Comparing two outputs of a stylesheet.
//!
//! Before comparing, both sides are normalised for differences that are not
//! real:
//!
//! - a byte order mark (and UTF-16 versus UTF-8);
//! - IDs made by `generate-id()`. Saxon writes them as `d<document>e<node>`,
//!   and the document number comes from a counter that keeps rising while
//!   the processor runs (a service that caches compiled stylesheets keeps one
//!   counter across all its publishes). Document numbers are renumbered in
//!   order of first appearance; the node number is kept.
//!
//! What still differs is reported as a list of [`Difference`]s, each with the
//! element it is in.

use similar::{Algorithm, DiffOp, capture_diff_slices_deadline};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// How many differences are described in detail; the rest are only counted.
pub const MAX_DESCRIBED: usize = 20;
/// Characters of each side shown for a difference.
const EXCERPT: usize = 240;

/// The result of comparing a baseline with a candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct Comparison {
    /// Same after normalising.
    pub identical: bool,
    /// Same bytes, before normalising.
    pub byte_identical: bool,
    /// Number of separate places that differ.
    pub count: usize,
    /// The first [`MAX_DESCRIBED`] of them.
    pub differences: Vec<Difference>,
    /// The comparison gave up on a fine-grained listing (very different
    /// outputs); `differences` then holds one entry for the whole region.
    pub coarse: bool,
}

/// One place where the outputs differ.
#[derive(Debug, Clone, PartialEq)]
pub struct Difference {
    /// Element path at the difference in the baseline, such as
    /// `fo:root/fo:page-sequence/fo:flow/fo:block`.
    pub path: String,
    /// The `id` of the nearest enclosing element that has one.
    pub near_id: Option<String>,
    /// Byte offset in the normalised baseline.
    pub offset: usize,
    /// Byte length of the changed text in the normalised baseline (which
    /// `baseline` may shorten).
    pub baseline_len: usize,
    pub baseline: String,
    pub candidate: String,
}

/// Decodes output bytes (UTF-8 or UTF-16 with a byte order mark).
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |le: bool| {
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&c| {
                if le {
                    u16::from_le_bytes(c)
                } else {
                    u16::from_be_bytes(c)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return utf16(true);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return utf16(false);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

/// Renumbers the document part of `generate-id()` values (`d31e20` →
/// `d1e20`) in order of first appearance.
pub fn renumber_generated_ids(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut docs: HashMap<&str, usize> = HashMap::new();
    let mut last = 0;
    let mut i = 0;
    // A generated ID can be joined to other text (`continuedTable-d62e206`),
    // so only a letter or digit next to it means it is part of a word.
    let word = |c: u8| c.is_ascii_alphanumeric();
    while i < b.len() {
        if b[i] == b'd' && (i == 0 || !word(b[i - 1])) {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < b.len() && b[j] == b'e' {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                if k > j + 1 && (k == b.len() || !word(b[k])) {
                    let doc = &text[i + 1..j];
                    let n = docs.len() + 1;
                    let n = *docs.entry(doc).or_insert(n);
                    out.push_str(&text[last..i]);
                    out.push('d');
                    out.push_str(&n.to_string());
                    out.push_str(&text[j..k]);
                    last = k;
                    i = k;
                    continue;
                }
            }
        }
        i += 1;
    }
    out.push_str(&text[last..]);
    out
}

/// Output as it is compared: decoded and with generated IDs renumbered.
pub fn normalise(bytes: &[u8]) -> String {
    renumber_generated_ids(&decode(bytes))
}

/// Splits markup into tags and the text between them.
fn tokens(s: &str) -> Vec<&str> {
    let mut v = Vec::new();
    let mut start = 0;
    let b = s.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c == b'<' && i > start {
            v.push(&s[start..i]);
            start = i;
        } else if c == b'>' {
            v.push(&s[start..=i]);
            start = i + 1;
        }
    }
    if start < s.len() {
        v.push(&s[start..]);
    }
    v
}

fn excerpt(parts: &[&str]) -> String {
    let joined: String = parts.concat();
    if joined.chars().count() <= EXCERPT {
        joined
    } else {
        let cut: String = joined.chars().take(EXCERPT).collect();
        format!("{cut}…")
    }
}

/// The element name of a start tag (`<fo:block id="x">` → `fo:block`).
fn tag_name(tag: &str) -> &str {
    let inner = tag.trim_start_matches('<').trim_start_matches('/');
    let end = inner
        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .unwrap_or(inner.len());
    &inner[..end]
}

fn tag_id(tag: &str) -> Option<String> {
    let at = tag.find(" id=")?;
    let rest = &tag[at + 4..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)?;
    Some(rest[1..1 + end].to_string())
}

/// Element paths at token positions `wanted` (sorted), in one pass.
fn paths_at(toks: &[&str], wanted: &[usize]) -> Vec<(String, Option<String>)> {
    let mut stack: Vec<(&str, Option<String>)> = Vec::new();
    let mut out = Vec::new();
    let mut next = 0;
    for (i, t) in toks.iter().enumerate() {
        while next < wanted.len() && wanted[next] == i {
            let path = stack.iter().map(|(n, _)| *n).collect::<Vec<_>>().join("/");
            let id = stack.iter().rev().find_map(|(_, id)| id.clone());
            out.push((path, id));
            next += 1;
        }
        if t.starts_with("</") {
            stack.pop();
        } else if t.starts_with('<')
            && !t.starts_with("<?")
            && !t.starts_with("<!")
            && !t.ends_with("/>")
        {
            stack.push((tag_name(t), tag_id(t)));
        }
    }
    while next < wanted.len() {
        let path = stack.iter().map(|(n, _)| *n).collect::<Vec<_>>().join("/");
        out.push((path, None));
        next += 1;
    }
    out
}

/// Compares a baseline output with a candidate output.
pub fn compare(baseline: &[u8], candidate: &[u8]) -> Comparison {
    let byte_identical = baseline == candidate;
    let a = normalise(baseline);
    let b = normalise(candidate);
    if a == b {
        return Comparison {
            identical: true,
            byte_identical,
            count: 0,
            differences: Vec::new(),
            coarse: false,
        };
    }
    let ta = tokens(&a);
    let tb = tokens(&b);
    let deadline = Instant::now() + Duration::from_secs(10);
    let ops = capture_diff_slices_deadline(Algorithm::Myers, &ta, &tb, Some(deadline));
    // Changed regions: runs of non-equal operations.
    let mut regions: Vec<(usize, usize, usize, usize)> = Vec::new();
    for op in &ops {
        let (ar, br) = match *op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index..old_index + old_len, new_index..new_index),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
            ),
        };
        match regions.last_mut() {
            Some(r) if r.1 == ar.start && r.3 == br.start => {
                r.1 = ar.end;
                r.3 = br.end;
            }
            _ => regions.push((ar.start, ar.end, br.start, br.end)),
        }
    }
    // A diff that ran out of time ends in one large replacement.
    let coarse = Instant::now() >= deadline;
    let count = regions.len();
    let shown: Vec<_> = regions.iter().take(MAX_DESCRIBED).collect();
    let starts: Vec<usize> = shown.iter().map(|r| r.0).collect();
    let paths = paths_at(&ta, &starts);
    let offsets: Vec<usize> = {
        let mut acc = 0;
        let mut pos = Vec::with_capacity(ta.len() + 1);
        for t in &ta {
            pos.push(acc);
            acc += t.len();
        }
        pos.push(acc);
        starts.iter().map(|&s| pos[s]).collect()
    };
    let differences = shown
        .iter()
        .zip(paths)
        .zip(offsets)
        .map(|((r, (path, near_id)), offset)| Difference {
            path,
            near_id,
            offset,
            baseline_len: ta[r.0..r.1].iter().map(|t| t.len()).sum(),
            baseline: excerpt(&ta[r.0..r.1]),
            candidate: excerpt(&tb[r.2..r.3]),
        })
        .collect();
    Comparison {
        identical: false,
        byte_identical,
        count,
        differences,
        coarse,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_ids_are_renumbered_by_first_appearance() {
        let a = r#"<b id="d31e20"/><x ref="d31e66"/><y id="d40e5"/>"#;
        let b = r#"<b id="d7709e20"/><x ref="d7709e66"/><y id="d7712e5"/>"#;
        assert_eq!(renumber_generated_ids(a), renumber_generated_ids(b));
        assert_eq!(
            renumber_generated_ids(a),
            r#"<b id="d1e20"/><x ref="d1e66"/><y id="d2e5"/>"#
        );
        // Words that merely look alike are left alone.
        assert_eq!(
            renumber_generated_ids("add12e3 d1e2x id9e9"),
            "add12e3 d1e2x id9e9"
        );
        // IDs joined to other text are still IDs.
        assert_eq!(
            renumber_generated_ids("continuedTable-d62e206 row_d62e9"),
            "continuedTable-d1e206 row_d1e9"
        );
    }

    #[test]
    fn identical_after_normalising() {
        let a = "\u{feff}<r><b id=\"d3e1\">x</b></r>".as_bytes();
        let b = "<r><b id=\"d9e1\">x</b></r>".as_bytes();
        let c = compare(a, b);
        assert!(c.identical && !c.byte_identical && c.count == 0);
    }

    #[test]
    fn differences_name_where_they_are() {
        let a = br#"<fo:root><fo:flow><fo:block id="s1">Step <fo:inline>1</fo:inline></fo:block><fo:block>Same</fo:block><fo:block id="s3">Old</fo:block></fo:flow></fo:root>"#;
        let b = br#"<fo:root><fo:flow><fo:block id="s1">Step <fo:inline>2</fo:inline></fo:block><fo:block>Same</fo:block><fo:block id="s3">New</fo:block></fo:flow></fo:root>"#;
        let c = compare(a, b);
        assert!(!c.identical);
        assert_eq!(c.count, 2);
        assert_eq!(c.differences[0].path, "fo:root/fo:flow/fo:block/fo:inline");
        assert_eq!(c.differences[0].near_id.as_deref(), Some("s1"));
        assert_eq!(c.differences[0].baseline, "1");
        assert_eq!(c.differences[0].candidate, "2");
        assert_eq!(c.differences[1].near_id.as_deref(), Some("s3"));
        assert_eq!(
            (
                c.differences[1].baseline.as_str(),
                c.differences[1].candidate.as_str()
            ),
            ("Old", "New")
        );
    }

    #[test]
    fn utf16_output_decodes() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "<a/>".encode_utf16() {
            bytes.extend(u.to_le_bytes());
        }
        assert!(compare(&bytes, b"<a/>").identical);
    }
}
