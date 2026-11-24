//! The XSLT test suite, XSLT 1.0 and 2.0 tests only.

use crate::Verdict;
use crate::catalog::Xml;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xylem_dom::NodeId;
use xylem_xslt_engine::api::{ResultItem, canonical_xml, with_session};
use xylem_xslt_engine::{Start, Stylesheet, transform_from};

struct Env {
    source: Option<Vec<u8>>,
    stylesheet: Option<PathBuf>,
}

fn environment(x: &Xml, e: NodeId, base: &Path) -> Result<Env, String> {
    let mut env = Env {
        source: None,
        stylesheet: None,
    };
    for c in x.all_kids(e) {
        match x.local(c) {
            "source" => {
                if x.attr(c, "role") != Some(".") {
                    return Err("environment: source other than the context item".into());
                }
                if x.attr(c, "select").is_some() || x.attr(c, "validation").is_some() {
                    return Err("environment: selected or validated source".into());
                }
                env.source = Some(match (x.attr(c, "file"), x.kid(c, "content")) {
                    (Some(f), _) => std::fs::read(base.join(f)).map_err(|e| format!("{f}: {e}"))?,
                    (None, Some(content)) => x.text(content).trim_start().as_bytes().to_vec(),
                    _ => return Err("environment: empty source".into()),
                });
            }
            "stylesheet" => {
                if let Some(f) = x.attr(c, "file") {
                    env.stylesheet = Some(base.join(f));
                }
            }
            "description" | "created" | "modified" => {}
            other => return Err(format!("environment: {other}")),
        }
    }
    Ok(env)
}

fn dependencies(x: &Xml, set: NodeId, case: NodeId) -> Result<(), String> {
    let deps = |n: NodeId| {
        x.kid(n, "dependencies")
            .map(|d| x.all_kids(d))
            .unwrap_or_default()
    };
    let set_deps = deps(set);
    let case_deps = deps(case);
    let spec_of = |ds: &[NodeId]| {
        ds.iter()
            .filter(|&&d| x.local(d) == "spec")
            .map(|&d| x.attr(d, "value").unwrap_or("").to_string())
            .next()
    };
    let spec = spec_of(&case_deps)
        .or_else(|| spec_of(&set_deps))
        .unwrap_or_default();
    let ok = spec
        .split_whitespace()
        .any(|t| matches!(t, "XSLT10+" | "XSLT20+" | "XSLT20" | "XSLT10"));
    if !ok {
        return Err("not an XSLT 1.0 or 2.0 test".into());
    }
    for &d in set_deps.iter().chain(&case_deps) {
        let what = x.local(d);
        let value = x.attr(d, "value").unwrap_or("");
        // Any Unicode version and 1.0 compatibility mode are available.
        if what == "spec"
            || what == "unicode-version"
            || (what == "feature" && value == "backwards_compatibility")
            || x.attr(d, "satisfied") == Some("false")
        {
            continue;
        }
        return Err(format!("needs {what} {value}"));
    }
    Ok(())
}

pub fn run_set(_root: &Path, file: &Path) -> Vec<(String, Verdict)> {
    let Ok(x) = Xml::load(file) else {
        return vec![(
            "(set)".into(),
            Verdict::Fail("could not read the test set".into()),
        )];
    };
    let base = file.parent().unwrap_or(Path::new(".")).to_path_buf();
    let set = x.root();
    let mut compiled: HashMap<PathBuf, Result<Stylesheet, String>> = HashMap::new();
    let mut out = Vec::new();
    for case in x.kids(set, "test-case") {
        let name = x.attr(case, "name").unwrap_or("?").to_string();
        let v = run_case(&x, set, case, &base, &mut compiled);
        out.push((name, v));
    }
    out
}

fn qname(x: &Xml, n: NodeId, lexical: &str) -> (String, String) {
    match lexical.split_once(':') {
        Some((p, l)) => (x.resolve(n, p).unwrap_or_default(), l.to_string()),
        None => (String::new(), lexical.to_string()),
    }
}

fn run_case(
    x: &Xml,
    set: NodeId,
    case: NodeId,
    base: &Path,
    compiled: &mut HashMap<PathBuf, Result<Stylesheet, String>>,
) -> Verdict {
    if let Err(why) = dependencies(x, set, case) {
        return Verdict::Skip(why);
    }
    let mut env = Env {
        source: None,
        stylesheet: None,
    };
    for e in x.kids(case, "environment") {
        let found = match x.attr(e, "ref") {
            Some(r) => x
                .kids(set, "environment")
                .into_iter()
                .find(|&n| x.attr(n, "name") == Some(r))
                .map(|n| environment(x, n, base))
                .unwrap_or_else(|| Err(format!("environment {r} not found"))),
            None => environment(x, e, base),
        };
        match found {
            Ok(f) => {
                env.source = f.source.or(env.source);
                env.stylesheet = f.stylesheet.or(env.stylesheet);
            }
            Err(why) => return Verdict::Skip(why),
        }
    }
    let Some(test) = x.kid(case, "test") else {
        return Verdict::Skip("no test".into());
    };
    let mut start = Start::Rules;
    let mut params: HashMap<String, String> = HashMap::new();
    let mut stylesheet = env.stylesheet.clone();
    for c in x.all_kids(test) {
        match x.local(c) {
            "stylesheet" => {
                if x.attr(c, "role").is_some_and(|r| r != "principal") {
                    continue;
                }
                match x.attr(c, "file") {
                    Some(f) => stylesheet = Some(base.join(f)),
                    None => return Verdict::Skip("inline stylesheet".into()),
                }
            }
            "initial-template" => {
                let (u, l) = qname(x, c, x.attr(c, "name").unwrap_or(""));
                start = Start::Template(u, l);
            }
            "initial-mode" if x.attr(c, "select").is_none() => {
                if let Some(n) = x.attr(c, "name") {
                    let (u, l) = qname(x, c, n);
                    start = Start::Mode(u, l);
                }
            }
            // Parameters given as literals; anything computed is skipped.
            "param" => {
                let (Some(n), Some(sel)) = (x.attr(c, "name"), x.attr(c, "select")) else {
                    return Verdict::Skip("test: param".into());
                };
                let sel = sel.trim();
                let quoted = |q: char| sel.len() >= 2 && sel.starts_with(q) && sel.ends_with(q);
                let value = if quoted('\'') || quoted('"') {
                    sel[1..sel.len() - 1].to_string()
                } else if sel.parse::<f64>().is_ok() {
                    sel.to_string()
                } else {
                    return Verdict::Skip("test: param".into());
                };
                if x.attr(c, "static") == Some("yes") || n.contains(':') {
                    return Verdict::Skip("test: param".into());
                }
                params.insert(n.to_string(), value);
            }
            "description" | "created" | "modified" => {}
            other => return Verdict::Skip(format!("test: {other}")),
        }
    }
    let Some(stylesheet) = stylesheet else {
        return Verdict::Skip("no stylesheet".into());
    };
    let Some(result) = x.kid(case, "result") else {
        return Verdict::Skip("no result".into());
    };
    let Some(assertion) = x.all_kids(result).into_iter().next() else {
        return Verdict::Skip("no assertion".into());
    };
    let ss = compiled
        .entry(stylesheet.clone())
        .or_insert_with(|| Stylesheet::compile(&stylesheet));
    let got = match ss {
        Ok(ss) => transform_from(ss, env.source.as_deref(), &params, &start).map(|o| o.text),
        Err(e) => Err(e.clone()),
    };
    check(x, assertion, &got, base)
}

fn check(x: &Xml, a: NodeId, got: &Result<String, String>, base: &Path) -> Verdict {
    let pass = |ok: bool, what: String| {
        if ok {
            Verdict::Pass
        } else {
            Verdict::Fail(what)
        }
    };
    match x.local(a) {
        "error" => {
            return match got {
                Err(_) => Verdict::Pass,
                Ok(t) => Verdict::Fail(format!(
                    "expected error {}, got {}",
                    x.attr(a, "code").unwrap_or(""),
                    t.chars().take(200).collect::<String>()
                )),
            };
        }
        "any-of" => {
            let vs: Vec<Verdict> = x
                .all_kids(a)
                .into_iter()
                .map(|k| check(x, k, got, base))
                .collect();
            if vs.contains(&Verdict::Pass) {
                return Verdict::Pass;
            }
            return vs
                .into_iter()
                .find(|v| matches!(v, Verdict::Skip(_)))
                .unwrap_or(Verdict::Fail("no alternative matched".into()));
        }
        "all-of" => {
            for k in x.all_kids(a) {
                let v = check(x, k, got, base);
                if v != Verdict::Pass {
                    return v;
                }
            }
            return Verdict::Pass;
        }
        "not" => {
            let Some(k) = x.all_kids(a).into_iter().next() else {
                return Verdict::Skip("empty not".into());
            };
            return match check(x, k, got, base) {
                Verdict::Pass => Verdict::Fail("negated assertion held".into()),
                Verdict::Fail(_) => Verdict::Pass,
                skip => skip,
            };
        }
        _ => {}
    }
    let text = match got {
        Ok(t) => t,
        Err(e) => return Verdict::Fail(format!("error: {}", e.lines().next().unwrap_or(""))),
    };
    match x.local(a) {
        "assert-xml" => {
            let want = match x.attr(a, "file") {
                Some(f) => match std::fs::read_to_string(base.join(f)) {
                    Ok(t) => t,
                    Err(e) => return Verdict::Fail(e.to_string()),
                },
                None => x.text(a),
            };
            match (canonical_xml(&want), canonical_xml(text)) {
                (Ok(w), Ok(h)) => pass(
                    w == h,
                    format!("got {}", h.chars().take(300).collect::<String>()),
                ),
                (Err(e), _) => Verdict::Skip(format!("expected XML unreadable: {e}")),
                (_, Err(_)) => Verdict::Fail(format!(
                    "got {}",
                    text.chars().take(300).collect::<String>()
                )),
            }
        }
        "assert-string-value" => {
            let have = with_session(Some(format!("<w>{text}</w>").as_bytes()), base, &[], |s| {
                s.eval("string(/)")
                    .ok()
                    .and_then(|v| match v.into_iter().next() {
                        Some(ResultItem::Atomic { value, .. }) => Some(value),
                        _ => None,
                    })
            })
            .ok()
            .flatten()
            .unwrap_or_default();
            let mut want = x.text(a);
            let mut have = have;
            if x.attr(a, "normalize-space") == Some("true") {
                want = want.split_whitespace().collect::<Vec<_>>().join(" ");
                have = have.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            pass(want == have, format!("got {have:?}"))
        }
        "assert" => {
            let ns = x.in_scope(a);
            let r = with_session(Some(text.as_bytes()), base, &ns, |s| {
                // The catalog takes the assertion's effective boolean value.
                s.eval(&format!("boolean(({}))", x.text(a).trim()))
            });
            match r {
                Ok(Ok(v)) => pass(
                    matches!(v.as_slice(), [ResultItem::Atomic { type_name: "xs:boolean", value }] if value == "true"),
                    format!(
                        "assertion false on {}",
                        text.chars().take(200).collect::<String>()
                    ),
                ),
                Ok(Err(e)) => Verdict::Fail(format!("assertion: {e}")),
                Err(_) => Verdict::Skip("result is not a single document".into()),
            }
        }
        other => Verdict::Skip(format!("assertion {other}")),
    }
}
