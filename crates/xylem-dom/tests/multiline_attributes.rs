//! Editing an element that has a multi-line attribute value changes only
//! what was edited when the document is saved: the other attribute's line
//! breaks (LF or CR LF, as written) stay byte for byte.

use xylem_dom::Document;

fn edit_and_save(text: &str) -> String {
    let mut doc = Document::from_bytes(text.as_bytes()).unwrap();
    let root = doc.document_element().unwrap();
    let el = doc.element_children(root).next().unwrap();
    doc.set_attr(el, "status", "done");
    String::from_utf8(doc.to_bytes()).unwrap()
}

#[test]
fn a_multi_line_attribute_survives_an_edit_to_another_attribute() {
    for nl in ["\n", "\r\n"] {
        let text = format!(
            "<doc>{nl}  <item note=\"first line{nl}second line{nl}  third\" status=\"open\">x</item>{nl}</doc>{nl}"
        );
        let saved = edit_and_save(&text);
        assert_eq!(saved, text.replace("status=\"open\"", "status=\"done\""), "line end {nl:?}");
    }
}

#[test]
fn an_untouched_multi_line_attribute_saves_unchanged() {
    for nl in ["\n", "\r\n"] {
        let text = format!("<doc note=\"a{nl}b\"/>{nl}");
        let doc = Document::from_bytes(text.as_bytes()).unwrap();
        assert_eq!(String::from_utf8(doc.to_bytes()).unwrap(), text);
        // The value, as XML reads it, has the line break as a space.
        let root = doc.document_element().unwrap();
        assert_eq!(doc.attr(root, "note"), Some("a b"));
    }
}
