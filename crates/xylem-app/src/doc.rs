//! Per-document state: the editor, its schema, doctype package, styles and
//! cached derived data (layout, validation).

use crate::view::canvas::Canvas;
use crate::view::layout::ViewMode;
use crate::view::style::{CssStyles, DefaultStyles, StyleLookup, WhiteSpace};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use xylem_customize::DoctypePackage;
use xylem_dom::{Document, NodeId};
use xylem_edit::{Editor, Rules};
use xylem_schema::{ContentType, Grammar};

#[derive(Copy, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum View {
    TagsOn,
    Normal,
    Source,
}

impl View {
    pub fn layout_mode(self) -> ViewMode {
        match self {
            View::Normal => ViewMode::Normal,
            _ => ViewMode::TagsOn,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub node: Option<NodeId>,
    pub severity: Severity,
    pub message: String,
    pub source: &'static str,
}

pub struct SourceState {
    pub text: String,
    pub error: Option<(String, usize)>,
    pub revision: u64,
}

pub struct DocState {
    #[allow(dead_code)]
    pub id: u64,
    pub path: Option<PathBuf>,
    pub editor: Editor,
    pub view: View,
    pub package: Arc<DoctypePackage>,
    pub grammar: Option<Arc<Grammar>>,
    pub grammar_status: String,
    pub style_engine: Option<std::sync::Arc<xylem_style::StyleEngine>>,
    pub style_warnings: Vec<String>,
    styles: Option<Rc<dyn StyleLookup>>,
    styles_key: (u64, u64),
    pub style_revision: u64,
    pub canvas: Canvas,
    pub source: Option<SourceState>,
    pub issues: Vec<Issue>,
    pub validated_revision: Option<u64>,
    pub s1000d: Option<xylem_s1000d::S1000dInfo>,
    pub read_only: bool,
    pub inline_styles: HashMap<NodeId, String>,
    pub image_cache: HashMap<String, Option<egui::TextureHandle>>,
    pub image_sizes: HashMap<String, Option<egui::Vec2>>,
    rules_memo: (u64, Rc<std::cell::RefCell<HashMap<NodeId, bool>>>),
    pub spell_cache: crate::spell::SpellCache,
    images: (u64, Rc<HashMap<NodeId, String>>),
    words: (u64, usize),
    /// Exact CSS styles for the last structure they were computed for.
    exact_css: Option<Rc<CssStyles>>,
    /// Background restyle in progress: (style key it is for, result channel).
    style_job: Option<((u64, u64), std::sync::mpsc::Receiver<CssStyles>)>,
    /// Asks the UI to repaint when background work finishes.
    pub repaint: Option<egui::Context>,
    /// Autosave copy this document was recovered from. The document counts as
    /// unsaved, and the copy is kept, until the user saves or discards it.
    pub recovered_from: Option<std::path::PathBuf>,
}

impl DocState {
    pub fn new(
        id: u64,
        doc: Document,
        path: Option<PathBuf>,
        package: Arc<DoctypePackage>,
    ) -> Self {
        let s1000d = xylem_s1000d::detect(&doc);
        Self {
            id,
            path,
            editor: Editor::new(doc),
            view: View::TagsOn,
            package,
            grammar: None,
            grammar_status: "No schema".into(),
            style_engine: None,
            style_warnings: Vec::new(),
            styles: None,
            styles_key: (u64::MAX, 0),
            style_revision: 1,
            canvas: Canvas::default(),
            source: None,
            issues: Vec::new(),
            validated_revision: None,
            s1000d,
            read_only: false,
            inline_styles: HashMap::new(),
            image_cache: HashMap::new(),
            image_sizes: HashMap::new(),
            rules_memo: (u64::MAX, Default::default()),
            spell_cache: Default::default(),
            images: (u64::MAX, Default::default()),
            words: (u64::MAX, 0),
            exact_css: None,
            style_job: None,
            repaint: None,
            recovered_from: None,
        }
    }

    /// Number of words in the document's text (cached per revision).
    pub fn word_count(&mut self) -> usize {
        let rev = self.editor.doc.revision();
        if self.words.0 != rev {
            let doc = &self.editor.doc;
            let n = doc
                .descendants(doc.root())
                .filter_map(|n| doc.text(n))
                .map(|t| {
                    t.split_whitespace()
                        .filter(|w| w.chars().any(char::is_alphanumeric))
                        .count()
                })
                .sum();
            self.words = (rev, n);
        }
        self.words.1
    }

    /// Removes the autosave copy this document was recovered from.
    pub fn forget_recovery(&mut self) {
        if let Some(f) = self.recovered_from.take() {
            let _ = std::fs::remove_file(&f);
            let _ = std::fs::remove_file(f.with_extension("path"));
        }
    }

    /// Stable name for this document's autosave file.
    pub fn id_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        match &self.path {
            Some(p) => p.hash(&mut h),
            None => (std::process::id(), self.id).hash(&mut h),
        }
        format!("{:016x}", h.finish())
    }

    /// Unsaved changes, including pending edits in the source view.
    pub fn is_dirty(&self) -> bool {
        self.editor.is_dirty()
            || self.source.as_ref().is_some_and(|s| s.revision == u64::MAX)
            // A new, never-saved document is unsaved work.
            || self.path.is_none()
            || self.recovered_from.is_some()
    }

    /// Elements that display images, with their source (cached per structure).
    pub fn images(&mut self) -> Rc<HashMap<NodeId, String>> {
        let rev = self.editor.doc.structure_revision();
        if self.images.0 != rev {
            let doc = &self.editor.doc;
            let mut map = HashMap::new();
            for n in doc.descendants(doc.root()) {
                if doc.is_element(n)
                    && let Some(src) = self.image_src(n)
                {
                    map.insert(n, src);
                }
            }
            self.images = (rev, Rc::new(map));
        }
        self.images.1.clone()
    }

    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        if self.is_dirty() {
            format!("{name} •")
        } else {
            name
        }
    }

    pub fn dir(&self) -> Option<&Path> {
        self.path.as_deref().and_then(Path::parent)
    }

    /// Loads the stylesheet from the package (or none → default styling).
    pub fn load_styles(&mut self) {
        self.style_engine = None;
        self.style_warnings.clear();
        if let Some(css) = self.package.resolved_files().style {
            match xylem_style::Stylesheet::load(&css) {
                Ok(sheet) => {
                    self.style_warnings = sheet.warnings().to_vec();
                    self.style_engine =
                        Some(std::sync::Arc::new(xylem_style::StyleEngine::new(sheet)));
                }
                Err(e) => self.style_warnings.push(format!("{}: {e}", css.display())),
            }
        }
        self.style_revision += 1;
    }

    /// Loads the grammar: package schema first, then the document's own reference.
    pub fn load_grammar(&mut self, catalog: &xylem_schema::Catalog) {
        self.grammar = None;
        let pkg_schema = self.package.resolved_files().schema;
        let result = match pkg_schema {
            Some(p) => {
                let ext = p
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                Some(
                    if ext == "dtd" {
                        Grammar::from_dtd(&p)
                    } else {
                        Grammar::from_xsd(&p)
                    }
                    .map(|g| (g, p.display().to_string())),
                )
            }
            None => Grammar::for_document(&self.editor.doc, self.path.as_deref(), catalog)
                .map(|r| r.map(|g| (g, "document reference".to_string()))),
        };
        match result {
            Some(Ok((g, from))) => {
                self.grammar_status = format!("Schema: {from}");
                self.grammar = Some(Arc::new(g));
            }
            Some(Err(e)) => {
                let msg = e.to_string();
                self.grammar_status = if msg.contains("catalog") {
                    format!(
                        "Schema not available: {msg}. Set the schema catalog or S1000D schema folder in File > Preferences."
                    )
                } else {
                    format!("Schema failed to load: {msg}")
                };
            }
            None => self.grammar_status = "No schema (well-formedness only)".into(),
        }
        self.validated_revision = None;
    }

    /// Current styles (recomputed when the document or stylesheet changed).
    pub fn styles(&mut self) -> Rc<dyn StyleLookup> {
        // CSS styles depend only on structure and attributes, so plain typing
        // reuses them; the default styling looks at text (mixed content).
        let doc_key = if self.style_engine.is_some() {
            self.editor.doc.structure_revision()
        } else {
            self.editor.doc.revision()
        };
        let key = (doc_key, self.style_revision);
        // Pick up a finished background restyle for the current structure.
        if let Some((job_key, rx)) = &self.style_job {
            match rx.try_recv() {
                Ok(css) => {
                    let fresh = *job_key == key;
                    self.style_job = None;
                    if fresh {
                        let css = Rc::new(css);
                        self.exact_css = Some(css.clone());
                        self.styles = Some(css);
                        self.styles_key = key;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.style_job = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.styles.is_some() && self.styles_key == key {
            return self.styles.clone().unwrap();
        }
        let t = std::time::Instant::now();
        let css_dir = self
            .package
            .resolved_files()
            .style
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let styles: Rc<dyn StyleLookup> = match self.style_engine.clone() {
            None => Rc::new(DefaultStyles::compute(&self.editor.doc)),
            Some(engine) => {
                // After a structural edit, show patched styles at once and
                // compute the exact ones on a worker thread.
                let patched = (self.style_revision == self.styles_key.1)
                    .then(|| self.exact_css.clone())
                    .flatten()
                    .and_then(|base| {
                        crate::view::style::PatchedStyles::new(base, &self.editor.doc)
                    });
                match patched {
                    Some(p) => {
                        self.spawn_restyle(key, engine, css_dir);
                        Rc::new(p)
                    }
                    None => {
                        let css = Rc::new(CssStyles::compute(
                            &self.editor.doc,
                            &engine,
                            css_dir.as_deref(),
                        ));
                        self.exact_css = Some(css.clone());
                        css
                    }
                }
            }
        };
        self.styles = Some(styles);
        self.styles_key = key;
        log::debug!("styles ready in {:?}", t.elapsed());
        self.styles.clone().unwrap()
    }

    /// Styles that are exact even right after a structural edit. `styles()`
    /// may then serve borrowed styles whose counters lag behind; that is
    /// fine on screen for a moment but not in an exported or printed copy,
    /// so this computes the exact styles on the spot instead of waiting.
    pub fn exact_styles(&mut self) -> Rc<dyn StyleLookup> {
        let styles = self.styles();
        let (Some(_), Some(engine)) = (&self.style_job, self.style_engine.clone()) else {
            return styles;
        };
        let css_dir = self
            .package
            .resolved_files()
            .style
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        Rc::new(CssStyles::compute(
            &self.editor.doc,
            &engine,
            css_dir.as_deref(),
        ))
    }

    fn spawn_restyle(
        &mut self,
        key: (u64, u64),
        engine: std::sync::Arc<xylem_style::StyleEngine>,
        css_dir: Option<std::path::PathBuf>,
    ) {
        let t = std::time::Instant::now();
        // Styling ignores text, so a structure-only copy is all the worker needs.
        let doc = self.editor.doc.structure_copy();
        log::debug!("restyle: document copied in {:?}", t.elapsed());
        let repaint = self.repaint.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let css = CssStyles::compute(&doc, &engine, css_dir.as_deref());
            if tx.send(css).is_ok()
                && let Some(ctx) = repaint
            {
                ctx.request_repaint();
            }
        });
        self.style_job = Some((key, rx));
    }

    /// Schema-derived memo for [`DocRules`], valid for the current structure.
    pub fn rules_memo(&mut self) -> Rc<std::cell::RefCell<HashMap<NodeId, bool>>> {
        let rev = self.editor.doc.structure_revision();
        if self.rules_memo.0 != rev {
            self.rules_memo = (rev, Default::default());
        }
        self.rules_memo.1.clone()
    }

    /// Style revision that changes whenever styles are recomputed.
    pub fn styles_revision_key(&self) -> u64 {
        self.style_revision.wrapping_mul(1_000_003) ^ self.styles_key.0
    }

    pub fn validate(&mut self) {
        let rev = self.editor.doc.revision();
        if self.validated_revision == Some(rev) {
            return;
        }
        self.validated_revision = Some(rev);
        self.issues.retain(|i| i.source == "BREX");
        let t = std::time::Instant::now();
        if let Some(g) = &self.grammar {
            for e in g.validate(&self.editor.doc) {
                self.issues.push(Issue {
                    node: Some(e.node),
                    severity: match e.severity {
                        xylem_schema::Severity::Error => Severity::Error,
                        _ => Severity::Warning,
                    },
                    message: e.message,
                    source: "Schema",
                });
            }
        }
        self.canvas.flagged = self.issues.iter().filter_map(|i| i.node).collect();
        log::debug!("validated in {:?}", t.elapsed());
    }

    /// The image source attribute for an element configured as an image.
    pub fn image_src(&self, el: NodeId) -> Option<String> {
        let doc = &self.editor.doc;
        let name = doc.name(el)?;
        for img in &self.package.behaviour.images {
            if img.element == name {
                return doc.attr(el, &img.source_attr).map(str::to_string);
            }
        }
        // Common defaults: S1000D graphic/symbol, DocBook/DITA image, HTML img.
        let local = xylem_dom::local_name(name);
        let attr = match local {
            "graphic" | "symbol" => ["infoEntityIdent", "boardno", "fileref", "href"]
                .iter()
                .find_map(|a| doc.attr(el, a)),
            "image" | "img" | "imagedata" => ["href", "src", "fileref", "xlink:href"]
                .iter()
                .find_map(|a| doc.attr(el, a)),
            _ => None,
        };
        attr.map(str::to_string)
    }
}

/// [`Rules`] built from the grammar, stylesheet and package behaviour.
pub struct DocRules<'a> {
    pub grammar: Option<&'a Grammar>,
    pub package: &'a DoctypePackage,
    pub styles: &'a dyn StyleLookup,
    /// Memo for `is_element_only` (queried for every whitespace text node),
    /// shared across frames and reset when the structure changes.
    pub element_only: Rc<std::cell::RefCell<HashMap<NodeId, bool>>>,
}

impl<'a> DocRules<'a> {
    pub fn new(
        grammar: Option<&'a Grammar>,
        package: &'a DoctypePackage,
        styles: &'a dyn StyleLookup,
        memo: Rc<std::cell::RefCell<HashMap<NodeId, bool>>>,
    ) -> Self {
        Self {
            grammar,
            package,
            styles,
            element_only: memo,
        }
    }
}

impl Rules for DocRules<'_> {
    fn can_contain_text(&self, doc: &Document, el: NodeId) -> bool {
        match self.grammar {
            Some(g) if g.element_decl_for(doc, el).is_some() => g.can_contain_text(doc, el),
            _ => true,
        }
    }

    fn can_insert(&self, doc: &Document, parent: NodeId, index: usize, name: &str) -> bool {
        match self.grammar {
            Some(g) if g.element_decl_for(doc, parent).is_some() => {
                g.can_insert(doc, parent, index, name)
            }
            _ => true,
        }
    }

    fn is_element_only(&self, doc: &Document, el: NodeId) -> bool {
        if let Some(&v) = self.element_only.borrow().get(&el) {
            return v;
        }
        let v = matches!(
            self.grammar
                .and_then(|g| g.element_decl_for(doc, el).map(|d| g.content_type(d))),
            Some(ContentType::ElementOnly | ContentType::Empty)
        );
        self.element_only.borrow_mut().insert(el, v);
        v
    }

    fn is_preformatted(&self, _doc: &Document, el: NodeId) -> bool {
        matches!(
            self.styles.get(el).white_space,
            WhiteSpace::Pre | WhiteSpace::PreWrap
        )
    }

    fn is_block(&self, doc: &Document, el: NodeId) -> bool {
        let name = doc.name(el).unwrap_or("");
        self.styles.get(el).display.is_block()
            || self.package.behaviour.paragraphs.iter().any(|p| p == name)
    }

    fn is_read_only(&self, doc: &Document, el: NodeId) -> bool {
        let name = doc.name(el).unwrap_or("");
        self.package
            .behaviour
            .read_only_elements
            .iter()
            .any(|p| p == name)
    }

    fn is_non_removable(&self, doc: &Document, el: NodeId) -> bool {
        let name = doc.name(el).unwrap_or("");
        self.package
            .behaviour
            .non_removable_elements
            .iter()
            .any(|p| p == name)
    }

    fn is_protected(&self, doc: &Document, node: NodeId) -> bool {
        let b = &self.package.behaviour;
        match doc.kind(node) {
            xylem_dom::NodeKind::Pi { target, .. } => b.protected_pis.iter().any(|t| t == target),
            xylem_dom::NodeKind::EntityRef(_) => b.protect_entity_refs,
            _ => false,
        }
    }

    fn followed_by(&self, name: &str) -> Option<String> {
        self.package.behaviour.followed_by.get(name).cloned()
    }

    fn new_element_xml(&self, doc: &Document, parent: NodeId, index: usize, name: &str) -> String {
        let xml = if let Some(t) = self.package.template(name)
            && xylem_dom::parse_fragment(&t.xml).is_ok()
        {
            t.xml.clone()
        } else if let Some(g) = self.grammar
            && let Some(ins) = g
                .insertable_at(doc, parent, index)
                .into_iter()
                .find(|i| i.name == name)
        {
            skeleton_xml(&g.required_skeleton(ins.decl))
        } else {
            format!("<{name}/>")
        };
        with_generated_ids(self.package, doc, &xml)
    }
}

/// Gives elements in a fragment about to be inserted an ID when the package
/// asks for generated IDs and the element doesn't have one yet. Running this
/// on the fragment (rather than the document afterwards) keeps the IDs in
/// the same undo step as the insert and leaves existing elements alone.
pub fn with_generated_ids(package: &DoctypePackage, doc: &Document, xml: &str) -> String {
    let Some(idg) = &package.behaviour.id_generation else {
        return xml.to_string();
    };
    let Ok(mut frag) = xylem_dom::parse_fragment(xml) else {
        return xml.to_string();
    };
    let mut taken: std::collections::HashSet<String> = doc
        .descendants(doc.root())
        .filter_map(|n| doc.attr(n, &idg.attr).map(str::to_string))
        .collect();
    let mut seq = taken.len() as u64;
    let targets: Vec<NodeId> = frag
        .descendants(frag.root())
        .filter(|&n| {
            frag.name(n)
                .is_some_and(|nm| idg.elements.iter().any(|e| e == nm))
        })
        .collect();
    let mut changed = false;
    for n in targets {
        // Templates may carry a fixed placeholder ID; replace it if it clashes.
        if frag.attr(n, &idg.attr).is_some_and(|v| !taken.contains(v)) {
            continue;
        }
        let name = frag.name(n).unwrap_or("").to_string();
        let id = loop {
            seq += 1;
            let candidate = idg.generate(&name, seq);
            if !taken.contains(&candidate) {
                break candidate;
            }
        };
        taken.insert(id.clone());
        frag.set_attr(n, &idg.attr, id);
        changed = true;
    }
    if !changed {
        return xml.to_string();
    }
    frag.children(frag.root())
        .iter()
        .map(|&c| frag.node_to_xml(c))
        .collect()
}

pub fn skeleton_xml(s: &xylem_schema::Skeleton) -> String {
    let mut out = format!("<{}", s.name);
    for (k, v) in &s.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        xylem_dom::escape_attr(v, &mut out);
        out.push('"');
    }
    if s.children.is_empty() && !s.wants_text {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    for c in &s.children {
        out.push_str(&skeleton_xml(c));
    }
    out.push_str(&format!("</{}>", s.name));
    out
}
