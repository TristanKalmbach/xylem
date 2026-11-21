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
