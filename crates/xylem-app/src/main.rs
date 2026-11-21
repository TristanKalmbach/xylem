//! Xylem — a structured XML and S1000D editor.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod autosave;
mod doc;
mod find;
mod find_files;
mod fonts;
mod html_export;
mod install;
mod panels;
mod project_ui;
mod review;
mod s1000d_ui;
mod script;
mod settings;
mod spell;
mod team;
mod view;

use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    // --install, --uninstall and friends run and exit without opening a window.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = install::run_cli(&args) {
        std::process::exit(code);
    }
    install::clean_up_old();
    let files: Vec<PathBuf> = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Xylem")
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([640.0, 400.0])
            .with_drag_and_drop(true),
        persist_window: true,
        // With XYLEM_CONFIG_DIR set, everything (eframe's store included)
        // stays inside that folder; otherwise eframe's default location.
        persistence_path: std::env::var_os("XYLEM_CONFIG_DIR")
            .map(|d| PathBuf::from(d).join("app.ron")),
        ..Default::default()
    };
    eframe::run_native(
        "Xylem",
        options,
        Box::new(move |cc| Ok(Box::new(app::XylemApp::new(cc, files)))),
    )
}
