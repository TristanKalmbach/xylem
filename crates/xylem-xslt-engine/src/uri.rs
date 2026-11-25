//! URIs for `base-uri()`, `document-uri()`, `static-base-uri()` and
//! `resolve-uri()`.
//!
//! Files are identified by `file:///` URIs built from their paths, with
//! characters outside the URI syntax percent-encoded (UTF-8). Resolution
//! follows RFC 3986, and results are written the way the reference
//! processor writes them: a `file` URI with an empty authority comes out as
//! `file:/C:/...` after resolution, and the relative reference keeps its
//! characters as given (`a b.png` stays unescaped).

use std::path::Path;

/// The `file:///` URI of a path (`C:\a b\x.xsl` gives
/// `file:///C:/a%20b/x.xsl`). A directory's URI has no trailing slash.
pub fn from_path(path: &Path) -> String {
    let s = path.to_string_lossy();
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s).replace('\\', "/");
    let mut out = String::from("file://");
    if !s.starts_with('/') {
        out.push('/');
    }
    for b in s.bytes() {
        let keep = b.is_ascii_alphanumeric() || b"-._~/:@!$&'()*+,;=".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The path of a `file:` URI (`file:///C:/a%20b/x.xsl` gives
/// `C:/a b/x.xsl`), percent-decoded; `None` for other schemes.
pub fn to_path(uri: &str) -> Option<std::path::PathBuf> {
    let rest = uri.strip_prefix("file:")?;
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    // `file:///C:/x` and `file:/C:/x` name a drive path; `file:///x` a root.
    let rest = match rest.strip_prefix('/') {
        Some(r) if r.as_bytes().get(1) == Some(&b':') => r,
        _ => rest,
    };
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(h) = rest.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(h, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).into_owned().into())
}

/// The parts of a URI reference (RFC 3986, appendix B).
struct Parts<'a> {
    scheme: Option<&'a str>,
    authority: Option<&'a str>,
    path: &'a str,
    query: Option<&'a str>,
    fragment: Option<&'a str>,
}

fn split(s: &str) -> Parts<'_> {
    let (s, fragment) = match s.find('#') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (s, query) = match s.find('?') {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let scheme_end = s.find(':').filter(|&i| {
        i > 0
            && s[..i].starts_with(|c: char| c.is_ascii_alphabetic())
            && s[..i]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
            && !s[..i].contains('/')
    });
    let (scheme, rest) = match scheme_end {
        Some(i) => (Some(&s[..i]), &s[i + 1..]),
        None => (None, s),
    };
    let (authority, path) = match rest.strip_prefix("//") {
        Some(r) => {
            let end = r.find('/').unwrap_or(r.len());
            (Some(&r[..end]), &r[end..])
        }
        None => (None, rest),
    };
    Parts {
        scheme,
        authority,
        path,
        query,
        fragment,
    }
}

/// RFC 3986 section 5.2.4.
fn remove_dot_segments(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let absolute = path.starts_with('/');
    let segs: Vec<&str> = path.split('/').collect();
    let last = segs.len().saturating_sub(1);
    for (i, seg) in segs.iter().enumerate() {
        match *seg {
            "." => {
                if i == last {
                    out.push("");
                }
            }
            ".." => {
                if out.len() > usize::from(absolute) {
                    out.pop();
                }
                if i == last {
                    out.push("");
                }
            }
            s => out.push(s),
        }
    }
    let mut r = out.join("/");
    if absolute && !r.starts_with('/') {
        r.insert(0, '/');
    }
    r
}

/// Resolves `relative` against the absolute URI `base`.
pub fn resolve(relative: &str, base: &str) -> Result<String, String> {
    let r = split(relative);
    if r.scheme.is_some() {
        return Ok(relative.to_string());
    }
    if relative.is_empty() {
        return Ok(base.to_string());
    }
    let b = split(base);
    let Some(scheme) = b.scheme else {
        return Err(format!(
            "FORG0002: the base URI '{base}' of resolve-uri() is not absolute"
        ));
    };
    let (authority, path, query) = if r.authority.is_some() {
        (r.authority, remove_dot_segments(r.path), r.query)
    } else if r.path.is_empty() {
        (b.authority, b.path.to_string(), r.query.or(b.query))
    } else if r.path.starts_with('/') {
        (b.authority, remove_dot_segments(r.path), r.query)
    } else {
        let merged = if b.authority.is_some() && b.path.is_empty() {
            format!("/{}", r.path)
        } else {
            match b.path.rfind('/') {
                Some(i) => format!("{}{}", &b.path[..=i], r.path),
                None => r.path.to_string(),
            }
        };
        (b.authority, remove_dot_segments(&merged), r.query)
    };
    let mut out = format!("{scheme}:");
    // An empty authority is dropped, as the reference processor does.
    if let Some(a) = authority.filter(|a| !a.is_empty()) {
        out.push_str("//");
        out.push_str(a);
    }
    out.push_str(&path);
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    if let Some(f) = r.fragment {
        out.push('#');
        out.push_str(f);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_become_file_uris() {
        assert_eq!(
            from_path(Path::new(r"\\?\C:\a b\x.xsl")),
            "file:///C:/a%20b/x.xsl"
        );
        assert_eq!(from_path(Path::new("/tmp/é")), "file:///tmp/%C3%A9");
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            to_path("file:///C:/a%20b/x.xsl").unwrap(),
            Path::new("C:/a b/x.xsl")
        );
        assert_eq!(to_path("file:/C:/x").unwrap(), Path::new("C:/x"));
        assert_eq!(to_path("file:///tmp/x").unwrap(), Path::new("/tmp/x"));
        assert!(to_path("http://e.com/x").is_none());
    }

    #[test]
    fn resolution_matches_the_reference_processor() {
        let base = "file:///C:/p/sp%20ace/u.xsl";
        assert_eq!(
            resolve("../img/a b.png", base).unwrap(),
            "file:/C:/p/img/a b.png"
        );
        assert_eq!(resolve("x.png", base).unwrap(), "file:/C:/p/sp%20ace/x.png");
        assert_eq!(
            resolve("/root/x", "file:///C:/a/b.xsl").unwrap(),
            "file:/root/x"
        );
        assert_eq!(
            resolve("http://e.com/a/b", base).unwrap(),
            "http://e.com/a/b"
        );
        assert_eq!(
            resolve("../../c?q=1#f", "http://e.com/a/b/c").unwrap(),
            "http://e.com/c?q=1#f"
        );
        assert_eq!(resolve("", "http://e.com/a/b").unwrap(), "http://e.com/a/b");
        assert_eq!(
            resolve("sub/", "file:///C:/p/sp%20ace").unwrap(),
            "file:/C:/p/sub/"
        );
        assert!(resolve("x", "rel/base").is_err());
    }
}
