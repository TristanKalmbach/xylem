//! Error type shared by the whole crate.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// Result alias used throughout the crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong while loading, saving, matching or importing.
#[derive(Debug)]
pub enum Error {
    /// A filesystem operation failed.
    Io { path: PathBuf, source: io::Error },
    /// `doctype.toml` could not be parsed.
    TomlParse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
    /// A package could not be rendered as TOML.
    TomlWrite(Box<toml::ser::Error>),
    /// An XML file (CTM, XSD, template snippet) was not well formed.
    Xml {
        what: String,
        source: Box<xylem_dom::ParseError>,
    },
    /// A `.xac` archive could not be read.
    Archive {
        path: PathBuf,
        source: Box<zip::result::ZipError>,
    },
    /// A toolbar action string was not recognised.
    BadAction(String),
    /// The package or source is structurally unusable.
    Invalid(String),
}

impl Error {
    pub(crate) fn io(path: impl AsRef<Path>, source: io::Error) -> Self {
        Self::Io {
            path: path.as_ref().to_path_buf(),
            source,
        }
    }

    pub(crate) fn xml(what: impl Into<String>, source: xylem_dom::ParseError) -> Self {
        Self::Xml {
            what: what.into(),
            source: Box::new(source),
        }
    }

    pub(crate) fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::TomlParse { path, source } => write!(f, "{}: {source}", path.display()),
            Self::TomlWrite(source) => write!(f, "cannot write doctype.toml: {source}"),
            Self::Xml { what, source } => write!(f, "{what}: {source}"),
            Self::Archive { path, source } => write!(f, "{}: {source}", path.display()),
            Self::BadAction(s) => write!(f, "unknown toolbar action `{s}`"),
            Self::Invalid(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::TomlParse { source, .. } => Some(source),
            Self::TomlWrite(source) => Some(source),
            Self::Xml { source, .. } => Some(source),
            Self::Archive { source, .. } => Some(source),
            Self::BadAction(_) | Self::Invalid(_) => None,
        }
    }
}
