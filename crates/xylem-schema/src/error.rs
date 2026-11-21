//! Error type for grammar loading.

use std::fmt;
use std::path::PathBuf;

/// An error raised while loading or compiling a grammar.
#[derive(Debug)]
pub enum SchemaError {
    /// A schema, DTD, entity or catalog file could not be read.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A schema or catalog file is not well-formed.
    Parse { path: PathBuf, message: String },
    /// A referenced resource could not be located.
    Unresolved(String),
    /// The schema is structurally unusable (for example, it has no `xs:schema` root).
    Invalid(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cannot read '{}': {source}", path.display()),
            Self::Parse { path, message } => {
                write!(f, "cannot parse '{}': {message}", path.display())
            }
            Self::Unresolved(what) => write!(f, "cannot resolve {what}"),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, SchemaError>;

/// Reads a file, mapping failures to [`SchemaError::Io`].
pub(crate) fn read_file(path: &std::path::Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|source| SchemaError::Io {
        path: path.to_path_buf(),
        source,
    })
}
