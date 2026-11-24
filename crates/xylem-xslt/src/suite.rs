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

/// A file name pattern with one `*`, such as `*.xml` or `*-input.xml`.
#[derive(Debug, Clone)]
pub struct NamePattern {
    prefix: String,
    suffix: String,
}

impl NamePattern {
    pub fn parse(pattern: &str) -> Result<NamePattern, String> {
        let mut parts = pattern.splitn(2, '*');
        let prefix = parts.next().unwrap_or_default();
        let Some(suffix) = parts.next() else {
            return Err(format!(
                "The pattern {pattern:?} needs one *, standing for the part file names share."
            ));
        };
        if suffix.contains('*') || pattern.contains(['/', '\\']) {
            return Err(format!(
                "The pattern {pattern:?} must be a file name with exactly one *."
            ));
        }
        Ok(NamePattern {
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
        })
    }

    /// The part `*` stands for, when `name` matches.
    fn matches<'a>(&self, name: &'a str) -> Option<&'a str> {
        let (p, x) = (self.prefix.len(), self.suffix.len());
        if name.len() < p + x || !name.is_char_boundary(p) || !name.is_char_boundary(name.len() - x)
        {
            return None;
        }
        // File names are case-insensitive on Windows.
        let same = |a: &str, b: &str| {
            if cfg!(windows) {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        (same(&name[..p], &self.prefix) && same(&name[name.len() - x..], &self.suffix))
            .then(|| &name[p..name.len() - x])
    }

    fn with(&self, stem: &str) -> String {
        format!("{}{stem}{}", self.prefix, self.suffix)
    }
}

/// Test inputs found under a folder: every file matching `input`, in that
/// folder or below, with the file matching `expected` next to it, if there
/// is one (`*` standing for the same text in both).
pub fn find_tests(
    folder: &Path,
    input: &NamePattern,
    expected: &NamePattern,
) -> Result<Vec<(PathBuf, Option<PathBuf>)>, String> {
    let mut found = Vec::new();
    let mut dirs = vec![folder.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("Could not read {}: {e}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = input.matches(&name) {
                let other = dir.join(expected.with(stem));
                // The same file cannot be both (`*.xml` and `*.xml`).
                let expected = (other != path && other.is_file()).then_some(other);
                found.push((path, expected));
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The text of a suite file at `path` for these tests, found under
/// `folder` (test names are their paths there); paths inside the suite's
/// folder are written relative to it.
pub fn write_suite(
    path: &Path,
    folder: &Path,
    stylesheet: Option<&Path>,
    params: &BTreeMap<String, String>,
    tests: &[(PathBuf, Option<PathBuf>)],
) -> String {
    let dir = path.parent().unwrap_or(Path::new(""));
    let show = |p: &Path| {
        let p = p.strip_prefix(dir).unwrap_or(p);
        toml::Value::String(p.to_string_lossy().replace('\\', "/")).to_string()
    };
    let mut out = String::from(
        "# Made by Xylem from a folder of saved inputs. Paths are relative to this file.\n",
    );
    if let Some(s) = stylesheet {
        out.push_str(&format!("stylesheet = {}\n", show(s)));
    }
    if !params.is_empty() {
        out.push_str("\n[params]\n");
        for (k, v) in params {
            out.push_str(&format!(
                "{} = {}\n",
                toml_key(k),
                toml::Value::String(v.clone())
            ));
        }
    }
    for (input, expected) in tests {
        let name = input
            .strip_prefix(folder)
            .unwrap_or(input)
            .with_extension("");
        out.push_str(&format!(
            "\n[[test]]\nname = {}\ninput = {}\n",
            toml::Value::String(name.to_string_lossy().replace('\\', "/")),
            show(input)
        ));
        if let Some(e) = expected {
            out.push_str(&format!("expected = {}\n", show(e)));
        }
    }
    out
}

fn toml_key(k: &str) -> String {
    if !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        k.to_string()
    } else {
        toml::Value::String(k.to_string()).to_string()
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
    fn a_suite_is_made_from_a_folder_of_exports() {
        let dir = std::env::temp_dir().join("xylem-xslt-new-suite");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("run 2")).unwrap();
        for f in [
            "a-in.xml",
            "a.fo",
            "b-in.xml",
            "notes.txt",
            "run 2/c-in.xml",
            "run 2/c.fo",
        ] {
            std::fs::write(dir.join(f), "<x/>").unwrap();
        }
        let tests = find_tests(
            &dir,
            &NamePattern::parse("*-in.xml").unwrap(),
            &NamePattern::parse("*.fo").unwrap(),
        )
        .unwrap();
        assert_eq!(tests.len(), 3);
        assert_eq!(tests[0].1.as_deref(), Some(dir.join("a.fo").as_path()));
        assert!(tests[1].1.is_none());
        let path = dir.join("suite.toml");
        let params = BTreeMap::from([("mode".to_string(), "final".to_string())]);
        let text = write_suite(&path, &dir, Some(&dir.join("x.xsl")), &params, &tests);
        let s = Suite::parse(&text, &path).unwrap();
        assert_eq!(s.tests.len(), 3);
        assert_eq!(s.tests[2].name, "run 2/c-in");
        assert_eq!(s.tests[2].input, dir.join("run 2/c-in.xml"));
        assert_eq!(s.tests[0].params["mode"], "final");
        assert_eq!(s.stylesheet.unwrap(), dir.join("x.xsl"));
        assert!(NamePattern::parse("*.xml*").is_err());
        assert!(NamePattern::parse("a.xml").is_err());
    }

    #[test]
    fn unknown_keys_and_empty_suites_are_errors() {
        assert!(Suite::parse("stylesheets = 1\n[[test]]\ninput='a'", Path::new("s.toml")).is_err());
        assert!(Suite::parse("stylesheet = 'a.xsl'", Path::new("s.toml")).is_err());
    }
}
