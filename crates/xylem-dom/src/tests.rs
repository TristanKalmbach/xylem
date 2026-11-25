use super::*;

const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE dmodule [
<!ENTITY ICN-1 SYSTEM "ICN-1.cgm" NDATA cgm>
<!NOTATION cgm SYSTEM "cgm">
]>
<!-- top comment -->
<dmodule xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'   id="a&amp;b">
  <para>Hello &amp; <emphasis  type = "bold">world</emphasis>&#169; &custom;</para>
  <empty/>
  <![CDATA[ <raw> ]]>
  <?xm-replace_text Type new para here ?>
</dmodule>
"#;

#[test]
fn round_trip_is_byte_exact() {
    let doc = Document::from_str(SAMPLE).unwrap();
    assert_eq!(doc.to_xml(), SAMPLE);
}

#[test]
fn parses_structure() {
    let doc = Document::from_str(SAMPLE).unwrap();
    let root = doc.document_element().unwrap();
    assert_eq!(doc.name(root), Some("dmodule"));
    assert_eq!(doc.attr(root, "id"), Some("a&b"));
    let dt = doc.doctype().unwrap();
    assert_eq!(dt.name, "dmodule");
    assert!(dt.internal_subset.as_ref().unwrap().contains("ICN-1"));
    let para = doc.element_children(root).next().unwrap();
    assert_eq!(doc.text_content(para), "Hello & world© ");
    assert!(
        doc.children(para)
            .iter()
            .any(|&c| matches!(doc.kind(c), NodeKind::EntityRef(n) if n == "custom"))
    );
}

#[test]
fn edits_regenerate_only_touched_nodes() {
    let mut doc = Document::from_str(SAMPLE).unwrap();
    let root = doc.document_element().unwrap();
    let para = doc.element_children(root).next().unwrap();
    let em = doc.element_children(para).next().unwrap();
    doc.set_attr(em, "type", "italic");
    let out = doc.to_xml();
    assert!(out.contains(r#"<emphasis type="italic">world</emphasis>"#));
    assert!(out.contains("xmlns:xsi='http://www.w3.org/2001/XMLSchema-instance'   id=\"a&amp;b\""));

    let empty = doc.element_children(root).nth(1).unwrap();
    let t = doc.create_text("x < y");
    doc.append_child(empty, t);
    assert!(doc.to_xml().contains("<empty>x &lt; y</empty>"));
}

#[test]
fn errors_have_positions() {
    let e = Document::from_str("<a>\n  <b></c>\n</a>").unwrap_err();
    assert_eq!(e.line, 2);
    assert!(e.message.contains("does not match"));
    assert!(Document::from_str("<a>").is_err());
    assert!(Document::from_str("<a x='1' x='2'/>").is_err());
}

#[test]
fn split_wrap_unwrap() {
    let mut doc = Document::from_str("<p>abcdef</p>").unwrap();
    let p = doc.document_element().unwrap();
    let t = doc.children(p)[0];
    let r = doc.split_text(t, 3);
    assert_eq!(doc.text(r), Some("def"));
    let b = doc.wrap_children(p, 1, 2, "b");
    assert_eq!(doc.to_xml(), "<p>abc<b>def</b></p>");
    doc.unwrap_node(b);
    assert_eq!(doc.to_xml(), "<p>abcdef</p>");
    assert_eq!(doc.children(p).len(), 1);
}

#[test]
fn fragments_and_order() {
    let mut doc = Document::from_str("<r><a/><b/></r>").unwrap();
    let nodes = doc.parse_fragment_into("<x>1</x>text<y/>").unwrap();
    assert_eq!(nodes.len(), 3);
    let r = doc.document_element().unwrap();
    for (i, n) in nodes.into_iter().enumerate() {
        doc.insert_child(r, 1 + i, n);
    }
    assert_eq!(doc.to_xml(), "<r><a/><x>1</x>text<y/><b/></r>");
    let a = doc.children(r)[0];
    let b = doc.children(r)[4];
    assert_eq!(doc.cmp_order(a, b), std::cmp::Ordering::Less);
    assert_eq!(doc.path(b), "/r/b");
}

#[test]
fn utf16_and_latin1() {
    let s = "<?xml version=\"1.0\" encoding=\"UTF-16\"?><a>é</a>";
    let mut bytes = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    let doc = Document::from_bytes(&bytes).unwrap();
    assert_eq!(doc.encoding, Encoding::Utf16Le);
    assert_eq!(doc.to_bytes(), bytes);

    let latin = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><a>\xE9</a>".to_vec();
    let doc = Document::from_bytes(&latin).unwrap();
    assert_eq!(doc.text_content(doc.document_element().unwrap()), "é");
    assert_eq!(doc.to_bytes(), latin);
}

#[test]
fn subtree_stamps_follow_changes() {
    let mut doc = Document::from_str("<r><a><t>x</t></a><b>y</b></r>").unwrap();
    let r = doc.document_element().unwrap();
    let a = doc.element_children(r).next().unwrap();
    let b = doc.element_children(r).nth(1).unwrap();
    let t = doc.element_children(a).next().unwrap();
    let (sa, sb, sr) = (
        doc.subtree_stamp(a),
        doc.subtree_stamp(b),
        doc.subtree_stamp(r),
    );
    let text = doc.children(t)[0];
    doc.set_text(text, "changed");
    assert!(doc.subtree_stamp(a) > sa);
    assert!(doc.subtree_stamp(r) > sr);
    assert_eq!(
        doc.subtree_stamp(b),
        sb,
        "untouched sibling keeps its stamp"
    );
    doc.set_attr(b, "k", "v");
    assert!(doc.subtree_stamp(b) > sb);
}

#[test]
fn revisions_are_unique_across_documents() {
    let a = Document::from_str("<r/>").unwrap();
    let b = Document::from_str("<r/>").unwrap();
    assert_ne!(a.revision(), b.revision());
    assert_ne!(a.structure_revision(), b.structure_revision());
    let ra = a.document_element().unwrap();
    let rb = b.document_element().unwrap();
    assert_eq!(ra, rb, "same node ids...");
    assert_ne!(
        a.subtree_stamp(ra),
        b.subtree_stamp(rb),
        "...but different stamps"
    );
}

/// The text node of `<p>` in a document whose root holds one `<p>`.
fn para_text(doc: &Document) -> NodeId {
    let root = doc.document_element().unwrap();
    let p = doc.element_children(root).next().unwrap();
    doc.children(p)[0]
}

#[test]
fn replace_text_keeps_references_and_line_endings_around_the_change() {
    let src =
        "<r>\r\n<p>a &#176; b &gt; c > old\r\nnext &amp; x</p>\r\n<q x=\"&#65;\"/>\r\n</r>\r\n";
    let mut doc = Document::from_str(src).unwrap();
    let t = para_text(&doc);
    let at = doc.text(t).unwrap().find("old").unwrap();
    doc.replace_text(t, at..at + 3, "new<1>");
    assert_eq!(
        doc.to_xml(),
        src.replace("old", "new&lt;1&gt;"),
        "only the replaced span changes"
    );
    assert!(doc.text(t).unwrap().contains("new<1>"));
}

#[test]
fn replace_text_in_legacy_encodings_round_trips() {
    // Latin-1: "café" with é as one byte; the replacement changes only "old".
    let mut bytes = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r><p>caf".to_vec();
    bytes.push(0xE9);
    bytes.extend_from_slice(b" old &#8212; end</p></r>");
    let mut doc = Document::from_bytes(&bytes).unwrap();
    assert_eq!(doc.encoding, Encoding::Latin1);
    let t = para_text(&doc);
    let at = doc.text(t).unwrap().find("old").unwrap();
    doc.replace_text(t, at..at + 3, "neu");
    let mut expected = b"<?xml version=\"1.0\" encoding=\"ISO-8859-1\"?><r><p>caf".to_vec();
    expected.push(0xE9);
    expected.extend_from_slice(b" neu &#8212; end</p></r>");
    assert_eq!(doc.to_bytes(), expected);

    // UTF-16 with a byte order mark.
    let encode = |s: &str| {
        let mut b = vec![0xFF, 0xFE];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_le_bytes());
        }
        b
    };
    let mut doc = Document::from_bytes(&encode("<r><p>one two</p></r>")).unwrap();
    let t = para_text(&doc);
    doc.replace_text(t, 4..7, "2");
    assert_eq!(doc.to_bytes(), encode("<r><p>one 2</p></r>"));
}

#[test]
fn replace_text_in_cdata_keeps_the_section() {
    let src = "<r><p><![CDATA[a < old ]]></p></r>";
    let mut doc = Document::from_str(src).unwrap();
    let t = para_text(&doc);
    let at = doc.text(t).unwrap().find("old").unwrap();
    doc.replace_text(t, at..at + 3, "new");
    assert_eq!(doc.to_xml(), "<r><p><![CDATA[a < new ]]></p></r>");
}

#[test]
fn replace_text_is_undone_through_the_journal() {
    let src = "<r><p>x &#169; old</p></r>";
    let mut doc = Document::from_str(src).unwrap();
    let t = para_text(&doc);
    let at = doc.text(t).unwrap().find("old").unwrap();
    doc.begin_journal();
    doc.replace_text(t, at..at + 3, "new");
    assert_eq!(doc.to_xml(), "<r><p>x &#169; new</p></r>");
    let ops = doc.take_journal();
    doc.revert(&ops);
    assert_eq!(doc.to_xml(), src);
}

#[test]
fn offsets_point_at_each_node() {
    let src = "<r>\n  <a x=\"1\">text</a>\n  <!--c-->\n</r>";
    let doc = Document::from_str(src).unwrap();
    let (xml, offs) = doc.to_xml_with_offsets();
    assert_eq!(xml, src);
    let root = doc.document_element().unwrap();
    let a = doc.element_children(root).next().unwrap();
    assert!(xml[offs[a.index()]..].starts_with("<a x="));
    let text = doc.children(a)[0];
    assert!(xml[offs[text.index()]..].starts_with("text</a>"));
}

/// Changes `"old"` in the first text node holding it to `"new"`, so the
/// node is written afresh.
fn edit_text(doc: &mut Document, old: &str) {
    let n = doc
        .descendants(doc.root())
        .find(|&n| doc.text(n).is_some_and(|t| t.contains(old)))
        .unwrap();
    let t = doc.text(n).unwrap().replace(old, "new");
    doc.set_text(n, t);
}

#[test]
fn a_carriage_return_reference_survives_an_edit() {
    // LF line endings: `&#13;` is the only way a CR gets into the text, and
    // written raw it would be read back as a line feed.
    let mut doc = Document::from_str("<a>old&#13;\nline&#13;</a>\n").unwrap();
    edit_text(&mut doc, "old");
    assert_eq!(doc.to_xml(), "<a>new&#13;\nline&#13;</a>\n");
}

#[test]
fn crlf_line_endings_stay_as_they_are_in_edited_text() {
    let mut doc = Document::from_str("<a>\r\n  <b>old\r\nline</b>\r\n</a>\r\n").unwrap();
    edit_text(&mut doc, "old");
    assert_eq!(doc.to_xml(), "<a>\r\n  <b>new\r\nline</b>\r\n</a>\r\n");
}

#[test]
fn a_lone_carriage_return_is_escaped_even_with_crlf_endings() {
    let mut doc = Document::from_str("<a>old&#13;x</a>\r\n").unwrap();
    edit_text(&mut doc, "old");
    assert_eq!(doc.to_xml(), "<a>new&#13;x</a>\r\n");
}

#[test]
fn a_carriage_return_in_an_attribute_is_escaped() {
    let mut doc = Document::from_str("<a/>").unwrap();
    let a = doc.document_element().unwrap();
    doc.set_attr(a, "v", "x\ry");
    assert_eq!(doc.to_xml(), "<a v=\"x&#13;y\"/>");
}

#[test]
fn attribute_line_ends_normalise_to_one_space_each() {
    let doc = Document::from_str("<a v=\"x\r\ny\rz\nw\tq\"/>").unwrap();
    let root = doc.document_element().unwrap();
    assert_eq!(doc.attr(root, "v"), Some("x y z w q"));
}
