//! S1000D integration: new data module wizard, BREX, CSDB naming, references.

use crate::doc::{DocState, Issue, Severity};
use crate::view::style::ApplicDisplay;
use egui::{Color32, RichText};
use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId};
use xylem_s1000d::{self as s1000d, Dmc, IssueDate, SchemaKind};

/// Maps `http://www.s1000d.org/S1000D_x-y/...` schema URLs to a local folder
/// laid out as `<root>/S1000D_x-y/...` (or `<root>/x.y/...`).
pub fn add_s1000d_catalog(cat: &mut xylem_schema::Catalog, root: &Path) {
    for issue in s1000d::Issue::ALL {
        let dir = format!("S1000D_{}", issue.schema_token());
        // Accept either the official folder name or the plain issue number.
        for folder in [root.join(&dir), root.join(issue.as_str())] {
            if folder.is_dir() {
                for scheme in ["http", "https"] {
                    cat.add_prefix(&format!("{scheme}://www.s1000d.org/{dir}/"), folder.clone());
                }
                break;
            }
        }
    }
}

pub fn suggested_filename(doc: &Document) -> Option<String> {
    s1000d::DmIdent::from_document(doc)
        .and_then(|d| d.csdb_filename())
        .or_else(|| s1000d::PmIdent::from_document(doc).and_then(|p| p.csdb_filename()))
}

pub fn run_brex(doc: &Document, path: &Path) -> Result<Vec<Issue>, String> {
    let brex = s1000d::Brex::load(path).map_err(|e| e.to_string())?;
    if brex.rules.is_empty() {
        return Err(format!("{} contains no structure rules.", path.display()));
    }
    Ok(s1000d::check_brex(doc, &brex)
        .into_iter()
        .map(|v| Issue {
            node: v.node,
            severity: match v.severity {
                s1000d::Severity::Error => Severity::Error,
                _ => Severity::Warning,
            },
            message: format!("{}: {}", v.rule_id, v.message),
            source: "BREX",
        })
        .collect())
}

/// Resolves a dmRef/pmRef at or around `n` to a file in `dir`.
pub fn resolve_ref(doc: &Document, n: NodeId, dir: Option<&Path>) -> Option<PathBuf> {
    let dir = dir?;
    let chain: Vec<NodeId> = std::iter::once(n).chain(doc.ancestors(n)).collect();
    for r in s1000d::find_dm_refs(doc) {
        if chain.contains(&r.node)
            || doc.ancestors(r.node).any(|a| chain.first() == Some(&a)) && chain.contains(&r.node)
        {
            return r.find_latest_in_dir(dir).ok().flatten();
        }
    }
    for r in s1000d::find_pm_refs(doc) {
        if chain.contains(&r.node) {
            return r.find_latest_in_dir(dir).ok().flatten();
        }
    }
    None
}

/// Whether a `dmRef` and a `pmRef` may be inserted at the caret, here or
/// after one of the enclosing elements (where `insert_element_xml` puts it).
pub fn reference_insertable(ds: &DocState) -> (bool, bool) {
    if ds.read_only || ds.view == crate::doc::View::Source {
        return (false, false);
    }
    // References are built in 4.0+ markup; legacy modules use refdm.
    if ds.s1000d.as_ref().is_some_and(|i| i.issue.is_legacy()) {
        return (false, false);
    }
    let Some(g) = ds.grammar.as_deref() else {
        return (true, true);
    };
    let doc = &ds.editor.doc;
    let (mut parent, mut index) = ds.editor.insertion_point();
    let (mut dm, mut pm) = (false, false);
    loop {
        dm |= g.can_insert(doc, parent, index, "dmRef");
        pm |= g.can_insert(doc, parent, index, "pmRef");
        let Some(up) = doc.parent(parent).filter(|&p| doc.is_element(p)) else {
            break;
        };
        index = doc.index_in_parent(parent).unwrap_or(0) + 1;
        parent = up;
    }
    (dm, pm)
}

/// State of the View > Applicability dialog.
#[derive(Default)]
pub struct ApplicDialog {
    pub open: bool,
    /// Properties of the document they were collected from, by (document id, structure revision).
    props: Option<((u64, u64), Vec<s1000d::ApplicProperty>)>,
}

pub fn applic_window(ctx: &egui::Context, dlg: &mut ApplicDialog, ds: Option<&mut DocState>) {
    let mut open = dlg.open;
    egui::Window::new("Applicability")
        .open(&mut open)
        .default_width(380.0)
        .show(ctx, |ui| {
            let Some(ds) = ds else {
                ui.label("Open a data module or publication module first.");
                return;
            };
            let key = (ds.id, ds.editor.doc.structure_revision());
            if dlg.props.as_ref().is_none_or(|(k, _)| *k != key) {
                dlg.props = Some((key, s1000d::applic_properties(&ds.editor.doc)));
            }
            let props = &dlg.props.as_ref().unwrap().1;
            let before = (
                ds.applic.enabled,
                ds.applic.display,
                ds.applic.choice.clone(),
            );
            if ds
                .s1000d
                .as_ref()
                .is_some_and(|i| i.issue < s1000d::Issue::V3_0)
            {
                ui.label(
                    RichText::new(
                        "Issues 2.0 to 2.3 state applicability as text (type, model, version) \
                         that cannot be evaluated, so nothing is filtered.",
                    )
                    .weak(),
                );
            }
            ui.checkbox(&mut ds.applic.enabled, "Filter by applicability");
            ui.horizontal(|ui| {
                ui.label("Inapplicable content:");
                ui.radio_value(&mut ds.applic.display, ApplicDisplay::Dim, "Dimmed");
                ui.radio_value(&mut ds.applic.display, ApplicDisplay::Hide, "Hidden");
            });
            ui.separator();
            if props.is_empty() {
                ui.label(
                    RichText::new(
                        "The document has no computable applicability (no assert elements).",
                    )
                    .weak(),
                );
            }
            egui::Grid::new("applic-props")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    for p in props {
                        ui.label(format!("{} ({})", p.key.ident, p.key.kind));
                        let current = ds.applic.choice.get(&p.key).cloned();
                        let mut chosen = current.clone();
                        egui::ComboBox::from_id_salt(("applic", &p.key.ident, &p.key.kind))
                            .selected_text(current.as_deref().unwrap_or("any"))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut chosen, None, "any");
                                for v in &p.values {
                                    ui.selectable_value(&mut chosen, Some(v.clone()), v);
                                }
                            });
                        if chosen != current {
                            match chosen {
                                Some(v) => {
                                    ds.applic.choice.insert(p.key.clone(), v);
                                }
                                None => {
                                    ds.applic.choice.remove(&p.key);
                                }
                            }
                            // Picking a value is a request to filter.
                            ds.applic.enabled = true;
                        }
                        ui.end_row();
                    }
                });
            if !props.is_empty() && ui.button("Set all to any").clicked() {
                ds.applic.choice.clear();
            }
            if (
                ds.applic.enabled,
                ds.applic.display,
                ds.applic.choice.clone(),
            ) != before
            {
                ds.applic.revision += 1;
            }
            if let Some(r) = ds.applic_result() {
                ui.separator();
                let module = match r.module {
                    Some(s1000d::Truth::False) => "does not apply",
                    Some(s1000d::Truth::True) => "applies",
                    Some(s1000d::Truth::Unknown) => "may apply (some properties are \"any\")",
                    None => "has no applicability statement",
                };
                ui.label(format!("The module {module}."));
                ui.label(format!("{} element(s) do not apply.", r.excluded.len()));
                if !r.dangling.is_empty() {
                    ui.colored_label(
                        Color32::from_rgb(220, 160, 40),
                        format!("⚠ applicRefId names no applic: {}", r.dangling.join(", ")),
                    );
                }
            }
        });
    dlg.open = open;
}

pub fn info_text(doc: &Document) -> String {
    let mut s = String::new();
    match s1000d::detect(doc) {
        Some(info) => {
            s.push_str(&format!(
                "Issue:        S1000D {} ({:?})\n",
                info.issue, info.source
            ));
            s.push_str(&format!("Schema:       {}\n", info.schema.display_name()));
        }
        None => s.push_str("Not recognised as an S1000D document.\n"),
    }
    if let Some(id) = s1000d::DmIdent::from_document(doc) {
        s.push_str(&format!("DMC:          {}\n", id.dmc));
        if let Some(i) = &id.issue_info {
            s.push_str(&format!("Issue/inwork: {i}\n"));
        }
        if let Some(l) = &id.language {
            s.push_str(&format!(
                "Language:     {}-{}\n",
                l.language_iso_code, l.country_iso_code
            ));
        }
        if let Some(d) = &id.issue_date {
            s.push_str(&format!("Issue date:   {d}\n"));
        }
        if let Some(t) = &id.title {
            s.push_str(&format!(
                "Title:        {}{}\n",
                t.tech_name,
                t.info_name
                    .as_deref()
                    .map(|i| format!(" - {i}"))
                    .unwrap_or_default()
            ));
        }
        if let Some(f) = id.csdb_filename() {
            s.push_str(&format!("CSDB name:    {f}\n"));
        }
        for p in id.dmc.validate() {
            s.push_str(&format!("⚠ {p}\n"));
        }
    }
    let refs = s1000d::find_dm_refs(doc);
    if !refs.is_empty() {
        s.push_str(&format!("\n{} data module reference(s):\n", refs.len()));
        for r in refs.iter().take(30) {
            s.push_str(&format!("  {}\n", r.dmc));
        }
    }
    let icns = s1000d::find_icn_refs(doc);
    if !icns.is_empty() {
        s.push_str(&format!("\n{} ICN reference(s):\n", icns.len()));
        for i in icns.iter().take(30) {
            s.push_str(&format!(
                "  {}{}\n",
                i.icn,
                i.file
                    .as_deref()
                    .map(|f| format!(" → {f}"))
                    .unwrap_or_default()
            ));
        }
    }
    s
}

pub struct NewDmDialog {
    pub issue: s1000d::Issue,
    pub kind: SchemaKind,
    pub code: String,
    pub tech_name: String,
    pub info_name: String,
    pub language: String,
    pub country: String,
    pub error: Option<String>,
    /// Used instead of `code` and the title fields for a publication module.
    pub pm_code: String,
    pub pm_title: String,
}

impl Default for NewDmDialog {
    fn default() -> Self {
        Self {
            issue: s1000d::Issue::V4_1,
            kind: SchemaKind::Descript,
            code: "DMC-XYLEM-A-00-00-00-00A-040A-A".into(),
            pm_code: "PMC-XYLEM-00000-00001-00".into(),
            pm_title: "New publication".into(),
            tech_name: "New equipment".into(),
            info_name: "Description".into(),
            language: "en".into(),
            country: "US".into(),
            error: None,
        }
    }
}

pub enum DialogResult {
    Open,
    Cancel,
    Create(String, String),
}

pub fn new_dm_window(ctx: &egui::Context, d: &mut NewDmDialog) -> DialogResult {
    let mut open = true;
    let mut result = DialogResult::Open;
    egui::Window::new("New S1000D module")
        .collapsible(false)
        .open(&mut open)
        .default_width(480.0)
        .show(ctx, |ui| {
            egui::Grid::new("newdm")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Issue");
                    egui::ComboBox::from_id_salt("dm-issue")
                        .selected_text(d.issue.as_str())
                        .show_ui(ui, |ui| {
                            for i in s1000d::Issue::ALL {
                                ui.selectable_value(&mut d.issue, i, i.as_str());
                            }
                        });
                    ui.end_row();
                    let mut kinds = s1000d::available_kinds(d.issue);
                    if !d.issue.is_legacy() {
                        kinds.push(SchemaKind::Pm);
                    }
                    if !kinds.contains(&d.kind) {
                        d.kind = kinds[0];
                    }
                    ui.label("Schema");
                    egui::ComboBox::from_id_salt("dm-kind")
                        .selected_text(d.kind.display_name())
                        .show_ui(ui, |ui| {
                            for k in kinds {
                                ui.selectable_value(&mut d.kind, k, k.display_name());
                            }
                        });
                    ui.end_row();
                    if d.kind == SchemaKind::Pm {
                        ui.label("Publication module code");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.pm_code)
                                .desired_width(300.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.end_row();
                        ui.label("Title");
                        ui.text_edit_singleline(&mut d.pm_title);
                        ui.end_row();
                    } else {
                        ui.label("Data module code");
                        ui.add(
                            egui::TextEdit::singleline(&mut d.code)
                                .desired_width(300.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        ui.end_row();
                        ui.label("Technical name");
                        ui.text_edit_singleline(&mut d.tech_name);
                        ui.end_row();
                        ui.label("Information name");
                        ui.text_edit_singleline(&mut d.info_name);
                        ui.end_row();
                    }
                    ui.label("Language / country");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut d.language).desired_width(40.0));
                        ui.add(egui::TextEdit::singleline(&mut d.country).desired_width(40.0));
                    });
                    ui.end_row();
                });
            if d.kind == SchemaKind::Pm {
                pm_part(ui, d, &mut result);
                return;
            }
            let dmc = Dmc::parse(&d.code);
            match &dmc {
                Ok(c) => {
                    let problems = c.validate();
                    for p in &problems {
                        ui.colored_label(egui::Color32::from_rgb(220, 160, 40), format!("⚠ {p}"));
                    }
                    let name = s1000d::csdb_filename(
                        &c.to_string(),
                        &s1000d::IssueInfo::new("001", "00"),
                        &s1000d::Language::new(d.language.clone(), d.country.clone()),
                    );
                    ui.label(RichText::new(format!("File: {name}")).weak());
                }
                Err(e) => {
                    ui.colored_label(egui::Color32::from_rgb(220, 70, 70), e.to_string());
                }
            }
            if let Some(e) = &d.error {
                ui.colored_label(egui::Color32::from_rgb(220, 70, 70), e);
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(dmc.is_ok(), egui::Button::new("Create"))
                    .clicked()
                {
                    let c = dmc.as_ref().unwrap();
                    let info = Some(d.info_name.as_str()).filter(|s| !s.trim().is_empty());
                    match s1000d::new_data_module(
                        d.issue,
                        d.kind,
                        c,
                        &d.tech_name,
                        info,
                        (&d.language, &d.country),
                        IssueDate::today(),
                    ) {
                        Ok(xml) => {
                            let name = s1000d::csdb_filename(
                                &c.to_string(),
                                &s1000d::IssueInfo::new("001", "00"),
                                &s1000d::Language::new(d.language.clone(), d.country.clone()),
                            );
                            result = DialogResult::Create(xml, name);
                        }
                        Err(e) => d.error = Some(e.to_string()),
                    }
                }
                if ui.button("Cancel").clicked() {
                    result = DialogResult::Cancel;
                }
            });
        });
    if !open {
        return DialogResult::Cancel;
    }
    result
}

/// The code check and buttons of the wizard for a publication module.
fn pm_part(ui: &mut egui::Ui, d: &mut NewDmDialog, result: &mut DialogResult) {
    let pmc = s1000d::Pmc::parse(&d.pm_code);
    let language = s1000d::Language::new(d.language.clone(), d.country.clone());
    let issue_info = s1000d::IssueInfo::new("001", "00");
    match &pmc {
        Ok(c) => {
            let name = s1000d::csdb_filename(&c.to_string(), &issue_info, &language);
            ui.label(RichText::new(format!("File: {name}")).weak());
        }
        Err(e) => {
            ui.colored_label(Color32::from_rgb(220, 70, 70), e.to_string());
        }
    }
    if let Some(e) = &d.error {
        ui.colored_label(Color32::from_rgb(220, 70, 70), e);
    }
    ui.horizontal(|ui| {
        if ui
            .add_enabled(pmc.is_ok(), egui::Button::new("Create"))
            .clicked()
            && let Ok(c) = &pmc
        {
            let options = s1000d::NewModuleOptions::new(IssueDate::today());
            match s1000d::new_publication_module(
                d.issue,
                c,
                &d.pm_title,
                (&d.language, &d.country),
                &options,
            ) {
                Ok(xml) => {
                    let name = s1000d::csdb_filename(&c.to_string(), &issue_info, &language);
                    *result = DialogResult::Create(xml, name);
                }
                Err(e) => d.error = Some(e.to_string()),
            }
        }
        if ui.button("Cancel").clicked() {
            *result = DialogResult::Cancel;
        }
    });
}
