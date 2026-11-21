//! Validates freshly generated data modules against the official S1000D
//! schemas. Ignored by default because the schemas aren't part of the
//! repository; point `XYLEM_S1000D_SCHEMAS` at a folder laid out like
//! www.s1000d.org (`S1000D_4-1/xml_schema_flat/descript.xsd`, ...) and run
//! `cargo test -p xylem-s1000d --test real_schemas -- --ignored --nocapture`.

use std::path::PathBuf;
use xylem_dom::Document;
use xylem_s1000d::{Dmc, Issue, IssueDate, available_kinds, new_data_module};
use xylem_schema::{Catalog, Grammar, Severity};

fn schema_root() -> Option<PathBuf> {
    let root = std::env::var_os("XYLEM_S1000D_SCHEMAS").map(PathBuf::from);
    if root.is_none() {
        eprintln!("XYLEM_S1000D_SCHEMAS is not set; skipping");
    }
    root
}

fn catalog(root: &std::path::Path) -> Catalog {
    let mut cat = Catalog::new();
    for issue in Issue::ALL {
        let dir = format!("S1000D_{}", issue.schema_token());
        if root.join(&dir).is_dir() {
            cat.add_prefix(&format!("http://www.s1000d.org/{dir}/"), root.join(&dir));
        }
    }
    cat
}

#[test]
#[ignore = "needs the official S1000D schemas (XYLEM_S1000D_SCHEMAS)"]
fn generated_modules_are_schema_valid() {
    let Some(root) = schema_root() else { return };
    let cat = catalog(&root);
    let dmc = Dmc::parse("DMC-XYLEM-A-29-10-00-00A-040A-A").unwrap();
    let mut failures = Vec::new();
    let mut checked = 0;
    for issue in Issue::ALL {
        for kind in available_kinds(issue) {
            let xml = new_data_module(
                issue,
                kind,
                &dmc,
                "Hydraulic pump",
                Some("Description"),
                ("en", "US"),
                IssueDate::new(2026, 11, 20),
            )
            .unwrap_or_else(|e| panic!("{issue} {kind:?}: {e}"));
            let doc = Document::from_str(&xml).unwrap_or_else(|e| panic!("{issue} {kind:?}: {e}"));
            let grammar = match Grammar::for_document(&doc, None, &cat) {
                Some(Ok(g)) => g,
                Some(Err(e)) => {
                    failures.push(format!("{issue} {kind:?}: schema did not load: {e}"));
                    continue;
                }
                None => {
                    failures.push(format!("{issue} {kind:?}: no schema reference"));
                    continue;
                }
            };
            checked += 1;
            let errors: Vec<String> = grammar
                .validate(&doc)
                .into_iter()
                .filter(|e| e.severity == Severity::Error)
                .map(|e| format!("{} ({})", e.message, doc.path(e.node)))
                .collect();
            if errors.is_empty() {
                println!("ok    {issue} {kind:?}");
            } else {
                println!("FAIL  {issue} {kind:?}");
                for e in &errors {
                    println!("        {e}");
                }
                failures.push(format!("{issue} {kind:?}: {} error(s)", errors.len()));
            }
        }
    }
    println!("{checked} modules validated, {} problem(s)", failures.len());
    assert!(failures.is_empty(), "{failures:#?}");
}
