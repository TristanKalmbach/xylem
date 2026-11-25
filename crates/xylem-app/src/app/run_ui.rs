//! Tools > Run stylesheet (Ctrl+Shift+R): runs a stylesheet on a document
//! and shows the result, for writing and debugging stylesheets. The input
//! is read as it is in the editor when it is open (unsaved edits
//! included); the stylesheet runs as saved. The result opens in Xylem, or
//! in the browser when it is HTML.

use super::XylemApp;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;
use xylem_xslt::Host;

#[derive(Default)]
pub struct RunState {
    pub open: bool,
    stylesheet: String,
    input: String,
    params: String,
    /// Write the result as the stylesheet's `xsl:output` asks, rather than
    /// the way a publishing service saves it.
    serialize: bool,
    job: Option<Receiver<Finished>>,
    last: Option<Finished>,
}

struct Finished {
    result: Result<PathBuf, String>,
    ms: u128,
    engine: String,
}

fn is_stylesheet(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xsl") || e.eq_ignore_ascii_case("xslt"))
}

/// `name=value` pairs separated by commas or new lines.
pub fn parse_params(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for pair in text
        .split([',', '\n'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        let (k, v) = pair
            .split_once('=')
            .ok_or(format!("A parameter is name=value; \"{pair}\" has no ="))?;
        let k = k.trim();
        if k.is_empty() {
            return Err(format!("\"{pair}\" has no parameter name"));
        }
        out.insert(k.to_string(), v.trim().to_string());
    }
    Ok(out)
}

/// An engine error made readable: runs of the same line (a template that
/// called itself thousands of times) become one line with a count, and the
/// whole is cut to `max_lines`.
pub fn readable_error(e: &str, max_lines: usize) -> String {
    let mut lines: Vec<(String, usize)> = Vec::new();
    for line in e.lines() {
        match lines.last_mut() {
            Some((last, n)) if *last == line => *n += 1,
            _ => lines.push((line.to_string(), 1)),
        }
    }
    let total = lines.len();
    let mut out: Vec<String> = lines
        .into_iter()
        .take(max_lines)
        .map(|(l, n)| if n > 1 { format!("{l}  (×{n})") } else { l })
        .collect();
    if total > max_lines {
        out.push(format!("  … {} more lines", total - max_lines));
    }
    out.join("\n")
}

/// The file extension for a result: `html` when it is an HTML page, `fo`
/// for XSL-FO, `xml` for other markup, `txt` otherwise.
pub fn result_extension(bytes: &[u8]) -> &'static str {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]);
    let head = text.trim_start_matches('\u{feff}').trim_start();
    let lower = head.to_ascii_lowercase();
    if lower.starts_with("<!doctype html") || lower.starts_with("<html") {
        "html"
    } else if head.contains("http://www.w3.org/1999/XSL/Format") {
        "fo"
    } else if head.starts_with('<') {
        "xml"
    } else {
        "txt"
    }
}

impl XylemApp {
    /// Opens the window, filling in the stylesheet and input from the open
    /// documents where it can.
    pub(super) fn open_run_stylesheet(&mut self) {
        let active = self.docs.get(self.active).and_then(|d| d.path.clone());
        let st = &mut self.run;
        st.open = true;
        if st.stylesheet.is_empty() {
            st.stylesheet = self.settings.run_stylesheet.clone().unwrap_or_default();
        }
        if st.input.is_empty() {
            st.input = self.settings.run_input.clone().unwrap_or_default();
        }
        if st.params.is_empty() {
            st.params = self.settings.run_params.clone();
        }
        // The active document is the stylesheet or the input; the other is
        // taken from the open documents when there is exactly one candidate.
        if let Some(p) = &active {
            if is_stylesheet(p) {
                st.stylesheet = p.display().to_string();
            } else {
                st.input = p.display().to_string();
            }
        }
        let open: Vec<PathBuf> = self.docs.iter().filter_map(|d| d.path.clone()).collect();
        let sheets: Vec<&PathBuf> = open.iter().filter(|p| is_stylesheet(p)).collect();
        let inputs: Vec<&PathBuf> = open.iter().filter(|p| !is_stylesheet(p)).collect();
        if st.stylesheet.is_empty() && sheets.len() == 1 {
            st.stylesheet = sheets[0].display().to_string();
        }
        if st.input.is_empty() && inputs.len() == 1 {
            st.input = inputs[0].display().to_string();
        }
    }

    fn start_run_stylesheet(&mut self) {
        let st = &self.run;
        let stylesheet = PathBuf::from(st.stylesheet.trim());
        let input = PathBuf::from(st.input.trim());
        let params = match parse_params(&st.params) {
            Ok(p) => p,
            Err(e) => return self.toast(e, true),
        };
        if !stylesheet.is_file() {
            return self.toast("Choose the stylesheet to run.", true);
        }
        // The input as it is in the editor when it is open.
        let key = crate::find_files::path_key(&input);
        let open_text = self
            .docs
            .iter()
            .find(|d| {
                d.path
                    .as_deref()
                    .is_some_and(|p| crate::find_files::path_key(p) == key)
            })
            .map(|d| d.editor.doc.to_bytes());
        if open_text.is_none() && !input.is_file() {
            return self.toast("Choose the document to run the stylesheet on.", true);
        }
        let dir = std::env::temp_dir().join("xylem-run");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return self.toast(format!("Could not create {}: {e}", dir.display()), true);
        }
        let stem = crate::html_export::safe_file_stem(
            &input
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "input".into()),
        );
        let run_input = match open_text {
            Some(bytes) => {
                let p = dir.join(format!("{stem}-input.xml"));
                if let Err(e) = std::fs::write(&p, bytes) {
                    return self.toast(format!("Could not write {}: {e}", p.display()), true);
                }
                p
            }
            None => input.clone(),
        };
        let engine = self
            .settings
            .xslt_host
            .clone()
            .or_else(|| self.team.xslt_host.clone())
            .or_else(|| std::env::var_os("XYLEM_XSLT_HOST").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(xylem_xslt::host::BUILTIN));
        let mut host = Host::new(engine);
        host.allow_scripts = self.team.xslt_allow_scripts;
        host.label_utf8 = true;
        host.serialize = st.serialize;
        let engine_name = if host.is_builtin() {
            "the built-in engine".to_string()
        } else {
            host.program
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        self.settings.run_stylesheet = Some(st.stylesheet.trim().to_string());
        self.settings.run_input = Some(st.input.trim().to_string());
        self.settings.run_params = st.params.clone();
        let (tx, rx) = channel();
        self.run.job = Some(rx);
        self.run.last = None;
        std::thread::spawn(move || {
            let started = Instant::now();
            // Named apart from the input, which may be open under its own name.
            let out = dir.join(format!("{stem}-result.out"));
            let run = host.run(&stylesheet, &[(run_input, out.clone())], &params, false);
            let result = match run
                .error
                .clone()
                .or_else(|| run.jobs.first().and_then(|j| j.error.clone()))
            {
                Some(e) => Err(e),
                None => std::fs::read(&out)
                    .map_err(|e| format!("{}: {e}", out.display()))
                    .and_then(|bytes| {
                        // The saved-document form is XML (`<div />`); only
                        // the xsl:output form is a page for a browser.
                        let ext = match result_extension(&bytes) {
                            "html" if !host.serialize => "xml",
                            ext => ext,
                        };
                        let named = out.with_extension(ext);
                        std::fs::rename(&out, &named)
                            .map(|_| named)
                            .map_err(|e| e.to_string())
                    }),
            };
            let _ = tx.send(Finished {
                result,
                ms: started.elapsed().as_millis(),
                engine: engine_name,
            });
        });
    }

    fn open_run_result(&mut self, path: PathBuf) {
        let html = path.extension().is_some_and(|e| e == "html");
        if !html {
            // A result already open from an earlier run is reloaded, not
            // shown as it was (unless it has been edited since).
            let key = crate::find_files::path_key(&path);
            if let Some(i) = self.docs.iter().position(|d| {
                d.path
                    .as_deref()
                    .is_some_and(|p| crate::find_files::path_key(p) == key)
            }) {
                if !self.docs[i].editor.is_dirty() {
                    self.reload_document(i);
                }
                self.active = i;
                return;
            }
            return self.open_path(path);
        }
        let headless = self.script.is_some()
            || self.screenshot.is_some()
            || std::env::var_os("XYLEM_NO_BROWSER").is_some();
        if headless {
            return self.toast(format!("Result written to {}", path.display()), false);
        }
        if let Err(e) = crate::html_export::open_in_browser(&path) {
            self.toast(
                format!(
                    "Could not open a browser ({e}); the result is {}",
                    path.display()
                ),
                true,
            );
        }
    }

    pub(super) fn run_stylesheet_window(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.run.job {
            match rx.try_recv() {
                Ok(done) => {
                    self.run.job = None;
                    let opened = done.result.as_ref().ok().cloned();
                    self.run.last = Some(done);
                    if let Some(p) = opened {
                        self.open_run_result(p);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.run.job = None;
                    self.toast("The stylesheet run stopped unexpectedly.", true);
                }
            }
        }
        if !self.run.open {
            return;
        }
        enum Act {
            Run,
            Browse(bool),
            Open(PathBuf),
            Folder(PathBuf),
            SaveSheet(usize),
        }
        let mut acts = Vec::new();
        // An open copy of the stylesheet with unsaved changes runs as saved.
        let sheet_key = crate::find_files::path_key(Path::new(self.run.stylesheet.trim()));
        let dirty_sheet = self.docs.iter().position(|d| {
            d.editor.is_dirty()
                && d.path
                    .as_deref()
                    .is_some_and(|p| crate::find_files::path_key(p) == sheet_key)
        });
        let st = &mut self.run;
        let busy = st.job.is_some();
        let (close, _) = crate::ui::Dialog::new("Run stylesheet")
            .width(640.0)
            .modeless()
            .grow()
            .show(ctx, |ui| {
                crate::ui::form(ui, "run-stylesheet", |f| {
                    f.row("Stylesheet", |ui| {
                        crate::ui::text_field(
                            ui,
                            "Stylesheet",
                            &mut st.stylesheet,
                            "book-to-fo.xsl",
                            420.0,
                        );
                        if crate::ui::button(ui, "Browse…").clicked() {
                            acts.push(Act::Browse(true));
                        }
                    });
                    f.row_with_hint(
                        "Document",
                        "Read as it is in the editor when it is open",
                        |ui| {
                            ui.horizontal(|ui| {
                                crate::ui::text_field(
                                    ui,
                                    "Document",
                                    &mut st.input,
                                    "manual.xml",
                                    420.0,
                                );
                                if crate::ui::button(ui, "Browse…").clicked() {
                                    acts.push(Act::Browse(false));
                                }
                            });
                        },
                    );
                    f.row_with_hint("Parameters", "name=value, separated by commas", |ui| {
                        crate::ui::text_field(ui, "Parameters", &mut st.params, "mode=draft", 420.0)
                    });
                    f.row("Output", |ui| {
                        crate::ui::segmented(
                            ui,
                            &mut st.serialize,
                            &[
                                (false, "As a publishing service saves it"),
                                (true, "As xsl:output asks"),
                            ],
                        );
                    });
                });
                if let Some(i) = dirty_sheet {
                    ui.horizontal(|ui| {
                        crate::ui::notice(
                            ui,
                            crate::ui::Tone::Warning,
                            "The stylesheet has unsaved changes; the saved version runs.",
                        );
                        if crate::ui::button(ui, "Save it").clicked() {
                            acts.push(Act::SaveSheet(i));
                        }
                    });
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if busy {
                        ui.spinner();
                        crate::ui::muted(ui, "Running…");
                    } else if crate::ui::primary_button(ui, "Run").clicked() {
                        acts.push(Act::Run);
                    }
                });
                if let Some(done) = &st.last {
                    ui.add_space(6.0);
                    match &done.result {
                        Ok(p) => {
                            ui.horizontal(|ui| {
                                crate::ui::badge(ui, "Done", crate::ui::Tone::Success);
                                crate::ui::muted(
                                    ui,
                                    format!("{:.2} s on {}", done.ms as f64 / 1000.0, done.engine),
                                );
                                if crate::ui::button(ui, "Open result").clicked() {
                                    acts.push(Act::Open(p.clone()));
                                }
                                if crate::ui::button(ui, "Show folder").clicked()
                                    && let Some(dir) = p.parent()
                                {
                                    acts.push(Act::Folder(dir.to_path_buf()));
                                }
                            });
                            crate::ui::muted(ui, p.display().to_string());
                        }
                        Err(e) => {
                            crate::ui::notice(
                                ui,
                                crate::ui::Tone::Danger,
                                format!("The stylesheet failed: {}", readable_error(e, 12)),
                            );
                        }
                    }
                }
            });
        if close {
            self.run.open = false;
        }
        for a in acts {
            match a {
                Act::Run => self.start_run_stylesheet(),
                Act::Browse(sheet) => {
                    let picked = crate::script::file_dialog(|| {
                        let d = rfd::FileDialog::new();
                        if sheet {
                            d.add_filter("Stylesheet", &["xsl", "xslt"]).pick_file()
                        } else {
                            d.add_filter("XML", &["xml"]).pick_file()
                        }
                    });
                    if let Some(p) = picked {
                        let text = p.display().to_string();
                        if sheet {
                            self.run.stylesheet = text;
                        } else {
                            self.run.input = text;
                        }
                    }
                }
                Act::Open(p) => self.open_run_result(p),
                Act::Folder(p) => {
                    if let Err(e) = crate::html_export::open_in_browser(&p) {
                        self.toast(format!("Could not open {}: {e}", p.display()), true);
                    }
                }
                Act::SaveSheet(i) => {
                    let back = self.active;
                    self.active = i;
                    self.save(false);
                    self.active = back.min(self.docs.len().saturating_sub(1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_repeated_errors_are_shortened() {
        let mut e = String::from("XTDE0000: too deep");
        for _ in 0..5000 {
            e.push_str("\n  in template again (a.xsl:3)");
        }
        e.push_str("\n  in template / (a.xsl:2)");
        let r = readable_error(&e, 12);
        assert_eq!(
            r,
            "XTDE0000: too deep\n  in template again (a.xsl:3)  (×5000)\n  in template / (a.xsl:2)"
        );
        let many: String = (0..30).map(|i| format!("line {i}\n")).collect();
        assert!(readable_error(&many, 12).ends_with("… 18 more lines"));
    }

    #[test]
    fn parameters_and_result_types() {
        let p = parse_params("mode = draft, pageCount=0\nempty=").unwrap();
        assert_eq!(p["mode"], "draft");
        assert_eq!(p["pageCount"], "0");
        assert_eq!(p["empty"], "");
        assert!(parse_params("novalue").is_err());
        assert!(parse_params("=x").is_err());
        assert!(parse_params("").unwrap().is_empty());

        assert_eq!(
            result_extension(b"\xef\xbb\xbf<!DOCTYPE html PUBLIC \"x\"><html>"),
            "html"
        );
        assert_eq!(
            result_extension(b"<fo:root xmlns:fo=\"http://www.w3.org/1999/XSL/Format\"/>"),
            "fo"
        );
        assert_eq!(result_extension(b"<book/>"), "xml");
        assert_eq!(result_extension(b"plain text"), "txt");
    }
}
