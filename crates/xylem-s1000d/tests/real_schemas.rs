//! Issue x schema-type matrix for new S1000D modules.
//!
//! For every issue the template code supports and every schema type the
//! "New S1000D module" dialog offers for it, this creates a module exactly as
//! the app does (same functions, same dialog defaults), then checks that it
//!
//! 1. validates against that issue's official schema with `xylem-schema`,
//!    reporting no errors (placeholders are not special-cased: the module must
//!    be valid as created);
//! 2. detects back as the same issue and schema type;
//! 3. survives save and reload byte for byte.
//!
//! The schemas are not part of the repository. The folder is read from
//! `XYLEM_S1000D_SCHEMAS`, or else `<config dir>/schemas/s1000d` (the app's
//! default; `XYLEM_CONFIG_DIR` is honoured). It must be laid out like
//! www.s1000d.org (`S1000D_4-1/xml_schema_flat/descript.xsd`, ...). When no
//! folder is found the test prints a message and passes, so machines without
//! the schemas are not affected. An issue whose schema subfolder is missing is
//! skipped on its own. Run with `--nocapture` to see the matrix.

use std::path::{Path, PathBuf};
use xylem_dom::Document;
use xylem_s1000d::{
    Dmc, Issue, IssueDate, NewModuleOptions, Pmc, SchemaKind, available_kinds, detect,
    new_data_module, new_publication_module,
};
use xylem_schema::{Catalog, Grammar, Severity};

/// The schema folder, following the app's lookup order.
fn schema_root() -> Option<PathBuf> {
    let configured = std::env::var_os("XYLEM_S1000D_SCHEMAS").map(PathBuf::from);
    let default = || {
        let base = if let Some(dir) = std::env::var_os("XYLEM_CONFIG_DIR") {
            PathBuf::from(dir)
        } else if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?).join("Xylem")
        } else if cfg!(target_os = "macos") {
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support/Xylem")
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?
                .join("Xylem")
        };
        Some(base.join("schemas").join("s1000d"))
    };
    configured.or_else(default).filter(|p| p.is_dir())
}

/// Same mapping as the app's `add_s1000d_catalog`.
fn issue_folder(root: &Path, issue: Issue) -> Option<PathBuf> {
    let dir = format!("S1000D_{}", issue.schema_token());
    [root.join(dir), root.join(issue.as_str())]
        .into_iter()
        .find(|p| p.is_dir())
}

fn catalog(root: &Path) -> Catalog {
    let mut cat = Catalog::new();
    for issue in Issue::ALL {
        if let Some(folder) = issue_folder(root, issue) {
            let dir = format!("S1000D_{}", issue.schema_token());
            for scheme in ["http", "https"] {
                cat.add_prefix(&format!("{scheme}://www.s1000d.org/{dir}/"), folder.clone());
            }
        }
    }
    cat
}

/// The defaults of the app's new-module dialog.
const DEFAULT_DMC: &str = "DMC-XYLEM-A-00-00-00-00A-040A-A";
const DEFAULT_PMC: &str = "PMC-XYLEM-00000-00001-00";

/// Creates the module as the app's dialog does for `issue` and `kind`.
fn create(issue: Issue, kind: SchemaKind) -> String {
    let date = IssueDate::new(2026, 11, 24);
    if kind == SchemaKind::Pm {
        let pmc = Pmc::parse(DEFAULT_PMC).unwrap();
        let options = NewModuleOptions::new(date);
        new_publication_module(issue, &pmc, "New publication", ("en", "US"), &options)
    } else {
        let dmc = Dmc::parse(DEFAULT_DMC).unwrap();
        new_data_module(
            issue,
            kind,
            &dmc,
            "New equipment",
            Some("Description"),
            ("en", "US"),
            date,
        )
    }
    .unwrap_or_else(|e| panic!("{issue} {kind:?}: {e}"))
}

/// The schema types the dialog lists for `issue`.
fn dialog_kinds(issue: Issue) -> Vec<SchemaKind> {
    let mut kinds = available_kinds(issue);
    if !issue.is_legacy() {
        kinds.push(SchemaKind::Pm);
    }
    kinds
}

/// Runs the checks; returns the problems found.
fn check(cat: &Catalog, issue: Issue, kind: SchemaKind) -> Vec<String> {
    let mut problems = Vec::new();
    let xml = create(issue, kind);
    let doc = match Document::from_str(&xml) {
        Ok(d) => d,
        Err(e) => return vec![format!("does not parse: {e}")],
    };

    // 1. Validation against the real schema.
    match Grammar::for_document(&doc, None, cat) {
        Some(Ok(grammar)) => {
            for e in grammar
                .validate(&doc)
                .into_iter()
                .filter(|e| e.severity == Severity::Error)
            {
                problems.push(format!("schema: {} ({})", e.message, doc.path(e.node)));
            }
        }
        Some(Err(e)) => problems.push(format!("schema did not load: {e}")),
        None => problems.push("no schema reference in the module".to_string()),
    }

    // 2. Detection.
    match detect(&doc) {
        Some(info) => {
            if info.issue != issue {
                problems.push(format!("detected issue {} instead of {issue}", info.issue));
            }
            if info.schema != kind {
                problems.push(format!(
                    "detected type {:?} instead of {kind:?}",
                    info.schema
                ));
            }
        }
        None => problems.push("not detected as S1000D".to_string()),
    }

    // 3. Save and reload, byte for byte (through a real file, as the app saves).
    let dir = std::env::temp_dir().join(format!("xylem-s1000d-matrix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{}-{kind:?}.xml", issue.schema_token()));
    std::fs::write(&path, doc.to_bytes()).unwrap();
    let saved = std::fs::read(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_dir(&dir);
    if saved != xml.as_bytes() {
        problems.push("saved bytes differ from the created module".to_string());
    }
    match Document::from_bytes(&saved) {
        Ok(reloaded) if reloaded.to_bytes() != saved => {
            problems.push("reloaded document serialises differently".to_string());
        }
        Ok(_) => {}
        Err(e) => problems.push(format!("saved file does not reload: {e}")),
    }
    problems
}

#[test]
fn new_modules_are_valid_for_every_issue_and_type() {
    let Some(root) = schema_root() else {
        eprintln!(
            "SKIPPED: no S1000D schema folder (set XYLEM_S1000D_SCHEMAS to a folder laid out \
             like www.s1000d.org, or install the schemas under <config dir>/schemas/s1000d)"
        );
        return;
    };
    let cat = catalog(&root);
    let (mut passed, mut skipped, mut failures) = (0, 0, Vec::new());
    for issue in Issue::ALL {
        if issue_folder(&root, issue).is_none() {
            eprintln!("skip  {issue}: no schema folder for this issue under the schema root");
            skipped += dialog_kinds(issue).len();
            continue;
        }
        for kind in dialog_kinds(issue) {
            let problems = check(&cat, issue, kind);
            if problems.is_empty() {
                passed += 1;
                println!("ok    {issue:<6} {kind:?}");
            } else {
                println!("FAIL  {issue:<6} {kind:?}");
                for p in &problems {
                    println!("          {p}");
                }
                failures.push(format!("{issue} {kind:?}: {problems:?}"));
            }
        }
    }
    println!("{passed} passed, {} failed, {skipped} skipped", failures.len());
    negative_control(&cat, &root);
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Proves the validator is really checking: a module with an element the
/// schema does not allow must produce errors.
fn negative_control(cat: &Catalog, root: &Path) {
    let issue = Issue::V4_1;
    if issue_folder(root, issue).is_none() {
        return;
    }
    let xml = create(issue, SchemaKind::Descript).replace("<content>", "<content><notAnElement/>");
    let doc = Document::from_str(&xml).unwrap();
    let grammar = Grammar::for_document(&doc, None, cat)
        .expect("schema reference")
        .expect("schema loads");
    assert!(
        grammar
            .validate(&doc)
            .iter()
            .any(|e| e.severity == Severity::Error),
        "the validator accepted an invalid module"
    );
}
