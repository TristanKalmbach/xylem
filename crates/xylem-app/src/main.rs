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
mod ui;
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
    // Scripted test runs: put the window where XYLEM_WINDOW_POS says ("x,y",
    // in screen points, e.g. on a second monitor) and don't take focus, so
    // tests can run while someone keeps using the machine.
    let scripted = std::env::var_os("XYLEM_SCRIPT").is_some();
    let window_pos = std::env::var("XYLEM_WINDOW_POS").ok().and_then(|v| {
        let (x, y) = v.split_once(',')?;
        Some([x.trim().parse::<f32>().ok()?, y.trim().parse::<f32>().ok()?])
    });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Xylem")
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([640.0, 400.0])
        .with_drag_and_drop(true);
    if let Some(pos) = window_pos {
        viewport = viewport.with_position(pos);
    }
    if scripted {
        // Test windows stay out of the way: no focus, no taskbar button, and
        // low CPU priority so they don't slow down whatever else is running.
        viewport = viewport.with_active(false).with_taskbar(false);
        lower_priority();
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: !scripted && window_pos.is_none(),
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

/// Runs this process at idle priority (scripted test runs only).
fn lower_priority() {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> isize;
            fn SetPriorityClass(process: isize, class: u32) -> i32;
        }
        const IDLE_PRIORITY_CLASS: u32 = 0x40;
        // SAFETY: plain Win32 calls on our own process handle.
        unsafe {
            SetPriorityClass(GetCurrentProcess(), IDLE_PRIORITY_CLASS);
        }
    }
}
