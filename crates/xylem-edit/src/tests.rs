use super::*;
use crate::pos::*;

/// Rules treating `p`, `title`, `entry` and `li` as blocks.
struct TestRules;
impl Rules for TestRules {
    fn is_block(&self, doc: &Document, el: NodeId) -> bool {
        matches!(
            doc.name(el),
            Some("p" | "title" | "entry" | "li" | "section")
        )
    }
    fn is_element_only(&self, doc: &Document, el: NodeId) -> bool {
        matches!(doc.name(el), Some("doc" | "section" | "list"))
    }
    fn can_contain_text(&self, doc: &Document, el: NodeId) -> bool {
        !self.is_element_only(doc, el)
    }
    fn followed_by(&self, name: &str) -> Option<String> {
        (name == "title").then(|| "p".to_string())
    }
}

fn ed(xml: &str) -> Editor {
    Editor::new(Document::from_str(xml).unwrap())
}

fn text_pos(e: &Editor, needle: &str, at: usize) -> Pos {
    for n in e.doc.descendants(e.doc.root()) {
        if let Some(t) = e.doc.text(n)
            && let Some(i) = t.find(needle)
        {
            return Pos::new(n, i + at);
        }
    }
    panic!("{needle} not found")
}

fn xml(e: &Editor) -> String {
    e.doc.to_xml()
}

#[test]
fn typing_and_undo_coalesce() {
    let mut e = ed("<doc><p>ab</p></doc>");
    e.set_caret(text_pos(&e, "ab", 1));
    for c in ["x", "y", "z"] {
        e.insert_text(&TestRules, c).unwrap();
    }
    assert_eq!(xml(&e), "<doc><p>axyzb</p></doc>");
    assert!(e.undo());
    assert_eq!(xml(&e), "<doc><p>ab</p></doc>");
    assert!(!e.can_undo());
    assert!(e.redo());
    assert_eq!(xml(&e), "<doc><p>axyzb</p></doc>");
}

#[test]
fn caret_walks_through_tags() {
    let e = ed("<doc><p>a<b>c</b>d</p></doc>");
    let r = &TestRules;
    let mut p = text_pos(&e, "a", 0);
    let mut stops = vec![p];
    for _ in 0..10 {
        let n = next_pos(&e.doc, r, p, false);
        if n == p {
            break;
        }
        stops.push(n);
        p = n;
    }
    // a| <b> |c| </b> |d|  ... out of p into doc
    assert!(stops.len() >= 6, "{stops:?}");
    // Normal-view movement skips tag-only stops.
    let s = text_pos(&e, "a", 1);
    let n = next_pos(&e.doc, r, s, true);
    assert_eq!(e.doc.text(n.node), Some("c"));
    assert_eq!(n.offset, 1);
}

#[test]
fn enter_splits_and_followed_by() {
    let mut e = ed("<doc><title>Head</title><p>hello world</p></doc>");
    e.set_caret(text_pos(&e, "world", 0));
    e.enter(&TestRules).unwrap();
    assert_eq!(
        xml(&e),
        "<doc><title>Head</title><p>hello </p><p>world</p></doc>"
    );
    e.set_caret(text_pos(&e, "Head", 4));
    e.enter(&TestRules).unwrap();
    assert_eq!(
        xml(&e),
        "<doc><title>Head</title><p/><p>hello </p><p>world</p></doc>"
    );
    e.insert_text(&TestRules, "new").unwrap();
    assert!(xml(&e).contains("<p>new</p>"));
}

#[test]
fn split_through_inline() {
    let mut e = ed("<doc><p>a<b id=\"x\" c=\"1\">bold text</b>z</p></doc>");
    e.set_caret(text_pos(&e, "text", 0));
    e.enter(&TestRules).unwrap();
    assert_eq!(
        xml(&e),
        "<doc><p>a<b id=\"x\" c=\"1\">bold </b></p><p><b c=\"1\">text</b>z</p></doc>"
    );
}

#[test]
fn backspace_joins_blocks_and_removes_empty_inline() {
    let mut e = ed("<doc><p>one</p><p>two</p></doc>");
    e.set_caret(text_pos(&e, "two", 0));
    e.delete_backward(&TestRules).unwrap(); // moves before <p> start -> join
    assert_eq!(xml(&e), "<doc><p>onetwo</p></doc>");
    let mut e = ed("<doc><p>a<b>x</b>c</p></doc>");
    e.set_caret(text_pos(&e, "c", 0));
    e.delete_backward(&TestRules).unwrap();
    assert_eq!(xml(&e), "<doc><p>ac</p></doc>");
    e.insert_text(&TestRules, "Q").unwrap();
    assert_eq!(xml(&e), "<doc><p>aQc</p></doc>");
}

#[test]
fn delete_range_across_elements() {
    let mut e = ed("<doc><p>hello <b>big</b> world</p><p>second para</p></doc>");
    let a = text_pos(&e, "hello", 2);
    let b = text_pos(&e, "second", 3);
    e.sel = Selection {
        anchor: a,
        focus: b,
    };
    e.delete_selection(&TestRules).unwrap();
    assert_eq!(xml(&e), "<doc><p>he</p><p>ond para</p></doc>");
    e.insert_text(&TestRules, "X").unwrap();
    assert_eq!(xml(&e), "<doc><p>heX</p><p>ond para</p></doc>");
}

#[test]
fn surround_and_remove_tags() {
    let mut e = ed("<doc><p>make this bold</p></doc>");
    let a = text_pos(&e, "this", 0);
    let b = text_pos(&e, "this", 4);
    e.sel = Selection {
        anchor: a,
        focus: b,
    };
    let w = e.surround(&TestRules, "b", &|_, _, _, _| true).unwrap();
    assert_eq!(xml(&e), "<doc><p>make <b>this</b> bold</p></doc>");
    e.remove_tags(&TestRules, w).unwrap();
    assert_eq!(xml(&e), "<doc><p>make this bold</p></doc>");
    assert_eq!(
        e.doc
            .children(
                e.doc
                    .element_children(e.doc.document_element().unwrap())
                    .next()
                    .unwrap()
            )
            .len(),
        1
    );
}

#[test]
fn copy_and_paste_fragment() {
    let mut e = ed("<doc><p>ab<i>cd</i>ef</p><p>X</p></doc>");
    e.sel = Selection {
        anchor: text_pos(&e, "ab", 1),
        focus: text_pos(&e, "cd", 1),
    };
    let frag = e.selection_xml().unwrap();
    assert_eq!(frag, "b<i>c</i>");
    e.set_caret(text_pos(&e, "X", 1));
    e.paste(&TestRules, &frag, true).unwrap();
    assert_eq!(xml(&e), "<doc><p>ab<i>cd</i>ef</p><p>Xb<i>c</i></p></doc>");
}

#[test]
fn insert_element_and_text_rules() {
    let mut e = ed("<doc><section><p>x</p></section></doc>");
    let sec = e
        .doc
        .element_children(e.doc.document_element().unwrap())
        .next()
        .unwrap();
    // Typing between blocks of element-only content goes into the neighbour.
    e.set_caret(Pos::new(sec, 1));
    e.insert_text(&TestRules, "!").unwrap();
    assert_eq!(xml(&e), "<doc><section><p>x!</p></section></doc>");
    e.undo();
    e.set_caret(Pos::new(sec, 1));
    e.insert_element(&TestRules, "p").unwrap();
    e.insert_text(&TestRules, "y").unwrap();
    assert_eq!(xml(&e), "<doc><section><p>x</p><p>y</p></section></doc>");
}

#[test]
fn placeholder_pi_is_replaced_by_typing() {
    let mut e = ed("<doc><p><?xm-replace_text Type here?></p></doc>");
    let p = e
        .doc
        .element_children(e.doc.document_element().unwrap())
        .next()
        .unwrap();
    e.set_caret(Pos::new(p, 0));
    e.insert_text(&TestRules, "hi").unwrap();
    assert_eq!(xml(&e), "<doc><p>hi</p></doc>");
}

#[test]
fn cals_table_ops() {
    let xml0 = table::cals_table_xml(2, 2, false, None);
    let mut e = ed(&format!("<doc>{xml0}</doc>"));
    let entry = e
        .doc
        .descendants(e.doc.root())
        .find(|&n| e.doc.name(n) == Some("entry"))
        .unwrap();
    e.set_caret(Pos::new(entry, 0));
    e.insert_text(&TestRules, "A").unwrap();
    e.table_insert_column(&TestRules, true).unwrap();
    let out = xml(&e);
    assert!(out.contains("cols=\"3\""), "{out}");
    assert_eq!(out.matches("<colspec").count(), 3);
    assert_eq!(out.matches("<entry").count(), 6);
    e.table_insert_rows(&TestRules, 1, true).unwrap();
    assert_eq!(xml(&e).matches("<row>").count(), 3);
    e.table_delete_row(&TestRules).unwrap();
    assert_eq!(xml(&e).matches("<row>").count(), 2);
    let first = e
        .doc
        .descendants(e.doc.root())
        .find(|&n| e.doc.name(n) == Some("entry"))
        .unwrap();
    e.set_caret(first_editable(&e.doc, &TestRules, first));
    e.table_merge_right(&TestRules).unwrap();
    assert!(
        xml(&e).contains("namest=\"col1\" nameend=\"col2\""),
        "{}",
        xml(&e)
    );
    e.table_split_cell(&TestRules).unwrap();
    assert!(!xml(&e).contains("namest"));
    e.table_delete_column(&TestRules).unwrap();
    assert!(xml(&e).contains("cols=\"2\""));
}

#[test]
fn ordering_of_positions() {
    let e = ed("<doc><p>ab</p><p>cd</p></doc>");
    let a = text_pos(&e, "ab", 1);
    let b = text_pos(&e, "cd", 0);
    assert_eq!(cmp_pos(&e.doc, a, b), Ordering::Less);
    let root = e.doc.document_element().unwrap();
    assert_eq!(cmp_pos(&e.doc, Pos::new(root, 1), a), Ordering::Greater);
    assert_eq!(cmp_pos(&e.doc, Pos::new(root, 0), a), Ordering::Less);
}

#[test]
fn dirty_flag_tracks_states_not_revisions() {
    let mut e = ed("<doc><p>ab</p></doc>");
    assert!(!e.is_dirty());
    e.set_caret(text_pos(&e, "ab", 2));
    e.insert_text(&TestRules, "x").unwrap();
    assert!(e.is_dirty());
    e.mark_saved();
    assert!(!e.is_dirty());
    e.undo();
    assert!(e.is_dirty(), "undo past the save point is a change");
    e.insert_text(&TestRules, "y").unwrap();
    assert!(e.is_dirty(), "different edit after undo must stay dirty");
    e.undo();
    e.redo();
    assert!(e.is_dirty());
    // Undo back to the saved state is clean again.
    let mut e = ed("<doc><p>ab</p></doc>");
    e.set_caret(text_pos(&e, "ab", 2));
    e.insert_text(&TestRules, "x").unwrap();
    e.mark_saved();
    e.insert_text(&TestRules, "z").unwrap();
    assert!(e.is_dirty());
    e.undo();
    assert!(!e.is_dirty(), "{}", xml(&e));
}

#[test]
fn journal_undo_redo_is_exact() {
    let src = "<doc>\n  <p id=\"a\">one <b>two</b> three</p>\n  <p>four</p>\n</doc>";
    let mut e = ed(src);
    let a = text_pos(&e, "one", 2);
    let b = text_pos(&e, "four", 2);
    e.sel = Selection {
        anchor: a,
        focus: b,
    };
    e.delete_selection(&TestRules).unwrap();
    e.enter(&TestRules).unwrap();
    e.insert_text(&TestRules, "Q").unwrap();
    let w = e.doc.document_element().unwrap();
    e.set_attribute(&TestRules, w, "v", "1").unwrap();
    let after = xml(&e);
    while e.undo() {}
    assert_eq!(xml(&e), src);
    while e.redo() {}
    assert_eq!(xml(&e), after);
}

#[test]
fn failed_edit_rolls_back_without_snapshot() {
    // Nowhere nearby can hold text, so typing fails and nothing changes.
    let mut e = ed("<doc><section><list/></section></doc>");
    let sec = e
        .doc
        .element_children(e.doc.document_element().unwrap())
        .next()
        .unwrap();
    e.set_caret(Pos::new(sec, 0));
    assert!(e.insert_text(&TestRules, "bad").is_err());
    assert_eq!(xml(&e), "<doc><section><list/></section></doc>");
    assert!(!e.can_undo());
    assert!(!e.is_dirty());
}

#[test]
fn read_only_elements_reject_edits() {
    struct Ro;
    impl Rules for Ro {
        fn is_read_only(&self, doc: &Document, el: NodeId) -> bool {
            doc.name(el) == Some("locked")
        }
        fn is_block(&self, doc: &Document, el: NodeId) -> bool {
            matches!(doc.name(el), Some("p" | "locked"))
        }
    }
    let mut e = ed("<doc><locked>fixed</locked><p>free</p></doc>");
    e.set_caret(text_pos(&e, "fixed", 2));
    assert_eq!(e.insert_text(&Ro, "x"), Err(EditError::ReadOnly));
    assert_eq!(e.delete_backward(&Ro), Err(EditError::ReadOnly));
    e.sel = Selection {
        anchor: text_pos(&e, "fixed", 1),
        focus: text_pos(&e, "free", 2),
    };
    assert_eq!(e.delete_selection(&Ro), Err(EditError::ReadOnly));
    assert_eq!(xml(&e), "<doc><locked>fixed</locked><p>free</p></doc>");
    e.set_caret(text_pos(&e, "free", 4));
    e.insert_text(&Ro, "!").unwrap();
    assert_eq!(xml(&e), "<doc><locked>fixed</locked><p>free!</p></doc>");
}

#[test]
fn protected_markers_survive_deletes_but_not_parent_removal() {
    struct Prot;
    impl Rules for Prot {
        fn is_protected(&self, doc: &Document, n: NodeId) -> bool {
            matches!(doc.kind(n), NodeKind::Pi { target, .. } if target == "Ref_Label")
        }
        fn is_block(&self, doc: &Document, el: NodeId) -> bool {
            doc.name(el) == Some("p")
        }
    }
    let src = "<doc><p>see <?Ref_Label Fig 1?> above</p><p>next</p></doc>";
    let mut e = ed(src);
    // Backspace right after the marker.
    e.set_caret(text_pos(&e, " above", 0));
    assert_eq!(e.delete_backward(&Prot), Err(EditError::Protected));
    // Delete right before it.
    e.set_caret(text_pos(&e, "see ", 4));
    assert_eq!(e.delete_forward(&Prot), Err(EditError::Protected));
    // A selection that covers it.
    e.sel = Selection {
        anchor: text_pos(&e, "see", 1),
        focus: text_pos(&e, "above", 2),
    };
    assert_eq!(e.delete_selection(&Prot), Err(EditError::Protected));
    assert_eq!(xml(&e), src);
    // Deleting the whole paragraph is allowed.
    let p = e
        .doc
        .element_children(e.doc.document_element().unwrap())
        .next()
        .unwrap();
    e.delete_element(&Prot, p).unwrap();
    assert_eq!(xml(&e), "<doc><p>next</p></doc>");
}

#[test]
fn reformat_only_touches_formatting_whitespace() {
    let src = "<doc><section><p>a <b>b</b> c</p><p>d</p></section><pre>  keep
  this</pre></doc>";
    let mut e = ed(src);
    let opts = format::FormatOptions {
        indent: 2,
        flat: &|_| false,
    };
    e.reformat(&TestRules, &opts).unwrap();
    assert_eq!(
        xml(&e),
        "<doc>
  <section>
    <p>a <b>b</b> c</p>
    <p>d</p>
  </section>
  <pre>  keep
  this</pre>
</doc>"
    );
    // Formatting twice is stable, and one undo restores the original.
    let once = xml(&e);
    e.reformat(&TestRules, &opts).unwrap();
    assert_eq!(xml(&e), once);
    e.undo();
    e.undo();
    assert_eq!(xml(&e), src);
}

#[test]
fn moving_elements_keeps_whitespace_layout() {
    let mut e = ed("<doc>
  <p>one</p>
  <p>two</p>
</doc>");
    let second = e
        .doc
        .element_children(e.doc.document_element().unwrap())
        .nth(1)
        .unwrap();
    e.move_element(&TestRules, second, true).unwrap();
    assert_eq!(
        xml(&e),
        "<doc>
  <p>two</p>
  <p>one</p>
</doc>"
    );
    e.move_element(&TestRules, second, false).unwrap();
    assert_eq!(
        xml(&e),
        "<doc>
  <p>one</p>
  <p>two</p>
</doc>"
    );
}
