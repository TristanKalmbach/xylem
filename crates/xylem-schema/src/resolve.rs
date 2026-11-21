//! Locating schema files: URI-to-path conversion, catalog lookup, and the
//! choice of grammar for an instance document.

use std::path::{Path, PathBuf};

use xylem_dom::Document;

use crate::catalog::Catalog;
use crate::error::{Result, SchemaError};
use crate::{Grammar, NsScope, XSI_NS, dtd};

/// Decodes `%XX` escapes (invalid escapes are kept literally).
pub(crate) fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// True for locations with a non-`file` URL scheme (`http:`, `urn:`, ...).
pub(crate) fn is_remote(loc: &str) -> bool {
    match loc.split_once(':') {
        // A single letter before ':' is a Windows drive, not a scheme.
        Some((scheme, _)) => {
            scheme.len() > 1
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && !scheme.eq_ignore_ascii_case("file")
        }
        None => false,
    }
}

/// Converts a local location (relative path, absolute path or `file:` URL)
/// to a path. Remote URLs yield `None`.
pub(crate) fn location_to_path(loc: &str, base: Option<&Path>) -> Option<PathBuf> {
    let loc = loc.trim();
    if let Some(rest) = loc
        .strip_prefix("file://")
        .or_else(|| loc.strip_prefix("file:"))
    {
        let decoded = percent_decode(rest);
        // file:///C:/x -> C:/x on Windows-style paths.
        let b = decoded.as_bytes();
        let p = if b.len() > 2 && b[0] == b'/' && b[2] == b':' {
            &decoded[1..]
        } else {
            &decoded[..]
        };
        return Some(PathBuf::from(p));
    }
    if is_remote(loc) {
        return None;
    }
    let p = PathBuf::from(percent_decode(loc));
    Some(match base {
        Some(b) if p.is_relative() => b.join(p),
        _ => p,
    })
}

/// Resolves a schema location relative to `base`, consulting the catalog
/// first. A location that does not exist falls back to a file with the same
/// name in `base` (documents often carry stale absolute or relative paths).
pub(crate) fn resolve_location(
    loc: &str,
    base: Option<&Path>,
    catalog: &Catalog,
) -> Result<PathBuf> {
    if let Some(p) = catalog.resolve_uri(loc) {
        return Ok(p);
    }
    if let Some(p) = location_to_path(loc, base) {
        if p.exists() {
            return Ok(p);
        }
        if let (Some(b), Some(name)) = (base, p.file_name()) {
            let sibling = b.join(name);
            if sibling.exists() {
                return Ok(sibling);
            }
        }
        return Ok(p);
    }
    // Remote: try a same-named local file as a last resort.
    if let (Some(b), Some(name)) = (base, loc.rsplit('/').next().filter(|n| !n.is_empty())) {
        let sibling = b.join(percent_decode(name));
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    Err(SchemaError::Unresolved(format!(
        "'{loc}' (remote locations need a catalog entry)"
    )))
}

pub(crate) fn for_document(
    doc: &Document,
    doc_path: Option<&Path>,
    catalog: &Catalog,
) -> Option<Result<Grammar>> {
    let base = doc_path.and_then(Path::parent);
    let root = doc.document_element();
    let mut locations = Vec::new();
    if let Some(root) = root {
        let scope = NsScope::at(doc, root);
        let el = doc.element(root)?;
        for a in &el.attrs {
            let Some((prefix, local)) = a.name.split_once(':') else {
                continue;
            };
            if prefix != "xsi" && scope.resolve(prefix) != Some(XSI_NS) {
                continue;
            }
            match local {
                "noNamespaceSchemaLocation" => {
                    locations.extend(a.value.split_whitespace().map(str::to_string))
                }
                "schemaLocation" => locations.extend(
                    a.value
                        .split_whitespace()
                        .skip(1)
                        .step_by(2)
                        .map(str::to_string),
                ),
                _ => {}
            }
        }
    }
    let doctype = doc.doctype();
    if !locations.is_empty() {
        let result = (|| {
            let paths: Vec<PathBuf> = locations
                .iter()
                .map(|l| resolve_location(l, base, catalog))
                .collect::<Result<_>>()?;
            let mut g = Grammar::from_xsd_files(&paths, catalog)?;
            if let Some(subset) = doctype.and_then(|d| d.internal_subset.as_deref()) {
                dtd::merge_internal_subset(&mut g, subset, base.unwrap_or(Path::new(".")), catalog);
            }
            Ok(g)
        })();
        return Some(result);
    }
    let dt = doctype?;
    let external = match (&dt.public_id, &dt.system_id) {
        (None, None) => None,
        (public, system) => {
            let found = catalog
                .resolve(public.as_deref(), system.as_deref())
                .or_else(|| {
                    system
                        .as_deref()
                        .and_then(|s| resolve_location(s, base, catalog).ok())
                        .filter(|p| p.exists())
                });
            match found {
                Some(p) => Some(p),
                None if dt.internal_subset.is_some() => None,
                None => {
                    let id = system
                        .clone()
                        .or_else(|| public.clone())
                        .unwrap_or_default();
                    return Some(Err(SchemaError::Unresolved(format!("DOCTYPE '{id}'"))));
                }
            }
        }
    };
    if external.is_none() && dt.internal_subset.is_none() {
        return None;
    }
    Some(dtd::load_doctype(
        dt.internal_subset.as_deref(),
        external.as_deref(),
        base.unwrap_or(Path::new(".")),
        catalog,
        &dt.name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locations() {
        assert_eq!(percent_decode("a%20b%zz"), "a b%zz");
        assert!(is_remote("http://x/y.xsd"));
        assert!(!is_remote("C:/x/y.xsd"));
        assert!(!is_remote("file:///x"));
        assert_eq!(
            location_to_path("file:///C:/a%20b/x.xsd", None),
            Some(PathBuf::from("C:/a b/x.xsd"))
        );
        assert_eq!(
            location_to_path("file:///opt/x.xsd", None),
            Some(PathBuf::from("/opt/x.xsd"))
        );
        assert_eq!(
            location_to_path("x.xsd", Some(Path::new("/base"))),
            Some(PathBuf::from("/base/x.xsd"))
        );
        assert_eq!(location_to_path("http://x/y", None), None);
    }
}
