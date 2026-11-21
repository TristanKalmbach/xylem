//! Lenient reader for XMetaL `.xdp` project files (an INI-like text format).

use std::collections::BTreeMap;

/// What the importer needs from a project file.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Xdp {
    /// `"Key" = "Value"` pairs; later duplicates win only if the earlier was empty.
    pub settings: BTreeMap<String, String>,
    /// Paths listed in the `Files` section, with `/` separators.
    pub files: Vec<String>,
}

impl Xdp {
    /// Non-empty setting value.
    pub fn setting(&self, key: &str) -> Option<&str> {
        self.settings
            .get(key)
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }
}

/// Decodes a project file (UTF-8 with or without BOM, or UTF-16LE).
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes)
        .trim_start_matches('\u{feff}')
        .to_string()
}

fn unquote(s: &str) -> Option<&str> {
    s.strip_prefix('"')?.strip_suffix('"')
}

fn unescape(s: &str) -> String {
    s.replace("\\\\", "\\")
}

/// Parses leniently: unknown lines are ignored, nothing is an error.
pub fn parse(bytes: &[u8]) -> Xdp {
    let text = decode(bytes);
    let mut xdp = Xdp::default();
    for line in text.lines().map(str::trim) {
        if let Some((key, value)) = line.split_once("\" = \"") {
            let (Some(key), Some(value)) = (key.strip_prefix('"'), value.strip_suffix('"')) else {
                continue;
            };
            let slot = xdp.settings.entry(key.to_string()).or_default();
            if slot.is_empty() {
                *slot = unescape(value);
            }
        } else if let Some(path) = unquote(line) {
            // A bare quoted line opens a file block (`"..\images\a.png"` then `{`).
            if path.contains('.') && !path.contains("\" = \"") {
                xdp.files.push(unescape(path).replace('\\', "/"));
            }
        }
    }
    xdp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_settings_and_files() {
        let text = "\u{feff}\"General\"\n{\n\"RulesFile\" = \"A.xsd\"\n\"CatFile\" = \"\"\n}\n\"Files\"\n{\n    \"..\\\\images\\\\x.png\"\n    {\n    \"Excluded\" = \"F\"\n    }\n}\n";
        let xdp = parse(text.as_bytes());
        assert_eq!(xdp.setting("RulesFile"), Some("A.xsd"));
        assert_eq!(xdp.setting("CatFile"), None);
        assert_eq!(xdp.files, ["../images/x.png"]);
    }

    #[test]
    fn garbage_is_not_an_error() {
        assert_eq!(parse(b"\x00\x01 not a project"), Xdp::default());
    }
}
