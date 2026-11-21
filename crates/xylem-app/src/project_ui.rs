//! The Project tab: a CSDB folder's data and publication modules, filterable,
//! with a reference check. Scanning and checking live in `xylem_s1000d`; this
//! file only runs the scan on a worker thread and draws the results.

use egui::{Color32, RichText, Sense, Ui};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, TryRecvError};
use xylem_s1000d::{Finding, FindingKind, ModuleRef, ModuleType, Project, ScanProgress};

/// What the user asked the project tab to do; carried out by the app.
pub enum ProjectAction {
    ChooseFolder,
    Open(PathBuf),
    /// Open the module and select the reference.
    OpenAt(PathBuf, ModuleRef),
    /// Insert a reference to this module at the caret of the active document.
    InsertRef(usize),
}

struct Scan {
    rx: Receiver<Project>,
    progress: Arc<ScanProgress>,
}

#[derive(Default)]
pub struct ProjectView {
    pub project: Option<Arc<Project>>,
    pub filter: String,
    scan: Option<Scan>,
    pub findings: Option<Vec<Finding>>,
    /// Also list references to the BREX (usually the S1000D default BREX,
    /// which a project folder does not hold).
    pub show_brex: bool,
    pub selected: Option<usize>,
    /// Row and time of the last click, to recognise a double click.
    last_click: Option<(usize, f64)>,
}

const DOUBLE_CLICK_SECONDS: f64 = 0.4;

impl ProjectView {
    pub fn is_scanning(&self) -> bool {
        self.scan.is_some()
    }

    /// Starts scanning `dir` on a worker thread, replacing any scan in progress.
    pub fn start_scan(&mut self, dir: PathBuf, ctx: &egui::Context) {
        if let Some(old) = self.scan.take() {
            // Its result would describe a folder that is no longer shown.
            old.progress.cancel.store(true, Ordering::Relaxed);
        }
        let progress = Arc::new(ScanProgress::default());
        let (tx, rx) = std::sync::mpsc::channel();
        let worker_progress = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let project = xylem_s1000d::scan_dir(&dir, &worker_progress);
            if tx.send(project).is_ok() {
                ctx.request_repaint();
            }
        });
        self.scan = Some(Scan { rx, progress });
        self.findings = None;
        self.selected = None;
    }

    /// Picks up a finished scan; call once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let Some(scan) = &self.scan else { return };
        match scan.rx.try_recv() {
            Ok(project) => {
                self.project = Some(Arc::new(project));
                self.scan = None;
            }
            Err(TryRecvError::Disconnected) => self.scan = None,
            Err(TryRecvError::Empty) => {
                // Keep the progress count moving.
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    pub fn check_references(&mut self) {
        if let Some(p) = &self.project {
            self.findings = Some(xylem_s1000d::check_references(&p.modules));
        }
    }

    /// `can_insert` says, for data and publication modules, whether a
    /// reference to one may be inserted at the active document's caret.
    pub fn ui(&mut self, ui: &mut Ui, can_insert: (bool, bool), out: &mut Vec<ProjectAction>) {
        ui.horizontal(|ui| {
            if ui.button("Folder…").clicked() {
                out.push(ProjectAction::ChooseFolder);
            }
            let has = self.project.is_some() && self.scan.is_none();
            if ui.add_enabled(has, egui::Button::new("Rescan")).clicked()
                && let Some(p) = &self.project
            {
                let dir = p.dir.clone();
                self.start_scan(dir, ui.ctx());
            }
            if ui
                .add_enabled(has, egui::Button::new("Check references"))
                .clicked()
            {
                self.check_references();
            }
        });
        if let Some(scan) = &self.scan {
            let done = scan.progress.done.load(Ordering::Relaxed);
            let total = scan.progress.total.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(format!("Scanning… {done} / {total}"));
            });
            return;
        }
        let Some(project) = self.project.clone() else {
            ui.label(RichText::new("Choose a folder of data and publication modules.").weak());
            return;
        };
        let folder = project.dir.file_name().map_or_else(
            || project.dir.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ui.label(
            RichText::new(format!(
                "{folder}: {} module(s){}",
                project.modules.len(),
                if project.errors.is_empty() {
                    String::new()
                } else {
                    format!(", {} unreadable", project.errors.len())
                }
            ))
            .weak(),
        )
        .on_hover_text(project.dir.display().to_string());
        ui.add(
            egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Filter: code, title, issue, language, file")
                .desired_width(f32::INFINITY),
        );
        let rows: Vec<usize> = project
            .modules
            .iter()
            .enumerate()
            .filter(|(_, m)| m.matches(&self.filter))
            .map(|(i, _)| i)
            .collect();
        let selected = self.selected.filter(|s| rows.contains(s));
        let insert_ok = selected.is_some_and(|s| match project.modules[s].kind {
            ModuleType::Data => can_insert.0,
            ModuleType::Publication => can_insert.1,
        });
        ui.horizontal(|ui| {
            if ui
                .add_enabled(selected.is_some(), egui::Button::new("Open"))
                .clicked()
                && let Some(s) = selected
            {
                out.push(ProjectAction::Open(project.modules[s].path.clone()));
            }
            if ui
                .add_enabled(insert_ok, egui::Button::new("Insert reference"))
                .on_disabled_hover_text(
                    "Select a module, and put the caret where a reference is allowed.",
                )
                .clicked()
                && let Some(s) = selected
            {
                out.push(ProjectAction::InsertRef(s));
            }
        });
        let findings_height = if self.findings.is_some() {
            (ui.available_height() * 0.4).max(120.0)
        } else {
            0.0
        };
        let list_height = (ui.available_height() - findings_height).max(80.0);
        let row_h = ui.text_style_height(&egui::TextStyle::Body) * 2.0 + 6.0;
        egui::ScrollArea::vertical()
            .id_salt("project-rows")
            .max_height(list_height)
            .auto_shrink([false, false])
            .show_rows(ui, row_h, rows.len(), |ui, range| {
                for &i in &rows[range] {
                    let m = &project.modules[i];
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_h),
                        Sense::click(),
                    );
                    let visuals = ui.visuals();
                    if self.selected == Some(i) {
                        ui.painter()
                            .rect_filled(rect, 2.0, visuals.selection.bg_fill);
                    } else if resp.hovered() {
                        ui.painter()
                            .rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
                    }
                    let painter = ui.painter_at(rect);
                    let strong = visuals.strong_text_color();
                    let weak = visuals.weak_text_color();
                    let mono = egui::FontId::monospace(12.0);
                    let body = egui::TextStyle::Body.resolve(ui.style());
                    let issue = m
                        .issue_info
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let lang = m
                        .language
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_default();
                    let top = rect.left_top() + egui::vec2(4.0, 2.0);
                    painter.text(top, egui::Align2::LEFT_TOP, &m.code, mono, strong);
                    let title = if m.title.is_empty() {
                        m.file_name()
                    } else {
                        m.title.clone()
                    };
                    painter.text(
                        top + egui::vec2(0.0, row_h / 2.0 - 1.0),
                        egui::Align2::LEFT_TOP,
                        format!("{issue}  {lang}  {title}"),
                        body,
                        weak,
                    );
                    let resp = resp.on_hover_text(format!(
                        "{}\n{}\n{}",
                        m.title,
                        m.path.display(),
                        m.issue.map(|i| format!("S1000D {i}")).unwrap_or_default()
                    ));
                    if resp.clicked() {
                        // egui counts clicks wherever they land, so a click on
                        // another widget just before can turn a double click
                        // into a triple one. Two clicks on the same row are
                        // tracked here instead.
                        let now = ui.input(|inp| inp.time);
                        if self
                            .last_click
                            .is_some_and(|(row, t)| row == i && now - t < DOUBLE_CLICK_SECONDS)
                        {
                            out.push(ProjectAction::Open(m.path.clone()));
                            self.last_click = None;
                        } else {
                            self.last_click = Some((i, now));
                        }
                        self.selected = Some(i);
                    }
                    let allowed = match m.kind {
                        ModuleType::Data => can_insert.0,
                        ModuleType::Publication => can_insert.1,
                    };
                    resp.context_menu(|ui| {
                        self.selected = Some(i);
                        if ui.button("Open").clicked() {
                            out.push(ProjectAction::Open(m.path.clone()));
                            ui.close();
                        }
                        if ui
                            .add_enabled(allowed, egui::Button::new("Insert reference"))
                            .clicked()
                        {
                            out.push(ProjectAction::InsertRef(i));
                            ui.close();
                        }
                        if ui.button("Copy code").clicked() {
                            ui.ctx().copy_text(m.code.clone());
                            ui.close();
                        }
                    });
                }
            });
        if self.findings.is_some() {
            ui.separator();
            self.findings_ui(ui, &project, out);
        }
    }

    fn findings_ui(&mut self, ui: &mut Ui, project: &Project, out: &mut Vec<ProjectAction>) {
        let Some(findings) = &self.findings else {
            return;
        };
        let brex = |f: &Finding| {
            f.reference
                .and_then(|r| project.modules[f.module].refs.get(r))
                .is_some_and(|r| r.in_brex)
        };
        let brex_count = findings.iter().filter(|f| brex(f)).count();
        let shown: Vec<&Finding> = findings
            .iter()
            .filter(|f| self.show_brex || !brex(f))
            .collect();
        let hidden = findings.len() - shown.len();
        let mut close = false;
        ui.horizontal(|ui| {
            ui.strong(format!("Reference check ({})", shown.len()));
            close = ui.small_button("x").on_hover_text("Close").clicked();
        });
        let mut show_brex = self.show_brex;
        ui.checkbox(
            &mut show_brex,
            format!("Include BREX references ({brex_count})"),
        );
        egui::ScrollArea::vertical()
            .id_salt("project-findings")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if shown.is_empty() {
                    ui.label(
                        RichText::new(if hidden > 0 {
                            "No problems apart from BREX references."
                        } else {
                            "No problems found."
                        })
                        .weak(),
                    );
                }
                for f in shown {
                    let m = &project.modules[f.module];
                    let color = match f.kind {
                        FindingKind::Unresolved => Color32::from_rgb(220, 70, 70),
                        FindingKind::OlderIssue { .. } | FindingKind::Outdated { .. } => {
                            Color32::from_rgb(210, 150, 40)
                        }
                    };
                    // The message matters more than the long file name, so
                    // it gets its own wrapped line.
                    let line = |ui: &mut Ui, label: egui::Label| {
                        ui.add(label.selectable(false).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand)
                            .on_hover_text(m.path.display().to_string())
                    };
                    let name = line(
                        ui,
                        egui::Label::new(RichText::new(m.file_name()).small().weak()).truncate(),
                    );
                    let message = line(
                        ui,
                        egui::Label::new(RichText::new(&f.message).color(color)).wrap(),
                    );
                    if name.clicked() || message.clicked() {
                        out.push(match f.reference.and_then(|i| m.refs.get(i)) {
                            Some(reference) => {
                                ProjectAction::OpenAt(m.path.clone(), reference.clone())
                            }
                            None => ProjectAction::Open(m.path.clone()),
                        });
                    }
                }
            });
        self.show_brex = show_brex;
        if close {
            self.findings = None;
        }
    }
}
