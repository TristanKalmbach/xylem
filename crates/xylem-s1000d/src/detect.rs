//! Detection of the S1000D issue and schema kind of a document.

use xylem_dom::{Document, NodeId, NodeKind};

use crate::issue::{Issue, Markup, SchemaKind};
use crate::xmlutil::{child, descendant};

/// How the answer in [`S1000dInfo`] was obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectSource {
    /// From an `xsi:noNamespaceSchemaLocation` / `xsi:schemaLocation` URL.
    SchemaLocation,
    /// From the DOCTYPE public or system identifier.
    DocType,
    /// Guessed from element structure; the issue is the best match, not a fact.
    Structure,
}

/// What [`detect`] learned about a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S1000dInfo {
    pub issue: Issue,
    pub schema: SchemaKind,
    /// Name of the document element (`dmodule`, `pm`, ...).
    pub root: String,
    pub source: DetectSource,
}

/// Root elements of S1000D documents other than data modules, with their kinds.
const OTHER_ROOTS: [(&str, SchemaKind); 7] = [
    ("pm", SchemaKind::Pm),
    ("dml", SchemaKind::Dml),
    ("ddn", SchemaKind::Ddn),
    ("comment", SchemaKind::Comment),
    ("update", SchemaKind::Update),
    ("scormContentPackage", SchemaKind::ScormContentPackage),
    ("container", SchemaKind::Container),
];

/// Detects the S1000D issue and kind of `doc`, or `None` if it does not look like S1000D.
pub fn detect(doc: &Document) -> Option<S1000dInfo> {
    let root = doc.document_element()?;
    let root_name = doc.name(root)?.to_string();
    let markup = structural_markup(doc, root, &root_name);

    let from_location = schema_location(doc, root).map(|url| parse_schema_url(&url));
    let (url_issue, url_kind) = from_location.unwrap_or((None, None));
    if url_issue.is_some() || url_kind.is_some() {
        // A schema URL only counts when it really is an S1000D one or the structure agrees.
        let looks_s1000d = url_issue.is_some() || markup.is_some();
        if looks_s1000d {
            let markup = markup
                .or_else(|| url_issue.map(Issue::markup))
                .unwrap_or(Markup::Modern);
            let schema = url_kind
                .or_else(|| kind_from_structure(doc, root, &root_name, markup))
                .unwrap_or(SchemaKind::Unknown);
            let issue = url_issue.unwrap_or_else(|| guess_issue(markup));
            let source = if url_issue.is_some() {
                DetectSource::SchemaLocation
            } else {
                DetectSource::Structure
            };
            return Some(S1000dInfo {
                issue,
                schema,
                root: root_name,
                source,
            });
        }
    }

    if let Some(dt) = doc.doctype() {
        let pubid = dt.public_id.as_deref().unwrap_or("");
        let sysid = dt.system_id.as_deref().unwrap_or("");
        let marked = pubid.to_ascii_uppercase().contains("S1000D")
            || sysid.to_ascii_uppercase().contains("S1000D");
        if marked || (markup == Some(Markup::Legacy) && root_name == "dmodule") {
            let markup = markup.unwrap_or(Markup::Legacy);
            let issue = issue_from_text(pubid)
                .or_else(|| issue_from_text(sysid))
                .or_else(|| issue_from_comments(doc))
                .unwrap_or_else(|| guess_issue(markup));
            let schema = kind_from_public_id(pubid)
                .or_else(|| kind_from_system_id(sysid))
                .or_else(|| kind_from_structure(doc, root, &root_name, markup))
                .unwrap_or(SchemaKind::Unknown);
            return Some(S1000dInfo {
                issue,
                schema,
                root: root_name,
                source: DetectSource::DocType,
            });
        }
    }

    let markup = markup?;
    let schema = kind_from_structure(doc, root, &root_name, markup).unwrap_or(SchemaKind::Unknown);
    let issue = issue_from_comments(doc).unwrap_or_else(|| guess_issue(markup));
    Some(S1000dInfo {
        issue,
        schema,
        root: root_name,
        source: DetectSource::Structure,
    })
}

/// The markup family implied by the document's identification section, if it has one.
fn structural_markup(doc: &Document, root: NodeId, root_name: &str) -> Option<Markup> {
    let is_known_root = root_name == "dmodule" || OTHER_ROOTS.iter().any(|(n, _)| *n == root_name);
    if !is_known_root {
        return None;
    }
    if child(doc, root, "identAndStatusSection").is_some() {
        return Some(Markup::Modern);
    }
    if child(doc, root, "idstatus").is_some()
        || (root_name == "pm" && descendant(doc, root, "pmaddres").is_some())
    {
        return Some(Markup::Legacy);
    }
    None
}

fn kind_from_structure(
    doc: &Document,
    root: NodeId,
    root_name: &str,
    markup: Markup,
) -> Option<SchemaKind> {
    if root_name != "dmodule" {
        return OTHER_ROOTS
            .iter()
            .find(|(n, _)| *n == root_name)
            .map(|&(_, k)| k);
    }
    let content = child(doc, root, "content")?;
    let first = doc.element_children(content).next()?;
    SchemaKind::from_content_element(doc.name(first)?, markup)
}

/// Best-effort issue when nothing declares it: the most common issue of each markup family.
fn guess_issue(markup: Markup) -> Issue {
    match markup {
        Markup::Legacy => Issue::V3_0,
        Markup::Modern => Issue::V4_1,
    }
}

fn schema_location(doc: &Document, root: NodeId) -> Option<String> {
    let el = doc.element(root)?;
    let by_suffix = |suffix: &str| {
        el.attrs
            .iter()
            .find(|a| a.name == suffix || a.name.ends_with(&format!(":{suffix}")))
            .map(|a| a.value.clone())
    };
    by_suffix("noNamespaceSchemaLocation").or_else(|| {
        // schemaLocation holds "namespace location" pairs; the location is the last token.
        by_suffix("schemaLocation").and_then(|v| v.split_whitespace().last().map(str::to_string))
    })
}

/// Extracts the issue (from `S1000D_4-1`) and kind (from `descript.xsd`) of a schema URL.
pub(crate) fn parse_schema_url(url: &str) -> (Option<Issue>, Option<SchemaKind>) {
    let issue = url
        .to_ascii_uppercase()
        .find("S1000D_")
        .and_then(|i| issue_from_numbers_at(&url[i + "S1000D_".len()..]));
    let file = url.rsplit(['/', '\\']).next().unwrap_or(url);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    (issue, SchemaKind::from_stem(stem))
}

/// Parses `4-0-1/...` or `5.0 ...` at the start of `s`.
fn issue_from_numbers_at(s: &str) -> Option<Issue> {
    let mut numbers = Vec::new();
    let mut chars = s.chars().peekable();
    loop {
        let mut digits = String::new();
        while let Some(&c) = chars.peek() {
            if c.is_ascii_digit() {
                digits.push(c);
                chars.next();
            } else {
                break;
            }
        }
        if digits.is_empty() {
            break;
        }
        numbers.push(digits.parse::<u32>().ok()?);
        match chars.peek() {
            Some('-' | '.' | '_') => {
                chars.next();
            }
            _ => break,
        }
    }
    Issue::from_numbers(&numbers)
}

/// Finds `Issue 3.0`, `issue 4-1` or `S1000D_4-2` inside free text.
fn issue_from_text(text: &str) -> Option<Issue> {
    let upper = text.to_ascii_uppercase();
    for marker in ["ISSUE", "S1000D_", "S1000D-"] {
        let mut from = 0;
        while let Some(i) = upper[from..].find(marker) {
            let after = from + i + marker.len();
            let rest = text[after..].trim_start();
            if let Some(issue) = issue_from_numbers_at(rest) {
                return Some(issue);
            }
            from = after;
        }
    }
    None
}

/// Top-level comments such as `<!-- S1000D issue 3.0 -->` hint at the issue of DTD documents.
fn issue_from_comments(doc: &Document) -> Option<Issue> {
    doc.children(doc.root())
        .iter()
        .find_map(|&c| match doc.kind(c) {
            NodeKind::Comment(text) if text.to_ascii_uppercase().contains("S1000D") => {
                issue_from_text(text)
            }
            _ => None,
        })
}

fn kind_from_public_id(pubid: &str) -> Option<SchemaKind> {
    let p = pubid.to_ascii_lowercase();
    const TABLE: [(&str, SchemaKind); 14] = [
        ("descriptive", SchemaKind::Descript),
        ("procedur", SchemaKind::Proced),
        ("fault", SchemaKind::Fault),
        ("illustrated parts", SchemaKind::Ipd),
        ("parts data", SchemaKind::Ipd),
        ("crew", SchemaKind::Crew),
        ("business rule", SchemaKind::Brex),
        ("wiring data", SchemaKind::Wrngdata),
        ("wiring fault", SchemaKind::Wrngflds),
        ("publication", SchemaKind::Pm),
        ("data management list", SchemaKind::Dml),
        ("dispatch", SchemaKind::Ddn),
        ("comment", SchemaKind::Comment),
        ("update", SchemaKind::Update),
    ];
    TABLE
        .iter()
        .find(|(needle, _)| p.contains(needle))
        .map(|&(_, k)| k)
}

fn kind_from_system_id(sysid: &str) -> Option<SchemaKind> {
    let file = sysid.rsplit(['/', '\\']).next()?;
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    SchemaKind::from_stem(stem)
}

#[cfg(test)]
mod tests;
