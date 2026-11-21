//! Opens and saves every XML file under `XYLEM_XML_CORPUS` and checks that
//! untouched documents come back byte for byte, then that a small edit
//! changes only the bytes it should.
//!
//! Run it against a real content export before trusting Xylem with it:
//!
//! ```text
//! XYLEM_XML_CORPUS=/path/to/content cargo test -p xylem-dom --release \
//!     --test roundtrip_corpus -- --ignored --nocapture
//! ```
//!
//! Files that are not well-formed are counted and listed, not failed: the
//! editor opens those in Source view instead.

use std::path::{Path, PathBuf};
use xylem_dom::Document;

fn xml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            // Tooling copies, not content.
            if p.file_name()
                .is_some_and(|n| n == ".git" || n == ".claude" || n == "node_modules")
            {
                continue;
            }
            xml_files(&p, out);
        } else if p
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("xml") || x.eq_ignore_ascii_case("dita"))
        {
            out.push(p);
        }
    }
}

#[test]
#[ignore = "needs a local XML corpus (XYLEM_XML_CORPUS)"]
fn corpus_round_trips_byte_for_byte() {
    let Some(root) = std::env::var_os("XYLEM_XML_CORPUS").map(PathBuf::from) else {
        eprintln!("XYLEM_XML_CORPUS is not set; skipping");
        return;
    };
    let mut files = Vec::new();
    for part in std::env::split_paths(&root) {
        if part.is_file() {
            files.push(part);
        } else {
            xml_files(&part, &mut files);
        }
    }
    files.sort();
    let (mut ok, mut not_xml, mut edited_ok) = (0usize, Vec::new(), 0usize);
    let mut failures = Vec::new();
    let start = std::time::Instant::now();
    for f in &files {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        let doc = match Document::from_bytes(&bytes) {
            Ok(d) => d,
            Err(e) => {
                not_xml.push(format!("{}: {e}", f.display()));
                continue;
            }
        };
        let out = doc.to_bytes();
        if out != bytes {
            let at = out
                .iter()
                .zip(&bytes)
                .position(|(a, b)| a != b)
                .unwrap_or(out.len().min(bytes.len()));
            failures.push(format!("{} differs at byte {at}", f.display()));
            continue;
        }
        ok += 1;

        // Change the first text node and check nothing else moved: the
        // bytes before it are identical and the document still parses.
        let mut doc = doc;
        let target = doc.descendants(doc.root()).find(|&n| {
            doc.text(n).is_some_and(|t| !t.trim().is_empty())
                && doc.parent(n).is_some_and(|p| doc.is_element(p))
        });
        if let Some(n) = target {
            let (before_xml, offsets) = doc.to_xml_with_offsets();
            let new_text = format!("{}+", doc.text(n).unwrap());
            doc.set_text(n, new_text);
            let after = doc.to_xml();
            let prefix = offsets.get(n.index()).copied().unwrap_or(0);
            if after.get(..prefix) != before_xml.get(..prefix)
                || Document::from_str(&after).is_err()
            {
                failures.push(format!(
                    "{}: editing one text node disturbed other markup",
                    f.display()
                ));
            } else {
                edited_ok += 1;
            }
        }
    }
    eprintln!(
        "{} files in {:.1?}: {ok} round-trip exactly, {edited_ok} edited cleanly, {} not well-formed, {} failed",
        files.len(),
        start.elapsed(),
        not_xml.len(),
        failures.len()
    );
    for n in not_xml.iter().take(10) {
        eprintln!("  not well-formed: {n}");
    }
    assert!(
        failures.is_empty(),
        "round-trip failures:\n{}",
        failures.join("\n")
    );
}
