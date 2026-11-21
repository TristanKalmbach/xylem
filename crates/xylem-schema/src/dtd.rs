//! XML DTD loader with parameter entities, external entity files and
//! conditional sections.
//!
//! Declarations are processed in document order and the first declaration
//! of an entity or of an attribute wins, so an internal subset processed
//! before the external subset overrides it (the usual S1000D customisation
//! pattern: `<!ENTITY % draft "INCLUDE">` in the instance).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::datatype::{TypeId, is_name_char, is_name_start};
use crate::error::{Result, read_file};
use crate::model::{Model, Particle, Term};
use crate::resolve::location_to_path;
use crate::{
    AttrDecl, ContentType, DeclId, ElemType, ElementDecl, EntityDecl, EntityKind, Grammar,
    GrammarKind, NotationDecl, attr_kind,
};

const MAX_DEPTH: usize = 64;

pub(crate) fn load_file(path: &Path, catalog: &Catalog) -> Result<Grammar> {
    let text = decode(&read_file(path)?);
    let base = path.parent().unwrap_or(Path::new("."));
    let mut p = DtdParser::new(catalog);
    p.process(strip_text_decl(&text), base, 0);
    Ok(p.into_grammar(None))
}

pub(crate) fn load_text(text: &str, base: &Path, catalog: &Catalog) -> Result<Grammar> {
    let mut p = DtdParser::new(catalog);
    p.process(strip_text_decl(text), base, 0);
    Ok(p.into_grammar(None))
}

/// Loads the grammar of a DOCTYPE: internal subset first, then the external one.
pub(crate) fn load_doctype(
    internal: Option<&str>,
    external: Option<&Path>,
    base: &Path,
    catalog: &Catalog,
    root: &str,
) -> Result<Grammar> {
    let mut p = DtdParser::new(catalog);
    if let Some(subset) = internal {
        p.process(subset, base, 0);
    }
    if let Some(ext) = external {
        let text = decode(&read_file(ext)?);
        p.process(strip_text_decl(&text), ext.parent().unwrap_or(base), 0);
    }
    Ok(p.into_grammar(Some(root)))
}

/// Merges a DOCTYPE internal subset into an existing (XSD) grammar: general
/// entities, notations and additional attributes of known elements.
pub(crate) fn merge_internal_subset(g: &mut Grammar, subset: &str, base: &Path, catalog: &Catalog) {
    let mut p = DtdParser::new(catalog);
    p.process(subset, base, 0);
    g.warnings.append(&mut p.warnings);
    for e in p.entities {
        if g.entity(&e.name).is_none() {
            g.entities.push(e);
        }
    }
    for n in p.notations {
        if !g.notations.iter().any(|x| x.name == n.name) {
            g.notations.push(n);
        }
    }
    for (elem, raw) in std::mem::take(&mut p.attlists) {
        let Some(decl) = g.global_decl(&elem) else {
            g.warnings
                .push(format!("ATTLIST for undeclared element '{elem}' ignored"));
            continue;
        };
        let attr = raw.into_decl(&mut g.simple);
        let mut ty = g.decl(decl).ty;
        // Types are shared between declarations: give this element its own copy.
        if g.decls.iter().filter(|d| d.ty == ty).count() > 1 {
            let copy = g.types[ty].clone();
            g.types.push(copy);
            ty = g.types.len() - 1;
            g.decls[decl.0 as usize].ty = ty;
        }
        let attrs = &mut g.types[ty].attrs;
        if !attrs.iter().any(|a| a.name == attr.name) {
            attrs.push(attr);
        }
    }
}

fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// Removes a leading `<?xml ...?>` text declaration.
fn strip_text_decl(text: &str) -> &str {
    let t = text.trim_start_matches('\u{FEFF}');
    if t.starts_with("<?xml")
        && t[5..].starts_with(|c: char| c.is_whitespace())
        && let Some(end) = t.find("?>")
    {
        return &t[end + 2..];
    }
    t
}

#[derive(Clone, Debug)]
enum PeValue {
    Text {
        text: String,
        base: PathBuf,
    },
    External {
        public: Option<String>,
        system: String,
        base: PathBuf,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum Cp {
    Name(String),
    Seq(Vec<Cp>),
    Choice(Vec<Cp>),
    Occurs(Box<Cp>, char),
}

#[derive(Clone, Debug, PartialEq)]
enum ContentSpec {
    Empty,
    Any,
    /// `(#PCDATA | a | b)*`; no names means `(#PCDATA)`.
    Mixed(Vec<String>),
    Children(Cp),
}

#[derive(Clone, Debug)]
enum RawType {
    Cdata,
    Builtin(&'static str),
    Enumeration(Vec<String>),
    Notation(Vec<String>),
}

#[derive(Clone, Debug)]
struct RawAttr {
    name: String,
    ty: RawType,
    required: bool,
    default: Option<String>,
    fixed: Option<String>,
}

impl RawAttr {
    fn into_decl(self, types: &mut crate::datatype::TypeTable) -> AttrDecl {
        let (datatype, type_name, enum_values): (TypeId, String, Vec<String>) = match &self.ty {
            RawType::Cdata => (types.string(), "CDATA".to_string(), Vec::new()),
            RawType::Builtin(n) => (
                types.builtin(n).unwrap_or(types.string()),
                n.to_string(),
                Vec::new(),
            ),
            RawType::Enumeration(values) | RawType::Notation(values) => {
                let notation = matches!(self.ty, RawType::Notation(_));
                let base = types
                    .builtin(if notation { "NOTATION" } else { "NMTOKEN" })
                    .unwrap_or(types.string());
                let mut st = types.restrict(base);
                st.facets.set_enumeration(values.clone());
                let label = format!(
                    "{}({})",
                    if notation { "NOTATION " } else { "" },
                    values.join("|")
                );
                st.name = Some(label.clone());
                (types.add(st), label, values.clone())
            }
        };
        let local_name = self.name.clone();
        AttrDecl {
            kind: attr_kind(types, datatype),
            name: self.name,
            local_name,
            namespace: None,
            datatype,
            type_name,
            required: self.required,
            default: self.default,
            fixed: self.fixed,
            enum_values,
            documentation: None,
        }
    }
}

struct DtdParser<'c> {
    catalog: &'c Catalog,
    pes: HashMap<String, PeValue>,
    elements: Vec<(String, ContentSpec, Option<String>)>,
    element_index: HashMap<String, usize>,
    attlists: Vec<(String, RawAttr)>,
    attr_seen: HashSet<(String, String)>,
    entities: Vec<EntityDecl>,
    notations: Vec<NotationDecl>,
    warnings: Vec<String>,
    last_comment: Option<String>,
}

/// A tiny cursor over declaration text.
struct Lex<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Lex<'a> {
    fn new(s: &'a str) -> Self {
        Lex { s, pos: 0 }
    }
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }
    fn ws(&mut self) {
        let r = self.rest();
        self.pos += r.len() - r.trim_start().len();
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn eat(&mut self, c: char) -> bool {
        self.ws();
        if self.peek() == Some(c) {
            self.pos += c.len_utf8();
            true
        } else {
            false
        }
    }
    fn word(&mut self) -> Option<&'a str> {
        self.ws();
        let r = self.rest();
        let mut it = r.char_indices();
        let first = it.next()?.1;
        if !(is_name_char(first) || first == '#') {
            return None;
        }
        let end = it
            .find(|&(_, c)| !is_name_char(c))
            .map(|(i, _)| i)
            .unwrap_or(r.len());
        self.pos += end;
        Some(&r[..end])
    }
    fn quoted(&mut self) -> Option<&'a str> {
        self.ws();
        let q = self.peek().filter(|&c| c == '"' || c == '\'')?;
        let r = &self.rest()[1..];
        let end = r.find(q)?;
        self.pos += end + 2;
        Some(&r[..end])
    }
    fn at_end(&mut self) -> bool {
        self.ws();
        self.pos >= self.s.len()
    }
}

/// Finds the end of a markup declaration starting at `start` (`<!`),
/// skipping quoted literals. Returns the index just after `>`.
fn decl_end(text: &str, start: usize) -> Option<usize> {
    let b = text.as_bytes();
    let mut i = start + 2;
    let mut quote = 0u8;
    while i < b.len() {
        let c = b[i];
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
        } else if c == b'"' || c == b'\'' {
            quote = c;
        } else if c == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// Finds the `]]>` closing a conditional section whose body starts at `start`.
fn cond_end(text: &str, start: usize) -> Option<(usize, usize)> {
    let mut depth = 1;
    let mut i = start;
    while i < text.len() {
        let r = &text[i..];
        if r.starts_with("<![") {
            depth += 1;
            i += 3;
        } else if r.starts_with("]]>") {
            depth -= 1;
            if depth == 0 {
                return Some((i, i + 3));
            }
            i += 3;
        } else if r.starts_with("<!--") {
            i += r.find("-->").map(|e| e + 3).unwrap_or(r.len());
        } else {
            i += r.chars().next().map_or(1, char::len_utf8);
        }
    }
    None
}

/// Decodes character references and predefined entities in a literal.
fn decode_refs(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';') else {
            out.push_str(tail);
            return out;
        };
        let name = &tail[1..semi];
        let ch = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "apos" => Some('\''),
            "quot" => Some('"'),
            n if n.starts_with("#x") => u32::from_str_radix(&n[2..], 16)
                .ok()
                .and_then(char::from_u32),
            n if n.starts_with('#') => n[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=semi]),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

impl<'c> DtdParser<'c> {
    fn new(catalog: &'c Catalog) -> Self {
        DtdParser {
            catalog,
            pes: HashMap::new(),
            elements: Vec::new(),
            element_index: HashMap::new(),
            attlists: Vec::new(),
            attr_seen: HashSet::new(),
            entities: Vec::new(),
            notations: Vec::new(),
            warnings: Vec::new(),
            last_comment: None,
        }
    }

    fn warn(&mut self, msg: String) {
        if !self.warnings.contains(&msg) {
            self.warnings.push(msg);
        }
    }

    /// Processes a sequence of DTD constructs.
    fn process(&mut self, text: &str, base: &Path, depth: usize) {
        if depth > MAX_DEPTH {
            self.warn("parameter entities nested too deeply".into());
            return;
        }
        let mut i = 0;
        while i < text.len() {
            let r = &text[i..];
            let c = r.chars().next().unwrap_or(' ');
            if c.is_whitespace() {
                i += c.len_utf8();
            } else if let Some(after_open) = r.strip_prefix("<!--") {
                let (body, end) = match after_open.find("-->") {
                    Some(e) => (&after_open[..e], e + 7),
                    None => (after_open, r.len()),
                };
                let body = body.trim();
                self.last_comment = (!body.is_empty()).then(|| body.to_string());
                i += end;
            } else if r.starts_with("<?") {
                i += r.find("?>").map(|e| e + 2).unwrap_or(r.len());
                self.last_comment = None;
            } else if let Some(after_marker) = r.strip_prefix("<![") {
                self.last_comment = None;
                let Some(open) = after_marker.find('[').map(|o| o + 3) else {
                    self.warn("unterminated conditional section".into());
                    return;
                };
                let keyword = self.expand(&r[3..open], depth).trim().to_string();
                let Some((body_end, after)) = cond_end(r, open + 1) else {
                    self.warn("unterminated conditional section".into());
                    return;
                };
                match keyword.as_str() {
                    "INCLUDE" => {
                        let body = r[open + 1..body_end].to_string();
                        self.process(&body, base, depth + 1);
                    }
                    "IGNORE" => {}
                    other => self.warn(format!(
                        "unknown conditional section keyword '{other}' (treated as IGNORE)"
                    )),
                }
                i += after;
            } else if r.starts_with("<!") {
                let Some(end) = decl_end(r, 0) else {
                    self.warn("unterminated markup declaration".into());
                    return;
                };
                let body = r[2..end - 1].to_string();
                self.declaration(&body, base, depth);
                i += end;
            } else if c == '%' {
                let semi = r.find(';');
                match semi {
                    Some(semi) if r[1..semi].chars().all(is_name_char) && semi > 1 => {
                        let name = r[1..semi].to_string();
                        if let Some((text, pe_base)) = self.pe_text(&name) {
                            self.process(strip_text_decl(&text), &pe_base, depth + 1);
                        }
                        i += semi + 1;
                    }
                    _ => i += 1,
                }
            } else {
                self.warn(format!("unexpected character '{c}' in DTD"));
                i += c.len_utf8();
            }
        }
    }

    /// The replacement text of a parameter entity and the base for its references.
    fn pe_text(&mut self, name: &str) -> Option<(String, PathBuf)> {
        let Some(v) = self.pes.get(name).cloned() else {
            self.warn(format!("undeclared parameter entity '%{name};'"));
            return None;
        };
        match v {
            PeValue::Text { text, base } => Some((text, base)),
            PeValue::External {
                public,
                system,
                base,
            } => {
                let path = self
                    .catalog
                    .resolve(public.as_deref(), Some(&system))
                    .or_else(|| location_to_path(&system, Some(&base)).filter(|p| p.exists()))
                    .or_else(|| {
                        // Fall back to a same-named file next to the declaring entity.
                        let file = system.rsplit(['/', '\\']).next()?;
                        Some(base.join(file)).filter(|p| p.exists())
                    });
                let Some(path) = path else {
                    self.warn(format!(
                        "cannot resolve external parameter entity '%{name};' ('{system}')"
                    ));
                    return None;
                };
                match read_file(&path) {
                    Ok(bytes) => {
                        let text = decode(&bytes);
                        let pe_base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
                        // Cache as text so repeated references do not re-read the file.
                        self.pes.insert(
                            name.to_string(),
                            PeValue::Text {
                                text: text.clone(),
                                base: pe_base.clone(),
                            },
                        );
                        Some((text, pe_base))
                    }
                    Err(e) => {
                        self.warn(e.to_string());
                        None
                    }
                }
            }
        }
    }

    /// Expands parameter-entity references outside quoted literals, padding
    /// each replacement with spaces.
    fn expand(&mut self, s: &str, depth: usize) -> String {
        if !s.contains('%') || depth > MAX_DEPTH {
            return s.to_string();
        }
        let mut out = String::with_capacity(s.len());
        let mut quote: Option<char> = None;
        let mut it = s.char_indices().peekable();
        while let Some((i, c)) = it.next() {
            match (quote, c) {
                (Some(q), c) if c == q => {
                    quote = None;
                    out.push(c);
                }
                (None, '"' | '\'') => {
                    quote = Some(c);
                    out.push(c);
                }
                (None, '%') => {
                    let rest = &s[i + 1..];
                    let name_len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
                    let starts_name = rest.chars().next().is_some_and(is_name_start);
                    if starts_name && rest[name_len..].starts_with(';') {
                        let name = rest[..name_len].to_string();
                        if let Some((text, _)) = self.pe_text(&name) {
                            out.push(' ');
                            out.push_str(&self.expand(strip_text_decl(&text), depth + 1));
                            out.push(' ');
                        }
                        for _ in 0..rest[..name_len + 1].chars().count() {
                            it.next();
                        }
                    } else {
                        out.push('%');
                    }
                }
                _ => out.push(c),
            }
        }
        out
    }

    /// Expands parameter entities inside an entity value literal (no padding).
    fn expand_literal(&mut self, s: &str, depth: usize) -> String {
        if !s.contains('%') || depth > MAX_DEPTH {
            return s.to_string();
        }
        let mut out = String::new();
        let mut rest = s;
        while let Some(i) = rest.find('%') {
            out.push_str(&rest[..i]);
            let tail = &rest[i + 1..];
            let n = tail.find(|c: char| !is_name_char(c)).unwrap_or(tail.len());
            if n > 0 && tail[n..].starts_with(';') {
                let name = tail[..n].to_string();
                if let Some((text, _)) = self.pe_text(&name) {
                    out.push_str(&self.expand_literal(&text, depth + 1));
                }
                rest = &tail[n + 1..];
            } else {
                out.push('%');
                rest = tail;
            }
        }
        out.push_str(rest);
        out
    }

    fn declaration(&mut self, body: &str, base: &Path, depth: usize) {
        let keyword_end = body
            .find(|c: char| c.is_whitespace() || c == '%')
            .unwrap_or(body.len());
        let (keyword, rest) = body.split_at(keyword_end);
        let comment = self.last_comment.take();
        match keyword {
            "ENTITY" => self.entity_decl(rest, base, depth),
            "ELEMENT" => {
                let rest = self.expand(rest, depth);
                self.element_decl(&rest, comment);
            }
            "ATTLIST" => {
                let rest = self.expand(rest, depth);
                self.attlist_decl(&rest);
            }
            "NOTATION" => {
                let rest = self.expand(rest, depth);
                let mut lx = Lex::new(&rest);
                let Some(name) = lx.word() else { return };
                let (public_id, system_id) = external_id(&mut lx);
                self.notations.push(NotationDecl {
                    name: name.to_string(),
                    public_id,
                    system_id,
                });
            }
            other => self.warn(format!("unknown declaration '<!{other}'")),
        }
    }

    fn entity_decl(&mut self, rest: &str, base: &Path, depth: usize) {
        let mut lx = Lex::new(rest);
        let is_pe = lx.eat('%');
        // The rest may itself use parameter entities (not inside literals).
        let tail = self.expand(lx.rest(), depth);
        let mut lx = Lex::new(&tail);
        let Some(name) = lx.word().map(str::to_string) else {
            self.warn("ENTITY declaration without a name".into());
            return;
        };
        let exists = if is_pe {
            self.pes.contains_key(&name)
        } else {
            self.entities.iter().any(|e| e.name == name)
        };
        if exists {
            return; // First declaration wins.
        }
        if let Some(value) = lx.quoted() {
            let value = decode_char_refs_only(&self.expand_literal(value, depth));
            if is_pe {
                self.pes.insert(
                    name,
                    PeValue::Text {
                        text: value,
                        base: base.to_path_buf(),
                    },
                );
            } else {
                self.entities.push(EntityDecl {
                    name,
                    kind: EntityKind::Internal(value),
                });
            }
            return;
        }
        let (public, system) = external_id(&mut lx);
        let Some(system) = system else {
            self.warn(format!("malformed ENTITY declaration '{name}'"));
            return;
        };
        if is_pe {
            self.pes.insert(
                name,
                PeValue::External {
                    public,
                    system,
                    base: base.to_path_buf(),
                },
            );
            return;
        }
        let resolved = self
            .catalog
            .resolve(public.as_deref(), Some(&system))
            .or_else(|| location_to_path(&system, Some(base)));
        let kind = if lx.word() == Some("NDATA") {
            let notation = lx.word().unwrap_or("").to_string();
            EntityKind::Unparsed {
                public_id: public,
                system_id: system,
                notation,
                resolved,
            }
        } else {
            EntityKind::External {
                public_id: public,
                system_id: system,
                resolved,
            }
        };
        self.entities.push(EntityDecl { name, kind });
    }

    fn element_decl(&mut self, rest: &str, comment: Option<String>) {
        let mut lx = Lex::new(rest);
        let Some(name) = lx.word().map(str::to_string) else {
            return;
        };
        let spec = match parse_content_spec(&mut lx) {
            Some(s) => s,
            None => {
                self.warn(format!(
                    "cannot parse content model of '{name}': {}",
                    rest.trim()
                ));
                ContentSpec::Any
            }
        };
        if self.element_index.contains_key(&name) {
            self.warn(format!("element '{name}' declared more than once"));
            return;
        }
        self.element_index.insert(name.clone(), self.elements.len());
        self.elements.push((name, spec, comment));
    }

    fn attlist_decl(&mut self, rest: &str) {
        let mut lx = Lex::new(rest);
        let Some(elem) = lx.word().map(str::to_string) else {
            return;
        };
        while !lx.at_end() {
            let Some(name) = lx.word().map(str::to_string) else {
                self.warn(format!("malformed ATTLIST for '{elem}'"));
                return;
            };
            let ty = if lx.eat('(') {
                RawType::Enumeration(name_list(&mut lx))
            } else {
                match lx.word() {
                    Some("CDATA") => RawType::Cdata,
                    Some("NOTATION") => {
                        if lx.eat('(') {
                            RawType::Notation(name_list(&mut lx))
                        } else {
                            RawType::Builtin("NOTATION")
                        }
                    }
                    Some(
                        t @ ("ID" | "IDREF" | "IDREFS" | "ENTITY" | "ENTITIES" | "NMTOKEN"
                        | "NMTOKENS"),
                    ) => RawType::Builtin(match t {
                        "ID" => "ID",
                        "IDREF" => "IDREF",
                        "IDREFS" => "IDREFS",
                        "ENTITY" => "ENTITY",
                        "ENTITIES" => "ENTITIES",
                        "NMTOKEN" => "NMTOKEN",
                        _ => "NMTOKENS",
                    }),
                    other => {
                        self.warn(format!(
                            "unknown attribute type '{}' for '{elem}/@{name}'",
                            other.unwrap_or("")
                        ));
                        RawType::Cdata
                    }
                }
            };
            let (mut required, mut default, mut fixed) = (false, None, None);
            lx.ws();
            if lx.peek() == Some('#') {
                match lx.word() {
                    Some("#REQUIRED") => required = true,
                    Some("#IMPLIED") => {}
                    Some("#FIXED") => fixed = lx.quoted().map(decode_refs),
                    other => self.warn(format!(
                        "unknown default '{}' for '{elem}/@{name}'",
                        other.unwrap_or("")
                    )),
                }
            } else {
                default = lx.quoted().map(decode_refs);
            }
            if self.attr_seen.insert((elem.clone(), name.clone())) {
                self.attlists.push((
                    elem.clone(),
                    RawAttr {
                        name,
                        ty,
                        required,
                        default,
                        fixed,
                    },
                ));
            }
        }
    }

    fn into_grammar(mut self, root: Option<&str>) -> Grammar {
        let mut g = Grammar::new(GrammarKind::Dtd);
        g.doctype_root = root.map(str::to_string);
        // Declared elements first, then names only referenced.
        let mut names: Vec<String> = self.elements.iter().map(|e| e.0.clone()).collect();
        let mut known: HashSet<String> = names.iter().cloned().collect();
        let mut referenced = Vec::new();
        for (_, spec, _) in &self.elements {
            collect_names(spec, &mut referenced);
        }
        for (elem, _) in &self.attlists {
            referenced.push(elem.clone());
        }
        for r in referenced {
            if known.insert(r.clone()) {
                names.push(r);
            }
        }
        let declared = self.elements.len();
        let mut ids: HashMap<String, DeclId> = HashMap::new();
        for (i, name) in names.iter().enumerate() {
            let sym = g.names.intern(None, name, name);
            let id = DeclId(i as u32);
            g.decls.push(ElementDecl {
                sym,
                ty: i,
                global: true,
                is_abstract: false,
                declared: i < declared,
                documentation: self.elements.get(i).and_then(|e| e.2.clone()),
            });
            g.globals.insert(sym, id);
            ids.insert(name.clone(), id);
        }
        let mut attrs: Vec<Vec<AttrDecl>> = vec![Vec::new(); names.len()];
        for (elem, raw) in std::mem::take(&mut self.attlists) {
            let decl = raw.into_decl(&mut g.simple);
            attrs[ids[&elem].0 as usize].push(decl);
        }
        for (i, attrs) in attrs.into_iter().enumerate() {
            let spec = self
                .elements
                .get(i)
                .map(|e| &e.1)
                .unwrap_or(&ContentSpec::Any);
            let (content, particle) = match spec {
                ContentSpec::Empty => (ContentType::Empty, Particle::Empty),
                ContentSpec::Any => (ContentType::Any, Particle::Empty),
                ContentSpec::Mixed(v) if v.is_empty() => (ContentType::TextOnly, Particle::Empty),
                ContentSpec::Mixed(v) => (
                    ContentType::Mixed,
                    Particle::Choice(
                        v.iter()
                            .map(|n| Particle::Term(Term::Elem(ids[n])))
                            .collect(),
                    )
                    .repeat(0, None),
                ),
                ContentSpec::Children(cp) => (ContentType::ElementOnly, cp_particle(cp, &ids)),
            };
            let model = Model::compile(&particle, &g);
            let simple = (content == ContentType::TextOnly).then(|| g.simple.string());
            g.types.push(ElemType {
                content,
                model,
                attrs,
                any_attribute: i >= declared,
                simple,
                documentation: None,
            });
        }
        g.entities = self.entities;
        g.notations = self.notations;
        g.warnings = self.warnings;
        if let Some(r) = root
            && !ids.contains_key(r)
        {
            g.warnings
                .push(format!("DOCTYPE root element '{r}' is not declared"));
        }
        g
    }
}

/// Decodes character references only (general entity references in entity
/// values are bypassed, as XML requires).
fn decode_char_refs_only(s: &str) -> String {
    if !s.contains("&#") {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("&#") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let Some(semi) = tail.find(';') else { break };
        let n = &tail[2..semi];
        let ch = match n.strip_prefix('x') {
            Some(h) => u32::from_str_radix(h, 16).ok(),
            None => n.parse().ok(),
        }
        .and_then(char::from_u32);
        match ch {
            Some(c) => out.push(c),
            None => out.push_str(&tail[..=semi]),
        }
        rest = &tail[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Parses `SYSTEM "sys"` or `PUBLIC "pub" ["sys"]`.
fn external_id(lx: &mut Lex<'_>) -> (Option<String>, Option<String>) {
    match lx.word() {
        Some("SYSTEM") => (None, lx.quoted().map(str::to_string)),
        Some("PUBLIC") => {
            let public = lx.quoted().map(str::to_string);
            (public, lx.quoted().map(str::to_string))
        }
        _ => (None, None),
    }
}

fn name_list(lx: &mut Lex<'_>) -> Vec<String> {
    let mut v = Vec::new();
    loop {
        if let Some(w) = lx.word() {
            v.push(w.to_string());
        }
        if lx.eat('|') {
            continue;
        }
        lx.eat(')');
        break;
    }
    v
}

fn parse_content_spec(lx: &mut Lex<'_>) -> Option<ContentSpec> {
    lx.ws();
    if lx.peek() != Some('(') {
        return match lx.word()? {
            "EMPTY" => Some(ContentSpec::Empty),
            "ANY" => Some(ContentSpec::Any),
            _ => None,
        };
    }
    let save = lx.pos;
    lx.eat('(');
    lx.ws();
    if lx.rest().starts_with("#PCDATA") {
        lx.pos += 7;
        let mut names = Vec::new();
        while lx.eat('|') {
            names.push(lx.word()?.to_string());
        }
        if !lx.eat(')') {
            return None;
        }
        lx.eat('*');
        return Some(ContentSpec::Mixed(names));
    }
    lx.pos = save;
    let cp = parse_cp(lx)?;
    Some(ContentSpec::Children(cp))
}

fn parse_cp(lx: &mut Lex<'_>) -> Option<Cp> {
    let base = if lx.eat('(') {
        let first = parse_cp(lx)?;
        let mut items = vec![first];
        let mut sep = None;
        loop {
            lx.ws();
            match lx.peek()? {
                ')' => {
                    lx.pos += 1;
                    break;
                }
                c @ (',' | '|') => {
                    if sep.is_some_and(|s| s != c) {
                        return None;
                    }
                    sep = Some(c);
                    lx.pos += 1;
                    items.push(parse_cp(lx)?);
                }
                _ => return None,
            }
        }
        if sep == Some('|') {
            Cp::Choice(items)
        } else {
            Cp::Seq(items)
        }
    } else {
        Cp::Name(lx.word()?.to_string())
    };
    match lx.peek() {
        Some(c @ ('?' | '*' | '+')) => {
            lx.pos += 1;
            Some(Cp::Occurs(Box::new(base), c))
        }
        _ => Some(base),
    }
}

fn collect_names(spec: &ContentSpec, out: &mut Vec<String>) {
    fn cp(c: &Cp, out: &mut Vec<String>) {
        match c {
            Cp::Name(n) => out.push(n.clone()),
            Cp::Seq(v) | Cp::Choice(v) => v.iter().for_each(|x| cp(x, out)),
            Cp::Occurs(x, _) => cp(x, out),
        }
    }
    match spec {
        ContentSpec::Mixed(v) => out.extend(v.iter().cloned()),
        ContentSpec::Children(c) => cp(c, out),
        _ => {}
    }
}

fn cp_particle(cp: &Cp, ids: &HashMap<String, DeclId>) -> Particle {
    match cp {
        Cp::Name(n) => Particle::Term(Term::Elem(ids[n])),
        Cp::Seq(v) => Particle::Seq(v.iter().map(|c| cp_particle(c, ids)).collect()),
        Cp::Choice(v) => Particle::Choice(v.iter().map(|c| cp_particle(c, ids)).collect()),
        Cp::Occurs(c, o) => {
            let p = cp_particle(c, ids);
            match o {
                '?' => p.repeat(0, Some(1)),
                '*' => p.repeat(0, None),
                _ => p.repeat(1, None),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_spec_parsing() {
        let parse = |s: &str| parse_content_spec(&mut Lex::new(s));
        assert_eq!(parse("EMPTY"), Some(ContentSpec::Empty));
        assert_eq!(parse(" ( #PCDATA ) "), Some(ContentSpec::Mixed(vec![])));
        assert_eq!(
            parse("(#PCDATA|a|b)*"),
            Some(ContentSpec::Mixed(vec!["a".into(), "b".into()]))
        );
        let Some(ContentSpec::Children(cp)) = parse("(a,(b|c)*,d?)+") else {
            panic!()
        };
        assert_eq!(
            cp,
            Cp::Occurs(
                Box::new(Cp::Seq(vec![
                    Cp::Name("a".into()),
                    Cp::Occurs(
                        Box::new(Cp::Choice(vec![Cp::Name("b".into()), Cp::Name("c".into())])),
                        '*'
                    ),
                    Cp::Occurs(Box::new(Cp::Name("d".into())), '?'),
                ])),
                '+'
            )
        );
        assert_eq!(parse("(a,b|c)"), None);
    }

    #[test]
    fn helpers() {
        assert_eq!(decode_refs("a&lt;&#65;&x;"), "a<A&x;");
        assert_eq!(decode_char_refs_only("&#x41;&amp;"), "A&amp;");
        assert_eq!(
            strip_text_decl("<?xml version='1.0'?>\n<!ELEMENT"),
            "\n<!ELEMENT"
        );
        let t = "<![ INCLUDE [ <![IGNORE[ x ]]> y ]]> z";
        assert_eq!(cond_end(t, 13), Some((33, 36)));
    }
}
