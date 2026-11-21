//! Autosave and crash recovery.
//!
//! Unsaved documents are written to `<config>/autosave/<key>.xml`, with a
//! small `<key>.path` file naming the original location. A clean save or
//! close removes them; anything left over at startup is offered for recovery.

use std::path::{Path, PathBuf};
use xylem_dom::Document;

fn dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|c| c.join("autosave"))
}

/// Writes an autosave copy. Errors are logged, never shown: autosave must not
/// get in the way of editing.
pub fn write(key: String, original: Option<&Path>, doc: &Document) {
    let Some(dir) = dir() else { return };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("autosave: {e}");
        return;
    }
    // Write to a temporary file and rename it, so a crash mid-write never
    // leaves a truncated copy in place of a good one.
    let xml = dir.join(format!("{key}.xml"));
    let tmp = dir.join(format!("{key}.xml.tmp"));
    if let Err(e) = std::fs::write(&tmp, doc.to_bytes()).and_then(|_| std::fs::rename(&tmp, &xml)) {
        log::warn!("autosave: {e}");
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    let meta = original
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let _ = std::fs::write(dir.join(format!("{key}.path")), meta);
}

/// Removes the autosave copy for `key` (after a save or a deliberate close).
pub fn discard(key: String) {
    if let Some(dir) = dir() {
        let _ = std::fs::remove_file(dir.join(format!("{key}.xml")));
        let _ = std::fs::remove_file(dir.join(format!("{key}.path")));
    }
}

/// An autosaved document found at startup.
pub struct Recovered {
    pub file: PathBuf,
    pub original: Option<PathBuf>,
    pub saved_at: Option<std::time::SystemTime>,
}

impl Recovered {
    pub fn describe(&self) -> String {
        let name = self
            .original
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled document".into());
        let age = self
            .saved_at
            .and_then(|t| t.elapsed().ok())
            .map(|d| match d.as_secs() {
                s if s < 120 => "just now".to_string(),
                s if s < 7200 => format!("{} minutes ago", s / 60),
                s => format!("{} hours ago", s / 3600),
            })
            .unwrap_or_default();
        format!("{name} ({age})")
    }

    pub fn load(&self) -> Result<Document, String> {
        let bytes = std::fs::read(&self.file).map_err(|e| e.to_string())?;
        Document::from_bytes(&bytes).map_err(|e| e.to_string())
    }

    pub fn remove(&self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_file(self.file.with_extension("path"));
    }
}

/// Autosaved documents left behind by a previous session.
pub fn find() -> Vec<Recovered> {
    let Some(dir) = dir() else { return Vec::new() };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for e in entries.flatten() {
        let file = e.path();
        if file.extension().is_none_or(|x| x != "xml") {
            continue;
        }
        let original = std::fs::read_to_string(file.with_extension("path"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let saved_at = e.metadata().and_then(|m| m.modified()).ok();
        out.push(Recovered {
            file,
            original,
            saved_at,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_find_and_discard() {
        // Point the config dir at a scratch folder for this test.
        let tmp = std::env::temp_dir().join(format!("xylem-autosave-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        // SAFETY: the test sets these before anything reads them and runs alone in this module.
        unsafe {
            std::env::set_var("APPDATA", &tmp);
            std::env::set_var("XDG_CONFIG_HOME", &tmp);
            std::env::set_var("HOME", &tmp);
        }
        let doc = Document::from_str("<a>unsaved</a>").unwrap();
        write("k1".into(), Some(Path::new("/docs/manual.xml")), &doc);
        let found = find();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].original.as_deref(),
            Some(Path::new("/docs/manual.xml"))
        );
        assert!(found[0].describe().starts_with("manual.xml"));
        assert_eq!(found[0].load().unwrap().to_xml(), "<a>unsaved</a>");
        discard("k1".into());
        assert!(find().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
