//! A prototype pure-Rust XSLT 2.0 / XPath 2.0 processor.
//!
//! Written from the W3C specifications (XSLT 2.0, XPath 2.0, XPath
//! Functions and Operators, XSLT and XQuery Serialization). Output is
//! checked against a reference processor used only as a black box; see
//! `docs/design/xslt-engine.md` for scope, design and what is missing.
//!
//! ```no_run
//! use std::collections::HashMap;
//! use std::path::Path;
//! let ss = xylem_xslt_engine::Stylesheet::compile(Path::new("book.xsl")).unwrap();
//! let out = xylem_xslt_engine::transform_file(&ss, Path::new("book.xml"), &HashMap::new()).unwrap();
//! println!("{}", out.text);
//! ```

mod compile;
mod eval;
mod exec;
mod funcs;
mod number;
mod serialize;
mod tree;
mod value;
pub mod xpath;

use std::collections::HashMap;
use std::path::Path;

pub use compile::Stylesheet;
pub use exec::TemplateTime;

/// The result of one transformation.
pub struct Output {
    /// The principal result, written the way the reference host writes it.
    pub text: String,
    /// `xsl:message` output, in order.
    pub messages: Vec<String>,
    /// Per template (indexed like the stylesheet's rules) when timing was
    /// asked for, with each template's location.
    pub timing: Vec<(String, TemplateTime)>,
}

/// Transforms an input file. Parameters set the global `xsl:param` of
/// exactly that name; values are converted to a declared boolean or
/// numeric type and are strings otherwise, as the publishing host does.
pub fn transform_file(
    ss: &Stylesheet,
    input: &Path,
    params: &HashMap<String, String>,
) -> Result<Output, String> {
    let bytes = std::fs::read(input).map_err(|e| format!("{}: {e}", input.display()))?;
    transform_bytes(ss, &bytes, params, false)
}

/// Transforms an input document given as bytes; with `time`, also times
/// every template.
pub fn transform_bytes(
    ss: &Stylesheet,
    input: &[u8],
    params: &HashMap<String, String>,
    time: bool,
) -> Result<Output, String> {
    let doc = xylem_dom::Document::from_bytes(input).map_err(|e| format!("input: {e}"))?;
    // The host gives the input the stylesheet's folder as its base URI.
    let source = tree::from_dom(&doc, &ss.base.to_string_lossy(), ss)?;
    let engine = exec::Engine::new(ss, source, params.clone(), time);
    let result = engine.run()?;
    let timing = match &engine.timing {
        Some(t) => ss
            .templates
            .iter()
            .map(|t| t.location.clone())
            .zip(t.borrow().iter().cloned())
            .collect(),
        None => Vec::new(),
    };
    Ok(Output {
        text: serialize::host_style(&result),
        messages: engine.messages.borrow().clone(),
        timing,
    })
}

impl Output {
    /// The result as the host writes it to a file: UTF-8 with a byte order
    /// mark.
    pub fn host_bytes(&self) -> Vec<u8> {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(self.text.as_bytes());
        b
    }
}
