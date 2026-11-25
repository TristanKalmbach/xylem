//! Reads stylesheet modules into a [`Stylesheet`]: declarations, template
//! rules with their priorities and import precedence, and instruction trees.

use crate::tree::{self, Kind, QName, StripSpace, Tree};
use crate::xpath::{self, BinOp, Expr, SeqType};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

pub const XSL_NS: &str = "http://www.w3.org/1999/XSL/Transform";

/// An attribute value template: literal text and `{expr}` parts.
#[derive(Clone, Debug)]
pub enum Avt {
    Lit(String),
    Parts(Vec<AvtPart>),
}

#[derive(Clone, Debug)]
pub enum AvtPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: QName,
    pub select: Option<Expr>,
    pub body: Vec<Instr>,
    pub as_: Option<SeqType>,
    pub tunnel: bool,
}

#[derive(Clone, Debug)]
pub struct Sort {
    pub select: Option<Expr>,
    pub order: Option<Avt>,
    pub data_type: Option<Avt>,
    pub case_order: Option<Avt>,
}

/// How `xsl:for-each-group` forms groups.
#[derive(Clone, Debug)]
pub enum GroupBy {
    By,
    Adjacent,
    StartingWith,
    EndingWith,
}

#[derive(Clone, Debug)]
pub enum ModeRef {
    Default,
    Current,
    Named(QName),
}

#[derive(Clone, Debug)]
pub struct Number {
    pub level: String,
    pub count: Option<Vec<Expr>>,
    pub from: Option<Vec<Expr>>,
    pub value: Option<Expr>,
    pub format: Avt,
    pub select: Option<Expr>,
}

#[derive(Clone, Debug)]
pub enum Instr {
    Text(String),
    Lre {
        name: QName,
        ns: Vec<(Rc<str>, Rc<str>)>,
        attrs: Vec<(QName, Avt)>,
        sets: Vec<QName>,
        body: Vec<Instr>,
    },
    ValueOf {
        select: Option<Expr>,
        sep: Option<Avt>,
        body: Vec<Instr>,
    },
    ApplyTemplates {
        select: Option<Expr>,
        mode: ModeRef,
        params: Vec<Param>,
        sorts: Vec<Sort>,
    },
    CallTemplate {
        name: QName,
        params: Vec<Param>,
    },
    Variable(Param),
    /// `xsl:next-match` (`imports` false) or `xsl:apply-imports` (true).
    NextMatch {
        params: Vec<Param>,
        imports: bool,
    },
    Choose(Vec<(Expr, Vec<Instr>)>, Vec<Instr>),
    If(Expr, Vec<Instr>),
    /// `xsl:result-document`: a secondary result at `href`, or the
    /// principal result when `href` is absent or empty.
    ResultDocument {
        href: Option<Avt>,
        body: Vec<Instr>,
    },
    /// `xsl:for-each-group`: `key` is the grouping expression or pattern.
    ForEachGroup {
        select: Expr,
        how: GroupBy,
        key: Expr,
        sorts: Vec<Sort>,
        body: Vec<Instr>,
    },
    /// `xsl:perform-sort`: `select`, or the body's sequence, sorted.
    PerformSort {
        select: Option<Expr>,
        sorts: Vec<Sort>,
        body: Vec<Instr>,
    },
    /// `xsl:document`: a new document node holding the body's result.
    Document(Vec<Instr>),
    /// `xsl:analyze-string`.
    AnalyzeString {
        select: Expr,
        regex: Avt,
        flags: Option<Avt>,
        matching: Vec<Instr>,
        non_matching: Vec<Instr>,
    },
    ForEach {
        select: Expr,
        sorts: Vec<Sort>,
        body: Vec<Instr>,
    },
    Attribute {
        name: Avt,
        namespace: Option<Avt>,
        scope: Rc<Vec<(Rc<str>, Rc<str>)>>,
        select: Option<Expr>,
        sep: Option<Avt>,
        body: Vec<Instr>,
    },
    Element {
        name: Avt,
        namespace: Option<Avt>,
        scope: Rc<Vec<(Rc<str>, Rc<str>)>>,
        sets: Vec<QName>,
        body: Vec<Instr>,
    },
    CopyOf(Expr),
    Copy {
        sets: Vec<QName>,
        body: Vec<Instr>,
    },
    Sequence(Expr),
    Comment {
        select: Option<Expr>,
        body: Vec<Instr>,
    },
    Pi {
        name: Avt,
        select: Option<Expr>,
        body: Vec<Instr>,
    },
    Number(Box<Number>),
    Message {
        select: Option<Expr>,
        body: Vec<Instr>,
        terminate: bool,
    },
}

/// The `xsl:output` settings.
#[derive(Debug, Clone, Default)]
pub struct OutputSpec {
    pub method: Option<String>,
    pub version: Option<String>,
    pub encoding: Option<String>,
    pub doctype_public: Option<String>,
    pub doctype_system: Option<String>,
    pub indent: bool,
    pub omit_declaration: bool,
    pub media_type: Option<String>,
}

/// A template's or function's declaration as written, for profiles.
#[derive(Debug, Clone, Default)]
pub struct Source {
    /// The module's path.
    pub file: String,
    /// Line where the declaration's start tag ends.
    pub line: u32,
    pub name: Option<String>,
    pub pattern: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug)]
pub struct Template {
    pub id: usize,
    pub matches: Vec<(Expr, f64)>,
    pub name: Option<QName>,
    /// None = the default (unnamed) mode; `#all` is `all_modes`.
    pub modes: Vec<Option<QName>>,
    pub all_modes: bool,
    pub params: Vec<Param>,
    pub body: Vec<Instr>,
    pub as_: Option<SeqType>,
    pub prec: usize,
    /// Where the rule is declared, for reports (`file:line`).
    pub location: String,
    pub source: Source,
}

#[derive(Debug)]
pub struct Function {
    /// Index among the stylesheet's functions (for timing).
    pub id: usize,
    /// Where it is declared, for profiles.
    pub source: Source,
    pub name: QName,
    pub params: Vec<Param>,
    pub body: Vec<Instr>,
    pub as_: Option<SeqType>,
}

#[derive(Debug)]
pub struct Key {
    pub matches: Vec<Expr>,
    pub use_: Expr,
}

#[derive(Debug)]
pub struct AttrSet {
    pub uses: Vec<QName>,
    pub body: Vec<Instr>,
}

#[derive(Debug, Default)]
pub struct Stylesheet {
    pub version: String,
    pub templates: Vec<Template>,
    pub named: HashMap<QName, usize>,
    pub functions: HashMap<(QName, usize), Function>,
    pub globals: Vec<(Param, bool)>,
    pub keys: HashMap<QName, Vec<Key>>,
    pub attr_sets: HashMap<QName, Vec<AttrSet>>,
    pub strip: Vec<(Option<String>, Option<String>, bool)>,
    /// The `xsl:output` declarations merged (later ones win).
    pub output: OutputSpec,
    pub base: PathBuf,
    /// Modules read besides the top one (imports and includes). The
    /// reference processor gives each a document number before the
    /// source document's (spec note 1).
    pub extra_modules: u32,
}

impl StripSpace for Stylesheet {
    /// `xsl:strip-space` / `xsl:preserve-space`, the later and more specific
    /// test winning (a name beats `prefix:*` beats `*`).
    fn strip(&self, element: &QName) -> bool {
        let mut best: Option<(u8, bool)> = None;
        for (uri, local, strip) in &self.strip {
            let rank = match (uri, local) {
                (Some(u), Some(l)) if **u == *element.uri && **l == *element.local => 3,
                (Some(u), None) if **u == *element.uri => 2,
                (None, None) => 1,
                _ => continue,
            };
            if best.is_none_or(|(r, _)| rank >= r) {
                best = Some((rank, *strip));
            }
        }
        best.is_some_and(|(_, s)| s)
    }
}

struct Compiler {
    ss: Stylesheet,
    next_prec: usize,
    loading: Vec<PathBuf>,
}

impl Stylesheet {
    pub fn compile(path: &Path) -> Result<Stylesheet, String> {
        let mut c = Compiler {
            ss: Stylesheet::default(),
            next_prec: 0,
            loading: Vec::new(),
        };
        c.ss.base = path.parent().map(Path::to_path_buf).unwrap_or_default();
        c.module(path, None)?;
        let ss = c.ss;
        Ok(ss)
    }
}

fn load_tree(path: &Path) -> Result<Tree, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc =
        xylem_dom::Document::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    struct Keep;
    impl StripSpace for Keep {
        fn strip(&self, _: &QName) -> bool {
            false
        }
    }
    // Lines as the reference processor reports them: where each element's
    // start tag ends, counted in the file's text (or its re-serialised
    // form when the parser does not keep it byte for byte).
    let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    let (serialised, offsets) = doc.to_xml_with_offsets();
    let text = match std::str::from_utf8(rest) {
        Ok(t) if t == serialised => t.to_string(),
        _ => serialised,
    };
    let mut starts = vec![0usize];
    starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let tb = text.as_bytes();
    let lines: Vec<u32> = offsets
        .iter()
        .map(|&o| {
            let mut i = o;
            let mut quote = None;
            while i < tb.len() {
                match (quote, tb[i]) {
                    (None, b'"' | b'\'') => quote = Some(tb[i]),
                    (Some(q), c) if c == q => quote = None,
                    (None, b'>') => break,
                    _ => {}
                }
                i += 1;
            }
            starts.partition_point(|&s| s <= i) as u32
        })
        .collect();
    tree::from_dom_lines(&doc, &path.to_string_lossy(), &Keep, Some(&lines))
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// A stylesheet element in context.
#[derive(Clone, Copy)]
struct El<'a> {
    t: &'a Tree,
    i: u32,
    file: &'a str,
}

impl<'a> El<'a> {
    /// The declaration's file, line and identifying attributes.
    fn source(&self) -> Source {
        let file = self.file.strip_prefix(r"\\?\").unwrap_or(self.file);
        Source {
            file: file.to_string(),
            line: self.t.lines.get(self.i as usize).copied().unwrap_or(0),
            name: self.attr("name").map(str::to_string),
            pattern: self.attr("match").map(str::to_string),
            mode: self.attr("mode").map(str::to_string),
        }
    }

    fn name(&self) -> &QName {
        self.t.node(self.i).name.as_ref().unwrap()
    }
    fn is_xsl(&self, local: &str) -> bool {
        let n = self.name();
        &*n.uri == XSL_NS && &*n.local == local
    }
    fn xsl_local(&self) -> Option<&str> {
        let n = self.name();
        (&*n.uri == XSL_NS).then_some(&*n.local)
    }
    fn attr(&self, local: &str) -> Option<&'a str> {
        self.attr_ns("", local)
    }
    fn attr_ns(&self, uri: &str, local: &str) -> Option<&'a str> {
        self.t
            .node(self.i)
            .attrs
            .iter()
            .map(|&a| self.t.node(a))
            .find_map(|a| {
                let n = a.name.as_ref().unwrap();
                (&*n.uri == uri && &*n.local == local).then_some(a.value.as_str())
            })
    }
    fn children(&self) -> impl Iterator<Item = u32> + 'a {
        self.t.node(self.i).children.clone().into_iter()
    }
    fn child_els(&self) -> impl Iterator<Item = El<'a>> + 'a {
        let (t, file) = (self.t, self.file);
        self.children()
            .filter(move |&c| t.node(c).kind == Kind::Element)
            .map(move |c| El { t, i: c, file })
    }
    fn resolver(&self) -> impl Fn(&str) -> Option<String> + 'a {
        let (t, i) = (self.t, self.i);
        let xdn = self.inherited("xpath-default-namespace");
        move |p: &str| {
            if p.is_empty() {
                return xdn.clone().filter(|s| !s.is_empty());
            }
            t.resolve(i, p).map(|u| u.to_string())
        }
    }
    /// An attribute on this element or the nearest ancestor (`xsl:`-prefixed
    /// on literal result elements).
    fn inherited(&self, local: &str) -> Option<String> {
        let mut cur = Some(self.i);
        while let Some(c) = cur {
            let e = El {
                t: self.t,
                i: c,
                file: self.file,
            };
            if self.t.node(c).kind == Kind::Element {
                let v = if &*e.name().uri == XSL_NS {
                    e.attr(local)
                } else {
                    e.attr_ns(XSL_NS, local)
                };
                if let Some(v) = v {
                    return Some(v.to_string());
                }
            }
            cur = self.t.node(c).parent;
        }
        None
    }
    fn expr(&self, src: &str) -> Result<Expr, String> {
        xpath::parse(src, &self.resolver()).map_err(|e| self.err(&e))
    }
    fn opt_expr(&self, attr: &str) -> Result<Option<Expr>, String> {
        self.attr(attr).map(|s| self.expr(s)).transpose()
    }
    fn req(&self, attr: &str) -> Result<&'a str, String> {
        self.attr(attr)
            .ok_or_else(|| self.err(&format!("missing '{attr}' attribute")))
    }
    fn qname(&self, lexical: &str) -> Result<QName, String> {
        let lexical = lexical.trim();
        let (p, l) = tree::split(lexical);
        if p.is_empty() {
            return Ok(QName::local(l));
        }
        let u = self
            .t
            .resolve(self.i, p)
            .ok_or_else(|| self.err(&format!("undeclared prefix '{p}'")))?;
        Ok(QName::new(&u, l, p))
    }
    fn avt(&self, src: &str) -> Result<Avt, String> {
        parse_avt(src, &self.resolver()).map_err(|e| self.err(&e))
    }
    fn err(&self, msg: &str) -> String {
        format!("{} ({}): {msg}", self.file, self.name().lexical())
    }
    fn seq_type(&self) -> Result<Option<SeqType>, String> {
        self.attr("as")
            .map(|s| xpath::parse_seq_type(s, &self.resolver()).map_err(|e| self.err(&e)))
            .transpose()
    }
    fn qnames(&self, attr: &str) -> Result<Vec<QName>, String> {
        self.attr(attr)
            .map(|s| s.split_whitespace().map(|n| self.qname(n)).collect())
            .unwrap_or(Ok(Vec::new()))
    }
}

pub fn parse_avt(src: &str, ns: xpath::Resolver) -> Result<Avt, String> {
    if !src.contains('{') && !src.contains('}') {
        return Ok(Avt::Lit(src.to_string()));
    }
    let c: Vec<char> = src.chars().collect();
    let mut parts = Vec::new();
    let mut lit = String::new();
    let mut i = 0;
    while i < c.len() {
        match c[i] {
            '{' if c.get(i + 1) == Some(&'{') => {
                lit.push('{');
                i += 2;
            }
            '}' if c.get(i + 1) == Some(&'}') => {
                lit.push('}');
                i += 2;
            }
            '{' => {
                let mut j = i + 1;
                let mut quote = None;
                while j < c.len() {
                    match (quote, c[j]) {
                        (None, '"' | '\'') => quote = Some(c[j]),
                        (Some(q), ch) if ch == q => quote = None,
                        (None, '}') => break,
                        _ => {}
                    }
                    j += 1;
                }
                if j >= c.len() {
                    return Err(format!("unclosed '{{' in '{src}'"));
                }
                if !lit.is_empty() {
                    parts.push(AvtPart::Lit(std::mem::take(&mut lit)));
                }
                let e: String = c[i + 1..j].iter().collect();
                parts.push(AvtPart::Expr(xpath::parse(&e, ns)?));
                i = j + 1;
            }
            '}' => return Err(format!("unmatched '}}' in '{src}'")),
            ch => {
                lit.push(ch);
                i += 1;
            }
        }
    }
    if !lit.is_empty() {
        parts.push(AvtPart::Lit(lit));
    }
    Ok(Avt::Parts(parts))
}

/// Splits a pattern into its union alternatives.
fn alternatives(e: Expr, out: &mut Vec<Expr>) {
    match e {
        Expr::Bin(BinOp::Union, a, b) => {
            alternatives(*a, out);
            alternatives(*b, out);
        }
        e => out.push(e),
    }
}

/// Default priority of a pattern alternative (XSLT 2.0 §6.4).
fn default_priority(e: &Expr) -> f64 {
    use crate::xpath::{Axis, NodeTest, PathStep};
    let Expr::Path { start: None, steps } = e else {
        return 0.5;
    };
    if steps.len() != 1 {
        return 0.5;
    }
    let PathStep::Axis(s) = &steps[0] else {
        return 0.5;
    };
    if !s.preds.is_empty() || !matches!(s.axis, Axis::Child | Axis::Attribute) {
        return 0.5;
    }
    match &s.test {
        NodeTest::Name {
            local: Some(_),
            uri: Some(_),
        } => 0.0,
        NodeTest::Name {
            local: None,
            uri: Some(_),
        }
        | NodeTest::Name {
            local: Some(_),
            uri: None,
        } => -0.25,
        NodeTest::Name { .. } => -0.5,
        NodeTest::Element(Some(_)) | NodeTest::Attribute(Some(_)) | NodeTest::Pi(Some(_)) => 0.0,
        _ => -0.5,
    }
}

impl Compiler {
    fn module(&mut self, path: &Path, prec_out: Option<&mut usize>) -> Result<(), String> {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if self.loading.contains(&path) {
            return Err(format!("{} includes itself", path.display()));
        }
        self.loading.push(path.clone());
        if self.loading.len() > 1 {
            self.ss.extra_modules += 1;
        }
        let tree = load_tree(&path)?;
        let file = path.to_string_lossy().to_string();
        let root = tree
            .node(0)
            .children
            .iter()
            .copied()
            .find(|&c| tree.node(c).kind == Kind::Element);
        let root = root.ok_or_else(|| format!("{file}: no root element"))?;
        let el = El {
            t: &tree,
            i: root,
            file: &file,
        };
        if !(el.is_xsl("stylesheet") || el.is_xsl("transform")) {
            // A simplified stylesheet: the literal result element is the
            // body of a template rule matching the document node.
            if self.ss.version.is_empty() {
                let v = el.attr_ns(XSL_NS, "version");
                let v = v.ok_or_else(|| format!("{file}: XTSE0150: no xsl:version"))?;
                self.ss.version = v.to_string();
            }
            let prec = self.next_prec;
            self.next_prec += 1;
            let id = self.ss.templates.len();
            let body = vec![self.lre(el)?];
            self.ss.templates.push(Template {
                id,
                matches: vec![(Expr::Root, -0.5)],
                name: None,
                modes: vec![None],
                all_modes: false,
                params: Vec::new(),
                body,
                as_: None,
                prec,
                location: file.clone(),
                source: el.source(),
            });
            if let Some(p) = prec_out {
                *p = prec;
            }
            self.loading.pop();
            return Ok(());
        }
        if self.ss.version.is_empty() {
            self.ss.version = el.attr("version").unwrap_or("1.0").to_string();
        }
        // Imports first: each gets a lower precedence than this module.
        for d in el.child_els() {
            if d.is_xsl("import") {
                let href = path.parent().unwrap().join(d.req("href")?);
                self.module(&href, None)?;
            }
        }
        let prec = self.next_prec;
        self.next_prec += 1;
        self.declarations(el, &path, prec)?;
        if let Some(p) = prec_out {
            *p = prec;
        }
        self.loading.pop();
        Ok(())
    }

    fn declarations(&mut self, el: El, path: &Path, prec: usize) -> Result<(), String> {
        for d in el.child_els() {
            let Some(local) = d.xsl_local() else { continue };
            match local {
                "import" | "decimal-format" | "namespace-alias" | "character-map" => {}
                "output" => {
                    let o = &mut self.ss.output;
                    let get = |a: &str| d.attr(a).map(|v| v.trim().to_string());
                    if let Some(v) = get("method") {
                        o.method = Some(v);
                    }
                    if let Some(v) = get("version") {
                        o.version = Some(v);
                    }
                    if let Some(v) = get("encoding") {
                        o.encoding = Some(v);
                    }
                    if let Some(v) = get("doctype-public") {
                        o.doctype_public = Some(v);
                    }
                    if let Some(v) = get("doctype-system") {
                        o.doctype_system = Some(v);
                    }
                    if let Some(v) = get("indent") {
                        o.indent = v == "yes";
                    }
                    if let Some(v) = get("omit-xml-declaration") {
                        o.omit_declaration = v == "yes";
                    }
                    if let Some(v) = get("media-type") {
                        o.media_type = Some(v);
                    }
                }
                "include" => {
                    let href = path.parent().unwrap().join(d.req("href")?);
                    let href = std::fs::canonicalize(&href).unwrap_or(href);
                    if self.loading.contains(&href) {
                        return Err(format!("{} includes itself", href.display()));
                    }
                    self.loading.push(href.clone());
                    self.ss.extra_modules += 1;
                    let tree = load_tree(&href)?;
                    let file = href.to_string_lossy().to_string();
                    let root = tree
                        .node(0)
                        .children
                        .iter()
                        .copied()
                        .find(|&c| tree.node(c).kind == Kind::Element);
                    let inc = El {
                        t: &tree,
                        i: root.ok_or("empty include")?,
                        file: &file,
                    };
                    for i in inc.child_els() {
                        if i.is_xsl("import") {
                            return Err(
                                i.err("xsl:import inside an included module is not supported yet")
                            );
                        }
                    }
                    self.declarations(inc, &href, prec)?;
                    self.loading.pop();
                }
                "template" => self.template(d, prec)?,
                "function" => {
                    let name = d.qname(d.req("name")?)?;
                    let (params, body) = self.params_and_body(d)?;
                    let arity = params.len();
                    let id = self.ss.functions.len();
                    let source = d.source();
                    self.ss.functions.insert(
                        (name.clone(), arity),
                        Function {
                            id,
                            source,
                            name,
                            params,
                            body,
                            as_: d.seq_type()?,
                        },
                    );
                }
                "variable" | "param" => {
                    let p = self.param(d)?;
                    // Later (higher precedence) declarations replace earlier.
                    self.ss.globals.retain(|(g, _)| !g.name.same(&p.name));
                    self.ss.globals.push((p, local == "param"));
                }
                "key" => {
                    let name = d.qname(d.req("name")?)?;
                    let mut m = Vec::new();
                    alternatives(d.expr(d.req("match")?)?, &mut m);
                    let k = Key {
                        matches: m,
                        use_: d.expr(d.req("use")?)?,
                    };
                    self.ss.keys.entry(name).or_default().push(k);
                }
                "attribute-set" => {
                    let name = d.qname(d.req("name")?)?;
                    let set = AttrSet {
                        uses: d.qnames("use-attribute-sets")?,
                        body: self.body(d)?,
                    };
                    self.ss.attr_sets.entry(name).or_default().push(set);
                }
                "strip-space" | "preserve-space" => {
                    for n in d.req("elements")?.split_whitespace() {
                        let (uri, local_) = if n == "*" {
                            (None, None)
                        } else if let Some(p) = n.strip_suffix(":*") {
                            (
                                Some(
                                    d.t.resolve(d.i, p)
                                        .map(|u| u.to_string())
                                        .unwrap_or_default(),
                                ),
                                None,
                            )
                        } else {
                            let q = d.qname(n)?;
                            (Some(q.uri.to_string()), Some(q.local.to_string()))
                        };
                        self.ss.strip.push((uri, local_, local == "strip-space"));
                    }
                }
                other => return Err(d.err(&format!("xsl:{other} is not supported yet"))),
            }
        }
        Ok(())
    }

    fn template(&mut self, d: El, prec: usize) -> Result<(), String> {
        let id = self.ss.templates.len();
        let mut matches = Vec::new();
        if let Some(m) = d.attr("match") {
            let mut alts = Vec::new();
            alternatives(d.expr(m)?, &mut alts);
            let explicit = d
                .attr("priority")
                .map(|p| p.trim().parse::<f64>())
                .transpose();
            let explicit = explicit.map_err(|_| d.err("bad priority"))?;
            for a in alts {
                let p = explicit.unwrap_or_else(|| default_priority(&a));
                matches.push((a, p));
            }
        }
        let name = d.attr("name").map(|n| d.qname(n)).transpose()?;
        let mut modes = Vec::new();
        let mut all_modes = false;
        match d.attr("mode") {
            None => modes.push(None),
            Some(m) => {
                for t in m.split_whitespace() {
                    match t {
                        "#all" => all_modes = true,
                        "#default" => modes.push(None),
                        n => modes.push(Some(d.qname(n)?)),
                    }
                }
            }
        }
        let (params, body) = self.params_and_body(d)?;
        if let Some(n) = &name {
            // A higher-precedence named template replaces a lower one.
            let replace = self
                .ss
                .named
                .get(n)
                .is_none_or(|&o| self.ss.templates[o].prec <= prec);
            if replace {
                self.ss.named.insert(n.clone(), id);
            }
        }
        let location = format!(
            "{}#{}",
            Path::new(d.file)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default(),
            d.attr("name").or(d.attr("match")).unwrap_or("")
        );
        self.ss.templates.push(Template {
            id,
            matches,
            name,
            modes,
            all_modes,
            params,
            body,
            as_: d.seq_type()?,
            prec,
            location,
            source: d.source(),
        });
        Ok(())
    }

    fn params_and_body(&mut self, d: El) -> Result<(Vec<Param>, Vec<Instr>), String> {
        let mut params = Vec::new();
        for c in d.child_els() {
            if c.is_xsl("param") {
                params.push(self.param(c)?);
            }
        }
        Ok((params, self.body(d)?))
    }

    fn param(&mut self, d: El) -> Result<Param, String> {
        Ok(Param {
            name: d.qname(d.req("name")?)?,
            select: d.opt_expr("select")?,
            body: self.body(d)?,
            as_: d.seq_type()?,
            tunnel: d.attr("tunnel").is_some_and(|t| t.trim() == "yes"),
        })
    }

    fn with_params(&mut self, d: El) -> Result<Vec<Param>, String> {
        d.child_els()
            .filter(|c| c.is_xsl("with-param"))
            .map(|c| self.param(c))
            .collect()
    }

    fn sorts(&mut self, d: El) -> Result<Vec<Sort>, String> {
        let mut v = Vec::new();
        for c in d.child_els().filter(|c| c.is_xsl("sort")) {
            let avt = |a: &str| c.attr(a).map(|s| c.avt(s)).transpose();
            v.push(Sort {
                select: c.opt_expr("select")?,
                order: avt("order")?,
                data_type: avt("data-type")?,
                case_order: avt("case-order")?,
            });
        }
        Ok(v)
    }

    /// The sequence constructor inside `d`, skipping `xsl:param`,
    /// `xsl:sort` and `xsl:with-param`.
    fn body(&mut self, d: El) -> Result<Vec<Instr>, String> {
        let mut out = Vec::new();
        let preserve_text = d.is_xsl("text");
        for c in d.children() {
            let n = d.t.node(c);
            match n.kind {
                Kind::Text => {
                    let keep = preserve_text
                        || !n.value.chars().all(tree::is_xml_space)
                        || d.inherited_space_preserve(c);
                    if keep {
                        out.push(Instr::Text(n.value.clone()));
                    }
                }
                Kind::Element => {
                    let e = El {
                        t: d.t,
                        i: c,
                        file: d.file,
                    };
                    if let Some(i) = self.instr(e)? {
                        out.push(i);
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn instr(&mut self, e: El) -> Result<Option<Instr>, String> {
        let Some(local) = e.xsl_local() else {
            return self.lre(e).map(Some);
        };
        let scope = || Rc::new(e.t.in_scope(e.i));
        Ok(Some(match local {
            "param" | "sort" | "with-param" | "fallback" => return Ok(None),
            "text" => Instr::Text(e.t.string_value(e.i)),
            "value-of" => Instr::ValueOf {
                select: e.opt_expr("select")?,
                sep: e.attr("separator").map(|s| e.avt(s)).transpose()?,
                body: self.body(e)?,
            },
            "apply-templates" => Instr::ApplyTemplates {
                select: e.opt_expr("select")?,
                mode: match e.attr("mode").map(str::trim) {
                    None | Some("#default") => ModeRef::Default,
                    Some("#current") => ModeRef::Current,
                    Some(m) => ModeRef::Named(e.qname(m)?),
                },
                params: self.with_params(e)?,
                sorts: self.sorts(e)?,
            },
            "perform-sort" => Instr::PerformSort {
                select: e.opt_expr("select")?,
                sorts: self.sorts(e)?,
                body: self.body(e)?,
            },
            "document" => Instr::Document(self.body(e)?),
            "for-each-group" => {
                let pick = [
                    ("group-by", GroupBy::By),
                    ("group-adjacent", GroupBy::Adjacent),
                    ("group-starting-with", GroupBy::StartingWith),
                    ("group-ending-with", GroupBy::EndingWith),
                ];
                let mut found = pick
                    .into_iter()
                    .filter_map(|(a, g)| e.attr(a).map(|v| (v.to_string(), g)));
                let (Some((v, how)), None) = (found.next(), found.next()) else {
                    return Err(
                        e.err("XTSE1080: xsl:for-each-group needs exactly one grouping attribute")
                    );
                };
                let key = match how {
                    GroupBy::By | GroupBy::Adjacent => e.expr(&v)?,
                    _ => e.expr(&v)?,
                };
                Instr::ForEachGroup {
                    select: e.expr(e.req("select")?)?,
                    how,
                    key,
                    sorts: self.sorts(e)?,
                    body: self.body(e)?,
                }
            }
            "next-match" | "apply-imports" => Instr::NextMatch {
                params: self.with_params(e)?,
                imports: local == "apply-imports",
            },
            "call-template" => Instr::CallTemplate {
                name: e.qname(e.req("name")?)?,
                params: self.with_params(e)?,
            },
            "variable" => Instr::Variable(self.param(e)?),
            "choose" => {
                let mut whens = Vec::new();
                let mut other = Vec::new();
                for c in e.child_els() {
                    if c.is_xsl("when") {
                        whens.push((c.expr(c.req("test")?)?, self.body(c)?));
                    } else if c.is_xsl("otherwise") {
                        other = self.body(c)?;
                    }
                }
                Instr::Choose(whens, other)
            }
            "if" => Instr::If(e.expr(e.req("test")?)?, self.body(e)?),
            "analyze-string" => {
                let mut matching = Vec::new();
                let mut non_matching = Vec::new();
                for c in e.child_els() {
                    if c.is_xsl("matching-substring") {
                        matching = self.body(c)?;
                    } else if c.is_xsl("non-matching-substring") {
                        non_matching = self.body(c)?;
                    }
                }
                Instr::AnalyzeString {
                    select: e.expr(e.req("select")?)?,
                    regex: e.avt(e.req("regex")?)?,
                    flags: e.attr("flags").map(|s| e.avt(s)).transpose()?,
                    matching,
                    non_matching,
                }
            }
            "for-each" => Instr::ForEach {
                select: e.expr(e.req("select")?)?,
                sorts: self.sorts(e)?,
                body: self.body(e)?,
            },
            "attribute" => Instr::Attribute {
                name: e.avt(e.req("name")?)?,
                namespace: e.attr("namespace").map(|s| e.avt(s)).transpose()?,
                scope: scope(),
                select: e.opt_expr("select")?,
                sep: e.attr("separator").map(|s| e.avt(s)).transpose()?,
                body: self.body(e)?,
            },
            "element" => Instr::Element {
                name: e.avt(e.req("name")?)?,
                namespace: e.attr("namespace").map(|s| e.avt(s)).transpose()?,
                scope: scope(),
                sets: e.qnames("use-attribute-sets")?,
                body: self.body(e)?,
            },
            "copy-of" => Instr::CopyOf(e.expr(e.req("select")?)?),
            "copy" => Instr::Copy {
                sets: e.qnames("use-attribute-sets")?,
                body: self.body(e)?,
            },
            "sequence" => Instr::Sequence(e.expr(e.req("select")?)?),
            "comment" => Instr::Comment {
                select: e.opt_expr("select")?,
                body: self.body(e)?,
            },
            "processing-instruction" => Instr::Pi {
                name: e.avt(e.req("name")?)?,
                select: e.opt_expr("select")?,
                body: self.body(e)?,
            },
            "result-document" => Instr::ResultDocument {
                href: e.attr("href").map(|s| e.avt(s)).transpose()?,
                body: self.body(e)?,
            },
            "message" => Instr::Message {
                select: e.opt_expr("select")?,
                body: self.body(e)?,
                terminate: e.attr("terminate").is_some_and(|t| t.trim() == "yes"),
            },
            "number" => {
                let pat = |a: &str| -> Result<Option<Vec<Expr>>, String> {
                    e.attr(a)
                        .map(|s| {
                            let mut v = Vec::new();
                            alternatives(e.expr(s)?, &mut v);
                            Ok(v)
                        })
                        .transpose()
                };
                Instr::Number(Box::new(Number {
                    level: e.attr("level").unwrap_or("single").trim().to_string(),
                    count: pat("count")?,
                    from: pat("from")?,
                    value: e.opt_expr("value")?,
                    format: e.avt(e.attr("format").unwrap_or("1"))?,
                    select: e.opt_expr("select")?,
                }))
            }
            other => return Err(e.err(&format!("xsl:{other} is not supported yet"))),
        }))
    }

    /// A literal result element: its name, the namespaces it copies to the
    /// result (all in scope, less XSLT's and excluded prefixes) and AVTs.
    fn lre(&mut self, e: El) -> Result<Instr, String> {
        let mut excluded: Vec<String> = Vec::new();
        let mut cur = Some(e.i);
        while let Some(c) = cur {
            let el = El {
                t: e.t,
                i: c,
                file: e.file,
            };
            if e.t.node(c).kind == Kind::Element {
                let v = if &*el.name().uri == XSL_NS {
                    el.attr("exclude-result-prefixes")
                } else {
                    el.attr_ns(XSL_NS, "exclude-result-prefixes")
                };
                if let Some(v) = v {
                    excluded.extend(v.split_whitespace().map(String::from));
                }
            }
            cur = e.t.node(c).parent;
        }
        let all = excluded.iter().any(|p| p == "#all");
        let ns: Vec<_> =
            e.t.in_scope(e.i)
                .into_iter()
                .filter(|(p, u)| {
                    &**u != XSL_NS
                        && !all
                        && !excluded
                            .iter()
                            .any(|x| x == &**p || (x == "#default" && p.is_empty()))
                })
                .collect();
        let mut attrs = Vec::new();
        for &a in &e.t.node(e.i).attrs {
            let an = e.t.node(a);
            let name = an.name.clone().unwrap();
            if &*name.uri == XSL_NS {
                continue;
            }
            attrs.push((name, e.avt(&an.value)?));
        }
        Ok(Instr::Lre {
            name: e.name().clone(),
            ns,
            attrs,
            sets: e
                .attr_ns(XSL_NS, "use-attribute-sets")
                .map(|s| s.split_whitespace().map(|n| e.qname(n)).collect())
                .unwrap_or(Ok(Vec::new()))?,
            body: self.body(e)?,
        })
    }
}

impl El<'_> {
    fn inherited_space_preserve(&self, _text: u32) -> bool {
        let mut cur = Some(self.i);
        while let Some(c) = cur {
            let n = self.t.node(c);
            for &a in &n.attrs {
                let an = self.t.node(a);
                let q = an.name.as_ref().unwrap();
                if &*q.uri == tree::XML_NS && &*q.local == "space" {
                    return an.value == "preserve";
                }
            }
            cur = n.parent;
        }
        false
    }
}
