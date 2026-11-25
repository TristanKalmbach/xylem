//! Serialized results name `utf-16`, as the publishing host's string does;
//! saved as a file they are relabelled as the UTF-8 they are.

use std::collections::HashMap;
use std::path::Path;

use xylem_xslt_engine::{ResultForm, Stylesheet, label_utf8, transform_bytes_to};

fn serialize(stylesheet: &str) -> String {
    let read = |_: &Path| Some(stylesheet.as_bytes().to_vec());
    let ss = Stylesheet::compile_with(Path::new("labels.xsl"), &read).unwrap();
    let out = transform_bytes_to(&ss, b"<x/>", &HashMap::new(), ResultForm::Serialize).unwrap();
    out.serialized
}

#[test]
fn xml_declaration_is_relabelled() {
    let text = serialize(
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
             <xsl:template match="/"><a>é</a></xsl:template>
           </xsl:stylesheet>"#,
    );
    assert!(text.starts_with(r#"<?xml version="1.0" encoding="utf-16"?>"#));
    assert_eq!(
        label_utf8(&text),
        r#"<?xml version="1.0" encoding="utf-8"?><a>é</a>"#
    );
}

#[test]
fn html_content_type_is_relabelled() {
    let text = serialize(
        r#"<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
             <xsl:output method="html"/>
             <xsl:template match="/"><html><head><title>t</title></head></html></xsl:template>
           </xsl:stylesheet>"#,
    );
    assert!(text.contains("charset=utf-16"));
    let fixed = label_utf8(&text);
    assert!(fixed.contains(r#"content="text/html; charset=utf-8""#));
    assert!(!fixed.contains("utf-16"));
}

#[test]
fn other_text_is_untouched() {
    for text in ["", "<a>utf-16</a>", "plain text"] {
        assert!(matches!(label_utf8(text), std::borrow::Cow::Borrowed(_)));
    }
}
