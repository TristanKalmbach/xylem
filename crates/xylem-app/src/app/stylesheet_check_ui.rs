//! The Check stylesheet output window: runs a stylesheet on a suite of test
//! inputs, compares the output with a baseline and shows what differs. The
//! work itself is in the `xylem-xslt` crate; it runs on a worker thread.

use super::XylemApp;
use egui::RichText;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use xylem_xslt::{Baseline, CheckEnv, CheckReport, Host, Outcome, Suite, check};

#[derive(Default, Clone, Copy, PartialEq)]
enum Against {
    /// The stylesheet as last committed to git.
    #[default]
    Committed,
    /// The output saved with each test.
    Saved,
    /// Another stylesheet.
    Other,
}

enum Event {
    Progress(String),
    Done(Box<CheckReport>),
}

#[derive(Default)]
pub struct StylesheetCheckState {
    pub open: bool,
    suite: String,
    stylesheet: String,
    against: Against,
    other: String,
    engine: String,
    running: Option<(Receiver<Event>, Arc<AtomicBool>)>,
    progress: String,
    report: Option<CheckReport>,
    error: Option<String>,
    /// Tests whose differences are shown.
    expanded: HashSet<usize>,
}

enum Action {
    Run,
    Stop,
    Browse(Field),
    Open(PathBuf),
    ShowFolder(PathBuf),
}

#[derive(Clone, Copy)]
enum Field {
    Suite,
    Stylesheet,
    Other,
    Engine,
}

/// Where check outputs and committed copies go.
fn work_dir() -> PathBuf {
    crate::settings::config_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("stylesheet-check")
        .join("work")
}

fn is_stylesheet(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xsl") || e.eq_ignore_ascii_case("xslt"))
}

impl XylemApp {
    /// Opens the window; `suite` (from a script) fills in the suite file.
    pub(super) fn open_stylesheet_check(&mut self, suite: Option<String>) {
        let st = &mut self.stylesheet_check;
        st.open = true;
        if let Some(s) = suite.filter(|s| !s.is_empty()) {
            st.suite = s;
        } else if st.suite.is_empty()
            && let Some(p) = &self.settings.stylesheet_suite
        {
            st.suite = p.display().to_string();
        }
        if st.engine.is_empty() {
            st.engine = self
                .settings
                .xslt_host
                .clone()
                .or_else(|| self.team.xslt_host.clone())
                .or_else(|| std::env::var_os("XYLEM_XSLT_HOST").map(PathBuf::from))
                .map(|p| p.display().to_string())
                .unwrap_or_default();
        }
        // The stylesheet being edited is the one to check.
        if let Some(path) = self.docs.get(self.active).and_then(|d| d.path.clone())
            && is_stylesheet(&path)
        {
            st.stylesheet = path.display().to_string();
        }
    }

    pub(super) fn stylesheet_check_window(&mut self, ctx: &egui::Context) {
        self.poll_stylesheet_check(ctx);
        if !self.stylesheet_check.open {
            return;
        }
        let mut actions = Vec::new();
        let (close, _) = crate::ui::Dialog::new("Check stylesheet output")
            .width(720.0)
            .modeless()
            .show(ctx, |ui| self.stylesheet_check_ui(ui, &mut actions));
        if close {
            self.stylesheet_check.open = false;
        }
        for a in actions {
            match a {
                Action::Run => self.start_stylesheet_check(),
                Action::Stop => {
                    if let Some((_, cancel)) = &self.stylesheet_check.running {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
                Action::Browse(field) => self.browse_check_field(field),
                Action::Open(p) => self.open_path(p),
                Action::ShowFolder(p) => {
                    if let Err(e) = crate::html_export::open_in_browser(&p) {
                        self.toast(format!("Could not open {}: {e}", p.display()), true);
                    }
                }
            }
        }
    }

    fn browse_check_field(&mut self, field: Field) {
        let st = &mut self.stylesheet_check;
        let current = match field {
            Field::Suite => &st.suite,
            Field::Stylesheet => &st.stylesheet,
            Field::Other => &st.other,
            Field::Engine => &st.engine,
        };
        let start = PathBuf::from(current.trim());
        let picked = crate::script::file_dialog(|| {
            let mut d = rfd::FileDialog::new();
            if let Some(dir) = start.parent().filter(|d| d.is_dir()) {
                d = d.set_directory(dir);
            }
            d = match field {
                Field::Suite => d.add_filter("Test suite", &["toml"]),
                Field::Stylesheet | Field::Other => d.add_filter("Stylesheet", &["xsl", "xslt"]),
                Field::Engine => d.add_filter("Program", &["exe"]),
            };
            d.pick_file()
        });
        if let Some(p) = picked {
            let text = p.display().to_string();
            match field {
                Field::Suite => st.suite = text,
                Field::Stylesheet => st.stylesheet = text,
                Field::Other => st.other = text,
                Field::Engine => st.engine = text,
            }
        }
    }

    fn start_stylesheet_check(&mut self) {
        let st = &mut self.stylesheet_check;
        st.error = None;
        st.report = None;
        st.expanded.clear();
        let suite_path = PathBuf::from(st.suite.trim());
        let suite = match Suite::load(&suite_path) {
            Ok(s) => s,
            Err(e) => {
                st.error = Some(e);
                return;
            }
        };
        let stylesheet = match st.stylesheet.trim() {
            "" => match &suite.stylesheet {
                Some(p) => p.clone(),
                None => {
                    st.error =
                        Some("Choose the stylesheet to check: the suite does not name one.".into());
                    return;
                }
            },
            s => PathBuf::from(s),
        };
        if st.engine.trim().is_empty() {
            st.error = Some(
                "Choose the engine program (SaxonDotnetHost.exe, see tools/saxon-dotnet), or ask \
                 for xslt_host in the team configuration."
                    .into(),
            );
            return;
        }
        let baseline = match st.against {
            Against::Committed => Baseline::Committed { rev: "HEAD".into() },
            Against::Saved => Baseline::Saved,
            Against::Other => {
                if st.other.trim().is_empty() {
                    st.error = Some("Choose the stylesheet to compare with.".into());
                    return;
                }
                Baseline::Stylesheet(PathBuf::from(st.other.trim()))
            }
        };
        let host = Host::new(PathBuf::from(st.engine.trim()));
        self.settings.stylesheet_suite = Some(suite_path);
        self.settings.xslt_host = Some(host.program.clone());
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        st.progress = "Starting…".into();
        st.running = Some((rx, cancel));
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let mut progress = move |s: String| {
                let _ = tx2.send(Event::Progress(s));
            };
            let work = work_dir();
            let report = check(
                &suite,
                &stylesheet,
                &baseline,
                CheckEnv {
                    host: &host,
                    work: &work,
                    progress: &mut progress,
                    cancel: &stop,
                },
            );
            let _ = tx.send(Event::Done(Box::new(report)));
        });
    }

    fn poll_stylesheet_check(&mut self, ctx: &egui::Context) {
        let st = &mut self.stylesheet_check;
        let Some((rx, _)) = &st.running else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(Event::Progress(p)) => st.progress = p,
                Ok(Event::Done(report)) => {
                    st.error = report.error.clone();
                    // Differences open by default when there are few tests.
                    if report.results.len() <= 3 {
                        st.expanded = (0..report.results.len()).collect();
                    }
                    st.report = Some(*report);
                    st.running = None;
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    return;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    st.running = None;
                    st.error = Some("The check stopped unexpectedly.".into());
                    return;
                }
            }
        }
    }

    fn stylesheet_check_ui(&mut self, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        let st = &mut self.stylesheet_check;
        let busy = st.running.is_some();
        let field_w = 520.0;
        let path_row = |ui: &mut egui::Ui,
                        label: &str,
                        value: &mut String,
                        hint: &str,
                        field: Field,
                        actions: &mut Vec<Action>| {
            ui.horizontal(|ui| {
                crate::ui::text_field(ui, label, value, hint, field_w - 90.0);
                if crate::ui::button(ui, "Browse…").clicked() {
                    actions.push(Action::Browse(field));
                }
            });
        };
        crate::ui::form(ui, "stylesheet-check-form", |f| {
            f.row("Test suite", |ui| {
                path_row(
                    ui,
                    "Test suite",
                    &mut st.suite,
                    "xylem-tests.toml",
                    Field::Suite,
                    actions,
                )
            });
            f.row("Stylesheet", |ui| {
                path_row(
                    ui,
                    "Stylesheet",
                    &mut st.stylesheet,
                    "The suite's stylesheet",
                    Field::Stylesheet,
                    actions,
                )
            });
            f.row("Compare with", |ui| {
                crate::ui::segmented(
                    ui,
                    &mut st.against,
                    &[
                        (Against::Committed, "Last committed version"),
                        (Against::Saved, "Saved output"),
                        (Against::Other, "Another stylesheet"),
                    ],
                );
            });
            if st.against == Against::Other {
                f.row("", |ui| {
                    path_row(
                        ui,
                        "Stylesheet to compare with",
                        &mut st.other,
                        "Stylesheet to compare with",
                        Field::Other,
                        actions,
                    )
                });
            }
            f.row("Engine", |ui| {
                path_row(
                    ui,
                    "Engine program",
                    &mut st.engine,
                    "SaxonDotnetHost.exe",
                    Field::Engine,
                    actions,
                )
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if busy {
                if crate::ui::button(ui, "Stop").clicked() {
                    actions.push(Action::Stop);
                }
                ui.spinner();
                crate::ui::muted(ui, st.progress.clone());
            } else if ui
                .add_enabled_ui(!st.suite.trim().is_empty(), |ui| {
                    crate::ui::primary_button(ui, "Run check")
                })
                .inner
                .clicked()
            {
                actions.push(Action::Run);
            }
            if let Some(r) = &st.report
                && !busy
                && r.error.is_none()
            {
                let count =
                    |f: fn(&Outcome) -> bool| r.results.iter().filter(|t| f(&t.outcome)).count();
                let parts: Vec<String> = [
                    (
                        count(|o| matches!(o, Outcome::Identical { .. })),
                        "the same",
                    ),
                    (count(|o| matches!(o, Outcome::Different(_))), "different"),
                    (count(|o| matches!(o, Outcome::Failed(_))), "failed"),
                    (
                        count(|o| matches!(o, Outcome::NoBaseline)),
                        "without saved output",
                    ),
                ]
                .into_iter()
                .filter(|(n, _)| *n > 0)
                .map(|(n, what)| format!("{n} {what}"))
                .collect();
                let summary = if r.cancelled {
                    "Stopped".to_string()
                } else {
                    parts.join(" · ")
                };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::ui::muted(ui, summary);
                });
            }
        });
        if let Some(e) = &st.error {
            crate::ui::notice(ui, crate::ui::Tone::Danger, e.clone());
        }
        let Some(report) = &st.report else {
            return;
        };
        if report.error.is_some() {
            return;
        }
        crate::ui::divider(ui);
        let against = match &report.baseline_commit {
            Some(c) => format!("{} ({})", report.baseline.describe(), &c[..c.len().min(10)]),
            None => report.baseline.describe(),
        };
        crate::ui::muted(ui, format!("Compared with {against}"));
        ui.add_space(4.0);
        let pal = crate::ui::palette(ui.ctx());
        let mono = egui::TextStyle::Monospace.resolve(ui.style());
        egui::ScrollArea::vertical()
            .id_salt("stylesheet-check-results")
            .auto_shrink([false, true])
            .max_height(420.0)
            .show(ui, |ui| {
                for (i, r) in report.results.iter().enumerate() {
                    let (label, tone) = match &r.outcome {
                        Outcome::Identical { .. } => ("Same".to_string(), crate::ui::Tone::Success),
                        Outcome::Different(c) => (
                            format!("{} difference{}", c.count, if c.count == 1 { "" } else { "s" }),
                            crate::ui::Tone::Danger,
                        ),
                        Outcome::NoBaseline => ("No saved output".to_string(), crate::ui::Tone::Neutral),
                        Outcome::Failed(_) => ("Failed".to_string(), crate::ui::Tone::Warning),
                    };
                    ui.horizontal(|ui| {
                        crate::ui::badge(ui, label, tone);
                        ui.label(RichText::new(&r.name).strong());
                        let time = match (r.ms, r.baseline_ms) {
                            (Some(a), Some(b)) => format!("{a} ms, was {b} ms"),
                            (Some(a), None) => format!("{a} ms"),
                            _ => String::new(),
                        };
                        crate::ui::muted(ui, time);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if let Some(out) = &r.output
                                && crate::ui::button(ui, "Open output").clicked()
                            {
                                actions.push(Action::Open(out.clone()));
                            }
                            if let Some(base) = &r.baseline_output
                                && matches!(r.outcome, Outcome::Different(_))
                                && crate::ui::button(ui, "Open baseline").clicked()
                            {
                                actions.push(Action::Open(base.clone()));
                            }
                            if let Some(dir) = r.output.as_ref().and_then(|o| o.parent())
                                && crate::ui::button(ui, "Show folder").clicked()
                            {
                                actions.push(Action::ShowFolder(dir.to_path_buf()));
                            }
                        });
                    });
                    match &r.outcome {
                        Outcome::Failed(e) => {
                            crate::ui::notice(ui, crate::ui::Tone::Warning, e.clone());
                        }
                        Outcome::Different(c) => {
                            let open = st.expanded.contains(&i);
                            let toggle = if open { "Hide differences" } else { "Show differences" };
                            if ui.link(toggle).clicked() {
                                if open {
                                    st.expanded.remove(&i);
                                } else {
                                    st.expanded.insert(i);
                                }
                            }
                            if open {
                                for d in &c.differences {
                                    crate::ui::card(ui, false, |ui| {
                                        let near = d
                                            .near_id
                                            .as_deref()
                                            .map(|id| format!("  (in id {id})"))
                                            .unwrap_or_default();
                                        crate::ui::muted(ui, format!("{}{near}", d.path));
                                        let line = |ui: &mut egui::Ui, tag: &str, text: &str, color| {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.label(RichText::new(tag).color(pal.text_muted));
                                                ui.label(
                                                    RichText::new(text)
                                                        .font(mono.clone())
                                                        .color(color),
                                                );
                                            });
                                        };
                                        line(ui, "Was", &d.baseline, pal.danger);
                                        line(ui, "Now", &d.candidate, pal.success);
                                    });
                                }
                                if c.count > c.differences.len() {
                                    crate::ui::muted(
                                        ui,
                                        format!("…and {} more", c.count - c.differences.len()),
                                    );
                                }
                                if c.coarse {
                                    crate::ui::muted(
                                        ui,
                                        "The outputs differ a lot; the listing is not fine-grained.",
                                    );
                                }
                            }
                        }
                        _ => {}
                    }
                    ui.add_space(6.0);
                }
            });
    }
}
