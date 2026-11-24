//! Finding patterns in a stylesheet that make it slow, and fixing the ones
//! that can be fixed without changing what it produces.
//!
//! Three kinds of finding:
//!
//! - **Fix**: a rewrite that is equivalent by construction and stays inside
//!   the element it came from (a lookup evaluated twice, computed once in a
//!   variable). Applied with Apply, then proven with Check.
//! - **Suggestion**: a rewrite that is equivalent under conditions the
//!   analyser cannot see from the stylesheet alone, or that adds a top-level
//!   declaration (a new `xsl:key`). Applied only on request, then checked.
//! - **Note**: a costly pattern with no mechanical fix (`xsl:number
//!   level="any"`, `preceding::`), reported with what to do instead.
//!
//! When a profile is given, every finding carries the time its template or
//! function took and how often it ran; findings in code the tests never
//! reached are marked, and their fixes are not applied by Apply all.

use crate::profile::Profile;
use crate::stylesheet::{Module, Stylesheet, path_key};
use crate::xpath::{self, Kind, Token};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use xylem_dom::NodeId;

/// How a finding can be acted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Fix,
    Suggestion,
    Note,
}

/// Which rule found it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    /// The same lookup in a `xsl:when` test and again inside it.
    RepeatedLookup,
    /// `//name[@attr = value]`: a whole-document search for a key value.
    DocumentLookup,
    /// `$var/name[@attr = value]`: a search through a sequence by value.
    SequenceLookup,
    /// `xsl:number level="any"`.
    NumberAny,
    /// `preceding::` or `following::`.
    DocumentOrderAxis,
    /// `//` or `descendant::` inside a template or function.
    DescendantSearch,
}

impl Rule {
    pub fn title(self) -> &'static str {
        match self {
            Rule::RepeatedLookup => "Same lookup done twice",
            Rule::DocumentLookup => "Whole-document search for a value",
            Rule::SequenceLookup => "Search by value through a list",
            Rule::NumberAny => "Numbering counts from the start of the document",
            Rule::DocumentOrderAxis => "Looks at everything before or after",
            Rule::DescendantSearch => "Searches everything below",
        }
    }
}

/// A change to a file: replace the bytes in `range` with `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub file: PathBuf,
    pub range: Range<usize>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub rule: Rule,
    pub severity: Severity,
    pub file: PathBuf,
    pub line: u32,
    /// The template or function it is in.
    pub within: Option<String>,
    /// Own time and calls of that template or function, from the profile.
    pub self_ms: Option<f64>,
    pub calls: Option<u64>,
    /// What the finding is about and what to do.
    pub detail: String,
    /// The expression as it is, and as the fix would make it.
    pub before: String,
    pub after: Option<String>,
    /// The edits of the fix (empty for a note).
    pub edits: Vec<TextEdit>,
}

impl Finding {
    /// Whether the tests ran this code (unknown without a profile).
    pub fn reached(&self) -> Option<bool> {
        self.calls.map(|c| c > 0)
    }

    /// Applied by Apply all: a fix in code the tests reach.
    pub fn auto(&self) -> bool {
        self.severity == Severity::Fix && !self.edits.is_empty() && self.reached() == Some(true)
    }
}

#[derive(Debug, Clone)]
pub struct Analysis {
    pub top: PathBuf,
    pub modules: Vec<PathBuf>,
    pub findings: Vec<Finding>,
    pub warnings: Vec<String>,
    /// Expressions the reader could not follow (left alone).
    pub unreadable: usize,
    /// Expressions using DTD entities (left alone).
    pub with_entities: usize,
    /// The text of each module when analysed, to refuse stale edits.
    texts: HashMap<PathBuf, String>,
}

/// The template or function around an element.
struct Within {
    label: String,
    key: (String, u32),
    /// Variable types declared in it (`as` of params and variables).
    types: HashMap<String, String>,
    /// Declared in backwards-compatible (XSLT 1.0) mode.
    compat: bool,
}

/// Profile timings by (file, line).
fn timings(profile: Option<&Profile>) -> HashMap<(String, u32), (f64, u64)> {
    let mut m = HashMap::new();
    if let Some(p) = profile {
        for t in &p.templates {
            let e = m.entry((path_key(&t.path()), t.line)).or_insert((0.0, 0));
            e.0 += t.self_ms;
            e.1 += t.calls;
        }
    }
    m
}

fn version_below_2(v: &str) -> bool {
    v.trim().parse::<f64>().is_ok_and(|x| x < 2.0)
}

/// Whether `el` or an ancestor asks for XSLT 1.0 behaviour.
fn compat_mode(m: &Module, el: NodeId) -> bool {
    std::iter::once(el).chain(m.doc.ancestors(el)).any(|a| {
        if !m.doc.is_element(a) {
            return false;
        }
        let v = if m.xsl_name(a).is_some() {
            m.doc.attr(a, "version")
        } else {
            m.doc.attr(a, &format!("{}:version", m.xsl))
        };
        v.is_some_and(version_below_2)
    })
}

fn within(m: &Module, el: NodeId) -> Option<Within> {
    let owner = std::iter::once(el)
        .chain(m.doc.ancestors(el))
        .find(|&a| m.is_xsl(a, "template") || m.is_xsl(a, "function"))?;
    let label = if m.is_xsl(owner, "function") {
        format!("{}()", m.doc.attr(owner, "name").unwrap_or("?"))
    } else if let Some(n) = m.doc.attr(owner, "name") {
        n.to_string()
    } else {
        format!("match=\"{}\"", m.doc.attr(owner, "match").unwrap_or("?"))
    };
    let mut types = HashMap::new();
    for d in m.doc.descendants(owner) {
        if (m.is_xsl(d, "param") || m.is_xsl(d, "variable"))
            && let (Some(n), Some(t)) = (m.doc.attr(d, "name"), m.doc.attr(d, "as"))
        {
            types.insert(n.to_string(), t.trim().to_string());
        }
    }
    Some(Within {
        label,
        key: (path_key(&m.path), m.saxon_line(owner)),
        types,
        compat: compat_mode(m, owner),
    })
}

/// Attributes of XSLT elements that hold XPath expressions.
const EXPRESSION_ATTRS: &[&str] = &[
    "select",
    "test",
    "use",
    "value",
    "group-by",
    "group-adjacent",
];

/// Elements that change the focus for what is inside them.
fn changes_focus(m: &Module, el: NodeId) -> bool {
    matches!(
        m.xsl_name(el),
        Some(
            "for-each"
                | "for-each-group"
                | "iterate"
                | "analyze-string"
                | "matching-substring"
                | "non-matching-substring"
                | "sort"
                | "template"
                | "function"
        )
    )
}

struct Scan<'a> {
    sheet: &'a Stylesheet,
    timings: HashMap<(String, u32), (f64, u64)>,
    globals: HashMap<String, String>,
    names_in_use: HashSet<String>,
    keys: Vec<(String, String, String)>,
    findings: Vec<Finding>,
    unreadable: usize,
    with_entities: usize,
}

impl<'a> Scan<'a> {
    fn finding(
        &self,
        m: &Module,
        el: NodeId,
        rule: Rule,
        severity: Severity,
        detail: String,
        before: String,
    ) -> Finding {
        let w = within(m, el);
        let t = w.as_ref().and_then(|w| self.timings.get(&w.key)).copied();
        let has_profile = !self.timings.is_empty();
        Finding {
            rule,
            severity,
            file: m.path.clone(),
            line: m.saxon_line(el),
            within: w.map(|w| w.label),
            self_ms: t.map(|t| t.0).or(has_profile.then_some(0.0)),
            calls: t.map(|t| t.1).or(has_profile.then_some(0)),
            detail,
            before,
            after: None,
            edits: Vec::new(),
        }
    }

    /// A variable name not used anywhere in the stylesheet.
    fn fresh_name(&mut self, base: &str) -> String {
        let base = base.trim_matches('-');
        let base = if base.is_empty() { "found" } else { base };
        let mut name = base.to_string();
        let mut n = 2;
        while self.names_in_use.contains(&name) {
            name = format!("{base}-{n}");
            n += 1;
        }
        self.names_in_use.insert(name.clone());
        name
    }

    fn string_typed(
        &self,
        w: Option<&Within>,
        tokens: &[Token],
        src: &str,
        range: Range<usize>,
        pairs: &[Option<usize>],
    ) -> bool {
        let t = &tokens[range.clone()];
        let declared = |name: &str| {
            let n = name.trim_start_matches('$');
            w.and_then(|w| w.types.get(n))
                .or_else(|| self.globals.get(n))
                .map(|s| s.as_str())
        };
        match t {
            [one] if one.kind == Kind::String => true,
            [one] if one.kind == Kind::Variable => {
                matches!(declared(one.text(src)), Some("xs:string" | "xs:string?"))
            }
            [f, open, ..] if f.kind == Kind::Name && open.kind == Kind::LParen => {
                pairs[range.start + 1] == Some(range.end - 1) && xpath::returns_string(f.text(src))
            }
            _ => false,
        }
    }

    fn scan_module(&mut self, mi: usize) {
        let m = &self.sheet.modules[mi];
        let elements: Vec<NodeId> = m
            .doc
            .descendants(m.doc.root())
            .filter(|&n| m.doc.is_element(n) && m.xsl_name(n).is_some())
            .collect();
        for el in elements {
            let in_body = within(m, el).is_some();
            if m.is_xsl(el, "choose") {
                self.repeated_lookup(m, el);
            }
            if m.is_xsl(el, "number") && m.doc.attr(el, "level") == Some("any") {
                let mut f = self.finding(
                    m,
                    el,
                    Rule::NumberAny,
                    Severity::Note,
                    "Each time this runs it counts every matching element from the start of the \
                     document (or of the `from` element), so its cost grows with the document and \
                     with how often it runs. When the same number is needed many times, compute \
                     the numbers once (in a variable or a pre-pass) and look them up."
                        .into(),
                    xml_of(m, el),
                );
                f.after = None;
                self.findings.push(f);
            }
            // (`count` and `from` of xsl:number are patterns, not searches.)
            for &attr in EXPRESSION_ATTRS {
                let Some(raw) = m.raw_attr(el, attr) else {
                    continue;
                };
                if raw.entities {
                    self.with_entities += 1;
                    continue;
                }
                let src = raw.value.clone();
                let Some(tokens) = xpath::tokenize(&src) else {
                    self.unreadable += 1;
                    continue;
                };
                let Some(pairs) = xpath::partners(&tokens) else {
                    self.unreadable += 1;
                    continue;
                };
                if m.is_xsl(el, "key") || (!in_body && m.is_xsl(el, "variable")) {
                    // Evaluated once per document, or indexed: not a concern.
                    continue;
                }
                if !in_body {
                    continue;
                }
                self.expression_rules(m, el, attr, &raw, &src, &tokens, &pairs);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn expression_rules(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        raw: &crate::stylesheet::RawValue,
        src: &str,
        tokens: &[Token],
        pairs: &[Option<usize>],
    ) {
        let mut reported_search = false;
        for i in 0..tokens.len() {
            let t = &tokens[i];
            // preceding:: / following::
            if t.kind == Kind::Name
                && matches!(t.text(src), "preceding" | "following")
                && tokens.get(i + 1).map(|t| t.kind) == Some(Kind::Axis)
            {
                let f = self.finding(
                    m,
                    el,
                    Rule::DocumentOrderAxis,
                    Severity::Note,
                    format!(
                        "`{}::` visits every node before or after the current one in the whole \
                         document, each time it is evaluated. Counting with it (`count(preceding::x)`) \
                         is the usual cause; a number computed once, or `preceding-sibling::` when \
                         only siblings matter, does far less work.",
                        t.text(src)
                    ),
                    format!("{attr}=\"{src}\""),
                );
                self.findings.push(f);
                break;
            }
            let starts_expr = i == 0 || !xpath::after_operand(tokens, src, i);
            if t.kind == Kind::DoubleSlash
                && starts_expr
                && let Some(f) = self.document_lookup(m, el, attr, raw, src, tokens, pairs, i)
            {
                self.findings.push(f);
                reported_search = true;
                continue;
            }
            if t.kind == Kind::Variable
                && starts_expr
                && let Some(f) = self.sequence_lookup(m, el, attr, src, tokens, pairs, i)
            {
                self.findings.push(f);
                continue;
            }
            let descendant = (t.kind == Kind::DoubleSlash)
                || (t.kind == Kind::Name
                    && matches!(t.text(src), "descendant" | "descendant-or-self")
                    && tokens.get(i + 1).map(|t| t.kind) == Some(Kind::Axis));
            if descendant && !reported_search {
                let whole = t.kind == Kind::DoubleSlash && starts_expr;
                let f = self.finding(
                    m,
                    el,
                    Rule::DescendantSearch,
                    Severity::Note,
                    if whole {
                        "Searches the whole document each time it is evaluated. If this runs for \
                         many elements, look the result up instead: an `xsl:key`, or a global \
                         variable computed once."
                            .into()
                    } else {
                        "Searches everything below a node each time it is evaluated. Fine for \
                         small subtrees; costly when it runs often over large ones."
                            .into()
                    },
                    format!("{attr}=\"{src}\""),
                );
                self.findings.push(f);
                reported_search = true;
            }
        }
    }

    /// `//name[@attr = value]…` → `key('name-by-attr', value)…`.
    #[allow(clippy::too_many_arguments)]
    fn document_lookup(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        raw: &crate::stylesheet::RawValue,
        src: &str,
        tokens: &[Token],
        pairs: &[Option<usize>],
        i: usize,
    ) -> Option<Finding> {
        let name = tokens.get(i + 1).filter(|t| t.kind == Kind::Name)?;
        let next = tokens.get(i + 2)?;
        if next.kind != Kind::LBracket {
            return None;
        }
        let close = pairs[i + 2]?;
        let at = tokens.get(i + 3).filter(|t| t.kind == Kind::At)?;
        let key_attr = tokens.get(i + 4).filter(|t| t.kind == Kind::Name)?;
        let eq = tokens.get(i + 5)?;
        if !(eq.text(src) == "=" || eq.text(src) == "eq") || i + 6 >= close {
            return None;
        }
        let _ = at;
        let value = i + 6..close;
        if !xpath::context_free(tokens, src, value.clone(), pairs) {
            return None;
        }
        let w = within(m, el);
        let typed = self.string_typed(w.as_ref(), tokens, src, value.clone(), pairs);
        let element = name.text(src).to_string();
        let attribute = key_attr.text(src).to_string();
        let value_text = &src[tokens[value.start].span.start..tokens[value.end - 1].span.end];
        let before = &src[t_start(tokens, i)..tokens[close].span.end];
        // An existing key on the same thing, or a new one.
        let existing = self
            .keys
            .iter()
            .find(|(_, mt, u)| mt == &element && u.replace(' ', "") == format!("@{attribute}"))
            .map(|(n, _, _)| n.clone());
        let key_name = existing.clone().unwrap_or_else(|| {
            let local = |s: &str| s.rsplit(':').next().unwrap_or(s).to_string();
            self.fresh_name(&format!("{}-by-{}", local(&element), local(&attribute)))
        });
        let call = format!("key('{key_name}', {value_text})");
        let mut f = self.finding(
            m,
            el,
            Rule::DocumentLookup,
            Severity::Suggestion,
            String::new(),
            format!("{attr}=\"{src}\""),
        );
        let rest = &src[tokens[close].span.end..];
        let after = format!("{}{call}{rest}", &src[..t_start(tokens, i)]);
        f.detail = format!(
            "`{before}` searches the whole document every time it runs. An `xsl:key` indexes \
             the document once; `{call}` then finds the same elements, in the same order. \
             {}{}{}",
            if existing.is_some() {
                "The stylesheet already has this key. "
            } else {
                "Adds a top-level `xsl:key` to this module. "
            },
            if typed {
                ""
            } else {
                "Equivalent only if the value is a string: a number would compare as a number in `=` but as a string in a key. "
            },
            if w.as_ref().is_some_and(|w| w.compat) {
                "This code runs in XSLT 1.0 compatibility mode (version=\"1.0\"), where comparisons follow XPath 1.0 rules; check carefully."
            } else {
                ""
            },
        );
        f.after = Some(format!("{attr}=\"{after}\""));
        if typed && m.editable {
            let range = raw.file_range(t_start(tokens, i)..tokens[close].span.end);
            f.edits.push(TextEdit {
                file: m.path.clone(),
                range,
                text: raw.escape(&call),
            });
            if existing.is_none() {
                f.edits
                    .push(key_declaration(m, &key_name, &element, &attribute));
                self.keys.push((key_name, element, format!("@{attribute}")));
            }
        }
        Some(f)
    }

    /// `$var/name[…@attr = value…]`: a scan through a sequence.
    #[allow(clippy::too_many_arguments)]
    fn sequence_lookup(
        &mut self,
        m: &Module,
        el: NodeId,
        attr: &str,
        src: &str,
        tokens: &[Token],
        pairs: &[Option<usize>],
        i: usize,
    ) -> Option<Finding> {
        if tokens.get(i + 1)?.kind != Kind::Slash || tokens.get(i + 2)?.kind != Kind::Name {
            return None;
        }
        let mut j = i + 3;
        let mut compares = false;
        while tokens.get(j).map(|t| t.kind) == Some(Kind::LBracket) {
            let close = pairs[j]?;
            let inner = &tokens[j + 1..close];
            if inner.windows(3).any(|w| {
                w[0].kind == Kind::At
                    && w[1].kind == Kind::Name
                    && matches!(w[2].text(src), "=" | "eq")
            }) || inner.iter().any(|t| t.text(src) == "=")
                && inner.iter().any(|t| t.kind == Kind::At)
            {
                compares = true;
            }
            j = close + 1;
        }
        if !compares {
            return None;
        }
        let expr = &src[tokens[i].span.start..tokens[j - 1].span.end];
        let f = self.finding(
            m,
            el,
            Rule::SequenceLookup,
            Severity::Note,
            format!(
                "`{expr}` checks every item in `{}` each time it runs. If that list comes from a \
                 document (a translations or configuration file) and this runs often, index it \
                 with an `xsl:key` and look the value up with `key(name, value, $doc)`. The key \
                 finds descendants, not only children, and the third argument must be a single \
                 node, so this is a change to make by hand and check.",
                tokens[i].text(src)
            ),
            format!("{attr}=\"{src}\""),
        );
        Some(f)
    }

    /// An `xsl:choose` whose first `xsl:when` tests a lookup that the code
    /// inside it then evaluates again.
    fn repeated_lookup(&mut self, m: &Module, choose: NodeId) {
        let Some(when) = m
            .doc
            .element_children(choose)
            .next()
            .filter(|&w| m.is_xsl(w, "when"))
        else {
            return;
        };
        if m.doc.attr(when, "version").is_some() {
            return;
        }
        let Some(test) = m.raw_attr(when, "test").filter(|t| !t.entities) else {
            return;
        };
        let src = test.value.clone();
        let Some(tokens) = xpath::tokenize(&src) else {
            return;
        };
        let Some(pairs) = xpath::partners(&tokens) else {
            return;
        };
        if xpath::simple_path_end(&tokens, &src, 0, &pairs) != Some(tokens.len())
            || !tokens.iter().any(|t| t.kind == Kind::LBracket)
        {
            return;
        }
        let key = xpath::canonical(&tokens, &src, 0..tokens.len());
        // Uses inside the xsl:when, with the same focus.
        let mut uses = Vec::new();
        for d in m.doc.descendants(when) {
            if d == when || !m.doc.is_element(d) || m.xsl_name(d).is_none() {
                continue;
            }
            let focus_changed = m
                .doc
                .ancestors(d)
                .take_while(|&a| a != when)
                .any(|a| changes_focus(m, a));
            if focus_changed {
                continue;
            }
            for &attr in EXPRESSION_ATTRS {
                let Some(raw) = m.raw_attr(d, attr).filter(|r| !r.entities) else {
                    continue;
                };
                let Some(t) = xpath::tokenize(&raw.value) else {
                    continue;
                };
                if t.len() < tokens.len()
                    || xpath::canonical(&t, &raw.value, 0..tokens.len()) != key
                {
                    continue;
                }
                let follows = t.get(tokens.len()).map(|n| n.kind);
                if matches!(follows, Some(Kind::LBracket | Kind::LParen)) {
                    continue;
                }
                uses.push((d, attr, raw, t));
            }
        }
        if uses.is_empty() || !m.editable {
            return;
        }
        // Name the variable after the last step outside the predicates.
        let mut depth = 0i32;
        let mut last_name = String::new();
        for (k, t) in tokens.iter().enumerate() {
            match t.kind {
                Kind::LBracket | Kind::LParen => depth += 1,
                Kind::RBracket | Kind::RParen => depth -= 1,
                Kind::Name
                    if depth == 0
                        && !matches!(
                            tokens.get(k + 1).map(|n| n.kind),
                            Some(Kind::LParen | Kind::Axis)
                        ) =>
                {
                    last_name = t.text(&src).rsplit(':').next().unwrap_or("").to_string();
                }
                _ => {}
            }
        }
        let var = self.fresh_name(&format!("{last_name}-match"));
        let mut f = self.finding(
            m,
            when,
            Rule::RepeatedLookup,
            Severity::Fix,
            format!(
                "The test of this `xsl:when` and {} inside it evaluate the same lookup, so it runs \
                 twice. Computing it once in a variable just before the `xsl:choose` gives the \
                 same nodes (the variable keeps them as nodes) and halves the work.",
                if uses.len() == 1 {
                    "an expression".to_string()
                } else {
                    format!("{} expressions", uses.len())
                }
            ),
            format!("test=\"{src}\""),
        );
        f.after = Some(format!(
            "<{x}:variable name=\"{var}\" select=\"{src}\"/>\n…test=\"${var}\"{more}",
            x = m.xsl,
            more = uses
                .iter()
                .map(|(_, a, raw, t)| format!(
                    " …{a}=\"${var}{}\"",
                    &raw.value[t[tokens.len() - 1].span.end..]
                ))
                .collect::<String>()
        ));
        let indent = m.indent_of(choose);
        let decl = format!(
            "<{x}:variable name=\"{var}\" select=\"{v}\"/>{nl}{indent}",
            x = m.xsl,
            v = test.escape(&src).replace('"', "&quot;"),
            nl = m.newline()
        );
        let at = m.offset(choose);
        f.edits.push(TextEdit {
            file: m.path.clone(),
            range: at..at,
            text: decl,
        });
        f.edits.push(TextEdit {
            file: m.path.clone(),
            range: test.raw.clone(),
            text: format!("${var}"),
        });
        for (_, _, raw, t) in &uses {
            let span = 0..t[tokens.len() - 1].span.end;
            f.edits.push(TextEdit {
                file: m.path.clone(),
                range: raw.file_range(span),
                text: format!("${var}"),
            });
        }
        self.findings.push(f);
    }
}

fn t_start(tokens: &[Token], i: usize) -> usize {
    tokens[i].span.start
}

fn xml_of(m: &Module, el: NodeId) -> String {
    m.text[m.offset(el)..m.start_tag_end(el)].to_string()
}

/// An `xsl:key` declaration inserted before the first top-level element
/// that is not an `xsl:import`.
fn key_declaration(m: &Module, name: &str, element: &str, attribute: &str) -> TextEdit {
    let root = m
        .doc
        .document_element()
        .expect("a stylesheet has a root element");
    let first = m
        .doc
        .element_children(root)
        .find(|&c| !m.is_xsl(c, "import"));
    let (at, indent) = match first {
        Some(c) => (m.offset(c), m.indent_of(c)),
        None => (m.start_tag_end(root), "    ".to_string()),
    };
    let text = format!(
        "<{x}:key name=\"{name}\" match=\"{element}\" use=\"@{attribute}\"/>{nl}{indent}",
        x = m.xsl,
        nl = m.newline()
    );
    TextEdit {
        file: m.path.clone(),
        range: at..at,
        text,
    }
}

/// Analyses a stylesheet and everything it includes and imports.
pub fn analyze(top: &Path, profile: Option<&Profile>) -> Result<Analysis, String> {
    let sheet = Stylesheet::load(top)?;
    let mut globals = HashMap::new();
    let mut names_in_use = HashSet::new();
    let mut keys = Vec::new();
    for m in &sheet.modules {
        for d in m.doc.descendants(m.doc.root()) {
            if !m.doc.is_element(d) {
                continue;
            }
            if (m.is_xsl(d, "variable") || m.is_xsl(d, "param") || m.is_xsl(d, "key"))
                && let Some(n) = m.doc.attr(d, "name")
            {
                names_in_use.insert(n.to_string());
                let top_level = m.doc.parent(d) == m.doc.document_element();
                if top_level && let Some(t) = m.doc.attr(d, "as") {
                    globals.insert(n.to_string(), t.trim().to_string());
                }
            }
            if m.is_xsl(d, "key")
                && let (Some(n), Some(mt), Some(u)) = (
                    m.doc.attr(d, "name"),
                    m.doc.attr(d, "match"),
                    m.doc.attr(d, "use"),
                )
            {
                keys.push((n.to_string(), mt.trim().to_string(), u.trim().to_string()));
            }
        }
    }
    let mut scan = Scan {
        sheet: &sheet,
        timings: timings(profile),
        globals,
        names_in_use,
        keys,
        findings: Vec::new(),
        unreadable: 0,
        with_entities: 0,
    };
    for mi in 0..sheet.modules.len() {
        scan.scan_module(mi);
    }
    let mut findings = scan.findings;
    let mut warnings = sheet.warnings.clone();
    if !sheet.runs_as_xslt2() {
        // The rewrites use XPath 2.0, which an XSLT 1.0 processor rejects:
        // keep what was found, as notes, and offer no edits.
        for f in &mut findings {
            if f.severity != Severity::Note {
                f.severity = Severity::Note;
                f.edits.clear();
                f.detail = format!(
                    "Not fixed automatically: this stylesheet runs on an XSLT 1.0 processor. {}",
                    f.detail
                );
            }
        }
        warnings.push(
            "This stylesheet runs on an XSLT 1.0 processor (its version is not 2.0), so fixes are shown as notes only."
                .into(),
        );
    }
    // Fixes the tests reach first, then by time spent, then by kind.
    findings.sort_by(|a, b| {
        let reached = |f: &Finding| f.reached() == Some(false);
        reached(a)
            .cmp(&reached(b))
            .then(a.severity.cmp(&b.severity))
            .then(
                b.self_ms
                    .unwrap_or(0.0)
                    .total_cmp(&a.self_ms.unwrap_or(0.0)),
            )
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    Ok(Analysis {
        top: top.to_path_buf(),
        modules: sheet.modules.iter().map(|m| m.path.clone()).collect(),
        findings,
        warnings,
        unreadable: scan.unreadable,
        with_entities: scan.with_entities,
        texts: sheet
            .modules
            .iter()
            .map(|m| (m.path.clone(), m.text.clone()))
            .collect(),
    })
}

/// What [`apply`] changed.
#[derive(Debug, Clone, Default)]
pub struct Applied {
    pub files: Vec<PathBuf>,
    pub findings: usize,
    /// Findings left out because their edits overlapped others.
    pub skipped: Vec<String>,
    /// Each changed file as it was before, and as written, for [`revert`].
    originals: Vec<(PathBuf, Vec<u8>, Vec<u8>)>,
}

/// Puts files changed by [`apply`] back as they were, unless they have been
/// changed again since.
pub fn revert(applied: &Applied) -> Result<(), String> {
    for (file, _, written) in &applied.originals {
        let now = std::fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?;
        if &now != written {
            return Err(format!(
                "{} has changed since the fixes were applied; not undone.",
                file.display()
            ));
        }
    }
    for (file, original, _) in &applied.originals {
        std::fs::write(file, original).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    Ok(())
}

/// Applies the edits of `chosen` findings to the files. Refuses if a file
/// changed since it was analysed.
pub fn apply(analysis: &Analysis, chosen: &[usize]) -> Result<Applied, String> {
    let mut by_file: BTreeMap<PathBuf, Vec<&TextEdit>> = BTreeMap::new();
    let mut applied = Applied::default();
    let mut taken: HashMap<PathBuf, Vec<Range<usize>>> = HashMap::new();
    for &i in chosen {
        let f = analysis.findings.get(i).ok_or("No such finding.")?;
        if f.edits.is_empty() {
            continue;
        }
        // Skip a fix whose edits overlap one already chosen.
        let overlaps = f.edits.iter().any(|e| {
            taken.get(&e.file).is_some_and(|rs| {
                rs.iter().any(|r| {
                    (e.range.start < r.end && r.start < e.range.end)
                        || (e.range.is_empty() && r.is_empty() && e.range.start == r.start && false)
                })
            })
        });
        if overlaps {
            applied
                .skipped
                .push(format!("{}:{}", f.file.display(), f.line));
            continue;
        }
        for e in &f.edits {
            taken
                .entry(e.file.clone())
                .or_default()
                .push(e.range.clone());
            by_file.entry(e.file.clone()).or_default().push(e);
        }
        applied.findings += 1;
    }
    for (file, mut edits) in by_file {
        let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let (bom, rest) = match bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
            Some(r) => (true, r),
            None => (false, &bytes[..]),
        };
        let mut text = String::from_utf8(rest.to_vec())
            .map_err(|_| format!("{} is not UTF-8.", file.display()))?;
        if analysis.texts.get(&file) != Some(&text) {
            return Err(format!(
                "{} changed since it was analysed. Analyse again.",
                file.display()
            ));
        }
        // Identical insertions (the same new key from two findings) once.
        edits.sort_by(|a, b| {
            b.range
                .start
                .cmp(&a.range.start)
                .then(b.range.end.cmp(&a.range.end))
        });
        edits.dedup_by(|a, b| a == b);
        for e in edits {
            text.replace_range(e.range.clone(), &e.text);
        }
        let mut out = Vec::with_capacity(text.len() + 3);
        if bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(text.as_bytes());
        std::fs::write(&file, &out).map_err(|e| format!("{}: {e}", file.display()))?;
        applied.originals.push((file.clone(), bytes, out));
        applied.files.push(file);
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(dir: &str, files: &[(&str, &str)]) -> PathBuf {
        let d = std::env::temp_dir().join("xylem-xslt-analyze").join(dir);
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (name, text) in files {
            std::fs::write(d.join(name), text).unwrap();
        }
        d.join(files[0].0)
    }

    const HEAD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:f">"#;

    #[test]
    fn an_xslt_1_stylesheet_gets_notes_and_no_edits() {
        let text = "<?xml version=\"1.0\"?>
<xsl:stylesheet version=\"1.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">
<xsl:template name=\"t\">
<xsl:choose>
<xsl:when test=\"item[@a = 'x']\">
<xsl:value-of select=\"item[@a = 'x']/@b\"/>
</xsl:when>
</xsl:choose>
</xsl:template>
</xsl:stylesheet>
";
        let top = sheet("xslt1", &[("top.xsl", text)]);
        let a = analyze(&top, None).unwrap();
        assert!(!a.findings.is_empty());
        assert!(
            a.findings
                .iter()
                .all(|f| f.severity == Severity::Note && f.edits.is_empty())
        );
        assert!(a.warnings.iter().any(|w| w.contains("XSLT 1.0")));
    }

    #[test]
    fn a_repeated_lookup_is_computed_once() {
        let text = format!(
            "{HEAD}\r\n  <xsl:function name=\"f:icon\" as=\"xs:string\">\r\n    <xsl:param name=\"k\" as=\"xs:string\"/>\r\n    <xsl:param name=\"list\"/>\r\n    <xsl:choose>\r\n      <xsl:when test=\"$list/icon[lower-case(@key) = lower-case($k)][1]\">\r\n        <xsl:value-of select=\"$list/icon[lower-case(@key) = lower-case($k)][1]/@path\"/>\r\n      </xsl:when>\r\n      <xsl:otherwise>none</xsl:otherwise>\r\n    </xsl:choose>\r\n  </xsl:function>\r\n</xsl:stylesheet>\r\n"
        );
        let top = sheet("repeat", &[("top.xsl", &text)]);
        let a = analyze(&top, None).unwrap();
        let f = a
            .findings
            .iter()
            .find(|f| f.rule == Rule::RepeatedLookup)
            .expect("found");
        assert_eq!(f.severity, Severity::Fix);
        assert_eq!(f.within.as_deref(), Some("f:icon()"));
        let i = a
            .findings
            .iter()
            .position(|x| x.rule == Rule::RepeatedLookup)
            .unwrap();
        apply(&a, &[i]).unwrap();
        let out = std::fs::read_to_string(&top).unwrap();
        assert!(out.contains(
            "    <xsl:variable name=\"icon-match\" select=\"$list/icon[lower-case(@key) = lower-case($k)][1]\"/>\r\n    <xsl:choose>"
        ), "{out}");
        assert!(out.contains("<xsl:when test=\"$icon-match\">"));
        assert!(out.contains("<xsl:value-of select=\"$icon-match/@path\"/>"));
        // Nothing else changed.
        let rest_before = text.replace("$list/icon[lower-case(@key) = lower-case($k)][1]", "X");
        let rest_after = out
            .replace("    <xsl:variable name=\"icon-match\" select=\"$list/icon[lower-case(@key) = lower-case($k)][1]\"/>\r\n", "")
            .replace("$icon-match", "X");
        assert_eq!(rest_before, rest_after);
        // Applying again to the changed file is refused.
        assert!(apply(&a, &[i]).is_err());
        // Undo restores the file exactly.
        let again = analyze(&top, None).unwrap();
        let j = again
            .findings
            .iter()
            .position(|x| x.rule == Rule::RepeatedLookup);
        assert!(j.is_none(), "already fixed");
        let fresh = sheet("repeat-undo", &[("top.xsl", &text)]);
        let b = analyze(&fresh, None).unwrap();
        let k = b
            .findings
            .iter()
            .position(|x| x.rule == Rule::RepeatedLookup)
            .unwrap();
        let done = apply(&b, &[k]).unwrap();
        revert(&done).unwrap();
        assert_eq!(std::fs::read_to_string(&fresh).unwrap(), text);
    }

    #[test]
    fn uses_under_a_focus_change_are_left_alone() {
        let text = format!(
            "{HEAD}\n<xsl:template name=\"t\">\n<xsl:choose>\n<xsl:when test=\"item[@a = 'x']\">\n<xsl:for-each select=\"other\"><xsl:value-of select=\"item[@a = 'x']/@b\"/></xsl:for-each>\n</xsl:when>\n</xsl:choose>\n</xsl:template>\n</xsl:stylesheet>\n"
        );
        let top = sheet("focus", &[("top.xsl", &text)]);
        let a = analyze(&top, None).unwrap();
        assert!(a.findings.iter().all(|f| f.rule != Rule::RepeatedLookup));
    }

    #[test]
    fn a_document_lookup_suggests_a_key() {
        let main = format!(
            "{HEAD}\n  <xsl:import href=\"base.xsl\"/>\n  <xsl:param name=\"ref\" as=\"xs:string\"/>\n  <xsl:template match=\"link\">\n    <xsl:apply-templates select=\"//figure[@id = $ref]/title\"/>\n    <xsl:value-of select=\"count(//table[@id = @ref])\"/>\n  </xsl:template>\n</xsl:stylesheet>\n"
        );
        let base = format!(
            "{HEAD}\n  <xsl:template name=\"n\"><xsl:number level=\"any\" count=\"step\"/></xsl:template>\n</xsl:stylesheet>\n"
        );
        let top = sheet("lookup", &[("main.xsl", &main), ("base.xsl", &base)]);
        let a = analyze(&top, None).unwrap();
        assert_eq!(a.modules.len(), 2);
        let i = a
            .findings
            .iter()
            .position(|f| f.rule == Rule::DocumentLookup)
            .expect("lookup");
        let f = &a.findings[i];
        assert_eq!(f.severity, Severity::Suggestion);
        assert_eq!(
            f.after.as_deref(),
            Some("select=\"key('figure-by-id', $ref)/title\"")
        );
        assert_eq!(f.edits.len(), 2);
        // `@ref` depends on the candidate, so the second search is only a note.
        assert!(a.findings.iter().any(|f| f.rule == Rule::DescendantSearch));
        assert!(a.findings.iter().any(|f| f.rule == Rule::NumberAny));
        apply(&a, &[i]).unwrap();
        let out = std::fs::read_to_string(&top).unwrap();
        assert!(out.contains("<xsl:import href=\"base.xsl\"/>\n  <xsl:key name=\"figure-by-id\" match=\"figure\" use=\"@id\"/>\n  <xsl:param"), "{out}");
        assert!(out.contains("select=\"key('figure-by-id', $ref)/title\""));
    }

    #[test]
    fn profiles_rank_and_mark_reach() {
        let text = format!(
            "{HEAD}\n<xsl:template name=\"a\">\n<xsl:value-of select=\"//x\"/>\n</xsl:template>\n<xsl:template name=\"b\">\n<xsl:value-of select=\"//y\"/>\n</xsl:template>\n</xsl:stylesheet>\n"
        );
        let top = sheet("rank", &[("top.xsl", &text)]);
        let uri = format!("file:/{}", top.display().to_string().replace('\\', "/"));
        let profile = Profile {
            total_ms: 5.0,
            templates: vec![crate::profile::TemplateTime {
                kind: "template".into(),
                name: Some("b".into()),
                pattern: None,
                mode: None,
                file: uri,
                line: 6,
                calls: 3,
                self_ms: 4.0,
                total_ms: 4.0,
            }],
        };
        let a = analyze(&top, Some(&profile)).unwrap();
        assert_eq!(a.findings[0].within.as_deref(), Some("b"));
        assert_eq!(a.findings[0].calls, Some(3));
        assert_eq!(a.findings[1].reached(), Some(false));
    }
}
