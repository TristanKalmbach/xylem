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

pub use compile::{OutputSpec, Source, Stylesheet};
pub use exec::TemplateTime;

/// One timed template or function.
#[derive(Debug, Clone)]
pub struct Timed {
    /// `template` or `function`.
    pub kind: &'static str,
    pub source: Source,
    pub time: TemplateTime,
}

/// The result of one transformation.
pub struct Output {
    /// The principal result, written the way the reference host writes it.
    pub text: String,
    /// The principal result written as the stylesheet's `xsl:output` asks,
    /// the way the host's `--serialize` step writes it.
    pub serialized: String,
    /// `xsl:message` output, in order.
    pub messages: Vec<String>,
    /// Every template and function that ran, when timing was asked for.
    pub timing: Vec<Timed>,
    /// Wall time of the transformation (timing runs only), in milliseconds.
    pub total_ms: f64,
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
    let started = std::time::Instant::now();
    let compat = ss.version.starts_with('1');
    let result = value::with_compat(compat, || engine.run())?;
    let total_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut timing = Vec::new();
    if let Some(p) = &engine.timing {
        let times = &p.borrow().times;
        for t in &ss.templates {
            timing.push(Timed {
                kind: "template",
                source: t.source.clone(),
                time: times[t.id].clone(),
            });
        }
        for f in ss.functions.values() {
            let time = times[ss.templates.len() + f.id].clone();
            timing.push(Timed {
                kind: "function",
                source: f.source.clone(),
                time,
            });
        }
        timing.retain(|t| t.time.calls > 0);
    }
    Ok(Output {
        text: value::with_compat(compat, || serialize::host_style(&result)),
        serialized: value::with_compat(compat, || serialize::with_output(&result, &ss.output)),
        messages: engine.messages.borrow().clone(),
        timing,
        total_ms: if time { total_ms } else { 0.0 },
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
