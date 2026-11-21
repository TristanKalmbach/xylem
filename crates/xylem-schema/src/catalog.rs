//! Entity and URI catalogs: map public identifiers, system identifiers and
//! URL prefixes to local files.
//!
//! Two formats are read:
//!
//! * `catalog.toml`:
//!   ```toml
//!   next_catalogs = ["s1000d/catalog.xml"]
//!
//!   [public]
//!   "-//S1000D//DTD Descriptive 2.3//EN" = "s1000d/2.3/descript.dtd"
//!
//!   [system]
//!   "http://example.com/a.dtd" = "local/a.dtd"
//!
//!   # Prefix rewriting for system identifiers and schema locations.
//!   [rewrite]
//!   "http://www.s1000d.org/S1000D_4-1/xml_schema_flat/" = "s1000d/4-1/"
//!   ```
//! * OASIS XML catalogs (`public`, `system`, `rewriteSystem`, `uri`,
//!   `rewriteURI`, `nextCatalog`, `group`, `xml:base`).
//!
//! Relative paths are relative to the catalog file.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use xylem_dom::Document;

use crate::error::{Result, SchemaError, read_file};
use crate::resolve::location_to_path;

/// Maps identifiers to local paths.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    public: HashMap<String, PathBuf>,
    system: HashMap<String, PathBuf>,
    uri: HashMap<String, PathBuf>,
    rewrite_system: Vec<(String, PathBuf)>,
    rewrite_uri: Vec<(String, PathBuf)>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct TomlCatalog {
    #[serde(alias = "nextCatalog", alias = "next")]
    next_catalogs: Vec<String>,
    public: HashMap<String, String>,
    system: HashMap<String, String>,
    uri: HashMap<String, String>,
    #[serde(alias = "prefix", alias = "rewriteSystem")]
    rewrite: HashMap<String, String>,
    #[serde(alias = "rewriteURI")]
    rewrite_uri: HashMap<String, String>,
}

fn normalize_public(id: &str) -> String {
    id.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Catalog {
    /// An empty catalog.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads a catalog file, choosing the format by extension (`.toml` or XML).
    pub fn load(path: impl AsRef<Path>) -> Result<Catalog> {
        let mut cat = Catalog::new();
        cat.load_into(path.as_ref(), &mut HashSet::new())?;
        Ok(cat)
    }

    /// Parses `catalog.toml` text; relative paths resolve against `base_dir`.
    pub fn from_toml_str(text: &str, base_dir: impl AsRef<Path>) -> Result<Catalog> {
        let mut cat = Catalog::new();
        cat.toml_into(
            text,
            base_dir.as_ref(),
            Path::new("catalog.toml"),
            &mut HashSet::new(),
        )?;
        Ok(cat)
    }

    /// Parses an OASIS XML catalog; relative paths resolve against `base_dir`.
    pub fn from_xml_str(text: &str, base_dir: impl AsRef<Path>) -> Result<Catalog> {
        let mut cat = Catalog::new();
        cat.xml_into(
            text,
            base_dir.as_ref(),
            Path::new("catalog.xml"),
            &mut HashSet::new(),
        )?;
        Ok(cat)
    }

    fn load_into(&mut self, path: &Path, seen: &mut HashSet<PathBuf>) -> Result<()> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if !seen.insert(key) {
            return Ok(());
        }
        let bytes = read_file(path)?;
        let text = String::from_utf8_lossy(&bytes);
        let base = path.parent().unwrap_or(Path::new("."));
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("toml"))
        {
            self.toml_into(&text, base, path, seen)
        } else {
            self.xml_into(&text, base, path, seen)
        }
    }

    fn toml_into(
        &mut self,
        text: &str,
        base: &Path,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        let parsed: TomlCatalog = toml::from_str(text).map_err(|e| SchemaError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let p = |s: &str| location_to_path(s, Some(base)).unwrap_or_else(|| PathBuf::from(s));
        for (k, v) in &parsed.public {
            self.public
                .entry(normalize_public(k))
                .or_insert_with(|| p(v));
        }
        for (k, v) in &parsed.system {
            self.system.entry(k.clone()).or_insert_with(|| p(v));
        }
        for (k, v) in &parsed.uri {
            self.uri.entry(k.clone()).or_insert_with(|| p(v));
        }
        for (k, v) in &parsed.rewrite {
            self.rewrite_system.push((k.clone(), p(v)));
            self.rewrite_uri.push((k.clone(), p(v)));
        }
        for (k, v) in &parsed.rewrite_uri {
            self.rewrite_uri.push((k.clone(), p(v)));
        }
        for next in &parsed.next_catalogs {
            self.load_into(&p(next), seen)?;
        }
        Ok(())
    }

    fn xml_into(
        &mut self,
        text: &str,
        base: &Path,
        path: &Path,
        seen: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        let doc = Document::from_str(text).map_err(|e| SchemaError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
        let Some(root) = doc.document_element() else {
            return Ok(());
        };
        let mut next = Vec::new();
        for n in doc.descendants(root) {
            let Some(el) = doc.element(n) else { continue };
            // Nearest xml:base on the element or its ancestors.
            let mut node_base = base.to_path_buf();
            let mut bases: Vec<&str> = std::iter::once(n)
                .chain(doc.ancestors(n))
                .filter_map(|a| doc.attr(a, "xml:base"))
                .collect();
            bases.reverse();
            for b in bases {
                if let Some(p) = location_to_path(b, Some(&node_base)) {
                    node_base = p;
                }
            }
            let target = |attr: &str| {
                el.attr(attr)
                    .and_then(|u| location_to_path(u, Some(&node_base)))
            };
            match el.local_name() {
                "public" => {
                    if let (Some(id), Some(t)) = (el.attr("publicId"), target("uri")) {
                        self.public.entry(normalize_public(id)).or_insert(t);
                    }
                }
                "system" => {
                    if let (Some(id), Some(t)) = (el.attr("systemId"), target("uri")) {
                        self.system.entry(id.to_string()).or_insert(t);
                    }
                }
                "uri" => {
                    if let (Some(id), Some(t)) = (el.attr("name"), target("uri")) {
                        self.uri.entry(id.to_string()).or_insert(t);
                    }
                }
                "rewriteSystem" => {
                    if let (Some(p), Some(t)) =
                        (el.attr("systemIdStartString"), target("rewritePrefix"))
                    {
                        self.rewrite_system.push((p.to_string(), t));
                    }
                }
                "rewriteURI" => {
                    if let (Some(p), Some(t)) = (el.attr("uriStartString"), target("rewritePrefix"))
                    {
                        self.rewrite_uri.push((p.to_string(), t));
                    }
                }
                "nextCatalog" => {
                    if let Some(t) = target("catalog") {
                        next.push(t);
                    }
                }
                _ => {}
            }
        }
        for n in next {
            // A missing next catalog is not fatal (OASIS: "ignore and continue").
            match self.load_into(&n, seen) {
                Err(SchemaError::Io { .. }) | Ok(()) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }

    /// Adds every entry of `other` that this catalog does not already map.
    pub fn merge(&mut self, other: &Catalog) {
        for (k, v) in &other.public {
            self.public.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.system {
            self.system.entry(k.clone()).or_insert_with(|| v.clone());
        }
        for (k, v) in &other.uri {
            self.uri.entry(k.clone()).or_insert_with(|| v.clone());
        }
        self.rewrite_system
            .extend(other.rewrite_system.iter().cloned());
        self.rewrite_uri.extend(other.rewrite_uri.iter().cloned());
    }

    pub fn add_public(&mut self, public_id: &str, path: impl Into<PathBuf>) {
        self.public.insert(normalize_public(public_id), path.into());
    }

    pub fn add_system(&mut self, system_id: &str, path: impl Into<PathBuf>) {
        self.system.insert(system_id.to_string(), path.into());
    }

    /// Maps every system identifier or URI starting with `prefix` into `dir`.
    pub fn add_prefix(&mut self, prefix: &str, dir: impl Into<PathBuf>) {
        let dir = dir.into();
        self.rewrite_system.push((prefix.to_string(), dir.clone()));
        self.rewrite_uri.push((prefix.to_string(), dir));
    }

    pub fn is_empty(&self) -> bool {
        self.public.is_empty()
            && self.system.is_empty()
            && self.uri.is_empty()
            && self.rewrite_system.is_empty()
            && self.rewrite_uri.is_empty()
    }

    pub fn resolve_public(&self, public_id: &str) -> Option<PathBuf> {
        self.public.get(&normalize_public(public_id)).cloned()
    }

    /// Resolves a system identifier (exact entries, then the longest rewrite prefix).
    pub fn resolve_system(&self, system_id: &str) -> Option<PathBuf> {
        self.system
            .get(system_id)
            .cloned()
            .or_else(|| rewrite(&self.rewrite_system, system_id))
    }

    /// Resolves a URI such as a schema location.
    pub fn resolve_uri(&self, uri: &str) -> Option<PathBuf> {
        self.uri
            .get(uri)
            .cloned()
            .or_else(|| rewrite(&self.rewrite_uri, uri))
            .or_else(|| self.resolve_system(uri))
    }

    /// Resolves an external identifier: the system identifier first, then the public one.
    pub fn resolve(&self, public_id: Option<&str>, system_id: Option<&str>) -> Option<PathBuf> {
        system_id
            .and_then(|s| self.resolve_system(s).or_else(|| self.uri.get(s).cloned()))
            .or_else(|| public_id.and_then(|p| self.resolve_public(p)))
    }
}

fn rewrite(rules: &[(String, PathBuf)], id: &str) -> Option<PathBuf> {
    let (prefix, target) = rules
        .iter()
        .filter(|(p, _)| id.starts_with(p.as_str()))
        .max_by_key(|(p, _)| p.len())?;
    let rest = crate::resolve::percent_decode(&id[prefix.len()..]);
    if rest.is_empty() {
        return Some(target.clone());
    }
    Some(target.join(rest.trim_start_matches('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_catalog() {
        let cat = Catalog::from_toml_str(
            r#"
            [public]
            "-//S1000D//DTD  Descriptive//EN" = "dtd/descript.dtd"
            [system]
            "http://example.com/a.dtd" = "a.dtd"
            [rewrite]
            "http://www.s1000d.org/S1000D_4-1/xml_schema_flat/" = "s1000d/4-1"
            "#,
            "/cat",
        )
        .unwrap();
        assert_eq!(
            cat.resolve_public("-//S1000D//DTD Descriptive//EN"),
            Some(PathBuf::from("/cat").join("dtd/descript.dtd"))
        );
        assert_eq!(
            cat.resolve_system("http://example.com/a.dtd"),
            Some(PathBuf::from("/cat").join("a.dtd"))
        );
        assert_eq!(
            cat.resolve_uri("http://www.s1000d.org/S1000D_4-1/xml_schema_flat/descript.xsd"),
            Some(
                PathBuf::from("/cat")
                    .join("s1000d/4-1")
                    .join("descript.xsd")
            )
        );
        assert!(
            cat.resolve(Some("-//S1000D//DTD Descriptive//EN"), Some("unknown.dtd"))
                .is_some()
        );
        assert!(Catalog::from_toml_str("bogus = 1", "/").is_err());
    }

    #[test]
    fn oasis_catalog() {
        let cat = Catalog::from_xml_str(
            r#"<catalog xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog">
                 <public publicId="-//X//DTD X//EN" uri="x.dtd"/>
                 <group xml:base="sub/">
                   <system systemId="urn:x:y" uri="y.dtd"/>
                 </group>
                 <rewriteSystem systemIdStartString="http://a/" rewritePrefix="local/a/"/>
                 <uri name="http://u/s.xsd" uri="s.xsd"/>
                 <rewriteURI uriStartString="http://b/" rewritePrefix="file:///opt/b/"/>
                 <nextCatalog catalog="does-not-exist.xml"/>
               </catalog>"#,
            "/c",
        )
        .unwrap();
        assert_eq!(
            cat.resolve_public("-//X//DTD X//EN"),
            Some(PathBuf::from("/c").join("x.dtd"))
        );
        assert_eq!(
            cat.resolve_system("urn:x:y"),
            Some(PathBuf::from("/c").join("sub/").join("y.dtd"))
        );
        assert_eq!(
            cat.resolve_system("http://a/b/c.dtd"),
            Some(PathBuf::from("/c").join("local/a/").join("b/c.dtd"))
        );
        assert_eq!(
            cat.resolve_uri("http://u/s.xsd"),
            Some(PathBuf::from("/c").join("s.xsd"))
        );
        assert!(
            cat.resolve_uri("http://b/z.xsd")
                .unwrap()
                .ends_with("z.xsd")
        );
    }

    #[test]
    fn merge_keeps_first() {
        let mut a = Catalog::new();
        a.add_public("P", "a");
        let mut b = Catalog::new();
        b.add_public("P", "b");
        b.add_system("S", "s");
        a.merge(&b);
        assert_eq!(a.resolve_public("P"), Some(PathBuf::from("a")));
        assert_eq!(a.resolve_system("S"), Some(PathBuf::from("s")));
    }
}
