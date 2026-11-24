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

mod export_ui;
mod find_files_ui;
mod preview_ui;
mod references_ui;
mod stylesheet_check_ui;

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
    FindInFiles,
    ExportHtml,
    PrintPreview,
    /// File > Publication preview: the publishing stylesheets on the document.
    PublicationPreview,
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
    /// File > Previous versions.
    PreviousVersions,
    /// Help > Open log folder.
    OpenLogFolder,
    /// Tools > Check stylesheet output (a script may name the suite file).
    CheckStylesheet(String),
    /// Tools > Stylesheet references: what the element at the caret refers to.
    StylesheetReferences,
    CheckUpdates,
    ReloadPackages,
    S1000dInfo,
    RenameToCsdb,
    FollowLink(NodeId),
    SpellCheck,
    /// Open the quick insert/surround palette at the caret.
    Palette,
    FindNext(bool),
    Shortcuts,
    /// Collapse (true) or expand (false) the current element in the view.
    Collapse(bool),
    /// Collapse every block child of the current element.
    CollapseChildren,
    ExpandAll,
    Replace(NodeId, std::ops::Range<usize>, String),
    AddToDictionary(String),
    /// Show the Project tab of the left panel.
    ShowProject,
    /// Choose the project (CSDB) folder and scan it.
    ProjectFolder,
    CheckReferences,
    /// View > Applicability.
    Applicability,
    /// Turn change tracking on or off for the active document.
    TrackChanges,
    /// Show or hide the Review panel.
    ReviewPanel,
    /// Add a comment at the caret or on the selection.
    NewComment,
    /// Accept (true) or reject the change at the caret.
    ResolveAtCaret(bool),
    /// Accept (true) or reject every change.
    ResolveAll(bool),
    /// Accept (true) or reject one change, by its marker.
    Resolve(NodeId, bool),
}

impl Cmd {
    /// The command a test script names (`cmd SaveAs`, `cmd InsertElement para`,
    /// `cmd View normal`). Scripts use these instead of clicking menus, so they
    /// keep working when the layout changes.
    pub fn from_script(name: &str, arg: &str) -> Option<Cmd> {
        let arg = arg.trim();
        Some(match name {
            "New" => Cmd::New,
            "NewS1000d" => Cmd::NewS1000d,
            "Open" => Cmd::Open,
            "Save" => Cmd::Save,
            "SaveAs" => Cmd::SaveAs,
            "Undo" => Cmd::Undo,
            "Redo" => Cmd::Redo,
            "Cut" => Cmd::Cut,
            "Copy" => Cmd::Copy,
            "Paste" => Cmd::Paste,
            "PasteText" => Cmd::PasteText,
            "SelectAll" => Cmd::SelectAll,
            "SelectElement" => Cmd::SelectElement,
            "Find" => Cmd::Find,
            "FindInFiles" => Cmd::FindInFiles,
            "ExportHtml" => Cmd::ExportHtml,
            "PrintPreview" => Cmd::PrintPreview,
            "PublicationPreview" => Cmd::PublicationPreview,
            "ZoomIn" => Cmd::ZoomIn,
            "ZoomOut" => Cmd::ZoomOut,
            "ZoomReset" => Cmd::ZoomReset,
            "Validate" => Cmd::Validate,
            "CheckBrex" => Cmd::CheckBrex,
            "RemoveTags" => Cmd::RemoveTags,
            "DeleteElement" => Cmd::DeleteElement,
            "SplitElement" => Cmd::SplitElement,
            "JoinPreceding" => Cmd::JoinPreceding,
            "EditAttributes" => Cmd::EditAttributes,
            "InsertComment" => Cmd::InsertComment,
            "InsertPi" => Cmd::InsertPi,
            "InsertCData" => Cmd::InsertCData,
            "InsertTable" => Cmd::InsertTable,
            "TableRowAbove" => Cmd::TableRowAbove,
            "TableRowBelow" => Cmd::TableRowBelow,
            "TableDeleteRow" => Cmd::TableDeleteRow,
            "TableColLeft" => Cmd::TableColLeft,
            "TableColRight" => Cmd::TableColRight,
            "TableDeleteColumn" => Cmd::TableDeleteColumn,
            "TableMergeRight" => Cmd::TableMergeRight,
            "TableMergeDown" => Cmd::TableMergeDown,
            "TableSplitCell" => Cmd::TableSplitCell,
            "TableDelete" => Cmd::TableDelete,
            "TableRowUp" => Cmd::TableRowUp,
            "TableRowDown" => Cmd::TableRowDown,
            "PrettyPrint" => Cmd::PrettyPrint,
            "ImportXmax" => Cmd::ImportXmax,
            "Preferences" => Cmd::Preferences,
            "About" => Cmd::About,
            "CheckUpdates" => Cmd::CheckUpdates,
            "ReloadPackages" => Cmd::ReloadPackages,
            "S1000dInfo" => Cmd::S1000dInfo,
            "RenameToCsdb" => Cmd::RenameToCsdb,
            "SpellCheck" => Cmd::SpellCheck,
            "Palette" => Cmd::Palette,
            "Shortcuts" => Cmd::Shortcuts,
            "CollapseChildren" => Cmd::CollapseChildren,
            "ExpandAll" => Cmd::ExpandAll,
            "ShowProject" => Cmd::ShowProject,
            "ProjectFolder" => Cmd::ProjectFolder,
            "CheckReferences" => Cmd::CheckReferences,
            "Applicability" => Cmd::Applicability,
            "TrackChanges" => Cmd::TrackChanges,
            "ReviewPanel" => Cmd::ReviewPanel,
            "NewComment" => Cmd::NewComment,
            "PreviousVersions" => Cmd::PreviousVersions,
            "OpenLogFolder" => Cmd::OpenLogFolder,
            "CheckStylesheet" => Cmd::CheckStylesheet(arg.to_string()),
            "StylesheetReferences" => Cmd::StylesheetReferences,
            "InsertElement" => Cmd::InsertElement(arg.to_string()),
            "Surround" => Cmd::Surround(arg.to_string()),
            "Toggle" => Cmd::Toggle(arg.to_string()),
            "View" => Cmd::View(match arg.to_ascii_lowercase().as_str() {
                "normal" => View::Normal,
                "source" => View::Source,
                _ => View::TagsOn,
            }),
            "FindNext" => Cmd::FindNext(arg != "back"),
            "Collapse" => Cmd::Collapse(arg != "expand"),
            "ResolveAtCaret" => Cmd::ResolveAtCaret(arg != "reject"),
            "ResolveAll" => Cmd::ResolveAll(arg != "reject"),
            _ => return None,
        })
    }
}

/// Which view the left side panel shows.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
enum LeftTab {
    #[default]
    Structure,
    Project,
}

#[derive(Default)]
struct Dialogs {
    about: bool,
    update: bool,
    prefs: bool,
    table: Option<TableDialog>,
    node_edit: Option<NodeEditDialog>,
    new_doc: Option<NewDocDialog>,
    new_dm: Option<crate::s1000d_ui::NewDmDialog>,
    info: bool,
    confirm_close: Option<usize>,
    confirm_exit: bool,
    import_result: Option<String>,
    comment: Option<CommentDialog>,
    /// Files someone else has open, waiting for the user's choice.
    locked: Vec<(PathBuf, crate::files::LockOwner)>,
    disk_changed: Option<DiskChanged>,
    versions: bool,
    /// A crash report from the last session, announced once.
    crash_report: Option<PathBuf>,
}

/// What a background disk check reports: each open file and its stamp now.
type DiskStamps = Vec<(PathBuf, Option<crate::files::Stamp>)>;

/// A file that changed on disk while it had unsaved changes here.
#[derive(Clone)]
struct DiskChanged {
    path: PathBuf,
    /// Raised by Save, which then continues if the user keeps their version.
    saving: bool,
}

/// New comment (no node yet) or an existing one being edited.
struct CommentDialog {
    node: Option<NodeId>,
    text: String,
    focus: bool,
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
    /// Shared team defaults (see `team.rs`); never written back.
    team: crate::team::TeamConfig,
    registry: DoctypeRegistry,
    catalog: xylem_schema::Catalog,
    docs: Vec<DocState>,
    active: usize,
    next_id: u64,
    clipboard: XmlClipboard,
    fonts: FontSet,
    panel: PanelState,
    find: FindState,
    find_files: find_files_ui::FindFilesState,
    stylesheet_check: stylesheet_check_ui::StylesheetCheckState,
    preview: preview_ui::PreviewState,
    references: references_ui::ReferencesState,
    dialogs: Dialogs,
    toasts: Vec<(String, f64, bool)>,
    context_menu: Option<ContextMenu>,
    tag_colors: HashMap<String, Color32>,
    speller: Option<Speller>,
    pending_open: Vec<PathBuf>,
    allow_close: bool,
    /// Debug hook: `XYLEM_SCREENSHOT=<png>` captures the window and exits.
    screenshot: Option<(PathBuf, u32)>,
    ctx: egui::Context,
    palette: Option<Palette>,
    /// Messages already shown this session (schema warnings repeat per file).
    shown_once: std::collections::HashSet<String>,
    shortcuts_open: bool,
    last_autosave: f64,
    /// Autosaved documents found at startup, offered for recovery.
    recovery: Vec<crate::autosave::Recovered>,
    /// Lock files for the open documents (see `files.rs`), by path. `None`
    /// where the folder could not take one.
    locks: HashMap<PathBuf, Option<crate::files::Lock>>,
    disk_poll: Option<std::sync::mpsc::Receiver<DiskStamps>>,
    last_disk_poll: f64,
    last_script_check: f64,
    update: UpdateState,
    script: Option<crate::script::Script>,
    shot_path: Option<PathBuf>,
    left_tab: LeftTab,
    project: crate::project_ui::ProjectView,
    applic_dialog: crate::s1000d_ui::ApplicDialog,
    review: crate::review::ReviewState,
}

impl XylemApp {
    pub fn new(cc: &eframe::CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        if std::env::var_os("XYLEM_SCRIPT").is_some() {
            // Screenshots must not catch menus and windows half faded in.
            cc.egui_ctx.all_styles_mut(|s| s.animation_time = 0.0);
        }
        let mut settings: Settings = cc
            .storage
            .and_then(|s| eframe::get_value(s, "settings"))
            .unwrap_or_default();
        if let Some(root) = std::env::var_os("XYLEM_S1000D_SCHEMAS") {
            settings.s1000d_schema_root = Some(PathBuf::from(root));
        }
        if let Some(dict) = std::env::var_os("XYLEM_DICTIONARY") {
            settings.dictionary_dir = Some(PathBuf::from(dict));
            settings.spell_check = true;
        }
        if let Some(secs) = std::env::var("XYLEM_AUTOSAVE_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
        {
            settings.autosave_seconds = secs;
        }
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
        // No saved choice: follow the system theme.
        // XYLEM_THEME=light|dark overrides it (screenshots and tests).
        let dark = match std::env::var("XYLEM_THEME").as_deref() {
            Ok("light") => false,
            Ok("dark") => true,
            _ => settings
                .dark_mode
                .unwrap_or_else(|| cc.egui_ctx.style().visuals.dark_mode),
        };
        crate::ui::theme::apply(&cc.egui_ctx, dark);
        install_builtin_packages();
        let team = crate::team::load();
        let mut app = Self {
            registry: DoctypeRegistry::scan(&all_package_dirs(&team, &settings)),
            catalog: Self::load_catalog(&settings, &team),
            settings,
            team,
            docs: Vec::new(),
            active: 0,
            next_id: 1,
            clipboard: XmlClipboard::default(),
            fonts,
            panel: PanelState::default(),
            find: FindState::default(),
            find_files: Default::default(),
            stylesheet_check: Default::default(),
            preview: Default::default(),
            references: Default::default(),
            dialogs: Dialogs::default(),
            toasts: Vec::new(),
            context_menu: None,
            tag_colors: HashMap::new(),
            speller: None,
            pending_open: files,
            allow_close: false,
            screenshot: std::env::var_os("XYLEM_SCREENSHOT").map(|p| (PathBuf::from(p), 0)),
            ctx: cc.egui_ctx.clone(),
            palette: None,
            shown_once: Default::default(),
            shortcuts_open: false,
            last_autosave: 0.0,
            recovery: Vec::new(),
            locks: HashMap::new(),
            disk_poll: None,
            last_disk_poll: 0.0,
            last_script_check: 0.0,
            update: UpdateState::Idle,
            script: crate::script::Script::from_env(),
            shot_path: None,
            left_tab: LeftTab::default(),
            project: Default::default(),
            applic_dialog: Default::default(),
            review: Default::default(),
        };
        app.load_speller();
        let scripted = app.script.is_some() || app.screenshot.is_some();
        let isolated = std::env::var_os("XYLEM_CONFIG_DIR").is_some();
        if !scripted || isolated {
            app.recovery = crate::autosave::find();
        }
        if (!scripted || isolated) && app.pending_open.is_empty() && app.settings.restore_session {
            app.pending_open = app
                .settings
                .last_session
                .iter()
                .filter(|p| p.is_file())
                .cloned()
                .collect();
        }
        if !scripted && app.team.update_source.is_some() {
            app.start_update_check(true);
        }
        app.dialogs.crash_report = crate::logging::take_unseen_crash_report();
        if !app.team.warnings.is_empty() {
            app.toast(
                "Team configuration has problems; see File › Preferences.",
                true,
            );
        }
        app
    }

    fn load_catalog(settings: &Settings, team: &crate::team::TeamConfig) -> xylem_schema::Catalog {
        let mut cat = settings
            .catalog
            .as_ref()
            .or(team.catalog.as_ref())
            .and_then(|p| xylem_schema::Catalog::load(p).ok())
            .unwrap_or_default();
        // Without a configured folder, use `<config>/schemas/s1000d` if present.
        let fallback = crate::settings::config_dir()
            .map(|c| c.join("schemas").join("s1000d"))
            .filter(|p| p.is_dir());
        if let Some(root) = settings
            .s1000d_schema_root
            .clone()
            .or_else(|| team.s1000d_schemas.clone())
            .or(fallback)
        {
            crate::s1000d_ui::add_s1000d_catalog(&mut cat, &root);
        }
        // Schema locations: the team's, then the user's (which win when
        // both map the same location).
        for (url, local) in &team.schema_locations {
            cat.add_prefix(url, local);
        }
        for (url, local) in &settings.schema_locations {
            if !url.trim().is_empty() {
                cat.add_prefix(url.trim(), local);
            }
        }
        cat
    }

    fn load_speller(&mut self) {
        self.speller = if self.settings.spell_check {
            Speller::load(
                self.settings
                    .dictionary_dir
                    .as_deref()
                    .or(self.team.dictionary_dir.as_deref()),
                self.team
                    .dictionary_lang
                    .as_deref()
                    .filter(|_| {
                        self.settings.dictionary_lang == Settings::default().dictionary_lang
                    })
                    .unwrap_or(&self.settings.dictionary_lang),
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
        if let Some(owner) = crate::files::held_by_someone_else(&path) {
            if !self.dialogs.locked.iter().any(|(p, _)| *p == path) {
                self.dialogs.locked.push((path, owner));
            }
            return;
        }
        self.open_path_now(path, false);
    }

    /// Opens `path` without the lock check; `read_only` for a copy that
    /// must not be saved over someone else's work.
    fn open_path_now(&mut self, path: PathBuf, read_only: bool) {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => return self.toast(format!("Cannot open {}: {e}", path.display()), true),
        };
        match Document::from_bytes(&bytes) {
            Ok(doc) => {
                self.add_doc(doc, Some(path.clone()));
                if let Some(ds) = self.docs.last_mut() {
                    ds.read_only = read_only;
                }
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
        ds.disk = ds.path.as_deref().and_then(crate::files::stamp);
        ds.repaint = Some(self.ctx.clone());
        ds.view = self.settings.default_view;
        ds.load_styles();
        ds.load_grammar(&self.catalog, &self.team.schema);
        for w in ds.style_warnings.iter().take(3) {
            log::warn!("{w}");
        }
        if (ds.grammar_status.contains("failed") || ds.grammar_status.contains("not available"))
            && self.shown_once.insert(ds.grammar_status.clone())
        {
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
        // Scripts answer with one path; people can pick several.
        let mut picked_many = None;
        let first = crate::script::file_dialog(|| {
            let paths = dlg.pick_files()?;
            let first = paths.first().cloned();
            picked_many = Some(paths);
            first
        });
        if let Some(paths) = picked_many.or_else(|| first.map(|p| vec![p])) {
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
                    ds.load_grammar(&self.catalog, &self.team.schema);
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
        if ds.read_only && !save_as {
            return self.toast(
                "This copy is read-only. Use Save as to keep it under another name.",
                true,
            );
        }
        // Someone else changed the file since it was opened: ask first.
        if !save_as && let Some(p) = ds.path.clone() {
            let now = crate::files::stamp(&p);
            if now.is_some() && now != ds.disk && now != ds.disk_ignored {
                self.dialogs.disk_changed = Some(DiskChanged {
                    path: p,
                    saving: true,
                });
                return;
            }
        }
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
            match crate::script::file_dialog(|| d.save_file()) {
                Some(p) => p,
                None => return,
            }
        } else {
            ds.path.clone().unwrap()
        };
        let bytes = ds.editor.doc.to_bytes();
        crate::files::back_up(&path);
        let tmp = path.with_extension("xylem-tmp");
        let result = std::fs::write(&tmp, &bytes).and_then(|_| std::fs::rename(&tmp, &path));
        match result {
            Ok(()) => {
                ds.editor.mark_saved();
                crate::autosave::discard(ds.id_key());
                ds.forget_recovery();
                ds.path = Some(path.clone());
                ds.disk = crate::files::stamp(&path);
                ds.disk_ignored = None;
                ds.missing_warned = false;
                ds.title_override = None;
                ds.read_only = false;
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

    /// Inserts prepared markup for `name` (table, image…) with generated IDs.
    fn insert_markup(&mut self, name: &str, xml: &str) -> Option<Result<(), EditError>> {
        let package = self.docs.get(self.active)?.package.clone();
        self.with_editor_result(|ed, r| {
            let xml = crate::doc::with_generated_ids(&package, &ed.doc, xml);
            ed.insert_element_xml(r, name, &xml).map(|_| ())
        })
    }

    fn with_editor_result<T>(
        &mut self,
        f: impl FnOnce(&mut xylem_edit::Editor, &dyn Rules) -> Result<T, EditError>,
    ) -> Option<Result<T, EditError>> {
        let ds = self.docs.get_mut(self.active)?;
        if ds.read_only || ds.view == View::Source {
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
        Some(r)
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
            Cmd::Find => {
                self.find.open = true;
                self.find.focus_query = true;
            }
            Cmd::FindInFiles => self.open_find_in_files(),
            Cmd::ExportHtml => self.export_html(),
            Cmd::PrintPreview => self.print_preview(),
            Cmd::PublicationPreview => self.publication_preview(),
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
            Cmd::ShowProject => self.show_project(ctx),
            Cmd::ProjectFolder => self.choose_project_folder(ctx),
            Cmd::CheckReferences => {
                self.show_project(ctx);
                if self.project.project.is_some() && !self.project.is_scanning() {
                    self.project.check_references();
                } else {
                    self.toast("Choose a project folder first.", false);
                }
            }
            Cmd::Applicability => self.applic_dialog.open = true,
            Cmd::InsertElement(name) => {
                self.with_editor(|ed, r| ed.insert_element(r, &name));
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
                let Some(ds) = self.docs.get(self.active) else {
                    return;
                };
                let pp = ds.package.pretty_print.clone();
                let indent = pp.indent_amount.unwrap_or(2) as usize;
                // Elements that the customization lists without "indent content".
                let flat = move |name: &str| {
                    pp.elements.get(name).is_some_and(|e| {
                        !e.indent_content
                            && (e.newline_before_start_tag || e.newline_before_end_tag)
                    })
                };
                let opts = xylem_edit::format::FormatOptions {
                    indent,
                    flat: &flat,
                };
                if self.with_editor(|ed, r| ed.reformat(r, &opts)).is_some() {
                    self.toast("Formatted the document (one undo step).", false);
                }
            }
            Cmd::ImportXmax => self.import_xmax(),
            Cmd::Preferences => self.dialogs.prefs = true,
            Cmd::TrackChanges => {
                let author = self.settings.author();
                if let Some(ds) = self.doc() {
                    ds.editor.tracking = match ds.editor.tracking {
                        Some(_) => None,
                        None => Some(xylem_edit::revision::Tracking {
                            author,
                            clock: crate::review::now,
                        }),
                    };
                    let on = ds.editor.is_tracking();
                    ds.editor.break_undo_group = true;
                    self.toast(
                        if on {
                            "Tracking changes."
                        } else {
                            "Stopped tracking changes."
                        },
                        false,
                    );
                }
            }
            Cmd::ReviewPanel => self.review.open = !self.review.open,
            Cmd::NewComment => {
                if self
                    .docs
                    .get(self.active)
                    .is_some_and(|d| d.view != View::Source)
                {
                    self.dialogs.comment = Some(CommentDialog {
                        node: None,
                        text: String::new(),
                        focus: true,
                    });
                }
            }
            Cmd::ResolveAtCaret(accept) => {
                let change = self
                    .docs
                    .get(self.active)
                    .and_then(|d| d.editor.change_at(d.editor.caret()));
                match change {
                    Some(c) if c.kind != xylem_edit::revision::ChangeKind::Comment => {
                        self.run(ctx, Cmd::Resolve(c.node, accept));
                    }
                    _ => self.toast("There is no tracked change at the caret.", false),
                }
            }
            Cmd::Resolve(node, accept) => {
                self.with_editor(|ed, r| {
                    if accept {
                        ed.accept_change(r, node)
                    } else {
                        ed.reject_change(r, node)
                    }
                });
            }
            Cmd::ResolveAll(accept) => {
                if self
                    .with_editor_result(|ed, r| ed.resolve_all(r, accept))
                    .is_some_and(|r| r == Err(EditError::Nothing))
                {
                    self.toast("There are no tracked changes.", false);
                }
            }
            Cmd::About => self.dialogs.about = true,
            Cmd::PreviousVersions => self.dialogs.versions = true,
            Cmd::CheckStylesheet(suite) => self.open_stylesheet_check(Some(suite)),
            Cmd::StylesheetReferences => self.show_stylesheet_references(),
            Cmd::OpenLogFolder => {
                if let Some(dir) = crate::logging::log_dir() {
                    let _ = std::fs::create_dir_all(&dir);
                    if let Err(e) = crate::html_export::open_in_browser(&dir) {
                        self.toast(format!("Could not open {}: {e}", dir.display()), true);
                    }
                }
            }
            Cmd::CheckUpdates => {
                self.dialogs.update = true;
                self.start_update_check(false);
            }
            Cmd::ReloadPackages => {
                self.registry =
                    DoctypeRegistry::scan(&all_package_dirs(&self.team, &self.settings));
                self.catalog = Self::load_catalog(&self.settings, &self.team);
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
            Cmd::Collapse(on) => {
                if let Some(ds) = self.docs.get_mut(self.active) {
                    let el = panels_current(&ds.editor);
                    if Some(el) != ds.editor.doc.document_element() || !on {
                        ds.canvas.set_collapsed(el, on);
                        if on {
                            // Keep the caret outside the hidden content.
                            let doc = &ds.editor.doc;
                            if let (Some(parent), Some(i)) =
                                (doc.parent(el), doc.index_in_parent(el))
                            {
                                ds.editor.set_caret(Pos::new(parent, i));
                            }
                        }
                    }
                }
            }
            Cmd::CollapseChildren => {
                if let Some(ds) = self.docs.get_mut(self.active) {
                    let el = panels_current(&ds.editor);
                    let kids: Vec<NodeId> = ds
                        .editor
                        .doc
                        .element_children(el)
                        .filter(|&c| ds.editor.doc.element_children(c).next().is_some())
                        .collect();
                    for k in kids {
                        ds.canvas.set_collapsed(k, true);
                    }
                }
            }
            Cmd::ExpandAll => {
                if let Some(ds) = self.docs.get_mut(self.active) {
                    ds.canvas.expand_all();
                }
            }
            Cmd::Palette => {
                if let Some(ds) = self.docs.get(self.active)
                    && ds.view != View::Source
                {
                    self.palette = Some(Palette {
                        query: String::new(),
                        selected: 0,
                        surround: !ds.editor.sel.is_caret(),
                    });
                }
            }
            Cmd::FindNext(forward) => {
                if !self.find.has_query() {
                    self.find.open = true;
                    return;
                }
                let Some(ds) = self.docs.get(self.active) else {
                    return;
                };
                if let Some(crate::find::FindAction::Goto(node, range)) =
                    self.find.step(ds, forward)
                {
                    let ds = self.docs.get_mut(self.active).unwrap();
                    ds.editor.sel = Selection {
                        anchor: Pos::new(node, range.start),
                        focus: Pos::new(node, range.end),
                    };
                    ds.canvas.scroll_to_caret = true;
                    ds.canvas.request_focus = true;
                }
            }
            Cmd::Shortcuts => self.shortcuts_open = true,
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

    /// Inserts a dropped image file as the package's image element at the
    /// caret. Returns false when the document has no image element configured.
    fn insert_image_file(&mut self, file: &Path) -> bool {
        let file = &std::path::absolute(file).unwrap_or_else(|_| file.to_path_buf());
        let Some(ds) = self.docs.get(self.active) else {
            return false;
        };
        if ds.view == View::Source || ds.package.behaviour.images.is_empty() {
            return false;
        }
        let images = ds.package.behaviour.images.clone();
        let dir = ds.dir().map(Path::to_path_buf);
        // S1000D refers to graphics by ICN (the file name without extension);
        // other markup gets a path relative to the document when possible.
        let reference = |attr: &str| -> String {
            if attr == "infoEntityIdent" || attr == "boardno" {
                return file
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
            match dir.as_deref().and_then(|d| file.strip_prefix(d).ok()) {
                Some(rel) => rel.to_string_lossy().replace('\\', "/"),
                None => file.to_string_lossy().replace('\\', "/"),
            }
        };
        let mut last_err = None;
        for img in images {
            let mut xml = format!("<{} {}=\"", img.element, img.source_attr);
            xylem_dom::escape_attr(&reference(&img.source_attr), &mut xml);
            xml.push_str("\"/>");
            let name = img.element.clone();
            match self.insert_markup(&name, &xml) {
                Some(Ok(())) => {
                    return true;
                }
                Some(Err(e)) => last_err = Some(e),
                None => return false,
            }
        }
        // Not allowed directly: wrap it in an allowed container whose template
        // or required content holds the image element (e.g. a figure).
        let ds = self.docs.get(self.active).unwrap();
        for img in &ds.package.behaviour.images {
            if let Some((name, xml)) = image_container_xml(
                ds,
                &img.element,
                &img.source_attr,
                &reference(&img.source_attr),
            ) {
                match self.insert_markup(&name, &xml) {
                    Some(Ok(())) => {
                        return true;
                    }
                    Some(Err(e)) => last_err = Some(e),
                    None => return false,
                }
                break;
            }
        }
        if let Some(e) = last_err {
            self.toast(format!("Can't insert the image here: {e}"), true);
        }
        true
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
        ds.repaint = Some(self.ctx.clone());
        ds.view = self.settings.default_view;
        ds.load_styles();
        ds.load_grammar(&self.catalog, &self.team.schema);
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
        // One dialog for both forms: the .ctm inside a customization folder
        // stands for the folder, a .xac archive for itself.
        let Some(picked) = crate::script::file_dialog(|| {
            rfd::FileDialog::new()
                .set_title("Choose an XMAX customization (.ctm in its folder, or a .xac)")
                .add_filter("XMAX customization", &["ctm", "CTM", "xac", "XAC"])
                .pick_file()
        }) else {
            return;
        };
        let is_ctm = picked
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ctm"));
        let src = match picked.parent() {
            Some(dir) if is_ctm => dir.to_path_buf(),
            _ => picked,
        };
        let Some(dest_root) = self
            .settings
            .package_dirs
            .first()
            .cloned()
            .or_else(|| crate::settings::config_dir().map(|d| d.join("doctypes")))
        else {
            self.toast(
                "No doctype package folder is set; add one in File › Preferences.",
                true,
            );
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
        self.registry = DoctypeRegistry::scan(&all_package_dirs(&self.team, &self.settings));
    }

    fn check_brex(&mut self) {
        let path = match self
            .settings
            .brex
            .clone()
            .or_else(|| self.team.brex.clone())
            .filter(|p| p.exists())
        {
            Some(p) => p,
            None => match crate::script::file_dialog(|| {
                rfd::FileDialog::new()
                    .set_title("Choose a BREX data module")
                    .add_filter("XML", &["xml", "XML"])
                    .pick_file()
            }) {
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
        let Some(href) = doc.attr(el, "href").or_else(|| doc.attr(el, "xlink:href")) else {
            return;
        };
        let (file, fragment) = match href.split_once('#') {
            Some((f, frag)) => (f.to_string(), Some(frag.to_string())),
            None => (href.to_string(), None),
        };
        if file.contains("://") {
            self.toast(format!("{href} is not a local file."), false);
            return;
        }
        let Some(dir) = ds.dir().map(Path::to_path_buf) else {
            self.toast(
                "Save the document first so relative links can be resolved.",
                false,
            );
            return;
        };
        if file.is_empty() {
            // "#id" alone points inside this document.
            if let Some(id) = fragment {
                self.reveal_id(&id);
            }
            return;
        }
        let p = dir.join(&file);
        if !p.is_file() {
            self.toast(format!("Link target not found: {}", p.display()), true);
            return;
        }
        self.open_path(p);
        if let Some(id) = fragment {
            self.reveal_id(&id);
        }
    }

    // ------------------------------------------------------------ project

    /// Shows the Project tab, scanning the remembered folder the first time.
    fn show_project(&mut self, ctx: &egui::Context) {
        self.settings.show_structure = true;
        self.left_tab = LeftTab::Project;
        if self.project.project.is_none()
            && !self.project.is_scanning()
            && let Some(dir) = self.settings.project_dir.clone().filter(|d| d.is_dir())
        {
            self.project.start_scan(dir, ctx);
        }
    }

    fn choose_project_folder(&mut self, ctx: &egui::Context) {
        let start = self.settings.project_dir.clone().or_else(|| {
            self.docs
                .get(self.active)
                .and_then(|d| d.dir().map(Path::to_path_buf))
        });
        let Some(dir) = crate::script::file_dialog(|| {
            let mut d = rfd::FileDialog::new().set_title("Choose a project (CSDB) folder");
            if let Some(s) = &start {
                d = d.set_directory(s);
            }
            d.pick_folder()
        }) else {
            return;
        };
        self.settings.project_dir = Some(dir.clone());
        self.settings.show_structure = true;
        self.left_tab = LeftTab::Project;
        self.project.start_scan(dir, ctx);
    }

    fn project_action(&mut self, ctx: &egui::Context, a: crate::project_ui::ProjectAction) {
        use crate::project_ui::ProjectAction;
        match a {
            ProjectAction::ChooseFolder => self.choose_project_folder(ctx),
            ProjectAction::Open(p) => self.open_path(p),
            ProjectAction::OpenAt(p, reference) => {
                let target = std::path::absolute(&p).unwrap_or(p.clone());
                self.open_path(p);
                if self.docs.get(self.active).map(|d| d.path.as_deref()) != Some(Some(&target)) {
                    return;
                }
                // Leaving the source view applies its pending edits first;
                // when the text does not parse, the view stays as it is.
                if self.docs[self.active].view == View::Source {
                    self.set_view(View::TagsOn);
                }
                let ds = &mut self.docs[self.active];
                if ds.view == View::Source {
                    return;
                }
                match reference.locate(&ds.editor.doc) {
                    Some(n) => {
                        ds.editor.select_element(n);
                        ds.canvas.reveal(n);
                        ds.canvas.request_focus = true;
                    }
                    None => self.toast(
                        "The reference is no longer where the check found it; check again.",
                        false,
                    ),
                }
            }
            ProjectAction::InsertRef(i) => {
                let Some(m) = self
                    .project
                    .project
                    .as_ref()
                    .and_then(|p| p.modules.get(i))
                    .cloned()
                else {
                    return;
                };
                let name = m.reference_element();
                match self.insert_markup(name, &m.reference_xml()) {
                    Some(Ok(())) => {
                        self.toast(format!("Inserted a reference to {}.", m.code), false)
                    }
                    Some(Err(e)) => self.toast(format!("Cannot insert {name} here: {e}"), true),
                    None => {}
                }
            }
        }
    }

    /// Puts the caret at, or selects, the first occurrence of `text` in the
    /// active document's text (scripted tests).
    fn script_caret(&mut self, text: &str, at: crate::script::CaretAt) {
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let doc = &ds.editor.doc;
        let hit = doc.descendants(doc.root()).find_map(|n| {
            let t = doc.text(n)?;
            t.find(text).map(|i| (n, i, i + text.len()))
        });
        let Some((node, start, end)) = hit else {
            log::warn!("script: `{text}` not found in the document");
            return;
        };
        use crate::script::CaretAt;
        ds.editor.sel = match at {
            CaretAt::Start => Selection::caret(Pos::new(node, start)),
            CaretAt::End => Selection::caret(Pos::new(node, end)),
            CaretAt::Select => Selection {
                anchor: Pos::new(node, start),
                focus: Pos::new(node, end),
            },
        };
        ds.canvas.scroll_to_caret = true;
    }

    /// Puts the caret in the element with this `id` in the active document.
    fn reveal_id(&mut self, id: &str) {
        let Some(ds) = self.docs.get_mut(self.active) else {
            return;
        };
        let doc = &ds.editor.doc;
        let Some(t) = doc
            .descendants(doc.root())
            .find(|&x| doc.attr(x, "id") == Some(id))
        else {
            return;
        };
        let p = first_editable(doc, &xylem_edit::NoRules, t);
        ds.editor.set_caret(p);
        ds.canvas.reveal(t);
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
            if i.consume_shortcut(&sh(Key::Comma)) {
                cmds.push(Cmd::Preferences);
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
            // Before Ctrl+F, which would otherwise match with Shift held too.
            if i.consume_shortcut(&shs(Key::F)) {
                cmds.push(Cmd::FindInFiles);
            }
            if i.consume_shortcut(&sh(Key::F)) || i.consume_shortcut(&sh(Key::H)) {
                cmds.push(Cmd::Find);
            }
            if i.consume_shortcut(&sh(Key::W)) {
                cmds.push(Cmd::Close(self.active));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::SHIFT, Key::F3)) {
                cmds.push(Cmd::FindNext(false));
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F3)) {
                cmds.push(Cmd::FindNext(true));
            }
            if i.consume_shortcut(&sh(Key::Space)) {
                cmds.push(Cmd::Palette);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F1)) {
                cmds.push(Cmd::Shortcuts);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F7)) {
                cmds.push(Cmd::Validate);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(Modifiers::NONE, Key::F12)) {
                cmds.push(Cmd::StylesheetReferences);
            }
            if i.consume_shortcut(&KeyboardShortcut::new(
                Modifiers::COMMAND | Modifiers::SHIFT,
                Key::F5,
            )) {
                cmds.push(Cmd::PublicationPreview);
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
            // Shift variants first: egui's matching ignores an extra Shift.
            if i.consume_shortcut(&shs(Key::Minus)) {
                cmds.push(Cmd::Collapse(true));
            }
            if i.consume_shortcut(&shs(Key::Equals)) || i.consume_shortcut(&shs(Key::Plus)) {
                cmds.push(Cmd::Collapse(false));
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
            if i.consume_shortcut(&shs(Key::T)) {
                cmds.push(Cmd::TrackChanges);
            }
            if i.consume_shortcut(&shs(Key::M)) {
                cmds.push(Cmd::NewComment);
            }
            if i.consume_shortcut(&shs(Key::A)) {
                cmds.push(Cmd::ResolveAll(true));
            }
            if i.consume_shortcut(&shs(Key::R)) {
                cmds.push(Cmd::ResolveAll(false));
            }
        });
        cmds
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        use crate::ui::icon;
        let has_doc = !self.docs.is_empty();
        // One line per entry: (enabled, icon, label, shortcut, command).
        let item = |ui: &mut egui::Ui,
                    enabled: bool,
                    glyph: Option<&str>,
                    label: &str,
                    shortcut: Option<&str>|
         -> bool {
            ui.add_enabled_ui(enabled, |ui| {
                crate::ui::menu_item(ui, glyph, label, shortcut)
            })
            .inner
            .clicked()
        };
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if item(ui, true, Some(icon::FILE_PLUS), "New…", Some("Ctrl+N")) {
                    cmds.push(Cmd::New);
                }
                if item(ui, true, Some(icon::CUBE), "New S1000D module…", None) {
                    cmds.push(Cmd::NewS1000d);
                }
                if item(ui, true, Some(icon::FOLDER_OPEN), "Open…", Some("Ctrl+O")) {
                    cmds.push(Cmd::Open);
                }
                ui.menu_button(
                    crate::ui::icon_text(icon::CLOCK_COUNTER_CLOCKWISE, "Open recent"),
                    |ui| {
                        if self.settings.recent_files.is_empty() {
                            crate::ui::muted(ui, "No recent files");
                        }
                        for p in self.settings.recent_files.clone() {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            let dir = p
                                .parent()
                                .map(|d| d.display().to_string())
                                .unwrap_or_default();
                            if crate::ui::menu_item(ui, None, &name, None)
                                .on_hover_text(dir)
                                .clicked()
                            {
                                cmds.push(Cmd::OpenPath(p));
                            }
                        }
                    },
                );
                ui.separator();
                if item(ui, has_doc, Some(icon::FLOPPY_DISK), "Save", Some("Ctrl+S")) {
                    cmds.push(Cmd::Save);
                }
                if item(ui, has_doc, None, "Save as…", Some("Ctrl+Shift+S")) {
                    cmds.push(Cmd::SaveAs);
                }
                if item(ui, has_doc, None, "Close", Some("Ctrl+W")) {
                    cmds.push(Cmd::Close(self.active));
                }
                let has_file = self.docs.get(self.active).is_some_and(|d| d.path.is_some());
                if item(
                    ui,
                    has_file,
                    Some(icon::CLOCK_COUNTER_CLOCKWISE),
                    "Previous versions…",
                    None,
                ) {
                    cmds.push(Cmd::PreviousVersions);
                }
                ui.separator();
                if item(ui, has_doc, Some(icon::FILE_HTML), "Export to HTML…", None) {
                    cmds.push(Cmd::ExportHtml);
                }
                if item(ui, has_doc, Some(icon::PRINTER), "Print preview", None) {
                    cmds.push(Cmd::PrintPreview);
                }
                if item(
                    ui,
                    has_doc,
                    Some(icon::BOOK_OPEN),
                    "Publication preview",
                    Some("Ctrl+Shift+F5"),
                ) {
                    cmds.push(Cmd::PublicationPreview);
                }
                ui.separator();
                if item(ui, true, None, "Import XMAX customization…", None) {
                    cmds.push(Cmd::ImportXmax);
                }
                if item(
                    ui,
                    true,
                    Some(icon::ARROWS_CLOCKWISE),
                    "Reload doctype packages",
                    None,
                ) {
                    cmds.push(Cmd::ReloadPackages);
                }
                if item(ui, true, Some(icon::GEAR), "Preferences…", Some("Ctrl+,")) {
                    cmds.push(Cmd::Preferences);
                }
                ui.separator();
                if item(ui, true, None, "Exit", Some("Alt+F4")) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let entries: [(Option<&str>, &str, Option<&str>, Cmd); 13] = [
                    (
                        Some(icon::ARROW_U_UP_LEFT),
                        "Undo",
                        Some("Ctrl+Z"),
                        Cmd::Undo,
                    ),
                    (
                        Some(icon::ARROW_U_UP_RIGHT),
                        "Redo",
                        Some("Ctrl+Y"),
                        Cmd::Redo,
                    ),
                    (None, "Cut", Some("Ctrl+X"), Cmd::Cut),
                    (None, "Copy", Some("Ctrl+C"), Cmd::Copy),
                    (None, "Paste", Some("Ctrl+V"), Cmd::Paste),
                    (None, "Paste as text", Some("Ctrl+Shift+V"), Cmd::PasteText),
                    (None, "Select all", Some("Ctrl+A"), Cmd::SelectAll),
                    (None, "Select element", Some("Ctrl+E"), Cmd::SelectElement),
                    (
                        Some(icon::MAGNIFYING_GLASS),
                        "Find and replace…",
                        Some("Ctrl+F"),
                        Cmd::Find,
                    ),
                    (None, "Find next", Some("F3"), Cmd::FindNext(true)),
                    (
                        None,
                        "Find previous",
                        Some("Shift+F3"),
                        Cmd::FindNext(false),
                    ),
                    (
                        None,
                        "Insert or surround…",
                        Some("Ctrl+Space"),
                        Cmd::Palette,
                    ),
                    (None, "", None, Cmd::Palette),
                ];
                for (i, (glyph, label, sc, c)) in entries.into_iter().enumerate() {
                    if label.is_empty() {
                        continue;
                    }
                    if matches!(i, 2 | 6 | 8 | 11) {
                        ui.separator();
                    }
                    if item(ui, has_doc, glyph, label, sc) {
                        cmds.push(c);
                    }
                }
                ui.separator();
                // Works on files, so it needs no open document.
                if item(
                    ui,
                    true,
                    Some(icon::FILE_MAGNIFYING_GLASS),
                    "Find in files…",
                    Some("Ctrl+Shift+F"),
                ) {
                    cmds.push(Cmd::FindInFiles);
                }
            });
            ui.menu_button("Insert", |ui| {
                if item(
                    ui,
                    has_doc,
                    Some(icon::BRACKETS_ANGLE),
                    "Element…",
                    Some("Ctrl+Space"),
                ) {
                    cmds.push(Cmd::Palette);
                }
                if item(ui, has_doc, Some(icon::TABLE), "Table…", None) {
                    cmds.push(Cmd::InsertTable);
                }
                ui.separator();
                for (label, c) in [
                    ("Comment", Cmd::InsertComment),
                    ("Processing instruction", Cmd::InsertPi),
                    ("CDATA section", Cmd::InsertCData),
                ] {
                    if item(ui, has_doc, None, label, None) {
                        cmds.push(c);
                    }
                }
            });
            ui.menu_button("Format", |ui| {
                for (glyph, label, sc, kind) in [
                    (icon::TEXT_B, "Bold", Some("Ctrl+B"), "bold"),
                    (icon::TEXT_ITALIC, "Italic", Some("Ctrl+I"), "italic"),
                    (
                        icon::TEXT_UNDERLINE,
                        "Underline",
                        Some("Ctrl+U"),
                        "underline",
                    ),
                    (icon::TEXT_SUPERSCRIPT, "Superscript", None, "superscript"),
                    (icon::TEXT_SUBSCRIPT, "Subscript", None, "subscript"),
                ] {
                    if item(ui, has_doc, Some(glyph), label, sc) {
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
                    if item(ui, has_doc, None, label, None) {
                        cmds.push(c);
                    }
                }
                ui.separator();
                if item(
                    ui,
                    has_doc,
                    Some(icon::TEXT_INDENT),
                    "Format document",
                    None,
                ) {
                    cmds.push(Cmd::PrettyPrint);
                }
            });
            ui.menu_button("Table", |ui| table_menu(ui, cmds));
            ui.menu_button("View", |ui| {
                let view = self.docs.get(self.active).map(|d| d.view);
                for (glyph, label, sc, v) in [
                    (icon::TAG, "Tags on", "F5", View::TagsOn),
                    (icon::ARTICLE, "Normal", "F6", View::Normal),
                    (icon::CODE, "Source", "F8", View::Source),
                ] {
                    let r = ui
                        .add_enabled_ui(has_doc, |ui| {
                            crate::ui::menu_item(ui, Some(glyph), label, Some(sc))
                        })
                        .inner;
                    if view == Some(v) {
                        let p = crate::ui::palette(ui.ctx());
                        ui.painter().circle_filled(
                            egui::pos2(r.rect.left() + 3.0, r.rect.center().y),
                            2.5,
                            p.accent,
                        );
                    }
                    if r.clicked() {
                        cmds.push(Cmd::View(v));
                    }
                }
                ui.separator();
                for (label, sc, c) in [
                    (
                        "Collapse element",
                        Some("Ctrl+Shift+-"),
                        Cmd::Collapse(true),
                    ),
                    ("Expand element", Some("Ctrl+Shift+="), Cmd::Collapse(false)),
                    ("Collapse children", None, Cmd::CollapseChildren),
                    ("Expand all", None, Cmd::ExpandAll),
                ] {
                    if item(ui, has_doc, None, label, sc) {
                        cmds.push(c);
                    }
                }
                ui.separator();
                ui.checkbox(
                    &mut self.settings.show_structure,
                    "Structure and project panel",
                );
                ui.checkbox(&mut self.settings.show_elements, "Elements panel");
                ui.checkbox(&mut self.settings.show_attributes, "Attributes panel");
                ui.checkbox(&mut self.settings.show_issues, "Issues panel");
                let mut review = self.review.open;
                if ui.checkbox(&mut review, "Review panel").clicked() {
                    cmds.push(Cmd::ReviewPanel);
                }
                ui.separator();
                if item(ui, true, Some(icon::FOLDERS), "Project", None) {
                    cmds.push(Cmd::ShowProject);
                }
                if item(ui, has_doc, Some(icon::FUNNEL), "Applicability…", None) {
                    cmds.push(Cmd::Applicability);
                }
                ui.separator();
                if item(
                    ui,
                    true,
                    Some(icon::MAGNIFYING_GLASS_PLUS),
                    "Zoom in",
                    Some("Ctrl++"),
                ) {
                    cmds.push(Cmd::ZoomIn);
                }
                if item(
                    ui,
                    true,
                    Some(icon::MAGNIFYING_GLASS_MINUS),
                    "Zoom out",
                    Some("Ctrl+-"),
                ) {
                    cmds.push(Cmd::ZoomOut);
                }
                if item(ui, true, None, "Actual size", Some("Ctrl+0")) {
                    cmds.push(Cmd::ZoomReset);
                }
                ui.separator();
                let dark = ui.visuals().dark_mode;
                let (glyph, label) = if dark {
                    (icon::SUN, "Light theme")
                } else {
                    (icon::MOON, "Dark theme")
                };
                if item(ui, true, Some(glyph), label, None) {
                    self.settings.dark_mode = Some(!dark);
                    crate::ui::theme::apply(ui.ctx(), !dark);
                }
            });
            ui.menu_button("Tools", |ui| {
                if item(
                    ui,
                    has_doc,
                    Some(icon::CHECK_CIRCLE),
                    "Validate",
                    Some("F7"),
                ) {
                    cmds.push(Cmd::Validate);
                }
                ui.checkbox(&mut self.settings.auto_validate, "Validate as you type");
                let mut spell = self.settings.spell_check;
                if ui.checkbox(&mut spell, "Check spelling").clicked() {
                    cmds.push(Cmd::SpellCheck);
                }
                ui.separator();
                ui.label(
                    RichText::new("S1000D")
                        .text_style(crate::ui::theme::section_style())
                        .color(crate::ui::palette(ui.ctx()).text_muted),
                );
                if item(ui, true, Some(icon::CUBE), "New module…", None) {
                    cmds.push(Cmd::NewS1000d);
                }
                if item(ui, has_doc, Some(icon::SEAL_WARNING), "Check BREX…", None) {
                    cmds.push(Cmd::CheckBrex);
                }
                if item(ui, has_doc, Some(icon::INFO), "Data module info…", None) {
                    cmds.push(Cmd::S1000dInfo);
                }
                if item(ui, has_doc, None, "Rename file to CSDB name", None) {
                    cmds.push(Cmd::RenameToCsdb);
                }
                if item(ui, true, Some(icon::FOLDERS), "Project folder…", None) {
                    cmds.push(Cmd::ProjectFolder);
                }
                if item(ui, true, Some(icon::LINK), "Check references", None) {
                    cmds.push(Cmd::CheckReferences);
                }
                if item(
                    ui,
                    true,
                    Some(icon::CHECKS),
                    "Check stylesheet output…",
                    None,
                ) {
                    cmds.push(Cmd::CheckStylesheet(String::new()));
                }
                if item(
                    ui,
                    has_doc,
                    Some(icon::LINK),
                    "Stylesheet references",
                    Some("F12"),
                ) {
                    cmds.push(Cmd::StylesheetReferences);
                }
            });
            ui.menu_button("Review", |ui| self.review_menu(ui, has_doc, cmds));
            ui.menu_button("Help", |ui| {
                if item(
                    ui,
                    true,
                    Some(icon::KEYBOARD),
                    "Keyboard shortcuts",
                    Some("F1"),
                ) {
                    cmds.push(Cmd::Shortcuts);
                }
                if item(
                    ui,
                    true,
                    Some(icon::ARROWS_CLOCKWISE),
                    "Check for updates…",
                    None,
                ) {
                    cmds.push(Cmd::CheckUpdates);
                }
                if item(ui, true, Some(icon::FOLDER_OPEN), "Open log folder", None) {
                    cmds.push(Cmd::OpenLogFolder);
                }
                if item(ui, true, Some(icon::INFO), "About Xylem", None) {
                    cmds.push(Cmd::About);
                }
            });
        });
    }

    fn review_menu(&mut self, ui: &mut egui::Ui, has_doc: bool, cmds: &mut Vec<Cmd>) {
        use crate::ui::icon;
        let tracking = self
            .docs
            .get(self.active)
            .is_some_and(|d| d.editor.is_tracking());
        let mut on = tracking;
        if ui
            .add_enabled(has_doc, egui::Checkbox::new(&mut on, "Track changes"))
            .on_hover_text("Ctrl+Shift+T")
            .clicked()
        {
            cmds.push(Cmd::TrackChanges);
        }
        let mut panel = self.review.open;
        if ui.checkbox(&mut panel, "Review panel").clicked() {
            cmds.push(Cmd::ReviewPanel);
        }
        ui.separator();
        let items: [(Option<&str>, &str, Option<&str>, Cmd); 5] = [
            (
                Some(icon::CHAT_TEXT),
                "New comment…",
                Some("Ctrl+Shift+M"),
                Cmd::NewComment,
            ),
            (
                Some(icon::CHECK),
                "Accept change at caret",
                None,
                Cmd::ResolveAtCaret(true),
            ),
            (
                Some(icon::X),
                "Reject change at caret",
                None,
                Cmd::ResolveAtCaret(false),
            ),
            (
                None,
                "Accept all changes",
                Some("Ctrl+Shift+A"),
                Cmd::ResolveAll(true),
            ),
            (
                None,
                "Reject all changes",
                Some("Ctrl+Shift+R"),
                Cmd::ResolveAll(false),
            ),
        ];
        for (i, (glyph, label, sc, cmd)) in items.into_iter().enumerate() {
            if i == 1 || i == 3 {
                ui.separator();
            }
            let r = ui
                .add_enabled_ui(has_doc, |ui| crate::ui::menu_item(ui, glyph, label, sc))
                .inner;
            if r.clicked() {
                cmds.push(cmd);
            }
        }
        ui.separator();
        crate::ui::muted(ui, format!("Author: {}", self.settings.author()));
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        use crate::ui::{icon, icon_button, icon_toggle, toolbar_divider};
        let has_doc = !self.docs.is_empty();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icon_button(ui, icon::FILE_PLUS, "New", Some("Ctrl+N")).clicked() {
                cmds.push(Cmd::New);
            }
            if icon_button(ui, icon::FOLDER_OPEN, "Open", Some("Ctrl+O")).clicked() {
                cmds.push(Cmd::Open);
            }
            ui.add_enabled_ui(has_doc, |ui| {
                if icon_button(ui, icon::FLOPPY_DISK, "Save", Some("Ctrl+S")).clicked() {
                    cmds.push(Cmd::Save);
                }
            });
            toolbar_divider(ui);
            let (can_undo, can_redo) = self.docs.get(self.active).map_or((false, false), |d| {
                (d.editor.can_undo(), d.editor.can_redo())
            });
            ui.add_enabled_ui(can_undo, |ui| {
                if icon_button(ui, icon::ARROW_U_UP_LEFT, "Undo", Some("Ctrl+Z")).clicked() {
                    cmds.push(Cmd::Undo);
                }
            });
            ui.add_enabled_ui(can_redo, |ui| {
                if icon_button(ui, icon::ARROW_U_UP_RIGHT, "Redo", Some("Ctrl+Y")).clicked() {
                    cmds.push(Cmd::Redo);
                }
            });
            toolbar_divider(ui);
            if let Some(view) = self.docs.get(self.active).map(|d| d.view) {
                let mut v = view;
                ui.add_space(2.0);
                if crate::ui::segmented(
                    ui,
                    &mut v,
                    &[
                        (View::TagsOn, "Tags on"),
                        (View::Normal, "Normal"),
                        (View::Source, "Source"),
                    ],
                ) {
                    cmds.push(Cmd::View(v));
                }
                ui.add_space(2.0);
                toolbar_divider(ui);
            }
            ui.add_enabled_ui(has_doc, |ui| {
                for (glyph, tip, sc, kind) in [
                    (icon::TEXT_B, "Bold", Some("Ctrl+B"), "bold"),
                    (icon::TEXT_ITALIC, "Italic", Some("Ctrl+I"), "italic"),
                    (
                        icon::TEXT_UNDERLINE,
                        "Underline",
                        Some("Ctrl+U"),
                        "underline",
                    ),
                ] {
                    if icon_button(ui, glyph, tip, sc).clicked() {
                        cmds.push(Cmd::Toggle(kind.into()));
                    }
                }
                // Buttons the doctype package defines (lists, images,
                // paragraphs…). Known kinds get an icon, the rest their label.
                if let Some(ds) = self.docs.get(self.active) {
                    let entries = ds.package.effective_toolbar();
                    // Room left for package buttons before the fixed tools
                    // (≈100 points) and the right-hand toggles (≈170). What
                    // doesn't fit goes into a More menu rather than off the edge.
                    let mut room = ui.available_width() - 290.0;
                    let mut overflow: Vec<(String, String, Cmd)> = Vec::new();
                    for entry in &entries {
                        let (cmd, what) = match &entry.action {
                            xylem_customize::Action::InsertElement(e) => {
                                (Cmd::InsertElement(e.clone()), format!("Insert <{e}>"))
                            }
                            xylem_customize::Action::Surround(e) => {
                                (Cmd::Surround(e.clone()), format!("Surround with <{e}>"))
                            }
                            xylem_customize::Action::Toggle(k)
                                if !matches!(k.as_str(), "bold" | "italic" | "underline") =>
                            {
                                (Cmd::Toggle(k.clone()), entry.label.clone())
                            }
                            _ => continue,
                        };
                        let glyph = toolbar_icon_for(&entry.label);
                        let need = match glyph {
                            Some(_) => 32.0,
                            None => entry.label.chars().count() as f32 * 7.5 + 16.0,
                        };
                        if need > room {
                            overflow.push((entry.label.clone(), what, cmd));
                            continue;
                        }
                        room -= need;
                        let clicked = match glyph {
                            Some(glyph) => icon_button(ui, glyph, &entry.label, None)
                                .on_hover_text(&what)
                                .clicked(),
                            None => {
                                let r = ui
                                    .add(
                                        egui::Button::new(&entry.label)
                                            .frame(false)
                                            .min_size(egui::vec2(0.0, 28.0)),
                                    )
                                    .on_hover_text(&what);
                                crate::script::track(&entry.label, &r);
                                r.clicked()
                            }
                        };
                        if clicked {
                            cmds.push(cmd);
                        }
                    }
                    if !overflow.is_empty() {
                        let r = ui.menu_button(
                            RichText::new(icon::DOTS_THREE)
                                .size(crate::ui::theme::size::ICON + 1.0),
                            |ui| {
                                for (label, what, cmd) in overflow {
                                    if crate::ui::menu_item(
                                        ui,
                                        toolbar_icon_for(&label),
                                        &label,
                                        None,
                                    )
                                    .on_hover_text(what)
                                    .clicked()
                                    {
                                        cmds.push(cmd);
                                    }
                                }
                            },
                        );
                        r.response
                            .on_hover_text("More buttons for this document type");
                    }
                }
                toolbar_divider(ui);
                if icon_button(ui, icon::TABLE, "Insert table", None).clicked() {
                    cmds.push(Cmd::InsertTable);
                }
                if icon_button(ui, icon::CHECK_CIRCLE, "Validate", Some("F7")).clicked() {
                    cmds.push(Cmd::Validate);
                }
                if icon_button(
                    ui,
                    icon::MAGNIFYING_GLASS,
                    "Find and replace",
                    Some("Ctrl+F"),
                )
                .clicked()
                {
                    cmds.push(Cmd::Find);
                }
            });
            // Panels and review on the right.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let review_open = self.review.open;
                if icon_toggle(
                    ui,
                    review_open,
                    icon::CHAT_CIRCLE_TEXT,
                    "Review panel",
                    None,
                )
                .clicked()
                {
                    cmds.push(Cmd::ReviewPanel);
                }
                let tracking = self
                    .docs
                    .get(self.active)
                    .is_some_and(|d| d.editor.is_tracking());
                ui.add_enabled_ui(has_doc, |ui| {
                    if icon_toggle(
                        ui,
                        tracking,
                        icon::GIT_DIFF,
                        "Track changes",
                        Some("Ctrl+Shift+T"),
                    )
                    .clicked()
                    {
                        cmds.push(Cmd::TrackChanges);
                    }
                });
                toolbar_divider(ui);
                // Shown as on only while the panel is actually on screen (narrow
                // windows hide it even when it is switched on).
                let width = ui.ctx().content_rect().width();
                let inspector = self.settings.show_elements || self.settings.show_attributes;
                let inspector_visible = inspector
                    && !self.docs.is_empty()
                    && (width >= 1000.0 || (!self.settings.show_structure && width >= 760.0));
                if icon_toggle(
                    ui,
                    inspector_visible,
                    icon::SIDEBAR,
                    "Elements and attributes",
                    None,
                )
                .clicked()
                {
                    let on = !inspector;
                    self.settings.show_elements = on;
                    self.settings.show_attributes = on;
                }
                let structure_visible = self.settings.show_structure && width >= 760.0;
                if icon_toggle(
                    ui,
                    structure_visible,
                    icon::TREE_STRUCTURE,
                    "Structure and project",
                    None,
                )
                .clicked()
                {
                    self.settings.show_structure = !self.settings.show_structure;
                }
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        use crate::ui::{icon, palette};
        let p = palette(ui.ctx());
        egui::ScrollArea::horizontal()
            .id_salt("doc-tabs")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (i, d) in self.docs.iter().enumerate() {
                        let active = i == self.active;
                        let title = d.title();
                        let dirty = d.is_dirty();
                        let font = egui::FontId::proportional(crate::ui::theme::size::BODY);
                        let galley = ui.painter().layout_no_wrap(
                            title.trim_end_matches(" •").to_string(),
                            font,
                            if active { p.text } else { p.text_muted },
                        );
                        let w = galley.size().x + 12.0 + 22.0 + 10.0;
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(w, 32.0), egui::Sense::click());
                        let hovered = resp.hovered();
                        let close_rect = egui::Rect::from_center_size(
                            egui::pos2(rect.right() - 16.0, rect.center().y),
                            egui::vec2(20.0, 20.0),
                        );
                        let over_close = ui
                            .ctx()
                            .pointer_hover_pos()
                            .is_some_and(|pos| close_rect.contains(pos));
                        if active {
                            ui.painter().rect_filled(
                                rect.shrink2(egui::vec2(0.0, 2.0)),
                                egui::CornerRadius {
                                    nw: 6,
                                    ne: 6,
                                    sw: 0,
                                    se: 0,
                                },
                                p.surface,
                            );
                            ui.painter().hline(
                                rect.x_range(),
                                rect.bottom() - 1.0,
                                egui::Stroke::new(2.0_f32, p.accent),
                            );
                        } else if hovered {
                            ui.painter().rect_filled(
                                rect.shrink2(egui::vec2(0.0, 4.0)),
                                egui::CornerRadius::same(6),
                                p.hover,
                            );
                        }
                        ui.painter().galley(
                            egui::pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            p.text,
                        );
                        // Unsaved documents show a dot where the close button
                        // goes; hovering turns it into the close button.
                        if over_close {
                            ui.painter().rect_filled(
                                close_rect,
                                egui::CornerRadius::same(4),
                                p.pressed,
                            );
                        }
                        let glyph_color = if over_close { p.text } else { p.text_faint };
                        if dirty && !(hovered || active && over_close) {
                            ui.painter()
                                .circle_filled(close_rect.center(), 3.5, p.accent);
                        } else if hovered || active {
                            ui.painter().text(
                                close_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                icon::X,
                                egui::FontId::proportional(13.0),
                                glyph_color,
                            );
                        }
                        resp.widget_info(|| {
                            let name = if dirty {
                                format!("{}, unsaved changes", title.trim_end_matches(" •"))
                            } else {
                                title.clone()
                            };
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                active,
                                name,
                            )
                        });
                        let resp = match &d.path {
                            Some(path) => resp.on_hover_text(path.display().to_string()),
                            None => resp.on_hover_text("Not saved yet"),
                        };
                        crate::script::track(&title, &resp);
                        if resp.clicked() {
                            if over_close {
                                cmds.push(Cmd::Close(i));
                            } else {
                                self.active = i;
                            }
                        }
                        if resp.middle_clicked() {
                            cmds.push(Cmd::Close(i));
                        }
                    }
                });
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
        use crate::ui::{icon, palette, theme::size, theme::space};
        let p = palette(ui.ctx());
        let width = 460.0_f32.min(ui.available_width() - 32.0);
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.14).clamp(24.0, 120.0));
            ui.label(
                RichText::new(icon::TREE_STRUCTURE)
                    .size(40.0)
                    .color(p.accent),
            );
            ui.add_space(space::S);
            ui.label(
                RichText::new("Xylem")
                    .size(30.0)
                    .family(crate::ui::theme::strong_family())
                    .color(p.text),
            );
            ui.label(RichText::new("Structured XML and S1000D authoring").color(p.text_muted));
            ui.add_space(space::XXL);
        });
        ui.vertical_centered(|ui| {
            ui.set_max_width(width);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::M;
                let actions: [(&str, &str, Cmd); 3] = [
                    (icon::FOLDER_OPEN, "Open…", Cmd::Open),
                    (icon::FILE_PLUS, "New document…", Cmd::New),
                    (icon::CUBE, "New S1000D module…", Cmd::NewS1000d),
                ];
                let each = (width - 2.0 * space::M) / 3.0;
                for (glyph, label, cmd) in actions {
                    let (rect, r) =
                        ui.allocate_exact_size(egui::vec2(each, 76.0), egui::Sense::click());
                    let hovered = r.hovered();
                    ui.painter().rect(
                        rect,
                        egui::CornerRadius::same(crate::ui::theme::radius::CARD),
                        if hovered { p.hover } else { p.surface },
                        egui::Stroke::new(
                            1.0_f32,
                            if hovered { p.border_strong } else { p.border },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center() - egui::vec2(0.0, 12.0),
                        egui::Align2::CENTER_CENTER,
                        glyph,
                        egui::FontId::proportional(22.0),
                        p.accent,
                    );
                    ui.painter().text(
                        rect.center() + egui::vec2(0.0, 16.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        egui::FontId::proportional(size::BODY),
                        p.text,
                    );
                    if hovered {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label)
                    });
                    crate::script::track(label, &r);
                    if r.clicked() {
                        cmds.push(cmd);
                    }
                }
            });
            ui.add_space(space::XL);
            if !self.settings.recent_files.is_empty() {
                ui.horizontal(|ui| crate::ui::section(ui, "Recent"));
                for path in self.settings.recent_files.iter().take(8) {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let dir = path
                        .parent()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default();
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::click());
                    crate::ui::row_background(ui, rect, false, resp.hovered());
                    let painter = ui.painter();
                    painter.text(
                        rect.left_center() + egui::vec2(10.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        icon::FILE_TEXT,
                        egui::FontId::proportional(18.0),
                        p.text_muted,
                    );
                    painter.text(
                        rect.left_top() + egui::vec2(38.0, 4.0),
                        egui::Align2::LEFT_TOP,
                        &name,
                        egui::FontId::proportional(size::BODY),
                        p.text,
                    );
                    painter.text(
                        rect.left_top() + egui::vec2(38.0, 22.0),
                        egui::Align2::LEFT_TOP,
                        &dir,
                        egui::FontId::proportional(size::SMALL),
                        p.text_faint,
                    );
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            true,
                            format!("Open {name}, {dir}"),
                        )
                    });
                    crate::script::track(&name, &resp);
                    if resp.on_hover_text(path.display().to_string()).clicked() {
                        cmds.push(Cmd::OpenPath(path.clone()));
                    }
                }
                ui.add_space(space::XL);
            }
            let n = self.registry.packages().len();
            ui.label(
                RichText::new(format!(
                    "{n} doctype package{} available · drop files here to open them",
                    if n == 1 { "" } else { "s" }
                ))
                .size(size::SMALL)
                .color(p.text_faint),
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
            doc_key: {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                match &ds.path {
                    Some(p) => p.hash(&mut h),
                    None => ds.id.hash(&mut h),
                }
                h.finish()
            },
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
                    use xylem_edit::revision::{MarkKind, mark_attrs, mark_kind};
                    match mark_kind(&ds.editor.doc, n) {
                        Some(MarkKind::Comment | MarkKind::CommentStart) => {
                            let text = mark_attrs(&ds.editor.doc, n)
                                .get("data")
                                .unwrap_or_default()
                                .to_string();
                            self.dialogs.comment = Some(CommentDialog {
                                node: Some(n),
                                text,
                                focus: true,
                            });
                            continue;
                        }
                        Some(_) => {
                            self.review.open = true;
                            continue;
                        }
                        None => {}
                    }
                    if rules.is_protected(&ds.editor.doc, n) {
                        self.toasts.push((
                            "This marker is protected and can't be edited.".into(),
                            0.0,
                            true,
                        ));
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
                    use crate::ui::icon;
                    // Look like a real menu: frameless rows, full width.
                    ui.style_mut().visuals.button_frame = false;
                    ui.set_min_width(230.0);
                    let mut picked: Option<Cmd> = None;
                    let item = |ui: &mut egui::Ui,
                                glyph: Option<&str>,
                                label: &str,
                                shortcut: Option<&str>,
                                c: Cmd,
                                picked: &mut Option<Cmd>| {
                        if crate::ui::menu_item(ui, glyph, label, shortcut).clicked() {
                            *picked = Some(c);
                        }
                    };
                    if let Some((node, range, word, sugg)) = &menu.suggestions {
                        crate::ui::muted(
                            ui,
                            format!("\u{201c}{word}\u{201d} is not in the dictionary"),
                        );
                        for s in sugg.iter().take(6) {
                            item(
                                ui,
                                None,
                                s,
                                None,
                                Cmd::Replace(*node, range.clone(), s.clone()),
                                &mut picked,
                            );
                        }
                        item(
                            ui,
                            Some(icon::PLUS),
                            "Add to dictionary",
                            None,
                            Cmd::AddToDictionary(word.clone()),
                            &mut picked,
                        );
                        ui.separator();
                    }
                    item(ui, None, "Cut", Some("Ctrl+X"), Cmd::Cut, &mut picked);
                    item(ui, None, "Copy", Some("Ctrl+C"), Cmd::Copy, &mut picked);
                    item(ui, None, "Paste", Some("Ctrl+V"), Cmd::Paste, &mut picked);
                    ui.separator();
                    if !menu.insertable.is_empty() {
                        ui.menu_button(
                            crate::ui::icon_text(icon::BRACKETS_ANGLE, "Insert element"),
                            |ui| {
                                egui::ScrollArea::vertical()
                                    .max_height(360.0)
                                    .show(ui, |ui| {
                                        for n in &menu.insertable {
                                            if crate::ui::menu_item(ui, None, n, None).clicked() {
                                                picked = Some(Cmd::InsertElement(n.clone()));
                                            }
                                        }
                                    });
                            },
                        );
                    }
                    if !name.is_empty() {
                        let sel = format!("Select <{name}>");
                        let split = format!("Split <{name}>");
                        let untag = format!("Remove <{name}> tags");
                        let del = format!("Delete <{name}>");
                        let fold = format!("Collapse <{name}>");
                        item(
                            ui,
                            None,
                            &sel,
                            Some("Ctrl+E"),
                            Cmd::SelectElement,
                            &mut picked,
                        );
                        item(
                            ui,
                            Some(icon::PENCIL_SIMPLE),
                            "Edit attributes",
                            None,
                            Cmd::EditAttributes,
                            &mut picked,
                        );
                        item(
                            ui,
                            Some(icon::LINK),
                            "Follow link",
                            Some("Ctrl+click"),
                            Cmd::FollowLink(menu.node),
                            &mut picked,
                        );
                        ui.separator();
                        item(ui, None, &split, None, Cmd::SplitElement, &mut picked);
                        item(
                            ui,
                            None,
                            "Join to preceding",
                            None,
                            Cmd::JoinPreceding,
                            &mut picked,
                        );
                        item(ui, None, &untag, None, Cmd::RemoveTags, &mut picked);
                        item(
                            ui,
                            Some(icon::TRASH),
                            &del,
                            None,
                            Cmd::DeleteElement,
                            &mut picked,
                        );
                        ui.separator();
                        item(
                            ui,
                            None,
                            &fold,
                            Some("Ctrl+Shift+-"),
                            Cmd::Collapse(true),
                            &mut picked,
                        );
                        item(
                            ui,
                            None,
                            "Collapse children",
                            None,
                            Cmd::CollapseChildren,
                            &mut picked,
                        );
                    }
                    if in_table {
                        ui.separator();
                        ui.menu_button(crate::ui::icon_text(icon::TABLE, "Table"), |ui| {
                            table_menu(ui, cmds)
                        });
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
        if self.review.open && !self.docs.is_empty() {
            let mut review_acts = Vec::new();
            egui::SidePanel::right("review")
                .resizable(true)
                .default_width(300.0)
                .frame(side_frame(ctx))
                .show(ctx, |ui| {
                    crate::ui::section(ui, "Review");
                    if let Some(ds) = self.docs.get(self.active) {
                        crate::review::panel(ui, ds, &mut self.review, &mut review_acts);
                    }
                });
            for a in review_acts {
                self.review_action(a, cmds);
            }
        }
        self.project.poll(ctx);
        // The Project tab is useful before any document is open.
        let project_tab = self.left_tab == LeftTab::Project;
        // In a narrow window the document keeps its room: the inspector goes
        // first, then the left panel. The toolbar toggles still work once the
        // window is wide enough again.
        let width = ctx.content_rect().width();
        let room_for_left = width >= 760.0;
        let room_for_right = width >= 1000.0 || (!self.settings.show_structure && width >= 760.0);
        if self.settings.show_structure && room_for_left && (project_tab || !self.docs.is_empty()) {
            let mut project_acts = Vec::new();
            let can_insert = match self.docs.get(self.active) {
                Some(ds) if project_tab => crate::s1000d_ui::reference_insertable(ds),
                _ => (false, false),
            };
            // Each tab keeps its own panel width: the module list needs room
            // for a full data module code, the structure tree does not.
            let (id, width, min_width) = if project_tab {
                ("project", 340.0, 300.0)
            } else {
                ("structure", 230.0, 96.0)
            };
            egui::SidePanel::left(id)
                .resizable(true)
                .default_width(width)
                .min_width(min_width)
                .frame(side_frame(ctx))
                .show(ctx, |ui| {
                    crate::ui::panel_tabs(
                        ui,
                        &mut self.left_tab,
                        &[
                            (LeftTab::Structure, "Structure"),
                            (LeftTab::Project, "Project"),
                        ],
                    );
                    match self.left_tab {
                        LeftTab::Structure => {
                            if let Some(ds) = self.docs.get(self.active) {
                                panels::structure(ui, ds, &mut self.panel, &mut acts);
                            }
                        }
                        LeftTab::Project => self.project.ui(ui, can_insert, &mut project_acts),
                    }
                });
            if self.left_tab == LeftTab::Project && !project_tab {
                cmds.push(Cmd::ShowProject);
            }
            for a in project_acts {
                self.project_action(ctx, a);
            }
        }
        if self.docs.is_empty() {
            return;
        }
        if (self.settings.show_elements || self.settings.show_attributes) && room_for_right {
            egui::SidePanel::right("inspector")
                .resizable(true)
                .default_width(280.0)
                .min_width(220.0)
                .frame(side_frame(ctx))
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
                        crate::ui::section(ui, "Elements");
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
                        crate::ui::divider(ui);
                        crate::ui::section(ui, "Attributes");
                        panels::attributes(ui, ds, &mut self.panel, &mut acts);
                    }
                });
        }
        if self.settings.show_issues {
            egui::TopBottomPanel::bottom("issues")
                .resizable(true)
                .default_height(120.0)
                .frame(
                    egui::Frame::new()
                        .fill(crate::ui::palette(ctx).panel)
                        .inner_margin(egui::Margin::symmetric(12, 8)),
                )
                .show(ctx, |ui| {
                    let Some(ds) = self.docs.get(self.active) else {
                        return;
                    };
                    let errs = ds
                        .issues
                        .iter()
                        .filter(|i| i.severity == Severity::Error)
                        .count();
                    let warns = ds.issues.len() - errs;
                    crate::ui::section_header(ui, "Issues", |ui| {
                        if errs == 0 && warns == 0 {
                            crate::ui::badge(ui, "No problems", crate::ui::Tone::Success);
                        } else {
                            if warns > 0 {
                                crate::ui::badge(
                                    ui,
                                    format!("{warns} other"),
                                    crate::ui::Tone::Warning,
                                );
                            }
                            if errs > 0 {
                                crate::ui::badge(
                                    ui,
                                    format!("{errs} error{}", if errs == 1 { "" } else { "s" }),
                                    crate::ui::Tone::Danger,
                                );
                            }
                        }
                        ui.add_space(8.0);
                        let p = crate::ui::palette(ui.ctx());
                        ui.add(
                            egui::Label::new(
                                RichText::new(&ds.grammar_status)
                                    .size(crate::ui::theme::size::SMALL)
                                    .color(p.text_faint),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&ds.grammar_status);
                    });
                    panels::issues(ui, ds, &mut acts);
                });
        }
        for a in acts {
            self.panel_action(a, cmds);
        }
    }

    fn review_action(&mut self, a: crate::review::ReviewAction, cmds: &mut Vec<Cmd>) {
        use crate::review::ReviewAction;
        match a {
            ReviewAction::Goto(n) => {
                let Some(ds) = self.doc() else { return };
                if !ds.editor.doc.contains(n) || ds.view == View::Source {
                    return;
                }
                let Some(parent) = ds.editor.doc.parent(n) else {
                    return;
                };
                let idx = ds.editor.doc.index_in_parent(n).unwrap_or(0);
                // Just past a start mark is inside the change it begins.
                let p = xylem_edit::pos::canonical(
                    &ds.editor.doc,
                    &xylem_edit::NoRules,
                    xylem_edit::Pos::new(parent, idx + 1),
                );
                ds.editor.set_caret(p);
                ds.canvas.reveal(parent);
                ds.canvas.scroll_to_caret = true;
                ds.canvas.request_focus = true;
            }
            ReviewAction::Accept(n) => cmds.push(Cmd::Resolve(n, true)),
            ReviewAction::Reject(n) => cmds.push(Cmd::Resolve(n, false)),
            ReviewAction::ResolveAll(accept) => cmds.push(Cmd::ResolveAll(accept)),
            ReviewAction::SaveComment(n, text) => {
                self.with_editor(|ed, r| ed.set_comment_text(r, n, &text));
            }
            ReviewAction::DeleteComment(n) => {
                self.with_editor(|ed, r| ed.delete_comment(r, n));
            }
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
            PanelAction::Collapse(n, on) => {
                if let Some(ds) = self.doc() {
                    ds.canvas.set_collapsed(n, on);
                }
            }
            PanelAction::MoveElement(n, up) => {
                self.with_editor(|ed, r| ed.move_element(r, n, up));
            }
        }
    }

    fn status_bar(&mut self, ctx: &egui::Context) {
        use crate::ui::{icon, palette, theme::size};
        let mut acts = Vec::new();
        let words = self.docs.get_mut(self.active).map(|d| d.word_count());
        let p = palette(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(p.chrome)
                    .inner_margin(egui::Margin::symmetric(10, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let Some(ds) = self.docs.get(self.active) else {
                        crate::ui::muted(ui, "Ready");
                        return;
                    };
                    panels::breadcrumb(ui, ds, &mut acts);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let seg = |ui: &mut egui::Ui, text: String| {
                            ui.label(RichText::new(text).size(size::SMALL).color(p.text_muted))
                        };
                        let sep = |ui: &mut egui::Ui| {
                            ui.label(RichText::new("·").color(p.text_faint));
                        };
                        seg(ui, format!("{:.0}%", self.settings.zoom * 100.0))
                            .on_hover_text("Zoom (Ctrl++ / Ctrl+- / Ctrl+0)");
                        if ds.editor.is_tracking() {
                            sep(ui);
                            crate::ui::badge(ui, "Tracking changes", crate::ui::Tone::Accent)
                                .on_hover_text("Review > Track changes (Ctrl+Shift+T)");
                        }
                        if let Some(w) = words {
                            sep(ui);
                            seg(ui, format!("{w} words"));
                        }
                        sep(ui);
                        seg(ui, ds.package.doctype.name.clone()).on_hover_text("Document type");
                        if let Some(info) = &ds.s1000d {
                            sep(ui);
                            seg(ui, format!("S1000D {}", info.issue));
                        }
                        let errs = ds
                            .issues
                            .iter()
                            .filter(|i| i.severity == Severity::Error)
                            .count();
                        if ds.grammar.is_some() {
                            sep(ui);
                            if errs == 0 {
                                ui.label(
                                    RichText::new(format!("{}  Valid", icon::CHECK_CIRCLE))
                                        .size(size::SMALL)
                                        .color(p.success),
                                );
                            } else {
                                ui.label(
                                    RichText::new(format!(
                                        "{}  {errs} error{}",
                                        icon::WARNING_CIRCLE,
                                        if errs == 1 { "" } else { "s" }
                                    ))
                                    .size(size::SMALL)
                                    .color(p.danger),
                                );
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

    /// Looks for a newer build in the team's update folder on a worker thread
    /// (it may be a slow network share). A `quiet` check only speaks up when
    /// there is something new.
    fn start_update_check(&mut self, quiet: bool) {
        if matches!(
            self.update,
            UpdateState::Checking { .. } | UpdateState::Applied(_)
        ) {
            return;
        }
        let Some(source) = self.team.update_source.clone() else {
            self.update = UpdateState::Failed(
                "No update folder is configured. Set update_source in xylem-team.toml (see docs/team-setup.md).".into(),
            );
            return;
        };
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(crate::install::check(&source));
            ctx.request_repaint();
        });
        self.update = UpdateState::Checking { rx, quiet };
    }

    fn poll_update_check(&mut self) {
        let UpdateState::Checking { rx, quiet } = &self.update else {
            return;
        };
        let quiet = *quiet;
        let result = match rx.try_recv() {
            Ok(r) => r,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("the update check stopped unexpectedly".into())
            }
        };
        self.update = match result {
            Ok(Some(a)) => {
                if quiet {
                    self.toast(
                        format!(
                            "Xylem {} is available: Help › Check for updates.",
                            a.version
                        ),
                        false,
                    );
                }
                UpdateState::Found(a)
            }
            Ok(None) => UpdateState::UpToDate,
            Err(e) => {
                if quiet {
                    log::warn!("update check: {e}");
                }
                UpdateState::Failed(e)
            }
        };
    }

    fn update_dialog(&mut self, ctx: &egui::Context) {
        use crate::ui::{Dialog, Tone, dialog_footer, notice};
        if !self.dialogs.update {
            return;
        }
        let mut apply = None;
        let mut restart = None;
        let mut close = false;
        let dirty = self.docs.iter().any(|d| d.is_dirty());
        let (x, _) = Dialog::new("Check for updates")
            .width(400.0)
            .show(ctx, |ui| {
                crate::ui::muted(ui, format!("This is Xylem {}.", crate::install::VERSION));
                ui.add_space(6.0);
                let mut primary: Option<(&str, bool)> = None;
                match &self.update {
                    UpdateState::Idle | UpdateState::Checking { .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Checking the update folder…");
                        });
                    }
                    UpdateState::UpToDate => {
                        notice(ui, Tone::Success, "You have the latest version.")
                    }
                    UpdateState::Failed(e) => notice(ui, Tone::Warning, e.clone()),
                    UpdateState::Found(a) => {
                        notice(
                            ui,
                            Tone::Accent,
                            format!("Version {} is available.", a.version),
                        );
                        primary = Some(("Update now", true));
                    }
                    UpdateState::Applied(_) => {
                        notice(
                            ui,
                            Tone::Success,
                            "Updated. The new version starts the next time you open Xylem.",
                        );
                        if dirty {
                            crate::ui::muted(ui, "Save your documents, then restart.");
                        } else {
                            primary = Some(("Restart now", true));
                        }
                    }
                }
                let r = dialog_footer(ui, primary, Some("Close"), |_| {});
                if r.confirmed {
                    match &self.update {
                        UpdateState::Found(a) => apply = Some(a.clone()),
                        UpdateState::Applied(exe) => restart = Some(exe.clone()),
                        _ => {}
                    }
                }
                close |= r.closed;
            });
        if let Some(a) = apply {
            self.update = match crate::install::apply(&a) {
                Ok(exe) => UpdateState::Applied(exe),
                Err(e) => UpdateState::Failed(e),
            };
        }
        if let Some(exe) = restart {
            let files: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
            match std::process::Command::new(&exe).args(&files).spawn() {
                Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => self.toast(format!("Could not start {}: {e}", exe.display()), true),
            }
        }
        if x || close {
            self.dialogs.update = false;
        }
    }

    /// Loads fonts for the writing systems the active document uses, once
    /// when it opens and then at most every two seconds while it changes
    /// (scanning a large document on every keystroke would show).
    fn check_scripts(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let Some(ds) = self.docs.get(self.active) else {
            return;
        };
        let rev = ds.editor.doc.revision();
        if ds.scripts_checked == Some(rev)
            || (ds.scripts_checked.is_some() && now - self.last_script_check < 2.0)
        {
            return;
        }
        self.last_script_check = now;
        let doc = &ds.editor.doc;
        let lang = doc
            .document_element()
            .and_then(|r| doc.attr(r, "xml:lang").or_else(|| doc.attr(r, "lang")))
            .unwrap_or("")
            .to_ascii_lowercase();
        let traditional = ["zh-tw", "zh-hk", "zh-mo", "zh-hant"]
            .iter()
            .any(|t| lang.starts_with(t));
        let scripts = crate::fonts::scripts_in(&doc.to_xml(), traditional);
        let (added, missing) = crate::fonts::ensure(ctx, &scripts);
        if !added.is_empty() {
            // Cached layout was measured without these glyphs.
            for d in &mut self.docs {
                d.canvas.invalidate();
            }
        }
        for s in missing {
            self.toast(
                format!(
                    "No font for {} text was found on this computer; it shows as boxes.",
                    s.name()
                ),
                true,
            );
        }
        let ds = &mut self.docs[self.active];
        ds.scripts_checked = Some(rev);
        let unshaped: Vec<&str> = scripts
            .iter()
            .filter(|s| s.needs_shaping())
            .map(|s| s.name())
            .collect();
        if !unshaped.is_empty() && !ds.shaping_warned {
            ds.shaping_warned = true;
            let names = unshaped.join(" and ");
            self.toast(
                format!(
                    "This document has {names} text. Xylem shows its characters but can't yet join them or lay out right-to-left text, so check that text in another viewer."
                ),
                true,
            );
        }
    }

    /// Holds a lock file for every open document that may be saved, and
    /// lets go of the rest (dropping a lock removes its file).
    fn sync_locks(&mut self) {
        let open: std::collections::HashSet<PathBuf> = self
            .docs
            .iter()
            .filter(|d| !d.read_only)
            .filter_map(|d| d.path.clone())
            .collect();
        self.locks.retain(|p, _| open.contains(p));
        for p in open {
            self.locks
                .entry(p)
                .or_insert_with_key(|p| crate::files::Lock::acquire(p));
        }
    }

    /// Every couple of seconds, checks on a worker thread (network shares
    /// can be slow) whether open files changed on disk.
    fn poll_disk(&mut self, ctx: &egui::Context) {
        const INTERVAL: f64 = 2.0;
        if let Some(rx) = &self.disk_poll {
            match rx.try_recv() {
                Ok(results) => {
                    self.disk_poll = None;
                    self.apply_disk_results(results);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.disk_poll = None,
            }
        }
        let paths: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        if paths.is_empty() {
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(INTERVAL));
        let now = ctx.input(|i| i.time);
        if now - self.last_disk_poll < INTERVAL {
            return;
        }
        self.last_disk_poll = now;
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let stamps = paths
                .into_iter()
                .map(|p| {
                    let s = crate::files::stamp(&p);
                    (p, s)
                })
                .collect();
            if tx.send(stamps).is_ok() {
                ctx.request_repaint();
            }
        });
        self.disk_poll = Some(rx);
    }

    fn apply_disk_results(&mut self, results: Vec<(PathBuf, Option<crate::files::Stamp>)>) {
        for (path, now) in results {
            let Some(i) = self
                .docs
                .iter()
                .position(|d| d.path.as_ref() == Some(&path))
            else {
                continue;
            };
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ds = &mut self.docs[i];
            if now == ds.disk || (now.is_some() && now == ds.disk_ignored) {
                continue;
            }
            let Some(now) = now else {
                if !ds.missing_warned {
                    ds.missing_warned = true;
                    self.toast(
                        format!(
                            "{name} was moved or deleted outside Xylem. Save to write it again."
                        ),
                        true,
                    );
                }
                continue;
            };
            ds.missing_warned = false;
            if self
                .dialogs
                .disk_changed
                .as_ref()
                .is_some_and(|d| d.path == path)
            {
                continue;
            }
            if !ds.is_dirty() && ds.source.is_none() {
                // Nothing to lose: take the new version, as other editors do.
                if self.reload_document(i) {
                    self.toast(
                        format!("Reloaded {name}: it was changed outside Xylem."),
                        false,
                    );
                } else {
                    self.docs[i].disk_ignored = Some(now);
                    self.toast(
                        format!("{name} was changed outside Xylem and can't be read now; your copy is unchanged."),
                        true,
                    );
                }
            } else {
                self.dialogs.disk_changed = Some(DiskChanged {
                    path,
                    saving: false,
                });
            }
        }
    }

    /// Lock, changed-on-disk, previous-version and crash dialogs.
    fn safety_dialogs(&mut self, ctx: &egui::Context) {
        use crate::ui::{Dialog, Tone, dialog_footer, icon, notice, palette};
        let p = palette(ctx);
        let name_of = |path: &Path| {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };

        // Someone else has the file open.
        if let Some((path, owner)) = self.dialogs.locked.first().cloned() {
            let name = name_of(&path);
            let mut choice = None;
            let mut cancel = false;
            let (x, _) = Dialog::new("Document in use")
                .id("locked")
                .width(460.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{name} is open by {} on {} (since {}).",
                        owner.user,
                        owner.host,
                        crate::files::readable_stamp(&owner.since)
                    ));
                    crate::ui::muted(
                        ui,
                        "If you both edit it, whoever saves last overwrites the other's changes.",
                    );
                    let r =
                        dialog_footer(ui, Some(("Open read-only", true)), Some("Cancel"), |ui| {
                            if crate::ui::button(ui, "Open anyway")
                                .on_hover_text("Edit it as well; agree with them who saves")
                                .clicked()
                            {
                                choice = Some(false);
                            }
                        });
                    if r.confirmed {
                        choice = Some(true);
                    } else if r.closed {
                        cancel = true;
                    }
                });
            if x || cancel {
                self.dialogs.locked.retain(|(q, _)| *q != path);
            }
            if let Some(read_only) = choice {
                self.dialogs.locked.retain(|(q, _)| *q != path);
                self.open_path_now(path, read_only);
                if read_only && let Some(ds) = self.docs.last_mut() {
                    ds.title_override = Some(format!("{name} (read-only)"));
                }
            }
        }

        // Changed on disk while edited here.
        if let Some(dc) = self.dialogs.disk_changed.clone() {
            let Some(i) = self
                .docs
                .iter()
                .position(|d| d.path.as_ref() == Some(&dc.path))
            else {
                self.dialogs.disk_changed = None;
                return;
            };
            let name = name_of(&dc.path);
            let mut action = None;
            let (x, _) = Dialog::new("Changed on disk")
                .id("disk-changed")
                .width(480.0)
                .show(ctx, |ui| {
                    notice(
                        ui,
                        Tone::Warning,
                        format!("{name} was changed outside Xylem after you opened it."),
                    );
                    ui.add_space(6.0);
                    crate::ui::muted(
                        ui,
                        "Keeping yours overwrites the other version when you save. Reloading \
                     discards your unsaved changes here.",
                    );
                    let keep = if dc.saving {
                        "Save mine anyway"
                    } else {
                        "Keep my version"
                    };
                    let r = dialog_footer(ui, Some((keep, true)), None, |ui| {
                        if crate::ui::danger_button(ui, "Reload from disk").clicked() {
                            action = Some("reload");
                        }
                        if crate::ui::button(ui, "Open the disk version")
                            .on_hover_text("Open the other version read-only, to compare")
                            .clicked()
                        {
                            action = Some("compare");
                        }
                    });
                    if r.confirmed {
                        action = Some("keep");
                    }
                });
            if x {
                action = Some("keep-quietly");
            }
            if let Some(a) = action {
                self.dialogs.disk_changed = None;
                let now = crate::files::stamp(&dc.path);
                match a {
                    "reload" => {
                        if !self.reload_document(i) {
                            self.toast(format!("{name} can't be read now."), true);
                        }
                    }
                    "compare" => {
                        self.docs[i].disk_ignored = now;
                        match std::fs::read(&dc.path)
                            .map_err(|e| e.to_string())
                            .and_then(|b| Document::from_bytes(&b).map_err(|e| e.to_string()))
                        {
                            Ok(doc) => {
                                self.add_doc(doc, None);
                                if let Some(ds) = self.docs.last_mut() {
                                    ds.read_only = true;
                                    ds.title_override = Some(format!("{name} (on disk)"));
                                    ds.editor.mark_saved();
                                }
                            }
                            Err(e) => self.toast(format!("Can't open the disk version: {e}"), true),
                        }
                    }
                    _ => {
                        self.docs[i].disk_ignored = now;
                        if dc.saving && a == "keep" {
                            self.active = i;
                            self.save(false);
                        }
                    }
                }
            }
        }

        // File > Previous versions.
        if self.dialogs.versions {
            let path = self.docs.get(self.active).and_then(|d| d.path.clone());
            let backups = path
                .as_deref()
                .map(crate::files::backups)
                .unwrap_or_default();
            let mut open = None;
            let (x, res) = Dialog::new("Previous versions").width(480.0).show(ctx, |ui| {
                match &path {
                    None => {
                        crate::ui::muted(ui, "Save the document first; earlier versions are kept from then on.");
                    }
                    Some(path) if backups.is_empty() => {
                        crate::ui::empty_state(
                            ui,
                            icon::CLOCK_COUNTER_CLOCKWISE,
                            "No earlier versions yet",
                            &format!(
                                "Each time {} is saved, the version it replaces is kept here (the last ten).",
                                name_of(path)
                            ),
                        );
                    }
                    Some(path) => {
                        crate::ui::muted(
                            ui,
                            format!(
                                "The versions {} had before recent saves. Opening one gives a read-only copy; use Save as to keep it.",
                                name_of(path)
                            ),
                        );
                        ui.add_space(6.0);
                        for (file, when) in &backups {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon::FILE_TEXT).color(p.text_muted));
                                // The backup's name carries its UTC time stamp.
                                let stamp = file
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .and_then(|s| s.rsplit('.').next())
                                    .map(|s| crate::files::readable_stamp(&s.replace('Z', "+")))
                                    .unwrap_or_default();
                                ui.label(format!("Saved {}", crate::files::age(*when)))
                                    .on_hover_text(&stamp);
                                // Time of day, so versions saved minutes apart differ.
                                crate::ui::muted(ui, stamp.get(11..).unwrap_or("").to_string());
                                let size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
                                crate::ui::muted(ui, format!("{:.1} KB", size as f64 / 1024.0));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if crate::ui::button(ui, "Open copy").clicked() {
                                        open = Some((file.clone(), *when));
                                    }
                                });
                            });
                        }
                    }
                }
                dialog_footer(ui, Some(("Close", true)), None, |ui| {
                    if let Some(dir) = path.as_deref().and_then(crate::files::backups_folder)
                        && dir.is_dir()
                        && crate::ui::link_button(ui, "Open folder").clicked()
                    {
                        let _ = crate::html_export::open_in_browser(&dir);
                    }
                })
            });
            if let Some((file, when)) = open {
                let name = path.as_deref().map(name_of).unwrap_or_default();
                match std::fs::read(&file)
                    .map_err(|e| e.to_string())
                    .and_then(|b| Document::from_bytes(&b).map_err(|e| e.to_string()))
                {
                    Ok(doc) => {
                        self.add_doc(doc, None);
                        if let Some(ds) = self.docs.last_mut() {
                            ds.read_only = true;
                            ds.title_override =
                                Some(format!("{name} (saved {})", crate::files::age(when)));
                            ds.editor.mark_saved();
                        }
                        self.dialogs.versions = false;
                    }
                    Err(e) => self.toast(format!("Can't open that version: {e}"), true),
                }
            } else if x || res.is_some_and(|r| r.confirmed) {
                self.dialogs.versions = false;
            }
        }

        // The last session crashed.
        if let Some(report) = self.dialogs.crash_report.clone() {
            let (x, res) = Dialog::new("Xylem closed unexpectedly")
                .id("crash")
                .width(460.0)
                .show(ctx, |ui| {
                    ui.label("The last session ended with an error.");
                    crate::ui::muted(
                        ui,
                        "A crash report was saved. Sending it to whoever looks after Xylem helps \
                     get it fixed. Unsaved work is offered for recovery separately.",
                    );
                    dialog_footer(ui, Some(("Close", true)), None, |ui| {
                        if crate::ui::button(ui, "Show report").clicked() {
                            let dir = report.parent().map(Path::to_path_buf).unwrap_or_default();
                            let _ = crate::html_export::open_in_browser(&dir);
                        }
                    })
                });
            if x || res.is_some_and(|r| r.confirmed) {
                self.dialogs.crash_report = None;
            }
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context, cmds: &mut Vec<Cmd>) {
        self.safety_dialogs(ctx);
        use crate::ui::{Dialog, Tone, dialog_footer, form, icon, notice, palette, section};
        self.find_window(ctx);
        self.find_files_window(ctx);
        self.stylesheet_check_window(ctx);
        self.poll_preview(ctx);
        self.stylesheet_references_window(ctx);
        let p = palette(ctx);
        if self.dialogs.about {
            let (x, close) = Dialog::new("About Xylem").width(380.0).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(icon::TREE_STRUCTURE)
                            .size(34.0)
                            .color(p.accent),
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Xylem")
                                .size(20.0)
                                .family(crate::ui::theme::strong_family()),
                        );
                        crate::ui::muted(ui, format!("Version {}", env!("CARGO_PKG_VERSION")));
                    });
                });
                ui.add_space(8.0);
                ui.label("A structured XML and S1000D editor, written in Rust.");
                ui.add_space(4.0);
                crate::ui::muted(
                    ui,
                    "CSS-driven tags-on and normal views, schema-guided editing (XSD and DTD), \
                     CALS tables, tracked changes, BREX checking.",
                );
                ui.add_space(4.0);
                crate::ui::muted(ui, "Released under the MIT licence.");
                dialog_footer(ui, Some(("Close", true)), None, |_| {})
            });
            if x || close.is_some_and(|r| r.confirmed || r.closed) {
                self.dialogs.about = false;
            }
        }
        if let Some(t) = &mut self.dialogs.table {
            let (x, res) = Dialog::new("Insert table").width(380.0).show(ctx, |ui| {
                form(ui, "table-form", |f| {
                    f.row("Rows", |ui| {
                        ui.add(egui::DragValue::new(&mut t.rows).range(1..=200))
                    });
                    f.row("Columns", |ui| {
                        ui.add(egui::DragValue::new(&mut t.cols).range(1..=40))
                    });
                    f.row("Title", |ui| {
                        crate::ui::text_field(ui, "Table title", &mut t.title, "Optional", 220.0)
                    });
                    f.row("Model", |ui| {
                        ui.vertical(|ui| {
                            ui.radio_value(&mut t.cals, true, "CALS (table, tgroup, row, entry)");
                            ui.radio_value(&mut t.cals, false, "HTML (table, tr, td)");
                        })
                    });
                    f.row("", |ui| ui.checkbox(&mut t.header, "Header row"));
                });
                dialog_footer(ui, Some(("Insert table", true)), Some("Cancel"), |_| {})
            });
            let res = res.unwrap_or_default();
            if res.confirmed {
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
                if let Some(Err(e)) = self.insert_markup("table", &xml) {
                    self.toast(e.to_string(), true);
                }
            } else if x || res.closed {
                self.dialogs.table = None;
            }
        }
        if let Some(cd) = &mut self.dialogs.comment {
            let title = if cd.node.is_some() {
                "Edit comment"
            } else {
                "New comment"
            };
            let author = self.settings.author();
            let (x, res) = Dialog::new(title)
                .id("comment")
                .width(420.0)
                .show(ctx, |ui| {
                    crate::ui::muted(ui, format!("As {author}"));
                    ui.add_space(4.0);
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut cd.text)
                            .desired_rows(5)
                            .desired_width(f32::INFINITY)
                            .margin(egui::Margin::symmetric(8, 6))
                            .hint_text("Write a comment"),
                    );
                    crate::ui::widgets::mark_multiline(ui, &r);
                    crate::script::track("Comment text", &r);
                    if std::mem::take(&mut cd.focus) {
                        r.request_focus();
                    }
                    let ctrl_enter = r.has_focus()
                        && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
                    let mut res = dialog_footer(
                        ui,
                        Some(("Save", !cd.text.trim().is_empty() || cd.node.is_some())),
                        Some("Cancel"),
                        |ui| {
                            crate::ui::muted(ui, "Ctrl+Enter to save");
                        },
                    );
                    res.confirmed |= ctrl_enter;
                    res
                });
            let res = res.unwrap_or_default();
            if res.confirmed {
                let (node, text) = (cd.node, cd.text.trim().to_string());
                self.dialogs.comment = None;
                let author = self.settings.author();
                match node {
                    Some(n) => {
                        self.with_editor(|ed, r| ed.set_comment_text(r, n, &text));
                    }
                    None if !text.is_empty() => {
                        self.with_editor(|ed, r| {
                            ed.add_comment(r, &author, &crate::review::now(), &text)
                                .map(|_| ())
                        });
                    }
                    None => {}
                }
            } else if x || res.closed {
                self.dialogs.comment = None;
            }
        }
        if let Some(ne) = &mut self.dialogs.node_edit {
            let title = if ne.target.is_empty() {
                "Edit text"
            } else {
                "Edit processing instruction"
            };
            let (x, res) = Dialog::new(title)
                .id("node-edit")
                .width(440.0)
                .show(ctx, |ui| {
                    if !ne.target.is_empty() {
                        form(ui, "node-form", |f| {
                            f.row("Target", |ui| {
                                crate::ui::text_field(ui, "PI target", &mut ne.target, "", 280.0)
                            });
                        });
                        ui.add_space(4.0);
                    }
                    let r = ui.add(
                        egui::TextEdit::multiline(&mut ne.text)
                            .desired_rows(6)
                            .desired_width(f32::INFINITY)
                            .code_editor()
                            .margin(egui::Margin::symmetric(8, 6)),
                    );
                    crate::ui::widgets::mark_multiline(ui, &r);
                    dialog_footer(ui, Some(("Apply", true)), Some("Cancel"), |_| {})
                });
            let res = res.unwrap_or_default();
            if res.confirmed {
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
            } else if x || res.closed {
                self.dialogs.node_edit = None;
            }
        }
        if let Some(nd) = &mut self.dialogs.new_doc {
            let packages: Vec<(String, String, String)> = std::iter::once((
                "generic".to_string(),
                "Generic XML".to_string(),
                "No schema; any element can go anywhere".to_string(),
            ))
            .chain(self.registry.packages().iter().map(|pk| {
                (
                    pk.doctype.name.clone(),
                    pk.doctype.name.clone(),
                    pk.doctype.description.clone(),
                )
            }))
            .collect();
            let roots: Vec<String> = self
                .registry
                .get(&nd.package)
                .map(|pk| pk.matching.roots.clone())
                .unwrap_or_default();
            let (x, res) = Dialog::new("New document").width(480.0).show(ctx, |ui| {
                section(ui, "Document type");
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0_f32, p.border))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(4))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(240.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (name, title, desc) in &packages {
                                    let row = crate::ui::list_row(
                                        ui,
                                        crate::ui::Row {
                                            icon: Some((
                                                if name == "generic" {
                                                    icon::FILE_CODE
                                                } else {
                                                    icon::FILE_TEXT
                                                },
                                                p.text_muted,
                                            )),
                                            text: title,
                                            detail: Some(desc),
                                            selected: &nd.package == name,
                                            ..Default::default()
                                        },
                                    );
                                    crate::script::track(title, &row.response);
                                    if row.response.clicked() {
                                        nd.package = name.clone();
                                        nd.root.clear();
                                    }
                                }
                            });
                    });
                ui.add_space(8.0);
                form(ui, "newdoc-form", |f| {
                    f.row("Root element", |ui| {
                        ui.horizontal(|ui| {
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
                            crate::ui::text_field(
                                ui,
                                "Root element",
                                &mut nd.root,
                                "or type a name",
                                150.0,
                            );
                        });
                    });
                });
                dialog_footer(ui, Some(("Create", true)), Some("Cancel"), |_| {})
            });
            let res = res.unwrap_or_default();
            if res.confirmed {
                let (pk, r) = (nd.package.clone(), nd.root.clone());
                self.dialogs.new_doc = None;
                self.new_from_package(&pk, Some(&r));
            } else if x || res.closed {
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
            let info = self
                .docs
                .get(self.active)
                .map(|d| crate::s1000d_ui::info_text(&d.editor.doc))
                .unwrap_or_default();
            let (x, res) = Dialog::new("Data module")
                .width(460.0)
                .modeless()
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(p.surface)
                        .stroke(egui::Stroke::new(1.0_f32, p.border))
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(info).monospace());
                        });
                    dialog_footer(ui, Some(("Close", true)), None, |_| {})
                });
            if x || res.is_some_and(|r| r.confirmed) {
                self.dialogs.info = false;
            }
        }
        if let Some(msg) = &self.dialogs.import_result {
            let failed = msg.starts_with("Import failed");
            let (x, res) = Dialog::new("XMAX import").width(540.0).show(ctx, |ui| {
                if failed {
                    notice(
                        ui,
                        Tone::Danger,
                        "The customization could not be converted.",
                    );
                } else {
                    notice(
                        ui,
                        Tone::Success,
                        "Converted. The new doctype package is ready to use.",
                    );
                }
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(p.surface)
                    .stroke(egui::Stroke::new(1.0_f32, p.border))
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(340.0)
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(RichText::new(msg).monospace().size(12.0));
                            });
                    });
                dialog_footer(ui, Some(("Done", true)), None, |_| {})
            });
            if x || res.is_some_and(|r| r.confirmed) {
                self.dialogs.import_result = None;
            }
        }
        if self.dialogs.prefs {
            let mut reload = false;
            let mut theme_choice: Option<Option<bool>> = None;
            let (x, res) = Dialog::new("Preferences").width(560.0).show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() - 220.0).max(240.0))
                    .show(ui, |ui| {
                        section(ui, "Appearance");
                        form(ui, "prefs-look", |f| {
                            f.row("Theme", |ui| {
                                let mut t = self.settings.dark_mode;
                                if crate::ui::segmented(
                                    ui,
                                    &mut t,
                                    &[(None, "System"), (Some(false), "Light"), (Some(true), "Dark")],
                                ) {
                                    theme_choice = Some(t);
                                }
                            });
                            f.row("Open documents in", |ui| {
                                crate::ui::segmented(
                                    ui,
                                    &mut self.settings.default_view,
                                    &[(View::TagsOn, "Tags on"), (View::Normal, "Normal")],
                                );
                            });
                        });
                        crate::ui::divider(ui);
                        section(ui, "Editing");
                        form(ui, "prefs-edit", |f| {
                            f.row("Validation", |ui| {
                                ui.checkbox(&mut self.settings.auto_validate, "Validate as you type")
                            });
                            f.row("Spelling", |ui| {
                                let mut on = self.settings.spell_check;
                                if ui.checkbox(&mut on, "Check spelling").clicked() {
                                    cmds.push(Cmd::SpellCheck);
                                }
                            });
                            let mut spell_reload = false;
                            f.row("Dictionaries", |ui| {
                                path_pref(ui, &mut self.settings.dictionary_dir, true, &mut spell_reload)
                            });
                            f.row("Language", |ui| {
                                if crate::ui::text_field(
                                    ui,
                                    "Dictionary language",
                                    &mut self.settings.dictionary_lang,
                                    "en_US",
                                    120.0,
                                )
                                .lost_focus()
                                {
                                    spell_reload = true;
                                }
                            });
                            if spell_reload {
                                self.speller = None;
                                reload = true;
                            }
                        });
                        crate::ui::divider(ui);
                        section(ui, "Files");
                        form(ui, "prefs-files", |f| {
                            f.row("On start", |ui| {
                                ui.checkbox(
                                    &mut self.settings.restore_session,
                                    "Reopen the documents from last time",
                                )
                            });
                            f.row_with_hint(
                                "Autosave",
                                "Seconds between recovery copies of unsaved work; 0 turns it off.",
                                |ui| {
                                    ui.add(
                                        egui::DragValue::new(&mut self.settings.autosave_seconds)
                                            .range(0..=3600)
                                            .suffix(" s"),
                                    )
                                },
                            );
                        });
                        crate::ui::divider(ui);
                        section(ui, "Review");
                        form(ui, "prefs-review", |f| {
                            f.row_with_hint(
                                "Author",
                                "Recorded on tracked changes and comments. Empty means your login name.",
                                |ui| {
                                    let hint = crate::review::os_user_name();
                                    crate::ui::text_field(
                                        ui,
                                        "Review author",
                                        &mut self.settings.review_author,
                                        &hint,
                                        240.0,
                                    )
                                },
                            );
                        });
                        crate::ui::divider(ui);
                        section(ui, "Stylesheets");
                        let mut unused = false;
                        form(ui, "prefs-stylesheets", |f| {
                            f.row_with_hint(
                                "Engine",
                                "The program that runs stylesheets (SaxonDotnetHost.exe, see tools/saxon-dotnet), or builtin. Used before the team's; Xylem's own engine when neither is set.",
                                |ui| path_pref(ui, &mut self.settings.xslt_host, false, &mut unused),
                            );
                        });
                        if let Some(team_host) = &self.team.xslt_host
                            && self.settings.xslt_host.is_none()
                        {
                            crate::ui::muted(ui, format!("Team engine: {}", team_host.display()));
                        } else if self.settings.xslt_host.is_none() {
                            crate::ui::muted(ui, "Using Xylem's built-in engine.");
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new("Publication previews").strong());
                        crate::ui::muted(
                            ui,
                            "Your own previews, tried before the team's. The first whose root and schema match the document is used.",
                        );
                        let mut remove_preview = None;
                        for (i, pv) in self.settings.preview.iter_mut().enumerate() {
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                form(ui, &format!("prefs-preview-{i}"), |f| {
                                    f.row("Name", |ui| {
                                        crate::ui::text_field(ui, "Preview name", &mut pv.name, "Manual", 260.0);
                                        if crate::ui::icon_button(ui, icon::X, "Remove this preview", None).clicked() {
                                            remove_preview = Some(i);
                                        }
                                    });
                                    f.row_with_hint("Root", "Document element; any when empty", |ui| {
                                        crate::ui::text_field(ui, "Preview root", &mut pv.root, "publication", 260.0)
                                    });
                                    f.row_with_hint(
                                        "Schema",
                                        "Optional: the file name of the schema the document points to; * matches anything",
                                        |ui| {
                                            crate::ui::text_field(
                                                ui,
                                                "Preview schema",
                                                &mut pv.schema_location,
                                                "*Front_Matter*.xsd",
                                                260.0,
                                            )
                                        },
                                    );
                                    f.row("Stylesheet", |ui| {
                                        path_pref(ui, &mut pv.stylesheet, false, &mut unused)
                                    });
                                    f.row_with_hint("HTML step", "Optional: turns the output (XSL-FO) into HTML", |ui| {
                                        path_pref(ui, &mut pv.html, false, &mut unused)
                                    });
                                    f.row_with_hint("Parameters", "name=value, separated by commas", |ui| {
                                        crate::ui::text_field(
                                            ui,
                                            "Preview parameters",
                                            &mut pv.params,
                                            "mode=html",
                                            260.0,
                                        )
                                    });
                                });
                            });
                        }
                        if let Some(i) = remove_preview {
                            self.settings.preview.remove(i);
                        }
                        if crate::ui::button(ui, "Add preview").clicked() {
                            self.settings.preview.push(Default::default());
                        }
                        crate::ui::divider(ui);
                        section(ui, "Document types");
                        match &self.team.source {
                            Some(src) => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(icon::FOLDERS).color(p.text_muted));
                                    ui.label("Team configuration");
                                });
                                crate::ui::muted(ui, src.display().to_string());
                            }
                            None => {
                                crate::ui::muted(
                                    ui,
                                    "No team configuration (xylem-team.toml). See docs/team-setup.md.",
                                );
                            }
                        }
                        for w in &self.team.warnings {
                            notice(ui, Tone::Warning, w.clone());
                        }
                        ui.add_space(6.0);
                        let mut remove = None;
                        let team_dirs = self.team.package_dirs.clone();
                        for d in team_dirs.iter() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon::FOLDER_SIMPLE).color(p.text_faint));
                                ui.label(d.display().to_string());
                                crate::ui::badge(ui, "team", Tone::Neutral);
                            });
                        }
                        for (i, d) in self.settings.package_dirs.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(icon::FOLDER_SIMPLE).color(p.text_muted));
                                ui.label(d.display().to_string());
                                if crate::ui::icon_button(ui, icon::X, "Remove folder", None).clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            self.settings.package_dirs.remove(i);
                            reload = true;
                        }
                        if crate::ui::button(ui, "Add folder…").clicked()
                            && let Some(pth) = crate::script::file_dialog(|| rfd::FileDialog::new().pick_folder())
                        {
                            self.settings.package_dirs.push(pth);
                            reload = true;
                        }
                        crate::ui::divider(ui);
                        section(ui, "Schemas");
                        form(ui, "prefs-schemas", |f| {
                            f.row_with_hint(
                                "Catalog",
                                "catalog.toml or an OASIS XML catalog",
                                |ui| path_pref(ui, &mut self.settings.catalog, false, &mut reload),
                            );
                            f.row_with_hint(
                                "S1000D schemas",
                                "A folder with one sub-folder per issue (S1000D_4-1, …)",
                                |ui| path_pref(ui, &mut self.settings.s1000d_schema_root, true, &mut reload),
                            );
                            f.row("Project BREX", |ui| {
                                path_pref(ui, &mut self.settings.brex, false, &mut reload)
                            });
                        });
                        ui.add_space(6.0);
                        ui.label(RichText::new("Schema locations").strong());
                        crate::ui::muted(
                            ui,
                            "Documents that point to a schema on a server are validated against a local copy: a location (or the start of one, ending in /) and the file or folder to read instead.",
                        );
                        for (url, local) in &self.team.schema_locations {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("{url}  →  {}", local.display())).color(p.text_muted));
                                crate::ui::badge(ui, "team", Tone::Neutral);
                            });
                        }
                        let mut remove_location = None;
                        for (i, (url, local)) in self.settings.schema_locations.iter_mut().enumerate() {
                            ui.horizontal(|ui| {
                                if crate::ui::text_field(ui, "Schema location", url, "https://server/schemas/", 230.0)
                                    .lost_focus()
                                {
                                    reload = true;
                                }
                                let mut path = Some(local.clone()).filter(|p| !p.as_os_str().is_empty());
                                let mut changed = false;
                                path_pref(ui, &mut path, url.trim_end().ends_with('/'), &mut changed);
                                if changed {
                                    *local = path.unwrap_or_default();
                                    reload = true;
                                }
                                if crate::ui::icon_button(ui, icon::X, "Remove this location", None).clicked() {
                                    remove_location = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove_location {
                            self.settings.schema_locations.remove(i);
                            reload = true;
                        }
                        if crate::ui::button(ui, "Add location").clicked() {
                            self.settings.schema_locations.push(Default::default());
                        }
                    });
                dialog_footer(ui, Some(("Done", true)), None, |_| {})
            });
            if let Some(t) = theme_choice {
                self.settings.dark_mode = t;
                let dark = t.unwrap_or_else(|| ctx.system_theme() == Some(egui::Theme::Dark));
                crate::ui::theme::apply(ctx, dark);
            }
            if x || res.is_some_and(|r| r.confirmed) {
                self.dialogs.prefs = false;
            }
            if reload {
                cmds.push(Cmd::ReloadPackages);
                self.load_speller();
            }
        }
        if let Some(i) = self.dialogs.confirm_close {
            let title = self.docs.get(i).map(|d| d.title()).unwrap_or_default();
            let title = title.trim_end_matches(" •").to_string();
            let mut choice = None;
            let (x, _) = Dialog::new("Unsaved changes").width(420.0).show(ctx, |ui| {
                ui.label(format!("Save the changes to {title} before closing?"));
                crate::ui::muted(ui, "If you don't save, your changes are lost.");
                let r = dialog_footer(ui, Some(("Save", true)), Some("Cancel"), |ui| {
                    if crate::ui::button(ui, "Don't save").clicked() {
                        choice = Some(1);
                    }
                });
                if r.confirmed {
                    choice = Some(0);
                } else if r.closed {
                    choice = Some(2);
                }
            });
            if x {
                choice = Some(2);
            }
            match choice {
                Some(0) => {
                    self.active = i;
                    self.save(false);
                    if !self.docs[i].is_dirty() {
                        crate::autosave::discard(self.docs[i].id_key());
                        self.docs[i].forget_recovery();
                        self.docs.remove(i);
                        self.active = self.active.min(self.docs.len().saturating_sub(1));
                    }
                    self.dialogs.confirm_close = None;
                }
                Some(1) => {
                    crate::autosave::discard(self.docs[i].id_key());
                    self.docs[i].forget_recovery();
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
            let n = self.docs.iter().filter(|d| d.is_dirty()).count();
            let (x, _) = Dialog::new("Quit Xylem").width(420.0).show(ctx, |ui| {
                ui.label(if n == 1 {
                    "A document has unsaved changes.".to_string()
                } else {
                    format!("{n} documents have unsaved changes.")
                });
                crate::ui::muted(ui, "Quit anyway and lose them?");
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::ui::danger_button(ui, "Quit without saving").clicked() {
                            choice = Some(true);
                        }
                        if crate::ui::button(ui, "Cancel").clicked() {
                            choice = Some(false);
                        }
                    });
                });
            });
            if x {
                choice = Some(false);
            }
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
                                if ed.is_tracking() {
                                    // A tracked replacement keeps the text in
                                    // front of the match in `node`, so the
                                    // earlier matches' ranges stay valid.
                                    if ed.doc.text(node).is_some_and(|t| range.end <= t.len()) {
                                        ed.replace_text_range(
                                            r,
                                            node,
                                            range.start,
                                            range.end,
                                            &with,
                                        )?;
                                    }
                                    continue;
                                }
                                // Keeps the rest of the node's source as written.
                                ed.doc.replace_text(node, range, &with);
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
        let p = crate::ui::palette(ctx);
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-16.0, -40.0])
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                for (msg, age, err) in &self.toasts {
                    // Fade out over the last half second.
                    let alpha = ((6.0 - *age) / 0.5).clamp(0.0, 1.0) as f32;
                    let (glyph, color) = if *err {
                        (crate::ui::icon::WARNING_CIRCLE, p.danger)
                    } else {
                        (crate::ui::icon::INFO, p.accent)
                    };
                    ui.set_opacity(alpha);
                    egui::Frame::new()
                        .fill(p.raised)
                        .stroke(egui::Stroke::new(1.0_f32, p.border))
                        .corner_radius(egui::CornerRadius::same(8))
                        .shadow(ui.style().visuals.popup_shadow)
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 14,
                            top: 9,
                            bottom: 9,
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(420.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(glyph).size(16.0).color(color));
                                ui.add(egui::Label::new(RichText::new(msg).color(p.text)).wrap());
                            });
                        });
                    ui.add_space(6.0);
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
            script.anchor = self
                .docs
                .get(self.active)
                .and_then(|d| d.canvas.caret_screen);
        }
        let script_cmds = self
            .script
            .as_mut()
            .map(|s| std::mem::take(&mut s.pending_cmds))
            .unwrap_or_default();
        for (name, arg) in script_cmds {
            match Cmd::from_script(&name, &arg) {
                Some(c) => self.run(ctx, c),
                None => log::warn!("script: unknown command `{name}`"),
            }
        }
        if let Some((text, at)) = self.script.as_mut().and_then(|s| s.pending_caret.take()) {
            self.script_caret(&text, at);
        }
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
                let errors = ds
                    .issues
                    .iter()
                    .filter(|i| i.severity == Severity::Error)
                    .count();
                eprintln!(
                    "script save {}: {} issue(s), {} error(s), dirty={}",
                    p.display(),
                    ds.issues.len(),
                    errors,
                    ds.is_dirty()
                );
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

/// The quick insert/surround popup (Ctrl+Space).
struct Palette {
    query: String,
    selected: usize,
    /// Wrap the selection instead of inserting at the caret.
    surround: bool,
}

impl XylemApp {
    /// Element names offered at the caret: valid ones first.
    fn palette_items(&self, surround: bool) -> Vec<(String, bool)> {
        let Some(ds) = self.docs.get(self.active) else {
            return Vec::new();
        };
        let doc = &ds.editor.doc;
        let Some(g) = ds.grammar.as_deref() else {
            // No schema: offer the element names the document already uses.
            let mut names: Vec<String> = doc
                .descendants(doc.root())
                .filter_map(|n| doc.name(n).map(str::to_string))
                .collect();
            names.sort();
            names.dedup();
            return names.into_iter().map(|n| (n, true)).collect();
        };
        let mut items: Vec<(String, bool)> = if surround {
            let (a, _) = ds.editor.sel.ordered(doc);
            let parent = container(doc, a);
            g.all_element_names()
                .into_iter()
                .map(|n| {
                    let ok = g.can_insert(doc, parent, 0, &n);
                    (n, ok)
                })
                .collect()
        } else {
            let (mut parent, mut index) = ds.editor.insertion_point();
            let mut v: Vec<(String, bool)> = Vec::new();
            // Valid at the caret first, then after each enclosing element.
            loop {
                for i in g.insertable_at(doc, parent, index) {
                    if !v.iter().any(|(n, _)| *n == i.name) {
                        v.push((i.name, i.keeps_valid));
                    }
                }
                let Some(up) = doc.parent(parent).filter(|&p| doc.is_element(p)) else {
                    break;
                };
                index = doc.index_in_parent(parent).unwrap_or(0) + 1;
                parent = up;
            }
            v
        };
        items.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });
        items
    }

    fn show_palette(&mut self, ctx: &egui::Context, cmds: &mut Vec<Cmd>) {
        let Some(pal) = &self.palette else { return };
        let surround = pal.surround;
        let q = pal.query.to_lowercase();
        let items: Vec<(String, bool)> = self
            .palette_items(surround)
            .into_iter()
            .filter(|(n, _)| q.is_empty() || n.to_lowercase().contains(&q))
            .collect();
        let anchor = self
            .docs
            .get(self.active)
            .and_then(|d| d.canvas.caret_screen)
            .map(|r| r.left_bottom() + egui::vec2(0.0, 4.0))
            .unwrap_or(egui::pos2(300.0, 200.0));
        let mut close = false;
        let mut chosen: Option<String> = None;
        let pal = self.palette.as_mut().unwrap();
        pal.selected = pal.selected.min(items.len().saturating_sub(1));
        egui::Area::new(egui::Id::new("xylem-palette"))
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let p = crate::ui::palette(ui.ctx());
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(280.0);
                        ui.label(
                            RichText::new(if surround {
                                "Surround the selection with"
                            } else {
                                "Insert an element"
                            })
                            .size(crate::ui::theme::size::SMALL)
                            .color(p.text_muted),
                        );
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut pal.query)
                                .hint_text(format!(
                                    "{}  Type to filter",
                                    crate::ui::icon::MAGNIFYING_GLASS
                                ))
                                .margin(egui::Margin::symmetric(8, 5))
                                .desired_width(f32::INFINITY),
                        );
                        edit.request_focus();
                        if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
                            pal.selected = (pal.selected + 1).min(items.len().saturating_sub(1));
                        }
                        if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
                            pal.selected = pal.selected.saturating_sub(1);
                        }
                        if ui.input(|i| i.key_pressed(Key::Escape)) {
                            close = true;
                        }
                        if ui.input(|i| i.key_pressed(Key::Enter)) {
                            chosen = items.get(pal.selected).map(|i| i.0.clone());
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                if items.is_empty() {
                                    crate::ui::muted(ui, "Nothing matches here.");
                                }
                                for (k, (name, valid)) in items.iter().enumerate() {
                                    let row = crate::ui::list_row(
                                        ui,
                                        crate::ui::Row {
                                            text: name,
                                            text_color: Some(if *valid {
                                                p.text
                                            } else {
                                                p.text_faint
                                            }),
                                            selected: k == pal.selected,
                                            ..Default::default()
                                        },
                                    );
                                    if k == pal.selected {
                                        row.response.scroll_to_me(None);
                                    }
                                    if row.response.clicked() {
                                        chosen = Some(name.clone());
                                    }
                                }
                            });
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "\u{2191}\u{2193} to choose · Enter to insert · Esc to close",
                            )
                            .size(crate::ui::theme::size::SMALL)
                            .color(p.text_faint),
                        );
                    });
            });
        if let Some(name) = chosen {
            cmds.push(if surround {
                Cmd::Surround(name)
            } else {
                Cmd::InsertElement(name)
            });
            close = true;
        }
        if close {
            self.palette = None;
            if let Some(ds) = self.docs.get_mut(self.active) {
                ds.canvas.request_focus = true;
            }
        }
    }

    fn show_shortcuts(&mut self, ctx: &egui::Context) {
        if !self.shortcuts_open {
            return;
        }
        // Grouped like the menus. Keys separated by " / " are alternatives.
        const GROUPS: &[(&str, &[(&str, &str)])] = &[
            (
                "Editing",
                &[
                    ("Ctrl+Space", "Insert an element, or surround the selection"),
                    ("Enter", "Split the element, or start the one that follows"),
                    ("Shift+Enter", "Line break inside the text"),
                    ("Ctrl+B / Ctrl+I / Ctrl+U", "Bold, italic, underline"),
                    ("Ctrl+E", "Select the current element"),
                    ("Tab / Shift+Tab", "Next or previous table cell"),
                    ("Ctrl+Z / Ctrl+Y", "Undo, redo"),
                    ("Ctrl+Shift+V", "Paste as plain text"),
                ],
            ),
            (
                "Navigating",
                &[
                    ("Ctrl+click", "Follow a link or reference"),
                    ("Double-click", "Edit an element's attributes (on a tag)"),
                    (
                        "Ctrl+Shift+- / Ctrl+Shift+=",
                        "Collapse or expand the element",
                    ),
                    ("Ctrl+F", "Find and replace"),
                    ("F3 / Shift+F3", "Find next, find previous"),
                    ("Ctrl+Shift+F", "Find in files"),
                    ("Ctrl+Shift+F5", "Publication preview"),
                    ("F12", "Stylesheet references (in a stylesheet)"),
                ],
            ),
            (
                "View",
                &[
                    ("F5 / F6 / F8", "Tags on, Normal, Source"),
                    ("F7", "Validate"),
                    ("Ctrl++ / Ctrl+- / Ctrl+0", "Zoom in, out, reset"),
                ],
            ),
            (
                "Review",
                &[
                    ("Ctrl+Shift+T", "Track changes on or off"),
                    ("Ctrl+Shift+M", "New comment"),
                    (
                        "Ctrl+Shift+A / Ctrl+Shift+R",
                        "Accept or reject all changes",
                    ),
                ],
            ),
            (
                "Files",
                &[
                    ("Ctrl+N / Ctrl+O", "New, open"),
                    ("Ctrl+S / Ctrl+Shift+S", "Save, save as"),
                    ("Ctrl+W", "Close"),
                    ("Ctrl+,", "Preferences"),
                ],
            ),
        ];
        let (x, res) = crate::ui::Dialog::new("Keyboard shortcuts")
            .width(560.0)
            .modeless()
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() - 200.0).max(240.0))
                    .show(ui, |ui| {
                        for (gi, (group, keys)) in GROUPS.iter().enumerate() {
                            if gi > 0 {
                                ui.add_space(6.0);
                            }
                            crate::ui::section(ui, group);
                            egui::Grid::new(("keys", gi))
                                .num_columns(2)
                                .spacing([16.0, 6.0])
                                .min_col_width(220.0)
                                .show(ui, |ui| {
                                    for (k, what) in keys.iter() {
                                        crate::ui::kbd(ui, k);
                                        ui.label(*what);
                                        ui.end_row();
                                    }
                                });
                        }
                    });
                crate::ui::dialog_footer(ui, Some(("Close", true)), None, |_| {})
            });
        if x || res.is_some_and(|r| r.confirmed) {
            self.shortcuts_open = false;
        }
    }

    fn show_recovery(&mut self, ctx: &egui::Context) {
        if self.recovery.is_empty() {
            return;
        }
        let mut open_items = Vec::new();
        let mut discard_all = false;
        let mut later_clicked = false;
        let p = crate::ui::palette(ctx);
        // Closing keeps the copies for next time; only Discard all deletes them.
        let (later, _) = crate::ui::Dialog::new("Recover unsaved work")
            .width(480.0)
            .show(ctx, |ui| {
                crate::ui::notice(
                    ui,
                    crate::ui::Tone::Warning,
                    "Xylem closed without saving these documents.",
                );
                ui.add_space(8.0);
                for (k, r) in self.recovery.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(crate::ui::icon::FILE_TEXT).color(p.text_muted));
                        ui.add(egui::Label::new(r.describe()).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if crate::ui::primary_button(ui, "Recover").clicked() {
                                open_items.push(k);
                            }
                        });
                    });
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::ui::button(ui, "Later")
                            .on_hover_text("Keep the copies and ask again next time")
                            .clicked()
                        {
                            later_clicked = true;
                        }
                        if crate::ui::danger_button(ui, "Discard all")
                            .on_hover_text("Delete the recovered copies. This cannot be undone.")
                            .clicked()
                        {
                            discard_all = true;
                        }
                    });
                });
            });
        for k in open_items.into_iter().rev() {
            let r = self.recovery.remove(k);
            let doc = match r.load() {
                Ok(doc) => doc,
                Err(e) => {
                    // Keep the copy: it may still be readable by hand.
                    self.toast(
                        format!(
                            "Could not open the recovered copy ({e}). It is kept at {}.",
                            r.file.display()
                        ),
                        true,
                    );
                    continue;
                }
            };
            // If the original is already open (session restore), put the
            // recovered content into that tab, so undo goes back to the saved file.
            let open_tab = r.original.as_ref().and_then(|p| {
                self.docs
                    .iter()
                    .position(|d| d.path.as_deref() == Some(p.as_path()))
            });
            match open_tab {
                Some(i) => {
                    let ds = &mut self.docs[i];
                    ds.editor.replace_document(doc);
                    ds.canvas.invalidate();
                    ds.recovered_from = Some(r.file.clone());
                    self.active = i;
                }
                None => {
                    self.add_doc(doc, r.original.clone());
                    if let Some(ds) = self.docs.last_mut() {
                        ds.recovered_from = Some(r.file.clone());
                    }
                }
            }
            self.toast(
                "Recovered unsaved work. Save the document to keep it.",
                false,
            );
        }
        if discard_all {
            for r in self.recovery.drain(..) {
                r.remove();
            }
        } else if later || later_clicked {
            // The copies stay in the autosave folder and are offered again
            // at the next start.
            self.recovery.clear();
        }
    }

    /// Writes unsaved documents to the autosave folder every few seconds.
    fn autosave_tick(&mut self, ctx: &egui::Context) {
        let isolated = std::env::var_os("XYLEM_CONFIG_DIR").is_some();
        if self.settings.autosave_seconds == 0
            || ((self.script.is_some() || self.screenshot.is_some()) && !isolated)
        {
            return;
        }
        let now = ctx.input(|i| i.time);
        if now - self.last_autosave < self.settings.autosave_seconds as f64 {
            return;
        }
        self.last_autosave = now;
        for ds in &self.docs {
            if ds.editor.is_dirty() {
                crate::autosave::write(ds.id_key(), ds.path.as_deref(), &ds.editor.doc);
            }
        }
    }
}

/// Team package folders first, then the user's own.
fn all_package_dirs(team: &crate::team::TeamConfig, settings: &Settings) -> Vec<PathBuf> {
    let mut dirs = team.package_dirs.clone();
    for d in &settings.package_dirs {
        if !dirs.contains(d) {
            dirs.push(d.clone());
        }
    }
    dirs
}

/// Progress of Help › Check for updates.
enum UpdateState {
    Idle,
    Checking {
        rx: std::sync::mpsc::Receiver<Result<Option<crate::install::Available>, String>>,
        quiet: bool,
    },
    UpToDate,
    Found(crate::install::Available),
    Applied(PathBuf),
    Failed(String),
}

/// The frame shared by the side panels.
fn side_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::new()
        .fill(crate::ui::palette(ctx).panel)
        .inner_margin(egui::Margin {
            left: 12,
            right: 10,
            top: 10,
            bottom: 8,
        })
}

/// Icon for a toolbar button a doctype package defines, by its label.
fn toolbar_icon_for(label: &str) -> Option<&'static str> {
    use crate::ui::icon;
    let l = label.to_ascii_lowercase();
    Some(if l.contains("superscript") {
        icon::TEXT_SUPERSCRIPT
    } else if l.contains("subscript") {
        icon::TEXT_SUBSCRIPT
    } else if l.contains("numbered") || l.contains("ordered list") {
        icon::LIST_NUMBERS
    } else if l.contains("definition") {
        icon::LIST_DASHES
    } else if l.contains("list") {
        icon::LIST_BULLETS
    } else if l.contains("paragraph") || l == "para" {
        icon::PARAGRAPH
    } else if l.contains("image") || l.contains("graphic") || l.contains("figure") {
        icon::IMAGE
    } else if l.contains("table") {
        icon::TABLE
    } else if l.contains("link") || l.contains("reference") {
        icon::LINK
    } else {
        return None;
    })
}

fn table_menu(ui: &mut egui::Ui, cmds: &mut Vec<Cmd>) {
    use crate::ui::icon;
    let groups: [&[(Option<&str>, &str, Cmd)]; 4] = [
        &[(Some(icon::TABLE), "Insert table…", Cmd::InsertTable)],
        &[
            (Some(icon::ROWS), "Insert row above", Cmd::TableRowAbove),
            (None, "Insert row below", Cmd::TableRowBelow),
            (Some(icon::COLUMNS), "Insert column left", Cmd::TableColLeft),
            (None, "Insert column right", Cmd::TableColRight),
            (None, "Move row up", Cmd::TableRowUp),
            (None, "Move row down", Cmd::TableRowDown),
        ],
        &[
            (None, "Merge cell right", Cmd::TableMergeRight),
            (None, "Merge cell down", Cmd::TableMergeDown),
            (None, "Split cell", Cmd::TableSplitCell),
        ],
        &[
            (Some(icon::TRASH), "Delete row", Cmd::TableDeleteRow),
            (None, "Delete column", Cmd::TableDeleteColumn),
            (None, "Delete table", Cmd::TableDelete),
        ],
    ];
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        for (glyph, label, c) in group.iter() {
            if crate::ui::menu_item(ui, *glyph, label, None).clicked() {
                cmds.push(c.clone());
                ui.close();
            }
        }
    }
}

/// A file or folder setting: the current path, a Choose button and a
/// clear button.
fn path_pref(ui: &mut egui::Ui, value: &mut Option<PathBuf>, folder: bool, changed: &mut bool) {
    use crate::ui::{icon, palette};
    let p = palette(ui.ctx());
    ui.horizontal(|ui| {
        let text = value
            .as_ref()
            .map(|v| v.display().to_string())
            .unwrap_or_else(|| "Not set".into());
        let color = if value.is_some() {
            p.text
        } else {
            p.text_faint
        };
        ui.add(egui::Label::new(RichText::new(&text).color(color)).truncate())
            .on_hover_text(&text);
        if crate::ui::button(ui, "Choose…").clicked() {
            let picked = crate::script::file_dialog(|| {
                if folder {
                    rfd::FileDialog::new().pick_folder()
                } else {
                    rfd::FileDialog::new().pick_file()
                }
            });
            if let Some(path) = picked {
                *value = Some(path);
                *changed = true;
            }
        }
        if value.is_some() && crate::ui::icon_button(ui, icon::X, "Clear", None).clicked() {
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

/// Markup for an element that may be inserted at the caret and contains the
/// image element (from the package template or the schema's required
/// content), with the image's source attribute filled in.
fn image_container_xml(
    ds: &DocState,
    image: &str,
    attr: &str,
    value: &str,
) -> Option<(String, String)> {
    let g = ds.grammar.as_deref()?;
    let doc = &ds.editor.doc;
    // Candidate positions: the caret, then after each enclosing element.
    let (mut parent, mut index) = ds.editor.insertion_point();
    let mut candidates = Vec::new();
    loop {
        candidates.extend(
            g.insertable_at(doc, parent, index)
                .into_iter()
                .filter(|i| i.keeps_valid)
                .map(|i| (i.name, i.decl)),
        );
        let Some(up) = doc.parent(parent).filter(|&p| doc.is_element(p)) else {
            break;
        };
        index = doc.index_in_parent(parent).unwrap_or(0) + 1;
        parent = up;
    }
    for (name, decl) in candidates {
        let xml = ds
            .package
            .template(&name)
            .map(|t| t.xml.clone())
            .filter(|x| x.contains(&format!("<{image}")))
            .unwrap_or_else(|| crate::doc::skeleton_xml(&g.required_skeleton(decl)));
        let Ok(mut frag) = xylem_dom::parse_fragment(&xml) else {
            continue;
        };
        let target = frag
            .descendants(frag.root())
            .find(|&n| frag.name(n) == Some(image));
        let Some(target) = target else { continue };
        frag.set_attr(target, attr, value);
        let out: String = frag
            .children(frag.root())
            .iter()
            .map(|&c| frag.node_to_xml(c))
            .collect();
        return Some((name, out));
    }
    None
}

fn is_image_file(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tif" | "tiff" | "svg" | "cgm" | "webp"
        )
    })
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
            s.ui_rects = crate::script::begin_frame();
            s.feed(raw);
        }
    }

    fn persist_egui_memory(&self) -> bool {
        // Scripted test runs must start from a clean UI state every time.
        self.script.is_none() && self.screenshot.is_none()
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Test runs must not change the user's settings, but may change an
        // isolated config folder (XYLEM_CONFIG_DIR), which is how session
        // restore and preference persistence are tested.
        let isolated = std::env::var_os("XYLEM_CONFIG_DIR").is_some();
        if (self.script.is_some() || self.screenshot.is_some()) && !isolated {
            return;
        }
        self.settings.last_session = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        eframe::set_value(storage, "settings", &self.settings);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let frame_start = std::time::Instant::now();
        crate::ui::widgets::begin_frame(ctx);
        self.sync_locks();
        self.poll_disk(ctx);
        self.check_scripts(ctx);
        self.poll_update_check();
        self.update_dialog(ctx);
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
            if is_image_file(&p) && self.insert_image_file(&p) {
                continue;
            }
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
        let author = self.settings.author();
        for d in &mut self.docs {
            if let Some(t) = &mut d.editor.tracking
                && t.author != author
            {
                t.author = author.clone();
            }
        }
        let mut cmds = self.shortcuts(ctx);
        let title = self
            .docs
            .get(self.active)
            .map(|d| format!("{} — Xylem", d.title()))
            .unwrap_or_else(|| "Xylem".into());
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        let pal = crate::ui::palette(ctx);
        egui::TopBottomPanel::top("menu")
            .frame(
                egui::Frame::new()
                    .fill(pal.chrome)
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 6,
                        top: 3,
                        bottom: 0,
                    }),
            )
            .show_separator_line(false)
            .show(ctx, |ui| self.menu_bar(ui, &mut cmds));
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(pal.chrome)
                    .inner_margin(egui::Margin::symmetric(8, 5)),
            )
            .show(ctx, |ui| self.toolbar(ui, &mut cmds));
        self.status_bar(ctx);
        self.panels(ctx, &mut cmds);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.surface))
            .show(ctx, |ui| {
                if self.docs.is_empty() {
                    self.welcome(ui, &mut cmds);
                    return;
                }
                let strip = egui::Frame::new()
                    .fill(pal.panel)
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 6,
                        top: 4,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.tabs(ui, &mut cmds);
                    });
                let r = strip.response.rect;
                ui.painter().hline(
                    r.x_range(),
                    r.bottom(),
                    egui::Stroke::new(1.0_f32, pal.border),
                );
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 0,
                        top: 2,
                        bottom: 0,
                    })
                    .show(ui, |ui| self.document_area(ui, &mut cmds));
            });
        self.show_context_menu(ctx, &mut cmds);
        self.show_palette(ctx, &mut cmds);
        self.show_shortcuts(ctx);
        self.show_recovery(ctx);
        self.autosave_tick(ctx);
        self.dialogs(ctx, &mut cmds);
        if self.applic_dialog.open {
            let ds = self.docs.get_mut(self.active);
            crate::s1000d_ui::applic_window(ctx, &mut self.applic_dialog, ds);
        }
        self.show_toasts(ctx);
        for c in cmds {
            self.run(ctx, c);
        }
        let ms = frame_start.elapsed().as_secs_f64() * 1000.0;
        if ms > 25.0 {
            log::debug!("slow frame: {ms:.1} ms");
        }
        // A script repaints every frame; cap it near 30 fps so test runs
        // don't keep a CPU core and the GPU busy. (After the timing above,
        // so slow-frame logs measure real work.)
        if self.script.is_some() {
            std::thread::sleep(std::time::Duration::from_millis(30));
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
