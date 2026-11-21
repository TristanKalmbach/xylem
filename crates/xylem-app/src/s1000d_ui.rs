//! S1000D integration: new data module wizard, BREX, CSDB naming, references.

use crate::doc::{Issue, Severity};
use egui::RichText;
use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId};
use xylem_s1000d::{self as s1000d, Dmc, IssueDate, SchemaKind};

/// Maps `http://www.s1000d.org/S1000D_x-y/...` schema URLs to a local folder
/// laid out as `<root>/S1000D_x-y/...` (or `<root>/x.y/...`).
pub fn add_s1000d_catalog(cat: &mut xylem_schema::Catalog, root: &Path) {
    for issue in s1000d::Issue::ALL {
        let token = issue.schema_token();
        for folder in [root.join(token), root.join(issue.as_str())] {
            if folder.is_dir() {
                cat.add_prefix(&format!("http://www.s1000d.org/{token}/"), folder.clone());
                cat.add_prefix(&format!("https://www.s1000d.org/{token}/"), folder.clone());
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
}

impl Default for NewDmDialog {
    fn default() -> Self {
        Self {
            issue: s1000d::Issue::V4_1,
            kind: SchemaKind::Descript,
            code: "DMC-XYLEM-A-00-00-00-00A-040A-A".into(),
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
    egui::Window::new("New S1000D data module")
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
                    let kinds = s1000d::available_kinds(d.issue);
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
                    ui.label("Language / country");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut d.language).desired_width(40.0));
                        ui.add(egui::TextEdit::singleline(&mut d.country).desired_width(40.0));
                    });
                    ui.end_row();
                });
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
