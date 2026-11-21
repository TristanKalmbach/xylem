//! The native "Xylem doctype package": a folder with a `doctype.toml`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::behaviour::{Behaviour, PrettyPrint};
use crate::error::{Error, Result};
use crate::template::Template;
use crate::toolbar::{ToolbarEntry, default_toolbar};

/// Name of the manifest file inside a package folder.
pub const MANIFEST_NAME: &str = "doctype.toml";

/// `[doctype]`: identity of the package.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DoctypeInfo {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "DoctypeInfo::default_version")]
    pub version: String,
}

impl DoctypeInfo {
    fn default_version() -> String {
        "1.0".to_string()
    }
}

impl Default for DoctypeInfo {
    fn default() -> Self {
        Self {
            name: String::new(),
            description: String::new(),
            version: Self::default_version(),
        }
    }
}

/// `[match]`: how the editor decides this package fits an opened document.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRules {
    /// Document element names (exact; a local-name comparison is used as a
    /// fallback for prefixed roots).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roots: Vec<String>,
    /// Globs tested against the file name of each schema location the
    /// document declares (`xsi:noNamespaceSchemaLocation`, `xsi:schemaLocation`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schema_locations: Vec<String>,
    /// Substrings of the DOCTYPE public identifier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub public_ids: Vec<String>,
    /// Globs tested against the document's own file name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_names: Vec<String>,
    /// Higher wins when several packages match.
    #[serde(default)]
    pub priority: i32,
}

/// `[files]`: package-relative paths of the supporting files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Files {
    /// XSD or DTD.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Main stylesheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// Stylesheet for the structure (tag) view.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure_style: Option<String>,
    /// DTD entity declarations.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entities: Option<String>,
}

/// Absolute paths of the files a package refers to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedFiles {
    pub schema: Option<PathBuf>,
    pub style: Option<PathBuf>,
    pub structure_style: Option<PathBuf>,
    pub entities: Option<PathBuf>,
}

/// A loaded doctype package.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DoctypePackage {
    pub doctype: DoctypeInfo,
    #[serde(rename = "match", default)]
    pub matching: MatchRules,
    #[serde(default)]
    pub files: Files,
    /// `[templates.<element>]`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub templates: BTreeMap<String, Template>,
    #[serde(default)]
    pub behaviour: Behaviour,
    #[serde(default)]
    pub pretty_print: PrettyPrint,
    /// Element -> long description.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub descriptions: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub toolbar: Vec<ToolbarEntry>,
    /// Absolute folder the package was loaded from; not serialised.
    #[serde(skip)]
    base_dir: Option<PathBuf>,
}

impl DoctypePackage {
    /// An empty package with the given identity (no files, no rules, no toolbar).
    pub fn new(doctype: DoctypeInfo) -> Self {
        Self {
            doctype,
            ..Self::default()
        }
    }

    /// The package used for plain XML: no files, no templates, default toolbar.
    pub fn generic() -> Self {
        let mut pkg = Self {
            doctype: DoctypeInfo {
                name: "generic".into(),
                description: "Plain XML without a doctype package".into(),
                version: DoctypeInfo::default_version(),
            },
            matching: MatchRules {
                priority: i32::MIN,
                ..MatchRules::default()
            },
            ..Self::default()
        };
        pkg.toolbar = default_toolbar(&pkg.behaviour);
        pkg
    }

    /// True for the package returned by [`DoctypePackage::generic`] (and any
    /// other package that was never loaded from disk).
    pub fn is_virtual(&self) -> bool {
        self.base_dir.is_none()
    }

    /// Reads `<dir>/doctype.toml`.
    pub fn load(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let manifest = dir.join(MANIFEST_NAME);
        let text = fs::read_to_string(&manifest).map_err(|e| Error::io(&manifest, e))?;
        let mut pkg: Self = toml::from_str(&text).map_err(|source| Error::TomlParse {
            path: manifest.clone(),
            source: Box::new(source),
        })?;
        pkg.validate()?;
        pkg.base_dir = Some(std::path::absolute(dir).map_err(|e| Error::io(dir, e))?);
        Ok(pkg)
    }

    /// Writes `<dir>/doctype.toml`. When the package was loaded from a
    /// different folder, the files it references are copied across too.
    pub fn save(&self, dir: impl AsRef<Path>) -> Result<()> {
        let dir = dir.as_ref();
        self.validate()?;
        fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        let text = toml::to_string_pretty(self).map_err(|e| Error::TomlWrite(Box::new(e)))?;
        let manifest = dir.join(MANIFEST_NAME);
        fs::write(&manifest, text).map_err(|e| Error::io(&manifest, e))?;

        let Some(base) = &self.base_dir else {
            return Ok(());
        };
        let same = match (fs::canonicalize(base), fs::canonicalize(dir)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if same {
            return Ok(());
        }
        for rel in self.referenced_files() {
            let from = base.join(rel);
            let to = dir.join(rel);
            if !from.is_file() {
                continue;
            }
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            fs::copy(&from, &to).map_err(|e| Error::io(&to, e))?;
        }
        Ok(())
    }

    /// The folder this package was loaded from (absolute).
    pub fn base_dir(&self) -> Option<&Path> {
        self.base_dir.as_deref()
    }

    /// Package-relative paths named in `[files]`.
    pub fn referenced_files(&self) -> impl Iterator<Item = &str> {
        let f = &self.files;
        [&f.schema, &f.style, &f.structure_style, &f.entities]
            .into_iter()
            .filter_map(|p| p.as_deref())
    }

    /// Absolute path of a package-relative file, if the package is on disk.
    pub fn resolve(&self, relative: &str) -> Option<PathBuf> {
        self.base_dir.as_ref().map(|b| b.join(relative))
    }

    /// Absolute paths of every `[files]` entry.
    pub fn resolved_files(&self) -> ResolvedFiles {
        let r = |p: &Option<String>| p.as_deref().and_then(|p| self.resolve(p));
        ResolvedFiles {
            schema: r(&self.files.schema),
            style: r(&self.files.style),
            structure_style: r(&self.files.structure_style),
            entities: r(&self.files.entities),
        }
    }

    /// `[files]` entries that do not exist on disk.
    pub fn missing_files(&self) -> Vec<String> {
        self.referenced_files()
            .filter(|rel| self.resolve(rel).is_some_and(|p| !p.is_file()))
            .map(str::to_string)
            .collect()
    }

    /// The package's own toolbar, or the default one derived from its behaviour.
    pub fn effective_toolbar(&self) -> Vec<ToolbarEntry> {
        if self.toolbar.is_empty() {
            default_toolbar(&self.behaviour)
        } else {
            self.toolbar.clone()
        }
    }

    /// Template for inserting `element`, if the package has one.
    pub fn template(&self, element: &str) -> Option<&Template> {
        self.templates.get(element)
    }

    fn validate(&self) -> Result<()> {
        if self.doctype.name.trim().is_empty() {
            return Err(Error::invalid("[doctype] name must not be empty"));
        }
        for rel in self.referenced_files() {
            let path = Path::new(rel);
            let escapes = path.is_absolute()
                || path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)));
            if escapes {
                return Err(Error::invalid(format!(
                    "[files] path `{rel}` must be relative and stay inside the package"
                )));
            }
        }
        Ok(())
    }
}
