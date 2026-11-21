//! Per-user installation and team updates.
//!
//! Xylem is one executable, so installing means copying it somewhere stable
//! and, on Windows, adding a Start Menu shortcut and an entry in
//! "Installed apps". No administrator rights are needed: everything goes
//! under the user's profile.
//!
//! Updates come from a folder the team controls (`update_source` in
//! `xylem-team.toml`): whoever builds a new version runs
//! `xylem --publish-update <folder>`, which copies the executable there with a
//! `version.txt`. Installations compare that version with their own and, when
//! it is newer, swap the executable in place. A running executable cannot be
//! overwritten on Windows but it can be renamed, so the old one is moved
//! aside and deleted on the next start.
//!
//! Command line:
//!
//! ```text
//! xylem --install [--team <path/to/xylem-team.toml>]
//! xylem --uninstall
//! xylem --publish-update <folder>
//! xylem --update        (from the team's update folder, without the editor)
//! xylem --version
//! ```
//!
//! `XYLEM_INSTALL_DIR` overrides the install folder; with it set, the
//! shortcut and the "Installed apps" entry are skipped (portable installs and
//! tests).

use std::path::{Path, PathBuf};

const EXE_NAME: &str = if cfg!(windows) { "xylem.exe" } else { "xylem" };
const NEW_SUFFIX: &str = ".new";
const OLD_SUFFIX: &str = ".old";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(windows)]
const UNINSTALL_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Xylem";

/// Runs a command-line action if the arguments ask for one. Returns the exit
/// code, or `None` to start the editor normally.
pub fn run_cli(args: &[String]) -> Option<i32> {
    let first = args.first()?.as_str();
    let result = match first {
        "--version" => {
            say(&format!("Xylem {VERSION}"));
            Ok(())
        }
        "--install" => {
            let team = flag_value(args, "--team").map(PathBuf::from);
            install(team.as_deref())
        }
        "--uninstall" => uninstall(),
        "--update" => update_now(),
        "--publish-update" => match args.get(1) {
            Some(dir) => publish_update(Path::new(dir)),
            None => Err("usage: xylem --publish-update <folder>".into()),
        },
        "--help" | "-h" => {
            say(
                "usage: xylem [files...]\n       xylem --install [--team <xylem-team.toml>]\n       xylem --uninstall\n       xylem --publish-update <folder>\n       xylem --update\n       xylem --version",
            );
            Ok(())
        }
        other if other.starts_with("--") => Err(format!("unknown option {other}; try --help")),
        _ => return None,
    };
    Some(match result {
        Ok(()) => 0,
        Err(e) => {
            say(&format!("xylem: {e}"));
            1
        }
    })
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

/// Prints to the console that started us. Release builds on Windows are GUI
/// programs with no console of their own, so attach to the parent's first.
fn say(msg: &str) {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn AttachConsole(process_id: u32) -> i32;
        }
        const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
        // SAFETY: plain Win32 call with a constant argument; failure (no
        // parent console, or already attached) is harmless.
        unsafe {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
    println!("{msg}");
}

/// Where `--install` puts the executable.
pub fn install_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XYLEM_INSTALL_DIR") {
        return Some(PathBuf::from(dir));
    }
    let home = || std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Programs").join("Xylem"))
    } else if cfg!(target_os = "macos") {
        home().map(|h| h.join("Applications").join("Xylem"))
    } else {
        home().map(|h| h.join(".local").join("bin"))
    }
}

fn integrate() -> bool {
    std::env::var_os("XYLEM_INSTALL_DIR").is_none()
}

fn install(team: Option<&Path>) -> Result<(), String> {
    let dir = install_dir().ok_or("cannot work out the install folder")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = dir.join(EXE_NAME);
    if !same_file(&exe, &target) {
        copy_replacing(&exe, &target)?;
    }
    if let Some(team) = team {
        let team = std::path::absolute(team).map_err(|e| e.to_string())?;
        if !team.is_file() {
            return Err(format!("{}: no such file", team.display()));
        }
        // A pointer rather than a copy, so the shared file's relative paths
        // keep resolving against the shared folder.
        let stub = format!(
            "# Written by `xylem --install`; the team configuration lives in the shared folder.\ninclude = {}\n",
            toml_string(&team.to_string_lossy())
        );
        std::fs::write(dir.join("xylem-team.toml"), stub).map_err(|e| e.to_string())?;
    }
    if integrate() {
        platform::integrate(&target)?;
    }
    say(&format!(
        "Installed Xylem {VERSION} to {}",
        target.display()
    ));
    Ok(())
}

fn uninstall() -> Result<(), String> {
    let dir = install_dir().ok_or("cannot work out the install folder")?;
    if integrate() {
        platform::remove_integration()?;
    }
    let mut files = vec![dir.join(EXE_NAME), dir.join("xylem-team.toml")];
    files.push(sibling(&dir.join(EXE_NAME), OLD_SUFFIX));
    files.push(sibling(&dir.join(EXE_NAME), NEW_SUFFIX));
    let exe = std::env::current_exe().ok();
    let mut running_from_dir = false;
    for f in files {
        if exe.as_deref().is_some_and(|e| same_file(e, &f)) {
            running_from_dir = true;
            continue;
        }
        let _ = std::fs::remove_file(&f);
    }
    if running_from_dir {
        platform::delete_after_exit(&dir.join(EXE_NAME), &dir)?;
    } else if !cfg!(target_os = "linux") || std::env::var_os("XYLEM_INSTALL_DIR").is_some() {
        // ~/.local/bin is shared with other programs; only remove our own folder.
        let _ = std::fs::remove_dir(&dir);
    }
    say(&format!("Removed Xylem from {}", dir.display()));
    Ok(())
}

/// Copies this executable into `dir` with a `version.txt` describing it.
fn publish_update(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let target = dir.join(EXE_NAME);
    // Copy under a temporary name first so that nobody updates from a
    // half-written file.
    let tmp = sibling(&target, NEW_SUFFIX);
    std::fs::copy(&exe, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    let size = std::fs::metadata(&tmp).map_err(|e| e.to_string())?.len();
    std::fs::rename(&tmp, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    let info = format!("version = \"{VERSION}\"\nsize = {size}\n");
    std::fs::write(dir.join("version.txt"), info).map_err(|e| e.to_string())?;
    say(&format!("Published Xylem {VERSION} to {}", dir.display()));
    Ok(())
}

fn update_now() -> Result<(), String> {
    let source = crate::team::load()
        .update_source
        .ok_or("no update_source in the team configuration (xylem-team.toml)")?;
    match check(&source)? {
        None => say(&format!("Xylem {VERSION} is up to date.")),
        Some(a) => {
            let exe = apply(&a)?;
            say(&format!(
                "Updated {} to Xylem {}.",
                exe.display(),
                a.version
            ));
        }
    }
    Ok(())
}

/// A newer version found in the update folder.
#[derive(Debug, Clone)]
pub struct Available {
    pub version: String,
    exe: PathBuf,
    size: u64,
}

#[derive(serde::Deserialize)]
struct VersionFile {
    version: String,
    size: u64,
}

/// Looks for a newer version in `source`. `Ok(None)` means up to date.
pub fn check(source: &Path) -> Result<Option<Available>, String> {
    let text = std::fs::read_to_string(source.join("version.txt"))
        .map_err(|e| format!("{}: {e}", source.join("version.txt").display()))?;
    let info: VersionFile = toml::from_str(&text).map_err(|e| format!("version.txt: {e}"))?;
    if !newer(&info.version, VERSION) {
        return Ok(None);
    }
    Ok(Some(Available {
        version: info.version,
        exe: source.join(EXE_NAME),
        size: info.size,
    }))
}

/// Replaces the running executable with `update`. Takes effect on restart.
pub fn apply(update: &Available) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let new = sibling(&exe, NEW_SUFFIX);
    let old = sibling(&exe, OLD_SUFFIX);
    let fail = |what: &str, e: std::io::Error| {
        format!(
            "{what}: {e}. If Xylem is installed somewhere you cannot write to, ask whoever installed it to update it."
        )
    };
    std::fs::copy(&update.exe, &new).map_err(|e| fail(&new.display().to_string(), e))?;
    let got = std::fs::metadata(&new).map(|m| m.len()).unwrap_or(0);
    if got != update.size {
        let _ = std::fs::remove_file(&new);
        return Err(format!(
            "the copied file is {got} bytes, version.txt says {}; the update folder may still be being written",
            update.size
        ));
    }
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| fail("moving the running program aside", e))?;
    if let Err(e) = std::fs::rename(&new, &exe) {
        // Put the old one back so the installation keeps working.
        let _ = std::fs::rename(&old, &exe);
        return Err(fail("putting the new program in place", e));
    }
    Ok(exe)
}

/// Deletes the executable left behind by the last update.
pub fn clean_up_old() {
    if let Ok(exe) = std::env::current_exe() {
        let old = sibling(&exe, OLD_SUFFIX);
        if old.exists() {
            let _ = std::fs::remove_file(&old);
        }
    }
}

/// True when `a` is a higher dotted version than `b` ("0.10.0" > "0.9.3").
/// Anything after a `-` is ignored.
fn newer(a: &str, b: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split('-')
            .next()
            .unwrap_or("")
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).copied().unwrap_or(0),
            b.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

/// `xylem.exe` -> `xylem.new.exe`; `xylem` -> `xylem.new`.
fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match exe.extension() {
        Some(ext) => format!("{stem}{suffix}.{}", ext.to_string_lossy()),
        None => format!("{stem}{suffix}"),
    };
    exe.with_file_name(name)
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn copy_replacing(from: &Path, to: &Path) -> Result<(), String> {
    let tmp = sibling(to, NEW_SUFFIX);
    std::fs::copy(from, &tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if to.exists() {
        // The installed copy may be running; rename works where delete does not.
        let old = sibling(to, OLD_SUFFIX);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(to, &old).map_err(|e| format!("{}: {e}", to.display()))?;
    }
    std::fs::rename(&tmp, to).map_err(|e| format!("{}: {e}", to.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// A TOML basic string (quotes and backslashes escaped).
fn toml_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(windows)]
mod platform {
    //! Start Menu shortcut and the "Installed apps" entry. Both use tools
    //! that ship with Windows (PowerShell for the shortcut, `reg` for the
    //! registry) so the editor needs no extra dependencies for them.
    use super::{UNINSTALL_KEY, VERSION};
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn shortcut_path() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|d| PathBuf::from(d).join(r"Microsoft\Windows\Start Menu\Programs\Xylem.lnk"))
    }

    fn run(cmd: &mut Command) -> Result<(), String> {
        let out = cmd
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    fn ps_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "''"))
    }

    pub fn integrate(exe: &Path) -> Result<(), String> {
        let lnk = shortcut_path().ok_or("APPDATA is not set")?;
        let exe_s = exe.display().to_string();
        let dir_s = exe
            .parent()
            .map(|d| d.display().to_string())
            .unwrap_or_default();
        let script = format!(
            "$s = (New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath = {}; $s.WorkingDirectory = {}; $s.Description = 'Xylem XML editor'; $s.Save()",
            ps_quote(&lnk.display().to_string()),
            ps_quote(&exe_s),
            ps_quote(&dir_s)
        );
        run(Command::new("powershell").args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script,
        ]))
        .map_err(|e| format!("creating the Start Menu shortcut: {e}"))?;
        let entries = [
            ("DisplayName", "Xylem".to_string()),
            ("DisplayVersion", VERSION.to_string()),
            ("Publisher", "Xylem contributors".to_string()),
            ("DisplayIcon", exe_s.clone()),
            ("InstallLocation", dir_s),
            ("UninstallString", format!("\"{exe_s}\" --uninstall")),
        ];
        for (name, value) in entries {
            run(Command::new("reg").args([
                "add",
                UNINSTALL_KEY,
                "/v",
                name,
                "/t",
                "REG_SZ",
                "/d",
                &value,
                "/f",
            ]))
            .map_err(|e| format!("registering with Installed apps: {e}"))?;
        }
        for name in ["NoModify", "NoRepair"] {
            run(Command::new("reg").args([
                "add",
                UNINSTALL_KEY,
                "/v",
                name,
                "/t",
                "REG_DWORD",
                "/d",
                "1",
                "/f",
            ]))
            .map_err(|e| format!("registering with Installed apps: {e}"))?;
        }
        Ok(())
    }

    pub fn remove_integration() -> Result<(), String> {
        if let Some(lnk) = shortcut_path() {
            let _ = std::fs::remove_file(lnk);
        }
        // Fails harmlessly when the key is already gone.
        let _ = run(Command::new("reg").args(["delete", UNINSTALL_KEY, "/f"]));
        Ok(())
    }

    /// The running executable cannot delete itself; leave a short-lived
    /// `cmd` behind to do it once we have exited.
    pub fn delete_after_exit(exe: &Path, dir: &Path) -> Result<(), String> {
        let script = format!(
            "ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"",
            exe.display(),
            dir.display()
        );
        // raw_arg: cmd.exe does its own parsing and does not understand the
        // backslash escaping Rust applies to quotes in normal arguments.
        Command::new("cmd")
            .arg("/c")
            .raw_arg(&script)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
mod platform {
    //! A desktop entry on Linux; nothing extra on macOS (the binary goes in
    //! ~/Applications/Xylem).
    use std::path::{Path, PathBuf};

    fn desktop_file() -> Option<PathBuf> {
        if cfg!(target_os = "macos") {
            return None;
        }
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
        Some(data.join("applications").join("xylem.desktop"))
    }

    pub fn integrate(exe: &Path) -> Result<(), String> {
        if let Some(file) = desktop_file() {
            if let Some(dir) = file.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let entry = format!(
                "[Desktop Entry]\nType=Application\nName=Xylem\nComment=Structured XML and S1000D editor\nExec=\"{}\" %F\nTerminal=false\nCategories=Office;TextEditor;\nMimeType=application/xml;text/xml;\n",
                exe.display()
            );
            std::fs::write(&file, entry).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn remove_integration() -> Result<(), String> {
        if let Some(file) = desktop_file() {
            let _ = std::fs::remove_file(file);
        }
        Ok(())
    }

    /// Unix lets a running program delete its own file.
    pub fn delete_after_exit(exe: &Path, _dir: &Path) -> Result<(), String> {
        std::fs::remove_file(exe).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("0.10.0", "0.9.3"));
        assert!(newer("1.0", "0.99.99"));
        assert!(newer("v0.2.0", "0.1.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0-rc1", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"), "never offer a downgrade");
    }

    #[test]
    fn sibling_names() {
        assert_eq!(
            sibling(Path::new("a/xylem.exe"), ".old"),
            PathBuf::from("a/xylem.old.exe")
        );
        assert_eq!(
            sibling(Path::new("a/xylem"), ".new"),
            PathBuf::from("a/xylem.new")
        );
    }

    #[test]
    fn toml_strings_escape_windows_paths() {
        let s = toml_string(r"\\server\share\xylem-team.toml");
        let v: toml::Table = toml::from_str(&format!("x = {s}")).unwrap();
        assert_eq!(v["x"].as_str(), Some(r"\\server\share\xylem-team.toml"));
    }
}
