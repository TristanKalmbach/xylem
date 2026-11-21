//! Where an XMAX customization comes from: a folder or a `.xac` archive.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::error::{Error, Result};
use crate::xdp::{self, Xdp};

/// Largest archive entry that will be read into memory.
const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
/// How many parent folders are searched for `@import`ed files.
const MAX_ANCESTORS: usize = 6;

/// A file read from a source.
pub struct Loaded {
    /// File name without directories.
    pub name: String,
    pub bytes: Vec<u8>,
    /// Folder the file came from, used to resolve its own relative references.
    pub origin: Option<PathBuf>,
}

pub enum Source {
    Folder(Folder),
    Archive(Archive),
}

pub struct Folder {
    dir: PathBuf,
    names: Vec<String>,
    /// Files named by the project file, by lower-cased base name.
    project_files: HashMap<String, PathBuf>,
    /// Name and contents of the `.xdp` project file, if the folder has one.
    pub xdp: Option<(String, Xdp)>,
}

pub struct Archive {
    path: PathBuf,
    zip: ZipArchive<File>,
    /// Sanitised entry names (forward slashes), sorted, with their zip index.
    entries: Vec<(String, usize)>,
    names: Vec<String>,
    /// Entries skipped because their names would escape the archive.
    pub unsafe_entries: usize,
}

impl Source {
    /// Opens a folder or a `.xac` (zip) file.
    pub fn open(path: &Path) -> Result<Self> {
        let meta = fs::metadata(path).map_err(|e| Error::io(path, e))?;
        if meta.is_dir() {
            Folder::open(path).map(Self::Folder)
        } else {
            Archive::open(path).map(Self::Archive)
        }
    }

    /// File names directly available (folder: its files; archive: all entries).
    pub fn names(&self) -> &[String] {
        match self {
            Self::Folder(f) => &f.names,
            Self::Archive(a) => &a.names,
        }
    }

    /// Reads the entry whose name equals `name`, ignoring case.
    pub fn read(&mut self, name: &str) -> Result<Option<Loaded>> {
        match self {
            Self::Folder(f) => f.read(name),
            Self::Archive(a) => a.read(name),
        }
    }

    /// Resolves a reference found in a stylesheet living in `from`.
    pub fn locate(&mut self, reference: &str, from: Option<&Path>) -> Result<Option<Loaded>> {
        match self {
            Self::Folder(f) => f.locate(reference, from),
            Self::Archive(a) => a.locate(reference),
        }
    }
}

fn base_name(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// Drops query/fragment and percent-encoded spaces from a CSS reference.
fn clean_reference(reference: &str) -> String {
    reference
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .replace("%20", " ")
        .replace('\\', "/")
}

impl Folder {
    fn open(dir: &Path) -> Result<Self> {
        let dir = std::path::absolute(dir).map_err(|e| Error::io(dir, e))?;
        let mut names: Vec<String> = fs::read_dir(&dir)
            .map_err(|e| Error::io(&dir, e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();

        let mut project_files = HashMap::new();
        let mut xdp = None;
        if let Some(name) = names
            .iter()
            .find(|n| n.to_ascii_lowercase().ends_with(".xdp"))
        {
            let path = dir.join(name);
            let parsed = xdp::parse(&fs::read(&path).map_err(|e| Error::io(&path, e))?);
            for rel in &parsed.files {
                project_files
                    .entry(base_name(rel).to_ascii_lowercase())
                    .or_insert_with(|| dir.join(rel));
            }
            xdp = Some((name.clone(), parsed));
        }
        Ok(Self {
            dir,
            names,
            project_files,
            xdp,
        })
    }

    fn load(&self, path: &Path) -> Result<Loaded> {
        let bytes = fs::read(path).map_err(|e| Error::io(path, e))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Loaded {
            name,
            bytes,
            origin: path.parent().map(Path::to_path_buf),
        })
    }

    fn read(&self, name: &str) -> Result<Option<Loaded>> {
        match self.names.iter().find(|n| n.eq_ignore_ascii_case(name)) {
            Some(n) => self.load(&self.dir.join(n)).map(Some),
            None => Ok(None),
        }
    }

    fn locate(&self, reference: &str, from: Option<&Path>) -> Result<Option<Loaded>> {
        let rel = clean_reference(reference);
        if rel.is_empty() {
            return Ok(None);
        }
        let first = from.unwrap_or(&self.dir);
        let mut candidates = vec![first.join(&rel)];
        candidates.extend(
            self.dir
                .ancestors()
                .take(MAX_ANCESTORS)
                .map(|a| a.join(&rel)),
        );
        if let Some(p) = self
            .project_files
            .get(&base_name(&rel).to_ascii_lowercase())
        {
            candidates.push(p.clone());
        }
        match candidates.into_iter().find(|c| c.is_file()) {
            Some(path) => self.load(&path).map(Some),
            None => Ok(None),
        }
    }
}

impl Archive {
    fn open(path: &Path) -> Result<Self> {
        let archive_err = |source| Error::Archive {
            path: path.to_path_buf(),
            source: Box::new(source),
        };
        let file = File::open(path).map_err(|e| Error::io(path, e))?;
        let mut zip = ZipArchive::new(file).map_err(archive_err)?;
        let mut entries = Vec::new();
        let mut unsafe_entries = 0;
        for index in 0..zip.len() {
            let entry = zip.by_index_raw(index).map_err(archive_err)?;
            if entry.is_dir() {
                continue;
            }
            match entry.enclosed_name() {
                Some(p) => entries.push((p.to_string_lossy().replace('\\', "/"), index)),
                None => unsafe_entries += 1,
            }
        }
        entries.sort();
        let names = entries.iter().map(|(n, _)| n.clone()).collect();
        Ok(Self {
            path: path.to_path_buf(),
            zip,
            entries,
            names,
            unsafe_entries,
        })
    }

    fn read(&mut self, name: &str) -> Result<Option<Loaded>> {
        let found = self
            .entries
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .cloned();
        found.map(|(n, i)| self.read_entry(&n, i)).transpose()
    }

    fn read_entry(&mut self, name: &str, index: usize) -> Result<Loaded> {
        let entry = self.zip.by_index(index).map_err(|source| Error::Archive {
            path: self.path.clone(),
            source: Box::new(source),
        })?;
        if entry.size() > MAX_ENTRY_BYTES {
            return Err(Error::invalid(format!(
                "archive entry `{name}` is larger than {MAX_ENTRY_BYTES} bytes"
            )));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .take(MAX_ENTRY_BYTES)
            .read_to_end(&mut bytes)
            .map_err(|e| Error::io(&self.path, e))?;
        Ok(Loaded {
            name: base_name(name).to_string(),
            bytes,
            origin: None,
        })
    }

    /// Archives are flat in practice, so references resolve by base name.
    fn locate(&mut self, reference: &str) -> Result<Option<Loaded>> {
        let wanted = base_name(&clean_reference(reference)).to_string();
        let found = self
            .entries
            .iter()
            .find(|(n, _)| base_name(n).eq_ignore_ascii_case(&wanted))
            .cloned();
        found.map(|(n, i)| self.read_entry(&n, i)).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_references() {
        assert_eq!(
            clean_reference("..\\img\\a%20b.png?v=2#x"),
            "../img/a b.png"
        );
        assert_eq!(base_name("a/b\\c.css"), "c.css");
    }
}
