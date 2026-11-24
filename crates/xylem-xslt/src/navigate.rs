//! Finding your way around stylesheets: where a named template, function,
//! key, mode or variable is defined, and everywhere it is used, across a
//! stylesheet and every module it includes or imports.
//!
//! An [`Index`] is built from one or more top-level stylesheets (for a
//! shared module, every stylesheet that uses it, so an override in another
//! module shows as a second definition). [`Index::references_at`] then lists
//! what the element at a position refers to or defines.
//!
//! Names are compared as expanded names (namespace URI and local name), so
//! `f:title()` and `fn2:title()` are the same function when both prefixes
//! are bound to the same URI. Variables follow XSLT scoping: a reference
//! goes to the nearest `xsl:variable` or `xsl:param` before it in the same
//! or an enclosing element, else to the global one; variables bound inside
//! an expression (`for $x in`) are left out.

use crate::stylesheet::{Module, Stylesheet, XSLT_NS, path_key};
use crate::xpath::{Kind as Tok, Token, tokenize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xylem_dom::NodeId;

/// What a name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Kind {
    Template,
    Function,
    Key,
    Mode,
    Variable,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Template => "Template",
            Kind::Function => "Function",
            Kind::Key => "Key",
            Kind::Mode => "Mode",
            Kind::Variable => "Variable",
        }
    }
}

/// A place in a stylesheet file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub file: PathBuf,
    /// 1-based.
    pub line: u32,
    /// The template or function it is in.
    pub within: Option<String>,
    /// A short excerpt: the attribute or element, as written.
    pub excerpt: String,
}

/// A name the element at a position refers to or defines.
#[derive(Debug, Clone)]
pub struct Reference {
    pub kind: Kind,
    /// As written at that position.
    pub name: String,
    pub definitions: Vec<Place>,
    pub usages: Vec<Place>,
}

/// What a name resolves to: its kind and expanded name, or for a local
/// variable, the place that declares it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Sym {
    kind: Kind,
    name: String,
}

/// Names defined and used in a set of stylesheet modules.
pub struct Index {
    pub stylesheet: Stylesheet,
    defs: HashMap<Sym, Vec<Place>>,
    uses: HashMap<Sym, Vec<Place>>,
    /// For each element (module, byte offset): the names it defines or
    /// uses, as written.
    at: HashMap<(String, usize), Vec<(Sym, String)>>,
}

/// Namespaces whose functions are built in (no definition to go to).
const BUILT_IN: &[&str] = &[
    "http://www.w3.org/2005/xpath-functions",
    "http://www.w3.org/2001/XMLSchema",
    "http://www.w3.org/2005/xpath-functions/math",
    "http://www.w3.org/2005/xpath-functions/map",
    "http://www.w3.org/2005/xpath-functions/array",
    XSLT_NS,
];

/// Attributes of XSLT elements that hold an XPath expression or pattern.
const EXPRESSION_ATTRS: &[&str] = &[
    "select",
    "test",
    "match",
    "use",
    "value",
    "group-by",
    "group-adjacent",
    "group-starting-with",
    "group-ending-with",
    "from",
    "count",
];

/// Attributes of XSLT elements that are attribute value templates.
const AVT_ATTRS: &[&str] = &["name", "namespace", "href", "format", "lang"];

impl Index {
    /// Indexes `tops` and every module they include or import; `read` gives
    /// the bytes to use instead of a file on disk (unsaved edits).
    pub fn build(
        tops: &[PathBuf],
        read: &dyn Fn(&Path) -> Option<Vec<u8>>,
    ) -> Result<Index, String> {
        let stylesheet = Stylesheet::load_many(tops, read)?;
        let mut index = Index {
            stylesheet: Stylesheet {
                top: PathBuf::new(),
                modules: Vec::new(),
                warnings: Vec::new(),
            },
            defs: HashMap::new(),
            uses: HashMap::new(),
            at: HashMap::new(),
        };
        for m in &stylesheet.modules {
            index.scan(m);
        }
        index.stylesheet = stylesheet;
        Ok(index)
    }

    /// What the element starting at byte `offset` of `file` (as the module
    /// was read) defines or refers to, each with its definitions and usages.
    pub fn references_at(&self, file: &Path, offset: usize) -> Vec<Reference> {
        let Some(found) = self.at.get(&(path_key(file), offset)) else {
            return Vec::new();
        };
        let mut out: Vec<Reference> = Vec::new();
        for (sym, written) in found {
            if out.iter().any(|r| r.kind == sym.kind && r.name == *written) {
                continue;
            }
            out.push(Reference {
                kind: sym.kind,
                name: written.clone(),
                definitions: self.defs.get(sym).cloned().unwrap_or_default(),
                usages: self.uses.get(sym).cloned().unwrap_or_default(),
            });
        }
        out
    }

    /// The element of `file` whose start tag contains byte `offset`, or the
    /// innermost element around it, as the byte offset of its start tag.
    pub fn element_at(&self, file: &Path, offset: usize) -> Option<usize> {
        let key = path_key(file);
        let m = self
            .stylesheet
            .modules
            .iter()
            .find(|m| path_key(&m.path) == key)?;
        m.doc
            .descendants(m.doc.root())
            .filter(|&n| m.doc.is_element(n))
            .map(|n| m.offset(n))
            .filter(|&o| o <= offset)
            .max()
    }

    fn scan(&mut self, m: &Module) {
        let Some(root) = m.doc.document_element() else {
            return;
        };
        let key = path_key(&m.path);
        for el in m.doc.descendants(root) {
            if !m.doc.is_element(el) {
                continue;
            }
            let mut found = Vec::new();
            match m.xsl_name(el) {
                Some(local) => self.scan_xsl(m, el, local, root, &mut found),
                None => {
                    // Literal result elements: every attribute is an AVT.
                    let names: Vec<String> = m
                        .doc
                        .element(el)
                        .map(|e| e.attrs.iter().map(|a| a.name.clone()).collect())
                        .unwrap_or_default();
                    for a in names {
                        if !a.starts_with("xmlns") {
                            self.scan_avt(m, el, &a, &mut found);
                        }
                    }
                }
            }
            if !found.is_empty() {
                self.at
                    .entry((key.clone(), m.offset(el)))
                    .or_default()
                    .extend(found);
            }
        }
    }

    fn scan_xsl(
        &mut self,
        m: &Module,
        el: NodeId,
        local: &str,
        root: NodeId,
        found: &mut Vec<(Sym, String)>,
    ) {
        let attr = |name: &str| m.doc.attr(el, name).map(str::to_string);
        match local {
            "template" => {
                if let Some(n) = attr("name") {
                    self.def(m, el, "name", Kind::Template, &n, found);
                }
                if let Some(modes) = attr("mode") {
                    for mode in modes.split_whitespace().filter(|t| !t.starts_with('#')) {
                        self.def(m, el, "mode", Kind::Mode, mode, found);
                    }
                }
            }
            "function" => {
                if let Some(n) = attr("name") {
                    self.def(m, el, "name", Kind::Function, &n, found);
                }
            }
            "key" => {
                if let Some(n) = attr("name") {
                    self.def(m, el, "name", Kind::Key, &n, found);
                }
            }
            "call-template" => {
                if let Some(n) = attr("name") {
                    self.use_(m, el, "name", Kind::Template, &n, found);
                }
            }
            "apply-templates" => {
                if let Some(mode) = attr("mode").filter(|t| !t.starts_with('#')) {
                    self.use_(m, el, "mode", Kind::Mode, mode.trim(), found);
                }
            }
            "variable" | "param" => {
                if let Some(n) = attr("name") {
                    if m.doc.parent(el) == Some(root) {
                        self.def(m, el, "name", Kind::Variable, &n, found);
                    } else {
                        // A local variable: its own symbol, named by where it is.
                        let sym = local_sym(m, el, &n);
                        let place = self.place(m, el, "name");
                        self.defs.entry(sym.clone()).or_default().push(place);
                        found.push((sym, n));
                    }
                }
            }
            _ => {}
        }
        for a in EXPRESSION_ATTRS {
            if let Some(v) = attr(a) {
                self.scan_expression(m, el, a, &v, 0, found);
            }
        }
        for a in AVT_ATTRS {
            // `name` of the declarations above is a name, not a template.
            let is_name = *a == "name"
                && matches!(
                    local,
                    "template"
                        | "function"
                        | "key"
                        | "call-template"
                        | "variable"
                        | "param"
                        | "with-param"
                        | "attribute-set"
                        | "character-map"
                        | "decimal-format"
                        | "output"
                );
            if !is_name
                && matches!(
                    local,
                    "element"
                        | "attribute"
                        | "result-document"
                        | "number"
                        | "sort"
                        | "processing-instruction"
                        | "namespace"
                )
            {
                self.scan_avt(m, el, a, found);
            }
        }
    }

    /// Scans `{…}` parts of attribute `name`.
    fn scan_avt(&mut self, m: &Module, el: NodeId, name: &str, found: &mut Vec<(Sym, String)>) {
        let Some(v) = m.doc.attr(el, name).map(str::to_string) else {
            return;
        };
        let b = v.as_bytes();
        let mut i = 0;
        while i < b.len() {
            match b[i] {
                b'{' if b.get(i + 1) == Some(&b'{') => i += 2,
                b'{' => {
                    let start = i + 1;
                    let mut j = start;
                    let mut quote = None;
                    while j < b.len() {
                        match (quote, b[j]) {
                            (None, b'"' | b'\'') => quote = Some(b[j]),
                            (Some(q), c) if c == q => quote = None,
                            (None, b'}') => break,
                            _ => {}
                        }
                        j += 1;
                    }
                    self.scan_expression(m, el, name, &v[start..j.min(b.len())], start, found);
                    i = j + 1;
                }
                _ => i += 1,
            }
        }
    }

    /// Records the functions, keys and variables an expression uses.
    /// `base` is where `src` starts in the attribute's value.
    fn scan_expression(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        src: &str,
        base: usize,
        found: &mut Vec<(Sym, String)>,
    ) {
        let Some(tokens) = tokenize(src) else {
            return;
        };
        let bound = expression_bindings(&tokens, src);
        for (i, t) in tokens.iter().enumerate() {
            let next = tokens.get(i + 1).map(|n| n.kind);
            match t.kind {
                Tok::Name if next == Some(Tok::LParen) => {
                    let name = t.text(src);
                    if name == "key" || name == "fn:key" {
                        if let (Some(arg), Some(Tok::String)) =
                            (tokens.get(i + 2), tokens.get(i + 2).map(|a| a.kind))
                        {
                            let lit = arg.text(src);
                            let key_name = &lit[1..lit.len().saturating_sub(1).max(1)];
                            let place = self.place_at(m, el, attr, base + arg.span.start);
                            self.record_use(m, el, Kind::Key, key_name, place, found);
                        }
                    } else if let Some((prefix, _)) = name.split_once(':')
                        && let Some(uri) = namespace_of(m, el, prefix)
                        && !BUILT_IN.contains(&uri.as_str())
                    {
                        let place = self.place_at(m, el, attr, base + t.span.start);
                        self.record_use(m, el, Kind::Function, name, place, found);
                    }
                }
                Tok::Variable => {
                    let name = &t.text(src)[1..];
                    if bound.iter().any(|b| b == name) {
                        continue;
                    }
                    let place = self.place_at(m, el, attr, base + t.span.start);
                    let sym = match local_declaration(m, el, name) {
                        Some(decl) => local_sym(m, decl, name),
                        None => Sym {
                            kind: Kind::Variable,
                            name: expand(m, el, name),
                        },
                    };
                    self.uses.entry(sym.clone()).or_default().push(place);
                    found.push((sym, name.to_string()));
                }
                _ => {}
            }
        }
    }

    fn def(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        kind: Kind,
        name: &str,
        found: &mut Vec<(Sym, String)>,
    ) {
        let sym = Sym {
            kind,
            name: expand(m, el, name.trim()),
        };
        let place = self.place(m, el, attr);
        self.defs.entry(sym.clone()).or_default().push(place);
        found.push((sym, name.trim().to_string()));
    }

    fn use_(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        kind: Kind,
        name: &str,
        found: &mut Vec<(Sym, String)>,
    ) {
        let place = self.place(m, el, attr);
        self.record_use(m, el, kind, name.trim(), place, found);
    }

    fn record_use(
        &mut self,
        m: &Module,
        el: NodeId,
        kind: Kind,
        name: &str,
        place: Place,
        found: &mut Vec<(Sym, String)>,
    ) {
        let sym = Sym {
            kind,
            name: expand(m, el, name),
        };
        self.uses.entry(sym.clone()).or_default().push(place);
        found.push((sym, name.to_string()));
    }

    /// The place of attribute `attr` of `el` (or of `el` itself).
    fn place(&self, m: &Module, el: NodeId, attr: &str) -> Place {
        let offset = m.raw_attr(el, attr).map_or(m.offset(el), |r| r.raw.start);
        Place {
            file: m.path.clone(),
            line: m.line_of(offset),
            within: within(m, el),
            excerpt: excerpt(m, el, attr),
        }
    }

    /// The place of byte `at` of attribute `attr`'s value.
    fn place_at(&self, m: &Module, el: NodeId, attr: &str, at: usize) -> Place {
        let offset = m
            .raw_attr(el, attr)
            .filter(|r| !r.entities && at <= r.value.len())
            .map_or(m.offset(el), |r| r.file_range(at..at).start);
        Place {
            file: m.path.clone(),
            line: m.line_of(offset),
            within: within(m, el),
            excerpt: excerpt(m, el, attr),
        }
    }
}

/// Variables bound inside the expression (`for $x in`, `some $x in`,
/// `let $x :=`).
fn expression_bindings(tokens: &[Token], src: &str) -> Vec<String> {
    tokens
        .windows(2)
        .filter(|w| {
            w[0].kind == Tok::Variable
                && ((w[1].kind == Tok::Name && w[1].text(src) == "in")
                    || (w[1].kind == Tok::Op && w[1].text(src) == ":="))
        })
        .map(|w| w[0].text(src)[1..].to_string())
        .collect()
}

/// The nearest local `xsl:variable` or `xsl:param` named `name` in scope
/// at `el`: before it, or before one of its ancestors, below the top level.
fn local_declaration(m: &Module, el: NodeId, name: &str) -> Option<NodeId> {
    let root = m.doc.document_element()?;
    let mut n = el;
    loop {
        let parent = m.doc.parent(n)?;
        if parent == root || n == root {
            return None;
        }
        let mut s = m.doc.prev_sibling(n);
        while let Some(sib) = s {
            if m.doc.is_element(sib)
                && (m.is_xsl(sib, "variable") || m.is_xsl(sib, "param"))
                && m.doc.attr(sib, "name").map(str::trim) == Some(name)
            {
                return Some(sib);
            }
            s = m.doc.prev_sibling(sib);
        }
        n = parent;
    }
}

fn local_sym(m: &Module, decl: NodeId, name: &str) -> Sym {
    Sym {
        kind: Kind::Variable,
        name: format!("{}#{}#{name}", path_key(&m.path), m.offset(decl)),
    }
}

/// The namespace URI bound to `prefix` at `el`.
fn namespace_of(m: &Module, el: NodeId, prefix: &str) -> Option<String> {
    let attr = format!("xmlns:{prefix}");
    std::iter::once(el)
        .chain(m.doc.ancestors(el))
        .find_map(|a| m.doc.attr(a, &attr).map(str::to_string))
}

/// `{uri}local` for a prefixed name, the name itself otherwise.
fn expand(m: &Module, el: NodeId, name: &str) -> String {
    match name.split_once(':') {
        Some((prefix, local)) => match namespace_of(m, el, prefix) {
            Some(uri) => format!("{{{uri}}}{local}"),
            None => name.to_string(),
        },
        None => name.to_string(),
    }
}

fn within(m: &Module, el: NodeId) -> Option<String> {
    let owner = std::iter::once(el)
        .chain(m.doc.ancestors(el))
        .find(|&a| m.is_xsl(a, "template") || m.is_xsl(a, "function"))?;
    Some(if m.is_xsl(owner, "function") {
        format!("{}()", m.doc.attr(owner, "name").unwrap_or("?"))
    } else if let Some(n) = m.doc.attr(owner, "name") {
        n.to_string()
    } else {
        format!("match=\"{}\"", m.doc.attr(owner, "match").unwrap_or("?"))
    })
}

fn excerpt(m: &Module, el: NodeId, attr: &str) -> String {
    let name = m.doc.name(el).unwrap_or("?");
    let text = match m.doc.attr(el, attr) {
        Some(v) => format!("<{name} {attr}=\"{}\">", v.trim()),
        None => format!("<{name}>"),
    };
    let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 100 {
        let cut: String = flat.chars().take(99).collect();
        format!("{cut}…")
    } else {
        flat
    }
}

/// The top-level stylesheets that use `file` (it included), found in the
/// folder the stylesheets live in: the top folder of its git repository, or
/// the file's own folder outside git.
pub fn tops_using(file: &Path) -> Vec<PathBuf> {
    let scope = crate::git::scope_of(file)
        .or_else(|| file.parent().map(Path::to_path_buf))
        .unwrap_or_default();
    let mut tops = crate::proof::dependents(&scope, &[file.to_path_buf()]);
    // The file itself first, so it is the index's top when nothing else uses it.
    let key = path_key(file);
    tops.retain(|t| path_key(t) != key);
    tops.insert(0, file.to_path_buf());
    tops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join("xylem-xslt-navigate").join(dir);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (name, text) in files {
            std::fs::write(d.join(name), text).unwrap();
        }
        d
    }

    const TOP: &str = r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:f="urn:f">
  <xsl:include href="common.xsl"/>
  <xsl:variable name="lang" select="'en'"/>
  <xsl:template match="/">
    <xsl:variable name="title" select="f:title(., $lang)"/>
    <xsl:call-template name="header"/>
    <xsl:apply-templates mode="toc"/>
    <out ref="{key('by-id', $title)}"/>
    <xsl:for-each select="for $lang in items return $lang"/>
  </xsl:template>
</xsl:stylesheet>
"#;

    const COMMON: &str = r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:g="urn:f">
  <xsl:key name="by-id" match="*" use="@id"/>
  <xsl:function name="g:title">
    <xsl:param name="n"/>
    <xsl:param name="lang"/>
    <xsl:sequence select="$n/title[@lang = $lang]"/>
  </xsl:function>
  <xsl:template name="header"><xsl:value-of select="$lang"/></xsl:template>
  <xsl:template match="*" mode="toc"/>
</xsl:stylesheet>
"#;

    fn refs_at(index: &Index, file: &Path, needle: &str) -> Vec<Reference> {
        let text = std::fs::read_to_string(file).unwrap();
        let at = text.find(needle).expect("needle");
        let el = index.element_at(file, at).unwrap();
        index.references_at(file, el)
    }

    #[test]
    fn names_go_to_their_definitions_across_modules() {
        let d = write("defs", &[("top.xsl", TOP), ("common.xsl", COMMON)]);
        let top = d.join("top.xsl");
        let common = d.join("common.xsl");
        let index = Index::build(std::slice::from_ref(&top), &|_| None).unwrap();

        let r = refs_at(&index, &top, "f:title(., $lang)");
        let f = r.iter().find(|r| r.kind == Kind::Function).unwrap();
        assert_eq!(f.name, "f:title");
        assert_eq!(f.definitions.len(), 1, "g:title is the same function");
        assert_eq!(f.definitions[0].file, common);
        let lang = r.iter().find(|r| r.name == "lang").unwrap();
        assert_eq!(lang.definitions[0].line, 3);
        // The global $lang is used here and in the header template; the
        // function's own $lang and the for-bound one are not counted.
        assert_eq!(lang.usages.len(), 2);

        let r = refs_at(&index, &top, "call-template");
        assert_eq!(r[0].kind, Kind::Template);
        assert_eq!(r[0].definitions[0].file, common);

        let r = refs_at(&index, &top, "apply-templates mode");
        assert_eq!(r[0].kind, Kind::Mode);
        assert_eq!(r[0].definitions.len(), 1);

        let r = refs_at(&index, &top, "<out ");
        let key = r.iter().find(|r| r.kind == Kind::Key).unwrap();
        assert_eq!(key.name, "by-id");
        assert_eq!(key.definitions[0].file, common);
        let title = r.iter().find(|r| r.name == "title").unwrap();
        assert_eq!(title.definitions[0].line, 5, "the local variable");

        // From a definition: its usages.
        let r = refs_at(&index, &common, "name=\"header\"");
        assert_eq!(r[0].usages.len(), 1);
        assert_eq!(r[0].usages[0].file, top);
        assert_eq!(r[0].usages[0].within.as_deref(), Some("match=\"/\""));
    }

    #[test]
    fn unsaved_text_is_used_instead_of_the_file() {
        let d = write("unsaved", &[("top.xsl", TOP), ("common.xsl", COMMON)]);
        let top = d.join("top.xsl");
        let edited = TOP.replace(
            "<xsl:call-template name=\"header\"/>",
            "<xsl:call-template name=\"footer\"/>",
        );
        let key = path_key(&top);
        let index = Index::build(std::slice::from_ref(&top), &|p| {
            (path_key(p) == key).then(|| edited.clone().into_bytes())
        })
        .unwrap();
        let at = edited.find("call-template").unwrap();
        let el = index.element_at(&top, at).unwrap();
        let r = index.references_at(&top, el);
        assert_eq!(r[0].name, "footer");
        assert!(r[0].definitions.is_empty());
    }
}
