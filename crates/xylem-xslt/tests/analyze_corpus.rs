//! Reads every stylesheet under `XYLEM_XSLT_CORPUS` and checks that the
//! analyser can follow it: every XPath attribute is read into tokens that
//! account for all of its text, every file can be edited exactly, and
//! analysing every top-level stylesheet (one that no other file includes or
//! imports) finishes.
//!
//! ```text
//! XYLEM_XSLT_CORPUS=/path/to/stylesheets cargo test -p xylem-xslt --release \
//!     --test analyze_corpus -- --ignored --nocapture
//! ```

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use xylem_xslt::stylesheet::{Module, path_key};
use xylem_xslt::{Severity, analyze, xpath};

fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == ".git" || n == ".claude") {
                continue;
            }
            files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("xsl") || x.eq_ignore_ascii_case("xslt"))
        {
            out.push(p);
        }
    }
}

/// Whether the tokens cover `src` apart from whitespace and comments.
fn covers(src: &str, tokens: &[xpath::Token]) -> bool {
    let mut rest = String::new();
    let mut last = 0;
    for t in tokens {
        rest.push_str(&src[last..t.span.start]);
        last = t.span.end;
    }
    rest.push_str(&src[last..]);
    // What lies between tokens may only be whitespace and (: comments :).
    let mut depth = 0;
    let b: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '(' && b.get(i + 1) == Some(&':') {
            depth += 1;
            i += 2;
        } else if b[i] == ':' && b.get(i + 1) == Some(&')') && depth > 0 {
            depth -= 1;
            i += 2;
        } else if depth > 0 || b[i].is_whitespace() {
            i += 1;
        } else {
            return false;
        }
    }
    true
}

#[test]
#[ignore = "needs a local stylesheet folder (XYLEM_XSLT_CORPUS)"]
fn the_analyser_reads_every_stylesheet() {
    let Some(root) = std::env::var_os("XYLEM_XSLT_CORPUS").map(PathBuf::from) else {
        eprintln!("XYLEM_XSLT_CORPUS is not set; skipping");
        return;
    };
    let mut all = Vec::new();
    for part in std::env::split_paths(&root) {
        files(&part, &mut all);
    }
    all.sort();
    let (mut expressions, mut unreadable, mut not_covered, mut not_editable, mut not_xml) =
        (0, 0, 0, 0, 0);
    let mut entity_values = 0;
    let mut problems = Vec::new();
    let mut referenced = HashSet::new();
    for f in &all {
        let m = match Module::load(f) {
            Ok(m) => m,
            Err(_) => {
                not_xml += 1;
                continue;
            }
        };
        if !m.editable {
            not_editable += 1;
            problems.push(format!("not editable exactly: {}", f.display()));
        }
        for n in m.doc.descendants(m.doc.root()) {
            if !m.doc.is_element(n) || m.xsl_name(n).is_none() {
                continue;
            }
            if (m.is_xsl(n, "include") || m.is_xsl(n, "import"))
                && let Some(h) = m.doc.attr(n, "href")
            {
                referenced.insert(path_key(&f.parent().unwrap().join(h)));
            }
            for attr in [
                "select",
                "test",
                "use",
                "value",
                "group-by",
                "group-adjacent",
            ] {
                let Some(v) = m.raw_attr(n, attr) else {
                    continue;
                };
                expressions += 1;
                if v.entities {
                    entity_values += 1;
                    continue;
                }
                match xpath::tokenize(&v.value) {
                    None => {
                        unreadable += 1;
                        problems.push(format!(
                            "unreadable {}:{} {attr}=\"{}\"",
                            f.display(),
                            m.saxon_line(n),
                            v.value
                        ));
                    }
                    Some(t) => {
                        if !covers(&v.value, &t) {
                            not_covered += 1;
                            problems.push(format!(
                                "tokens miss text {}:{} {attr}=\"{}\"",
                                f.display(),
                                m.saxon_line(n),
                                v.value
                            ));
                        }
                        if xpath::partners(&t).is_none() {
                            unreadable += 1;
                            problems.push(format!(
                                "unbalanced {}:{} {attr}=\"{}\"",
                                f.display(),
                                m.saxon_line(n),
                                v.value
                            ));
                        }
                    }
                }
            }
        }
    }
    let tops: Vec<&PathBuf> = all
        .iter()
        .filter(|f| !referenced.contains(&path_key(f)))
        .collect();
    let mut by_rule: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut analysed = 0;
    let start = std::time::Instant::now();
    for top in &tops {
        match analyze(top, None) {
            Ok(a) => {
                analysed += 1;
                for f in &a.findings {
                    let e = by_rule.entry(format!("{:?}", f.rule)).or_default();
                    e.0 += 1;
                    if f.severity != Severity::Note && !f.edits.is_empty() {
                        e.1 += 1;
                    }
                }
            }
            Err(e) => problems.push(format!("analysis failed: {e}")),
        }
    }
    eprintln!(
        "{} stylesheet files, {not_xml} not XML, {not_editable} not editable exactly; {expressions} expressions, \
         {entity_values} using DTD entities (skipped), {unreadable} unreadable, {not_covered} not fully covered by tokens",
        all.len()
    );
    eprintln!(
        "{} top-level stylesheets analysed in {:.1?} ({analysed} ok)",
        tops.len(),
        start.elapsed()
    );
    for (rule, (n, fixable)) in &by_rule {
        eprintln!(
            "  {rule}: {n} findings, {fixable} with a fix (counted once per top stylesheet that includes them)"
        );
    }
    for p in problems.iter().take(15) {
        eprintln!("  {p}");
    }
    assert!(
        unreadable == 0 && not_covered == 0 && analysed == tops.len(),
        "{} problems",
        problems.len()
    );
}
