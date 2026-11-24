//! Persistent user settings.

use crate::doc::View;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub recent_files: Vec<PathBuf>,
    /// Folders scanned for doctype packages.
    pub package_dirs: Vec<PathBuf>,
    /// Catalog (catalog.toml or OASIS XML) used to resolve schema URLs.
    pub catalog: Option<PathBuf>,
    /// Folder holding S1000D schemas, one sub-folder per issue.
    pub s1000d_schema_root: Option<PathBuf>,
    /// BREX data module used by "Check BREX".
    pub brex: Option<PathBuf>,
    /// Folder with Hunspell `.aff`/`.dic` files.
    pub dictionary_dir: Option<PathBuf>,
    pub dictionary_lang: String,
    pub spell_check: bool,
    pub zoom: f32,
    pub dark_mode: Option<bool>,
    pub default_view: View,
    pub show_structure: bool,
    pub show_elements: bool,
    pub show_attributes: bool,
    pub show_issues: bool,
    pub auto_validate: bool,
    pub tab_size: usize,
    /// Reopen the documents that were open when Xylem last closed.
    pub restore_session: bool,
    pub last_session: Vec<PathBuf>,
    /// Seconds between autosaves of unsaved work (0 turns it off).
    pub autosave_seconds: u32,
    /// Folder and file pattern last used by Find in files.
    pub find_files_folder: Option<PathBuf>,
    pub find_files_pattern: String,
    /// The folder last shown in the Project tab.
    pub project_dir: Option<PathBuf>,
    /// Name recorded on tracked changes and comments; empty means the
    /// login name.
    pub review_author: String,
    /// Test suite last used by Check stylesheet output.
    pub stylesheet_suite: Option<PathBuf>,
    /// Program that runs stylesheets (Preferences, or the Check stylesheet
    /// window's Engine field); used before the team's.
    pub xslt_host: Option<PathBuf>,
    /// A second engine Check stylesheet output also runs, to report any
    /// test whose output differs between the two engines.
    pub xslt_reference: Option<PathBuf>,
    /// The user's own publication previews, tried before the team's.
    pub preview: Vec<UserPreview>,
    /// The user's own schema locations: a URL (or its start) and the local
    /// file or folder to read instead. Used before the team's.
    pub schema_locations: Vec<(String, PathBuf)>,
}

/// A publication preview set up in Preferences.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct UserPreview {
    pub name: String,
    /// Document element it applies to; any when empty.
    pub root: String,
    /// Glob against the file name of the schema the document points to.
    pub schema_location: String,
    pub stylesheet: Option<PathBuf>,
    pub html: Option<PathBuf>,
    /// Parameters as `name=value`, separated by commas or new lines.
    pub params: String,
}

impl UserPreview {
    /// As a preview configuration, once it names a stylesheet.
    pub fn to_config(&self) -> Option<crate::team::PreviewConfig> {
        let opt = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        Some(crate::team::PreviewConfig {
            name: opt(&self.name),
            root: opt(&self.root),
            schema_location: opt(&self.schema_location),
            file_name: None,
            stylesheet: self.stylesheet.clone()?,
            html: self.html.clone(),
            params: self
                .params
                .split([',', '\n'])
                .filter_map(|pair| {
                    let (k, v) = pair.split_once('=')?;
                    let k = k.trim();
                    (!k.is_empty())
                        .then(|| (k.to_string(), toml::Value::String(v.trim().to_string())))
                })
                .collect(),
        })
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent_files: Vec::new(),
            package_dirs: default_package_dirs(),
            catalog: None,
            s1000d_schema_root: None,
            brex: None,
            dictionary_dir: None,
            dictionary_lang: "en_US".into(),
            spell_check: true,
            zoom: 1.0,
            dark_mode: None,
            default_view: View::TagsOn,
            show_structure: true,
            show_elements: true,
            show_attributes: true,
            show_issues: true,
            auto_validate: true,
            tab_size: 2,
            restore_session: true,
            last_session: Vec::new(),
            autosave_seconds: 60,
            find_files_folder: None,
            find_files_pattern: crate::find_files::DEFAULT_PATTERN.into(),
            project_dir: None,
            review_author: String::new(),
            stylesheet_suite: None,
            xslt_host: None,
            xslt_reference: None,
            preview: Vec::new(),
            schema_locations: Vec::new(),
        }
    }
}

/// `<config dir>/xylem/doctypes`, plus `doctypes` next to the executable.
pub fn default_package_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = config_dir() {
        v.push(d.join("doctypes"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        v.push(dir.join("doctypes"));
    }
    v
}

pub fn config_dir() -> Option<PathBuf> {
    // XYLEM_CONFIG_DIR replaces the whole folder (portable installs, tests).
    if let Some(dir) = std::env::var_os("XYLEM_CONFIG_DIR") {
        return Some(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    base.map(|b| b.join("Xylem"))
}

impl Settings {
    /// The author for tracked changes and comments.
    pub fn author(&self) -> String {
        match self.review_author.trim() {
            "" => crate::review::os_user_name(),
            name => name.to_string(),
        }
    }

    pub fn add_recent(&mut self, p: PathBuf) {
        self.recent_files.retain(|r| r != &p);
        self.recent_files.insert(0, p);
        self.recent_files.truncate(15);
    }
}

#[cfg(test)]
mod user_preview_tests {
    use super::*;

    #[test]
    fn a_user_preview_becomes_a_preview_configuration() {
        let mut p = UserPreview {
            root: " publication ".into(),
            params: "mode=html, pageCount = 0\nempty=".into(),
            ..Default::default()
        };
        assert!(p.to_config().is_none(), "needs a stylesheet");
        p.stylesheet = Some("top.xsl".into());
        let c = p.to_config().unwrap();
        assert_eq!(c.root.as_deref(), Some("publication"));
        assert!(c.name.is_none());
        let params = c.param_strings();
        assert_eq!(params["mode"], "html");
        assert_eq!(params["pageCount"], "0");
        assert_eq!(params["empty"], "");
    }
}
