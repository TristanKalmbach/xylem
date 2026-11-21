//! References between modules (dmRef, pmRef) and to graphics (ICN entities).

use std::io;
use std::path::{Path, PathBuf};

use xylem_dom::{Document, NodeId};

use crate::ident::{Dmc, IssueInfo, Language, Pmc};
use crate::issue::Markup;
use crate::xmlutil::{child, descendant};

// ------------------------------------------------------------------ module references

/// A reference to another data module.
#[derive(Debug, Clone, PartialEq)]
pub struct DmRef {
    /// The `dmRef` / `refdm` element.
    pub node: NodeId,
    pub dmc: Dmc,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
}

/// A reference to a publication module.
#[derive(Debug, Clone, PartialEq)]
pub struct PmRef {
    /// The `pmRef` element.
    pub node: NodeId,
    pub pmc: Pmc,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
}

/// Finds every `dmRef` (modern) and `refdm` (legacy) in document order.
pub fn find_dm_refs(doc: &Document) -> Vec<DmRef> {
    doc.descendants(doc.root()).filter_map(|n| read_dm_ref(doc, n)).collect()
}

/// Finds every `pmRef` in document order.
pub fn find_pm_refs(doc: &Document) -> Vec<PmRef> {
    doc.descendants(doc.root()).filter_map(|n| read_pm_ref(doc, n)).collect()
}

fn read_dm_ref(doc: &Document, node: NodeId) -> Option<DmRef> {
    match doc.name(node)? {
        "dmRef" => {
            let ident = child(doc, node, "dmRefIdent")?;
            Some(DmRef {
                node,
                dmc: Dmc::from_dm_code_element(doc, child(doc, ident, "dmCode")?)?,
                issue_info: child(doc, ident, "issueInfo")
                    .and_then(|e| IssueInfo::from_element(doc, e, Markup::Modern)),
                language: child(doc, ident, "language").and_then(|e| Language::from_element(doc, e)),
            })
        }
        "refdm" => Some(DmRef {
            node,
            dmc: Dmc::from_legacy_container(doc, node)?,
            issue_info: descendant(doc, node, "issno").and_then(|e| IssueInfo::from_element(doc, e, Markup::Legacy)),
            language: descendant(doc, node, "language").and_then(|e| Language::from_element(doc, e)),
        }),
        _ => None,
    }
}

fn read_pm_ref(doc: &Document, node: NodeId) -> Option<PmRef> {
    if doc.name(node)? != "pmRef" {
        return None;
    }
    let ident = child(doc, node, "pmRefIdent")?;
    Some(PmRef {
        node,
        pmc: Pmc::from_pm_code_element(doc, child(doc, ident, "pmCode")?)?,
        issue_info: child(doc, ident, "issueInfo").and_then(|e| IssueInfo::from_element(doc, e, Markup::Modern)),
        language: child(doc, ident, "language").and_then(|e| Language::from_element(doc, e)),
    })
}

fn pattern_for(code: &str, issue: Option<&IssueInfo>, language: Option<&Language>) -> String {
    let issue = issue.map_or_else(|| "*".to_string(), ToString::to_string);
    let language = language.map_or_else(|| "*".to_string(), ToString::to_string);
    format!("{code}_{issue}_{language}.XML")
}

impl DmRef {
    /// File name pattern for the referenced module. The issue and language are
    /// included when the reference states them and are wildcards otherwise.
    pub fn filename_pattern(&self) -> String {
        pattern_for(&self.dmc.to_string(), self.issue_info.as_ref(), self.language.as_ref())
    }

    /// Like [`DmRef::filename_pattern`] but accepting any issue, to find the latest one.
    pub fn latest_filename_pattern(&self) -> String {
        pattern_for(&self.dmc.to_string(), None, self.language.as_ref())
    }

    /// Files in `dir` matching [`DmRef::filename_pattern`], oldest issue first.
    pub fn find_in_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        find_files(dir, &self.filename_pattern())
    }

    /// The newest issue of the referenced module found in `dir`.
    pub fn find_latest_in_dir(&self, dir: &Path) -> io::Result<Option<PathBuf>> {
        Ok(find_files(dir, &self.latest_filename_pattern())?.pop())
    }
}

impl PmRef {
    pub fn filename_pattern(&self) -> String {
        pattern_for(&self.pmc.to_string(), self.issue_info.as_ref(), self.language.as_ref())
    }

    pub fn latest_filename_pattern(&self) -> String {
        pattern_for(&self.pmc.to_string(), None, self.language.as_ref())
    }

    pub fn find_in_dir(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        find_files(dir, &self.filename_pattern())
    }

    pub fn find_latest_in_dir(&self, dir: &Path) -> io::Result<Option<PathBuf>> {
        Ok(find_files(dir, &self.latest_filename_pattern())?.pop())
    }
}

/// Case-insensitive glob match supporting `*` (any run) and `?` (any one character).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_uppercase().chars().collect();
    let t: Vec<char> = text.to_uppercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            backtrack = Some((pi, ti));
            pi += 1;
        } else if let Some((star, matched)) = backtrack {
            pi = star + 1;
            ti = matched + 1;
            backtrack = Some((star, matched + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// Sort key from the `_001-02_` part of a CSDB file name.
fn issue_sort_key(path: &Path) -> (u32, u32, String) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let issue = name.split('_').nth(1).and_then(IssueInfo::parse);
    let number = |s: Option<&IssueInfo>, f: fn(&IssueInfo) -> &str| s.and_then(|i| f(i).parse().ok()).unwrap_or(0);
    (
        number(issue.as_ref(), |i| i.issue_number.as_str()),
        number(issue.as_ref(), |i| i.in_work.as_str()),
        name.to_uppercase(),
    )
}

/// Files directly inside `dir` whose names match `pattern`, ordered by issue then in-work number.
pub fn find_files(dir: &Path, pattern: &str) -> io::Result<Vec<PathBuf>> {
    let mut hits = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_str().is_some_and(|n| glob_match(pattern, n)) && entry.file_type()?.is_file() {
            hits.push(entry.path());
        }
    }
    hits.sort_by_key(|p| issue_sort_key(p));
    Ok(hits)
}

// ------------------------------------------------------------------ graphics

/// How a graphic is referenced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IcnRefKind {
    /// `infoEntityIdent` attribute (issue 4.0 and later).
    InfoEntityIdent,
    /// `boardno` attribute (legacy issues).
    BoardNo,
}

/// A reference to an information control number (ICN) graphic.
#[derive(Debug, Clone, PartialEq)]
pub struct IcnRef {
    /// The element carrying the attribute.
    pub node: NodeId,
    pub kind: IcnRefKind,
    /// The ICN / entity name.
    pub icn: String,
    /// File named by a matching `<!ENTITY ... SYSTEM "file">` declaration, if any.
    pub file: Option<String>,
    /// The `NDATA` notation of that declaration (for example `cgm`).
    pub notation: Option<String>,
}

/// A general entity declared in the DOCTYPE internal subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntityDecl {
    pub name: String,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub notation: Option<String>,
}

/// Finds ICN references in document order, resolving them through entity declarations.
pub fn find_icn_refs(doc: &Document) -> Vec<IcnRef> {
    let entities = entity_declarations(doc);
    let mut refs = Vec::new();
    for node in doc.descendants(doc.root()) {
        for (attr, kind) in [("infoEntityIdent", IcnRefKind::InfoEntityIdent), ("boardno", IcnRefKind::BoardNo)] {
            if let Some(icn) = doc.attr(node, attr) {
                let decl = entities.iter().find(|e| e.name == icn);
                refs.push(IcnRef {
                    node,
                    kind,
                    icn: icn.to_string(),
                    file: decl.and_then(|d| d.system_id.clone()),
                    notation: decl.and_then(|d| d.notation.clone()),
                });
            }
        }
    }
    refs
}

/// Parses the general entity declarations of the document's internal DTD subset.
pub fn entity_declarations(doc: &Document) -> Vec<EntityDecl> {
    doc.doctype().and_then(|d| d.internal_subset.as_deref()).map(parse_entity_declarations).unwrap_or_default()
}

/// Parses `<!ENTITY name SYSTEM "file" NDATA notation>` style declarations (parameter entities are skipped).
pub fn parse_entity_declarations(subset: &str) -> Vec<EntityDecl> {
    let mut decls = Vec::new();
    let mut rest = subset;
    while let Some(i) = rest.find("<!") {
        rest = &rest[i..];
        if let Some(body) = rest.strip_prefix("<!--") {
            rest = body.find("-->").map_or("", |e| &body[e + 3..]);
            continue;
        }
        let Some((decl, after)) = split_declaration(rest) else { break };
        rest = after;
        if let Some(body) = decl.strip_prefix("<!ENTITY")
            && let Some(d) = parse_entity(body)
        {
            decls.push(d);
        }
    }
    decls
}

/// Splits off one markup declaration, honouring quoted strings.
fn split_declaration(s: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum Tok<'a> {
    Word(&'a str),
    Quoted(&'a str),
}

fn tokenize_declaration(s: &str) -> Vec<Tok<'_>> {
    let mut toks = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '"' || c == '\'' {
            chars.next();
            let start = i + 1;
            let mut end = s.len();
            for (j, d) in chars.by_ref() {
                if d == c {
                    end = j;
                    break;
                }
            }
            toks.push(Tok::Quoted(&s[start..end]));
        } else {
            let start = i;
            let mut end = s.len();
            while let Some(&(j, d)) = chars.peek() {
                if d.is_whitespace() {
                    end = j;
                    break;
                }
                chars.next();
            }
            toks.push(Tok::Word(&s[start..end]));
        }
    }
    toks
}

fn parse_entity(body: &str) -> Option<EntityDecl> {
    let toks = tokenize_declaration(body);
    let mut it = toks.iter();
    let Tok::Word(name) = it.next()? else { return None };
    if *name == "%" {
        return None;
    }
    let mut decl = EntityDecl { name: (*name).to_string(), public_id: None, system_id: None, notation: None };
    match it.next()? {
        Tok::Word("SYSTEM") => {
            if let Some(Tok::Quoted(s)) = it.next() {
                decl.system_id = Some((*s).to_string());
            }
        }
        Tok::Word("PUBLIC") => {
            if let Some(Tok::Quoted(p)) = it.next() {
                decl.public_id = Some((*p).to_string());
            }
            if let Some(Tok::Quoted(s)) = it.next() {
                decl.system_id = Some((*s).to_string());
            }
        }
        _ => return Some(decl),
    }
    if let Some(Tok::Word("NDATA")) = it.next()
        && let Some(Tok::Word(n)) = it.next()
    {
        decl.notation = Some((*n).to_string());
    }
    Some(decl)
}

#[cfg(test)]
mod tests;
