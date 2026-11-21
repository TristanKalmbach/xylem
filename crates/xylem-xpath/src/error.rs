use std::fmt;

/// An error raised while compiling or evaluating an XPath expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPathError {
    message: String,
    position: Option<usize>,
}

impl XPathError {
    pub(crate) fn syntax(position: usize, message: impl Into<String>) -> Self {
        Self { message: message.into(), position: Some(position) }
    }

    pub(crate) fn eval(message: impl Into<String>) -> Self {
        Self { message: message.into(), position: None }
    }

    /// Creates an evaluation error; useful for custom functions.
    pub fn custom(message: impl Into<String>) -> Self {
        Self::eval(message)
    }

    /// Human readable description of the problem.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Character offset into the expression for syntax errors.
    pub fn position(&self) -> Option<usize> {
        self.position
    }
}

impl fmt::Display for XPathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.position {
            Some(p) => write!(f, "XPath syntax error at offset {p}: {}", self.message),
            None => write!(f, "XPath error: {}", self.message),
        }
    }
}

impl std::error::Error for XPathError {}
