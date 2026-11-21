//! Randomised editing: long runs of typing, deleting, splitting and
//! selection replacement at random places, checking after every step that
//! the document is still well-formed, and at the end that undo restores the
//! original bytes exactly and redo brings the edits back.
//!
//! The runs are seeded, so a failure names a seed that reproduces it.
//! `XYLEM_FUZZ_RUNS` raises the number of runs for a longer soak.

use super::*;
use crate::pos::Pos;

/// Small, fast, reproducible random numbers (xorshift64*).
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

struct Rules;
impl crate::Rules for Rules {
    fn is_block(&self, doc: &Document, el: NodeId) -> bool {
        matches!(
            doc.name(el),
            Some("para" | "title" | "entry" | "item" | "warning")
        )
    }
    fn is_element_only(&self, doc: &Document, el: NodeId) -> bool {
        matches!(
            doc.name(el),
            Some("manual" | "chapter" | "list" | "table" | "tgroup" | "tbody" | "row")
        )
    }
    fn can_contain_text(&self, doc: &Document, el: NodeId) -> bool {
        !self.is_element_only(doc, el)
    }
    fn followed_by(&self, name: &str) -> Option<String> {
        (name == "title").then(|| "para".to_string())
    }
}

const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- A comment before the root -->
<manual id="m1">
  <title>Hydraulic &amp; lift <b>manual</b></title>
  <chapter id="c1">
    <title>Safety</title>
    <para>Never stand under a <i>raised</i> load. Check &#169; and &lt;tags&gt;.</para>
    <warning><para>Hot surfaces — wait 10 min.</para></warning>
    <list>
      <item><para>Inspect the lines.</para></item>
      <item><para>Check the <b>fluid</b> level.</para></item>
    </list>
    <?xm-replace_text Type here?>
    <table><tgroup cols="2"><tbody>
      <row><entry>Hoses</entry><entry>Daily</entry></row>
      <row><entry>Seals</entry><entry>Monthly</entry></row>
    </tbody></tgroup></table>
  </chapter>
</manual>
"#;

const WORDS: &[&str] = &[
    "a", "lift", " ", "Grüße", "&", "<x>", "\"q\"", "→", "1.5", "\t", "é",
];

fn text_positions(doc: &Document) -> Vec<(NodeId, usize)> {
    doc.descendants(doc.root())
        .filter_map(|n| doc.text(n).map(|t| (n, t)))
        .filter(|(n, _)| doc.parent(*n).is_some_and(|p| doc.is_element(p)))
        .flat_map(|(n, t)| {
            t.char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(t.len()))
                .map(move |i| (n, i))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn clock() -> String {
    "20261121T120000+0000".into()
}

fn no_marks(xml: &str) -> bool {
    !xml.contains("<?xm-insertion") && !xml.contains("<?xm-deletion")
}

fn run(seed: u64, steps: usize, tracking: bool) {
    let original = Document::from_str(SAMPLE)
        .expect("sample parses")
        .to_bytes();
    let mut ed = Editor::new(Document::from_str(SAMPLE).unwrap());
    if tracking {
        ed.tracking = Some(crate::revision::Tracking {
            author: "fuzz".into(),
            clock,
        });
    }
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut done = 0usize;
    for step in 0..steps {
        let spots = text_positions(&ed.doc);
        if spots.is_empty() {
            break;
        }
        let (n, i) = spots[rng.below(spots.len())];
        let caret = Pos::new(n, i);
        // Edits are separate undo steps, so undo can be counted exactly.
        ed.set_caret(caret);
        // Deleting a whole element is rarer than the other edits.
        let op = match rng.below(14) {
            13 => 5,
            k => [0, 1, 2, 3, 4, 6][k % 6],
        };
        let result = match op {
            0 | 1 => ed.insert_text(&Rules, WORDS[rng.below(WORDS.len())]),
            2 => ed.delete_backward(&Rules),
            3 => ed.delete_forward(&Rules),
            4 => ed.enter(&Rules),
            5 => {
                // Delete the element around the caret (never the root).
                match ed
                    .doc
                    .parent(n)
                    .filter(|&e| Some(e) != ed.doc.document_element())
                {
                    Some(e) => ed.delete_element(&Rules, e),
                    None => Err(EditError::Nothing),
                }
            }
            _ => {
                // Replace a selection inside one text node.
                let len = ed.doc.text(n).map_or(0, str::len);
                let mut j = (i + 1 + rng.below(8)).min(len);
                while j < len && !ed.doc.text(n).unwrap().is_char_boundary(j) {
                    j += 1;
                }
                ed.sel = Selection {
                    anchor: caret,
                    focus: Pos::new(n, j),
                };
                ed.insert_text(&Rules, "X")
            }
        };
        if result.is_ok() {
            done += 1;
        }
        let xml = ed.doc.to_xml();
        if let Err(e) = Document::from_str(&xml) {
            panic!("seed {seed}, step {step}, op {op}: not well-formed ({e}):\n{xml}");
        }
        if tracking {
            let starts = xml.matches("<?xm-insertion_mark_start").count();
            let ends = xml.matches("<?xm-insertion_mark_end").count();
            assert_eq!(
                starts, ends,
                "seed {seed}, step {step}, op {op}: unpaired insertion marks:\n{xml}"
            );
        }
    }
    // Guard against a test that silently does nothing.
    assert!(
        done >= 8,
        "seed {seed}: only {done} of {steps} edits applied"
    );
    assert_ne!(
        ed.doc.to_bytes(),
        original,
        "seed {seed}: the edits changed nothing"
    );
    let edited = ed.doc.to_bytes();
    let mut undone = 0;
    while ed.undo() {
        undone += 1;
        assert!(
            undone <= done + 1,
            "seed {seed}: more undo steps than edits"
        );
    }
    assert_eq!(
        String::from_utf8_lossy(&ed.doc.to_bytes()),
        String::from_utf8_lossy(&original),
        "seed {seed}: undoing everything must give back the original bytes"
    );
    while ed.redo() {}
    assert_eq!(
        String::from_utf8_lossy(&ed.doc.to_bytes()),
        String::from_utf8_lossy(&edited),
        "seed {seed}: redoing everything must give back the edited document"
    );
    if tracking {
        // Accept all and reject all each leave a well-formed document
        // with no revision marks.
        for accept in [true, false] {
            let mut e = Editor::new(Document::from_bytes(&edited).unwrap());
            let before = crate::revision::changes(&e.doc).len();
            let marks_before = e.doc.to_xml().matches("<?xm-").count();
            let result = e.resolve_all(&Rules, accept);
            let result = format!("{result:?} ({before} changes, {marks_before} marks before)");
            let out = e.doc.to_xml();
            assert!(
                Document::from_str(&out).is_ok() && no_marks(&out),
                "seed {seed}: resolve_all({accept}) gave {result:?} and left marks or broke the document:\n{out}"
            );
        }
    }
}

fn runs() -> u64 {
    std::env::var("XYLEM_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150)
}

#[test]
fn random_edits_stay_well_formed_and_undo_exactly() {
    for seed in 1..=runs() {
        run(seed, 60, false);
    }
}

#[test]
fn random_tracked_edits_keep_marks_paired_and_resolve_cleanly() {
    for seed in 1..=runs() {
        run(seed, 60, true);
    }
}
