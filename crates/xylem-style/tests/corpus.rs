//! Parses the real-world XMetaL stylesheets from a local corpus and reports any
//! construct the engine does not support, plus compute timing.
//!
//! Ignored by default (needs the corpus on disk). Run with:
//! `cargo test --release -p xylem-style --test corpus -- --ignored --nocapture`
//!
//! Set the corpus root with the `XYLEM_CSS_CORPUS` environment variable.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use xylem_style::{StyleEngine, Stylesheet};


fn collect_css(dir: &Path, in_customizations: bool, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            collect_css(&path, in_customizations || name == "Customizations", out);
        } else if in_customizations && name.to_ascii_lowercase().ends_with(".css") {
            out.push(path);
        }
    }
}

/// Strips the `origin:line: ` prefix so identical problems group together.
fn message_of(warning: &str) -> &str {
    warning.split_once(": ").map_or(warning, |(_, m)| m)
}

/// A deterministic S1000D-flavoured document with `elements` elements.
fn synthetic_document(elements: usize) -> String {
    const BLOCKS: [&str; 8] = [
        "para",
        "note",
        "warning",
        "figure",
        "bulletedList",
        "numberedList",
        "table",
        "subsection",
    ];
    let mut xml = String::from("<dmodule><content><description>");
    let mut count = 0;
    let mut n = 0usize;
    while count < elements {
        let block = BLOCKS[n % BLOCKS.len()];
        xml.push_str(&format!(
            "<levelledPara><title>Section {n}</title><{block} id=\"b{n}\" applicability=\"a{}\">",
            n % 7
        ));
        count += 3;
        for i in 0..6 {
            xml.push_str(&format!(
                "<listItem><para>item <emphasis bold=\"true\">{i}</emphasis> text</para></listItem>"
            ));
            count += 3;
        }
        xml.push_str(&format!("</{block}></levelledPara>"));
        n += 1;
    }
    xml.push_str("</description></content></dmodule>");
    xml
}

#[test]
#[ignore = "needs a local stylesheet corpus (XYLEM_CSS_CORPUS)"]
fn corpus_parses_and_computes() {
    let Ok(root) = std::env::var("XYLEM_CSS_CORPUS") else {
        eprintln!("XYLEM_CSS_CORPUS is not set; skipping the stylesheet corpus test");
        return;
    };
    let mut files = Vec::new();
    collect_css(Path::new(&root), false, &mut files);
    files.sort();
    assert!(!files.is_empty(), "no stylesheets found under {root}");

    let started = Instant::now();
    let mut sheets = Vec::new();
    let mut problems: BTreeMap<String, usize> = BTreeMap::new();
    let mut rules = 0;
    for file in &files {
        let sheet = Stylesheet::load(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
        rules += sheet.rule_count();
        for warning in sheet.warnings() {
            *problems.entry(message_of(warning).to_string()).or_default() += 1;
        }
        for import in sheet.unresolved_imports() {
            *problems
                .entry(format!("unresolved @import {import}"))
                .or_default() += 1;
        }
        sheets.push((file.clone(), sheet));
    }
    let parse_time = started.elapsed();
    println!(
        "parsed {} stylesheets, {rules} rules in {parse_time:?}",
        files.len()
    );
    println!("{} distinct problems:", problems.len());
    let mut by_count: Vec<_> = problems.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1));
    for (message, count) in by_count {
        println!("  {count:5}  {message}");
    }

    // Compute against a ~20k element synthetic document with the largest sheet.
    let (path, sheet) = sheets.iter().max_by_key(|(_, s)| s.rule_count()).unwrap();
    println!(
        "largest sheet: {} ({} rules)",
        path.display(),
        sheet.rule_count()
    );
    let xml = synthetic_document(20_000);
    let doc = xylem_dom::parse(&xml).expect("synthetic document parses");
    let engine = StyleEngine::new(sheet.clone());
    let mut best = std::time::Duration::MAX;
    for _ in 0..5 {
        let t = Instant::now();
        let map = engine.compute(&doc);
        best = best.min(t.elapsed());
        std::hint::black_box(&map);
    }
    println!(
        "compute over {} nodes: best of 5 = {best:?}",
        doc.node_count()
    );
}
