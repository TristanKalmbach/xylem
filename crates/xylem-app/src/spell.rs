//! Spell checking with Hunspell dictionaries (pure-Rust `spellbook`).

use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId};

pub struct Speller {
    dict: spellbook::Dictionary,
    user_words: HashSet<String>,
    user_file: Option<PathBuf>,
}

/// Folders commonly holding Hunspell dictionaries.
fn candidate_dirs(explicit: Option<&Path>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = explicit.map(Path::to_path_buf).into_iter().collect();
    if let Some(c) = crate::settings::config_dir() {
        v.push(c.join("dictionaries"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(d) = exe.parent()
    {
        v.push(d.join("dictionaries"));
    }
    for p in [
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/Library/Spelling",
        "C:\\Program Files\\LibreOffice\\share\\extensions\\dict-en",
        "C:\\Program Files (x86)\\LibreOffice\\share\\extensions\\dict-en",
        "/Applications/LibreOffice.app/Contents/Resources/extensions/dict-en",
        "/usr/lib/libreoffice/share/extensions/dict-en",
    ] {
        v.push(PathBuf::from(p));
    }
    if let Some(h) = std::env::var_os("HOME") {
        v.push(PathBuf::from(h).join("Library/Spelling"));
    }
    v
}

impl Speller {
    pub fn load(dir: Option<&Path>, lang: &str) -> Option<Self> {
        for d in candidate_dirs(dir) {
            let aff = d.join(format!("{lang}.aff"));
            let dic = d.join(format!("{lang}.dic"));
            if aff.is_file() && dic.is_file() {
                let aff_s = read_lossy(&aff)?;
                let dic_s = read_lossy(&dic)?;
                match spellbook::Dictionary::new(&aff_s, &dic_s) {
                    Ok(dict) => {
                        let user_file =
                            crate::settings::config_dir().map(|c| c.join("user-dictionary.txt"));
                        let mut s = Self {
                            dict,
                            user_words: HashSet::new(),
                            user_file,
                        };
                        if let Some(f) = &s.user_file
                            && let Ok(t) = std::fs::read_to_string(f)
                        {
                            s.user_words.extend(t.lines().map(str::to_string));
                        }
                        log::info!("Spell checking with {}", dic.display());
                        return Some(s);
                    }
                    Err(e) => log::warn!("Dictionary {} failed to load: {e}", dic.display()),
                }
            }
        }
        None
    }

    pub fn is_correct(&self, w: &str) -> bool {
        if self.user_words.contains(w) || w.chars().any(|c| c.is_ascii_digit()) || w.len() < 2 {
            return true;
        }
        // All-caps words are usually acronyms or codes.
        if w.chars().all(|c| !c.is_lowercase()) {
            return true;
        }
        self.dict.check(w)
    }

    pub fn suggest(&self, w: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.dict.suggest(w, &mut out);
        out
    }

    pub fn add_word(&mut self, w: &str) {
        self.user_words.insert(w.to_string());
        if let Some(f) = &self.user_file {
            if let Some(p) = f.parent() {
                let _ = std::fs::create_dir_all(p);
            }
            let mut words: Vec<_> = self.user_words.iter().cloned().collect();
            words.sort();
            let _ = std::fs::write(f, words.join("\n"));
        }
    }

    /// Misspelled words in all text nodes (skips code-like elements). Results
    /// are cached per text node and recomputed only when its text changes.
    pub fn check_document(
        &self,
        doc: &Document,
        cache: &mut SpellCache,
    ) -> Vec<(NodeId, Range<usize>)> {
        use std::hash::{Hash, Hasher};
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for n in doc.descendants(doc.root()) {
            let Some(t) = doc.text(n) else { continue };
            let skip = doc.parent(n).and_then(|p| doc.name(p)).is_some_and(|name| {
                matches!(
                    xylem_dom::local_name(name),
                    "code"
                        | "codeph"
                        | "verbatimText"
                        | "pre"
                        | "programlisting"
                        | "dmCode"
                        | "partNumber"
                        | "identNumber"
                )
            });
            if skip {
                continue;
            }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.hash(&mut h);
            let key = h.finish();
            seen.insert(n);
            let entry = cache.entry(n).or_insert((key.wrapping_add(1), Vec::new()));
            if entry.0 != key {
                *entry = (key, self.check_text(t));
            }
            out.extend(entry.1.iter().map(|r| (n, r.clone())));
            if out.len() > 5000 {
                break;
            }
        }
        cache.retain(|n, _| seen.contains(n));
        out
    }

    fn check_text(&self, t: &str) -> Vec<Range<usize>> {
        let mut out = Vec::new();
        let mut start = None;
        for (i, c) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
            let word_char = c.is_alphabetic() || c == '\'';
            match (start, word_char) {
                (None, true) => start = Some(i),
                (Some(s), false) => {
                    let w = t[s..i].trim_matches('\'');
                    if !w.is_empty() && !self.is_correct(w) {
                        let off = s + t[s..i].find(w).unwrap_or(0);
                        out.push(off..off + w.len());
                    }
                    start = None;
                }
                _ => {}
            }
        }
        out
    }
}

/// Per-text-node spelling results keyed by a hash of the node's text.
pub type SpellCache = std::collections::HashMap<NodeId, (u64, Vec<Range<usize>>)>;

fn read_lossy(p: &Path) -> Option<String> {
    let b = std::fs::read(p).ok()?;
    Some(match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => e.into_bytes().iter().map(|&b| b as char).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speller() -> Speller {
        let aff = "SET UTF-8\nTRY esianrtolcdugmphbyfvkwz\n";
        let dic = "3\nhello\nworld\nhydraulic\n";
        Speller {
            dict: spellbook::Dictionary::new(aff, dic).unwrap(),
            user_words: HashSet::new(),
            user_file: None,
        }
    }

    #[test]
    fn flags_misspellings_and_caches_per_node() {
        let sp = speller();
        let doc =
            Document::from_str("<doc><p>hello wrold</p><p>hydraulic ABC 42x</p></doc>").unwrap();
        let mut cache = SpellCache::new();
        let found = sp.check_document(&doc, &mut cache);
        assert_eq!(found.len(), 1);
        let (n, r) = &found[0];
        assert_eq!(&doc.text(*n).unwrap()[r.clone()], "wrold");
        assert!(sp.suggest("wrold").iter().any(|s| s == "world"));
        assert_eq!(cache.len(), 2);
        // Unchanged text reuses cached results.
        assert_eq!(sp.check_document(&doc, &mut cache), found);
    }
}
