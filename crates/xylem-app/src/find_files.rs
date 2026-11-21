//! Find and replace across the XML files of a folder.
//!
//! This module knows nothing about the UI: it walks folders, parses files
//! with the lossless DOM, reports matches with line numbers and context, and
//! rewrites files in place. The single-document find (`find.rs`) uses the
//! same matcher, so "the third match in this file" means the same thing in
//! both places.
//!
//! Matching is done on decoded character data (text and CDATA nodes) and,
//! optionally, on attribute values; tags, comments and processing
//! instructions are never matched. Replacing goes through the DOM, so a
//! rewritten file differs from the original only inside the replaced spans
//! (and, for attributes, the start tags that carried them).

use regex::{Regex, RegexBuilder};
use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use xylem_dom::{Document, NodeId};

/// The file pattern offered when nothing else has been chosen.
pub const DEFAULT_PATTERN: &str = "*.xml;*.XML;*.sgm";

/// What to look for. Shared by the single-document find and find in files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub text: String,
    pub match_case: bool,
    pub whole_word: bool,
    pub regex: bool,
    /// Also match attribute values (find in files only).
    pub attributes: bool,
}

impl SearchOptions {
    /// The compiled matcher. Plain text is escaped, so only regex mode
    /// treats metacharacters specially.
    pub fn build(&self) -> Result<Regex, regex::Error> {
        let pattern = if self.regex {
            self.text.clone()
        } else {
            regex::escape(&self.text)
        };
        let pattern = if self.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        RegexBuilder::new(&pattern)
            .case_insensitive(!self.match_case)
            .build()
    }

    /// The text that replaces one match. In regex mode `$1`, `${name}` and
    /// `$$` are expanded from the match's groups; otherwise the replacement
    /// is used exactly as typed, dollar signs included.
    pub fn expand(
        &self,
        re: &Regex,
        haystack: &str,
        at: Range<usize>,
        replacement: &str,
    ) -> String {
        if !self.regex {
            return replacement.to_string();
        }
        // Re-run the regex at the match start so look-behind style anchors
        // such as \b see the same surroundings as the original search.
        match re.captures_at(haystack, at.start) {
            Some(c) if c.get(0).map(|m| m.range()) == Some(at) => {
                let mut out = String::new();
                c.expand(replacement, &mut out);
                out
            }
            _ => replacement.to_string(),
        }
    }
}

/// Non-empty matches of `re` in a string, in order.
fn find_in(re: &Regex, s: &str) -> impl Iterator<Item = Range<usize>> {
    re.find_iter(s).filter(|m| !m.is_empty()).map(|m| m.range())
}

/// Every text match in document order, restricted to text nodes `keep`
/// accepts. This is the order "match n" refers to everywhere.
pub fn text_matches(
    doc: &Document,
    re: &Regex,
    keep: impl Fn(NodeId) -> bool,
) -> Vec<(NodeId, Range<usize>)> {
    let mut out = Vec::new();
    for n in doc.descendants(doc.root()) {
        let Some(t) = doc.text(n) else { continue };
        // Text outside the document element is only whitespace.
        if doc.parent(n) == Some(doc.root()) || !keep(n) {
            continue;
        }
        out.extend(find_in(re, t).map(|r| (n, r)));
    }
    out
}

/// Where a match is, in a form that survives reopening the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The `nth` text match in the document (see [`text_matches`]).
    Text { nth: usize },
    /// A match in attribute `name` of the `element`-th element in document
    /// order.
    Attribute { element: usize, name: String },
}

#[derive(Clone, Debug)]
pub struct Hit {
    /// 1-based line in the file.
    pub line: usize,
    pub place: Place,
    /// The text around the match on one line, whitespace collapsed.
    pub context: String,
    /// The match within `context`.
    pub highlight: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct FileHits {
    pub path: PathBuf,
    pub hits: Vec<Hit>,
}

/// Line lookup for byte offsets in the serialised document.
struct Lines(Vec<usize>);

impl Lines {
    fn new(xml: &str) -> Self {
        let mut starts = vec![0];
        starts.extend(xml.match_indices('\n').map(|(i, _)| i + 1));
        Self(starts)
    }
    fn line_of(&self, offset: usize) -> usize {
        self.0.partition_point(|&s| s <= offset).max(1)
    }
}

/// Finds every match in a document.
pub fn search_document(doc: &Document, re: &Regex, attributes: bool) -> Vec<Hit> {
    let (xml, offsets) = doc.to_xml_with_offsets();
    let lines = Lines::new(&xml);
    let mut hits = Vec::new();
    for (nth, (node, range)) in text_matches(doc, re, |_| true).into_iter().enumerate() {
        let text = doc.text(node).unwrap_or("");
        // Character references never stand for line breaks in practice, so
        // counting the decoded text's newlines gives the source line.
        let base = lines.line_of(offsets[node.index()]);
        let line = base + text[..range.start].matches('\n').count();
        let (context, highlight) = snippet(text, range);
        hits.push(Hit {
            line,
            place: Place::Text { nth },
            context,
            highlight,
        });
    }
    if attributes {
        let elements = doc.descendants(doc.root()).filter(|&n| doc.is_element(n));
        for (index, el) in elements.enumerate() {
            let Some(e) = doc.element(el) else { continue };
            for a in &e.attrs {
                for range in find_in(re, &a.value) {
                    let (value, mut highlight) = snippet(&a.value, range);
                    let prefix = format!("{}=\"", a.name);
                    highlight = highlight.start + prefix.len()..highlight.end + prefix.len();
                    hits.push(Hit {
                        line: lines.line_of(offsets[el.index()]),
                        place: Place::Attribute {
                            element: index,
                            name: a.name.clone(),
                        },
                        context: format!("<{} {prefix}{value}\">", e.name),
                        highlight: highlight.start + e.name.len() + 2
                            ..highlight.end + e.name.len() + 2,
                    });
                }
            }
        }
        hits.sort_by_key(|h| h.line);
    }
    hits
}

/// A one-line excerpt around `range` of `text`, with the match's position in
/// the excerpt.
fn snippet(text: &str, range: Range<usize>) -> (String, Range<usize>) {
    const BEFORE: usize = 40;
    const AFTER: usize = 60;
    let collapse = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let keep_edge_space = |orig: &str, collapsed: String, lead: bool| {
        // Collapsing drops the space between the context and the match.
        let has = if lead {
            orig.starts_with(char::is_whitespace)
        } else {
            orig.ends_with(char::is_whitespace)
        };
        match (has && !collapsed.is_empty(), lead) {
            (true, true) => format!(" {collapsed}"),
            (true, false) => format!("{collapsed} "),
            _ => collapsed,
        }
    };
    let before_full = &text[..range.start];
    let after_full = &text[range.end..];
    let before_chars: Vec<char> = before_full.chars().collect();
    let cut_before = before_chars.len() > BEFORE;
    let before: String = before_chars[before_chars.len().saturating_sub(BEFORE)..]
        .iter()
        .collect();
    let after_chars: Vec<char> = after_full.chars().collect();
    let cut_after = after_chars.len() > AFTER;
    let after: String = after_chars[..after_chars.len().min(AFTER)].iter().collect();
    let mut out = String::new();
    if cut_before {
        out.push('…');
    }
    let b = collapse(&before);
    out.push_str(&keep_edge_space(&before, b, false));
    let start = out.len();
    out.push_str(&collapse(&text[range]));
    let end = out.len();
    out.push_str(&keep_edge_space(&after, collapse(&after), true));
    if cut_after {
        out.push('…');
    }
    // Trim the ends, keeping the highlight inside the result: a match made
    // only of whitespace at the end of the text trims away entirely.
    let shift = out.len() - out.trim_start().len();
    let out = out.trim().to_string();
    let clamp = |i: usize| i.saturating_sub(shift).min(out.len());
    let highlight = clamp(start)..clamp(end);
    (out, highlight)
}

/// Replaces every match in a document and returns how many were replaced.
pub fn replace_in_document(
    doc: &mut Document,
    opts: &SearchOptions,
    re: &Regex,
    replacement: &str,
) -> usize {
    let mut count = 0;
    let texts: Vec<NodeId> = doc
        .descendants(doc.root())
        .filter(|&n| doc.text(n).is_some() && doc.parent(n) != Some(doc.root()))
        .collect();
    for node in texts {
        let text = doc.text(node).unwrap_or("").to_string();
        let edits: Vec<(Range<usize>, String)> = find_in(re, &text)
            .map(|r| {
                let with = opts.expand(re, &text, r.clone(), replacement);
                (r, with)
            })
            .collect();
        // From the end backwards, so earlier ranges stay valid.
        for (range, with) in edits.iter().rev() {
            doc.replace_text(node, range.clone(), with);
        }
        count += edits.len();
    }
    if opts.attributes {
        let elements: Vec<NodeId> = doc
            .descendants(doc.root())
            .filter(|&n| doc.is_element(n))
            .collect();
        for el in elements {
            let attrs: Vec<(String, String)> = doc
                .element(el)
                .map(|e| {
                    e.attrs
                        .iter()
                        .map(|a| (a.name.clone(), a.value.clone()))
                        .collect()
                })
                .unwrap_or_default();
            for (name, value) in attrs {
                let mut new = String::with_capacity(value.len());
                let mut last = 0;
                let mut n = 0;
                for r in find_in(re, &value) {
                    new.push_str(&value[last..r.start]);
                    new.push_str(&opts.expand(re, &value, r.clone(), replacement));
                    last = r.end;
                    n += 1;
                }
                if n > 0 {
                    new.push_str(&value[last..]);
                    doc.set_attr(el, &name, new);
                    count += n;
                }
            }
        }
    }
    count
}

/// File name patterns such as `*.xml;*.sgm`, matched without regard to case.
#[derive(Clone, Debug)]
pub struct FilePattern(Vec<Vec<char>>);

impl FilePattern {
    pub fn new(spec: &str) -> Self {
        let parts: Vec<Vec<char>> = spec
            .split([';', ','])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase().chars().collect())
            .collect();
        if parts.is_empty() {
            return Self(vec![vec!['*']]);
        }
        Self(parts)
    }

    pub fn matches(&self, name: &str) -> bool {
        let name: Vec<char> = name.to_lowercase().chars().collect();
        self.0.iter().any(|p| glob(p, &name))
    }
}

/// `*` and `?` wildcard matching.
fn glob(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Files under `root` whose names match, in a stable order. Hidden folders
/// (`.git` and friends) are skipped, as are Xylem's own temporary files.
pub fn list_files(
    root: &Path,
    pattern: &FilePattern,
    subfolders: bool,
    cancel: &AtomicBool,
) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        let mut sub = Vec::new();
        for e in entries {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_dir() {
                if subfolders && !name.starts_with('.') {
                    sub.push(e.path());
                }
            } else if !name.ends_with(".xylem-tmp") && pattern.matches(&name) {
                out.push(e.path());
            }
        }
        // Depth first, in name order.
        dirs.extend(sub.into_iter().rev());
    }
    out
}

/// What a search reports while it runs.
#[derive(Debug)]
pub enum SearchEvent {
    /// A file with at least one match.
    File(FileHits),
    /// A file that matched the pattern but could not be read or parsed.
    Skipped(PathBuf, String),
    /// Files looked at so far.
    Progress(usize),
    Done {
        files: usize,
        cancelled: bool,
    },
}

/// A folder search: where, which files, what.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSpec {
    pub root: PathBuf,
    pub pattern: String,
    pub subfolders: bool,
    pub options: SearchOptions,
}

/// Searches a folder on a worker thread. Results arrive on the returned
/// channel; `notify` runs after each event so the UI can repaint. Setting
/// `cancel` stops the search after the current file.
pub fn spawn_search(
    spec: SearchSpec,
    cancel: Arc<AtomicBool>,
    notify: Box<dyn Fn() + Send>,
) -> Result<Receiver<SearchEvent>, regex::Error> {
    let re = spec.options.build()?;
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let send = |e: SearchEvent| {
            let ok = tx.send(e).is_ok();
            notify();
            ok
        };
        let files = list_files(
            &spec.root,
            &FilePattern::new(&spec.pattern),
            spec.subfolders,
            &cancel,
        );
        let mut seen = 0;
        for path in files {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            seen += 1;
            let event = match read_document(&path) {
                Ok(doc) => {
                    let hits = search_document(&doc, &re, spec.options.attributes);
                    (!hits.is_empty()).then_some(SearchEvent::File(FileHits { path, hits }))
                }
                Err(e) => Some(SearchEvent::Skipped(path, e)),
            };
            if let Some(e) = event
                && !send(e)
            {
                return;
            }
            if seen % 20 == 0 && !send(SearchEvent::Progress(seen)) {
                return;
            }
        }
        send(SearchEvent::Done {
            files: seen,
            cancelled: cancel.load(Ordering::Relaxed),
        });
    });
    Ok(rx)
}

fn read_document(path: &Path) -> Result<Document, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    Document::from_bytes(&bytes).map_err(|e| format!("not well-formed: {e}"))
}

/// The outcome of a replace across files.
#[derive(Clone, Debug, Default)]
pub struct ReplaceReport {
    /// Files written, with the number of replacements in each.
    pub changed: Vec<(PathBuf, usize)>,
    /// Files left alone because they are open with unsaved changes.
    pub skipped_unsaved: Vec<PathBuf>,
    /// Files that could not be read, parsed or written.
    pub failed: Vec<(PathBuf, String)>,
}

impl ReplaceReport {
    pub fn replacements(&self) -> usize {
        self.changed.iter().map(|c| c.1).sum()
    }
}

/// A key for comparing paths that tolerates the different spellings one
/// file can have (relative parts, and letter case on Windows).
pub fn path_key(p: &Path) -> PathBuf {
    let p = std::fs::canonicalize(p)
        .or_else(|_| std::path::absolute(p))
        .unwrap_or_else(|_| p.to_path_buf());
    if cfg!(windows) {
        PathBuf::from(p.to_string_lossy().to_lowercase())
    } else {
        p
    }
}

/// Replaces every match in `files`, skipping those in `skip` (compared with
/// [`path_key`]). Each file is parsed afresh, so the counts are what was
/// actually replaced, and written through a temporary file so a failure
/// never leaves a half-written document behind. Files without a match are
/// not touched at all.
pub fn replace_in_files(
    files: &[PathBuf],
    opts: &SearchOptions,
    replacement: &str,
    skip: &HashSet<PathBuf>,
) -> Result<ReplaceReport, regex::Error> {
    let re = opts.build()?;
    let mut report = ReplaceReport::default();
    for path in files {
        if skip.contains(&path_key(path)) {
            report.skipped_unsaved.push(path.clone());
            continue;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                report.failed.push((path.clone(), e.to_string()));
                continue;
            }
        };
        let mut doc = match Document::from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                report
                    .failed
                    .push((path.clone(), format!("not well-formed: {e}")));
                continue;
            }
        };
        // Only rewrite files the DOM reproduces exactly (an invalid byte in
        // a UTF-8 file, say, would otherwise be replaced as a side effect).
        if doc.to_bytes() != bytes {
            report.failed.push((
                path.clone(),
                "left unchanged: the file would not be written back exactly as read (check its encoding)"
                    .into(),
            ));
            continue;
        }
        let n = replace_in_document(&mut doc, opts, &re, replacement);
        if n == 0 {
            continue;
        }
        match write_atomically(path, &doc.to_bytes()) {
            Ok(()) => report.changed.push((path.clone(), n)),
            Err(e) => report.failed.push((path.clone(), e.to_string())),
        }
    }
    Ok(report)
}

fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("xylem-tmp");
    std::fs::write(&tmp, bytes)
        .and_then(|_| std::fs::rename(&tmp, path))
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
}

/// Runs [`replace_in_files`] on a worker thread.
pub fn spawn_replace(
    files: Vec<PathBuf>,
    opts: SearchOptions,
    replacement: String,
    skip: HashSet<PathBuf>,
    notify: Box<dyn Fn() + Send>,
) -> Receiver<Result<ReplaceReport, String>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let r = replace_in_files(&files, &opts, &replacement, &skip).map_err(|e| e.to_string());
        let _ = tx.send(r);
        notify();
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(text: &str) -> SearchOptions {
        SearchOptions {
            text: text.into(),
            ..Default::default()
        }
    }

    const DOC: &str = "<?xml version=\"1.0\"?>\n<!-- pump notes -->\n<doc kind=\"pump\">\n  <title>Pump &amp; motor</title>\n  <p>Check the pump.\nThe <b>Pump</b> must be primed.</p>\n  <?note pump?>\n</doc>\n";

    #[test]
    fn finds_text_but_not_tags_comments_or_pis() {
        let doc = Document::from_str(DOC).unwrap();
        let re = opts("pump").build().unwrap();
        let hits = search_document(&doc, &re, false);
        let lines: Vec<usize> = hits.iter().map(|h| h.line).collect();
        assert_eq!(lines, vec![4, 5, 6]);
        assert_eq!(hits[0].context, "Pump & motor");
        assert_eq!(&hits[0].context[hits[0].highlight.clone()], "Pump");
        assert_eq!(hits[1].context, "Check the pump. The");
        assert_eq!(&hits[1].context[hits[1].highlight.clone()], "pump");
        assert_eq!(hits[2].place, Place::Text { nth: 2 });
    }

    #[test]
    fn attribute_values_are_searched_on_request() {
        let doc = Document::from_str(DOC).unwrap();
        let mut o = opts("pump");
        o.attributes = true;
        let hits = search_document(&doc, &o.build().unwrap(), true);
        assert_eq!(hits.len(), 4);
        let attr = hits
            .iter()
            .find(|h| matches!(h.place, Place::Attribute { .. }))
            .unwrap();
        assert_eq!(attr.line, 3);
        assert_eq!(attr.context, "<doc kind=\"pump\">");
        assert_eq!(&attr.context[attr.highlight.clone()], "pump");
        assert_eq!(
            attr.place,
            Place::Attribute {
                element: 0,
                name: "kind".into()
            }
        );
    }

    #[test]
    fn options_change_what_matches() {
        let doc = Document::from_str(DOC).unwrap();
        let count = |o: &SearchOptions| search_document(&doc, &o.build().unwrap(), false).len();
        let mut o = opts("Pump");
        o.match_case = true;
        assert_eq!(count(&o), 2);
        let mut o = opts("pum");
        assert_eq!(count(&o), 3);
        o.whole_word = true;
        assert_eq!(count(&o), 0);
        let mut o = opts(r"p\w+d");
        o.regex = true;
        assert_eq!(count(&o), 1, "primed");
        assert!(opts("a.b").build().unwrap().is_match("a.b"));
        assert!(!opts("a.b").build().unwrap().is_match("axb"));
    }

    #[test]
    fn replace_changes_only_the_matched_text() {
        let mut doc = Document::from_str(DOC).unwrap();
        let o = opts("pump");
        let n = replace_in_document(&mut doc, &o, &o.build().unwrap(), "valve");
        assert_eq!(n, 3);
        assert_eq!(
            doc.to_xml(),
            DOC.replacen("Pump &amp;", "valve &amp;", 1)
                .replacen("the pump.", "the valve.", 1)
                .replacen("<b>Pump</b>", "<b>valve</b>", 1)
        );
    }

    #[test]
    fn literal_replacement_keeps_dollar_signs_and_regex_expands_groups() {
        let src = "<r><p>cost 5 units</p></r>";
        let mut doc = Document::from_str(src).unwrap();
        let o = opts("5");
        replace_in_document(&mut doc, &o, &o.build().unwrap(), "$1");
        assert_eq!(doc.to_xml(), "<r><p>cost $1 units</p></r>");

        let mut doc = Document::from_str(src).unwrap();
        let mut o = opts(r"(\d+) (\w+)");
        o.regex = true;
        replace_in_document(&mut doc, &o, &o.build().unwrap(), "$2: $1");
        assert_eq!(doc.to_xml(), "<r><p>cost units: 5</p></r>");
    }

    #[test]
    fn replace_in_attributes_only_when_asked() {
        let mut doc = Document::from_str(DOC).unwrap();
        let mut o = opts("pump");
        o.match_case = true;
        replace_in_document(&mut doc, &o, &o.build().unwrap(), "valve");
        assert!(doc.to_xml().contains("kind=\"pump\""));
        o.attributes = true;
        replace_in_document(&mut doc, &o, &o.build().unwrap(), "valve");
        assert!(doc.to_xml().contains("<doc kind=\"valve\">"));
        assert!(doc.to_xml().contains("<!-- pump notes -->"));
        assert!(doc.to_xml().contains("<?note pump?>"));
    }

    #[test]
    fn file_patterns() {
        let p = FilePattern::new(DEFAULT_PATTERN);
        assert!(p.matches("a.xml"));
        assert!(p.matches("B.XML"));
        assert!(p.matches("c.Sgm"));
        assert!(!p.matches("a.xml.bak"));
        assert!(!p.matches("notes.txt"));
        let p = FilePattern::new("DMC-*_??.xml");
        assert!(p.matches("DMC-ABC_01.xml"));
        assert!(!p.matches("DMC-ABC_1.xml"));
        assert!(FilePattern::new("").matches("anything"));
    }

    /// A scratch folder that is removed afterwards.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let p = std::env::temp_dir().join(format!("xylem-ff-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn folder_search_and_replace() {
        let dir = TempDir::new("replace");
        let a = dir.0.join("a.xml");
        let sub = dir.0.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let b = sub.join("b.XML");
        let c = dir.0.join("c.xml");
        let broken = dir.0.join("broken.sgm");
        let ignored = dir.0.join("notes.txt");
        let hidden = dir.0.join(".git");
        std::fs::create_dir_all(&hidden).unwrap();
        // CRLF line ends and a character reference must survive untouched.
        let a_src = "<r>\r\n  <p>old &#169; text</p>\r\n</r>\r\n";
        std::fs::write(&a, a_src).unwrap();
        std::fs::write(&b, "<r><p>old and old</p></r>").unwrap();
        let c_src = "<r a='old'><p>nothing here</p></r>";
        std::fs::write(&c, c_src).unwrap();
        std::fs::write(&broken, "<r><p>old</r>").unwrap();
        std::fs::write(&ignored, "old").unwrap();
        std::fs::write(hidden.join("x.xml"), "<r>old</r>").unwrap();

        let spec = SearchSpec {
            root: dir.0.clone(),
            pattern: DEFAULT_PATTERN.into(),
            subfolders: true,
            options: opts("old"),
        };
        let rx = spawn_search(
            spec.clone(),
            Arc::new(AtomicBool::new(false)),
            Box::new(|| {}),
        )
        .unwrap();
        let mut files = Vec::new();
        let mut skipped = Vec::new();
        let mut total = None;
        for e in rx {
            match e {
                SearchEvent::File(f) => files.push(f),
                SearchEvent::Skipped(p, _) => skipped.push(p),
                SearchEvent::Progress(_) => {}
                SearchEvent::Done { files, cancelled } => {
                    assert!(!cancelled);
                    total = Some(files);
                }
            }
        }
        assert_eq!(total, Some(4), "a, broken, c and sub/b");
        assert_eq!(skipped, vec![broken.clone()]);
        let found: Vec<(&Path, usize)> = files
            .iter()
            .map(|f| (f.path.as_path(), f.hits.len()))
            .collect();
        assert_eq!(found, vec![(a.as_path(), 1), (b.as_path(), 2)]);
        assert_eq!(files[0].hits[0].line, 2);

        // b is "open with unsaved changes".
        let skip: HashSet<PathBuf> = [path_key(&b)].into_iter().collect();
        let paths: Vec<PathBuf> = files
            .iter()
            .map(|f| f.path.clone())
            .chain([c.clone()])
            .collect();
        let report = replace_in_files(&paths, &spec.options, "new", &skip).unwrap();
        assert_eq!(report.changed, vec![(a.clone(), 1)]);
        assert_eq!(report.skipped_unsaved, vec![b.clone()]);
        assert_eq!(report.replacements(), 1);
        assert_eq!(
            std::fs::read_to_string(&a).unwrap(),
            "<r>\r\n  <p>new &#169; text</p>\r\n</r>\r\n"
        );
        assert_eq!(
            std::fs::read_to_string(&b).unwrap(),
            "<r><p>old and old</p></r>"
        );
        assert_eq!(std::fs::read_to_string(&c).unwrap(), c_src, "no text match");
        assert!(!dir.0.join("a.xylem-tmp").exists());
    }

    #[test]
    fn cancelled_search_stops() {
        let dir = TempDir::new("cancel");
        std::fs::write(dir.0.join("a.xml"), "<r>x</r>").unwrap();
        let cancel = Arc::new(AtomicBool::new(true));
        let spec = SearchSpec {
            root: dir.0.clone(),
            pattern: "*.xml".into(),
            subfolders: true,
            options: opts("x"),
        };
        let events: Vec<_> = spawn_search(spec, cancel, Box::new(|| {}))
            .unwrap()
            .iter()
            .collect();
        assert!(matches!(
            events.last(),
            Some(SearchEvent::Done {
                files: 0,
                cancelled: true
            })
        ));
    }

    #[test]
    fn bad_regex_is_reported() {
        let mut o = opts("(unclosed");
        o.regex = true;
        assert!(o.build().is_err());
        assert!(opts("(unclosed").build().is_ok());
    }

    #[test]
    fn highlights_stay_inside_the_context() {
        let doc = Document::from_str("<r><p>abc.\n    </p><p>  x  </p></r>").unwrap();
        let mut o = opts(r"\s+");
        o.regex = true;
        for h in search_document(&doc, &o.build().unwrap(), false) {
            assert!(h.highlight.start <= h.highlight.end);
            assert!(h.highlight.end <= h.context.len(), "{h:?}");
            assert!(h.context.is_char_boundary(h.highlight.start));
        }
        let (ctx, hl) = snippet("abc.\n    ", 4..9);
        assert_eq!((ctx.as_str(), hl), ("abc.", 4..4));
    }

    #[test]
    fn files_that_would_not_round_trip_are_left_alone() {
        let dir = TempDir::new("lossy");
        let path = dir.0.join("a.xml");
        // UTF-16 with an unpaired surrogate, which the parser has to replace:
        // writing the document back would change it too.
        let mut bytes = vec![0xFF, 0xFE];
        let units: Vec<u16> = "<r><p>old "
            .encode_utf16()
            .chain([0xD800])
            .chain("</p></r>".encode_utf16())
            .collect();
        for u in units {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        std::fs::write(&path, &bytes).unwrap();
        let report = replace_in_files(
            std::slice::from_ref(&path),
            &opts("old"),
            "new",
            &HashSet::new(),
        )
        .unwrap();
        assert!(report.changed.is_empty());
        assert_eq!(report.failed.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
