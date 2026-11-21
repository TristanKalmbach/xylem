use std::fmt;

use crate::issue::{Issue, SchemaKind};

/// Errors produced by the S1000D helpers.
#[derive(Debug)]
pub enum S1000dError {
    /// A data module or publication module code string could not be parsed.
    InvalidCode(String),
    /// The document lacks an element needed for the requested operation.
    MissingElement(String),
    /// Reading a file failed.
    Io(std::io::Error),
    /// The file is not well-formed XML.
    Xml(xylem_dom::ParseError),
    /// No template exists for this combination.
    Unsupported { issue: Issue, kind: SchemaKind },
}

impl fmt::Display for S1000dError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            S1000dError::InvalidCode(s) => write!(f, "invalid S1000D code: {s}"),
            S1000dError::MissingElement(s) => write!(f, "the document has no <{s}> element"),
            S1000dError::Io(e) => write!(f, "I/O error: {e}"),
            S1000dError::Xml(e) => write!(f, "XML error: {e}"),
            S1000dError::Unsupported { issue, kind } => {
                write!(
                    f,
                    "no {} template for S1000D issue {issue}",
                    kind.display_name()
                )
            }
        }
    }
}

impl std::error::Error for S1000dError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            S1000dError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for S1000dError {
    fn from(e: std::io::Error) -> Self {
        S1000dError::Io(e)
    }
}

impl From<xylem_dom::ParseError> for S1000dError {
    fn from(e: xylem_dom::ParseError) -> Self {
        S1000dError::Xml(e)
    }
}
