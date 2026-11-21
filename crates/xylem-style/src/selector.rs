//! Selector parsing, specificity and matching (CSS 2.1 plus XMetaL names).

use xylem_dom::{Document, NodeId, NodeKind};

use crate::token::Token;

/// Which XMetaL pseudo-element name a compound targets.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Special {
    /// `$DOCUMENT`
    Document,
    /// `$COMMENT`
    Comment,
    /// `$PROCINS`
    ProcessingInstruction,
    /// `$MARKSEC` (CDATA sections)
    MarkedSection,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Subject {
    Any,
    /// Element local name (case-sensitive, as XML is).
    Name(String),
    Special(Special),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum AttrOp {
    Exists,
    Equals,
    /// `~=`: whitespace-separated word.
    Includes,
    /// `|=`: equal, or prefix followed by `-`.
    DashMatch,
    Prefix,
    Suffix,
    Substring,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Condition {
    Attr {
        name: String,
        op: AttrOp,
        value: String,
    },
    FirstChild,
    LastChild,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Compound {
    pub subject: Subject,
    pub conditions: Vec<Condition>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Combinator {
    Descendant,
    Child,
    AdjacentSibling,
    GeneralSibling,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum PseudoElement {
    Before,
    After,
}

/// A complex selector, stored left to right.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Selector {
    pub compounds: Vec<Compound>,
    /// `combinators[i]` joins `compounds[i]` and `compounds[i + 1]`.
    pub combinators: Vec<Combinator>,
    pub pseudo: Option<PseudoElement>,
    /// Packed (ids, attributes/pseudo-classes, types/pseudo-elements).
    pub specificity: u32,
    /// [`name_hash`] of element names that must appear among the subject's
    /// ancestors (reached only through descendant/child combinators); used to
    /// reject candidates cheaply before the full match.
    pub ancestor_hashes: Vec<u32>,
}

/// Maximum ancestor names remembered per selector for quick rejection.
const MAX_ANCESTOR_HASHES: usize = 4;

/// FNV-1a hash of an element local name.
pub(crate) fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    })
}

fn ancestor_hashes(compounds: &[Compound], combinators: &[Combinator]) -> Vec<u32> {
    let mut hashes = Vec::new();
    for i in (0..combinators.len()).rev() {
        if !matches!(combinators[i], Combinator::Descendant | Combinator::Child) {
            break;
        }
        if let Subject::Name(name) = &compounds[i].subject {
            let hash = name_hash(name);
            if !hashes.contains(&hash) && hashes.len() < MAX_ANCESTOR_HASHES {
                hashes.push(hash);
            }
        }
    }
    hashes
}

/// Per-node sibling facts used by sibling combinators and `:first-child`.
pub(crate) struct Siblings {
    prev_element: Vec<Option<NodeId>>,
    no_next_element: Vec<bool>,
}

impl Siblings {
    pub(crate) fn build(doc: &Document) -> Self {
        let n = doc.node_count();
        let mut prev_element = vec![None; n];
        let mut no_next_element = vec![true; n];
        for node in doc.descendants(doc.root()) {
            let children = doc.children(node);
            let mut last_element: Option<NodeId> = None;
            for &child in children {
                prev_element[child.index()] = last_element;
                if doc.is_element(child) {
                    last_element = Some(child);
                }
            }
            let mut element_follows = false;
            for &child in children.iter().rev() {
                no_next_element[child.index()] = !element_follows;
                element_follows |= doc.is_element(child);
            }
        }
        Self {
            prev_element,
            no_next_element,
        }
    }

    pub(crate) fn prev_element(&self, id: NodeId) -> Option<NodeId> {
        self.prev_element.get(id.index()).copied().flatten()
    }

    fn is_last(&self, id: NodeId) -> bool {
        self.no_next_element
            .get(id.index())
            .copied()
            .unwrap_or(true)
    }
}

// ------------------------------------------------------------------ parsing

/// Parses a comma-separated selector list. Fails as a whole if any member is
/// invalid (CSS drops the entire rule in that case); empty members are skipped.
pub(crate) fn parse_selector_list(tokens: &[Token]) -> Result<Vec<Selector>, String> {
    let mut selectors = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for i in 0..=tokens.len() {
        if i < tokens.len() {
            match tokens[i] {
                Token::LBracket | Token::LParen | Token::Function(_) => depth += 1,
                Token::RBracket | Token::RParen => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if i == tokens.len() || (depth == 0 && tokens[i] == Token::Comma) {
            let member = &tokens[start..i];
            // XMetaL tolerates stray commas (`a, b, {`): empty members are skipped.
            if !member.iter().all(|t| *t == Token::Whitespace) {
                selectors.push(parse_selector(member)?);
            }
            start = i + 1;
        }
    }
    if selectors.is_empty() {
        return Err("empty selector".to_string());
    }
    Ok(selectors)
}

struct Cursor<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn peek_at(&self, offset: usize) -> Option<&'a Token> {
        self.tokens.get(self.pos + offset)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let t = self.tokens.get(self.pos);
        self.pos += 1;
        t
    }

    fn skip_ws(&mut self) -> bool {
        let start = self.pos;
        while self.peek() == Some(&Token::Whitespace) {
            self.pos += 1;
        }
        self.pos > start
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }
}

fn parse_selector(tokens: &[Token]) -> Result<Selector, String> {
    let mut cur = Cursor { tokens, pos: 0 };
    cur.skip_ws();
    if cur.at_end() {
        return Err("empty selector".to_string());
    }
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut pseudo = None;
    loop {
        let (compound, compound_pseudo) = parse_compound(&mut cur)?;
        compounds.push(compound);
        let had_space = cur.skip_ws();
        if let Some(p) = compound_pseudo {
            if !cur.at_end() {
                return Err("pseudo-element must end the selector".to_string());
            }
            pseudo = Some(p);
            break;
        }
        if cur.at_end() {
            break;
        }
        let combinator = match cur.peek() {
            Some(Token::Delim('>')) => Combinator::Child,
            Some(Token::Delim('+')) => Combinator::AdjacentSibling,
            Some(Token::Delim('~')) => Combinator::GeneralSibling,
            _ if had_space => Combinator::Descendant,
            other => return Err(format!("unexpected token {other:?} in selector")),
        };
        if combinator != Combinator::Descendant {
            cur.next();
            cur.skip_ws();
            if cur.at_end() {
                return Err("selector ends with a combinator".to_string());
            }
        }
        combinators.push(combinator);
    }
    let specificity = specificity(&compounds, pseudo);
    let ancestor_hashes = ancestor_hashes(&compounds, &combinators);
    Ok(Selector {
        compounds,
        combinators,
        pseudo,
        specificity,
        ancestor_hashes,
    })
}

fn special_from(name: &str) -> Result<Special, String> {
    match name.to_ascii_lowercase().as_str() {
        "$document" => Ok(Special::Document),
        "$comment" => Ok(Special::Comment),
        "$procins" => Ok(Special::ProcessingInstruction),
        "$marksec" => Ok(Special::MarkedSection),
        _ => Err(format!("unsupported XMetaL pseudo-element `{name}`")),
    }
}

fn subject_from(name: &str) -> Result<Subject, String> {
    if name.starts_with('$') {
        special_from(name).map(Subject::Special)
    } else {
        Ok(Subject::Name(name.to_string()))
    }
}

/// Consumes an optional `prefix|` after a type or `*` selector, returning the
/// local part if a namespace separator followed.
fn skip_namespace_prefix(cur: &mut Cursor<'_>) -> Result<Option<Subject>, String> {
    if cur.peek() != Some(&Token::Delim('|')) {
        return Ok(None);
    }
    match cur.peek_at(1) {
        Some(Token::Ident(local)) => {
            let subject = subject_from(local)?;
            cur.pos += 2;
            Ok(Some(subject))
        }
        Some(Token::Delim('*')) => {
            cur.pos += 2;
            Ok(Some(Subject::Any))
        }
        _ => Ok(None),
    }
}

fn parse_compound(cur: &mut Cursor<'_>) -> Result<(Compound, Option<PseudoElement>), String> {
    let mut subject = None;
    match cur.peek() {
        Some(Token::Ident(name)) => {
            cur.next();
            subject = Some(subject_from(name)?);
            if let Some(local) = skip_namespace_prefix(cur)? {
                subject = Some(local);
            }
        }
        Some(Token::Delim('*')) => {
            cur.next();
            subject = Some(Subject::Any);
            if let Some(local) = skip_namespace_prefix(cur)? {
                subject = Some(local);
            }
        }
        Some(Token::Delim('|')) => {
            // `|name`: no-namespace type selector.
            if let Some(local) = skip_namespace_prefix(cur)? {
                subject = Some(local);
            }
        }
        _ => {}
    }
    let mut conditions = Vec::new();
    let mut pseudo = None;
    loop {
        match cur.peek() {
            Some(Token::Hash(id)) => {
                cur.next();
                conditions.push(Condition::Attr {
                    name: "id".into(),
                    op: AttrOp::Equals,
                    value: id.clone(),
                });
            }
            Some(Token::Delim('.')) => {
                cur.next();
                let Some(Token::Ident(class)) = cur.next() else {
                    return Err("expected class name after `.`".to_string());
                };
                conditions.push(Condition::Attr {
                    name: "class".into(),
                    op: AttrOp::Includes,
                    value: class.clone(),
                });
            }
            Some(Token::LBracket) => {
                cur.next();
                conditions.push(parse_attribute(cur)?);
            }
            Some(Token::Colon) => {
                cur.next();
                let double = cur.peek() == Some(&Token::Colon);
                if double {
                    cur.next();
                }
                let Some(Token::Ident(name)) = cur.next() else {
                    return Err("expected pseudo-class or pseudo-element name".to_string());
                };
                match (name.to_ascii_lowercase().as_str(), double) {
                    ("before", _) => pseudo = Some(PseudoElement::Before),
                    ("after", _) => pseudo = Some(PseudoElement::After),
                    ("first-child", false) => conditions.push(Condition::FirstChild),
                    ("last-child", false) => conditions.push(Condition::LastChild),
                    (other, _) => return Err(format!("unsupported pseudo `:{other}`")),
                }
                if pseudo.is_some() {
                    break;
                }
            }
            Some(Token::Function(name)) => {
                return Err(format!("unsupported selector function `{name}()`"));
            }
            _ => break,
        }
    }
    if subject.is_none() && conditions.is_empty() && pseudo.is_none() {
        return Err(format!("unexpected token {:?} in selector", cur.peek()));
    }
    let compound = Compound {
        subject: subject.unwrap_or(Subject::Any),
        conditions,
    };
    Ok((compound, pseudo))
}

fn parse_attribute(cur: &mut Cursor<'_>) -> Result<Condition, String> {
    cur.skip_ws();
    let Some(Token::Ident(first)) = cur.next() else {
        return Err("expected attribute name".to_string());
    };
    let mut name = first.clone();
    // `ns|name` (prefix dropped) and `xml:lang` (kept as written).
    match (cur.peek(), cur.peek_at(1)) {
        (Some(Token::Delim('|')), Some(Token::Ident(local))) => {
            name = local.clone();
            cur.pos += 2;
        }
        (Some(Token::Colon), Some(Token::Ident(local))) => {
            name = format!("{name}:{local}");
            cur.pos += 2;
        }
        _ => {}
    }
    cur.skip_ws();
    let op = match cur.next() {
        Some(Token::RBracket) => {
            return Ok(Condition::Attr {
                name,
                op: AttrOp::Exists,
                value: String::new(),
            });
        }
        Some(Token::Delim('=')) => AttrOp::Equals,
        Some(Token::MatchOp('~')) => AttrOp::Includes,
        Some(Token::MatchOp('|')) => AttrOp::DashMatch,
        Some(Token::MatchOp('^')) => AttrOp::Prefix,
        Some(Token::MatchOp('$')) => AttrOp::Suffix,
        Some(Token::MatchOp('*')) => AttrOp::Substring,
        other => return Err(format!("invalid attribute operator {other:?}")),
    };
    cur.skip_ws();
    let value = match cur.next() {
        Some(Token::Ident(v) | Token::Str(v)) => v.clone(),
        Some(Token::Number(_, text)) => text.clone(),
        other => return Err(format!("invalid attribute value {other:?}")),
    };
    cur.skip_ws();
    match cur.next() {
        Some(Token::RBracket) => Ok(Condition::Attr { name, op, value }),
        other => Err(format!(
            "expected `]` after attribute selector, found {other:?}"
        )),
    }
}

fn specificity(compounds: &[Compound], pseudo: Option<PseudoElement>) -> u32 {
    let (mut ids, mut attrs, mut types) = (0u32, 0u32, u32::from(pseudo.is_some()));
    for compound in compounds {
        if !matches!(compound.subject, Subject::Any) {
            types += 1;
        }
        for condition in &compound.conditions {
            match condition {
                Condition::Attr {
                    name,
                    op: AttrOp::Equals,
                    ..
                } if name == "id" => ids += 1,
                _ => attrs += 1,
            }
        }
    }
    (ids.min(255) << 16) | (attrs.min(255) << 8) | types.min(255)
}

// ----------------------------------------------------------------- matching

fn attribute_value<'d>(doc: &'d Document, node: NodeId, name: &str) -> Option<&'d str> {
    match doc.kind(node) {
        NodeKind::Element(e) => e.attr(name),
        // XMetaL exposes processing instructions as pseudo-attributes.
        NodeKind::Pi { target, data } => match name {
            "xm-pi-target" => Some(target),
            "xm-pi-data" => Some(data),
            _ => None,
        },
        _ => None,
    }
}

impl AttrOp {
    fn test(self, actual: &str, expected: &str) -> bool {
        match self {
            AttrOp::Exists => true,
            AttrOp::Equals => actual == expected,
            AttrOp::Includes => {
                !expected.is_empty() && actual.split_whitespace().any(|w| w == expected)
            }
            AttrOp::DashMatch => {
                actual == expected
                    || actual
                        .strip_prefix(expected)
                        .is_some_and(|rest| rest.starts_with('-'))
            }
            AttrOp::Prefix => !expected.is_empty() && actual.starts_with(expected),
            AttrOp::Suffix => !expected.is_empty() && actual.ends_with(expected),
            AttrOp::Substring => !expected.is_empty() && actual.contains(expected),
        }
    }
}

impl Compound {
    fn matches(&self, doc: &Document, siblings: &Siblings, node: NodeId) -> bool {
        let subject_ok = match (&self.subject, doc.kind(node)) {
            (Subject::Any, NodeKind::Element(_)) => true,
            (Subject::Name(name), NodeKind::Element(e)) => e.local_name() == name,
            (Subject::Special(Special::Document), NodeKind::Document)
            | (Subject::Special(Special::Comment), NodeKind::Comment(_))
            | (Subject::Special(Special::ProcessingInstruction), NodeKind::Pi { .. })
            | (Subject::Special(Special::MarkedSection), NodeKind::CData(_)) => true,
            _ => false,
        };
        subject_ok
            && self.conditions.iter().all(|condition| match condition {
                Condition::Attr { name, op, value } => {
                    attribute_value(doc, node, name).is_some_and(|actual| op.test(actual, value))
                }
                Condition::FirstChild => siblings.prev_element(node).is_none(),
                Condition::LastChild => siblings.is_last(node),
            })
    }
}

impl Selector {
    /// The compound that must match the styled node itself.
    pub(crate) fn subject(&self) -> &Compound {
        self.compounds
            .last()
            .expect("a selector has at least one compound")
    }

    pub(crate) fn matches(&self, doc: &Document, siblings: &Siblings, node: NodeId) -> bool {
        self.matches_from(self.compounds.len() - 1, doc, siblings, node)
    }

    fn matches_from(
        &self,
        index: usize,
        doc: &Document,
        siblings: &Siblings,
        node: NodeId,
    ) -> bool {
        if !self.compounds[index].matches(doc, siblings, node) {
            return false;
        }
        if index == 0 {
            return true;
        }
        let left = index - 1;
        match self.combinators[left] {
            Combinator::Child => doc
                .parent(node)
                .is_some_and(|p| self.matches_from(left, doc, siblings, p)),
            Combinator::Descendant => {
                let mut ancestor = doc.parent(node);
                while let Some(a) = ancestor {
                    if self.matches_from(left, doc, siblings, a) {
                        return true;
                    }
                    ancestor = doc.parent(a);
                }
                false
            }
            Combinator::AdjacentSibling => siblings
                .prev_element(node)
                .is_some_and(|p| self.matches_from(left, doc, siblings, p)),
            Combinator::GeneralSibling => {
                let mut sibling = siblings.prev_element(node);
                while let Some(s) = sibling {
                    if self.matches_from(left, doc, siblings, s) {
                        return true;
                    }
                    sibling = siblings.prev_element(s);
                }
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::tokenize;

    #[test]
    fn ancestor_hashes_stop_at_sibling_combinators() {
        assert_eq!(
            one("a > b c").ancestor_hashes,
            vec![name_hash("b"), name_hash("a")]
        );
        assert_eq!(one("a + b c").ancestor_hashes, vec![name_hash("b")]);
        assert_eq!(one("* c").ancestor_hashes, Vec::<u32>::new());
    }

    fn parse(css: &str) -> Result<Vec<Selector>, String> {
        parse_selector_list(&tokenize(css).tokens)
    }

    fn one(css: &str) -> Selector {
        let mut list = parse(css).unwrap();
        assert_eq!(list.len(), 1);
        list.remove(0)
    }

    /// Returns the ids (as document-order element indices) of elements matching.
    fn matching(css: &str, xml: &str) -> Vec<String> {
        let doc = xylem_dom::parse(xml).unwrap();
        let siblings = Siblings::build(&doc);
        let selectors = parse(css).unwrap();
        doc.descendants(doc.root())
            .filter(|&n| {
                selectors
                    .iter()
                    .any(|s| s.pseudo.is_none() && s.matches(&doc, &siblings, n))
            })
            .map(|n| match doc.kind(n) {
                NodeKind::Element(e) => doc
                    .attr(n, "id")
                    .map_or_else(|| e.name.clone(), str::to_string),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn specificity_follows_css21() {
        assert_eq!(one("*").specificity, 0);
        assert_eq!(one("a").specificity, 1);
        assert_eq!(one("a b").specificity, 2);
        assert_eq!(one("a[x]").specificity, 0x0101);
        assert_eq!(one("a:first-child").specificity, 0x0101);
        assert_eq!(one("a:before").specificity, 2);
        assert_eq!(one("#x").specificity, 0x010000);
        assert!(one("a[x][y]").specificity > one("a[x]").specificity);
        assert!(one("[x]").specificity > one("a b c").specificity);
    }

    #[test]
    fn comma_groups_and_pseudo_elements() {
        let list = parse("title:before, note>title::after").unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].pseudo, Some(PseudoElement::Before));
        assert_eq!(list[1].pseudo, Some(PseudoElement::After));
        assert_eq!(list[1].combinators, vec![Combinator::Child]);
        assert_eq!(parse("a, b, ").unwrap().len(), 2);
    }

    #[test]
    fn rejects_unsupported_selectors() {
        assert!(parse("a:hover").is_err());
        assert!(parse("a:not(b)").is_err());
        assert!(parse("a >").is_err());
        assert!(parse("a:before b").is_err());
        assert!(parse("$bogus").is_err());
        assert!(parse("").is_err());
        assert!(parse(" , ").is_err());
    }

    #[test]
    fn type_descendant_child() {
        let xml = r#"<r><a id="a1"><b id="b1"><c id="c1"/></b></a><c id="c2"/></r>"#;
        assert_eq!(matching("c", xml), ["c1", "c2"]);
        assert_eq!(matching("a c", xml), ["c1"]);
        assert_eq!(matching("a>c", xml), Vec::<String>::new());
        assert_eq!(matching("a > b > c", xml), ["c1"]);
        assert_eq!(matching("r>*", xml), ["a1", "c2"]);
    }

    #[test]
    fn sibling_combinators_skip_text() {
        let xml = r#"<r><a id="a1"/> text <b id="b1"/><b id="b2"/><c id="c1"/></r>"#;
        assert_eq!(matching("a+b", xml), ["b1"]);
        assert_eq!(matching("b+b", xml), ["b2"]);
        assert_eq!(matching("a~b", xml), ["b1", "b2"]);
        assert_eq!(matching("a~c", xml), ["c1"]);
        assert_eq!(matching("b+c", xml), ["c1"]);
    }

    #[test]
    fn first_and_last_child() {
        let xml = r#"<r>x<a id="a1"/><a id="a2"/><a id="a3"/>y</r>"#;
        assert_eq!(matching("a:first-child", xml), ["a1"]);
        assert_eq!(matching("a:last-child", xml), ["a3"]);
        assert_eq!(matching("r:first-child", xml), ["r"]);
    }

    #[test]
    fn attribute_operators() {
        let xml = r#"<r><e id="1" k="alpha beta" l="en-US" m="foobar"/><e id="2"/></r>"#;
        assert_eq!(matching("e[k]", xml), ["1"]);
        assert_eq!(matching("e[k~=beta]", xml), ["1"]);
        assert_eq!(matching("e[k~=alp]", xml), Vec::<String>::new());
        assert_eq!(matching("e[l|=en]", xml), ["1"]);
        assert_eq!(matching("e[l|=e]", xml), Vec::<String>::new());
        assert_eq!(matching("e[m^=foo]", xml), ["1"]);
        assert_eq!(matching("e[m$=bar]", xml), ["1"]);
        assert_eq!(matching("e[m*=oba]", xml), ["1"]);
        assert_eq!(matching("e[m='foobar']", xml), ["1"]);
        assert_eq!(matching("e[m=\"foobar\"][l]", xml), ["1"]);
        assert_eq!(matching("e[m^='']", xml), Vec::<String>::new());
    }

    #[test]
    fn namespace_prefixes_are_ignored() {
        let xml = r#"<r xmlns:d="urn:d"><d:item id="i"/></r>"#;
        assert_eq!(matching("item", xml), ["i"]);
        assert_eq!(matching("d|item", xml), ["i"]);
        assert_eq!(matching("*|item", xml), ["i"]);
    }

    #[test]
    fn xmetal_pseudo_elements() {
        let xml = r#"<!--c--><root><ref><?Ref_Label x y?><?Other z?><![CDATA[d]]></ref></root>"#;
        let doc = xylem_dom::parse(xml).unwrap();
        let siblings = Siblings::build(&doc);
        let count = |css: &str| {
            let sels = parse(css).unwrap();
            doc.descendants(doc.root())
                .filter(|&n| sels.iter().any(|s| s.matches(&doc, &siblings, n)))
                .count()
        };
        assert_eq!(count("$DOCUMENT"), 1);
        assert_eq!(count("$document>*"), 1);
        assert_eq!(count("$COMMENT"), 1);
        assert_eq!(count("$procins"), 2);
        assert_eq!(count("ref $PROCINS[xm-pi-target=\"Ref_Label\"]"), 1);
        assert_eq!(count("$procins[xm-pi-data='z']"), 1);
        assert_eq!(count("ref>$MARKSEC"), 1);
        assert_eq!(count("other $procins"), 0);
    }
}
