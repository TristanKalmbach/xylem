//! Test suites: which inputs to run a stylesheet on, with which parameters,
//! and what each should produce.
//!
//! ```toml
//! # xylem-tests.toml
//! stylesheet = "../stylesheets/book-to-fo.xsl"
//!
//! [params]            # passed to every test
//! mode = "draft"
//! draft = "yes"
//!
//! [[test]]
//! name = "Large manual"
//! input = "large-manual.xml"
//! expected = "large-manual.fo"    # output the publishing service saved
//! params = { mode = "final" }     # overrides for this test
//! ```
//!
//! Paths are relative to the suite file.

use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A set of test inputs for one stylesheet.
#[derive(Debug, Clone)]
pub struct Suite {
    /// The suite file.
    pub path: PathBuf,
    /// Stylesheet the tests run with, unless the caller names another.
    pub stylesheet: Option<PathBuf>,
    pub tests: Vec<Test>,
}

/// One test input.
#[derive(Debug, Clone)]
pub struct Test {
    pub name: String,
    pub input: PathBuf,
    /// Output saved by the publishing service for this input, if any.
    pub expected: Option<PathBuf>,
    /// Stylesheet parameters (the suite's, with this test's overrides).
    pub params: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuite {
    stylesheet: Option<PathBuf>,
    #[serde(default)]
    params: BTreeMap<String, toml::Value>,
    #[serde(default, rename = "test")]
    tests: Vec<RawTest>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTest {
    name: Option<String>,
    input: PathBuf,
    expected: Option<PathBuf>,
    #[serde(default)]
    params: BTreeMap<String, toml::Value>,
}

/// A parameter value as the stylesheet receives it: strings as they are,
/// numbers and booleans written out.
fn param_text(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

impl Suite {
    /// Reads a suite file.
    pub fn load(path: &Path) -> Result<Suite, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
        Suite::parse(&text, path)
    }

    /// Parses suite text; relative paths resolve against `path`'s folder.
    pub fn parse(text: &str, path: &Path) -> Result<Suite, String> {
        let raw: RawSuite = toml::from_str(text).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().unwrap_or(Path::new("."));
        let resolve = |p: &Path| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                dir.join(p)
            }
        };
        let shared: BTreeMap<String, String> = raw
            .params
            .iter()
            .map(|(k, v)| (k.clone(), param_text(v)))
            .collect();
        let mut tests = Vec::new();
        for (i, t) in raw.tests.into_iter().enumerate() {
            let mut params = shared.clone();
            for (k, v) in &t.params {
                params.insert(k.clone(), param_text(v));
            }
            let name = t.name.unwrap_or_else(|| {
                t.input
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("Test {}", i + 1))
            });
            tests.push(Test {
                name,
                input: resolve(&t.input),
                expected: t.expected.as_deref().map(resolve),
                params,
            });
        }
        if tests.is_empty() {
            return Err(format!("{} lists no [[test]] entries.", path.display()));
        }
        Ok(Suite {
            path: path.to_path_buf(),
            stylesheet: raw.stylesheet.as_deref().map(resolve),
            tests,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameters_merge_and_paths_resolve() {
        let text = r#"
stylesheet = "../xsl/top.xsl"
[params]
mode = "draft"
draft = 0
[[test]]
name = "Big"
input = "a.xml"
expected = "a.fo"
params = { mode = "final" }
[[test]]
input = "sub/b.xml"
"#;
        let s = Suite::parse(text, Path::new("/tests/suite.toml")).unwrap();
        assert_eq!(s.stylesheet.unwrap(), Path::new("/tests/../xsl/top.xsl"));
        assert_eq!(s.tests[0].name, "Big");
        assert_eq!(s.tests[0].params["mode"], "final");
        assert_eq!(s.tests[0].params["draft"], "0");
        assert_eq!(
            s.tests[0].expected.as_deref(),
            Some(Path::new("/tests/a.fo"))
        );
        assert_eq!(s.tests[1].name, "b.xml");
        assert_eq!(s.tests[1].params["mode"], "draft");
        assert!(s.tests[1].expected.is_none());
    }

    #[test]
    fn unknown_keys_and_empty_suites_are_errors() {
        assert!(Suite::parse("stylesheets = 1\n[[test]]\ninput='a'", Path::new("s.toml")).is_err());
        assert!(Suite::parse("stylesheet = 'a.xsl'", Path::new("s.toml")).is_err());
    }
}
