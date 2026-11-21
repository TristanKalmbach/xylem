//! Evaluation context: variable bindings, namespace prefixes and custom functions.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use xylem_dom::Document;

use crate::error::XPathError;
use crate::value::{Value, XNode};

/// The dynamic position handed to a custom function.
pub struct FunctionContext<'a> {
    pub doc: &'a Document,
    /// The context node of the call.
    pub node: XNode,
    /// The context position (1-based).
    pub position: usize,
    /// The context size.
    pub size: usize,
}

/// Signature of a user-supplied function.
pub type CustomFunction =
    dyn Fn(&FunctionContext<'_>, &[Value]) -> Result<Value, XPathError> + Send + Sync;

/// Variables, namespace prefixes and custom functions available to an expression.
#[derive(Clone, Default)]
pub struct Context {
    pub(crate) variables: HashMap<String, Value>,
    pub(crate) namespaces: HashMap<String, String>,
    pub(crate) functions: HashMap<String, Arc<CustomFunction>>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `$name` to a value.
    pub fn set_variable(&mut self, name: impl Into<String>, value: impl Into<Value>) -> &mut Self {
        self.variables.insert(name.into(), value.into());
        self
    }

    /// Maps a prefix used in name tests to a namespace URI.
    ///
    /// Without a mapping, `prefix:name` is compared literally against the
    /// qualified name as written in the document.
    pub fn set_namespace(
        &mut self,
        prefix: impl Into<String>,
        uri: impl Into<String>,
    ) -> &mut Self {
        self.namespaces.insert(prefix.into(), uri.into());
        self
    }

    /// Registers a function callable as `name(...)`. Custom functions take
    /// precedence over the core library.
    pub fn register_function<F>(&mut self, name: impl Into<String>, f: F) -> &mut Self
    where
        F: Fn(&FunctionContext<'_>, &[Value]) -> Result<Value, XPathError> + Send + Sync + 'static,
    {
        self.functions.insert(name.into(), Arc::new(f));
        self
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut fns: Vec<&String> = self.functions.keys().collect();
        fns.sort();
        f.debug_struct("Context")
            .field("variables", &self.variables)
            .field("namespaces", &self.namespaces)
            .field("functions", &fns)
            .finish()
    }
}
