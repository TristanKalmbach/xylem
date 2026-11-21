//! Computable applicability: evaluating `applic` statements against chosen
//! product attribute and condition values.
//!
//! Issues 4.0 to 6.0 describe applicability with `assert`
//! (`applicPropertyIdent`, `applicPropertyType`, `applicPropertyValues`) and
//! `evaluate` (`andOr`), collected in `referencedApplicGroup` and referenced
//! from content through `applicRefId`. Issue 3.0 has the same model under
//! other names (`actidref`, `actreftype`, `actvalues`, `evaluate/@operator`,
//! `inlineapplics` and `refapplic`), so it is evaluated too.
//!
//! Issues 2.0 to 2.3 have no asserts. Their `applic` lists the models it
//! covers (`model/@model`), each optionally narrowed to versions
//! (`version/@version`) and serial numbers (`serialno` or `version/versrank`,
//! as `single` values and `range from/to`); content points at a `version` by
//! its id through `refapplic`. Those become three properties, `model`,
//! `version` and `serialno` (kind `2.x`), and a statement applies when one of
//! its models matches everything chosen. The specification does not define
//! this evaluation; it is the reading of the markup an author would expect.
//!
//! Evaluation is three-valued. An assert on a property the user left at "any"
//! is unknown, and unknown counts as applicable: the filter only removes what
//! is certainly excluded by the values chosen.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use xylem_dom::{Document, NodeId};

use crate::ident::sections;
use crate::issue::Markup;
use crate::xmlutil::child;

/// Element and attribute names of the applicability model in one markup family.
struct Names {
    ident: &'static str,
    kind: &'static str,
    values: &'static str,
    operator: &'static str,
    reference: &'static str,
}

const MODERN: Names = Names {
    ident: "applicPropertyIdent",
    kind: "applicPropertyType",
    values: "applicPropertyValues",
    operator: "andOr",
    reference: "applicRefId",
};

const LEGACY: Names = Names {
    ident: "actidref",
    kind: "actreftype",
    values: "actvalues",
    operator: "operator",
    reference: "refapplic",
};

fn names(markup: Markup) -> &'static Names {
    match markup {
        Markup::Modern => &MODERN,
        Markup::Legacy => &LEGACY,
    }
}

/// A product attribute or condition, identified the way asserts name it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PropertyKey {
    /// `applicPropertyIdent` (`actidref` in 3.0).
    pub ident: String,
    /// `prodattr` or `condition`.
    pub kind: String,
}

impl PropertyKey {
    pub fn new(ident: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            ident: ident.into(),
            kind: kind.into(),
        }
    }
}

/// A property found in the document's asserts, with the values they mention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicProperty {
    pub key: PropertyKey,
    /// Single values, plus both ends of each range, in sorted order.
    pub values: Vec<String>,
}

/// Chosen values: a property that is absent is "any".
pub type ApplicChoice = BTreeMap<PropertyKey, String>;

/// Result of evaluating an applicability statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    /// Depends on a property left at "any", or cannot be computed.
    Unknown,
}

impl Truth {
    fn and(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::False, _) | (_, Truth::False) => Truth::False,
            (Truth::True, Truth::True) => Truth::True,
            _ => Truth::Unknown,
        }
    }

    fn or(self, other: Truth) -> Truth {
        match (self, other) {
            (Truth::True, _) | (_, Truth::True) => Truth::True,
            (Truth::False, Truth::False) => Truth::False,
            _ => Truth::Unknown,
        }
    }

    /// Unknown is treated as applicable when filtering.
    pub fn is_applicable(self) -> bool {
        self != Truth::False
    }
}

fn markup_of(doc: &Document) -> Markup {
    sections(doc).map_or(Markup::Modern, |s| s.markup)
}

/// Property kind used for the issue 2.x model/version/serial properties.
pub const KIND_2X: &str = "2.x";

/// Every property named by an assert in the document, sorted by identifier.
pub fn applic_properties(doc: &Document) -> Vec<ApplicProperty> {
    let n = names(markup_of(doc));
    let mut found: BTreeMap<PropertyKey, BTreeSet<String>> = BTreeMap::new();
    // Issue 2.x statements.
    for model in doc.descendants(doc.root()).filter(|&e| is_2x_model(doc, e)) {
        if let Some(m) = doc.attr(model, "model") {
            found
                .entry(PropertyKey::new("model", KIND_2X))
                .or_default()
                .insert(m.to_string());
        }
        let versions: Vec<NodeId> = doc
            .element_children(model)
            .filter(|&c| doc.name(c) == Some("version"))
            .collect();
        for &v in &versions {
            if let Some(name) = doc.attr(v, "version") {
                found
                    .entry(PropertyKey::new("version", KIND_2X))
                    .or_default()
                    .insert(name.to_string());
            }
        }
        let mut serials = serial_ranges(doc, model);
        for &v in &versions {
            serials.extend(serial_ranges(doc, v));
        }
        for (low, high) in serials {
            let set = found
                .entry(PropertyKey::new("serialno", KIND_2X))
                .or_default();
            set.insert(low);
            if let Some(h) = high {
                set.insert(h);
            }
        }
    }
    for el in doc.descendants(doc.root()) {
        if doc.name(el) != Some("assert") {
            continue;
        }
        let (Some(ident), Some(kind)) = (doc.attr(el, n.ident), doc.attr(el, n.kind)) else {
            continue;
        };
        let values = found.entry(PropertyKey::new(ident, kind)).or_default();
        for token in doc.attr(el, n.values).unwrap_or_default().split('|') {
            let (low, high) = split_range(token);
            for v in [Some(low), high].into_iter().flatten() {
                if !v.is_empty() {
                    values.insert(v.to_string());
                }
            }
        }
    }
    found
        .into_iter()
        .map(|(key, values)| ApplicProperty {
            key,
            values: values.into_iter().collect(),
        })
        .collect()
}

fn split_range(token: &str) -> (&str, Option<&str>) {
    match token.split_once('~') {
        Some((low, high)) => (low.trim(), Some(high.trim())),
        None => (token.trim(), None),
    }
}

/// Orders two property values: numerically when both are numbers, else as text.
fn compare_values(a: &str, b: &str) -> Ordering {
    match (a.parse::<f64>(), b.parse::<f64>()) {
        (Ok(x), Ok(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => a.cmp(b),
    }
}

/// True when `value` is one of the `|`-separated values or `~` ranges in `spec`.
pub fn values_match(spec: &str, value: &str) -> bool {
    let value = value.trim();
    spec.split('|').any(|token| match split_range(token) {
        (single, None) => single == value,
        (low, Some(high)) => {
            compare_values(value, low) != Ordering::Less
                && compare_values(value, high) != Ordering::Greater
        }
    })
}

/// A `model` element inside an issue 2.x `applic`.
fn is_2x_model(doc: &Document, el: NodeId) -> bool {
    doc.name(el) == Some("model")
        && doc
            .parent(el)
            .is_some_and(|p| doc.name(p) == Some("applic"))
}

/// Serial numbers an element (model or version) is limited to, as
/// (single or low, high) pairs from its `serialno` and `versrank` children,
/// each holding `single` text and `range from/to`.
fn serial_ranges(doc: &Document, el: NodeId) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for holder in doc
        .element_children(el)
        .filter(|&c| matches!(doc.name(c), Some("serialno" | "versrank")))
    {
        for item in doc.element_children(holder) {
            match doc.name(item) {
                Some("single") => {
                    let t: String = doc.descendants(item).filter_map(|n| doc.text(n)).collect();
                    if !t.trim().is_empty() {
                        out.push((t.trim().to_string(), None));
                    }
                }
                Some("range") => {
                    if let (Some(f), Some(t)) = (doc.attr(item, "from"), doc.attr(item, "to")) {
                        out.push((f.trim().to_string(), Some(t.trim().to_string())));
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// One part of a 2.x statement. Nothing chosen is unknown (unless the
/// statement doesn't restrict it); otherwise it holds when a value matches.
fn component(chosen: Option<&String>, allowed: &[(String, Option<String>)]) -> Truth {
    match chosen {
        _ if allowed.is_empty() => Truth::True,
        None => Truth::Unknown,
        Some(v) => {
            let spec: Vec<String> = allowed
                .iter()
                .map(|(l, h)| match h {
                    Some(h) => format!("{l}~{h}"),
                    None => l.clone(),
                })
                .collect();
            if values_match(&spec.join("|"), v) {
                Truth::True
            } else {
                Truth::False
            }
        }
    }
}

fn chosen_2x<'a>(choice: &'a ApplicChoice, ident: &str) -> Option<&'a String> {
    choice.get(&PropertyKey::new(ident, KIND_2X))
}

/// A 2.x model, optionally narrowed to one of its versions.
fn evaluate_2x_model(
    doc: &Document,
    model: NodeId,
    only: Option<NodeId>,
    choice: &ApplicChoice,
) -> Truth {
    let name = doc.attr(model, "model").unwrap_or("").to_string();
    let model_t = component(chosen_2x(choice, "model"), &[(name, None)]);
    let versions: Vec<NodeId> = match only {
        Some(v) => vec![v],
        None => doc
            .element_children(model)
            .filter(|&c| doc.name(c) == Some("version"))
            .collect(),
    };
    let version_names: Vec<(String, Option<String>)> = versions
        .iter()
        .filter_map(|&v| doc.attr(v, "version").map(|n| (n.to_string(), None)))
        .collect();
    let version_t = component(chosen_2x(choice, "version"), &version_names);
    let mut serials = serial_ranges(doc, model);
    for &v in &versions {
        serials.extend(serial_ranges(doc, v));
    }
    let serial_t = component(chosen_2x(choice, "serialno"), &serials);
    model_t.and(version_t).and(serial_t)
}

/// Evaluates an `applic`, `assert` or `evaluate` element.
pub fn evaluate(doc: &Document, el: NodeId, choice: &ApplicChoice) -> Truth {
    evaluate_with(doc, el, choice, names(markup_of(doc)))
}

fn evaluate_with(doc: &Document, el: NodeId, choice: &ApplicChoice, n: &Names) -> Truth {
    match doc.name(el) {
        Some("applic") if doc.element_children(el).any(|c| is_2x_model(doc, c)) => {
            // Issue 2.x: applies when any listed model does.
            let mut models = doc.element_children(el).filter(|&c| is_2x_model(doc, c));
            let first = models
                .next()
                .map_or(Truth::True, |m| evaluate_2x_model(doc, m, None, choice));
            models.fold(first, |acc, m| {
                acc.or(evaluate_2x_model(doc, m, None, choice))
            })
        }
        // Issue 2.x content points at one version of one model.
        Some("version") => match doc.parent(el).filter(|&m| is_2x_model(doc, m)) {
            Some(model) => evaluate_2x_model(doc, model, Some(el), choice),
            None => Truth::Unknown,
        },
        Some("applic") => {
            // Display text alone states no computable condition.
            let logic = doc
                .element_children(el)
                .find(|&c| matches!(doc.name(c), Some("assert" | "evaluate")));
            logic.map_or(Truth::True, |c| evaluate_with(doc, c, choice, n))
        }
        Some("assert") => {
            let (Some(ident), Some(kind), Some(values)) = (
                doc.attr(el, n.ident),
                doc.attr(el, n.kind),
                doc.attr(el, n.values),
            ) else {
                // An assert with only text is a statement for the reader.
                return Truth::True;
            };
            match choice.get(&PropertyKey::new(ident, kind)) {
                None => Truth::Unknown,
                Some(v) if values_match(values, v) => Truth::True,
                Some(_) => Truth::False,
            }
        }
        Some("evaluate") => {
            let or = doc.attr(el, n.operator) == Some("or");
            let mut parts = doc
                .element_children(el)
                .filter(|&c| matches!(doc.name(c), Some("assert" | "evaluate")))
                .map(|c| evaluate_with(doc, c, choice, n));
            let first = parts.next().unwrap_or(Truth::Unknown);
            parts.fold(first, |acc, t| if or { acc.or(t) } else { acc.and(t) })
        }
        _ => Truth::Unknown,
    }
}

/// The module-level `applic` in the status section.
pub fn module_applic(doc: &Document) -> Option<NodeId> {
    let s = sections(doc)?;
    let root = doc.document_element()?;
    let status = match s.markup {
        Markup::Modern => {
            let section = child(doc, root, "identAndStatusSection")?;
            doc.element_children(section)
                .find(|&c| matches!(doc.name(c), Some("dmStatus" | "pmStatus")))?
        }
        Markup::Legacy => {
            let idstatus = child(doc, root, "idstatus")?;
            doc.element_children(idstatus)
                .find(|&c| matches!(doc.name(c), Some("status" | "pmstatus")))?
        }
    };
    child(doc, status, "applic")
}

/// Applicability of a whole document under one choice of values.
#[derive(Debug, Clone, Default)]
pub struct ApplicFilterResult {
    /// The module-level statement's result (`None` when there is none).
    pub module: Option<Truth>,
    /// Elements whose own applicability evaluates false, outermost first.
    /// When the module itself is inapplicable this is the `content` element.
    pub excluded: Vec<NodeId>,
    /// `applicRefId` values that name no `applic` in the document.
    pub dangling: Vec<String>,
}

impl ApplicFilterResult {
    /// The excluded elements and everything inside them.
    pub fn excluded_subtrees(&self, doc: &Document) -> HashSet<NodeId> {
        let mut out = HashSet::new();
        for &el in &self.excluded {
            out.extend(doc.descendants(el));
        }
        out
    }
}

/// Finds the elements that do not apply under `choice`.
///
/// An element inside an excluded element is not listed again: it is hidden
/// with its ancestor.
pub fn filter_document(doc: &Document, choice: &ApplicChoice) -> ApplicFilterResult {
    let markup = markup_of(doc);
    let n = names(markup);
    let mut result = ApplicFilterResult {
        module: module_applic(doc).map(|a| evaluate_with(doc, a, choice, n)),
        ..Default::default()
    };
    let Some(root) = doc.document_element() else {
        return result;
    };
    let content = child(doc, root, "content");
    if result.module == Some(Truth::False)
        && let Some(content) = content
    {
        result.excluded.push(content);
        return result;
    }
    // `applic` statements, and in issue 2.x the `version`s content points at.
    let by_id: HashMap<&str, NodeId> = doc
        .descendants(root)
        .filter(|&e| {
            doc.name(e) == Some("applic")
                || (doc.name(e) == Some("version")
                    && doc.parent(e).is_some_and(|m| is_2x_model(doc, m)))
        })
        .filter_map(|e| Some((doc.attr(e, "id")?, e)))
        .collect();
    let mut memo: HashMap<NodeId, Truth> = HashMap::new();
    let walk = doc.descendants(root);
    let mut skip_inside: Option<NodeId> = None;
    for el in walk {
        if let Some(outer) = skip_inside {
            if doc.ancestors(el).any(|a| a == outer) {
                continue;
            }
            skip_inside = None;
        }
        let Some(reference) = doc.attr(el, n.reference) else {
            continue;
        };
        // An element may name several statements; all of them must hold.
        let mut truth = Truth::True;
        for id in reference.split_whitespace() {
            match by_id.get(id) {
                Some(&applic) => {
                    let t = *memo
                        .entry(applic)
                        .or_insert_with(|| evaluate_with(doc, applic, choice, n));
                    truth = truth.and(t);
                }
                None => {
                    if !result.dangling.iter().any(|d| d == id) {
                        result.dangling.push(id.to_string());
                    }
                }
            }
        }
        if truth == Truth::False {
            result.excluded.push(el);
            skip_inside = Some(el);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const DM: &str = r#"<dmodule>
<identAndStatusSection>
<dmAddress><dmIdent><dmCode modelIdentCode="X" systemDiffCode="A" systemCode="00" subSystemCode="0" subSubSystemCode="0" assyCode="00" disassyCode="00" disassyCodeVariant="A" infoCode="040" infoCodeVariant="A" itemLocationCode="A"/></dmIdent><dmAddressItems/></dmAddress>
<dmStatus><applic><displayText><simplePara>Models A and B</simplePara></displayText>
<evaluate andOr="or">
<assert applicPropertyIdent="model" applicPropertyType="prodattr" applicPropertyValues="A"/>
<assert applicPropertyIdent="model" applicPropertyType="prodattr" applicPropertyValues="B"/>
</evaluate></applic></dmStatus>
</identAndStatusSection>
<content>
<referencedApplicGroup>
<applic id="app-a"><assert applicPropertyIdent="model" applicPropertyType="prodattr" applicPropertyValues="A"/></applic>
<applic id="app-b-late"><evaluate andOr="and">
<assert applicPropertyIdent="model" applicPropertyType="prodattr" applicPropertyValues="B"/>
<assert applicPropertyIdent="serialno" applicPropertyType="prodattr" applicPropertyValues="100~199|250"/>
</evaluate></applic>
<applic id="app-text"><displayText><simplePara>Some</simplePara></displayText></applic>
</referencedApplicGroup>
<description>
<para id="p1" applicRefId="app-a">Model A only <emphasis>here</emphasis>.</para>
<para id="p2" applicRefId="app-b-late">Model B, late serials.</para>
<para id="p3" applicRefId="app-text">Always.</para>
<para id="p4">No statement.</para>
<para id="p5" applicRefId="missing">Dangling.</para>
<levelledPara id="l1" applicRefId="app-a"><para id="p6" applicRefId="app-b-late">nested</para></levelledPara>
</description>
</content>
</dmodule>"#;

    fn by_id(doc: &Document, id: &str) -> NodeId {
        doc.descendants(doc.root())
            .find(|&n| doc.attr(n, "id") == Some(id))
            .unwrap()
    }

    fn choose(pairs: &[(&str, &str)]) -> ApplicChoice {
        pairs
            .iter()
            .map(|(k, v)| (PropertyKey::new(*k, "prodattr"), v.to_string()))
            .collect()
    }

    fn excluded_ids(doc: &Document, choice: &ApplicChoice) -> Vec<String> {
        filter_document(doc, choice)
            .excluded
            .iter()
            .map(|&n| doc.attr(n, "id").unwrap_or("?").to_string())
            .collect()
    }

    #[test]
    fn value_lists_and_ranges() {
        assert!(values_match("A|B|C", "B"));
        assert!(!values_match("A|B|C", "D"));
        assert!(values_match("100~199|250", "150"));
        assert!(values_match("100~199|250", "100"));
        assert!(values_match("100~199|250", "199"));
        assert!(values_match("100~199|250", "250"));
        assert!(!values_match("100~199|250", "200"));
        // Numbers compare as numbers, not text.
        assert!(values_match("9~10", "9.5"));
        assert!(!values_match("2~10", "100"));
        // Alphanumeric serials compare as text.
        assert!(values_match("N001~N099", "N050"));
        assert!(!values_match("N001~N099", "N100"));
    }

    #[test]
    fn properties_are_collected_with_range_ends() {
        let doc = Document::from_str(DM).unwrap();
        let props = applic_properties(&doc);
        assert_eq!(props.len(), 2);
        assert_eq!(props[0].key, PropertyKey::new("model", "prodattr"));
        assert_eq!(props[0].values, ["A", "B"]);
        assert_eq!(props[1].key.ident, "serialno");
        assert_eq!(props[1].values, ["100", "199", "250"]);
    }

    #[test]
    fn three_valued_logic() {
        let doc = Document::from_str(DM).unwrap();
        let late = by_id(&doc, "app-b-late");
        assert_eq!(evaluate(&doc, late, &ApplicChoice::new()), Truth::Unknown);
        assert_eq!(
            evaluate(&doc, late, &choose(&[("model", "A")])),
            Truth::False
        );
        assert_eq!(
            evaluate(&doc, late, &choose(&[("model", "B")])),
            Truth::Unknown
        );
        assert_eq!(
            evaluate(&doc, late, &choose(&[("model", "B"), ("serialno", "120")])),
            Truth::True
        );
        assert_eq!(
            evaluate(&doc, late, &choose(&[("model", "B"), ("serialno", "200")])),
            Truth::False
        );
        // Text-only statements always apply.
        let text = by_id(&doc, "app-text");
        assert_eq!(
            evaluate(&doc, text, &choose(&[("model", "Z")])),
            Truth::True
        );
    }

    #[test]
    fn filtering_marks_outermost_inapplicable_elements() {
        let doc = Document::from_str(DM).unwrap();
        // Nothing chosen: nothing is certainly excluded.
        assert!(excluded_ids(&doc, &ApplicChoice::new()).is_empty());
        assert_eq!(
            excluded_ids(&doc, &choose(&[("model", "B"), ("serialno", "120")])),
            ["p1", "l1"]
        );
        // Under model B, p6 is inapplicable as well, but it sits inside l1,
        // which is already listed. Under model A, l1 applies and p6 does not.
        assert_eq!(excluded_ids(&doc, &choose(&[("model", "A")])), ["p2", "p6"]);
        let r = filter_document(&doc, &choose(&[("model", "A")]));
        assert_eq!(r.module, Some(Truth::True));
        assert_eq!(r.dangling, ["missing"]);
        let all = r.excluded_subtrees(&doc);
        assert!(all.contains(&by_id(&doc, "p2")));
        assert!(!all.contains(&by_id(&doc, "p1")));
        assert!(!all.contains(&by_id(&doc, "l1")));
        let r = filter_document(&doc, &choose(&[("model", "B"), ("serialno", "120")]));
        assert!(r.excluded_subtrees(&doc).contains(&by_id(&doc, "p6")));
    }

    #[test]
    fn inapplicable_module_excludes_its_content() {
        let doc = Document::from_str(DM).unwrap();
        let r = filter_document(&doc, &choose(&[("model", "C")]));
        assert_eq!(r.module, Some(Truth::False));
        assert_eq!(r.excluded.len(), 1);
        assert_eq!(doc.name(r.excluded[0]), Some("content"));
    }

    #[test]
    fn issue_3_0_names() {
        let xml = r#"<dmodule><idstatus><dmaddres><dmc><avee><modelic>X</modelic></avee></dmc></dmaddres>
<status><applic><displaytext>All</displaytext></applic></status></idstatus>
<content><refs><norefs/></refs><descript>
<inlineapplics>
<applic id="a1"><displaytext>Pre</displaytext><evaluate operator="or">
<assert actidref="mod" actreftype="condition" actvalues="pre"/>
<assert actidref="mod" actreftype="condition" actvalues="none"/>
</evaluate></applic>
</inlineapplics>
<para0 id="x" refapplic="a1"><title>T</title></para0>
</descript></content></dmodule>"#;
        let doc = Document::from_str(xml).unwrap();
        let props = applic_properties(&doc);
        assert_eq!(props.len(), 1);
        assert_eq!(props[0].key, PropertyKey::new("mod", "condition"));
        let mut choice = ApplicChoice::new();
        choice.insert(PropertyKey::new("mod", "condition"), "post".into());
        assert_eq!(filter_document(&doc, &choice).excluded.len(), 1);
        choice.insert(PropertyKey::new("mod", "condition"), "pre".into());
        assert!(filter_document(&doc, &choice).excluded.is_empty());
    }

    #[test]
    fn issue_2_x_models_versions_and_serials() {
        let xml = r#"<dmodule><idstatus><dmaddres><dmc><avee><modelic>X</modelic></avee></dmc></dmaddres>
<status><applic><type>Lift</type>
<model model="L100"><version version="A" id="v-a"/><version version="B" id="v-b"/></model>
<model model="L200"><version version="A" id="v2-a"><versrank><range from="500" to="999"/></versrank></version></model>
</applic></status></idstatus>
<content><descript>
<para0 id="all"><title>Both</title></para0>
<para0 id="l100a" refapplic="v-a"><title>L100 A</title></para0>
<para0 id="l100b" refapplic="v-b"><title>L100 B</title></para0>
<para0 id="late" refapplic="v2-a"><title>L200 late</title></para0>
</descript></content></dmodule>"#;
        let doc = Document::from_str(xml).unwrap();
        let props = applic_properties(&doc);
        let key = |i: &str| PropertyKey::new(i, KIND_2X);
        let values = |i: &str| {
            props
                .iter()
                .find(|p| p.key == key(i))
                .map(|p| p.values.clone())
        };
        assert_eq!(values("model"), Some(vec!["L100".into(), "L200".into()]));
        assert_eq!(values("version"), Some(vec!["A".into(), "B".into()]));
        assert_eq!(values("serialno"), Some(vec!["500".into(), "999".into()]));

        let pick = |pairs: &[(&str, &str)]| -> ApplicChoice {
            pairs.iter().map(|(k, v)| (key(k), v.to_string())).collect()
        };
        let excluded = |c: &ApplicChoice| -> Vec<String> {
            filter_document(&doc, c)
                .excluded
                .iter()
                .map(|&n| doc.attr(n, "id").unwrap_or("?").to_string())
                .collect()
        };
        assert!(
            excluded(&ApplicChoice::new()).is_empty(),
            "nothing chosen excludes nothing"
        );
        assert_eq!(excluded(&pick(&[("model", "L100")])), ["late"]);
        assert_eq!(
            excluded(&pick(&[("model", "L100"), ("version", "B")])),
            ["l100a", "late"]
        );
        assert_eq!(
            excluded(&pick(&[("model", "L200"), ("serialno", "600")])),
            ["l100a", "l100b"]
        );
        // An L200 below serial 500 isn't covered by the module at all.
        assert_eq!(
            filter_document(&doc, &pick(&[("model", "L200"), ("serialno", "100")])).module,
            Some(Truth::False)
        );
        // A model the module doesn't cover excludes all its content.
        let r = filter_document(&doc, &pick(&[("model", "L300")]));
        assert_eq!(r.module, Some(Truth::False));
        assert_eq!(doc.name(r.excluded[0]), Some("content"));
        assert!(r.dangling.is_empty());
    }
}
