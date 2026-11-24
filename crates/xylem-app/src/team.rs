//! Team-wide configuration (`xylem-team.toml`).
//!
//! A team keeps its doctype packages, schemas and dictionaries in one shared
//! place (a network folder or a checked-out repository) and points every
//! installation at it with a small TOML file. Xylem looks for the file in this
//! order and uses the first one it finds:
//!
//! 1. the path in `XYLEM_TEAM_CONFIG`;
//! 2. `xylem-team.toml` next to the executable (handy for IT-managed installs);
//! 3. `xylem-team.toml` in the user's config folder.
//!
//! Relative paths in the file are relative to the file itself. Everything is
//! optional. A user's own Preferences win over single values (schema folder,
//! BREX, dictionary); team package folders are scanned before the user's, so
//! on an equal match the team's package is used.
//!
//! A file can consist of just `include = "<path>"`, which reads the real
//! configuration from elsewhere. That is what the installer leaves next to
//! the executable, so that relative paths still resolve against the shared
//! folder.
//!
//! ```toml
//! package_dirs = ["doctypes"]
//! s1000d_schemas = "schemas/s1000d"
//! catalog = "schemas/catalog.xml"
//! brex = "brex/DMC-ACME-A-00-00-00-00A-022A-D_001-00_EN-US.XML"
//! dictionary_dir = "dictionaries"
//! dictionary_lang = "en_US"
//! update_source = "//fileserver/tools/xylem"
//!
//! # Schemas the documents point to on a server, read from a local copy:
//! [schema_locations]
//! "https://docs.example.com/schemas/" = "schemas/"
//!
//! # A schema for documents with this root (when no package gives one):
//! [[schema]]
//! root = "frontMatter"
//! schema = "schemas/front-matter.xsd"
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct TeamConfig {
    /// Read the configuration from this file instead.
    pub include: Option<PathBuf>,
    /// Folders scanned for doctype packages, before the user's own.
    pub package_dirs: Vec<PathBuf>,
    /// Folder laid out like www.s1000d.org (`S1000D_4-1/xml_schema_flat/...`).
    pub s1000d_schemas: Option<PathBuf>,
    /// OASIS XML catalog or `catalog.toml` for other schemas.
    pub catalog: Option<PathBuf>,
    /// Project BREX data module used by Check BREX.
    pub brex: Option<PathBuf>,
    /// Folder with Hunspell `.aff`/`.dic` files.
    pub dictionary_dir: Option<PathBuf>,
    pub dictionary_lang: Option<String>,
    /// Folder holding the current `xylem` executable and `version.txt`; see
    /// [`crate::install`].
    pub update_source: Option<PathBuf>,
    /// Program that runs stylesheets for Check stylesheet output
    /// (`SaxonDotnetHost.exe`, see `tools/saxon-dotnet`).
    pub xslt_host: Option<PathBuf>,
    /// Let XSLT 1.0 stylesheets run embedded scripts (`msxsl:script`), as
    /// some publishing services allow. Off unless the team turns it on:
    /// scripts are code that runs on the author's machine.
    pub xslt_allow_scripts: bool,
    /// How to preview a publication: which stylesheets to run on which
    /// documents (File > Publication preview).
    pub preview: Vec<PreviewConfig>,
    /// Schema locations on a server (or any URL prefix) mapped to local
    /// folders or files, so documents that point at a server validate
    /// against a local copy.
    pub schema_locations: std::collections::BTreeMap<String, PathBuf>,
    /// Schemas chosen by the document element (and optionally the schema
    /// the document points to), for documents no package gives a schema.
    pub schema: Vec<SchemaRule>,
    /// Where the configuration was read from (filled in by [`load`]).
    #[serde(skip)]
    pub source: Option<PathBuf>,
    /// Problems worth showing the user (unknown keys, unreadable includes).
    #[serde(skip)]
    pub warnings: Vec<String>,
}

/// One way to preview publications:
///
/// ```toml
/// [[preview]]
/// name = "Manual"                          # shown when several match
/// root = "book"                            # document element it applies to
/// stylesheet = "stylesheets/book-to-fo.xsl"
/// html = "stylesheets/fo-to-html.xsl"      # XSL-FO to HTML
/// params = { output = "html" }
/// ```
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct PreviewConfig {
    pub name: Option<String>,
    /// Document element name it applies to; any document when missing.
    pub root: Option<String>,
    /// Glob against the file name of the schema the document points to
    /// (`xsi:noNamespaceSchemaLocation`, `xsi:schemaLocation`, DOCTYPE).
    pub schema_location: Option<String>,
    /// Glob against the document's own file name.
    pub file_name: Option<String>,
    pub stylesheet: PathBuf,
    /// A stylesheet turning the first one's output (XSL-FO) into HTML. When
    /// missing, the first stylesheet's output is opened as it is.
    pub html: Option<PathBuf>,
    pub params: std::collections::BTreeMap<String, toml::Value>,
}

/// The facts about a document that previews and schema rules match on.
#[derive(Debug, Default, Clone)]
pub struct DocFacts {
    pub root: String,
    /// File names (last path part) of the schemas the document points to.
    pub schema_files: Vec<String>,
    pub file_name: Option<String>,
}

impl DocFacts {
    pub fn of(doc: &xylem_dom::Document, path: Option<&Path>) -> DocFacts {
        let root_el = doc.document_element();
        let root = root_el
            .and_then(|r| doc.name(r))
            .unwrap_or_default()
            .to_string();
        let mut locations: Vec<String> = Vec::new();
        if let Some(el) = root_el.and_then(|r| doc.element(r)) {
            for a in &el.attrs {
                match a.name.rsplit(':').next() {
                    Some("noNamespaceSchemaLocation") if a.name.contains(':') => {
                        locations.extend(a.value.split_whitespace().map(str::to_string))
                    }
                    Some("schemaLocation") if a.name.contains(':') => locations.extend(
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
        if let Some(sys) = doc.doctype().and_then(|d| d.system_id.clone()) {
            locations.push(sys);
        }
        DocFacts {
            root,
            schema_files: locations
                .iter()
                .filter_map(|l| l.rsplit(['/', '\\']).find(|p| !p.is_empty()))
                .map(str::to_string)
                .collect(),
            file_name: path
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned()),
        }
    }

    /// `root`, `schema_location` and `file_name` conditions all hold (a
    /// missing one always does).
    fn matches(&self, root: Option<&str>, schema: Option<&str>, file: Option<&str>) -> bool {
        use xylem_s1000d::glob_match;
        root.is_none_or(|r| r == self.root)
            && schema.is_none_or(|g| self.schema_files.iter().any(|f| glob_match(g, f)))
            && file.is_none_or(|g| self.file_name.as_deref().is_some_and(|f| glob_match(g, f)))
    }
}

/// A schema for documents with a given root:
///
/// ```toml
/// [[schema]]
/// root = "frontMatter"                       # document element
/// schema_location = "*Front_Matter*.xsd"     # optional: what it points to
/// schema = "schemas/front-matter.xsd"        # .xsd or .dtd
/// ```
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct SchemaRule {
    pub root: Option<String>,
    pub schema_location: Option<String>,
    pub file_name: Option<String>,
    pub schema: PathBuf,
}

impl SchemaRule {
    pub fn applies_to(&self, facts: &DocFacts) -> bool {
        facts.matches(
            self.root.as_deref(),
            self.schema_location.as_deref(),
            self.file_name.as_deref(),
        )
    }
}

impl PreviewConfig {
    pub fn applies_to(&self, facts: &DocFacts) -> bool {
        facts.matches(
            self.root.as_deref(),
            self.schema_location.as_deref(),
            self.file_name.as_deref(),
        )
    }

    /// Parameters as the stylesheet receives them.
    pub fn param_strings(&self) -> std::collections::BTreeMap<String, String> {
        self.params
            .iter()
            .map(|(k, v)| {
                let s = match v {
                    toml::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                (k.clone(), s)
            })
            .collect()
    }
}

const KNOWN_KEYS: &[&str] = &[
    "include",
    "package_dirs",
    "s1000d_schemas",
    "catalog",
    "brex",
    "dictionary_dir",
    "dictionary_lang",
    "update_source",
    "xslt_host",
    "xslt_allow_scripts",
    "preview",
    "schema_locations",
    "schema",
];
const MAX_INCLUDES: usize = 4;

fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("XYLEM_TEAM_CONFIG") {
        v.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        v.push(dir.join("xylem-team.toml"));
    }
    if let Some(dir) = crate::settings::config_dir() {
        v.push(dir.join("xylem-team.toml"));
    }
    v
}

/// Reads the first team configuration found. A broken shared file must not
/// stop anyone from editing, so errors become warnings on an empty config.
pub fn load() -> TeamConfig {
    for path in candidates() {
        if !path.is_file() {
            continue;
        }
        let cfg = match parse(&path) {
            Ok(cfg) => cfg,
            Err(e) => TeamConfig {
                source: Some(path.clone()),
                warnings: vec![format!("{}: {e}", path.display())],
                ..TeamConfig::default()
            },
        };
        log::info!("team configuration: {}", path.display());
        for w in &cfg.warnings {
            log::warn!("team configuration: {w}");
        }
        return cfg;
    }
    TeamConfig::default()
}

/// Parses `path`, following `include` chains.
pub fn parse(path: &Path) -> Result<TeamConfig, String> {
    let mut path = path.to_path_buf();
    let mut warnings = Vec::new();
    for _ in 0..=MAX_INCLUDES {
        let mut cfg = parse_one(&path)?;
        warnings.append(&mut cfg.warnings);
        match cfg.include.take() {
            Some(next) => path = next,
            None => {
                cfg.warnings = warnings;
                return Ok(cfg);
            }
        }
    }
    Err(format!("more than {MAX_INCLUDES} nested includes"))
}

fn parse_one(path: &Path) -> Result<TeamConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let table: toml::Table =
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    // Unknown keys are warned about, not rejected: a shared file gains keys
    // over time and older installations must keep working with it.
    let warnings = table
        .keys()
        .filter(|k| !KNOWN_KEYS.contains(&k.as_str()))
        .map(|k| format!("{}: unknown key `{k}`", path.display()))
        .collect();
    let mut cfg: TeamConfig = table
        .try_into()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    cfg.warnings = warnings;
    let base = path.parent().unwrap_or(Path::new("."));
    let resolve = |p: &mut PathBuf| {
        if p.is_relative() {
            *p = base.join(&*p);
        }
    };
    cfg.package_dirs.iter_mut().for_each(resolve);
    for p in [
        &mut cfg.include,
        &mut cfg.s1000d_schemas,
        &mut cfg.catalog,
        &mut cfg.brex,
        &mut cfg.dictionary_dir,
        &mut cfg.update_source,
        &mut cfg.xslt_host,
    ]
    .into_iter()
    .flatten()
    {
        resolve(p);
    }
    for p in cfg.schema_locations.values_mut() {
        resolve(p);
    }
    for s in &mut cfg.schema {
        resolve(&mut s.schema);
    }
    for p in &mut cfg.preview {
        resolve(&mut p.stylesheet);
        if let Some(h) = &mut p.html {
            resolve(h);
        }
    }
    cfg.source = Some(path.to_path_buf());
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_resolve_against_the_file() {
        let dir = std::env::temp_dir().join(format!("xylem-team-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("xylem-team.toml");
        // An absolute path on every platform (a bare "/x" has no drive on Windows).
        let elsewhere = std::env::temp_dir().join("elsewhere");
        let text = format!(
            "package_dirs = [\"doctypes\", {:?}]\ns1000d_schemas = \"schemas/s1000d\"\n",
            elsewhere.to_string_lossy()
        );
        std::fs::write(&file, text).unwrap();
        let cfg = parse(&file).unwrap();
        assert_eq!(cfg.package_dirs[0], dir.join("doctypes"));
        assert_eq!(cfg.package_dirs[1], elsewhere);
        assert_eq!(cfg.s1000d_schemas, Some(dir.join("schemas/s1000d")));
        std::fs::write(&file, "pakage_dirs = []\n").unwrap();
        let cfg = parse(&file).unwrap();
        assert!(
            cfg.warnings[0].contains("pakage_dirs"),
            "typos are reported, not silently ignored"
        );

        // A stub elsewhere that includes the shared file resolves paths
        // against the shared file, not the stub.
        let stub_dir = dir.join("install");
        std::fs::create_dir_all(&stub_dir).unwrap();
        std::fs::write(&file, "package_dirs = [\"doctypes\"]\n").unwrap();
        let stub = stub_dir.join("xylem-team.toml");
        std::fs::write(&stub, format!("include = {:?}\n", file.to_string_lossy())).unwrap();
        let cfg = parse(&stub).unwrap();
        assert_eq!(cfg.package_dirs, vec![dir.join("doctypes")]);
        assert_eq!(cfg.source, Some(file.clone()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schemas_and_previews_match_on_root_and_schema_location() {
        let dir = std::env::temp_dir().join(format!("xylem-team-schema-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("xylem-team.toml");
        std::fs::write(
            &file,
            r#"
[schema_locations]
"https://docs.example.com/schemas/" = "schemas"

[[schema]]
root = "frontMatter"
schema = "schemas/front.xsd"

[[preview]]
root = "publication"
schema_location = "*Cover*.xsd"
stylesheet = "cover.xsl"

[[preview]]
root = "publication"
stylesheet = "book.xsl"
"#,
        )
        .unwrap();
        let cfg = parse(&file).unwrap();
        assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
        assert_eq!(
            cfg.schema_locations["https://docs.example.com/schemas/"],
            dir.join("schemas")
        );
        assert_eq!(cfg.schema[0].schema, dir.join("schemas/front.xsd"));

        let doc = |xml: &str| xylem_dom::Document::from_str(xml).unwrap();
        let front = DocFacts::of(&doc("<frontMatter/>"), None);
        assert!(cfg.schema[0].applies_to(&front));
        let cover = DocFacts::of(
            &doc(r#"<publication xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="https://docs.example.com/schemas/Book-Cover.xsd"/>"#),
            None,
        );
        assert_eq!(cover.schema_files, vec!["Book-Cover.xsd".to_string()]);
        let chosen = cfg.preview.iter().find(|p| p.applies_to(&cover)).unwrap();
        assert_eq!(chosen.stylesheet, dir.join("cover.xsl"));
        let book = DocFacts::of(&doc("<publication/>"), None);
        let chosen = cfg.preview.iter().find(|p| p.applies_to(&book)).unwrap();
        assert_eq!(chosen.stylesheet, dir.join("book.xsl"));
        assert!(!cfg.schema[0].applies_to(&book));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
