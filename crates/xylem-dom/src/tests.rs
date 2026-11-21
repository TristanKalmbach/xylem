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
