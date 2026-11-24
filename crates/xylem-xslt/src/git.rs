//! The committed version of a stylesheet, for comparing a working copy with
//! it.
//!
//! Stylesheets import and include each other and read files next to them, so
//! the whole folder tree they live in is needed, not one file. [`committed_copy`]
//! writes the stylesheet-related files of the top-level folder holding the
//! stylesheet (as committed at a revision) to a cache folder, once per
//! commit, and returns the stylesheet's path inside that copy.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// File types copied: stylesheets and the XML, entity and DTD files they
/// read. Graphics and everything else are left out.
const EXTENSIONS: &[&str] = &["xsl", "xslt", "xml", "ent", "dtd", "mod", "xsd"];

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    no_window(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("Could not run git: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

fn no_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// A path without Windows' `\\?\` prefix, so it compares with git's paths.
fn plain(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p.to_path_buf(),
    }
}

/// Where `stylesheet` lies inside its git repository: (repository root,
/// path inside it).
pub fn locate(stylesheet: &Path) -> Result<(PathBuf, PathBuf), String> {
    let full = plain(
        &stylesheet
            .canonicalize()
            .map_err(|e| format!("{}: {e}", stylesheet.display()))?,
    );
    let dir = full.parent().ok_or("The stylesheet has no folder.")?;
    let top = git(dir, &["rev-parse", "--show-toplevel"])
        .map_err(|e| format!("{} is not in a git repository ({e}).", stylesheet.display()))?;
    let top = plain(
        &PathBuf::from(top)
            .canonicalize()
            .map_err(|e| e.to_string())?,
    );
    let rel = full
        .strip_prefix(&top)
        .map_err(|_| format!("{} is outside {}.", full.display(), top.display()))?
        .to_path_buf();
    Ok((top, rel))
}

/// Copies the committed stylesheet tree holding `stylesheet` at `rev` into
/// `cache` (reused if already there) and returns the stylesheet's path in
/// the copy, with the commit it came from.
pub fn committed_copy(
    stylesheet: &Path,
    rev: &str,
    cache: &Path,
) -> Result<(PathBuf, String), String> {
    let (top, rel) = locate(stylesheet)?;
    let commit = git(
        &top,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
    )
    .map_err(|e| format!("No commit {rev}: {e}"))?;
    let short = &commit[..commit.len().min(12)];
    let dest = cache.join(short);
    let rel_git = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    if rel_git.is_empty() {
        return Err("The stylesheet path is empty.".into());
    }
    // The stylesheet's own top-level folder (imports climb within it).
    let scope = if rel_git.len() > 1 {
        rel_git[0].clone()
    } else {
        String::new()
    };
    let done = dest.join(format!(
        ".complete-{}",
        if scope.is_empty() { "root" } else { &scope }
    ));
    if !done.exists() {
        export(&top, &commit, &scope, &dest)?;
        std::fs::write(&done, &commit).map_err(|e| e.to_string())?;
    }
    let copy = dest.join(rel_git.join("/"));
    if !copy.exists() {
        return Err(format!(
            "{} is not in commit {short}: it may be new since then.",
            rel_git.join("/")
        ));
    }
    Ok((copy, commit))
}

/// Writes the stylesheet-related files under `scope` at `commit` to `dest`.
fn export(top: &Path, commit: &str, scope: &str, dest: &Path) -> Result<(), String> {
    let mut ls = Command::new("git");
    ls.arg("-C")
        .arg(top)
        .args(["ls-tree", "-r", "-z", "--name-only", commit]);
    if !scope.is_empty() {
        ls.arg("--").arg(scope);
    }
    no_window(&mut ls);
    let listing = ls.output().map_err(|e| format!("Could not run git: {e}"))?;
    if !listing.status.success() {
        return Err(String::from_utf8_lossy(&listing.stderr).trim().to_string());
    }
    let paths: Vec<String> = listing
        .stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .filter(|p| {
            Path::new(p)
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
        })
        .collect();

    let mut cat = Command::new("git");
    cat.arg("-C").arg(top).args(["cat-file", "--batch"]);
    cat.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    no_window(&mut cat);
    let mut child = cat.spawn().map_err(|e| format!("Could not run git: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("git cat-file has no input")?;
    let requests: Vec<String> = paths.iter().map(|p| format!("{commit}:{p}\n")).collect();
    let writer = std::thread::spawn(move || {
        for r in requests {
            if stdin.write_all(r.as_bytes()).is_err() {
                break;
            }
        }
    });
    let mut out = BufReader::new(child.stdout.take().ok_or("git cat-file has no output")?);
    for p in &paths {
        let mut header = String::new();
        out.read_line(&mut header).map_err(|e| e.to_string())?;
        // "<sha> blob <size>" or "<request> missing"
        let size: usize = match header
            .trim_end()
            .rsplit(' ')
            .next()
            .and_then(|s| s.parse().ok())
        {
            Some(n) if header.contains(" blob ") => n,
            _ => continue,
        };
        let mut data = vec![0; size];
        out.read_exact(&mut data).map_err(|e| e.to_string())?;
        let mut newline = [0u8; 1];
        out.read_exact(&mut newline).map_err(|e| e.to_string())?;
        let target = dest.join(p);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, data).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    let _ = writer.join();
    let _ = child.wait();
    Ok(())
}
