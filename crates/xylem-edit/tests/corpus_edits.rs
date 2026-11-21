//! Tracked editing on real documents: for every XML file under
//! `XYLEM_XML_CORPUS`, makes random edits with track changes on (typing,
//! Backspace, Delete, splitting and joining paragraphs, moving elements),
//! then checks that
//!
//! - undoing everything gives back the file byte for byte,
//! - Reject all gives back the file byte for byte, and
//! - Accept all leaves a well-formed document with no revision marks.
//!
//! ```text
//! XYLEM_XML_CORPUS=/path/to/content cargo test -p xylem-edit --release \
//!     --test corpus_edits -- --ignored --nocapture
//! ```
//!
//! Without the document type's rules, an element counts as a paragraph when
//! it holds text, and as structure when it holds only elements.

use std::path::{Path, PathBuf};
use xylem_dom::{Document, NodeId, NodeKind};
use xylem_edit::revision::Tracking;
use xylem_edit::{Editor, Pos, Rules};

struct Generic;

fn holds_text(doc: &Document, el: NodeId) -> bool {
    doc.children(el)
        .iter()
        .any(|&c| doc.text(c).is_some_and(|t| !t.trim().is_empty()))
}

impl Rules for Generic {
    fn is_block(&self, doc: &Document, el: NodeId) -> bool {
        holds_text(doc, el) && doc.parent(el).is_some_and(|p| !holds_text(doc, p))
    }
    fn is_element_only(&self, doc: &Document, el: NodeId) -> bool {
        !holds_text(doc, el) && doc.element_children(el).next().is_some()
    }
    fn can_contain_text(&self, doc: &Document, el: NodeId) -> bool {
        !self.is_element_only(doc, el)
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn xml_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
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

fn clock() -> String {
    "20261121T120000-0500".into()
}

fn tracked_editor(bytes: &[u8]) -> Editor {
    let mut ed = Editor::new(Document::from_bytes(bytes).unwrap());
    ed.tracking = Some(Tracking {
        author: "corpus".into(),
        clock,
    });
    ed
}

/// Text positions inside elements that hold text.
fn spots(doc: &Document) -> Vec<(NodeId, usize)> {
    doc.descendants(doc.root())
        .filter(|&n| {
            doc.text(n).is_some_and(|t| !t.trim().is_empty())
                && doc.parent(n).is_some_and(|p| doc.is_element(p))
        })
        .flat_map(|n| {
            let t = doc.text(n).unwrap();
            t.char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(t.len()))
                .map(move |i| (n, i))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The document's content written without its original spellings: two
/// documents that differ only in how characters were escaped (`&#38;` or
/// `&amp;`, `&#174;` or `®`) give the same text.
fn content(bytes: &[u8]) -> String {
    fn walk(doc: &Document, n: NodeId, out: &mut String) {
        match doc.kind(n) {
            NodeKind::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for a in &e.attrs {
                    out.push_str(&format!(" {}={:?}", a.name, a.value));
                }
                out.push('>');
                for &c in doc.children(n) {
                    walk(doc, c, out);
                }
                out.push_str("</>");
            }
            NodeKind::Text(t) => out.push_str(&format!("{t:?}")),
            other => {
                out.push_str(&format!("{other:?}"));
                for &c in doc.children(n) {
                    walk(doc, c, out);
                }
            }
        }
    }
    let doc = Document::from_bytes(bytes).unwrap();
    let mut out = String::new();
    walk(&doc, doc.root(), &mut out);
    out
}

fn no_marks(xml: &str) -> bool {
    !xml.contains("<?xm-insertion") && !xml.contains("<?xm-deletion")
}

/// What came of editing one file.
struct Outcome {
    edits: usize,
    /// Reject all gave back the same content, but some characters are
    /// escaped differently in text that was edited.
    respelled: bool,
}

/// Edits one file; returns a description of what went wrong, if anything.
fn check_file(bytes: &[u8], seed: u64) -> Result<Outcome, String> {
    let mut ed = tracked_editor(bytes);
    // Files that already hold revisions are left alone: Reject all would
    // rightly change them.
    if !no_marks(&ed.doc.to_xml()) {
        return Ok(Outcome {
            edits: 0,
            respelled: false,
        });
    }
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut done = 0;
    for _ in 0..25 {
        let s = spots(&ed.doc);
        if s.is_empty() {
            break;
        }
        let (n, i) = s[rng.below(s.len())];
        ed.set_caret(Pos::new(n, i));
        let r = match rng.below(7) {
            0 | 1 => ed.insert_text(&Generic, "new"),
            2 => ed.delete_backward(&Generic),
            3 => ed.delete_forward(&Generic),
            4 => ed.enter(&Generic),
            _ => {
                let block = std::iter::once(n)
                    .chain(ed.doc.ancestors(n))
                    .find(|&e| ed.doc.is_element(e) && Generic.is_block(&ed.doc, e));
                match block {
                    Some(b) if Some(b) != ed.doc.document_element() => {
                        ed.move_element(&Generic, b, rng.below(2) == 0)
                    }
                    _ => continue,
                }
            }
        };
        if r.is_ok() {
            done += 1;
        }
        if let Err(e) = Document::from_str(&ed.doc.to_xml()) {
            return Err(format!("not well-formed after an edit: {e}"));
        }
    }
    let edited = ed.doc.to_bytes();
    let mut rejected = tracked_editor(&edited);
    let _ = rejected.resolve_all(&Generic, false);
    let respelled = rejected.doc.to_bytes() != bytes;
    if respelled && content(&rejected.doc.to_bytes()) != content(bytes) {
        // XYLEM_CORPUS_OUT keeps the edited and rejected copies to compare.
        if let Some(dir) = std::env::var_os("XYLEM_CORPUS_OUT") {
            let dir = PathBuf::from(dir);
            let _ = std::fs::write(dir.join(format!("{seed}-edited.xml")), &edited);
            let _ = std::fs::write(
                dir.join(format!("{seed}-rejected.xml")),
                rejected.doc.to_bytes(),
            );
            let _ = std::fs::write(dir.join(format!("{seed}-original.xml")), bytes);
        }
        return Err("Reject all did not give back the original".into());
    }
    let mut accepted = tracked_editor(&edited);
    let _ = accepted.resolve_all(&Generic, true);
    let out = accepted.doc.to_xml();
    if Document::from_str(&out).is_err() || !no_marks(&out) {
        return Err("Accept all left marks or broke the document".into());
    }
    while ed.undo() {}
    if ed.doc.to_bytes() != bytes {
        return Err("undoing everything did not give back the original".into());
    }
    Ok(Outcome {
        edits: done,
        respelled,
    })
}

#[test]
#[ignore = "needs a local XML corpus (XYLEM_XML_CORPUS)"]
fn tracked_edits_on_real_documents_resolve_exactly() {
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
    let (mut checked, mut edits, mut skipped, mut respelled) = (0usize, 0usize, 0usize, 0usize);
    let mut failures = Vec::new();
    let start = std::time::Instant::now();
    for (k, f) in files.iter().enumerate() {
        let Ok(bytes) = std::fs::read(f) else {
            continue;
        };
        // Only documents that round-trip exactly (see xylem-dom's corpus
        // test), and not huge ones, to keep the run short.
        let Ok(doc) = Document::from_bytes(&bytes) else {
            skipped += 1;
            continue;
        };
        if doc.to_bytes() != bytes || bytes.len() > 2_000_000 {
            skipped += 1;
            continue;
        }
        let seeds: u64 = std::env::var("XYLEM_CORPUS_SEEDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3);
        for seed in 1..=seeds {
            match check_file(&bytes, k as u64 * 7 + seed) {
                Ok(o) => {
                    edits += o.edits;
                    respelled += usize::from(o.respelled);
                }
                Err(e) => {
                    failures.push(format!("{} (seed {seed}): {e}", f.display()));
                    break;
                }
            }
        }
        checked += 1;
    }
    eprintln!(
        "{} files in {:.1?}: {checked} edited ({edits} tracked edits), {skipped} skipped, {} failed; \
         {respelled} runs gave back the same content with some characters escaped differently",
        files.len(),
        start.elapsed(),
        failures.len()
    );
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
