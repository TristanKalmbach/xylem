//! Cascade and whole-document style computation.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

use xylem_dom::{Document, NodeId, NodeKind};

use crate::counter::{CounterScopes, format_counter};
use crate::decl::{ContentItem, ContentValue, Declaration, Working, parse_declarations};
use crate::selector::{PseudoElement, Siblings, Special, Subject, name_hash};
use crate::sheet::{Rule, Stylesheet};
use crate::token::tokenize;
use crate::types::{ComputedStyle, Display, Generated};

const OPEN_QUOTE: char = '\u{201c}';
const CLOSE_QUOTE: char = '\u{201d}';
const LIST_ITEM_COUNTER: &str = "list-item";
/// Marks a node without an assigned style slot.
const NO_STYLE: u32 = u32::MAX;
/// Style slot 0 is the initial style that the root inherits from.
const INITIAL_STYLE: u32 = 0;

/// FNV-1a: a fast hasher for the short, trusted keys used here (element
/// names, node ids), where SipHash's DoS resistance is not needed.
#[derive(Default)]
struct FnvHasher(u64);

impl Hasher for FnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut hash = if self.0 == 0 { OFFSET } else { self.0 };
        for &b in bytes {
            hash = (hash ^ u64::from(b)).wrapping_mul(PRIME);
        }
        self.0 = hash;
    }
}

type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FnvHasher>>;

/// A counting Bloom filter over the local names of the open element's
/// ancestors, so most descendant selectors are rejected without walking up.
struct AncestorFilter {
    counts: Box<[u32; Self::SLOTS]>,
}

impl AncestorFilter {
    const SLOTS: usize = 1024;

    fn new() -> Self {
        Self {
            counts: Box::new([0; Self::SLOTS]),
        }
    }

    fn slots(hash: u32) -> [usize; 2] {
        [
            hash as usize % Self::SLOTS,
            (hash >> 16) as usize % Self::SLOTS,
        ]
    }

    fn insert(&mut self, hash: u32) {
        for slot in Self::slots(hash) {
            self.counts[slot] += 1;
        }
    }

    fn remove(&mut self, hash: u32) {
        for slot in Self::slots(hash) {
            self.counts[slot] -= 1;
        }
    }

    fn may_contain(&self, hash: u32) -> bool {
        Self::slots(hash).iter().all(|&slot| self.counts[slot] > 0)
    }
}

// ------------------------------------------------------------------- index

/// Rules bucketed by the rightmost compound's subject, so that each node only
/// tests rules that could possibly match it.
#[derive(Default)]
struct RuleIndex {
    by_name: FastMap<String, Vec<u32>>,
    universal: Vec<u32>,
    /// Indexed by [`special_slot`].
    special: [Vec<u32>; 4],
}

fn special_slot(special: Special) -> usize {
    match special {
        Special::Document => 0,
        Special::Comment => 1,
        Special::ProcessingInstruction => 2,
        Special::MarkedSection => 3,
    }
}

impl RuleIndex {
    fn build(rules: &[Rule]) -> Self {
        let mut index = RuleIndex::default();
        for (i, rule) in rules.iter().enumerate() {
            let i = i as u32;
            match &rule.selector.subject().subject {
                Subject::Name(name) => index.by_name.entry(name.clone()).or_default().push(i),
                Subject::Any => index.universal.push(i),
                Subject::Special(s) => index.special[special_slot(*s)].push(i),
            }
        }
        index
    }
}

// ------------------------------------------------------------------ engine

/// Computes styles for documents from a style sheet plus per-node inline
/// overrides.
pub struct StyleEngine {
    sheet: Stylesheet,
    index: RuleIndex,
    inline: FastMap<NodeId, Vec<Declaration>>,
}

impl StyleEngine {
    pub fn new(sheet: Stylesheet) -> Self {
        let index = RuleIndex::build(&sheet.rules);
        Self {
            sheet,
            index,
            inline: FastMap::default(),
        }
    }

    pub fn stylesheet(&self) -> &Stylesheet {
        &self.sheet
    }

    /// Replaces the style sheet, keeping inline overrides.
    pub fn set_stylesheet(&mut self, sheet: Stylesheet) {
        self.index = RuleIndex::build(&sheet.rules);
        self.sheet = sheet;
    }

    /// Attaches declaration text such as `"background-color:#ebffeb; color:#008000;"`
    /// to `node`, emulating XMAX `Range.ContainerStyle`. It outranks every
    /// style sheet rule (except that style sheet `!important` beats a normal
    /// inline declaration, as in CSS). Replaces any earlier inline style.
    pub fn set_inline_style(&mut self, node: NodeId, decls: &str) {
        let lexed = tokenize(decls);
        let declarations = parse_declarations(&lexed.tokens, &lexed.lines, &mut |_, _| {});
        if declarations.is_empty() {
            self.inline.remove(&node);
        } else {
            self.inline.insert(node, declarations);
        }
    }

    pub fn clear_inline_style(&mut self, node: NodeId) {
        self.inline.remove(&node);
    }

    /// Computes styles for every node reachable from the document root.
    pub fn compute(&self, doc: &Document) -> StyleMap {
        let mut run = Run {
            engine: self,
            doc,
            siblings: Siblings::build(doc),
            map: StyleMap::new(doc.node_count()),
            counters: CounterScopes::default(),
            frames: Vec::new(),
            matches: Default::default(),
            ancestors: AncestorFilter::new(),
        };
        run.enter(doc.root());
        while let Some(frame) = run.frames.last_mut() {
            match doc.children(frame.node).get(frame.next_child) {
                Some(&child) => {
                    frame.next_child += 1;
                    run.visit(child);
                }
                None => run.leave(),
            }
        }
        run.map
    }
}

// --------------------------------------------------------------- StyleMap

/// The result of [`StyleEngine::compute`].
#[derive(Debug)]
pub struct StyleMap {
    styles: Vec<ComputedStyle>,
    slot: Vec<u32>,
    before: FastMap<NodeId, Generated>,
    after: FastMap<NodeId, Generated>,
    list_index: FastMap<NodeId, i32>,
}

impl StyleMap {
    fn new(node_count: usize) -> Self {
        Self {
            styles: vec![ComputedStyle::default_root()],
            slot: vec![NO_STYLE; node_count],
            before: FastMap::default(),
            after: FastMap::default(),
            list_index: FastMap::default(),
        }
    }

    /// The style of `id`. Text and other unstyled nodes inherit from their
    /// parent (inline, no box properties); nodes that were not part of the
    /// computed document get the initial style.
    pub fn get(&self, id: NodeId) -> &ComputedStyle {
        match self.slot.get(id.index()) {
            Some(&slot) if slot != NO_STYLE => &self.styles[slot as usize],
            _ => &self.styles[INITIAL_STYLE as usize],
        }
    }

    /// The resolved `::before` box of `id`, if its `content` generates one.
    pub fn before(&self, id: NodeId) -> Option<&Generated> {
        self.before.get(&id)
    }

    /// The resolved `::after` box of `id`, if its `content` generates one.
    pub fn after(&self, id: NodeId) -> Option<&Generated> {
        self.after.get(&id)
    }

    /// For `display: list-item` elements: the value of the implicit
    /// `list-item` counter, to be formatted with [`format_counter`] and the
    /// element's `list_style`.
    pub fn list_index(&self, id: NodeId) -> Option<i32> {
        self.list_index.get(&id).copied()
    }

    fn push_style(&mut self, style: ComputedStyle) -> u32 {
        self.styles.push(style);
        (self.styles.len() - 1) as u32
    }
}

// -------------------------------------------------------------------- walk

/// An open node during the document-order walk.
struct Frame {
    node: NodeId,
    next_child: usize,
    /// Style slot of this node.
    style: u32,
    /// Style slot shared by this node's unstyled children.
    anonymous: Option<u32>,
    /// Counter names whose instances this node's scope owns.
    created: Vec<String>,
    /// Inside a `display: none` subtree: no boxes, no counters.
    suppressed: bool,
    /// Pre-cascaded `::after`, evaluated when the node closes.
    after: Option<Working>,
    /// Hash this element contributed to the ancestor filter.
    name_hash: Option<u32>,
}

#[derive(Default)]
struct Matches {
    element: Vec<(u32, u32)>,
    before: Vec<(u32, u32)>,
    after: Vec<(u32, u32)>,
}

struct Run<'a> {
    engine: &'a StyleEngine,
    doc: &'a Document,
    siblings: Siblings,
    map: StyleMap,
    counters: CounterScopes,
    frames: Vec<Frame>,
    matches: Matches,
    ancestors: AncestorFilter,
}

impl Run<'_> {
    fn visit(&mut self, node: NodeId) {
        match self.doc.kind(node) {
            NodeKind::Text(_) | NodeKind::EntityRef(_) | NodeKind::Doctype(_) => {
                let slot = self.anonymous_slot();
                self.map.slot[node.index()] = slot;
            }
            _ => self.enter(node),
        }
    }

    /// The shared "inherit everything, own nothing" style of the open node's
    /// children.
    fn anonymous_slot(&mut self) -> u32 {
        let frame = self
            .frames
            .last_mut()
            .expect("anonymous style requested outside any node");
        if let Some(slot) = frame.anonymous {
            return slot;
        }
        let style = ComputedStyle::inherit_from(&self.map.styles[frame.style as usize]);
        let slot = self.map.push_style(style);
        frame.anonymous = Some(slot);
        slot
    }

    fn collect_matches(&mut self, node: NodeId) {
        let index = &self.engine.index;
        let rules = &self.engine.sheet.rules;
        let none: &[u32] = &[];
        let candidates: [&[u32]; 2] = match self.doc.kind(node) {
            NodeKind::Element(e) => [
                index
                    .by_name
                    .get(e.local_name())
                    .map_or(none, Vec::as_slice),
                &index.universal,
            ],
            NodeKind::Document => [&index.special[special_slot(Special::Document)], none],
            NodeKind::Comment(_) => [&index.special[special_slot(Special::Comment)], none],
            NodeKind::Pi { .. } => [
                &index.special[special_slot(Special::ProcessingInstruction)],
                none,
            ],
            NodeKind::CData(_) => [&index.special[special_slot(Special::MarkedSection)], none],
            _ => [none, none],
        };
        let m = &mut self.matches;
        m.element.clear();
        m.before.clear();
        m.after.clear();
        for list in candidates {
            for &rule_index in list {
                let selector = &rules[rule_index as usize].selector;
                if selector
                    .ancestor_hashes
                    .iter()
                    .all(|&h| self.ancestors.may_contain(h))
                    && selector.matches(self.doc, &self.siblings, node)
                {
                    let bucket = match selector.pseudo {
                        None => &mut m.element,
                        Some(PseudoElement::Before) => &mut m.before,
                        Some(PseudoElement::After) => &mut m.after,
                    };
                    bucket.push((selector.specificity, rule_index));
                }
            }
        }
    }

    fn enter(&mut self, node: NodeId) {
        let is_root = self.frames.is_empty();
        let parent_slot = self.frames.last().map_or(INITIAL_STYLE, |f| f.style);
        let parent_suppressed = self.frames.last().is_some_and(|f| f.suppressed);
        self.collect_matches(node);
        let inline = self.engine.inline.get(&node).map(Vec::as_slice);
        let rules = &self.engine.sheet.rules;

        let styled = is_root || !self.matches.element.is_empty() || inline.is_some();
        let (slot, reset, increment) = if styled {
            let display = if is_root {
                Display::Block
            } else {
                Display::Inline
            };
            let w = cascade(
                rules,
                &mut self.matches.element,
                inline,
                &self.map.styles[parent_slot as usize],
                display,
            );
            let Working {
                style,
                counter_reset,
                counter_increment,
                ..
            } = w;
            (self.map.push_style(style), counter_reset, counter_increment)
        } else {
            (self.anonymous_slot(), Vec::new(), Vec::new())
        };
        self.map.slot[node.index()] = slot;
        let display = self.map.styles[slot as usize].display;
        let suppressed = parent_suppressed || display == Display::None;

        let mut frame = Some(Frame {
            node,
            next_child: 0,
            style: slot,
            // A node that matched nothing is its own inherited style.
            anonymous: (!styled).then_some(slot),
            created: Vec::new(),
            suppressed,
            after: None,
            name_hash: self.doc.element(node).map(|e| name_hash(e.local_name())),
        });
        let list_item = display == Display::ListItem;
        // An element's own counters are scoped to its parent; the root has no
        // parent, so it scopes its own.
        if is_root {
            self.frames.extend(frame.take());
        }
        if !suppressed {
            self.apply_counter_ops(&reset, &increment, list_item, node);
        }
        if let Some(hash) = frame.as_ref().and_then(|f| f.name_hash) {
            self.ancestors.insert(hash);
        }
        self.frames.extend(frame);
        if suppressed {
            return;
        }

        if !self.matches.before.is_empty() {
            let w = cascade(
                rules,
                &mut self.matches.before,
                None,
                &self.map.styles[slot as usize],
                Display::Inline,
            );
            if let Some(generated) = self.generate(node, w) {
                self.map.before.insert(node, generated);
            }
        }
        if !self.matches.after.is_empty() {
            let w = cascade(
                rules,
                &mut self.matches.after,
                None,
                &self.map.styles[slot as usize],
                Display::Inline,
            );
            if let Some(f) = self.frames.last_mut() {
                f.after = Some(w);
            }
        }
    }

    fn leave(&mut self) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let node = frame.node;
        if let Some(w) = frame.after.take()
            && let Some(generated) = self.generate(node, w)
        {
            self.map.after.insert(node, generated);
        }
        if let Some(frame) = self.frames.pop() {
            for name in frame.created.iter().rev() {
                self.counters.close_scope(name);
            }
            if let Some(hash) = frame.name_hash {
                self.ancestors.remove(hash);
            }
        }
    }

    /// Applies `counter-reset`/`counter-increment` (resets first) to the
    /// scope owned by the innermost open frame, then records the implicit
    /// `list-item` counter.
    fn apply_counter_ops(
        &mut self,
        reset: &[(String, i32)],
        increment: &[(String, i32)],
        list_item: bool,
        node: NodeId,
    ) {
        let implicit = (list_item && !increment.iter().any(|(n, _)| n == LIST_ITEM_COUNTER))
            .then(|| (LIST_ITEM_COUNTER.to_string(), 1));
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let owner = frame.node;
        for (name, value) in reset {
            if self.counters.reset(name, *value, owner) {
                frame.created.push(name.clone());
            }
        }
        for (name, by) in increment.iter().chain(implicit.as_ref()) {
            if self.counters.increment(name, *by, owner) {
                frame.created.push(name.clone());
            }
        }
        if list_item && let Some(value) = self.counters.value(LIST_ITEM_COUNTER) {
            self.map.list_index.insert(node, value);
        }
    }

    /// Turns a cascaded pseudo-element into a [`Generated`] box, running its
    /// counter operations. The innermost open frame is the originating node.
    fn generate(&mut self, node: NodeId, w: Working) -> Option<Generated> {
        let Some(ContentValue::Items(items)) = &w.content else {
            return None;
        };
        if w.style.display == Display::None {
            return None;
        }
        self.apply_counter_ops(&w.counter_reset, &w.counter_increment, false, node);
        let mut text = String::new();
        let mut image = None;
        for item in items {
            match item {
                ContentItem::Str(s) => text.push_str(s),
                ContentItem::Attr(name) => {
                    text.push_str(self.doc.attr(node, name).unwrap_or_default())
                }
                ContentItem::Counter { name, style } => {
                    let value = self.counter_value(name);
                    text.push_str(&format_counter(value, *style));
                }
                ContentItem::Counters {
                    name,
                    separator,
                    style,
                } => {
                    self.counter_value(name);
                    let parts: Vec<String> = self
                        .counters
                        .values(name)
                        .map(|v| format_counter(v, *style))
                        .collect();
                    text.push_str(&parts.join(separator));
                }
                ContentItem::OpenQuote => text.push(OPEN_QUOTE),
                ContentItem::CloseQuote => text.push(CLOSE_QUOTE),
                ContentItem::NoOpenQuote | ContentItem::NoCloseQuote => {}
                ContentItem::Url(url) => {
                    image.get_or_insert_with(|| url.clone());
                }
            }
        }
        Some(Generated {
            text,
            image,
            style: w.style,
        })
    }

    /// The innermost value of `name`, instantiating it on the open node if no
    /// instance is in scope.
    fn counter_value(&mut self, name: &str) -> i32 {
        if let Some(frame) = self.frames.last_mut()
            && self.counters.ensure(name, frame.node)
        {
            frame.created.push(name.to_string());
        }
        self.counters.value(name).unwrap_or(0)
    }
}

/// Runs the cascade for one node (or pseudo-element) over its matched rules.
///
/// Declarations are applied in ascending priority: normal rules by
/// (specificity, source order), normal inline, then the same for `!important`.
fn cascade(
    rules: &[Rule],
    matched: &mut [(u32, u32)],
    inline: Option<&[Declaration]>,
    parent: &ComputedStyle,
    display: Display,
) -> Working {
    matched.sort_unstable();
    let mut ordered: Vec<&Declaration> = Vec::new();
    for important in [false, true] {
        for &(_, rule_index) in matched.iter() {
            ordered.extend(
                rules[rule_index as usize]
                    .declarations
                    .iter()
                    .filter(|d| d.important == important),
            );
        }
        ordered.extend(
            inline
                .into_iter()
                .flatten()
                .filter(|d| d.important == important),
        );
    }
    let mut w = Working::new(parent, display);
    // Font size first: every em-based length depends on the final value.
    for d in &ordered {
        d.prop.apply_font_size(&mut w, parent);
    }
    for d in &ordered {
        d.prop.apply(&mut w);
    }
    w.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{BorderStyle, ListStyle, TextAlign, VerticalAlign};

    fn engine(css: &str) -> StyleEngine {
        let sheet = Stylesheet::parse(css);
        assert!(
            sheet.warnings().is_empty(),
            "css warnings: {:?}",
            sheet.warnings()
        );
        StyleEngine::new(sheet)
    }

    fn doc(xml: &str) -> Document {
        xylem_dom::parse(xml).unwrap()
    }

    /// First element named `name` in document order.
    fn find(doc: &Document, name: &str) -> NodeId {
        doc.descendants(doc.root())
            .find(|&n| doc.name(n) == Some(name))
            .unwrap_or_else(|| panic!("no <{name}>"))
    }

    fn find_all(doc: &Document, name: &str) -> Vec<NodeId> {
        doc.descendants(doc.root())
            .filter(|&n| doc.name(n) == Some(name))
            .collect()
    }

    #[test]
    fn defaults_without_rules() {
        let d = doc("<r>hi<a/></r>");
        let map = StyleEngine::new(Stylesheet::new()).compute(&d);
        assert_eq!(map.get(d.root()).display, Display::Block);
        let r = find(&d, "r");
        let s = map.get(r);
        assert_eq!(s.display, Display::Inline);
        assert_eq!(s.font_size, 16.0);
        assert_eq!(s.color, None);
        assert_eq!(map.get(d.children(r)[0]).display, Display::Inline);
        assert_eq!(*s, ComputedStyle::default_root());
    }

    #[test]
    fn specificity_order_and_important() {
        let e = engine(
            "p { color: #010101 } p { color: #020202 }\n\
             r p { color: #030303 }\n\
             p[x] { color: #040404 }\n\
             * { color: #ff0000 }\n\
             q { color: #050505 !important } r q { color: #060606 }",
        );
        let d = doc(r#"<r><p x="1"/><p/><q/></r>"#);
        let map = e.compute(&d);
        let ps = find_all(&d, "p");
        // Attribute selector (0,1,1) beats descendant (0,0,2).
        assert_eq!(map.get(ps[0]).color, Some([4, 4, 4, 255]));
        assert_eq!(map.get(ps[1]).color, Some([3, 3, 3, 255]));
        assert_eq!(map.get(find(&d, "q")).color, Some([5, 5, 5, 255]));
        assert_eq!(map.get(find(&d, "r")).color, Some([255, 0, 0, 255]));
    }

    #[test]
    fn later_rule_wins_equal_specificity() {
        let d = doc("<r><p/></r>");
        let map = engine("p { color: red } p { color: blue }").compute(&d);
        assert_eq!(map.get(find(&d, "p")).color, Some([0, 0, 255, 255]));
    }

    #[test]
    fn inheritance_and_non_inheritance() {
        let e = engine(
            "r { display: block; color: green; font-weight: bold; margin: 10px; background-color: red; \
               border: 1px solid black; text-align: center; vertical-align: super; font-family: 'Courier New', monospace }",
        );
        let d = doc("<r><c>text</c></r>");
        let map = e.compute(&d);
        let c = map.get(find(&d, "c"));
        assert_eq!(c.display, Display::Inline);
        assert_eq!(c.color, Some([0, 128, 0, 255]));
        assert!(c.bold && c.monospace);
        assert_eq!(c.text_align, TextAlign::Center);
        assert_eq!(c.font_family.as_deref(), Some("Courier New, monospace"));
        assert_eq!(c.margin.top, 0.0);
        assert_eq!(c.background, None);
        assert_eq!(c.border.top.style, BorderStyle::None);
        assert_eq!(c.vertical_align, VerticalAlign::Baseline);
        let text = map.get(d.children(find(&d, "c"))[0]);
        assert!(text.bold && text.color.is_some());
        assert_eq!(text.display, Display::Inline);
        let r = map.get(find(&d, "r"));
        assert_eq!(r.margin.left, 10.0);
        assert_eq!(r.border.left.width, 1.0);
        assert_eq!(r.background, Some([255, 0, 0, 255]));
    }

    #[test]
    fn font_sizes_resolve_against_the_parent() {
        let e = engine(
            "a { font-size: 20px } b { font-size: 1.5em } c { font-size: 50% } d { font-size: 12pt } \
             e { font-size: larger } f { font-size: x-large } g { font-size: smaller }",
        );
        let d = doc("<a><b><c/></b><d/><e/><f><g/></f></a>");
        let map = e.compute(&d);
        let size = |n: &str| map.get(find(&d, n)).font_size;
        assert_eq!(size("a"), 20.0);
        assert_eq!(size("b"), 30.0);
        assert_eq!(size("c"), 15.0);
        assert_eq!(size("d"), 16.0);
        assert_eq!(size("e"), 24.0);
        assert_eq!(size("f"), 24.0);
        assert_eq!(size("g"), 24.0 / 1.2);
    }

    #[test]
    fn em_lengths_use_the_elements_own_final_font_size() {
        // The margin rule comes first, but the font-size applies to it.
        let e = engine(
            "p { margin-left: 2em; line-height: 1.5; text-indent: 1em } p { font-size: 10px } c { font-size: 20px }",
        );
        let d = doc("<p><c/></p>");
        let map = e.compute(&d);
        let p = map.get(find(&d, "p"));
        assert_eq!(p.margin.left, 20.0);
        assert_eq!(p.line_height, Some(15.0));
        assert_eq!(p.text_indent, 10.0);
        // A unitless line-height inherits as a factor, text-indent as px.
        let c = map.get(find(&d, "c"));
        assert_eq!(c.line_height, Some(30.0));
        assert_eq!(c.text_indent, 10.0);
    }

    #[test]
    fn text_decoration_font_style_and_weight() {
        let e = engine(
            "u { text-decoration: underline } s { text-decoration: line-through } n { text-decoration: none } \
             i { font-style: italic } b { font-weight: 700 } l { font-weight: normal }",
        );
        let d = doc("<r><u><n/><s/></u><i/><b><l/></b></r>");
        let map = e.compute(&d);
        assert!(map.get(find(&d, "u")).underline);
        assert!(!map.get(find(&d, "n")).underline);
        let s = map.get(find(&d, "s"));
        assert!(s.line_through && !s.underline);
        assert!(map.get(find(&d, "i")).italic);
        assert!(map.get(find(&d, "b")).bold);
        assert!(!map.get(find(&d, "l")).bold);
    }

    #[test]
    fn border_none_zeroes_width_and_style_applies_in_order() {
        let e = engine(
            "a { border: 4px solid red; border-top: none } b { border-width: 2px } c { border-bottom: 3px dashed }",
        );
        let d = doc("<r><a/><b/><c/></r>");
        let map = e.compute(&d);
        let a = map.get(find(&d, "a"));
        assert_eq!(a.border.top.width, 0.0);
        assert_eq!(a.border.left.width, 4.0);
        assert_eq!(a.border.left.color, Some([255, 0, 0, 255]));
        // Width without a style draws nothing.
        assert_eq!(map.get(find(&d, "b")).border.top.width, 0.0);
        let c = map.get(find(&d, "c"));
        assert_eq!(c.border.bottom.width, 3.0);
        assert_eq!(c.border.bottom.style, BorderStyle::Dashed);
        assert_eq!(c.border.bottom.color, None);
    }

    #[test]
    fn inline_styles_override_everything() {
        let d = doc("<r><p/></r>");
        let mut e = engine("p { color: red; display: block } p[id] { color: blue }");
        let p = find(&d, "p");
        e.set_inline_style(p, "background-color:#ebffeb; color:#008000;");
        let map = e.compute(&d);
        assert_eq!(map.get(p).color, Some([0, 128, 0, 255]));
        assert_eq!(map.get(p).background, Some([0xeb, 0xff, 0xeb, 255]));
        assert_eq!(map.get(p).display, Display::Block);
        e.clear_inline_style(p);
        let map = e.compute(&d);
        assert_eq!(map.get(p).color, Some([255, 0, 0, 255]));
        assert_eq!(map.get(p).background, None);
    }

    #[test]
    fn xmetal_pseudo_elements_get_styles() {
        let e = engine(
            "$DOCUMENT { background-color: #eeeeee } $COMMENT { color: green; display: block } \
             externalpubref $procins[xm-pi-target=\"Ref_Label\"] { display: none } $MARKSEC { color: red } \
             $DOCUMENT>* { display: block }",
        );
        let d = doc(
            "<!--c--><root><externalpubref><?Ref_Label z?><?Other y?></externalpubref><![CDATA[x]]></root>",
        );
        let map = e.compute(&d);
        assert_eq!(map.get(d.root()).background, Some([238, 238, 238, 255]));
        let comment = d
            .children(d.root())
            .iter()
            .copied()
            .find(|&n| matches!(d.kind(n), NodeKind::Comment(_)))
            .unwrap();
        assert_eq!(map.get(comment).color, Some([0, 128, 0, 255]));
        assert_eq!(map.get(comment).display, Display::Block);
        assert_eq!(map.get(find(&d, "root")).display, Display::Block);
        let pis: Vec<_> = d
            .descendants(d.root())
            .filter(|&n| matches!(d.kind(n), NodeKind::Pi { .. }))
            .collect();
        assert_eq!(map.get(pis[0]).display, Display::None);
        assert_eq!(map.get(pis[1]).display, Display::Inline);
        let cdata = d
            .descendants(d.root())
            .find(|&n| matches!(d.kind(n), NodeKind::CData(_)))
            .unwrap();
        assert_eq!(map.get(cdata).color, Some([255, 0, 0, 255]));
    }

    // ---------------------------------------------------------- generated

    #[test]
    fn before_after_text_attr_and_escapes() {
        let e = engine(
            "t:before { content: \"\\201C\" attr(k) \"\\A\" open-quote close-quote; font-weight: bold; display: block } \
             t:after { content: ' [' attr(missing) ']' } u:before { content: none } v::before { content: \"\" }",
        );
        let d = doc(r#"<r><t k="v">x</t><u/><v/></r>"#);
        let map = e.compute(&d);
        let t = find(&d, "t");
        let before = map.before(t).unwrap();
        assert_eq!(before.text, "\u{201c}v\n\u{201c}\u{201d}");
        assert!(before.style.bold);
        assert_eq!(before.style.display, Display::Block);
        assert_eq!(map.after(t).unwrap().text, " []");
        assert!(map.before(find(&d, "u")).is_none());
        assert_eq!(map.before(find(&d, "v")).unwrap().text, "");
        assert!(map.after(find(&d, "v")).is_none());
    }

    #[test]
    fn generated_style_inherits_from_the_element() {
        let e =
            engine("t { color: red; font-size: 20px } t:before { content: 'x'; font-size: 0.5em }");
        let d = doc("<t/>");
        let map = e.compute(&d);
        let g = map.before(find(&d, "t")).unwrap();
        assert_eq!(g.style.color, Some([255, 0, 0, 255]));
        assert_eq!(g.style.font_size, 10.0);
        assert_eq!(g.style.display, Display::Inline);
    }

    #[test]
    fn url_content_is_an_image() {
        let e = engine("t:before { content: url(check.gif) \"ok\" }");
        let d = doc("<t/>");
        let g = engine_before(&e, &d, "t");
        assert_eq!(g.image.as_deref(), Some("check.gif"));
        assert_eq!(g.text, "ok");
    }

    fn engine_before(e: &StyleEngine, d: &Document, name: &str) -> Generated {
        e.compute(d).before(find(d, name)).cloned().unwrap()
    }

    fn texts(map: &StyleMap, d: &Document, name: &str) -> Vec<String> {
        find_all(d, name)
            .into_iter()
            .map(|n| map.before(n).map(|g| g.text.clone()).unwrap_or_default())
            .collect()
    }

    #[test]
    fn counters_increment_in_document_order() {
        let e = engine(
            "r { counter-reset: n } i { counter-increment: n } i:before { content: counter(n) }",
        );
        let d = doc("<r><i/><i/><x><i/></x><i/></r>");
        assert_eq!(texts(&e.compute(&d), &d, "i"), ["1", "2", "3", "4"]);
    }

    #[test]
    fn counter_without_reset_is_instantiated_implicitly() {
        let e = engine("i { counter-increment: n } i:before { content: counter(n, upper-roman) }");
        let d = doc("<r><i/><i/><i/></r>");
        assert_eq!(texts(&e.compute(&d), &d, "i"), ["I", "II", "III"]);
    }

    #[test]
    fn counter_reset_scopes_to_following_siblings_and_descendants() {
        // Each <sec> restarts <p> numbering inside it; headings count across.
        let e = engine(
            "doc { counter-reset: h } h { counter-increment: h } h:before { content: counter(h) '.' } \
             sec { counter-reset: p } p { counter-increment: p } p:before { content: counter(p) }",
        );
        let d = doc("<doc><h/><sec><p/><p/><h/></sec><sec><p/><p/><p/></sec><h/></doc>");
        let map = e.compute(&d);
        assert_eq!(texts(&map, &d, "h"), ["1.", "2.", "3."]);
        assert_eq!(texts(&map, &d, "p"), ["1", "2", "1", "2", "3"]);
    }

    #[test]
    fn sibling_reset_replaces_instead_of_nesting() {
        let e = engine(
            "h { counter-reset: s } p { counter-increment: s } p:before { content: counters(s, '.') }",
        );
        let d = doc("<r><h/><p/><p/><h/><p/></r>");
        assert_eq!(texts(&e.compute(&d), &d, "p"), ["1", "2", "1"]);
    }

    #[test]
    fn nested_counters_join_with_separator() {
        let e = engine(
            "ol { counter-reset: item } li { display: block; counter-increment: item } \
             li:before { content: counters(item, '.') ' ' }",
        );
        let d = doc("<ol><li/><li><ol><li/><li><ol><li/></ol></li></ol></li><li/></ol>");
        assert_eq!(
            texts(&e.compute(&d), &d, "li"),
            ["1 ", "2 ", "2.1 ", "2.2 ", "2.2.1 ", "3 "]
        );
    }

    #[test]
    fn before_and_after_participate_in_counter_order() {
        let e = engine(
            "r { counter-reset: n } r:before { content: counter(n); counter-increment: n } \
             i { counter-increment: n } i:before { content: counter(n) } r:after { content: counter(n) }",
        );
        let d = doc("<r><i/><i/></r>");
        let map = e.compute(&d);
        // Counter properties of a pseudo-element apply before its content.
        assert_eq!(map.before(find(&d, "r")).unwrap().text, "1");
        assert_eq!(texts(&map, &d, "i"), ["2", "3"]);
        assert_eq!(map.after(find(&d, "r")).unwrap().text, "3");
    }

    #[test]
    fn reset_inside_before_scopes_to_the_elements_content() {
        let e = engine(
            "sec:before { content: ''; counter-reset: k } p { counter-increment: k } p:before { content: counter(k) }",
        );
        let d = doc("<r><sec><p/><p/></sec><sec><p/></sec></r>");
        assert_eq!(texts(&e.compute(&d), &d, "p"), ["1", "2", "1"]);
    }

    #[test]
    fn counter_style_arguments() {
        let e = engine(
            "i { counter-increment: n 4 } i:before { content: counter(n, lower-alpha) counter(n, decimal-leading-zero) counter(n, none) 'x' }",
        );
        let d = doc("<r><i/></r>");
        assert_eq!(texts(&e.compute(&d), &d, "i"), ["d04x"]);
    }

    #[test]
    fn list_items_get_an_implicit_counter() {
        let e = engine(
            "ul { display: block; list-style-type: upper-roman } li { display: list-item } \
             li:before { content: counter(list-item) }",
        );
        let d = doc("<r><ul><li/><li/></ul><ul><li/></ul></r>");
        let map = e.compute(&d);
        let lis = find_all(&d, "li");
        assert_eq!(texts(&map, &d, "li"), ["1", "2", "1"]);
        assert_eq!(map.list_index(lis[1]), Some(2));
        assert_eq!(map.list_index(lis[2]), Some(1));
        assert_eq!(map.get(lis[0]).list_style, ListStyle::UpperRoman);
        assert_eq!(map.list_index(find(&d, "ul")), None);
    }

    #[test]
    fn explicit_reset_restarts_list_items() {
        let e = engine("ul { counter-reset: list-item 4 } li { display: list-item }");
        let d = doc("<r><ul><li/><li/></ul></r>");
        let map = e.compute(&d);
        let lis = find_all(&d, "li");
        assert_eq!(map.list_index(lis[0]), Some(5));
        assert_eq!(map.list_index(lis[1]), Some(6));
    }

    #[test]
    fn display_none_suppresses_counters_and_generated_content() {
        let e = engine(
            "h { display: hidden } i { counter-increment: n } i:before { content: counter(n) } h:before { content: 'x' }",
        );
        let d = doc("<r><i/><h><i/><i/></h><i/></r>");
        let map = e.compute(&d);
        let texts = texts(&map, &d, "i");
        // Items inside the hidden subtree neither count nor generate content.
        assert_eq!(texts, ["1", "", "", "2"]);
        assert!(map.before(find(&d, "h")).is_none());
    }

    #[test]
    fn namespace_prefixed_elements_match_on_local_name() {
        let e = engine("item { display: block }");
        let d = doc(r#"<r xmlns:d="urn:d"><d:item/></r>"#);
        let map = e.compute(&d);
        let item = d
            .descendants(d.root())
            .find(|&n| d.name(n) == Some("d:item"))
            .unwrap();
        assert_eq!(map.get(item).display, Display::Block);
    }

    #[test]
    fn unknown_nodes_get_the_initial_style() {
        let e = engine("a { color: red }");
        let mut d = doc("<a/>");
        let map = e.compute(&d);
        let detached = d.create_element("later");
        assert_eq!(*map.get(detached), ComputedStyle::default_root());
    }

    #[test]
    fn large_documents_compute_without_recursion() {
        // 5000-deep nesting must not overflow the stack.
        let depth = 5000;
        let xml = format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let d = doc(&xml);
        let e = engine("a { color: red } a a { font-weight: bold }");
        let map = e.compute(&d);
        let last = d.descendants(d.root()).last().unwrap();
        assert!(map.get(last).bold);
    }
}
