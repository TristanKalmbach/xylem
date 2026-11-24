//! Time spent per template and function, from a host run with `--profile`.
//!
//! The host times every template and function while the stylesheet runs and
//! writes `<output>.profile.json`. [`Profile::load`] reads one; [`combine`]
//! adds up several (one per test input) so a suite gives one ranking.
//!
//! Own time ("self") excludes the templates and functions a template called;
//! total time includes them and counts a recursive template once. Timing
//! hooks slow the run down, so the numbers are for comparing templates with
//! each other rather than for predicting publishing time.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Timings of one template or function.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TemplateTime {
    /// `template` or `function`.
    pub kind: String,
    /// Name of a named template or function.
    pub name: Option<String>,
    /// Match pattern of a template rule.
    #[serde(rename = "match")]
    pub pattern: Option<String>,
    pub mode: Option<String>,
    /// Stylesheet file (a URI as Saxon reports it).
    pub file: String,
    pub line: u32,
    pub calls: u64,
    pub self_ms: f64,
    pub total_ms: f64,
}

impl TemplateTime {
    /// How the template is shown: its name, or its match pattern.
    pub fn label(&self) -> String {
        let base = match (&self.name, &self.pattern) {
            (Some(n), _) if self.kind == "function" => format!("{n}()"),
            (Some(n), _) => n.clone(),
            (None, Some(p)) => format!("match=\"{p}\""),
            (None, None) => "(unnamed)".into(),
        };
        match &self.mode {
            Some(m) if !m.is_empty() && m != "#default" => format!("{base}  mode={m}"),
            _ => base,
        }
    }

    /// The stylesheet as a file path.
    pub fn path(&self) -> PathBuf {
        file_uri_to_path(&self.file)
    }
}

/// One profile: a run of a stylesheet on one input (or several, combined).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Profile {
    /// Wall time of the run with timing on.
    pub total_ms: f64,
    /// Sorted by own time, largest first.
    pub templates: Vec<TemplateTime>,
}

impl Profile {
    pub fn load(path: &Path) -> Result<Profile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut p: Profile =
            serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        sort(&mut p.templates);
        Ok(p)
    }

    /// Own time of all templates and functions together.
    pub fn timed_ms(&self) -> f64 {
        self.templates.iter().map(|t| t.self_ms).sum()
    }
}

fn sort(t: &mut [TemplateTime]) {
    t.sort_by(|a, b| b.self_ms.total_cmp(&a.self_ms));
}

/// Adds profiles together, template by template.
pub fn combine(profiles: &[Profile]) -> Profile {
    let mut index: HashMap<(String, u32, String, String), usize> = HashMap::new();
    let mut out = Profile::default();
    for p in profiles {
        out.total_ms += p.total_ms;
        for t in &p.templates {
            let key = (
                t.file.clone(),
                t.line,
                t.name.clone().unwrap_or_default(),
                t.pattern.clone().unwrap_or_default(),
            );
            match index.get(&key) {
                Some(&i) => {
                    let o = &mut out.templates[i];
                    o.calls += t.calls;
                    o.self_ms += t.self_ms;
                    o.total_ms += t.total_ms;
                }
                None => {
                    index.insert(key, out.templates.len());
                    out.templates.push(t.clone());
                }
            }
        }
    }
    sort(&mut out.templates);
    out
}

/// `file:/C:/a%20b/x.xsl` → `C:/a b/x.xsl`; anything else is returned as is.
pub fn file_uri_to_path(uri: &str) -> PathBuf {
    let Some(rest) = uri.strip_prefix("file:") else {
        return PathBuf::from(uri);
    };
    let rest = rest.trim_start_matches('/');
    // Keep a leading slash on Unix paths, drop it before a drive letter.
    let rest = if rest.len() > 1 && rest.as_bytes()[1] == b':' {
        rest.to_string()
    } else if cfg!(windows) && !uri.starts_with("file:///") && uri.starts_with("file://") {
        format!("//{rest}") // file://server/share
    } else {
        format!("/{rest}")
    };
    let mut bytes = Vec::with_capacity(rest.len());
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            bytes.push(v);
            i += 3;
            continue;
        }
        bytes.push(b[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(
        name: Option<&str>,
        pattern: Option<&str>,
        line: u32,
        calls: u64,
        self_ms: f64,
    ) -> TemplateTime {
        TemplateTime {
            kind: "template".into(),
            name: name.map(Into::into),
            pattern: pattern.map(Into::into),
            mode: None,
            file: "file:/C:/work/style%20sheets/common.xsl".into(),
            line,
            calls,
            self_ms,
            total_ms: self_ms,
        }
    }

    #[test]
    fn profiles_parse_and_combine() {
        let json = r#"{"total_ms":10.5,"templates":[
            {"kind":"function","name":"fn:lookupLabel","file":"file:/C:/x.xsl","line":114,"calls":85,"self_ms":4.0,"total_ms":4.0},
            {"kind":"template","match":"para","mode":"toc","file":"file:/C:/x.xsl","line":9,"calls":3,"self_ms":6.5,"total_ms":7.0}]}"#;
        let mut p: Profile = serde_json::from_str(json).unwrap();
        sort(&mut p.templates);
        assert_eq!(p.templates[0].label(), "match=\"para\"  mode=toc");
        assert_eq!(p.templates[1].label(), "fn:lookupLabel()");
        let a = Profile {
            total_ms: 1.0,
            templates: vec![t(Some("n"), None, 5, 2, 1.0), t(None, Some("p"), 9, 1, 3.0)],
        };
        let b = Profile {
            total_ms: 2.0,
            templates: vec![t(Some("n"), None, 5, 3, 4.0)],
        };
        let c = combine(&[a, b]);
        assert_eq!(c.total_ms, 3.0);
        assert_eq!(c.templates[0].label(), "n");
        assert_eq!((c.templates[0].calls, c.templates[0].self_ms), (5, 5.0));
        assert_eq!(c.templates[1].calls, 1);
    }

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_uri_to_path("file:/C:/work/style%20sheets/a.xsl"),
            PathBuf::from("C:/work/style sheets/a.xsl")
        );
        assert_eq!(
            file_uri_to_path("file:///C:/a.xsl"),
            PathBuf::from("C:/a.xsl")
        );
        assert_eq!(file_uri_to_path("C:\\a.xsl"), PathBuf::from("C:\\a.xsl"));
    }
}
