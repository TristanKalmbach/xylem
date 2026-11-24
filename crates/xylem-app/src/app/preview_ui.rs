//! File > Publication preview: runs the publishing stylesheets set up in the
//! team configuration on the document as it is in the editor (unsaved edits
//! included) and opens the result in the browser. With an FO-to-HTML
//! stylesheet that is a readable page with the real content, numbering and
//! boilerplate; it is not paginated like the PDF.

use super::XylemApp;
use crate::team::PreviewConfig;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;
use xylem_xslt::Host;

#[derive(Default)]
pub struct PreviewState {
    job: Option<Receiver<Result<(PathBuf, u128), String>>>,
}

fn run_preview(
    host: &Host,
    cfg: &PreviewConfig,
    input: PathBuf,
    dir: PathBuf,
    stem: &str,
) -> Result<PathBuf, String> {
    let fo = dir.join(format!("{stem}.fo"));
    let first = host.run(
        &cfg.stylesheet,
        &[(input, fo.clone())],
        &cfg.param_strings(),
        false,
    );
    if let Some(e) = first
        .error
        .or_else(|| first.jobs.first().and_then(|j| j.error.clone()))
    {
        return Err(format!("The publication stylesheet failed: {e}"));
    }
    let Some(html_sheet) = &cfg.html else {
        return Ok(fo);
    };
    let html = dir.join(format!("{stem}.html"));
    // The second step runs as a publishing service runs it on text it
    // already has: the FO is read as it is and the HTML written as the
    // stylesheet's xsl:output asks.
    let mut html_host = host.clone();
    html_host.serialize = true;
    let second = html_host.run(html_sheet, &[(fo, html.clone())], &BTreeMap::new(), false);
    if let Some(e) = second
        .error
        .or_else(|| second.jobs.first().and_then(|j| j.error.clone()))
    {
        return Err(format!("Turning the output into HTML failed: {e}"));
    }
    Ok(html)
}

impl XylemApp {
    pub(super) fn publication_preview(&mut self) {
        if self.preview.job.is_some() {
            return self.toast("A preview is already being made.", false);
        }
        let Some(ds) = self.docs.get(self.active) else {
            return;
        };
        let facts = crate::team::DocFacts::of(&ds.editor.doc, ds.path.as_deref());
        // The user's own previews first, then the team's.
        let Some(cfg) = self
            .settings
            .preview
            .iter()
            .filter_map(|p| p.to_config())
            .chain(self.team.preview.iter().cloned())
            .find(|p| p.applies_to(&facts))
        else {
            return self.toast(
                format!(
                    "No publication preview is set up for <{}> documents. Add one in File > \
                     Preferences > Stylesheets, or for everyone in the team configuration \
                     ([[preview]], see docs/stylesheet-check.md).",
                    facts.root
                ),
                true,
            );
        };
        let engine = self
            .settings
            .xslt_host
            .clone()
            .or_else(|| self.team.xslt_host.clone())
            .or_else(|| std::env::var_os("XYLEM_XSLT_HOST").map(PathBuf::from));
        let Some(engine) = engine else {
            return self.toast(
                "No stylesheet engine is set up. Choose it in File > Preferences > Stylesheets (or xslt_host in the team configuration).",
                true,
            );
        };
        let stem = crate::html_export::safe_file_stem(
            &ds.path
                .as_ref()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "preview".into()),
        );
        let dir = std::env::temp_dir().join("xylem-preview");
        let input = dir.join(format!("{stem}-input.xml"));
        // The document as it is in the editor, unsaved edits included.
        let bytes = ds.editor.doc.to_bytes();
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&input, bytes)) {
            return self.toast(format!("Could not write the preview input: {e}"), true);
        }
        let mut host = Host::new(engine);
        host.allow_scripts = self.team.xslt_allow_scripts;
        let (tx, rx) = channel();
        self.preview.job = Some(rx);
        let name = cfg.name.clone().unwrap_or_else(|| "publication".into());
        self.toast(format!("Making the {name} preview…"), false);
        std::thread::spawn(move || {
            let started = Instant::now();
            let _cancel = AtomicBool::new(false);
            let result = run_preview(&host, &cfg, input, dir, &stem)
                .map(|p| (p, started.elapsed().as_millis()));
            let _ = tx.send(result);
        });
    }

    pub(super) fn poll_preview(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.preview.job else { return };
        match rx.try_recv() {
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.preview.job = None;
                self.toast("The preview stopped unexpectedly.", true);
            }
            Ok(Err(e)) => {
                self.preview.job = None;
                self.toast(e, true);
            }
            Ok(Ok((path, ms))) => {
                self.preview.job = None;
                let is_html = path.extension().is_some_and(|e| e == "html");
                if !is_html {
                    // No HTML step: show the stylesheet's output itself.
                    self.open_path(path);
                    return;
                }
                let headless = self.script.is_some()
                    || self.screenshot.is_some()
                    || std::env::var_os("XYLEM_NO_BROWSER").is_some();
                if headless {
                    return self.toast(
                        format!("Preview written to {} ({ms} ms)", path.display()),
                        false,
                    );
                }
                match crate::html_export::open_in_browser(&path) {
                    Ok(()) => self.toast(
                        format!(
                            "Preview opened in your browser ({:.1} s).",
                            ms as f64 / 1000.0
                        ),
                        false,
                    ),
                    Err(e) => self.toast(
                        format!(
                            "Could not open a browser ({e}); the preview is {}",
                            path.display()
                        ),
                        true,
                    ),
                }
            }
        }
    }
}
