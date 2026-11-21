//! The log file and crash reports.
//!
//! Release builds on Windows have no console, so log output also goes to
//! `logs/xylem.log` in the config folder (rotated at 2 MB, three files
//! kept). If Xylem panics, a crash report with the message, the place in the
//! code and a backtrace is written next to it, and the next start says so.

use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;
const KEPT_LOGS: usize = 3;
const KEPT_CRASH_REPORTS: usize = 10;
const LOG_NAME: &str = "xylem.log";

/// Where logs and crash reports go.
pub fn log_dir() -> Option<PathBuf> {
    crate::settings::config_dir().map(|d| d.join("logs"))
}

/// Writes every log line to stderr and the log file.
struct Tee {
    file: Option<std::fs::File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        if let Some(f) = &mut self.file {
            let _ = f.write_all(buf);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(f) = &mut self.file {
            let _ = f.flush();
        }
        Ok(())
    }
}

/// Sets up logging and the crash handler. Call once, first thing.
pub fn init() {
    let file = log_dir().and_then(|dir| {
        std::fs::create_dir_all(&dir).ok()?;
        rotate(&dir);
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_NAME))
            .ok()
    });
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(Box::new(Tee { file })))
        .init();
    install_panic_hook();
}

/// Moves `xylem.log` to `xylem.1.log` (and so on) once it is too big.
fn rotate(dir: &Path) {
    let current = dir.join(LOG_NAME);
    let big = std::fs::metadata(&current).is_ok_and(|m| m.len() > MAX_LOG_BYTES);
    if !big {
        return;
    }
    let numbered = |i: usize| dir.join(format!("xylem.{i}.log"));
    let _ = std::fs::remove_file(numbered(KEPT_LOGS - 1));
    for i in (1..KEPT_LOGS - 1).rev() {
        let _ = std::fs::rename(numbered(i), numbered(i + 1));
    }
    let _ = std::fs::rename(&current, numbered(1));
}

fn install_panic_hook() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".into());
        let place = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "(unknown)".into());
        log::error!("crash: {message} at {place}");
        let report = format!(
            "Xylem {version} closed unexpectedly.\n\n\
             Time:     {time}\n\
             System:   {os} {arch}\n\
             Thread:   {thread}\n\
             Message:  {message}\n\
             Location: {place}\n\n\
             Backtrace:\n{backtrace}\n",
            version = crate::install::VERSION,
            time = crate::review::now(),
            os = std::env::consts::OS,
            arch = std::env::consts::ARCH,
            thread = std::thread::current().name().unwrap_or("unnamed"),
            backtrace = std::backtrace::Backtrace::force_capture(),
        );
        if let Some(dir) = log_dir()
            && std::fs::create_dir_all(&dir).is_ok()
        {
            let file = dir.join(format!(
                "crash-{}.txt",
                crate::review::now().replace('+', "Z")
            ));
            let _ = std::fs::write(file, report);
            prune_crash_reports(&dir);
        }
        default(info);
    }));
}

fn crash_reports(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    // The names start with a sortable time stamp.
    v.sort();
    v
}

fn prune_crash_reports(dir: &Path) {
    let reports = crash_reports(dir);
    let extra = reports.len().saturating_sub(KEPT_CRASH_REPORTS);
    for p in reports.into_iter().take(extra) {
        let _ = std::fs::remove_file(p);
    }
}

/// The newest crash report nobody has been told about yet. Reports are
/// marked as seen by renaming them, so each is announced once.
pub fn take_unseen_crash_report() -> Option<PathBuf> {
    let dir = log_dir()?;
    let unseen: Vec<PathBuf> = crash_reports(&dir)
        .into_iter()
        .filter(|p| !p.to_string_lossy().ends_with(".seen.txt"))
        .collect();
    let newest = unseen.last()?.clone();
    let mut shown = newest.clone();
    for p in unseen {
        let seen = p.with_extension("seen.txt");
        if std::fs::rename(&p, &seen).is_ok() && p == newest {
            shown = seen;
        }
    }
    Some(shown)
}
