//! Keeping documents on disk safe.
//!
//! * **Lock files.** While a document is open, Xylem leaves a small hidden
//!   `.~lock.<name>#` file next to it saying who has it open (the same
//!   convention LibreOffice uses). Opening a file someone else holds offers
//!   a read-only copy instead of letting two people overwrite each other.
//! * **Backups.** Before a save replaces a file, the previous version is
//!   copied to `backups/` in the config folder; the last ten are kept per
//!   document (File › Previous versions).
//! * **Change stamps.** The size and modification time of each open file
//!   are remembered, so the app can notice when something else changes it.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A file's modification time and size, compared to notice outside changes.
pub type Stamp = (SystemTime, u64);

pub fn stamp(path: &Path) -> Option<Stamp> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

// ------------------------------------------------------------------ locks

/// Who holds a lock file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockOwner {
    pub user: String,
    pub host: String,
    pub since: String,
    pid: u32,
}

impl LockOwner {
    fn me() -> Self {
        Self {
            user: crate::review::os_user_name(),
            host: host_name(),
            since: crate::review::now(),
            pid: std::process::id(),
        }
    }

    /// Same person on the same machine: their own lock, perhaps left behind
    /// by a crash, which they may simply take over.
    fn is_mine(&self) -> bool {
        let me = Self::me();
        self.user.eq_ignore_ascii_case(&me.user) && self.host.eq_ignore_ascii_case(&me.host)
    }

    fn to_text(&self) -> String {
        format!(
            "{}\n{}\n{}\n{}\n",
            self.user, self.host, self.pid, self.since
        )
    }

    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        Some(Self {
            user: lines.next()?.trim().to_string(),
            host: lines.next()?.trim().to_string(),
            pid: lines.next()?.trim().parse().ok()?,
            since: lines.next().unwrap_or("").trim().to_string(),
        })
    }
}

fn host_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

fn lock_path(doc: &Path) -> Option<PathBuf> {
    let name = doc.file_name()?.to_string_lossy();
    Some(doc.with_file_name(format!(".~lock.{name}#")))
}

/// The other person who has `doc` open, if anyone.
pub fn held_by_someone_else(doc: &Path) -> Option<LockOwner> {
    let text = std::fs::read_to_string(lock_path(doc)?).ok()?;
    LockOwner::parse(&text).filter(|o| !o.is_mine())
}

/// A lock this instance holds; dropping it removes the lock file.
pub struct Lock {
    file: PathBuf,
}

impl Lock {
    /// Takes the lock on `doc`. `None` when the folder can't be written to,
    /// which only means no lock (read-only shares, for example).
    pub fn acquire(doc: &Path) -> Option<Self> {
        let file = lock_path(doc)?;
        write_hidden(&file, LockOwner::me().to_text().as_bytes()).ok()?;
        Some(Self { file })
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        // Only remove it if it is still ours: someone may have taken over.
        let ours = std::fs::read_to_string(&self.file)
            .ok()
            .and_then(|t| LockOwner::parse(&t))
            .is_some_and(|o| o.is_mine() && o.pid == std::process::id());
        if ours {
            let _ = std::fs::remove_file(&self.file);
        }
    }
}

fn write_hidden(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        options.attributes(FILE_ATTRIBUTE_HIDDEN);
    }
    // Windows refuses to truncate a hidden file opened without the hidden
    // attribute, and the reverse; remove any earlier copy first.
    let _ = std::fs::remove_file(path);
    options.open(path)?.write_all(bytes)
}

// ------------------------------------------------------------------ backups

const KEPT_BACKUPS: usize = 10;

/// FNV-1a, so a document's backup folder name stays the same across builds.
fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn backup_dir(doc: &Path) -> Option<PathBuf> {
    let key = std::path::absolute(doc).unwrap_or_else(|_| doc.to_path_buf());
    let key = key.to_string_lossy().to_lowercase();
    crate::settings::config_dir().map(|d| d.join("backups").join(format!("{:016x}", fnv(&key))))
}

/// Copies the current version of `doc` aside before it is overwritten.
/// Failures are logged and otherwise ignored: a missing backup must never
/// stop a save.
pub fn back_up(doc: &Path) {
    if !doc.is_file() {
        return;
    }
    let Some(dir) = backup_dir(doc) else { return };
    let stem = doc
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = doc
        .extension()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "xml".into());
    let time = crate::review::now().replace('+', "Z");
    let copy = dir.join(format!("{stem}.{time}.{ext}"));
    let result = std::fs::create_dir_all(&dir)
        .and_then(|_| std::fs::write(dir.join("source.txt"), doc.display().to_string()))
        .and_then(|_| std::fs::copy(doc, &copy).map(|_| ()));
    if let Err(e) = result {
        log::warn!("backup of {}: {e}", doc.display());
        return;
    }
    let all = backups(doc);
    for (old, _) in all.into_iter().skip(KEPT_BACKUPS) {
        let _ = std::fs::remove_file(old);
    }
}

/// The folder holding `doc`'s backups.
pub fn backups_folder(doc: &Path) -> Option<PathBuf> {
    backup_dir(doc)
}

/// Earlier versions of `doc`, newest first, with the time each was saved.
pub fn backups(doc: &Path) -> Vec<(PathBuf, SystemTime)> {
    let Some(dir) = backup_dir(doc) else {
        return Vec::new();
    };
    let mut v: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name() != "source.txt")
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.modified().ok()?)))
        .collect();
    // Names carry the time of the backup, so they sort chronologically.
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v
}

/// A stamp such as `20261121T141502-0500` as `2026-11-21 14:15`. Stamps are
/// in the writer's local time; the offset is shown only when it differs
/// from this machine's (someone in another time zone).
pub fn readable_stamp(stamp: &str) -> String {
    let d = stamp.as_bytes();
    if !(stamp.is_ascii() && stamp.len() >= 13 && d[8] == b'T') {
        return stamp.to_string();
    }
    let base = format!(
        "{}-{}-{} {}:{}",
        &stamp[0..4],
        &stamp[4..6],
        &stamp[6..8],
        &stamp[9..11],
        &stamp[11..13]
    );
    let theirs = stamp.get(15..).unwrap_or("");
    let mine = crate::review::now();
    match (theirs, mine.get(15..)) {
        ("", _) => base,
        (t, Some(m)) if t == m => base,
        (t, _) => format!(
            "{base} (UTC{}:{})",
            &t[..t.len().min(3)],
            t.get(3..).unwrap_or("00")
        ),
    }
}

/// "3 minutes ago" and the like.
pub fn age(t: SystemTime) -> String {
    match t.elapsed().map(|d| d.as_secs()).unwrap_or(0) {
        s if s < 60 => "just now".into(),
        s if s < 7200 => format!(
            "{} minute{} ago",
            s / 60,
            if s / 60 == 1 { "" } else { "s" }
        ),
        s if s < 172_800 => format!("{} hours ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("xylem-files-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn lock_files_name_their_owner_and_go_away() {
        let dir = scratch("lock");
        let doc = dir.join("a.xml");
        std::fs::write(&doc, "<a/>").unwrap();
        {
            let _lock = Lock::acquire(&doc).expect("folder is writable");
            assert!(lock_path(&doc).unwrap().exists());
            // Our own lock never blocks us.
            assert_eq!(held_by_someone_else(&doc), None);
        }
        assert!(
            !lock_path(&doc).unwrap().exists(),
            "dropping the lock removes the file"
        );

        // Someone else's lock is reported, and not removed by us.
        let other = LockOwner {
            user: "pat".into(),
            host: "OTHER-PC".into(),
            since: "x".into(),
            pid: 1,
        };
        write_hidden(&lock_path(&doc).unwrap(), other.to_text().as_bytes()).unwrap();
        assert_eq!(
            held_by_someone_else(&doc).map(|o| o.user),
            Some("pat".into())
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stamps_change_with_content() {
        let dir = scratch("stamp");
        let doc = dir.join("a.xml");
        std::fs::write(&doc, "<a/>").unwrap();
        let before = stamp(&doc);
        std::fs::write(&doc, "<a>longer</a>").unwrap();
        assert_ne!(before, stamp(&doc));
        let _ = std::fs::remove_dir_all(dir);
    }
}
