use super::*;
use crate::pos::{next_pos, prev_pos};
use crate::{Editor, NoRules};

/// `p` and `title` are blocks; `doc` and `section` hold only elements.
struct R;
impl Rules for R {
    fn is_block(&self, doc: &Document, el: NodeId) -> bool {
        matches!(doc.name(el), Some("p" | "title"))
    }
    fn is_element_only(&self, doc: &Document, el: NodeId) -> bool {
        matches!(doc.name(el), Some("doc" | "section"))
    }
    fn can_contain_text(&self, doc: &Document, el: NodeId) -> bool {
        !self.is_element_only(doc, el)
    }
}

const T: &str = "20261121T101500+0000";
const INS: &str = r#"<?xm-insertion_mark_start author="me" time="20261121T101500+0000"?>"#;
const END: &str = "<?xm-insertion_mark_end?>";

fn fixed_clock() -> String {
    T.to_string()
}

fn tracked(xml: &str) -> Editor {
    let mut e = Editor::new(Document::from_str(xml).unwrap());
    e.tracking = Some(Tracking {
        author: "me".into(),
        clock: fixed_clock,
    });
    e
}

fn del(data: &str) -> String {
    format!(r#"<?xm-deletion_mark author="me" time="{T}" data="{data}"?>"#)
}

fn at(e: &Editor, needle: &str, off: usize) -> Pos {
    for n in e.doc.descendants(e.doc.root()) {
        if let Some(t) = e.doc.text(n)
            && let Some(i) = t.find(needle)
        {
            return Pos::new(n, i + off);
        }
    }
    panic!("{needle} not found in {}", e.doc.to_xml())
}

fn type_str(e: &mut Editor, s: &str) {
    for c in s.chars() {
        e.insert_text(&R, &c.to_string()).unwrap();
    }
}

fn xml(e: &Editor) -> String {
    e.doc.to_xml()
}

/// No edit may leave half an insertion or comment behind.
fn assert_balanced(e: &Editor) {
    for n in e.doc.descendants(e.doc.root()) {
        if mark_kind(&e.doc, n).is_some_and(MarkKind::is_paired) {
            assert!(partner(&e.doc, n).is_some(), "unmatched mark in {}", xml(e));
        }
    }
}

#[test]
fn pseudo_attributes_round_trip() {
    let a = PiAttrs::parse(
        r#"author="Ann O'Hara" time='2026' data="a &lt;b&gt; &amp; &quot;c&quot;&#10;d" x="1""#,
    );
    assert_eq!(a.get("author"), Some("Ann O'Hara"));
    assert_eq!(a.get("time"), Some("2026"));
    assert_eq!(a.get("data"), Some("a <b> & \"c\"\nd"));
    assert_eq!(a.get("x"), Some("1"));
    let back = PiAttrs::parse(&a.to_data());
    assert_eq!(back, a);
    assert!(!a.to_data().contains('>'), "data must never close the PI");
}

#[test]
fn time_stamps() {
    assert_eq!(format_time(0, 0), "19700101T000000+0000");
    assert_eq!(format_time(951_782_400, 0), "20000229T000000+0000");
    assert_eq!(format_time(0, -300), "19691231T190000-0500");
    assert_eq!(format_time(3_600 + 61, 90), "19700101T023101+0130");
    assert_eq!(display_time("20261121T101500+0000"), "2026-11-21 10:15");
    assert_eq!(display_time("yesterday"), "yesterday");
}

#[test]
fn untouched_marks_round_trip_byte_for_byte() {
    let src = r#"<doc><p>a<?xm-insertion_mark_start author='x' time="1"?>b<?xm-insertion_mark_end?><?xm-deletion_mark  author="x" time="1" data="c&amp;lt;"?><?xm-annotation author="x" time="1" data="hi"?><?xm-annotation_start author="x" time="1" data="r"?>d<?xm-annotation_end?></p></doc>"#;
    let doc = Document::from_str(src).unwrap();
    assert_eq!(doc.to_xml(), src);
    // An edit elsewhere leaves them alone too.
    let mut e = Editor::new(doc);
    e.set_caret(at(&e, "a", 0));
    e.insert_text(&R, "Z").unwrap();
    assert_eq!(xml(&e), src.replace("<p>a", "<p>Za"));
}

#[test]
fn typing_makes_one_insertion_and_undoes_in_one_step() {
    let mut e = tracked("<doc><p>ab</p></doc>");
    e.set_caret(at(&e, "ab", 1));
    type_str(&mut e, "xyz");
    assert_eq!(xml(&e), format!("<doc><p>a{INS}xyz{END}b</p></doc>"));
    assert!(e.undo());
    assert_eq!(xml(&e), "<doc><p>ab</p></doc>");
    assert!(!e.can_undo());
    assert!(e.redo());
    assert_eq!(xml(&e), format!("<doc><p>a{INS}xyz{END}b</p></doc>"));
}

#[test]
fn typing_at_either_edge_extends_the_insertion() {
    let mut e = tracked(&format!("<doc><p>a{INS}xyz{END}b</p></doc>"));
    // Just after the end mark.
    e.set_caret(at(&e, "b", 0));
    type_str(&mut e, "1");
    assert_eq!(xml(&e), format!("<doc><p>a{INS}xyz1{END}b</p></doc>"));
    // Just before the start mark.
    e.set_caret(at(&e, "a", 1));
    type_str(&mut e, "0");
    assert_eq!(xml(&e), format!("<doc><p>a{INS}0xyz1{END}b</p></doc>"));
}

#[test]
fn typing_in_someone_elses_insertion_splits_it() {
    let theirs = r#"<?xm-insertion_mark_start author="ann" time="1"?>"#;
    let mut e = tracked(&format!("<doc><p>{theirs}abcd{END}</p></doc>"));
    e.set_caret(at(&e, "abcd", 2));
    type_str(&mut e, "X");
    assert_eq!(
        xml(&e),
        format!("<doc><p>{theirs}ab{END}{INS}X{END}{theirs}cd{END}</p></doc>")
    );
    // At the very end of theirs no empty half is left over.
    let mut e = tracked(&format!("<doc><p>{theirs}ab{END}c</p></doc>"));
    e.set_caret(at(&e, "ab", 2));
    type_str(&mut e, "X");
    assert_eq!(
        xml(&e),
        format!("<doc><p>{theirs}ab{END}{INS}X{END}c</p></doc>")
    );
    assert_balanced(&e);
}

#[test]
fn backspace_in_own_insertion_just_removes_text() {
    let mut e = tracked("<doc><p>ab</p></doc>");
    e.set_caret(at(&e, "ab", 1));
    type_str(&mut e, "xy");
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>a{INS}x{END}b</p></doc>"));
    // Removing the last inserted character removes the marks with it.
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), "<doc><p>ab</p></doc>");
    assert_eq!(e.caret(), at(&e, "ab", 1));
}

#[test]
fn backspace_on_original_text_makes_a_deletion_that_grows() {
    let mut e = tracked("<doc><p>abc</p></doc>");
    e.set_caret(at(&e, "abc", 3));
    e.delete_backward(&R).unwrap();
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>a{}</p></doc>", del("bc")));
    // The caret stays in front of the deletion, but new text goes after it.
    e.insert_text(&R, "Q").unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>a{}{INS}Q{END}</p></doc>", del("bc"))
    );
    // Backspacing and typing coalesce into one undo step, as untracked.
    assert!(e.undo());
    assert_eq!(xml(&e), "<doc><p>abc</p></doc>");
    assert!(!e.can_undo());
}

#[test]
fn delete_key_makes_a_deletion_that_grows_forwards() {
    let mut e = tracked("<doc><p>abc</p></doc>");
    e.set_caret(at(&e, "abc", 0));
    e.delete_forward(&R).unwrap();
    e.delete_forward(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>{}c</p></doc>", del("ab")));
    assert_eq!(e.caret(), at(&e, "c", 0));
}

#[test]
fn backspace_steps_over_deletions_and_comments() {
    let mut e = tracked(
        r#"<doc><p>ab<?xm-deletion_mark author="ann" time="1" data="X"?><?xm-annotation author="ann" time="1" data="note"?>c</p></doc>"#,
    );
    e.set_caret(at(&e, "c", 0));
    e.delete_backward(&R).unwrap();
    assert_eq!(
        xml(&e),
        format!(
            r#"<doc><p>a{}<?xm-deletion_mark author="ann" time="1" data="X"?><?xm-annotation author="ann" time="1" data="note"?>c</p></doc>"#,
            del("b")
        )
    );
    // Untracked, the marks are still not deleted as text.
    e.tracking = None;
    e.set_caret(at(&e, "c", 0));
    e.delete_backward(&R).unwrap();
    assert!(xml(&e).contains("data=\"note\""));
    assert!(xml(&e).contains("data=\"X\""));
}

#[test]
fn escaped_text_is_kept_in_deletions() {
    let mut e = tracked("<doc><p>a&lt;b&amp;c</p></doc>");
    let p = at(&e, "a<b&c", 0);
    e.sel = Selection {
        anchor: p,
        focus: Pos::new(p.node, 5),
    };
    e.delete_selection(&R).unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>{}</p></doc>", del("a&amp;lt;b&amp;amp;c"))
    );
    let node = changes(&e.doc)[0].node;
    assert_eq!(changes(&e.doc)[0].excerpt, "a<b&c");
    e.reject_change(&R, node).unwrap();
    assert_eq!(e.doc.text_content(e.doc.root()), "a<b&c");
}

#[test]
fn replacing_a_selection_records_both_sides() {
    let mut e = tracked("<doc><p>abcd</p></doc>");
    let p = at(&e, "abcd", 1);
    e.sel = Selection {
        anchor: p,
        focus: Pos::new(p.node, 3),
    };
    e.insert_text(&R, "X").unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>a{}{INS}X{END}d</p></doc>", del("bc"))
    );
    assert!(e.undo());
    assert_eq!(xml(&e), "<doc><p>abcd</p></doc>");
}

#[test]
fn deleted_inline_markup_is_restored_on_reject() {
    let src = "<doc><p>a<b>bold</b>c</p></doc>";
    let mut e = tracked(src);
    e.sel = Selection {
        anchor: at(&e, "a", 0),
        focus: at(&e, "c", 1),
    };
    e.delete_selection(&R).unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>{}</p></doc>", del("a&lt;b&gt;bold&lt;/b&gt;c"))
    );
    e.resolve_all(&R, false).unwrap();
    assert_eq!(xml(&e), src);
}

#[test]
fn deleting_across_paragraphs_keeps_the_paragraphs() {
    let mut e = tracked("<doc><p>ab</p><p>cd</p></doc>");
    e.sel = Selection {
        anchor: at(&e, "ab", 1),
        focus: at(&e, "cd", 1),
    };
    e.delete_selection(&R).unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>a{}</p><p>{}d</p></doc>", del("b"), del("c"))
    );
    e.resolve_all(&R, true).unwrap();
    assert_eq!(xml(&e), "<doc><p>a</p><p>d</p></doc>");
}

#[test]
fn own_insertions_inside_a_deleted_range_just_go() {
    let mut e = tracked(&format!("<doc><p>ab{INS}XY{END}cd</p></doc>"));
    e.sel = Selection {
        anchor: at(&e, "ab", 1),
        focus: at(&e, "cd", 1),
    };
    e.delete_selection(&R).unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>a{}{}d</p></doc>", del("b"), del("c"))
    );
    assert_balanced(&e);
}

#[test]
fn accept_and_reject_each_change() {
    let marked = format!("<doc><p>a{INS}new{END}b{}c</p></doc>", del("old"));
    let cases = [
        (0, true, format!("<doc><p>anewb{}c</p></doc>", del("old"))),
        (0, false, format!("<doc><p>ab{}c</p></doc>", del("old"))),
        (1, true, format!("<doc><p>a{INS}new{END}bc</p></doc>")),
        (1, false, format!("<doc><p>a{INS}new{END}boldc</p></doc>")),
    ];
    for (which, accept, want) in cases {
        let mut e = tracked(&marked);
        let node = changes(&e.doc)[which].node;
        if accept {
            e.accept_change(&R, node).unwrap();
        } else {
            e.reject_change(&R, node).unwrap();
        }
        assert_eq!(xml(&e), want);
        assert!(e.undo(), "one undo step");
        assert_eq!(xml(&e), marked);
        assert!(!e.can_undo());
    }
}

#[test]
fn accept_all_and_reject_all() {
    let marked = format!(
        "<doc><p>a{INS}new{END}b{}c</p><p>{INS}x{}y{END}</p></doc>",
        del("old"),
        del("gone")
    );
    let mut e = tracked(&marked);
    e.resolve_all(&R, true).unwrap();
    assert_eq!(xml(&e), "<doc><p>anewbc</p><p>xy</p></doc>");
    assert!(e.undo());
    assert_eq!(xml(&e), marked);
    e.resolve_all(&R, false).unwrap();
    assert_eq!(xml(&e), "<doc><p>aboldc</p><p></p></doc>");
    assert!(e.undo());
    assert!(!e.can_undo());
    assert_eq!(e.resolve_all(&R, true), Ok(()));
    assert_eq!(e.resolve_all(&R, true), Err(EditError::Nothing));
}

#[test]
fn untracked_edits_never_leave_half_a_pair() {
    let src = format!("<doc><p>a{INS}bc{END}d</p></doc>");
    // A selection covering only the start of the insertion.
    let mut e = Editor::new(Document::from_str(&src).unwrap());
    e.sel = Selection {
        anchor: at(&e, "a", 0),
        focus: at(&e, "bc", 1),
    };
    e.delete_selection(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>{INS}c{END}d</p></doc>"));
    assert_balanced(&e);
    // Backspacing the last inserted character removes the empty pair.
    e.set_caret(at(&e, "c", 1));
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), "<doc><p>d</p></doc>");
    // Deleting an element that holds one half moves that half out.
    let src = format!("<doc><p>a<b>x{INS}y</b>z{END}</p></doc>");
    let mut e = Editor::new(Document::from_str(&src).unwrap());
    let b = e
        .doc
        .descendants(e.doc.root())
        .find(|&n| e.doc.name(n) == Some("b"))
        .unwrap();
    e.select_element(b);
    e.delete_selection(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>a{INS}z{END}</p></doc>"));
    assert_balanced(&e);
}

#[test]
fn caret_passes_over_hidden_marks() {
    let e = tracked(&format!("<doc><p>a{INS}x{END}b</p></doc>"));
    let p = next_pos(&e.doc, &R, at(&e, "a", 1), false);
    assert_eq!(p, at(&e, "x", 1));
    let p = next_pos(&e.doc, &R, p, false);
    assert_eq!(p, at(&e, "b", 1));
    let p = prev_pos(&e.doc, &R, at(&e, "b", 0), false);
    assert_eq!(p, at(&e, "x", 0));
}

#[test]
fn pasted_markup_is_one_insertion() {
    let mut e = tracked("<doc><p>ab</p></doc>");
    e.set_caret(at(&e, "ab", 1));
    e.paste(&R, "<b>B</b>", true).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>a{INS}<b>B</b>{END}b</p></doc>"));
    type_str(&mut e, "z");
    assert_eq!(xml(&e), format!("<doc><p>a{INS}<b>B</b>z{END}b</p></doc>"));
    // Marks in the pasted fragment itself are dropped.
    let mut e = tracked("<doc><p>ab</p></doc>");
    e.set_caret(at(&e, "ab", 1));
    let clip = format!(
        r#"<i>q</i><?xm-insertion_mark_start author="ann" time="1"?>r{END}{}"#,
        del("s")
    );
    e.paste(&R, &clip, true).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>a{INS}<i>q</i>r{END}b</p></doc>"));
    assert_balanced(&e);
}

#[test]
fn replace_text_range_is_tracked() {
    let mut e = tracked("<doc><p>colour</p></doc>");
    let p = at(&e, "colour", 0);
    e.replace_text_range(&R, p.node, 0, 6, "color").unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><p>{}{INS}color{END}</p></doc>", del("colour"))
    );
}

#[test]
fn comments_add_edit_delete() {
    let mut e = tracked("<doc><p>abcd</p></doc>");
    e.set_caret(at(&e, "abcd", 1));
    let c = e.add_comment(&R, "me", T, "Point & \"quote\"").unwrap();
    assert_eq!(
        xml(&e),
        format!(
            r#"<doc><p>a<?xm-annotation author="me" time="{T}" data="Point &amp; &quot;quote&quot;"?>bcd</p></doc>"#
        )
    );
    e.set_comment_text(&R, c, "Changed").unwrap();
    assert_eq!(changes(&e.doc)[0].comment, "Changed");
    e.delete_comment(&R, c).unwrap();
    assert_eq!(xml(&e), "<doc><p>abcd</p></doc>");
    // A comment on a selection is a range.
    let p = at(&e, "abcd", 1);
    e.sel = Selection {
        anchor: p,
        focus: Pos::new(p.node, 3),
    };
    let c = e.add_comment(&R, "me", T, "Range").unwrap();
    assert_eq!(
        xml(&e),
        format!(
            r#"<doc><p>a<?xm-annotation_start author="me" time="{T}" data="Range"?>bc<?xm-annotation_end?>d</p></doc>"#
        )
    );
    let list = changes(&e.doc);
    assert_eq!(list.len(), 1);
    assert_eq!(
        (list[0].kind, list[0].excerpt.as_str()),
        (ChangeKind::Comment, "bc")
    );
    // Comments are not changes: accept-all leaves them.
    assert_eq!(e.resolve_all(&R, true), Err(EditError::Nothing));
    e.delete_comment(&R, c).unwrap();
    assert_eq!(xml(&e), "<doc><p>abcd</p></doc>");
    assert!(e.undo());
    assert!(xml(&e).contains("annotation_start"));
}

#[test]
fn change_at_finds_the_change_under_the_caret() {
    let e = tracked(&format!("<doc><p>a{INS}new{END}b{}c</p></doc>", del("old")));
    let c = e.change_at(at(&e, "new", 1)).unwrap();
    assert_eq!((c.kind, c.excerpt.as_str()), (ChangeKind::Insertion, "new"));
    let c = e.change_at(at(&e, "c", 0)).unwrap();
    assert_eq!((c.kind, c.excerpt.as_str()), (ChangeKind::Deletion, "old"));
    assert!(e.change_at(at(&e, "a", 0)).is_none());
}

#[test]
fn listing_reports_every_change_in_order() {
    let e = tracked(&format!(
        r#"<doc><p>{INS}one{END}{}<?xm-annotation author="ann" time="20200101T000000+0000" data="c"?></p></doc>"#,
        del("two")
    ));
    let kinds: Vec<_> = changes(&e.doc)
        .iter()
        .map(|c| (c.kind, c.author.clone()))
        .collect();
    assert_eq!(
        kinds,
        [
            (ChangeKind::Insertion, "me".to_string()),
            (ChangeKind::Deletion, "me".to_string()),
            (ChangeKind::Comment, "ann".to_string())
        ]
    );
    let _ = NoRules;
}

#[test]
fn enter_after_tracked_typing_keeps_each_paragraph_paired() {
    let mut e = tracked("<doc><p>ab</p></doc>");
    e.set_caret(at(&e, "ab", 2));
    type_str(&mut e, "x");
    e.enter(&R).unwrap();
    type_str(&mut e, "y");
    assert_balanced(&e);
    assert_eq!(
        xml(&e),
        format!("<doc><p>ab{INS}x{END}</p><p>{INS}y{END}</p></doc>")
    );
    // The new text is still the author's own: Backspace just removes it.
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>ab{INS}x{END}</p><p/></doc>"));
    assert_balanced(&e);
}

#[test]
fn splitting_inside_an_insertion_ends_and_restarts_it() {
    let src = format!("<doc><p>a{INS}xy{END}b</p></doc>");
    for tracking in [true, false] {
        let mut e = tracked(&src);
        if !tracking {
            e.tracking = None;
        }
        e.set_caret(at(&e, "xy", 1));
        e.enter(&R).unwrap();
        assert_eq!(
            xml(&e),
            format!("<doc><p>a{INS}x{END}</p><p>{INS}y{END}b</p></doc>")
        );
        assert_balanced(&e);
        assert!(e.undo());
        assert_eq!(xml(&e), src);
    }
}

#[test]
fn an_insertion_across_paragraphs_counts_as_one() {
    // As a file from elsewhere might have it.
    let mut e = tracked(&format!("<doc><p>{INS}ab</p><p>cd{END}e</p></doc>"));
    e.set_caret(at(&e, "cd", 1));
    type_str(&mut e, "Z");
    assert_eq!(
        xml(&e),
        format!("<doc><p>{INS}ab</p><p>cZd{END}e</p></doc>")
    );
    e.delete_backward(&R).unwrap();
    e.delete_backward(&R).unwrap();
    assert_eq!(xml(&e), format!("<doc><p>{INS}ab</p><p>d{END}e</p></doc>"));
    let c = e.change_at(at(&e, "d", 0)).unwrap();
    assert_eq!(c.kind, ChangeKind::Insertion);
    e.accept_change(&R, c.node).unwrap();
    assert_eq!(xml(&e), "<doc><p>ab</p><p>de</p></doc>");
}

#[test]
fn surrounding_part_of_an_insertion_keeps_it_working() {
    let mut e = tracked(&format!("<doc><p>a{INS}xy{END}b</p></doc>"));
    e.sel = Selection {
        anchor: at(&e, "a", 0),
        focus: at(&e, "xy", 1),
    };
    e.surround(&R, "b", &|_, _, _, _| true).unwrap();
    assert_balanced(&e);
    // Text typed inside the moved part is still the author's insertion.
    e.set_caret(at(&e, "x", 1));
    type_str(&mut e, "Q");
    assert_balanced(&e);
    let marks = e
        .doc
        .descendants(e.doc.root())
        .filter(|&n| is_mark(&e.doc, n))
        .count();
    assert_eq!(marks, 2, "{}", xml(&e));
}

#[test]
fn replacing_several_matches_in_one_text_node() {
    // Replace all works from the last match backwards.
    let mut e = tracked("<doc><p>cat and cat</p></doc>");
    let n = at(&e, "cat and", 0).node;
    e.transact(|ed| {
        ed.replace_text_range(&R, n, 8, 11, "dog")?;
        ed.replace_text_range(&R, n, 0, 3, "dog")
    })
    .unwrap();
    assert_eq!(
        xml(&e),
        format!(
            "<doc><p>{}{INS}dog{END} and {}{INS}dog{END}</p></doc>",
            del("cat"),
            del("cat")
        )
    );
    assert!(e.undo());
    assert_eq!(xml(&e), "<doc><p>cat and cat</p></doc>");
}

// ------------------------------------------------------------------ elements

const TWO: &str = "<doc><section><p>One</p><p>Two</p></section></doc>";

fn section(e: &Editor) -> NodeId {
    e.doc
        .descendants(e.doc.root())
        .find(|&n| e.doc.name(n) == Some("section"))
        .unwrap()
}

fn p_with(e: &Editor, text: &str) -> NodeId {
    let t = at(e, text, 0).node;
    e.doc.parent(t).unwrap()
}

#[test]
fn inserted_elements_are_tracked_and_reject_removes_them() {
    let mut e = tracked(TWO);
    let s = section(&e);
    e.set_caret(Pos::new(s, 1));
    e.insert_element_xml(&R, "p", "<p>New</p>").unwrap();
    assert_eq!(
        xml(&e),
        format!("<doc><section><p>One</p>{INS}<p>New</p>{END}<p>Two</p></section></doc>")
    );
    // Typing inside it extends the same insertion, no nested marks.
    e.set_caret(at(&e, "New", 3));
    type_str(&mut e, "er");
    assert_eq!(xml(&e).matches("insertion_mark_start").count(), 1);

    let mark = changes(&e.doc)[0].node;
    e.reject_change(&R, mark).unwrap();
    assert_eq!(xml(&e), TWO);
}

#[test]
fn accepting_an_inserted_element_keeps_it() {
    let mut e = tracked(TWO);
    let s = section(&e);
    e.set_caret(Pos::new(s, 2));
    e.insert_element_xml(&R, "p", "<p>Three</p>").unwrap();
    e.resolve_all(&R, true).unwrap();
    assert_eq!(
        xml(&e),
        "<doc><section><p>One</p><p>Two</p><p>Three</p></section></doc>"
    );
}

#[test]
fn deleted_elements_are_kept_in_a_deletion_mark() {
    let mut e = tracked(TWO);
    let two = p_with(&e, "Two");
    e.delete_element(&R, two).unwrap();
    assert_eq!(
        xml(&e),
        format!(
            "<doc><section><p>One</p>{}</section></doc>",
            del("&lt;p&gt;Two&lt;/p&gt;")
        )
    );
    let c = &changes(&e.doc)[0];
    assert_eq!(c.kind, ChangeKind::Deletion);
    assert_eq!(c.excerpt, "Two");
    // Reject brings the element back exactly; undo does too.
    e.reject_change(&R, c.node).unwrap();
    assert_eq!(xml(&e), TWO);
    e.undo();
    e.undo();
    assert_eq!(xml(&e), TWO);

    let mut e = tracked(TWO);
    let two = p_with(&e, "Two");
    e.delete_element(&R, two).unwrap();
    e.resolve_all(&R, true).unwrap();
    assert_eq!(xml(&e), "<doc><section><p>One</p></section></doc>");
}

#[test]
fn deleting_your_own_inserted_element_leaves_no_trace() {
    let mut e = tracked(TWO);
    let s = section(&e);
    e.set_caret(Pos::new(s, 1));
    let n = e.insert_element_xml(&R, "p", "<p>Oops</p>").unwrap();
    e.delete_element(&R, n).unwrap();
    assert!(!xml(&e).contains("deletion_mark"), "{}", xml(&e));
    // Only the now-empty insertion pair is left, which tidying removes or
    // accepting clears; either way no deletion is recorded.
    e.resolve_all(&R, true).ok();
    assert_eq!(xml(&e), TWO);
}

#[test]
fn removing_an_empty_element_is_not_recorded() {
    let mut e = tracked("<doc><section><p>One</p><p></p></section></doc>");
    let empty = e
        .doc
        .descendants(e.doc.root())
        .filter(|&n| e.doc.name(n) == Some("p"))
        .nth(1)
        .unwrap();
    e.delete_element(&R, empty).unwrap();
    assert_eq!(xml(&e), "<doc><section><p>One</p></section></doc>");
}

#[test]
fn resolving_works_on_documents_read_from_disk() {
    let src = format!(
        "<doc><section><p>a{INS}X{END}b{}c</p></section></doc>",
        del("Y")
    );
    for (accept, want) in [(true, "aXbc"), (false, "abYc")] {
        let mut e = Editor::new(Document::from_str(&src).unwrap());
        e.resolve_all(&R, accept).unwrap();
        assert_eq!(
            xml(&e),
            format!("<doc><section><p>{want}</p></section></doc>"),
            "accept={accept}"
        );
    }
}
