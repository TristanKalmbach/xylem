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
use xylem_xslt::{
    Baseline, CheckEnv, CheckReport, Host, Outcome, ProfileReport, Suite, check, profile,
};

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
    Profiled(Box<ProfileReport>),
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
    /// The last Time templates run; shown instead of the check results.
    profile: Option<ProfileReport>,
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
    Profile,
    OpenAt(PathBuf, u32),
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
            .grow()
            .show(ctx, |ui| self.stylesheet_check_ui(ui, &mut actions));
        if close {
            self.stylesheet_check.open = false;
        }
        for a in actions {
            match a {
                Action::Run => self.start_stylesheet_check(),
                Action::Profile => self.start_stylesheet_profile(),
                Action::OpenAt(path, line) => self.open_stylesheet_at(&path, line),
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

    /// Reads the suite, the stylesheet and the engine from the window, or
    /// shows what is missing.
    fn prepare_check(&mut self) -> Option<(Suite, PathBuf, Host)> {
        let st = &mut self.stylesheet_check;
        st.error = None;
        st.report = None;
        st.profile = None;
        st.expanded.clear();
        let suite_path = PathBuf::from(st.suite.trim());
        let suite = match Suite::load(&suite_path) {
            Ok(s) => s,
            Err(e) => {
                st.error = Some(e);
                return None;
            }
        };
        let stylesheet = match st.stylesheet.trim() {
            "" => match &suite.stylesheet {
                Some(p) => p.clone(),
                None => {
                    st.error =
                        Some("Choose the stylesheet to check: the suite does not name one.".into());
                    return None;
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
            return None;
        }
        let host = Host::new(PathBuf::from(st.engine.trim()));
        self.settings.stylesheet_suite = Some(suite_path);
        self.settings.xslt_host = Some(host.program.clone());
        Some((suite, stylesheet, host))
    }

    fn start_stylesheet_check(&mut self) {
        let Some((suite, stylesheet, host)) = self.prepare_check() else {
            return;
        };
        let st = &mut self.stylesheet_check;
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

    fn start_stylesheet_profile(&mut self) {
        let Some((suite, stylesheet, host)) = self.prepare_check() else {
            return;
        };
        let st = &mut self.stylesheet_check;
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
            let report = profile(
                &suite,
                &stylesheet,
                CheckEnv {
                    host: &host,
                    work: &work,
                    progress: &mut progress,
                    cancel: &stop,
                },
            );
            let _ = tx.send(Event::Profiled(Box::new(report)));
        });
    }

    /// Opens a stylesheet with the template or function on `line` selected.
    /// Saxon reports the line where the element's start tag ends, so the
    /// template is the last `xsl:template` or `xsl:function` starting on or
    /// before that line.
    fn open_stylesheet_at(&mut self, path: &Path, line: u32) {
        let key = crate::find_files::path_key(path);
        let find = |docs: &[crate::doc::DocState]| {
            docs.iter().position(|d| {
                d.path
                    .as_deref()
                    .is_some_and(|p| crate::find_files::path_key(p) == key)
            })
        };
        if find(&self.docs).is_none() {
            self.open_path(path.to_path_buf());
        }
        let Some(i) = find(&self.docs) else {
            return;
        };
        self.active = i;
        let ds = &mut self.docs[i];
        let doc = &ds.editor.doc;
        let (xml, offsets) = doc.to_xml_with_offsets();
        // Character offset where the reported line ends.
        let mut line_end = xml.len();
        let mut seen = 1u32;
        for (at, c) in xml.char_indices() {
            if c == '\n' {
                if seen == line {
                    line_end = at;
                    break;
                }
                seen += 1;
            }
        }
        let target = doc
            .descendants(doc.root())
            .filter(|&n| {
                doc.name(n).is_some_and(|name| {
                    let local = name.rsplit(':').next().unwrap_or(name);
                    local == "template" || local == "function"
                })
            })
            .filter(|&n| offsets.get(n.index()).is_some_and(|&o| o <= line_end))
            .last();
        if let Some(el) = target {
            let p = xylem_edit::pos::first_editable(doc, &xylem_edit::NoRules, el);
            ds.editor.set_caret(p);
            ds.canvas.scroll_to_caret = true;
            ds.canvas.request_focus = true;
        }
    }

    fn poll_stylesheet_check(&mut self, ctx: &egui::Context) {
        let st = &mut self.stylesheet_check;
        let Some((rx, _)) = &st.running else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(Event::Progress(p)) => st.progress = p,
                Ok(Event::Profiled(report)) => {
                    st.error = report.error.clone();
                    st.profile = Some(*report);
                    st.running = None;
                    return;
                }
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
            if !busy
                && ui
                    .add_enabled_ui(!st.suite.trim().is_empty(), |ui| {
                        crate::ui::button(ui, "Time templates")
                    })
                    .inner
                    .on_hover_text("Run the stylesheet on every test with timing on, and list where the time goes")
                    .clicked()
            {
                actions.push(Action::Profile);
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
        if let Some(prof) = &st.profile {
            if prof.error.is_none() {
                profile_ui(ui, prof, actions);
            }
            return;
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

/// The Time templates results: a summary per test and the templates ranked
/// by own time across all tests.
fn profile_ui(ui: &mut egui::Ui, prof: &ProfileReport, actions: &mut Vec<Action>) {
    crate::ui::divider(ui);
    if prof.cancelled {
        crate::ui::muted(ui, "Stopped.");
        return;
    }
    for t in &prof.tests {
        if let Err(e) = &t.profile {
            crate::ui::notice(ui, crate::ui::Tone::Warning, format!("{}: {e}", t.name));
        }
    }
    let c = &prof.combined;
    let timed = c.timed_ms().max(f64::EPSILON);
    let ok = prof.tests.iter().filter(|t| t.profile.is_ok()).count();
    crate::ui::muted(
        ui,
        format!(
            "{} templates and functions on {ok} test{}, {:.0} ms in them. Timing slows the run, so \
             compare templates with each other. Click one to open it.",
            c.templates.len(),
            if ok == 1 { "" } else { "s" },
            c.timed_ms()
        ),
    );
    ui.add_space(4.0);
    let pal = crate::ui::palette(ui.ctx());
    let mono = egui::TextStyle::Monospace.resolve(ui.style());
    egui::ScrollArea::vertical()
        .id_salt("stylesheet-profile")
        .auto_shrink([false, true])
        .max_height(420.0)
        .show(ui, |ui| {
            egui::Grid::new("stylesheet-profile-grid")
                .striped(true)
                .num_columns(5)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    for h in ["Own time", "Total", "Calls", "Share", "Template"] {
                        ui.label(RichText::new(h).color(pal.text_muted).small());
                    }
                    ui.end_row();
                    for t in c.templates.iter().take(200) {
                        let share = t.self_ms / timed;
                        ui.label(RichText::new(format!("{:.1} ms", t.self_ms)).font(mono.clone()));
                        ui.label(
                            RichText::new(format!("{:.1} ms", t.total_ms))
                                .font(mono.clone())
                                .color(pal.text_muted),
                        );
                        ui.label(RichText::new(t.calls.to_string()).font(mono.clone()));
                        // A bar for the share, so the expensive ones stand out.
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(60.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, pal.hover);
                        let mut bar = rect;
                        bar.set_width((rect.width() * share as f32).max(1.0));
                        ui.painter().rect_filled(bar, 2.0, pal.accent);
                        let file = t.path();
                        let name = file
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        let r = ui
                            .link(format!("{}   {}:{}", t.label(), name, t.line))
                            .on_hover_text(format!(
                                "{:.0}% of the time · {}",
                                share * 100.0,
                                file.display()
                            ));
                        if r.clicked() {
                            actions.push(Action::OpenAt(file, t.line));
                        }
                        ui.end_row();
                    }
                });
        });
}
