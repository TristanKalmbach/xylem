//! The Xylem application: windows, menus, toolbar, document tabs and dialogs.

use crate::doc::{DocRules, DocState, Issue, Severity, SourceState, View};
use crate::find::FindState;
use crate::panels::{self, PanelAction, PanelState};
use crate::settings::Settings;
use crate::spell::Speller;
use crate::view::canvas::{CanvasAction, CanvasCtx, XmlClipboard};
use crate::view::layout::FontSet;
use crate::view::source;
use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use xylem_customize::{DoctypePackage, DoctypeRegistry};
use xylem_dom::{Document, NodeId, NodeKind};
use xylem_edit::pos::{container, first_editable};
use xylem_edit::{EditError, Pos, Rules, Selection};

/// Commands reachable from menus, the toolbar, shortcuts and context menus.
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    New,
    NewS1000d,
    Open,
    OpenPath(PathBuf),
    Save,
    SaveAs,
    Close(usize),
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteText,
    SelectAll,
    SelectElement,
    Find,
    View(View),
    ZoomIn,
    ZoomOut,
    ZoomReset,
    Validate,
    CheckBrex,
    InsertElement(String),
    Surround(String),
    Toggle(String),
    RemoveTags,
    DeleteElement,
    SplitElement,
    JoinPreceding,
    EditAttributes,
    InsertComment,
    InsertPi,
    InsertCData,
    InsertTable,
    TableRowAbove,
    TableRowBelow,
    TableDeleteRow,
    TableColLeft,
    TableColRight,
    TableDeleteColumn,
    TableMergeRight,
    TableMergeDown,
    TableSplitCell,
    TableDelete,
    TableRowUp,
    TableRowDown,
    PrettyPrint,
    ImportXmax,
    Preferences,
    About,
    ReloadPackages,
    S1000dInfo,
    RenameToCsdb,
    FollowLink(NodeId),
    SpellCheck,
    Replace(NodeId, std::ops::Range<usize>, String),
    AddToDictionary(String),
}

#[derive(Default)]
struct Dialogs {
    about: bool,
    prefs: bool,
    table: Option<TableDialog>,
    node_edit: Option<NodeEditDialog>,
    new_doc: Option<NewDocDialog>,
    new_dm: Option<crate::s1000d_ui::NewDmDialog>,
    info: bool,
    confirm_close: Option<usize>,
    confirm_exit: bool,
    import_result: Option<String>,
}

struct TableDialog {
    rows: usize,
    cols: usize,
    header: bool,
    cals: bool,
    title: String,
}

struct NodeEditDialog {
    node: NodeId,
    target: String,
    text: String,
}

struct NewDocDialog {
    package: String,
    root: String,
}

struct ContextMenu {
    pos: egui::Pos2,
    node: NodeId,
    insertable: Vec<String>,
    suggestions: Option<(NodeId, std::ops::Range<usize>, String, Vec<String>)>,
}

pub struct XylemApp {
    pub settings: Settings,
    registry: DoctypeRegistry,
    catalog: xylem_schema::Catalog,
    docs: Vec<DocState>,
    active: usize,
    next_id: u64,
    clipboard: XmlClipboard,
    fonts: FontSet,
    panel: PanelState,
    find: FindState,
    dialogs: Dialogs,
    toasts: Vec<(String, f64, bool)>,
    context_menu: Option<ContextMenu>,
    tag_colors: HashMap<String, Color32>,
    speller: Option<Speller>,
    pending_open: Vec<PathBuf>,
    allow_close: bool,
    /// Debug hook: `XYLEM_SCREENSHOT=<png>` captures the window and exits.
    screenshot: Option<(PathBuf, u32)>,
    script: Option<crate::script::Script>,
    shot_path: Option<PathBuf>,
}

impl XylemApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "settings"))
            .unwrap_or_default();
        if let Some(brex) = std::env::var_os("XYLEM_BREX") {
            settings.brex = Some(PathBuf::from(brex));
        }
        if let Some(extra) = std::env::var_os("XYLEM_PACKAGES") {
            for p in std::env::split_paths(&extra) {
                if !settings.package_dirs.contains(&p) {
                    settings.package_dirs.insert(0, p);
                }
            }
        }
        let fonts = crate::fonts::install(&cc.egui_ctx);
        if let Some(dark) = settings.dark_mode {
            cc.egui_ctx.set_theme(if dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            });
        }
        install_builtin_packages();
        let mut app = Self {
            registry: DoctypeRegistry::scan(&settings.package_dirs),
            catalog: Self::load_catalog(&settings),
            settings,
            docs: Vec::new(),
            active: 0,
            next_id: 1,
            clipboard: XmlClipboard::default(),
            fonts,
            panel: PanelState::default(),
            find: FindState::default(),
            dialogs: Dialogs::default(),
            toasts: Vec::new(),
            context_menu: None,
            tag_colors: HashMap::new(),
            speller: None,
            pending_open: files,
            allow_close: false,
            screenshot: std::env::var_os("XYLEM_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
            script: crate::script::Script::from_env(),
            shot_path: None,
        };
        app.load_speller();
        app
    }

    fn load_catalog(settings: &Settings) -> xylem_schema::Catalog {
        let mut cat = settings
            .catalog
            .as_ref()
            .and_then(|p| xylem_schema::Catalog::load(p).ok())
            .unwrap_or_default();
        if let Some(root) = &settings.s1000d_schema_root {
            crate::s1000d_ui::add_s1000d_catalog(&mut cat, root);
        }
        cat
    }

    fn load_speller(&mut self) {
        self.speller = if self.settings.spell_check {
            Speller::load(
                self.settings.dictionary_dir.as_deref(),
                &self.settings.dictionary_lang,
            )
        } else {
            None
        };
    }

    fn toast(&mut self, msg: impl Into<String>, error: bool) {
        let msg = msg.into();
        log::info!("{msg}");
        self.toasts.push((msg, 0.0, error));
        if self.toasts.len() > 5 {
            self.toasts.remove(0);
        }
    }

    fn doc(&mut self) -> Option<&mut DocState> {
        self.docs.get_mut(self.active)
    }

    // ------------------------------------------------------------ files

    pub fn open_path(&mut self, path: PathBuf) {
        let path = std::path::absolute(&path).unwrap_or(path);
        if let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path.as_path()))
        {
            self.active = i;
            return;
        }
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return self.toast(format!("Cannot open {}: {e}", path.display()), true),
        };
        match Document::from_bytes(&bytes) {
            Ok(doc) => {
                self.add_doc(doc, Some(path.clone()));
            }
            Err(e) => {
                // Not well-formed: open in source view so it can be fixed.
                let text = String::from_utf8_lossy(&bytes).into_owned();
                let mut doc = Document::new();
                let root = doc.create_element("recovered");
                doc.append_child(doc.root(), root);
                self.add_doc(doc, Some(path.clone()));
                let ds = self.docs.last_mut().unwrap();
                ds.view = View::Source;
                ds.source = Some(SourceState {
                    text,
                    error: Some((e.to_string(), e.offset)),
                    revision: u64::MAX,
                });
                self.toast(
                    format!(
                        "{} is not well-formed ({e}). Opened in Source view.",
                        path.display()
                    ),
                    true,
                );
            }
        }
        self.settings.add_recent(path);
    }

    fn add_doc(&mut self, doc: Document, path: Option<PathBuf>) {
        let pkg = self
            .registry
            .match_document(&doc, path.as_deref())
            .cloned()
            .map(Arc::new)
            .unwrap_or_else(|| Arc::new(DoctypePackage::generic()));
        let id = self.next_id;
        self.next_id += 1;
        let mut ds = DocState::new(id, doc, path, pkg);
        ds.view = self.settings.default_view;
        ds.load_styles();
        ds.load_grammar(&self.catalog);
        for w in ds.style_warnings.iter().take(3) {
            log::warn!("{w}");
        }
        if ds.grammar_status.contains("failed") || ds.grammar_status.contains("not available") {
            let m = ds.grammar_status.clone();
            self.toast(m, true);
        }
        // Tag colours from the package pretty-print settings.
        for (el, pp) in &ds.package.pretty_print.elements {
            if let Some(c) = pp.tag_color.as_deref().and_then(parse_color) {
                self.tag_colors.insert(el.clone(), c);
            }
        }
        let r = ds
            .editor
            .doc
            .document_element()
            .map(|r| first_editable(&ds.editor.doc, &xylem_edit::NoRules, r));
        if let Some(p) = r {
            ds.editor.set_caret(p);
        }
        ds.canvas.request_focus = true;
        self.docs.push(ds);
        self.active = self.docs.len() - 1;
    }

    fn open_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new()
            .add_filter(
                "XML documents",
                &[
                    "xml", "XML", "sgm", "dita", "xhtml", "htm", "svg", "xsd", "xsl",
                ],
            )
            .add_filter("All files", &["*"]);
        if let Some(dir) = self.doc().and_then(|d| d.dir().map(Path::to_path_buf)) {
            dlg = dlg.set_directory(dir);
        }
        if let Some(paths) = dlg.pick_files() {
            for p in paths {
                self.open_path(p);
            }
        }
    }

    fn sync_source_to_doc(&mut self) -> bool {
        let Some(ds) = self.docs.get_mut(self.active) else {
            return true;
        };
        let Some(src) = &mut ds.source else {
            return true;
        };
        if src.revision == ds.editor.doc.revision() {
            return true;
        }
        match Document::from_str(&src.text) {
            Ok(mut doc) => {
                doc.encoding = ds.editor.doc.encoding;
                ds.editor.replace_document(doc);
                ds.source = None;
                ds.inline_styles.clear();
                let was_recovered = ds.grammar.is_none();
                if was_recovered {
                    ds.load_grammar(&self.catalog);
                }
                ds.canvas.invalidate();
                true
            }
            Err(e) => {
                src.error = Some((e.to_string(), e.offset));
                false
            }
        }
    }

    fn save(&mut self, save_as: bool) {
        if !self.sync_source_to_doc() {
            return self.toast("Fix the well-formedness error before saving.", true);
        }
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let path = if save_as || ds.path.is_none() {
            let mut d = rfd::FileDialog::new().add_filter("XML", &["xml", "XML"]);
            if let Some(dir) = ds.dir() {
                d = d.set_directory(dir);
            }
            if let Some(name) = ds.path.as_ref().and_then(|p| p.file_name()) {
                d = d.set_file_name(name.to_string_lossy());
            } else if let Some(name) = crate::s1000d_ui::suggested_filename(&ds.editor.doc) {
                d = d.set_file_name(name);
            }
            match d.save_file() {
                Some(p) => p,
                None => return,
            }
        } else {
            ds.path.clone().unwrap()
        };
        let bytes = ds.editor.doc.to_bytes();
        let tmp = path.with_extension("xylem-tmp");
        let result = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path));
        match result {
            Ok(()) => {
                ds.editor.mark_saved();
                ds.path = Some(path.clone());
                self.settings.add_recent(path.clone());
                self.toast(format!("Saved {}", path.display()), false);
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                self.toast(format!("Save failed: {e}"), true);
            }
        }
    }

    // ------------------------------------------------------------ commands

    fn with_editor<T>(
        &mut self,
        f: impl FnOnce(&mut xylem_edit::Editor, &dyn Rules) -> Result<T, EditError>,
    ) -> Option<T> {
        let ds = self.docs.get_mut(self.active)?;
        if ds.read_only {
            self.toasts
                .push(("The document is read-only.".into(), 0.0, true));
            return None;
        }
        if ds.view == View::Source {
            return None;
        }
        let styles = ds.styles();
        let grammar = ds.grammar.clone();
        let package = ds.package.clone();
        let memo = ds.rules_memo();
        let rules = DocRules::new(grammar.as_deref(), &package, &*styles, memo);
        let r = f(&mut ds.editor, &rules);
        ds.canvas.scroll_to_caret = true;
        ds.canvas.request_focus = true;
        match r {
            Ok(v) => Some(v),
            Err(EditError::Nothing) => None,
            Err(e) => {
                self.toast(e.to_string(), true);
                None
            }
        }
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::New => {
                self.dialogs.new_doc = Some(NewDocDialog {
                    package: "generic".into(),
                    root: String::new(),
                });
            }
            Cmd::NewS1000d => self.dialogs.new_dm = Some(crate::s1000d_ui::NewDmDialog::default()),
            Cmd::Open => self.open_dialog(),
            Cmd::OpenPath(p) => self.open_path(p),
            Cmd::Save => self.save(false),
            Cmd::SaveAs => self.save(true),
            Cmd::Close(i) => {
                if self.docs.get(i).is_some_and(|d| d.is_dirty()) {
                    self.dialogs.confirm_close = Some(i);
                } else if i < self.docs.len() {
                    self.docs.remove(i);
                    self.active = self.active.min(self.docs.len().saturating_sub(1));
                }
            }
            Cmd::Undo => {
                self.with_editor(|ed, _| {
                    if ed.undo() {
                        Ok(())
                    } else {
                        Err(EditError::Nothing)
                    }
                });
            }
            Cmd::Redo => {
                self.with_editor(|ed, _| {
                    if ed.redo() {
                        Ok(())
                    } else {
                        Err(EditError::Nothing)
                    }
                });
            }
            Cmd::Copy | Cmd::Cut => {
                let Some(ds) = self.docs.get(self.active) else {
                    return;
                };
                if let (Some(xml), Some(text)) =
                    (ds.editor.selection_xml(), ds.editor.selection_text())
                {
                    self.clipboard = XmlClipboard {
                        xml,
                        text: text.clone(),
                    };
                    ctx.copy_text(text);
                }
                if cmd == Cmd::Cut {
                    self.with_editor(|ed, r| ed.delete_selection(r));
                }
            }
            Cmd::Paste | Cmd::PasteText => {
                // egui only delivers clipboard text through a Paste event; use the
                // internal clipboard here (system paste arrives via Ctrl+V).
                let clip = self.clipboard.clone();
                if clip.text.is_empty() {
                    self.toast("Use Ctrl+V to paste from the system clipboard.", false);
                    return;
                }
                let as_xml = cmd == Cmd::Paste;
                self.with_editor(|ed, r| {
                    ed.paste(r, if as_xml { &clip.xml } else { &clip.text }, as_xml)
                });
            }
            Cmd::SelectAll => {
                self.with_editor(|ed, r| {
                    ed.select_all(r);
                    Ok(())
                });
            }
            Cmd::SelectElement => {
                self.with_editor(|ed, _| {
                    let c = container(&ed.doc, ed.caret());
                    if Some(c) != ed.doc.document_element() {
                        ed.select_element(c);
                    }
                    Ok(())
                });
            }
            Cmd::Find => self.find.open = true,
            Cmd::View(v) => self.set_view(v),
            Cmd::ZoomIn => self.settings.zoom = (self.settings.zoom + 0.1).min(3.0),
            Cmd::ZoomOut => self.settings.zoom = (self.settings.zoom - 0.1).max(0.5),
            Cmd::ZoomReset => self.settings.zoom = 1.0,
            Cmd::Validate => {
                if let Some(ds) = self.doc() {
                    ds.validated_revision = None;
                    ds.validate();
                    let n = ds
                        .issues
                        .iter()
                        .filter(|i| i.severity == Severity::Error)
                        .count();
                    self.settings.show_issues = true;
                    self.toast(
                        if n == 0 {
                            "Document is valid.".to_string()
                        } else {
                            format!("{n} validation error(s).")
                        },
                        n > 0,
                    );
                }
            }
            Cmd::CheckBrex => self.check_brex(),
            Cmd::InsertElement(name) => {
                let r = self.with_editor(|ed, r| ed.insert_element(r, &name));
                if r.is_some() {
                    self.after_insert();
                }
            }
            Cmd::Surround(name) => {
                let g = self.docs.get(self.active).and_then(|d| d.grammar.clone());
                let n2 = name.clone();
                self.with_editor(move |ed, r| {
                    let check = |doc: &Document, p: NodeId, s: usize, e: usize| {
                        g.as_ref().is_none_or(|g| {
                            g.element_decl_for(doc, p).is_none()
                                || g.can_surround(doc, p, s, e, &n2)
                        })
                    };
                    ed.surround(r, &name, &check).map(|_| ())
                });
            }
            Cmd::Toggle(kind) => {
                let Some(ds) = self.docs.get(self.active) else {
                    return;
                };
                let Some(el) = ds
                    .package
                    .behaviour
                    .toggles
                    .get(&kind)
                    .cloned()
                    .or_else(|| default_toggle(&kind, ds))
                else {
                    return self.toast(format!("No element is configured for {kind}."), true);
                };
                let g = ds.grammar.clone();
                let n2 = el.clone();
                self.with_editor(move |ed, r| {
                    let check = |doc: &Document, p: NodeId, s: usize, e: usize| {
                        g.as_ref().is_none_or(|g| {
                            g.element_decl_for(doc, p).is_none()
                                || g.can_surround(doc, p, s, e, &n2)
                        })
                    };
                    ed.toggle_inline(r, &el, &check)
                });
            }
            Cmd::RemoveTags => {
                self.with_editor(|ed, r| {
                    let el = panels_current(ed);
                    ed.remove_tags(r, el)
                });
            }
            Cmd::DeleteElement => {
                self.with_editor(|ed, r| {
                    let el = panels_current(ed);
                    ed.delete_element(r, el)
                });
            }
            Cmd::SplitElement => {
                self.with_editor(|ed, r| ed.split_container(r));
            }
            Cmd::JoinPreceding => {
                self.with_editor(|ed, r| {
                    let el = panels_current(ed);
                    let block = std::iter::once(el)
                        .chain(ed.doc.ancestors(el))
                        .find(|&n| ed.can_join_to_preceding(r, n))
                        .unwrap_or(el);
                    ed.join_to_preceding(r, block)
                });
            }
            Cmd::EditAttributes => self.settings.show_attributes = true,
            Cmd::InsertComment => {
                if let Some(n) =
                    self.with_editor(|ed, r| ed.insert_special(r, NodeKind::Comment(" ".into())))
                {
                    self.dialogs.node_edit = Some(NodeEditDialog {
                        node: n,
                        target: String::new(),
                        text: String::new(),
                    });
                }
            }
            Cmd::InsertPi => {
                if let Some(n) = self.with_editor(|ed, r| {
                    ed.insert_special(
                        r,
                        NodeKind::Pi {
                            target: "pi".into(),
                            data: String::new(),
                        },
                    )
                }) {
                    self.dialogs.node_edit = Some(NodeEditDialog {
                        node: n,
                        target: "pi".into(),
                        text: String::new(),
                    });
                }
            }
            Cmd::InsertCData => {
                if let Some(n) =
                    self.with_editor(|ed, r| ed.insert_special(r, NodeKind::CData(String::new())))
                {
                    self.dialogs.node_edit = Some(NodeEditDialog {
                        node: n,
                        target: String::new(),
                        text: String::new(),
                    });
                }
            }
            Cmd::InsertTable => {
                self.dialogs.table = Some(TableDialog {
                    rows: 3,
                    cols: 3,
                    header: true,
                    cals: true,
                    title: String::new(),
                })
            }
            Cmd::TableRowAbove => {
                self.with_editor(|ed, r| ed.table_insert_rows(r, 1, false));
            }
            Cmd::TableRowBelow => {
                self.with_editor(|ed, r| ed.table_insert_rows(r, 1, true));
            }
            Cmd::TableDeleteRow => {
                self.with_editor(|ed, r| ed.table_delete_row(r));
            }
            Cmd::TableColLeft => {
                self.with_editor(|ed, r| ed.table_insert_column(r, false));
            }
            Cmd::TableColRight => {
                self.with_editor(|ed, r| ed.table_insert_column(r, true));
            }
            Cmd::TableDeleteColumn => {
                self.with_editor(|ed, r| ed.table_delete_column(r));
            }
            Cmd::TableMergeRight => {
                self.with_editor(|ed, r| ed.table_merge_right(r));
            }
            Cmd::TableMergeDown => {
                self.with_editor(|ed, r| ed.table_merge_down(r));
            }
            Cmd::TableSplitCell => {
                self.with_editor(|ed, r| ed.table_split_cell(r));
            }
            Cmd::TableDelete => {
                self.with_editor(|ed, r| ed.table_delete(r));
            }
            Cmd::TableRowUp => {
                self.with_editor(|ed, r| ed.table_move_row(r, true));
            }
            Cmd::TableRowDown => {
                self.with_editor(|ed, r| ed.table_move_row(r, false));
            }
            Cmd::PrettyPrint => {
                let Some(ds) = self.doc() else { return };
                if !matches!(ds.view, View::Source) {
                    ds.view = View::Source;
                }
                let text = ds.editor.doc.pretty_xml(ds.editor.doc.root());
                ds.source = Some(SourceState {
                    text,
                    error: None,
                    revision: u64::MAX,
                });
            }
            Cmd::ImportXmax => self.import_xmax(),
            Cmd::Preferences => self.dialogs.prefs = true,
            Cmd::About => self.dialogs.about = true,
            Cmd::ReloadPackages => {
                self.registry = DoctypeRegistry::scan(&self.settings.package_dirs);
                self.catalog = Self::load_catalog(&self.settings);
                let n = self.registry.packages().len();
                let errs = self.registry.errors().len();
                self.toast(
                    format!(
                        "{n} doctype package(s) loaded{}",
                        if errs > 0 {
                            format!(", {errs} failed")
                        } else {
                            String::new()
                        }
                    ),
                    errs > 0,
                );
            }
            Cmd::S1000dInfo => self.dialogs.info = true,
            Cmd::RenameToCsdb => self.rename_to_csdb(),
            Cmd::FollowLink(n) => self.follow_link(n),
            Cmd::SpellCheck => {
                self.settings.spell_check = !self.settings.spell_check;
                self.load_speller();
                if self.settings.spell_check && self.speller.is_none() {
                    self.toast(
                        "No Hunspell dictionary found. Set the dictionary folder in Preferences.",
                        true,
                    );
                }
            }
            Cmd::Replace(node, range, with) => {
                self.with_editor(|ed, r| {
                    ed.replace_text_range(r, node, range.start, range.end, &with)
                });
            }
            Cmd::AddToDictionary(w) => {
                if let Some(s) = &mut self.speller {
                    s.add_word(&w);
                }
            }
        }
        if let Some(ds) = self.docs.get_mut(self.active) {
            ds.editor.fix_selection(&xylem_edit::NoRules);
        }
    }

    fn after_insert(&mut self) {
        // Auto IDs for configured elements.
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let Some(idg) = ds.package.behaviour.id_generation.clone() else {
            return;
        };
        let doc = &ds.editor.doc;
        let mut todo = Vec::new();
        let mut seq = doc
            .descendants(doc.root())
            .filter(|&n| doc.attr(n, &idg.attr).is_some())
            .count();
        for n in doc.descendants(doc.root()) {
            if let Some(name) = doc.name(n)
                && idg.elements.iter().any(|e| e == name)
                && doc.attr(n, &idg.attr).is_none()
            {
                seq += 1;
                todo.push((n, idg.generate(name, seq as u64)));
            }
        }
        for (n, id) in todo {
            ds.editor.doc.set_attr(n, &idg.attr, id);
        }
    }

    fn set_view(&mut self, v: View) {
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        if ds.view == v {
            return;
        }
        if ds.view == View::Source && !self.sync_source_to_doc() {
            return self.toast(
                "The source is not well-formed; fix it before leaving Source view.",
                true,
            );
        }
        let ds = self.docs.get_mut(self.active).unwrap();
        if v == View::Source {
            ds.source = Some(SourceState {
                text: ds.editor.doc.to_xml(),
                error: None,
                revision: ds.editor.doc.revision(),
            });
        }
        ds.view = v;
        ds.canvas.invalidate();
        ds.canvas.request_focus = true;
    }

    fn new_from_package(&mut self, pkg: &str, root: Option<&str>) {
        let package = if pkg == "generic" {
            Some(DoctypePackage::generic())
        } else {
            self.registry.get(pkg).cloned()
        };
        let Some(package) = package else { return };
        let root_name = root
            .map(str::to_string)
            .filter(|r| !r.is_empty())
            .or_else(|| package.matching.roots.first().cloned())
            .unwrap_or_else(|| "document".into());
        let xml = package
            .template(&root_name)
            .map(|t| t.xml.clone())
            .filter(|x| xylem_dom::parse_fragment(x).is_ok())
            .unwrap_or_else(|| format!("<{root_name}/>"));
        let mut text = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let schema_name = package.files.schema.as_deref().and_then(|s| {
            Path::new(s)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
        });
        let mut doc = match Document::from_str(&format!("{text}{xml}")) {
            Ok(d) => d,
            Err(e) => return self.toast(format!("Template error: {e}"), true),
        };
        let r = doc.document_element().unwrap();
        if let (Some(s), true) = (
            schema_name,
            doc.attr(r, "xsi:noNamespaceSchemaLocation").is_none(),
        ) && s.ends_with(".xsd")
        {
            doc.set_attr(r, "xmlns:xsi", "http://www.w3.org/2001/XMLSchema-instance");
            doc.set_attr(r, "xsi:noNamespaceSchemaLocation", s);
        }
        // Expand a bare root into its required skeleton.
        text.clear();
        let pkg_arc = Arc::new(package);
        let id = self.next_id;
        self.next_id += 1;
        let mut ds = DocState::new(id, doc, None, pkg_arc);
        ds.view = self.settings.default_view;
        ds.load_styles();
        ds.load_grammar(&self.catalog);
        if let Some(g) = ds.grammar.clone() {
            let doc = &mut ds.editor.doc;
            let r = doc.document_element().unwrap();
            if doc.children(r).is_empty()
                && let Some(d) = g.element_decl_for(doc, r)
            {
                let sk = g.required_skeleton(d);
                for c in &sk.children {
                    if let Ok(nodes) = doc.parse_fragment_into(&crate::doc::skeleton_xml(c)) {
                        for n in nodes {
                            doc.append_child(r, n);
                        }
                    }
                }
            }
        }
        let r = ds.editor.doc.document_element().unwrap();
        let p = first_editable(&ds.editor.doc, &xylem_edit::NoRules, r);
        ds.editor.set_caret(p);
        ds.editor.clear_undo();
        ds.canvas.request_focus = true;
        self.docs.push(ds);
        self.active = self.docs.len() - 1;
    }

    fn import_xmax(&mut self) {
        let Some(src) = rfd::FileDialog::new()
            .set_title("Choose an XMAX customization folder (with a .ctm)")
            .pick_folder()
            .or_else(|| {
                rfd::FileDialog::new()
                    .set_title("…or choose a .xac file")
                    .add_filter("XMAX customization", &["xac"])
                    .pick_file()
            })
        else {
            return;
        };
        let Some(dest_root) = self.settings.package_dirs.first().cloned() else {
            return;
        };
        let name = src
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "imported".into());
        let out = dest_root.join(&name);
        let msg = match xylem_customize::import_xmax(&src, &out) {
            Ok(report) => format!("{report}"),
            Err(e) => format!("Import failed: {e}"),
        };
        self.dialogs.import_result = Some(msg);
        self.run_reload();
    }

    fn run_reload(&mut self) {
        self.registry = DoctypeRegistry::scan(&self.settings.package_dirs);
    }

    fn check_brex(&mut self) {
        let path = match self.settings.brex.clone().filter(|p| p.exists()) {
            Some(p) => p,
            None => match rfd::FileDialog::new()
                .set_title("Choose a BREX data module")
                .add_filter("XML", &["xml", "XML"])
                .pick_file()
            {
                Some(p) => {
                    self.settings.brex = Some(p.clone());
                    p
                }
                None => return,
            },
        };
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        match crate::s1000d_ui::run_brex(&ds.editor.doc, &path) {
            Ok(issues) => {
                ds.issues.retain(|i| i.source != "BREX");
                let n = issues.len();
                ds.issues.extend(issues);
                ds.canvas.flagged = ds.issues.iter().filter_map(|i| i.node).collect();
                self.settings.show_issues = true;
                self.toast(
                    if n == 0 {
                        "No BREX violations.".to_string()
                    } else {
                        format!("{n} BREX violation(s).")
                    },
                    n > 0,
                );
            }
            Err(e) => self.toast(format!("BREX: {e}"), true),
        }
    }

    fn rename_to_csdb(&mut self) {
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let (Some(path), Some(name)) = (
            ds.path.clone(),
            crate::s1000d_ui::suggested_filename(&ds.editor.doc),
        ) else {
            return self.toast("Save the data module first (and give it a DMC).", true);
        };
        let new = path.with_file_name(&name);
        if new == path {
            return self.toast("The file name already matches the DMC.", false);
        }
        if new.exists() {
            return self.toast(format!("{name} already exists."), true);
        }
        match std::fs::rename(&path, &new) {
            Ok(()) => {
                ds.path = Some(new.clone());
                self.toast(format!("Renamed to {name}"), false);
            }
            Err(e) => self.toast(format!("Rename failed: {e}"), true),
        }
    }

    fn follow_link(&mut self, n: NodeId) {
        let Some(ds) = self.docs.get(self.active) else {
            return;
        };
        let doc = &ds.editor.doc;
        // S1000D references.
        if let Some(target) = crate::s1000d_ui::resolve_ref(doc, n, ds.dir()) {
            return self.open_path(target);
        }
        // Generic href/xlink:href to a local file, or an IDREF inside the document.
        let el = std::iter::once(n).chain(doc.ancestors(n)).find(|&a| {
            ["href", "xlink:href", "linkend", "refid", "internalRefId"]
                .iter()
                .any(|k| doc.attr(a, k).is_some())
        });
        let Some(el) = el else { return };
        for k in ["linkend", "refid", "internalRefId"] {
            if let Some(id) = doc.attr(el, k)
                && let Some(t) = doc
                    .descendants(doc.root())
                    .find(|&x| doc.attr(x, "id") == Some(id))
            {
                let p = first_editable(doc, &xylem_edit::NoRules, t);
                let ds = self.docs.get_mut(self.active).unwrap();
                ds.editor.set_caret(p);
                ds.canvas.reveal(t);
                return;
            }
        }
        if let Some(href) = doc.attr(el, "href").or_else(|| doc.attr(el, "xlink:href")) {
            let file = href.split('#').next().unwrap_or("");
            if let Some(dir) = ds.dir() {
                let p = dir.join(file);
                if p.is_file() {
                    self.open_path(p)
                }
            }
        }
    }

    // ------------------------------------------------------------ UI

    fn shortcuts(&mut self, ctx: &egui::Context) -> Vec<Cmd> {
        let mut cmds = Vec::new();
        let cmd = Modifiers::COMMAND;
        let sh = |k| KeyboardShortcut::new(cmd, k);
        let shs = |k| KeyboardShortcut::new(cmd | Modifiers::SHIFT, k);
        ctx.input_mut(|i| {
            if i.consume_shortcut(&sh(Key::O)) {
                cmds.push(Cmd::Open);
            }
            if i.consume_shortcut(&shs(Key::S)) {
                cmds.push(Cmd::SaveAs);
            }
            if i.consume_shortcut(&sh(Key::S)) {
                cmds.push(Cmd::Save);
            }
            if i.consume_shortcut(&sh(Key::N)) {
                cmds.push(Cmd::New);
            }
            if i.consume_shortcut(&sh(Key::F)) || i.consume_shortcut(&sh(Key::H)) {
                cmds.push(Cmd::Find);
            }
            if i.consume_shortcut(&sh(Key::W)) {
                cmds.push(Cmd::Close(self.active));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F7)) {
                cmds.push(Cmd::Validate);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F5)) {
                cmds.push(Cmd::View(View::TagsOn));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F6)) {
                cmds.push(Cmd::View(View::Normal));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F8)) {
                cmds.push(Cmd::View(View::Source));
            }
            if i.consume_shortcut(&sh(Key::Plus)) || i.consume_shortcut(&sh(Key::Equals)) {
                cmds.push(Cmd::ZoomIn);
            }
            if i.consume_shortcut(&sh(Key::Minus)) {
                cmds.push(Cmd::ZoomOut);
            }
            if i.consume_shortcut(&sh(Key::Num0)) {
                cmds.push(Cmd::ZoomReset);
            }
            if i.consume_shortcut(&sh(Key::B)) {
                cmds.push(Cmd::Toggle("bold".into()));
            }
            if i.consume_shortcut(&sh(Key::I)) {
                cmds.push(Cmd::Toggle("italic".into()));
            }
            if i.consume_shortcut(&sh(Key::U)) {
                cmds.push(Cmd::Toggle("underline".into()));
            }
            if i.consume_shortcut(&sh(Key::E)) {
                cmds.push(Cmd::SelectElement);
            }
            if i.consume_shortcut(&shs(Key::V)) {
                cmds.push(Cmd::PasteText);
            }
        });
        cmds
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        let has_doc = !self.docs.is_empty();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New…\tCtrl+N").clicked() {
                    cmds.push(Cmd::New);
                }
                if ui.button("New S1000D data module…").clicked() {
                    cmds.push(Cmd::NewS1000d);
                }
                if ui.button("Open…\tCtrl+O").clicked() {
                    cmds.push(Cmd::Open);
                }
                ui.menu_button("Open recent", |ui| {
                    if self.settings.recent_files.is_empty() {
                        ui.label(RichText::new("(none)").weak());
                    }
                    for p in self.settings.recent_files.clone() {
                        if ui.button(p.display().to_string()).clicked() {
                            cmds.push(Cmd::OpenPath(p));
                        }
                    }
                });
                ui.separator();
                if ui
                    .add_enabled(has_doc, egui::Button::new("Save\tCtrl+S"))
                    .clicked()
                {
                    cmds.push(Cmd::Save);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Save as…\tCtrl+Shift+S"))
                    .clicked()
                {
                    cmds.push(Cmd::SaveAs);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Close\tCtrl+W"))
                    .clicked()
                {
                    cmds.push(Cmd::Close(self.active));
                }
                ui.separator();
                if ui.button("Import XMAX customization…").clicked() {
                    cmds.push(Cmd::ImportXmax);
                }
                if ui.button("Reload doctype packages").clicked() {
                    cmds.push(Cmd::ReloadPackages);
                }
                if ui.button("Preferences…").clicked() {
                    cmds.push(Cmd::Preferences);
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                for (label, c) in [
                    ("Undo\tCtrl+Z", Cmd::Undo),
                    ("Redo\tCtrl+Y", Cmd::Redo),
                    ("Cut\tCtrl+X", Cmd::Cut),
                    ("Copy\tCtrl+C", Cmd::Copy),
                    ("Paste (internal clipboard)", Cmd::Paste),
                    ("Paste as text\tCtrl+Shift+V", Cmd::PasteText),
                    ("Select all\tCtrl+A", Cmd::SelectAll),
                    ("Select element\tCtrl+E", Cmd::SelectElement),
                    ("Find / Replace…\tCtrl+F", Cmd::Find),
                ] {
                    if ui.add_enabled(has_doc, egui::Button::new(label)).clicked() {
                        cmds.push(c);
                    }
                }
            });
            ui.menu_button("Insert", |ui| {
                if ui
                    .add_enabled(has_doc, egui::Button::new("Element…"))
                    .clicked()
                {
                    self.settings.show_elements = true;
                    self.panel.element_mode = panels::ElementMode::Insert;
                }
                for (label, c) in [
                    ("Table…", Cmd::InsertTable),
                    ("Comment", Cmd::InsertComment),
                    ("Processing instruction", Cmd::InsertPi),
                    ("CDATA section", Cmd::InsertCData),
                ] {
                    if ui.add_enabled(has_doc, egui::Button::new(label)).clicked() {
                        cmds.push(c);
                    }
                }
            });
            ui.menu_button("Format", |ui| {
                for (label, kind) in [
                    ("Bold\tCtrl+B", "bold"),
                    ("Italic\tCtrl+I", "italic"),
                    ("Underline\tCtrl+U", "underline"),
                    ("Superscript", "superscript"),
                    ("Subscript", "subscript"),
                ] {
                    if ui.add_enabled(has_doc, egui::Button::new(label)).clicked() {
                        cmds.push(Cmd::Toggle(kind.into()));
                    }
                }
                ui.separator();
                for (label, c) in [
                    ("Remove tags", Cmd::RemoveTags),
                    ("Split element", Cmd::SplitElement),
                    ("Join to preceding", Cmd::JoinPreceding),
                    ("Delete element", Cmd::DeleteElement),
                ] {
                    if ui.add_enabled(has_doc, egui::Button::new(label)).clicked() {
                        cmds.push(c);
                    }
                }
            });
            ui.menu_button("Table", |ui| table_menu(ui, cmds));
            ui.menu_button("View", |ui| {
                for (label, v) in [
                    ("Tags On\tF5", View::TagsOn),
                    ("Normal\tF6", View::Normal),
                    ("Source\tF8", View::Source),
                ] {
                    if ui.add_enabled(has_doc, egui::Button::new(label)).clicked() {
                        cmds.push(Cmd::View(v));
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.settings.show_structure, "Structure");
                ui.checkbox(&mut self.settings.show_elements, "Elements");
                ui.checkbox(&mut self.settings.show_attributes, "Attributes");
                ui.checkbox(&mut self.settings.show_issues, "Issues");
                ui.separator();
                if ui.button("Zoom in\tCtrl++").clicked() {
                    cmds.push(Cmd::ZoomIn);
                }
                if ui.button("Zoom out\tCtrl+-").clicked() {
                    cmds.push(Cmd::ZoomOut);
                }
                if ui.button("Actual size\tCtrl+0").clicked() {
                    cmds.push(Cmd::ZoomReset);
                }
                ui.separator();
                let dark = ui.visuals().dark_mode;
                if ui
                    .button(if dark { "Light theme" } else { "Dark theme" })
                    .clicked()
                {
                    self.settings.dark_mode = Some(!dark);
                    ui.ctx().set_theme(if dark {
                        egui::Theme::Light
                    } else {
                        egui::Theme::Dark
                    });
                }
            });
            ui.menu_button("Tools", |ui| {
                if ui
                    .add_enabled(has_doc, egui::Button::new("Validate\tF7"))
                    .clicked()
                {
                    cmds.push(Cmd::Validate);
                }
                ui.checkbox(&mut self.settings.auto_validate, "Validate as you type");
                if ui
                    .button(if self.settings.spell_check {
                        "Turn spell check off"
                    } else {
                        "Turn spell check on"
                    })
                    .clicked()
                {
                    cmds.push(Cmd::SpellCheck);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Pretty-print source"))
                    .clicked()
                {
                    cmds.push(Cmd::PrettyPrint);
                }
                ui.separator();
                ui.label(RichText::new("S1000D").weak());
                if ui.button("New data module…").clicked() {
                    cmds.push(Cmd::NewS1000d);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Check BREX…"))
                    .clicked()
                {
                    cmds.push(Cmd::CheckBrex);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Data module info…"))
                    .clicked()
                {
                    cmds.push(Cmd::S1000dInfo);
                }
                if ui
                    .add_enabled(has_doc, egui::Button::new("Rename file to CSDB name"))
                    .clicked()
                {
                    cmds.push(Cmd::RenameToCsdb);
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About Xylem").clicked() {
                    cmds.push(Cmd::About);
                }
            });
        });
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        ui.horizontal_wrapped(|ui| {
            let b = |ui: &mut egui::Ui, label: &str, tip: &str| {
                ui.button(label).on_hover_text(tip).clicked()
            };
            if b(ui, "New", "New (Ctrl+N)") {
                cmds.push(Cmd::New);
            }
            if b(ui, "Open", "Open (Ctrl+O)") {
                cmds.push(Cmd::Open);
            }
            if b(ui, "Save", "Save (Ctrl+S)") {
                cmds.push(Cmd::Save);
            }
            ui.separator();
            if b(ui, "Undo", "Undo (Ctrl+Z)") {
                cmds.push(Cmd::Undo);
            }
            if b(ui, "Redo", "Redo (Ctrl+Y)") {
                cmds.push(Cmd::Redo);
            }
            ui.separator();
            let view = self.docs.get(self.active).map(|d| d.view);
            for (label, v, tip) in [
                ("Tags", View::TagsOn, "Tags On view (F5)"),
                ("Normal", View::Normal, "Normal view (F6)"),
                ("Source", View::Source, "Source view (F8)"),
            ] {
                if ui
                    .selectable_label(view == Some(v), label)
                    .on_hover_text(tip)
                    .clicked()
                {
                    cmds.push(Cmd::View(v));
                }
            }
            ui.separator();
            if ui
                .button(RichText::new("B").strong())
                .on_hover_text("Bold (Ctrl+B)")
                .clicked()
            {
                cmds.push(Cmd::Toggle("bold".into()));
            }
            if ui
                .button(RichText::new("I").italics())
                .on_hover_text("Italic (Ctrl+I)")
                .clicked()
            {
                cmds.push(Cmd::Toggle("italic".into()));
            }
            if ui
                .button(RichText::new("U").underline())
                .on_hover_text("Underline (Ctrl+U)")
                .clicked()
            {
                cmds.push(Cmd::Toggle("underline".into()));
            }
            // Package-defined insert buttons (lists, images, paragraphs…).
            if let Some(ds) = self.docs.get(self.active) {
                for entry in ds.package.effective_toolbar() {
                    let label = entry.label.clone();
                    match &entry.action {
                        xylem_customize::Action::InsertElement(e) => {
                            if ui
                                .button(&label)
                                .on_hover_text(format!("Insert <{e}>"))
                                .clicked()
                            {
                                cmds.push(Cmd::InsertElement(e.clone()));
                            }
                        }
                        xylem_customize::Action::Surround(e) => {
                            if ui
                                .button(&label)
                                .on_hover_text(format!("Surround with <{e}>"))
                                .clicked()
                            {
                                cmds.push(Cmd::Surround(e.clone()));
                            }
                        }
                        xylem_customize::Action::Toggle(k)
                            if !matches!(k.as_str(), "bold" | "italic" | "underline")
                                && ui.button(&label).clicked() =>
                        {
                            cmds.push(Cmd::Toggle(k.clone()));
                        }
                        _ => {}
                    }
                }
            }
            ui.separator();
            if b(ui, "Table", "Insert table") {
                cmds.push(Cmd::InsertTable);
            }
            if b(ui, "Validate", "Validate (F7)") {
                cmds.push(Cmd::Validate);
            }
            if b(ui, "Find", "Find / Replace (Ctrl+F)") {
                cmds.push(Cmd::Find);
            }
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        ui.horizontal(|ui| {
            for (i, d) in self.docs.iter().enumerate() {
                let r = ui.selectable_label(i == self.active, d.title());
                let r = match &d.path {
                    Some(p) => r.on_hover_text(p.display().to_string()),
                    None => r,
                };
                if r.clicked() {
                    self.active = i;
                }
                if r.middle_clicked() {
                    cmds.push(Cmd::Close(i));
                }
                if ui.small_button("x").on_hover_text("Close").clicked() {
                    cmds.push(Cmd::Close(i));
                }
                ui.separator();
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.heading(RichText::new("Xylem").size(34.0));
            ui.label("Structured XML and S1000D authoring");
            ui.add_space(20.0);
            if ui.button("Open a document…").clicked() {
                cmds.push(Cmd::Open);
            }
            if ui.button("New document…").clicked() {
                cmds.push(Cmd::New);
            }
            if ui.button("New S1000D data module…").clicked() {
                cmds.push(Cmd::NewS1000d);
            }
            ui.add_space(16.0);
            if !self.settings.recent_files.is_empty() {
                ui.label(RichText::new("Recent").strong());
                for p in self.settings.recent_files.iter().take(10) {
                    if ui.link(p.display().to_string()).clicked() {
                        cmds.push(Cmd::OpenPath(p.clone()));
                    }
                }
            }
            ui.add_space(16.0);
            let n = self.registry.packages().len();
            ui.label(
                RichText::new(format!(
                    "{n} doctype package(s) available. Drop files here to open them."
                ))
                .weak(),
            );
        });
    }

    fn document_area(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        let zoom = self.settings.zoom;
        let fonts = self.fonts.clone();
        let tag_colors = self.tag_colors.clone();
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        if ds.view == View::Source {
            let src = ds.source.get_or_insert_with(|| SourceState {
                text: ds.editor.doc.to_xml(),
                error: None,
                revision: ds.editor.doc.revision(),
            });
            let mut goto = src
                .error
                .as_ref()
                .map(|e| e.1)
                .filter(|_| ui.input(|i| i.key_pressed(Key::F4)));
            let err = src.error.clone();
            if source::show(ui, &mut src.text, err.as_ref(), zoom, &mut goto) {
                src.revision = u64::MAX;
                src.error = None;
            }
            return;
        }
        let styles_rc = ds.styles();
        let style_rev = ds.styles_revision_key();
        let styles: &dyn crate::view::style::StyleLookup = &*styles_rc;
        let grammar = ds.grammar.clone();
        let package = ds.package.clone();
        let memo = ds.rules_memo();
        let rules = DocRules::new(grammar.as_deref(), &package, styles, memo);
        let image_src = ds.images();
        let mut sizes: HashMap<NodeId, (String, egui::Vec2)> = HashMap::new();
        for (n, src) in image_src.iter() {
            let size = ds
                .image_sizes
                .entry(src.clone())
                .or_insert_with(|| {
                    let p = ds_resolve(ds_dir(&ds.path), src)?;
                    image_dims(&p).or(Some(egui::vec2(160.0, 60.0)))
                })
                .unwrap_or(egui::vec2(160.0, 40.0));
            sizes.insert(*n, (src.clone(), size));
        }
        let dir = ds.dir().map(Path::to_path_buf);
        let image_size = |n: NodeId| sizes.get(&n).cloned();
        let tex_cache = std::cell::RefCell::new(std::mem::take(&mut ds.image_cache));
        let image_texture = |ctx: &egui::Context, n: NodeId| -> Option<egui::TextureHandle> {
            let src = image_src.get(&n)?;
            let mut cache = tex_cache.borrow_mut();
            cache
                .entry(src.clone())
                .or_insert_with(|| {
                    let p = ds_resolve(dir.as_deref(), src)?;
                    load_texture(ctx, &p)
                })
                .clone()
        };
        if let Some(sp) = &self.speller {
            if ds.canvas.misspelled_revision() != Some(ds.editor.doc.revision()) {
                let words = sp.check_document(&ds.editor.doc, &mut ds.spell_cache);
                ds.canvas.set_misspelled(words, ds.editor.doc.revision());
            }
        } else {
            ds.canvas.misspelled.clear();
        }
        let mut cx = CanvasCtx {
            rules: &rules,
            styles,
            style_revision: style_rev,
            view: ds.view.layout_mode(),
            zoom,
            fonts: &fonts,
            tag_colors: &tag_colors,
            clipboard: &mut self.clipboard,
            image_size: &image_size,
            image_texture: &image_texture,
            read_only: ds.read_only,
        };
        let mut actions = Vec::new();
        ds.canvas.show(ui, &mut ds.editor, &mut cx, &mut actions);
        ds.image_cache = tex_cache.into_inner();
        let pointer = ui.input(|i| i.pointer.latest_pos()).unwrap_or_default();
        for a in actions {
            match a {
                CanvasAction::Message(m) => self.toasts.push((m, 0.0, true)),
                CanvasAction::ContextMenu { node } => {
                    let insertable = grammar
                        .as_ref()
                        .map(|g| {
                            let (p, i) = ds.editor.insertion_point();
                            let mut v: Vec<String> = g
                                .insertable_at(&ds.editor.doc, p, i)
                                .into_iter()
                                .filter(|x| x.keeps_valid)
                                .map(|x| x.name)
                                .collect();
                            v.sort();
                            v.dedup();
                            v
                        })
                        .unwrap_or_default();
                    let suggestions = self.speller.as_ref().and_then(|sp| {
                        let p = ds.editor.caret();
                        let t = ds.editor.doc.text(p.node)?;
                        let (s, e) = word_at(t, p.offset)?;
                        let w = &t[s..e];
                        if sp.is_correct(w) {
                            return None;
                        }
                        Some((p.node, s..e, w.to_string(), sp.suggest(w)))
                    });
                    self.context_menu = Some(ContextMenu {
                        pos: pointer,
                        node,
                        insertable,
                        suggestions,
                    });
                }
                CanvasAction::EditAttributes(_) => self.settings.show_attributes = true,
                CanvasAction::EditNode(n) => {
                    if rules.is_protected(&ds.editor.doc, n) {
                        self.toasts.push(("This marker is protected and can't be edited.".into(), 0.0, true));
                        continue;
                    }
                    let (target, text) = match ds.editor.doc.kind(n) {
                        NodeKind::Pi { target, data } => (target.clone(), data.clone()),
                        NodeKind::Comment(t) | NodeKind::CData(t) => (String::new(), t.clone()),
                        _ => continue,
                    };
                    self.dialogs.node_edit = Some(NodeEditDialog {
                        node: n,
                        target,
                        text,
                    });
                }
                CanvasAction::OpenLink(n) => cmds.push(Cmd::FollowLink(n)),
                CanvasAction::Changed => {}
            }
        }
    }

    fn show_context_menu(&mut self, ctx: &egui::Context, cmds: &mut Vec<Cmd>) {
        let Some(menu) = &self.context_menu else {
            return;
        };
        let mut close = false;
        let in_table = self
            .docs
            .get(self.active)
            .is_some_and(|d| d.editor.in_table());
        let name = self
            .docs
            .get(self.active)
            .and_then(|d| d.editor.doc.name(menu.node).map(str::to_string))
            .unwrap_or_default();
        let area = egui::Area::new(egui::Id::new("xylem-ctx-menu"))
            .fixed_pos(menu.pos)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::menu(ui.style()).show(ui, |ui| {
                    ui.set_min_width(180.0);
                    let mut picked: Option<Cmd> = None;
                    let item =
                        |ui: &mut egui::Ui, label: &str, c: Cmd, picked: &mut Option<Cmd>| {
                            if ui.button(label).clicked() {
                                *picked = Some(c);
                            }
                        };
                    if let Some((node, range, word, sugg)) = &menu.suggestions {
                        ui.label(RichText::new(format!("“{word}”")).weak());
                        for s in sugg.iter().take(6) {
                            item(
                                ui,
                                s,
                                Cmd::Replace(*node, range.clone(), s.clone()),
                                &mut picked,
                            );
                        }
                        item(
                            ui,
                            "Add to dictionary",
                            Cmd::AddToDictionary(word.clone()),
                            &mut picked,
                        );
                        ui.separator();
                    }
                    item(ui, "Cut", Cmd::Cut, &mut picked);
                    item(ui, "Copy", Cmd::Copy, &mut picked);
                    item(ui, "Paste", Cmd::Paste, &mut picked);
                    ui.separator();
                    if !menu.insertable.is_empty() {
                        ui.menu_button("Insert element", |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(360.0)
                                .show(ui, |ui| {
                                    for n in &menu.insertable {
                                        if ui.button(n).clicked() {
                                            picked = Some(Cmd::InsertElement(n.clone()));
                                        }
                                    }
                                });
                        });
                    }
                    if !name.is_empty() {
                        item(
                            ui,
                            &format!("Select <{name}>"),
                            Cmd::SelectElement,
                            &mut picked,
                        );
                        item(
                            ui,
                            &format!("Remove <{name}> tags"),
                            Cmd::RemoveTags,
                            &mut picked,
                        );
                        item(
                            ui,
                            &format!("Split <{name}>"),
                            Cmd::SplitElement,
                            &mut picked,
                        );
                        item(ui, "Join to preceding", Cmd::JoinPreceding, &mut picked);
                        item(
                            ui,
                            &format!("Delete <{name}>"),
                            Cmd::DeleteElement,
                            &mut picked,
                        );
                        item(ui, "Edit attributes", Cmd::EditAttributes, &mut picked);
                        item(ui, "Follow link", Cmd::FollowLink(menu.node), &mut picked);
                    }
                    if in_table {
                        ui.separator();
                        ui.menu_button("Table", |ui| table_menu(ui, cmds));
                    }
                    if let Some(c) = picked {
                        cmds.push(c);
                        close = true;
                    }
                });
            });
        let clicked_elsewhere = ctx.input(|i| i.pointer.any_pressed())
            && !area
                .response
                .rect
                .contains(ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default())
            && !ctx.is_pointer_over_area();
        if close || clicked_elsewhere || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.context_menu = None;
        }
    }

    fn panels(&mut self, ctx: &egui::Context, cmds: &mut Vec<Cmd>) {
        let mut acts = Vec::new();
        if self.docs.is_empty() {
            return;
        }
        if self.settings.show_structure {
            egui::SidePanel::left("structure")
                .resizable(true)
                .default_width(230.0)
                .show(ctx, |ui| {
                    ui.heading("Structure");
                    if let Some(ds) = self.docs.get(self.active) {
                        panels::structure(ui, ds, &mut self.panel, &mut acts);
                    }
                });
        }
        if self.settings.show_elements || self.settings.show_attributes {
            egui::SidePanel::right("inspector")
                .resizable(true)
                .default_width(260.0)
                .show(ctx, |ui| {
                    let Some(ds) = self.docs.get_mut(self.active) else {
                        return;
                    };
                    let styles = ds.styles();
                    let memo = ds.rules_memo();
                    let ds = &*ds;
                    let rules = DocRules::new(ds.grammar.as_deref(), &ds.package, &*styles, memo);
                    let h = ui.available_height();
                    if self.settings.show_elements {
                        ui.heading("Elements");
                        egui::Frame::NONE.show(ui, |ui| {
                            ui.set_height(if self.settings.show_attributes {
                                h * 0.48
                            } else {
                                h - 30.0
                            });
                            panels::elements(ui, ds, &rules, &mut self.panel, &mut acts);
                        });
                    }
                    if self.settings.show_attributes {
                        ui.separator();
                        ui.heading("Attributes");
                        panels::attributes(ui, ds, &mut self.panel, &mut acts);
                    }
                });
        }
        if self.settings.show_issues {
            egui::TopBottomPanel::bottom("issues")
                .resizable(true)
                .default_height(120.0)
                .show(ctx, |ui| {
                    let Some(ds) = self.docs.get(self.active) else {
                        return;
                    };
                    let errs = ds
                        .issues
                        .iter()
                        .filter(|i| i.severity == Severity::Error)
                        .count();
                    ui.horizontal(|ui| {
                        ui.strong(format!("Issues ({errs})"));
                        ui.label(RichText::new(&ds.grammar_status).weak());
                    });
                    panels::issues(ui, ds, &mut acts);
                });
        }
        for a in acts {
            self.panel_action(a, cmds);
        }
    }

    fn panel_action(&mut self, a: PanelAction, cmds: &mut Vec<Cmd>) {
        match a {
            PanelAction::Select(n) => {
                if let Some(ds) = self.doc() {
                    if Some(n) == ds.editor.doc.document_element() {
                        let p = first_editable(&ds.editor.doc, &xylem_edit::NoRules, n);
                        ds.editor.set_caret(p);
                    } else {
                        ds.editor.select_element(n);
                    }
                    ds.canvas.reveal(n);
                    ds.canvas.request_focus = true;
                }
            }
            PanelAction::InsertElement(n) => cmds.push(Cmd::InsertElement(n)),
            PanelAction::Surround(n) => cmds.push(Cmd::Surround(n)),
            PanelAction::Rename(el, n) => {
                self.with_editor(|ed, r| ed.rename_element(r, el, &n));
            }
            PanelAction::SetAttr(el, k, v) => {
                self.with_editor(|ed, r| ed.set_attribute(r, el, &k, &v));
            }
            PanelAction::RemoveAttr(el, k) => {
                self.with_editor(|ed, r| ed.remove_attribute(r, el, &k));
            }
            PanelAction::GotoIssue(node) => {
                let Some(ds) = self.doc() else { return };
                let Some(n) = node.filter(|n| ds.editor.doc.contains(*n)) else {
                    return;
                };
                if ds.view == View::Source {
                    return;
                }
                let target = if ds.editor.doc.is_element(n) {
                    n
                } else {
                    ds.editor.doc.parent(n).unwrap_or(n)
                };
                let p = first_editable(&ds.editor.doc, &xylem_edit::NoRules, target);
                ds.editor.set_caret(p);
                ds.canvas.reveal(target);
                ds.canvas.request_focus = true;
            }
            PanelAction::RemoveTags(n) => {
                self.with_editor(|ed, r| ed.remove_tags(r, n));
            }
            PanelAction::DeleteElement(n) => {
                self.with_editor(|ed, r| ed.delete_element(r, n));
            }
            PanelAction::MoveElement(n, up) => {
                self.with_editor(|ed, r| ed.move_element(r, n, up));
            }
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        let mut acts = Vec::new();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let Some(ds) = self.docs.get(self.active) else {
                    ui.label("Ready");
                    return;
                };
                panels::breadcrumb(ui, ds, &mut acts);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("{:.0}%", self.settings.zoom * 100.0));
                    ui.separator();
                    ui.label(&ds.package.doctype.name);
                    if let Some(info) = &ds.s1000d {
                        ui.separator();
                        ui.label(format!("S1000D {}", info.issue));
                    }
                    ui.separator();
                    let errs = ds
                        .issues
                        .iter()
                        .filter(|i| i.severity == Severity::Error)
                        .count();
                    if ds.grammar.is_some() {
                        if errs == 0 {
                            ui.colored_label(Color32::from_rgb(60, 160, 80), "✔ valid");
                        } else {
                            ui.colored_label(Color32::from_rgb(220, 70, 70), format!("⛔ {errs}"));
                        }
                    }
                });
            });
        });
        let mut cmds = Vec::new();
        for a in acts {
            self.panel_action(a, &mut cmds);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, cmds: &mut Vec<Cmd>) {
        self.find_window(ctx);
        if self.dialogs.about {
            egui::Window::new("About Xylem").collapsible(false).resizable(false).open(&mut self.dialogs.about).show(ctx, |ui| {
                ui.heading("Xylem");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A structured XML and S1000D editor written entirely in Rust.");
                ui.label("Tags-on and normal views driven by CSS, schema-guided editing (XSD and DTD), CALS tables, BREX checking.");
            });
        }
        if let Some(t) = &mut self.dialogs.table {
            let mut open = true;
            let mut insert = false;
            egui::Window::new("Insert table")
                .collapsible(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::Grid::new("tbl").num_columns(2).show(ui, |ui| {
                        ui.label("Rows");
                        ui.add(egui::DragValue::new(&mut t.rows).range(1..=200));
                        ui.end_row();
                        ui.label("Columns");
                        ui.add(egui::DragValue::new(&mut t.cols).range(1..=40));
                        ui.end_row();
                        ui.label("Title");
                        ui.text_edit_singleline(&mut t.title);
                        ui.end_row();
                    });
                    ui.checkbox(&mut t.header, "Header row");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut t.cals, true, "CALS (table/tgroup/row/entry)");
                        ui.radio_value(&mut t.cals, false, "HTML (table/tr/td)");
                    });
                    insert = ui.button("Insert").clicked();
                });
            if insert {
                let xml = if t.cals {
                    xylem_edit::table::cals_table_xml(
                        t.rows,
                        t.cols,
                        t.header,
                        Some(t.title.as_str()).filter(|s| !s.is_empty()),
                    )
                } else {
                    xylem_edit::table::html_table_xml(t.rows, t.cols, t.header)
                };
                self.dialogs.table = None;
                self.with_editor(|ed, r| ed.insert_element_xml(r, "table", &xml).map(|_| ()));
            } else if !open {
                self.dialogs.table = None;
            }
        }
        if let Some(ne) = &mut self.dialogs.node_edit {
            let mut open = true;
            let mut apply = false;
            egui::Window::new("Edit node")
                .collapsible(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    if !ne.target.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label("Target");
                            ui.text_edit_singleline(&mut ne.target);
                        });
                    }
                    ui.add(
                        egui::TextEdit::multiline(&mut ne.text)
                            .desired_rows(5)
                            .desired_width(380.0),
                    );
                    apply = ui.button("OK").clicked();
                });
            if apply {
                let (n, target, text) = (ne.node, ne.target.clone(), ne.text.clone());
                self.dialogs.node_edit = None;
                self.with_editor(|ed, _| {
                    ed.transact(|ed| {
                        if !ed.doc.contains(n) {
                            return Err(EditError::Nothing);
                        }
                        if let NodeKind::Pi { .. } = ed.doc.kind(n) {
                            let parent = ed.doc.parent(n).ok_or(EditError::Nothing)?;
                            let idx = ed.doc.index_in_parent(n).unwrap();
                            ed.doc.detach(n);
                            let pi = ed.doc.create_node(NodeKind::Pi { target, data: text });
                            ed.doc.insert_child(parent, idx, pi);
                        } else {
                            ed.doc.set_text(n, text);
                        }
                        Ok(())
                    })
                });
            } else if !open {
                self.dialogs.node_edit = None;
            }
        }
        if let Some(nd) = &mut self.dialogs.new_doc {
            let mut open = true;
            let mut create = false;
            let names: Vec<(String, String)> =
                std::iter::once(("generic".to_string(), "Generic XML (no schema)".to_string()))
                    .chain(self.registry.packages().iter().map(|p| {
                        (
                            p.doctype.name.clone(),
                            if p.doctype.description.is_empty() {
                                p.doctype.name.clone()
                            } else {
                                format!("{} — {}", p.doctype.name, p.doctype.description)
                            },
                        )
                    }))
                    .collect();
            egui::Window::new("New document")
                .collapsible(false)
                .open(&mut open)
                .default_width(460.0)
                .show(ctx, |ui| {
                    ui.label("Document type");
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (name, label) in &names {
                                if ui.selectable_label(&nd.package == name, label).clicked() {
                                    nd.package = name.clone();
                                    nd.root.clear();
                                }
                            }
                        });
                    let roots: Vec<String> = self
                        .registry
                        .get(&nd.package)
                        .map(|p| p.matching.roots.clone())
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label("Root element");
                        egui::ComboBox::from_id_salt("newroot")
                            .selected_text(if nd.root.is_empty() {
                                roots.first().cloned().unwrap_or_else(|| "document".into())
                            } else {
                                nd.root.clone()
                            })
                            .show_ui(ui, |ui| {
                                for r in &roots {
                                    ui.selectable_value(&mut nd.root, r.clone(), r);
                                }
                            });
                        ui.text_edit_singleline(&mut nd.root);
                    });
                    create = ui.button("Create").clicked();
                });
            if create {
                let (p, r) = (nd.package.clone(), nd.root.clone());
                self.dialogs.new_doc = None;
                self.new_from_package(&p, Some(&r));
            } else if !open {
                self.dialogs.new_doc = None;
            }
        }
        if let Some(dm) = &mut self.dialogs.new_dm {
            match crate::s1000d_ui::new_dm_window(ctx, dm) {
                crate::s1000d_ui::DialogResult::Open => {}
                crate::s1000d_ui::DialogResult::Cancel => self.dialogs.new_dm = None,
                crate::s1000d_ui::DialogResult::Create(xml, filename) => {
                    self.dialogs.new_dm = None;
                    match Document::from_str(&xml) {
                        Ok(doc) => {
                            self.add_doc(doc, None);
                            if let Some(ds) = self.doc() {
                                ds.editor.clear_undo();
                            }
                            self.toast(
                                format!("Created {filename}. Save to choose its folder."),
                                false,
                            );
                        }
                        Err(e) => self.toast(format!("Template error: {e}"), true),
                    }
                }
            }
        }
        if self.dialogs.info {
            let mut open = true;
            let info = self
                .docs
                .get(self.active)
                .map(|d| crate::s1000d_ui::info_text(&d.editor.doc))
                .unwrap_or_default();
            egui::Window::new("Data module")
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(RichText::new(info).monospace());
                });
            self.dialogs.info = open;
        }
        if let Some(msg) = &self.dialogs.import_result {
            let mut open = true;
            egui::Window::new("XMAX import")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(400.0)
                        .show(ui, |ui| ui.label(RichText::new(msg).monospace()));
                });
            if !open {
                self.dialogs.import_result = None;
            }
        }
        if self.dialogs.prefs {
            let mut open = true;
            let mut reload = false;
            egui::Window::new("Preferences")
                .open(&mut open)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.strong("Doctype package folders");
                    let mut remove = None;
                    for (i, d) in self.settings.package_dirs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(d.display().to_string());
                            if ui.small_button("✕").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        self.settings.package_dirs.remove(i);
                        reload = true;
                    }
                    if ui.button("Add folder…").clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_folder()
                    {
                        self.settings.package_dirs.push(p);
                        reload = true;
                    }
                    ui.separator();
                    path_pref(
                        ui,
                        "Schema catalog (catalog.toml or OASIS XML)",
                        &mut self.settings.catalog,
                        false,
                        &mut reload,
                    );
                    path_pref(
                        ui,
                        "S1000D schema folder (sub-folder per issue)",
                        &mut self.settings.s1000d_schema_root,
                        true,
                        &mut reload,
                    );
                    path_pref(
                        ui,
                        "BREX data module",
                        &mut self.settings.brex,
                        false,
                        &mut reload,
                    );
                    let mut spell_reload = false;
                    path_pref(
                        ui,
                        "Hunspell dictionary folder",
                        &mut self.settings.dictionary_dir,
                        true,
                        &mut spell_reload,
                    );
                    ui.horizontal(|ui| {
                        ui.label("Dictionary language");
                        if ui
                            .text_edit_singleline(&mut self.settings.dictionary_lang)
                            .lost_focus()
                        {
                            spell_reload = true;
                        }
                    });
                    if spell_reload {
                        self.speller = None;
                        reload = true;
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Default view");
                        ui.selectable_value(
                            &mut self.settings.default_view,
                            View::TagsOn,
                            "Tags On",
                        );
                        ui.selectable_value(
                            &mut self.settings.default_view,
                            View::Normal,
                            "Normal",
                        );
                    });
                    ui.checkbox(&mut self.settings.auto_validate, "Validate as you type");
                });
            self.dialogs.prefs = open;
            if reload {
                cmds.push(Cmd::ReloadPackages);
                self.load_speller();
            }
        }
        if let Some(i) = self.dialogs.confirm_close {
            let title = self.docs.get(i).map(|d| d.title()).unwrap_or_default();
            let mut choice = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Save changes to {title}?"));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            choice = Some(0);
                        }
                        if ui.button("Don't save").clicked() {
                            choice = Some(1);
                        }
                        if ui.button("Cancel").clicked() {
                            choice = Some(2);
                        }
                    });
                });
            match choice {
                Some(0) => {
                    self.active = i;
                    self.save(false);
                    if !self.docs[i].is_dirty() {
                        self.docs.remove(i);
                        self.active = self.active.min(self.docs.len().saturating_sub(1));
                    }
                    self.dialogs.confirm_close = None;
                }
                Some(1) => {
                    self.docs.remove(i);
                    self.active = self.active.min(self.docs.len().saturating_sub(1));
                    self.dialogs.confirm_close = None;
                }
                Some(_) => self.dialogs.confirm_close = None,
                None => {}
            }
        }
        if self.dialogs.confirm_exit {
            let mut choice = None;
            egui::Window::new("Quit Xylem")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    let n = self.docs.iter().filter(|d| d.is_dirty()).count();
                    ui.label(format!(
                        "{n} document(s) have unsaved changes. Quit anyway?"
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Quit without saving").clicked() {
                            choice = Some(true);
                        }
                        if ui.button("Cancel").clicked() {
                            choice = Some(false);
                        }
                    });
                });
            match choice {
                Some(true) => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                Some(false) => self.dialogs.confirm_exit = false,
                None => {}
            }
        }
    }

    fn find_window(&mut self, ctx: &egui::Context) {
        if !self.find.open {
            return;
        }
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let mut acts = Vec::new();
        crate::find::window(ctx, &mut self.find, ds, &mut acts);
        for a in acts {
            match a {
                crate::find::FindAction::Goto(node, range) => {
                    let ds = self.docs.get_mut(self.active).unwrap();
                    if ds.view == View::Source {
                        continue;
                    }
                    ds.editor.sel = Selection {
                        anchor: Pos::new(node, range.start),
                        focus: Pos::new(node, range.end),
                    };
                    ds.canvas.scroll_to_caret = true;
                }
                crate::find::FindAction::Replace(node, range, with) => {
                    self.with_editor(|ed, r| {
                        ed.replace_text_range(r, node, range.start, range.end, &with)
                    });
                }
                crate::find::FindAction::ReplaceAll(items) => {
                    let n = items.len();
                    self.with_editor(|ed, r| {
                        ed.transact(|ed| {
                            // Apply from the end of each node backwards so ranges stay valid.
                            let mut items = items.clone();
                            items.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.start.cmp(&a.1.start)));
                            for (node, range, with) in items {
                                if r.is_read_only(&ed.doc, ed.doc.parent(node).unwrap_or(node)) {
                                    continue;
                                }
                                let mut s = ed.doc.text(node).unwrap_or("").to_string();
                                if range.end <= s.len() {
                                    s.replace_range(range, &with);
                                    ed.doc.set_text(node, s);
                                }
                            }
                            Ok(())
                        })
                    });
                    self.toast(format!("Replaced {n} occurrence(s)."), false);
                }
            }
        }
        if let Some(ds) = self.docs.get_mut(self.active) {
            ds.canvas.highlights = self.find.highlights();
        }
    }

    fn show_toasts(&mut self, ctx: &egui::Context) {
        let dt = ctx.input(|i| i.stable_dt).min(0.1) as f64;
        self.toasts.retain_mut(|t| {
            t.1 += dt;
            t.1 < 6.0
        });
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -40.0])
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                for (msg, _, err) in &self.toasts {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(RichText::new(msg).color(if *err {
                            Color32::from_rgb(230, 90, 80)
                        } else {
                            ui.visuals().text_color()
                        }));
                    });
                }
            });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl XylemApp {
    fn debug_screenshot(&mut self, ctx: &egui::Context) {
        // One-shot capture after N frames.
        if let Some((path, frame)) = &mut self.screenshot {
            *frame += 1;
            let wait: u32 = std::env::var("XYLEM_SCREENSHOT_FRAMES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30);
            if *frame == wait {
                self.shot_path = Some(path.clone());
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
        }
        // Scripted steps.
        if let Some(script) = &mut self.script {
            if let Some(p) = script.pending_shot.take() {
                self.shot_path = Some(p);
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            }
            if let Some(p) = script.pending_save.take()
                && let Some(ds) = self.docs.get(self.active)
            {
                let xml = match &ds.source {
                    Some(src) if ds.view == View::Source => src.text.clone(),
                    _ => ds.editor.doc.to_xml(),
                };
                let _ = std::fs::write(&p, xml);
                let errors = ds.issues.iter().filter(|i| i.severity == Severity::Error).count();
                eprintln!("script save {}: {} issue(s), {} error(s), dirty={}", p.display(), ds.issues.len(), errors, ds.is_dirty());
                for i in ds.issues.iter().take(5) {
                    eprintln!("  [{}] {}", i.source, i.message);
                }
            }
            if script.quit && self.shot_path.is_none() {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        let shot = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(img) = shot.filter(|_| self.shot_path.is_some()) {
            let path = self.shot_path.take().unwrap();
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                let _ = buf.save(&path);
            }
            if self.script.is_none() {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.screenshot.is_some() || self.script.is_some() {
            ctx.request_repaint();
        }
    }
}

/// Built-in doctype packages shipped inside the binary.
const BUILTIN: &[(&str, &str, &str)] = &[
    (
        "s1000d",
        "doctype.toml",
        include_str!("../builtin/s1000d/doctype.toml"),
    ),
    (
        "s1000d",
        "s1000d.css",
        include_str!("../builtin/s1000d/s1000d.css"),
    ),
];

/// Writes the built-in packages to `<config>/doctypes/_builtin` (refreshed on
/// every start; copy a package elsewhere to customise it).
fn install_builtin_packages() {
    let Some(base) = crate::settings::config_dir().map(|c| c.join("doctypes").join("_builtin"))
    else {
        return;
    };
    for (pkg, file, text) in BUILTIN {
        let dir = base.join(pkg);
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join(file);
        if std::fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            let _ = std::fs::write(path, text);
        }
    }
}

fn table_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
    for (label, c) in [
        ("Insert table…", Cmd::InsertTable),
        ("Insert row above", Cmd::TableRowAbove),
        ("Insert row below", Cmd::TableRowBelow),
        ("Insert column left", Cmd::TableColLeft),
        ("Insert column right", Cmd::TableColRight),
        ("Delete row", Cmd::TableDeleteRow),
        ("Delete column", Cmd::TableDeleteColumn),
        ("Move row up", Cmd::TableRowUp),
        ("Move row down", Cmd::TableRowDown),
        ("Merge cell right", Cmd::TableMergeRight),
        ("Merge cell down", Cmd::TableMergeDown),
        ("Split cell", Cmd::TableSplitCell),
        ("Delete table", Cmd::TableDelete),
    ] {
        if ui.button(label).clicked() {
            cmds.push(c);
            ui.close();
        }
    }
}

fn path_pref(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut Option<PathBuf>,
    folder: bool,
    changed: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.label(
            RichText::new(
                value
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "(not set)".into()),
            )
            .weak(),
        );
        if ui.small_button("Choose…").clicked() {
            let picked = if folder {
                rfd::FileDialog::new().pick_folder()
            } else {
                rfd::FileDialog::new().pick_file()
            };
            if let Some(p) = picked {
                *value = Some(p);
                *changed = true;
            }
        }
        if value.is_some() && ui.small_button("✕").clicked() {
            *value = None;
            *changed = true;
        }
    });
}

fn panels_current(ed: &xylem_edit::Editor) -> NodeId {
    let doc = &ed.doc;
    if !ed.sel.is_caret() {
        let (a, b) = ed.sel.ordered(doc);
        if a.node == b.node && b.offset == a.offset + 1 && doc.is_element(a.node) {
            let n = doc.children(a.node)[a.offset];
            if doc.is_element(n) {
                return n;
            }
        }
    }
    container(doc, ed.caret())
}

fn default_toggle(kind: &str, ds: &DocState) -> Option<String> {
    let candidates: &[&str] = match kind {
        "bold" => &["b", "bold", "strong", "emphasis"],
        "italic" => &["i", "italic", "em", "emphasis"],
        "underline" => &["u", "underline"],
        "superscript" => &["sup", "superScript", "superscript"],
        "subscript" => &["sub", "subScript", "subscript"],
        _ => &[],
    };
    match &ds.grammar {
        Some(g) => {
            let names = g.all_element_names();
            candidates
                .iter()
                .find(|c| names.iter().any(|n| n == *c))
                .map(|s| s.to_string())
        }
        None => candidates.first().map(|s| s.to_string()),
    }
}

fn ds_dir(p: &Option<PathBuf>) -> Option<&Path> {
    p.as_deref().and_then(Path::parent)
}

fn ds_resolve(dir: Option<&Path>, src: &str) -> Option<PathBuf> {
    let p = Path::new(src);
    if p.is_absolute() && p.exists() {
        return Some(p.to_path_buf());
    }
    let dir = dir?;
    let direct = dir.join(src);
    if direct.is_file() {
        return Some(direct);
    }
    for ext in ["png", "jpg", "jpeg", "gif", "bmp", "svg", "PNG", "JPG"] {
        let c = dir.join(format!("{src}.{ext}"));
        if c.is_file() {
            return Some(c);
        }
    }
    None
}

fn image_dims(p: &Path) -> Option<egui::Vec2> {
    let (w, h) = image::image_dimensions(p).ok()?;
    let scale = (480.0 / w as f32).min(1.0);
    Some(egui::vec2(w as f32 * scale, h as f32 * scale))
}

fn load_texture(ctx: &egui::Context, p: &Path) -> Option<egui::TextureHandle> {
    let img = image::open(p).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let ci = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(p.display().to_string(), ci, egui::TextureOptions::LINEAR))
}

fn parse_color(s: &str) -> Option<Color32> {
    let s = s.trim().trim_start_matches('#');
    if s.len() == 6 {
        let v = u32::from_str_radix(s, 16).ok()?;
        return Some(Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8));
    }
    None
}

fn word_at(t: &str, off: usize) -> Option<(usize, usize)> {
    let is_w = |c: char| c.is_alphabetic() || c == '\'';
    let off = off.min(t.len());
    let mut s = off;
    while s > 0 {
        let c = t[..s].chars().next_back()?;
        if !is_w(c) {
            break;
        }
        s -= c.len_utf8();
    }
    let mut e = off;
    while e < t.len() {
        let c = t[e..].chars().next()?;
        if !is_w(c) {
            break;
        }
        e += c.len_utf8();
    }
    (e > s).then_some((s, e))
}

impl eframe::App for XylemApp {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(s) = &mut self.script {
            s.feed(raw);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if self.script.is_some() || self.screenshot.is_some() {
            return; // test runs must not change the user's settings
        }
        eframe::set_value(storage, "settings", &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        for p in std::mem::take(&mut self.pending_open) {
            self.open_path(p);
        }
        self.debug_screenshot(ctx);
        // Drag & drop files.
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        for p in dropped {
            self.open_path(p);
        }
        // Confirm before closing with unsaved work.
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.docs.iter().any(|d| d.is_dirty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialogs.confirm_exit = true;
        }
        let mut cmds = self.shortcuts(ctx);
        let title = self
            .docs
            .get(self.active)
            .map(|d| format!("{} — Xylem", d.title()))
            .unwrap_or_else(|| "Xylem".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            self.menu_bar(ui, &mut cmds);
            self.toolbar(ui, &mut cmds);
        });
        self.status_bar(ctx);
        self.panels(ctx, &mut cmds);
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.docs.is_empty() {
                self.welcome(ui, &mut cmds);
                return;
            }
            self.tabs(ui, &mut cmds);
            ui.separator();
            self.document_area(ui, &mut cmds);
        });
        self.show_context_menu(ctx, &mut cmds);
        self.dialogs(ctx, &mut cmds);
        self.show_toasts(ctx);
        for c in cmds {
            self.run(ctx, c);
        }
        let ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        if ms > 25.0 {
            log::debug!("slow frame: {ms:.1} ms");
        }
        // Background validation (cheap: only when the document changed).
        if self.settings.auto_validate
            && let Some(ds) = self.docs.get_mut(self.active)
            && ds.view != View::Source
            && ds.validated_revision != Some(ds.editor.doc.revision())
        {
            ds.validate();
        }
        let _ = Issue {
            node: None,
            severity: Severity::Info,
            message: String::new(),
            source: "",
        };
    }
}
