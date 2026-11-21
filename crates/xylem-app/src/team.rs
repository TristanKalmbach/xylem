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
//! optional, and a user's own Preferences win over the team values.
//!
//! ```toml
//! package_dirs = ["doctypes"]
//! s1000d_schemas = "schemas/s1000d"
//! catalog = "schemas/catalog.xml"
//! brex = "brex/DMC-ACME-A-00-00-00-00A-022A-D_001-00_EN-US.XML"
//! dictionary_dir = "dictionaries"
//! dictionary_lang = "en_US"
//! update_source = "//fileserver/tools/xylem"
//! ```

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TeamConfig {
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
    /// Where the configuration was read from (filled in by [`load`]).
    #[serde(skip)]
    pub source: Option<PathBuf>,
}

fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("XYLEM_TEAM_CONFIG") {
        v.push(PathBuf::from(p));
    }
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        v.push(dir.join("xylem-team.toml"));
    }
    if let Some(dir) = crate::settings::config_dir() {
        v.push(dir.join("xylem-team.toml"));
    }
    v
}

/// Reads the first team configuration found. Errors are logged and the file
/// is ignored: a broken shared file must not stop anyone from editing.
pub fn load() -> TeamConfig {
    for path in candidates() {
        if !path.is_file() {
            continue;
        }
        match parse(&path) {
            Ok(cfg) => {
                log::info!("team configuration: {}", path.display());
                return cfg;
            }
            Err(e) => log::warn!("ignoring team configuration {}: {e}", path.display()),
        }
    }
    TeamConfig::default()
}

pub fn parse(path: &Path) -> Result<TeamConfig, String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut cfg: TeamConfig = toml::from_str(&text).map_err(|e| e.to_string())?;
    let base = path.parent().unwrap_or(Path::new("."));
    let resolve = |p: &mut PathBuf| {
        if p.is_relative() {
            *p = base.join(&*p);
        }
    };
    cfg.package_dirs.iter_mut().for_each(resolve);
    for p in [&mut cfg.s1000d_schemas, &mut cfg.catalog, &mut cfg.brex, &mut cfg.dictionary_dir, &mut cfg.update_source]
        .into_iter()
        .flatten()
    {
        resolve(p);
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
        assert!(parse(&file).is_err(), "typos are reported, not silently ignored");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
