//! Compiles and validates against a local corpus of real schemas.
//!
//! Ignored by default; run with
//! `cargo test -p xylem-schema --release --test corpus -- --ignored --nocapture`.
//! The corpus root is taken from `XYLEM_SCHEMA_CORPUS`; the tests are skipped
//! when it is not set.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use xylem_dom::Document;
use xylem_schema::{Catalog, ErrorKind, Grammar};

fn corpus_root() -> Option<PathBuf> {
    let root = std::env::var_os("XYLEM_SCHEMA_CORPUS").map(PathBuf::from);
    if root.is_none() {
        eprintln!("XYLEM_SCHEMA_CORPUS is not set; skipping the schema corpus test");
    }
    root
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
}

fn kind_name(k: &ErrorKind) -> &'static str {
    match k {
        ErrorKind::WrongRoot { .. } => "WrongRoot",
        ErrorKind::UndeclaredElement { .. } => "UndeclaredElement",
        ErrorKind::ElementNotAllowed { .. } => "ElementNotAllowed",
        ErrorKind::IncompleteContent { .. } => "IncompleteContent",
        ErrorKind::TextNotAllowed => "TextNotAllowed",
        ErrorKind::InvalidText { .. } => "InvalidText",
        ErrorKind::MissingAttribute { .. } => "MissingAttribute",
        ErrorKind::UndeclaredAttribute { .. } => "UndeclaredAttribute",
        ErrorKind::InvalidAttributeValue { .. } => "InvalidAttributeValue",
        ErrorKind::DuplicateId { .. } => "DuplicateId",
        ErrorKind::UnresolvedIdRef { .. } => "UnresolvedIdRef",
        ErrorKind::UndeclaredEntity { .. } => "UndeclaredEntity",
    }
}

#[test]
#[ignore = "needs the local schema corpus"]
fn compile_all_corpus_schemas() {
    let Some(root) = corpus_root() else { return };
    let mut files = Vec::new();
    walk(&root, "xsd", &mut files);
    files.sort();
    assert!(!files.is_empty(), "no schemas under {}", root.display());
    let mut total_decls = 0;
    let mut slowest = (0.0f64, PathBuf::new());
    let mut warnings: BTreeMap<String, usize> = BTreeMap::new();
    let start = Instant::now();
    let mut failures = 0;
    for f in &files {
        let t = Instant::now();
        match Grammar::from_xsd(f) {
            Ok(g) => {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                if ms > slowest.0 {
                    slowest = (ms, f.clone());
                }
                total_decls += g.decl_ids().count();
                for w in g.warnings() {
                    *warnings.entry(w.clone()).or_default() += 1;
                }
            }
            Err(e) => {
                failures += 1;
                println!("FAILED {}: {e}", f.display());
            }
        }
    }
    println!(
        "compiled {} schemas ({} element declarations) in {:.1} ms; slowest {:.2} ms ({}); {failures} failures",
        files.len(),
        total_decls,
        start.elapsed().as_secs_f64() * 1000.0,
        slowest.0,
        slowest.1.display()
    );
    for (w, n) in &warnings {
        println!("  warning x{n}: {w}");
    }
    assert_eq!(failures, 0);
}

#[test]
#[ignore = "needs the local schema corpus"]
fn validate_corpus_instances() {
    let Some(root) = corpus_root() else { return };
    let mut files = Vec::new();
    walk(&root, "xml", &mut files);
    files.sort();
    let catalog = Catalog::default();
    let mut histogram: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut samples: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    let mut validated = 0;
    let mut largest: Option<(usize, PathBuf)> = None;
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let Ok(doc) = Document::from_bytes(&bytes) else {
            continue;
        };
        let Some(grammar) = Grammar::for_document(&doc, Some(f), &catalog) else {
            continue;
        };
        let grammar = match grammar {
            Ok(g) => g,
            Err(e) => {
                println!("{}: cannot load grammar: {e}", f.display());
                continue;
            }
        };
        let elements = doc
            .descendants(doc.root())
            .filter(|&n| doc.is_element(n))
            .count();
        let t = Instant::now();
        let errors = grammar.validate(&doc);
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        validated += 1;
        if largest.as_ref().is_none_or(|(n, _)| elements > *n) {
            largest = Some((elements, f.clone()));
        }
        println!(
            "{}: {elements} elements, {} errors, validated in {ms:.2} ms",
            f.display(),
            errors.len()
        );
        for e in &errors {
            let k = kind_name(&e.kind);
            *histogram.entry(k).or_default() += 1;
            let s = samples.entry(k).or_default();
            if s.len() < 3 {
                s.push(format!("{} at {}", e.message, doc.path(e.node)));
            }
        }
    }
    println!("validated {validated} documents; errors by kind:");
    for (k, n) in &histogram {
        println!("  {k}: {n}");
        for s in &samples[k] {
            println!("      e.g. {s}");
        }
    }

    // Timing on the largest instance (best of five runs).
    if let Some((elements, path)) = largest {
        let doc = Document::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
        let t = Instant::now();
        let g = Grammar::for_document(&doc, Some(&path), &catalog)
            .unwrap()
            .unwrap();
        let compile = t.elapsed().as_secs_f64() * 1000.0;
        let mut best = f64::MAX;
        let mut n = 0;
        for _ in 0..5 {
            let t = Instant::now();
            n = g.validate(&doc).len();
            best = best.min(t.elapsed().as_secs_f64() * 1000.0);
        }
        println!(
            "largest instance {}: {elements} elements; grammar load {compile:.2} ms; validate {best:.2} ms; {n} errors",
            path.display()
        );
    }
}

/// Generates a schema with 28,000 global element declarations and a 20,000
/// element instance, and times compilation and validation.
#[test]
#[ignore = "benchmark"]
fn synthetic_large_schema() {
    const N: usize = 28_000;
    let mut xsd = String::from(
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
<xs:simpleType name="code"><xs:restriction base="xs:token"><xs:pattern value="[A-Z]{2}\d{3}"/></xs:restriction></xs:simpleType>
<xs:attributeGroup name="common"><xs:attribute name="id" type="xs:ID"/>
<xs:attribute name="status"><xs:simpleType><xs:restriction base="xs:NMTOKEN"><xs:enumeration value="a"/><xs:enumeration value="b"/></xs:restriction></xs:simpleType></xs:attribute></xs:attributeGroup>
<xs:element name="doc"><xs:complexType><xs:choice maxOccurs="unbounded">"#,
    );
    for i in 0..200 {
        xsd.push_str(&format!(r#"<xs:element ref="e{i}"/>"#));
    }
    xsd.push_str(
        "</xs:choice></xs:complexType></xs:element>
",
    );
    for i in 0..N {
        let (a, b, c) = ((i + 1) % N, (i + 2) % N, (i * 7 + 3) % N);
        xsd.push_str(&format!(
            r#"<xs:element name="e{i}"><xs:annotation><xs:documentation>Element {i}.</xs:documentation></xs:annotation>
<xs:complexType mixed="true"><xs:sequence><xs:element ref="e{a}" minOccurs="0"/><xs:choice minOccurs="0" maxOccurs="unbounded"><xs:element ref="e{b}"/><xs:element ref="e{c}"/></xs:choice></xs:sequence>
<xs:attributeGroup ref="common"/><xs:attribute name="code" type="code"/><xs:attribute name="n" type="xs:positiveInteger"/></xs:complexType></xs:element>
"#
        ));
    }
    xsd.push_str("</xs:schema>");
    let dir = std::env::temp_dir().join(format!("xylem-schema-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("big.xsd");
    std::fs::write(&path, &xsd).unwrap();

    let t = Instant::now();
    let g = Grammar::from_xsd(&path).unwrap();
    let compile = t.elapsed().as_secs_f64() * 1000.0;

    let mut xml = String::from("<doc>");
    for k in 0..10_000 {
        let i = k % 200;
        xml.push_str(&format!(
            r#"<e{i} id="x{k}" status="a" code="AB123" n="3">text<e{}/></e{i}>"#,
            (i + 1) % N
        ));
    }
    xml.push_str("</doc>");
    let doc = Document::from_str(&xml).unwrap();
    let elements = doc
        .descendants(doc.root())
        .filter(|&n| doc.is_element(n))
        .count();
    let mut best = f64::MAX;
    let mut errors = 0;
    for _ in 0..5 {
        let t = Instant::now();
        errors = g.validate(&doc).len();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    std::fs::remove_dir_all(&dir).ok();
    println!(
        "synthetic: {} declarations, {:.1} MB schema, compile {compile:.1} ms; {elements}-element instance validated in {best:.2} ms ({errors} errors)",
        g.decl_ids().count(),
        xsd.len() as f64 / 1e6
    );
    assert_eq!(errors, 0);
}

/// Validates every `*.xml` under `XYLEM_INSTANCE_CORPUS` against the schema
/// it points to, with `XYLEM_SCHEMA_MAP=<url prefix>=<local folder>` mapping
/// a server location to a local copy (as `schema_locations` does in the
/// app), and prints the error count and first messages per document.
#[test]
#[ignore = "needs XYLEM_INSTANCE_CORPUS and XYLEM_SCHEMA_MAP"]
fn instances_validate_against_mapped_schemas() {
    let (Some(root), Some(map)) = (
        std::env::var_os("XYLEM_INSTANCE_CORPUS").map(PathBuf::from),
        std::env::var("XYLEM_SCHEMA_MAP").ok(),
    ) else {
        eprintln!("XYLEM_INSTANCE_CORPUS or XYLEM_SCHEMA_MAP is not set; skipping");
        return;
    };
    // Several mappings are separated by `;`.
    let mut catalog = Catalog::new();
    for entry in map.split(';').filter(|e| !e.trim().is_empty()) {
        let (prefix, folder) = entry
            .split_once('=')
            .expect("XYLEM_SCHEMA_MAP is <url prefix>=<folder>[;...]");
        catalog.add_prefix(prefix.trim(), folder.trim());
    }
    let mut files = Vec::new();
    walk(&root, "xml", &mut files);
    files.sort();
    let started = Instant::now();
    let (mut valid, mut invalid, mut unresolved) = (0, 0, 0);
    let mut messages: BTreeMap<String, usize> = BTreeMap::new();
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let Ok(doc) = Document::from_bytes(&bytes) else {
            continue;
        };
        match Grammar::for_document(&doc, Some(f), &catalog) {
            None => {}
            Some(Err(e)) => {
                unresolved += 1;
                eprintln!("{}: schema not loaded: {e}", f.display());
            }
            Some(Ok(g)) => {
                let errors: Vec<_> = g
                    .validate(&doc)
                    .into_iter()
                    .filter(|e| e.severity == xylem_schema::Severity::Error)
                    .collect();
                if errors.is_empty() {
                    valid += 1;
                } else {
                    invalid += 1;
                    for e in errors {
                        *messages.entry(e.message).or_default() += 1;
                    }
                }
            }
        }
    }
    println!(
        "{} documents in {:.1?}: {valid} valid, {invalid} with errors, {unresolved} whose schema did not load",
        files.len(),
        started.elapsed()
    );
    let mut common: Vec<_> = messages.into_iter().collect();
    common.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (m, n) in common.into_iter().take(15) {
        println!("  {n:>5}  {m}");
    }
}
