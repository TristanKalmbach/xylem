//! Business rules exchange (BREX): parsing and checking.
//!
//! Handles both the modern markup (`structureObjectRule/objectPath/objectUse/objectValue`)
//! and the legacy markup (`objrule/objpath/objuse/objval`). The default S1000D BREX is not
//! bundled; load one from disk with [`Brex::load`].

use std::path::Path;

use xylem_dom::{Document, NodeId};
use xylem_xpath::{Context, XNode, XPath, string_value};

use crate::error::S1000dError;
use crate::ident::Dmc;
use crate::pattern::Pattern;
use crate::xmlutil::{attr, child, text};

/// Whether the objects selected by a rule may, must or must not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowedFlag {
    /// `0`: the object is prohibited.
    Prohibited,
    /// `1`: the object is required.
    Required,
    /// `2`: the object is optional (any value restrictions still apply).
    Optional,
}

impl AllowedFlag {
    fn parse(s: &str) -> AllowedFlag {
        match s.trim() {
            "0" => AllowedFlag::Prohibited,
            "1" => AllowedFlag::Required,
            _ => AllowedFlag::Optional,
        }
    }
}

/// How `valueAllowed` is to be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueForm {
    /// One or more exact values separated by `|`.
    Single,
    /// One or more `low~high` ranges (or exact values) separated by `|`.
    Range,
    /// A regular expression matching the whole value.
    Pattern,
}

/// An `objectValue` / `objval` constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectValue {
    pub form: ValueForm,
    /// The `valueAllowed` expression.
    pub allowed: String,
    /// Descriptive text of the element, if any.
    pub description: String,
}

impl ObjectValue {
    /// Compiles the constraint for repeated checking.
    pub fn matcher(&self) -> Result<ValueMatcher, String> {
        Ok(match self.form {
            ValueForm::Single => ValueMatcher::Exact(split_alternatives(&self.allowed)),
            ValueForm::Range => ValueMatcher::Ranges(
                split_alternatives(&self.allowed)
                    .into_iter()
                    .map(|alt| match alt.split_once('~') {
                        Some((lo, hi)) => (lo.trim().to_string(), hi.trim().to_string()),
                        None => (alt.clone(), alt),
                    })
                    .collect(),
            ),
            ValueForm::Pattern => ValueMatcher::Regex(Pattern::new(self.allowed.trim())?),
        })
    }

    /// True if `value` satisfies this constraint (an invalid pattern allows nothing).
    pub fn allows(&self, value: &str) -> bool {
        self.matcher().is_ok_and(|m| m.allows(value))
    }
}

fn split_alternatives(s: &str) -> Vec<String> {
    s.split('|').map(|a| a.trim().to_string()).collect()
}

/// A compiled [`ObjectValue`].
#[derive(Debug, Clone)]
pub enum ValueMatcher {
    Exact(Vec<String>),
    Ranges(Vec<(String, String)>),
    Regex(Pattern),
}

impl ValueMatcher {
    pub fn allows(&self, value: &str) -> bool {
        let value = value.trim();
        match self {
            ValueMatcher::Exact(values) => values.iter().any(|v| v == value),
            ValueMatcher::Ranges(ranges) => ranges.iter().any(|(lo, hi)| in_range(value, lo, hi)),
            ValueMatcher::Regex(p) => p.is_match(value),
        }
    }
}

fn parse_number(s: &str) -> Option<f64> {
    let digits = s.strip_prefix('-').unwrap_or(s);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.') { s.parse().ok() } else { None }
}

fn in_range(value: &str, lo: &str, hi: &str) -> bool {
    match (parse_number(value), parse_number(lo), parse_number(hi)) {
        (Some(v), Some(l), Some(h)) => l <= v && v <= h,
        _ => lo <= value && value <= hi,
    }
}

/// A `structureObjectRule` (or legacy `objrule`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrexRule {
    /// The element's `id` attribute, or `rule-N` by position.
    pub id: String,
    /// The `objectPath` XPath expression.
    pub path: String,
    pub flag: AllowedFlag,
    /// The `objectUse` message.
    pub use_text: String,
    pub values: Vec<ObjectValue>,
}

/// An entry of the SNS rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnsRule {
    /// Full code, with the codes of enclosing levels joined by `-`.
    pub code: String,
    pub title: String,
    /// The element name of the level (`snsSystem`, `snsSubSystem`, ...).
    pub level: String,
}

/// A notation rule: which notations (graphic formats) are allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotationRule {
    pub name: String,
    pub description: String,
}

/// The parsed content of a BREX data module.
#[derive(Debug, Clone, Default)]
pub struct Brex {
    pub rules: Vec<BrexRule>,
    pub sns_rules: Vec<SnsRule>,
    pub notation_rules: Vec<NotationRule>,
    /// The data module code of the BREX itself, if present.
    pub dmc: Option<Dmc>,
}

impl Brex {
    /// Loads and parses a BREX data module from a file.
    pub fn load(path: impl AsRef<Path>) -> Result<Brex, S1000dError> {
        let bytes = std::fs::read(path)?;
        let doc = Document::from_bytes(&bytes)?;
        Ok(Brex::from_document(&doc))
    }

    /// Parses the rules of a BREX document; a document without rules yields an empty `Brex`.
    pub fn from_document(doc: &Document) -> Brex {
        let mut brex = Brex { dmc: Dmc::from_document(doc), ..Brex::default() };
        for node in doc.descendants(doc.root()) {
            match doc.name(node) {
                Some("structureObjectRule") => {
                    let n = brex.rules.len() + 1;
                    brex.rules.push(read_rule(doc, node, n, RuleNames::MODERN));
                }
                Some("objrule") => {
                    let n = brex.rules.len() + 1;
                    brex.rules.push(read_rule(doc, node, n, RuleNames::LEGACY));
                }
                Some("snsRules" | "snsrules") => read_sns(doc, node, "", &mut brex.sns_rules),
                Some("notationRule" | "notrule") => {
                    if let Some(rule) = read_notation(doc, node) {
                        brex.notation_rules.push(rule);
                    }
                }
                _ => {}
            }
        }
        brex
    }

    /// Adds the rules of `other` (for example a project BREX layered over the S1000D one).
    pub fn merge(&mut self, other: Brex) {
        let offset = self.rules.len();
        for (i, mut rule) in other.rules.into_iter().enumerate() {
            if rule.id == format!("rule-{}", i + 1) {
                rule.id = format!("rule-{}", offset + i + 1);
            }
            self.rules.push(rule);
        }
        self.sns_rules.extend(other.sns_rules);
        self.notation_rules.extend(other.notation_rules);
    }
}

struct RuleNames {
    path: &'static str,
    usage: &'static str,
    value: &'static str,
    flag_attrs: [&'static str; 3],
}

impl RuleNames {
    const MODERN: RuleNames = RuleNames {
        path: "objectPath",
        usage: "objectUse",
        value: "objectValue",
        flag_attrs: ["allowedObjectFlag", "allowed", "objflag"],
    };
    const LEGACY: RuleNames = RuleNames {
        path: "objpath",
        usage: "objuse",
        value: "objval",
        flag_attrs: ["allowed", "allowedObjectFlag", "objflag"],
    };
}

fn read_rule(doc: &Document, node: NodeId, ordinal: usize, names: RuleNames) -> BrexRule {
    let path_el = child(doc, node, names.path);
    let flag = path_el
        .and_then(|p| names.flag_attrs.iter().find_map(|a| attr(doc, p, a)))
        .map_or(AllowedFlag::Optional, |f| AllowedFlag::parse(&f));
    BrexRule {
        id: attr(doc, node, "id").unwrap_or_else(|| format!("rule-{ordinal}")),
        path: path_el.map(|p| text(doc, p)).unwrap_or_default(),
        flag,
        use_text: child(doc, node, names.usage).map(|u| text(doc, u)).unwrap_or_default(),
        values: doc
            .element_children(node)
            .filter(|&c| doc.name(c) == Some(names.value))
            .map(|v| read_value(doc, v))
            .collect(),
    }
}

fn read_value(doc: &Document, el: NodeId) -> ObjectValue {
    let form_text = attr(doc, el, "valueForm").or_else(|| attr(doc, el, "valtype")).unwrap_or_default();
    let form = match form_text.trim().to_ascii_lowercase().as_str() {
        "range" => ValueForm::Range,
        "pattern" => ValueForm::Pattern,
        _ => ValueForm::Single,
    };
    let allowed = attr(doc, el, "valueAllowed").unwrap_or_else(|| {
        // Legacy markup spells ranges as val1/val2.
        match (attr(doc, el, "val1"), attr(doc, el, "val2")) {
            (Some(a), Some(b)) if form == ValueForm::Range => format!("{a}~{b}"),
            (Some(a), _) => a,
            _ => String::new(),
        }
    });
    ObjectValue { form, allowed, description: text(doc, el) }
}

const SNS_CODE: [&str; 2] = ["snsCode", "snscode"];
const SNS_TITLE: [&str; 3] = ["snsTitle", "snsName", "snstitle"];

fn first_child_text(doc: &Document, node: NodeId, names: &[&str]) -> Option<String> {
    names.iter().find_map(|n| child(doc, node, n)).map(|e| text(doc, e))
}

fn read_sns(doc: &Document, node: NodeId, parent_code: &str, out: &mut Vec<SnsRule>) {
    let mut code = parent_code.to_string();
    if let Some(own) = first_child_text(doc, node, &SNS_CODE) {
        code = if parent_code.is_empty() { own } else { format!("{parent_code}-{own}") };
        out.push(SnsRule {
            code: code.clone(),
            title: first_child_text(doc, node, &SNS_TITLE).unwrap_or_default(),
            level: doc.name(node).unwrap_or_default().to_string(),
        });
    }
    for c in doc.element_children(node).collect::<Vec<_>>() {
        read_sns(doc, c, &code, out);
    }
}

fn read_notation(doc: &Document, node: NodeId) -> Option<NotationRule> {
    let name = first_child_text(doc, node, &["notationName", "notname"])?;
    let description = first_child_text(doc, node, &["notationUse", "notuse", "notationDescr"]).unwrap_or_default();
    Some(NotationRule { name, description })
}

// ------------------------------------------------------------------ checking

/// How serious a [`BrexViolation`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The document breaks a rule.
    Error,
    /// The rule itself could not be applied (bad XPath, bad pattern).
    Warning,
}

/// A rule broken by a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrexViolation {
    /// The offending element (the owner element for attributes); `None` when a
    /// required object is absent or the rule itself is faulty.
    pub node: Option<NodeId>,
    pub rule_id: String,
    pub message: String,
    pub severity: Severity,
}

/// Checks `doc` against the rules of `brex`.
///
/// Each `objectPath` is evaluated from the document root. Prohibited objects that
/// exist, required objects that do not, and selected values outside the allowed
/// set are reported. Rules that cannot be applied are reported as warnings.
pub fn check_brex(doc: &Document, brex: &Brex) -> Vec<BrexViolation> {
    let mut out = Vec::new();
    let ctx = Context::new();
    for rule in &brex.rules {
        let warn = |message: String| BrexViolation {
            node: None,
            rule_id: rule.id.clone(),
            message,
            severity: Severity::Warning,
        };
        let xpath: XPath = match xylem_xpath::compile(&rule.path) {
            Ok(x) => x,
            Err(e) => {
                out.push(warn(format!("objectPath '{}' is not valid XPath: {e}", rule.path)));
                continue;
            }
        };
        let nodes = match xpath.select_with(doc, XNode::Node(doc.root()), &ctx) {
            Ok(n) => n,
            Err(e) => {
                out.push(warn(format!("objectPath '{}' could not be evaluated: {e}", rule.path)));
                continue;
            }
        };
        let mut matchers = Vec::new();
        for value in &rule.values {
            match value.matcher() {
                Ok(m) => matchers.push(m),
                Err(e) => out.push(warn(format!("invalid pattern '{}': {e}", value.allowed))),
            }
        }
        // A rule whose value patterns cannot be compiled cannot be enforced.
        let constrained = !rule.values.is_empty() && matchers.len() == rule.values.len();
        let in_allowed_set = |v: &str| matchers.iter().any(|m| m.allows(v));
        let error = |node: &XNode, message: String| BrexViolation {
            node: Some(node.owner()),
            rule_id: rule.id.clone(),
            message,
            severity: Severity::Error,
        };
        match rule.flag {
            AllowedFlag::Prohibited => {
                for n in &nodes {
                    let value = node_value(doc, *n);
                    if !constrained {
                        out.push(error(n, format!("{} ({})", describe(rule, "prohibited object present"), location(doc, *n))));
                    } else if in_allowed_set(&value) {
                        out.push(error(
                            n,
                            format!("{} ({}, value '{value}')", describe(rule, "prohibited value"), location(doc, *n)),
                        ));
                    }
                }
            }
            AllowedFlag::Required | AllowedFlag::Optional => {
                if rule.flag == AllowedFlag::Required && nodes.is_empty() {
                    out.push(BrexViolation {
                        node: None,
                        rule_id: rule.id.clone(),
                        message: describe(rule, &format!("required object not found: {}", rule.path)),
                        severity: Severity::Error,
                    });
                }
                if constrained {
                    for n in &nodes {
                        let value = node_value(doc, *n);
                        if !in_allowed_set(&value) {
                            out.push(error(
                                n,
                                format!(
                                    "{} ({}, value '{value}'; allowed: {})",
                                    describe(rule, "value not allowed"),
                                    location(doc, *n),
                                    allowed_summary(rule)
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    out
}

fn describe(rule: &BrexRule, fallback: &str) -> String {
    if rule.use_text.is_empty() { fallback.to_string() } else { rule.use_text.clone() }
}

fn allowed_summary(rule: &BrexRule) -> String {
    rule.values.iter().map(|v| v.allowed.as_str()).collect::<Vec<_>>().join(", ")
}

fn node_value(doc: &Document, n: XNode) -> String {
    let v = string_value(doc, n);
    if n.is_attr() { v } else { v.trim().to_string() }
}

fn location(doc: &Document, n: XNode) -> String {
    match n {
        XNode::Node(id) => doc.path(id),
        XNode::Attr(id, i) => {
            let name = doc.element(id).and_then(|e| e.attrs.get(i)).map(|a| a.name.as_str()).unwrap_or("?");
            format!("{}/@{name}", doc.path(id))
        }
    }
}

#[cfg(test)]
mod tests;
