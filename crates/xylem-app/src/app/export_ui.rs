//! File › Export to HTML and File › Print preview.

use super::{XylemApp, ds_resolve};
use crate::doc::DocRules;
use crate::html_export::{ExportInput, ImageRef, open_in_browser, safe_file_stem, to_html};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

impl XylemApp {
    /// The active document as HTML, with a file stem for it.
    fn render_html(&mut self) -> Option<(String, String)> {
        if !self.sync_source_to_doc() {
            self.toast("Fix the well-formedness error before exporting.", true);
            return None;
        }
        let ds = self.docs.get_mut(self.active)?;
        let styles = ds.exact_styles();
        let grammar = ds.grammar.clone();
        let package = ds.package.clone();
        let memo = ds.rules_memo();
        let rules = DocRules::new(grammar.as_deref(), &package, &*styles, memo);
        let dir = ds.dir().map(Path::to_path_buf);
        let images: HashMap<_, _> = ds
            .images()
            .iter()
            .map(|(&n, src)| {
                let path = ds_resolve(dir.as_deref(), src);
                (
                    n,
                    ImageRef {
                        src: src.clone(),
                        path,
                    },
                )
            })
            .collect();
        let stem = ds
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let html = to_html(&ExportInput {
            doc: &ds.editor.doc,
            styles: &*styles,
            rules: &rules,
            images: &images,
            title: &stem,
        });
        Some((html, stem))
    }

    pub(super) fn export_html(&mut self) {
        let Some((html, stem)) = self.render_html() else {
            return;
        };
        let dir = self
            .docs
            .get(self.active)
            .and_then(|d| d.dir().map(Path::to_path_buf));
        let Some(path) = crate::script::file_dialog(|| {
            let mut d = rfd::FileDialog::new()
                .add_filter("HTML", &["html", "htm"])
                .set_file_name(format!("{stem}.html"));
            if let Some(dir) = &dir {
                d = d.set_directory(dir);
            }
            d.save_file()
        }) else {
            return;
        };
        match std::fs::write(&path, html) {
            Ok(()) => self.toast(format!("Exported {}", path.display()), false),
            Err(e) => self.toast(format!("Export failed: {e}"), true),
        }
    }

    /// Writes the HTML to a temporary file and opens it in the browser,
    /// whose print dialog does the paging (and can save as PDF). Scripted
    /// test runs, and `XYLEM_NO_BROWSER`, write the file without opening it.
    pub(super) fn print_preview(&mut self) {
        let Some((html, stem)) = self.render_html() else {
            return;
        };
        let dir = std::env::temp_dir().join("xylem-print");
        // The name is built here from safe characters only, because on
        // Windows it passes through cmd.exe.
        let path: PathBuf = dir.join(format!("{}.html", safe_file_stem(&stem)));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, html)) {
            return self.toast(format!("Print preview failed: {e}"), true);
        }
        let headless = self.script.is_some()
            || self.screenshot.is_some()
            || std::env::var_os("XYLEM_NO_BROWSER").is_some();
        if headless {
            return self.toast(
                format!("Print preview written to {}", path.display()),
                false,
            );
        }
        match open_in_browser(&path) {
            Ok(()) => self.toast("Print preview opened in your browser.", false),
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
