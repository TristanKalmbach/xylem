//! The QT3 (XPath and XQuery) suite, XPath 2.0 tests only.

use crate::Verdict;
use crate::catalog::Xml;
use std::path::{Path, PathBuf};
use xylem_dom::NodeId;
use xylem_xslt_engine::api::{ResultItem, Session, with_session};

/// What a test needs before its expression can run.
struct Env {
    source: Option<PathBuf>,
    namespaces: Vec<(String, String)>,
}

/// Turns an `<environment>` into what the engine can provide, or the
/// reason it cannot.
fn environment(x: &Xml, e: NodeId, base: &Path) -> Result<Env, String> {
    let mut env = Env {
        source: None,
        namespaces: Vec::new(),
    };
    for c in x.all_kids(e) {
        match x.local(c) {
            "source" => match (x.attr(c, "role"), x.attr(c, "file")) {
                (Some("."), Some(f)) => env.source = Some(base.join(f)),
                _ => return Err("environment: source other than the context item".into()),
            },
            "namespace" => {
                if let (Some(p), Some(u)) = (x.attr(c, "prefix"), x.attr(c, "uri")) {
                    env.namespaces.push((p.to_string(), u.to_string()));
                }
            }
            "description" | "created" | "modified" => {}
            other => return Err(format!("environment: {other}")),
        }
    }
    Ok(env)
}

fn spec_ok(value: &str) -> bool {
    value
        .split_whitespace()
        .any(|t| t == "XP20" || t == "XP20+")
}

/// Whether the dependencies allow running the test, or why not.
fn dependencies(x: &Xml, set: NodeId, case: NodeId) -> Result<(), String> {
    let set_deps = x.kids(set, "dependency");
    let case_deps = x.kids(case, "dependency");
    let spec_of = |deps: &[NodeId]| {
        deps.iter()
            .filter(|&&d| x.attr(d, "type") == Some("spec"))
            .map(|&d| x.attr(d, "value").unwrap_or("").to_string())
            .next()
    };
    let spec = spec_of(&case_deps).or_else(|| spec_of(&set_deps));
    // No spec dependency means the test applies to every version.
    if !spec.as_deref().is_none_or(spec_ok) {
        return Err("not an XPath 2.0 test".into());
    }
    for &d in set_deps.iter().chain(&case_deps) {
        let ty = x.attr(d, "type").unwrap_or("");
        let value = x.attr(d, "value").unwrap_or("");
        let satisfied = x.attr(d, "satisfied") != Some("false");
        match ty {
            "spec" => {}
            "xml-version" | "xsd-version" if value.contains("1.0") || !satisfied => {}
            _ if !satisfied => {}
            _ => return Err(format!("needs {ty} {value}")),
        }
    }
    Ok(())
}

pub fn run_set(root: &Path, file: &Path) -> Vec<(String, Verdict)> {
    let Ok(x) = Xml::load(file) else {
        return vec![(
            "(set)".into(),
            Verdict::Fail("could not read the test set".into()),
        )];
    };
    let catalog = Xml::load(&root.join("catalog.xml")).ok();
    let base = file.parent().unwrap_or(root).to_path_buf();
    let set = x.root();
    let mut out = Vec::new();
    for case in x.kids(set, "test-case") {
        let name = x.attr(case, "name").unwrap_or("?").to_string();
        let v = run_case(&x, set, case, &base, root, catalog.as_ref());
        out.push((name, v));
    }
    out
}

fn run_case(
    x: &Xml,
    set: NodeId,
    case: NodeId,
    base: &Path,
    root: &Path,
    catalog: Option<&Xml>,
) -> Verdict {
    if let Err(why) = dependencies(x, set, case) {
        return Verdict::Skip(why);
    }
    // The environment: inline, named in the test set, or in the catalog.
    let env = match x.kid(case, "environment") {
        Some(e) => match x.attr(e, "ref") {
            None => environment(x, e, base),
            Some(r) => {
                let local = x
                    .kids(set, "environment")
                    .into_iter()
                    .find(|&n| x.attr(n, "name") == Some(r));
                match local {
                    Some(n) => environment(x, n, base),
                    None => match catalog.and_then(|c| {
                        c.kids(c.root(), "environment")
                            .into_iter()
                            .find(|&n| c.attr(n, "name") == Some(r))
                            .map(|n| (c, n))
                    }) {
                        Some((c, n)) => environment(c, n, root),
                        None => Err(format!("environment {r} not found")),
                    },
                }
            }
        },
        None => Ok(Env {
            source: None,
            namespaces: Vec::new(),
        }),
    };
    let env = match env {
        Ok(e) => e,
        Err(why) => return Verdict::Skip(why),
    };
    let Some(test) = x.kid(case, "test") else {
        return Verdict::Skip("no test".into());
    };
    let expr = match x.attr(test, "file") {
        Some(f) => match std::fs::read_to_string(base.join(f)) {
            Ok(t) => t,
            Err(e) => return Verdict::Fail(e.to_string()),
        },
        None => x.text(test),
    };
    let Some(result) = x.kid(case, "result") else {
        return Verdict::Skip("no result".into());
    };
    let Some(assertion) = x.all_kids(result).into_iter().next() else {
        return Verdict::Skip("no assertion".into());
    };
    let context = match &env.source {
        Some(p) => match std::fs::read(p) {
            Ok(b) => Some(b),
            Err(e) => return Verdict::Fail(format!("{}: {e}", p.display())),
        },
        None => None,
    };
    let r = with_session(context.as_deref(), base, &env.namespaces, |s| {
        let got = s.keep("result", &expr);
        check(x, assertion, s, &got)
    });
    match r {
        Ok(v) => v,
        Err(e) => Verdict::Fail(format!("context: {e}")),
    }
}

fn string_of(items: &[ResultItem]) -> String {
    items
        .iter()
        .map(|i| match i {
            ResultItem::Node { string, .. } => string.clone(),
            ResultItem::Atomic { value, .. } => value.clone(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_true(items: &[ResultItem]) -> bool {
    matches!(items, [ResultItem::Atomic { type_name: "xs:boolean", value }] if value == "true")
}

/// Checks one assertion against the expression's outcome.
fn check(x: &Xml, a: NodeId, s: &mut Session, got: &Result<Vec<ResultItem>, String>) -> Verdict {
    let pass = |ok: bool, what: String| {
        if ok {
            Verdict::Pass
        } else {
            Verdict::Fail(what)
        }
    };
    let text = x.text(a);
    if x.local(a) == "error" {
        return match got {
            Err(_) => Verdict::Pass,
            Ok(v) => Verdict::Fail(format!(
                "expected error {}, got {}",
                x.attr(a, "code").unwrap_or(""),
                string_of(v)
            )),
        };
    }
    match x.local(a) {
        "any-of" => {
            let kids = x.all_kids(a);
            let vs: Vec<Verdict> = kids.iter().map(|&k| check(x, k, s, got)).collect();
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
                let v = check(x, k, s, got);
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
            return match check(x, k, s, got) {
                Verdict::Pass => Verdict::Fail("negated assertion held".into()),
                Verdict::Fail(_) => Verdict::Pass,
                skip => skip,
            };
        }
        _ => {}
    }
    let items = match got {
        Ok(v) => v,
        Err(e) => return Verdict::Fail(format!("error: {e}")),
    };
    match x.local(a) {
        "assert-true" => pass(is_true(items), format!("got {}", string_of(items))),
        "assert-false" => pass(
            matches!(items.as_slice(), [ResultItem::Atomic { type_name: "xs:boolean", value }] if value == "false"),
            format!("got {}", string_of(items)),
        ),
        "assert-empty" => pass(items.is_empty(), format!("got {}", string_of(items))),
        "assert-count" => {
            let n: usize = text.trim().parse().unwrap_or(usize::MAX);
            pass(items.len() == n, format!("got {} items", items.len()))
        }
        "assert-string-value" => {
            let mut want = text.clone();
            let mut have = string_of(items);
            if x.attr(a, "normalize-space") == Some("true") {
                want = want.split_whitespace().collect::<Vec<_>>().join(" ");
                have = have.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            pass(want == have, format!("got {have:?}, want {want:?}"))
        }
        "assert-eq" => match s.eval(&format!("$result eq ({})", text.trim())) {
            Ok(v) => pass(
                is_true(&v),
                format!("got {}, want {}", string_of(items), text.trim()),
            ),
            Err(e) => Verdict::Fail(format!("comparing: {e}")),
        },
        "assert-deep-eq" => match s.eval(&format!("({})", text.trim())) {
            Ok(v) => pass(
                string_of(&v) == string_of(items) && v.len() == items.len(),
                format!("got {}", string_of(items)),
            ),
            Err(e) => Verdict::Fail(format!("expected value: {e}")),
        },
        "assert-type" => match s.eval(&format!("$result instance of {}", text.trim())) {
            Ok(v) => pass(is_true(&v), format!("not of type {}", text.trim())),
            Err(e) => Verdict::Fail(format!("type test: {e}")),
        },
        "assert" => match s.eval(text.trim()) {
            Ok(v) => pass(
                is_true(&v),
                format!("assertion false; got {}", string_of(items)),
            ),
            Err(e) => Verdict::Fail(format!("assertion: {e}")),
        },
        "assert-permutation" => match s.eval(&format!("({})", text.trim())) {
            Ok(v) => {
                let mut a: Vec<String> = v
                    .iter()
                    .map(|i| string_of(std::slice::from_ref(i)))
                    .collect();
                let mut b: Vec<String> = items
                    .iter()
                    .map(|i| string_of(std::slice::from_ref(i)))
                    .collect();
                a.sort();
                b.sort();
                pass(a == b, format!("got {}", string_of(items)))
            }
            Err(e) => Verdict::Fail(format!("expected value: {e}")),
        },
        "assert-xml" => {
            let ours: String = items
                .iter()
                .map(|i| match i {
                    ResultItem::Node { xml, .. } => xml.clone(),
                    ResultItem::Atomic { value, .. } => value.clone(),
                })
                .collect();
            match (
                xylem_xslt_engine::api::canonical_xml(&text),
                xylem_xslt_engine::api::canonical_xml(&ours),
            ) {
                (Ok(w), Ok(h)) => pass(w == h, format!("got {h}")),
                _ => Verdict::Fail(format!("got {ours}")),
            }
        }
        other => Verdict::Skip(format!("assertion {other}")),
    }
}
