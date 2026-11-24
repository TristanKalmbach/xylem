//! A stylesheet and the modules it includes and imports, read so that any
//! attribute value can be mapped back to the exact characters in its file.
//! Fixes are made by replacing those characters, so everything else in the
//! file stays byte for byte as it was.

use std::collections::{HashSet, VecDeque};
use std::ops::Range;
use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId};

pub const XSLT_NS: &str = "http://www.w3.org/1999/XSL/Transform";

/// One stylesheet file.
pub struct Module {
    pub path: PathBuf,
    /// The file's text (without a byte order mark).
    pub text: String,
    /// The file started with a UTF-8 byte order mark.
    pub bom: bool,
    pub doc: Document,
    /// Byte offset of each node in `text`.
    offsets: Vec<usize>,
    line_starts: Vec<usize>,
    /// Prefix bound to the XSLT namespace (normally `xsl`).
    pub xsl: String,
    /// The text could be edited: UTF-8, and the document reads back as the
    /// same text.
    pub editable: bool,
}

/// An attribute value as written in the file.
pub struct RawValue {
    /// Where the value is in the file (inside the quotes).
    pub raw: Range<usize>,
    pub quote: char,
    /// The value as the XML parser sees it.
    pub value: String,
    /// The value uses an entity defined in the document's DTD (`&NAME;`),
    /// which is left as written: such values are not analysed.
    pub entities: bool,
    /// For each byte of `value` (and one past the end), the matching offset
    /// in the file.
    map: Vec<usize>,
}

impl RawValue {
    /// The file range holding `value[range]`.
    pub fn file_range(&self, range: Range<usize>) -> Range<usize> {
        self.map[range.start]..self.map[range.end]
    }

    /// `text` escaped to go inside this value's quotes.
    pub fn escape(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '"' if self.quote == '"' => out.push_str("&quot;"),
                '\'' if self.quote == '\'' => out.push_str("&apos;"),
                c => out.push(c),
            }
        }
        out
    }
}

impl Module {
    pub fn load(path: &Path) -> Result<Module, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let doc = Document::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let (bom, rest) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(r) => (true, r),
            None => (false, &bytes[..]),
        };
        let utf8 = std::str::from_utf8(rest).ok().map(str::to_string);
        let (serialised, offsets) = doc.to_xml_with_offsets();
        let editable = utf8.as_deref() == Some(serialised.as_str());
        let text = if editable {
            utf8.unwrap_or_default()
        } else {
            serialised
        };
        let mut line_starts = vec![0];
        line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
        let xsl = doc
            .document_element()
            .and_then(|root| {
                doc.element(root)?.attrs.iter().find_map(|a| {
                    (a.value == XSLT_NS)
                        .then(|| a.name.strip_prefix("xmlns:").map(str::to_string))
                        .flatten()
                })
            })
            .unwrap_or_else(|| "xsl".into());
        Ok(Module {
            path: path.to_path_buf(),
            text,
            bom,
            doc,
            offsets,
            line_starts,
            xsl,
            editable,
        })
    }

    /// Whether `el` is the XSLT element `local`.
    pub fn is_xsl(&self, el: NodeId, local: &str) -> bool {
        self.doc
            .name(el)
            .and_then(|n| n.split_once(':'))
            .is_some_and(|(p, l)| p == self.xsl && l == local)
    }

    /// The XSLT local name of `el`, if it is an XSLT element.
    pub fn xsl_name(&self, el: NodeId) -> Option<&str> {
        let (p, l) = self.doc.name(el)?.split_once(':')?;
        (p == self.xsl).then_some(l)
    }

    pub fn offset(&self, n: NodeId) -> usize {
        self.offsets.get(n.index()).copied().unwrap_or(0)
    }

    /// 1-based line of a byte offset.
    pub fn line_of(&self, offset: usize) -> u32 {
        self.line_starts.partition_point(|&s| s <= offset) as u32
    }

    /// Byte offset just past the start tag of the element at `n`.
    pub fn start_tag_end(&self, n: NodeId) -> usize {
        let b = self.text.as_bytes();
        let mut i = self.offset(n);
        let mut quote = None;
        while i < b.len() {
            match (quote, b[i]) {
                (None, b'"' | b'\'') => quote = Some(b[i]),
                (Some(q), c) if c == q => quote = None,
                (None, b'>') => return i + 1,
                _ => {}
            }
            i += 1;
        }
        b.len()
    }

    /// The line Saxon reports for an element: where its start tag ends.
    pub fn saxon_line(&self, n: NodeId) -> u32 {
        self.line_of(self.start_tag_end(n).saturating_sub(1))
    }

    /// The whitespace before `n` on its line, for indenting new siblings.
    pub fn indent_of(&self, n: NodeId) -> String {
        let at = self.offset(n);
        let line_start = self.text[..at].rfind('\n').map_or(0, |i| i + 1);
        let lead = &self.text[line_start..at];
        if lead.chars().all(|c| c == ' ' || c == '\t') {
            lead.to_string()
        } else {
            String::new()
        }
    }

    /// The line ending the file uses.
    pub fn newline(&self) -> &'static str {
        if self.text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        }
    }

    /// Where attribute `name` of element `n` is in the file.
    pub fn raw_attr(&self, n: NodeId, name: &str) -> Option<RawValue> {
        let start = self.offset(n);
        let end = self.start_tag_end(n);
        let tag = &self.text[start..end];
        let b = tag.as_bytes();
        // Skip "<name".
        let mut i = 1;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        loop {
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= b.len() || b[i] == b'>' || b[i] == b'/' {
                return None;
            }
            let name_start = i;
            while i < b.len() && b[i] != b'=' && !b[i].is_ascii_whitespace() {
                i += 1;
            }
            let attr = &tag[name_start..i];
            while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'=') {
                i += 1;
            }
            let quote = *b.get(i)? as char;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let value_start = i + 1;
            let value_end = value_start + tag[value_start..].find(quote)?;
            if attr == name {
                return Some(decode_value(
                    &self.text,
                    start + value_start..start + value_end,
                    quote,
                ));
            }
            i = value_end + 1;
        }
    }
}

/// Decodes an attribute value as the XML parser does (entities, character
/// references, whitespace characters as spaces), keeping the position map.
fn decode_value(text: &str, raw: Range<usize>, quote: char) -> RawValue {
    let src = &text[raw.clone()];
    let mut value = String::new();
    let mut map = Vec::new();
    let mut entities = false;
    let mut i = 0;
    let b = src.as_bytes();
    let push = |value: &mut String, map: &mut Vec<usize>, s: &str, at: usize| {
        for _ in 0..s.len() {
            map.push(raw.start + at);
        }
        value.push_str(s);
    };
    while i < src.len() {
        if b[i] == b'&'
            && let Some(semi) = src[i..].find(';')
        {
            let entity = &src[i + 1..i + semi];
            let decoded = match entity {
                "lt" => Some("<".to_string()),
                "gt" => Some(">".to_string()),
                "amp" => Some("&".to_string()),
                "quot" => Some("\"".to_string()),
                "apos" => Some("'".to_string()),
                e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from),
                e if e.starts_with('#') => e[1..]
                    .parse()
                    .ok()
                    .and_then(char::from_u32)
                    .map(String::from),
                _ => None,
            };
            if let Some(d) = decoded {
                push(&mut value, &mut map, &d, i);
                i += semi + 1;
                continue;
            }
            entities = true;
        }
        if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
            push(&mut value, &mut map, " ", i);
            i += 2;
            continue;
        }
        let c = src[i..].chars().next().unwrap_or(' ');
        let s = if matches!(c, '\n' | '\r' | '\t') {
            " ".to_string()
        } else {
            c.to_string()
        };
        push(&mut value, &mut map, &s, i);
        i += c.len_utf8();
    }
    map.push(raw.end);
    RawValue {
        raw,
        quote,
        value,
        entities,
        map,
    }
}

/// A stylesheet and every module it includes or imports.
pub struct Stylesheet {
    pub top: PathBuf,
    pub modules: Vec<Module>,
    /// Problems reading included or imported modules.
    pub warnings: Vec<String>,
}

/// `a/b/../c` as `a/c`, without touching the file system.
pub fn tidy(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Comparable form of a path (case-insensitive on Windows).
pub fn path_key(p: &Path) -> String {
    let s = p
        .canonicalize()
        .unwrap_or_else(|_| p.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('\\', "/");
    if cfg!(windows) { s.to_lowercase() } else { s }
}

impl Stylesheet {
    pub fn load(top: &Path) -> Result<Stylesheet, String> {
        let mut modules: Vec<Module> = Vec::new();
        let mut warnings = Vec::new();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::from([tidy(top)]);
        while let Some(path) = queue.pop_front() {
            if !seen.insert(path_key(&path)) {
                continue;
            }
            let module = match Module::load(&path) {
                Ok(m) => m,
                Err(e) if modules.is_empty() => return Err(e),
                Err(e) => {
                    warnings.push(e);
                    continue;
                }
            };
            let dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
            if let Some(root) = module.doc.document_element() {
                for c in module.doc.element_children(root) {
                    if (module.is_xsl(c, "include") || module.is_xsl(c, "import"))
                        && let Some(href) = module.doc.attr(c, "href")
                    {
                        queue.push_back(tidy(&dir.join(href.replace("%20", " "))));
                    }
                }
            }
            modules.push(module);
        }
        Ok(Stylesheet {
            top: top.to_path_buf(),
            modules,
            warnings,
        })
    }

    /// Paths of every module, comparable with [`path_key`].
    pub fn module_keys(&self) -> HashSet<String> {
        self.modules.iter().map(|m| path_key(&m.path)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_values_map_back_to_the_file() {
        let text = "<x a='1' select=\"$a/b[@c = &quot;q&quot;]\r\n   [1]\" z='&lt;'/>";
        let dir = std::env::temp_dir().join("xylem-xslt-stylesheet-test");
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.xsl");
        std::fs::write(&f, text).unwrap();
        let m = Module::load(&f).unwrap();
        assert!(m.editable);
        let root = m.doc.document_element().unwrap();
        let v = m.raw_attr(root, "select").unwrap();
        assert_eq!(v.value, "$a/b[@c = \"q\"]    [1]");
        let q = v.value.find("\"q\"").unwrap();
        assert_eq!(&text[v.file_range(q..q + 3)], "&quot;q&quot;");
        assert_eq!(&text[v.file_range(0..2)], "$a");
        assert_eq!(v.escape("a<\"b\"&'"), "a&lt;&quot;b&quot;&amp;'");
        let z = m.raw_attr(root, "z").unwrap();
        assert_eq!((z.value.as_str(), z.quote), ("<", '\''));
        assert!(m.raw_attr(root, "missing").is_none());
        assert_eq!(m.line_of(text.find("[1]").unwrap()), 2);
    }

    #[test]
    fn paths_are_tidied() {
        assert_eq!(
            tidy(Path::new("C:/IA/Service/../default.xsl")),
            PathBuf::from("C:/IA/default.xsl")
        );
        assert_eq!(tidy(Path::new("a/./b/../../c")), PathBuf::from("c"));
        assert_eq!(tidy(Path::new("../x")), PathBuf::from("../x"));
    }
}
