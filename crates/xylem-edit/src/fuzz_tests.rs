//! Randomised editing: long runs of typing, deleting, splitting, renaming,
//! moving and selection replacement at random places, checking after every step that
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

/// The text with numeric character references written as the characters:
/// edited text is written out afresh, so `&#169;` may come back as `©`.
pub(crate) fn same_chars(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
    while let Some(i) = rest.find("&#") {
        out.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        let end = tail.find(';').unwrap_or(0);
        let code = match tail.strip_prefix('x') {
            Some(h) if end > 0 => u32::from_str_radix(&h[..end - 1], 16).ok(),
            _ => tail[..end].parse().ok(),
        };
        match code.and_then(char::from_u32) {
            Some(c) if !matches!(c, '<' | '>' | '&' | '"' | '\'' | '\n' | '\r' | '\t') => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            _ => {
                out.push_str("&#");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn no_marks(xml: &str) -> bool {
    !xml.contains("<?xm-insertion") && !xml.contains("<?xm-deletion")
}

/// How a run tracks changes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    /// Track changes off throughout.
    Off,
    /// One author, tracking on throughout.
    Tracked,
    /// Two authors taking turns at random, tracking on throughout.
    TwoAuthors,
    /// Two authors, with tracking switched off for some edits.
    Mixed,
}

fn tracking_as(author: &str) -> Option<crate::revision::Tracking> {
    Some(crate::revision::Tracking {
        author: author.into(),
        clock,
    })
}

/// Insertion starts and ends alternate in document order.
fn marks_paired(doc: &Document) -> bool {
    let mut open = false;
    let mut ok = true;
    for n in doc.descendants(doc.root()) {
        match crate::revision::mark_kind(doc, n) {
            Some(crate::revision::MarkKind::InsertionStart) => {
                ok &= !open;
                open = true;
            }
            Some(crate::revision::MarkKind::InsertionEnd) => {
                ok &= open;
                open = false;
            }
            _ => {}
        }
    }
    ok && !open
}

fn run(seed: u64, steps: usize, mode: Mode) {
    let original = Document::from_str(SAMPLE)
        .expect("sample parses")
        .to_bytes();
    let mut ed = Editor::new(Document::from_str(SAMPLE).unwrap());
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut done = 0usize;
    for step in 0..steps {
        ed.tracking = match mode {
            Mode::Off => None,
            Mode::Tracked => tracking_as("fuzz"),
            Mode::TwoAuthors => tracking_as(["fuzz", "ann"][rng.below(2)]),
            Mode::Mixed => match rng.below(5) {
                0 => None,
                k => tracking_as(["fuzz", "ann"][k % 2]),
            },
        };
        let spots = text_positions(&ed.doc);
        if spots.is_empty() {
            break;
        }
        let (n, i) = spots[rng.below(spots.len())];
        let caret = Pos::new(n, i);
        let before = format!(
            "{}\ncaret in {:?} at {i}, as {:?}",
            ed.doc.to_xml(),
            ed.doc.text(n),
            ed.tracking.as_ref().map(|t| t.author.clone())
        );
        // Edits are separate undo steps, so undo can be counted exactly.
        ed.set_caret(caret);
        // Deleting, renaming and moving whole elements are rarer.
        let op = match rng.below(16) {
            13 => 5,
            14 => 7,
            15 => 8,
            k => [0, 1, 2, 3, 4, 6][k % 6],
        };
        let around = ed
            .doc
            .parent(n)
            .filter(|&e| Some(e) != ed.doc.document_element());
        let result = match op {
            0 | 1 => ed.insert_text(&Rules, WORDS[rng.below(WORDS.len())]),
            2 => ed.delete_backward(&Rules),
            3 => ed.delete_forward(&Rules),
            4 => ed.enter(&Rules),
            5 => match around {
                Some(e) => ed.delete_element(&Rules, e),
                None => Err(EditError::Nothing),
            },
            7 => {
                let to = around.and_then(|e| match ed.doc.name(e) {
                    Some("para") => Some((e, "note")),
                    Some("note") => Some((e, "para")),
                    Some("b") => Some((e, "i")),
                    Some("i") => Some((e, "b")),
                    _ => None,
                });
                match to {
                    Some((e, name)) => ed.rename_element(&Rules, e, name),
                    None => Err(EditError::Nothing),
                }
            }
            8 => {
                let up = rng.below(2) == 0;
                let block = std::iter::once(n)
                    .chain(ed.doc.ancestors(n))
                    .find(|&e| ed.doc.is_element(e) && crate::Rules::is_block(&Rules, &ed.doc, e));
                match block {
                    Some(b) => ed.move_element(&Rules, b, up),
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
        assert!(
            marks_paired(&ed.doc),
            "seed {seed} ({mode:?}), step {step}, op {op}: unpaired insertion marks:\n{xml}\nbefore:\n{before}"
        );
        // XYLEM_FUZZ_TRACE: check Reject all after every step, to find the
        // edit that first breaks it.
        if matches!(mode, Mode::Tracked | Mode::TwoAuthors)
            && std::env::var_os("XYLEM_FUZZ_TRACE").is_some()
        {
            let mut e = Editor::new(Document::from_bytes(&ed.doc.to_bytes()).unwrap());
            let _ = e.resolve_all(&Rules, false);
            assert_eq!(
                same_chars(&String::from_utf8_lossy(&e.doc.to_bytes())),
                same_chars(&String::from_utf8_lossy(&original)),
                "seed {seed} ({mode:?}), step {step}, op {op}: Reject all breaks here.\nafter:\n{xml}\nbefore:\n{before}"
            );
        }
    }
    // Guard against a test that silently does nothing.
    assert!(
        done >= 5,
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
        "seed {seed} ({mode:?}): undoing everything must give back the original bytes"
    );
    while ed.redo() {}
    assert_eq!(
        String::from_utf8_lossy(&ed.doc.to_bytes()),
        String::from_utf8_lossy(&edited),
        "seed {seed} ({mode:?}): redoing everything must give back the edited document"
    );
    if mode == Mode::Off {
        return;
    }
    let edited_text = String::from_utf8_lossy(&edited).into_owned();
    // Accept all and Reject all each leave a well-formed document with no
    // revision marks. When every edit was tracked, Reject all gives back the
    // original.
    for accept in [true, false] {
        let mut e = Editor::new(Document::from_bytes(&edited).unwrap());
        let result = e.resolve_all(&Rules, accept);
        let out = e.doc.to_xml();
        if !accept && mode != Mode::Mixed {
            assert_eq!(
                same_chars(&String::from_utf8_lossy(&e.doc.to_bytes())),
                same_chars(&String::from_utf8_lossy(&original)),
                "seed {seed} ({mode:?}): Reject all must give back the original exactly; edited:\n{edited_text}"
            );
        }
        assert!(
            Document::from_str(&out).is_ok() && no_marks(&out),
            "seed {seed} ({mode:?}): resolve_all({accept}) gave {result:?} and left marks or broke the document:\n{out}\nfrom:\n{edited_text}"
        );
    }
    // One change at a time, in random order, each accepted or rejected at
    // random (as in the Review panel): the document stays well-formed and
    // paired, and in the end no change is left.
    let mut e = Editor::new(Document::from_bytes(&edited).unwrap());
    for k in 0..2000 {
        let open: Vec<NodeId> = crate::revision::changes(&e.doc)
            .into_iter()
            .filter(|c| c.kind != crate::revision::ChangeKind::Comment)
            .map(|c| c.node)
            .collect();
        if open.is_empty() {
            break;
        }
        let node = open[rng.below(open.len())];
        let accept = rng.below(2) == 0;
        let what = e.doc.node_to_xml(node);
        let before = e.doc.to_xml();
        let _ = if accept {
            e.accept_change(&Rules, node)
        } else {
            e.reject_change(&Rules, node)
        };
        let out = e.doc.to_xml();
        assert!(
            Document::from_str(&out).is_ok() && marks_paired(&e.doc),
            "seed {seed} ({mode:?}): resolving {what} (accept: {accept}, change {k}) broke the document:\n{out}\nbefore:\n{before}"
        );
    }
    let out = e.doc.to_xml();
    assert!(
        no_marks(&out),
        "seed {seed} ({mode:?}): changes left after resolving one at a time:\n{out}\nfrom:\n{edited_text}"
    );
}

/// Seeds that once found a bug; always run.
const FOUND: &[u64] = &[
    34, 138, 144, 363, 394, 726, 3107, 3557, 4245, 5626, 6642, 60609, 65787, 84102,
];

/// The seeds to run: `XYLEM_FUZZ_SEED` picks one, else 1 to
/// `XYLEM_FUZZ_RUNS` (150 by default) and the seeds in [`FOUND`].
fn seeds() -> Vec<u64> {
    if let Some(s) = std::env::var("XYLEM_FUZZ_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        return vec![s];
    }
    let n = runs();
    (1..=n)
        .chain(FOUND.iter().copied().filter(|&s| s > n))
        .collect()
}

fn runs() -> u64 {
    std::env::var("XYLEM_FUZZ_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150)
}

#[test]
fn random_edits_stay_well_formed_and_undo_exactly() {
    for seed in seeds() {
        run(seed, 60, Mode::Off);
    }
}

#[test]
fn random_tracked_edits_keep_marks_paired_and_resolve_cleanly() {
    for seed in seeds() {
        run(seed, 60, Mode::Tracked);
    }
}

#[test]
fn two_authors_editing_resolve_cleanly() {
    for seed in seeds() {
        run(seed, 60, Mode::TwoAuthors);
    }
}

#[test]
fn switching_tracking_off_keeps_marks_paired() {
    for seed in seeds() {
        run(seed, 60, Mode::Mixed);
    }
}
