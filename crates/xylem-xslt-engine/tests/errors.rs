//! Errors carry the stylesheet file and line (and, at run time, the
//! templates and functions they passed through), and an editor can compile
//! unsaved text with `Stylesheet::compile_with`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use xylem_xslt_engine::{Stylesheet, XsltError, transform_bytes};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xylem-engine-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // As modules are named to a reader: absolute, links resolved.
    let dir = std::fs::canonicalize(&dir).unwrap();
    PathBuf::from(dir.to_string_lossy().trim_start_matches(r"\\?\"))
}

const TOP: &str = r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:include href="inc.xsl"/>
  <xsl:template match="/"><r><xsl:call-template name="t"/></r></xsl:template>
</xsl:stylesheet>"#;

const INC: &str = r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:template name="t"><xsl:value-of select="'ok'"/></xsl:template>
</xsl:stylesheet>"#;

#[test]
fn compile_with_uses_unsaved_text() {
    let dir = temp("compile-with");
    let top = dir.join("top.xsl");
    std::fs::write(&top, TOP).unwrap();
    std::fs::write(dir.join("inc.xsl"), INC).unwrap();

    // The saved files compile and run.
    let ss = Stylesheet::compile_with(&top, &|_| None).unwrap();
    let out = transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap();
    assert_eq!(out.text, "<r>ok</r>");

    // An unsaved include replaces the file on disk.
    let inc_path = dir.join("inc.xsl");
    let edited = INC.replace("'ok'", "'edited'");
    let read = |p: &Path| (p == inc_path).then(|| edited.clone().into_bytes());
    let ss = Stylesheet::compile_with(&top, &read).unwrap();
    let out = transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap();
    assert_eq!(out.text, "<r>edited</r>");

    // A top module that only exists in the editor.
    let new_top = dir.join("unsaved.xsl");
    let read = |p: &Path| (p == new_top).then(|| TOP.as_bytes().to_vec());
    assert!(Stylesheet::compile_with(&new_top, &read).is_ok());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_errors_name_the_file_and_line() {
    let dir = temp("compile-errors");
    let top = dir.join("top.xsl");
    std::fs::write(&top, TOP).unwrap();
    let inc_path = dir.join("inc.xsl");

    // An XPath error in an included module: its file and the element's line.
    let bad_xpath = INC.replace("select=\"'ok'\"", "select=\"1 +\"");
    let read = |p: &Path| (p == inc_path).then(|| bad_xpath.clone().into_bytes());
    let e: XsltError = Stylesheet::compile_with(&top, &read).err().unwrap();
    assert_eq!(
        e.file.as_deref(),
        Some(inc_path.to_string_lossy().as_ref()),
        "{e}"
    );
    assert_eq!(e.line, Some(2), "{e}");

    // Malformed XML: file, line and column.
    let malformed = "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n  <xsl:template>\n</xsl:stylesheet>";
    let read = |p: &Path| (p == inc_path).then(|| malformed.as_bytes().to_vec());
    let e = Stylesheet::compile_with(&top, &read).err().unwrap();
    assert_eq!(
        e.file.as_deref(),
        Some(inc_path.to_string_lossy().as_ref()),
        "{e}"
    );
    assert_eq!(e.line, Some(3), "{e}");
    assert!(e.column.is_some(), "{e}");

    // The text form keeps the location for callers that want a string.
    let text = Stylesheet::compile(&top).err().unwrap();
    assert!(text.contains("inc.xsl"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runtime_errors_carry_the_template_chain() {
    let dir = temp("runtime-errors");
    let top = dir.join("top.xsl");
    std::fs::write(&top, TOP).unwrap();
    std::fs::write(
        dir.join("inc.xsl"),
        INC.replace("select=\"'ok'\"", "select=\"error((), 'stop here')\""),
    )
    .unwrap();
    let ss = Stylesheet::compile(&top).unwrap();
    let text = transform_bytes(&ss, b"<a/>", &HashMap::new(), false)
        .err()
        .unwrap();
    let e = XsltError::from_message(&text);
    assert!(e.message.contains("stop here"), "{text}");
    assert_eq!(e.stack.len(), 2, "{text}");
    assert_eq!(e.stack[0].kind, "template");
    assert!(e.stack[0].file.ends_with("inc.xsl"), "{text}");
    assert_eq!(e.stack[0].line, 2);
    assert!(e.stack[1].file.ends_with("top.xsl"), "{text}");
    assert_eq!(e.stack[1].line, 3);
    // With no location of its own, the error points at the innermost frame.
    assert_eq!(e.line, Some(2));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn undeclared_variables_are_compile_errors() {
    let dir = temp("undeclared");
    let compile = |name: &str, body: &str| {
        let path = dir.join(format!("{name}.xsl"));
        let text = format!(
            "<xsl:stylesheet version=\"2.0\" xmlns:xsl=\"http://www.w3.org/1999/XSL/Transform\">\n{body}\n</xsl:stylesheet>"
        );
        std::fs::write(&path, text).unwrap();
        Stylesheet::compile_with(&path, &|_| None)
    };
    // In scope: params, earlier variables, globals declared anywhere,
    // variables bound inside the expression.
    let ok = compile(
        "ok",
        r#"<xsl:template match="/"><xsl:param name="p" select="1"/><xsl:variable name="v" select="$p + $g"/>
  <r a="{$v}"><xsl:value-of select="for $i in (1, 2) return $i + $v, some $j in 1 satisfies $j"/></r></xsl:template>
<xsl:variable name="g" select="2"/>"#,
    );
    assert!(ok.is_ok(), "{:?}", ok.err());
    // A variable used outside the element that declares it.
    let e = compile(
        "scope",
        "<xsl:template match=\"/\">\n<xsl:if test=\"1\"><xsl:variable name=\"x\" select=\"1\"/></xsl:if>\n<r><xsl:value-of select=\"$x\"/></r></xsl:template>",
    )
    .err()
    .unwrap();
    assert!(e.message.contains("XPST0008") && e.message.contains("$x"), "{e}");
    assert_eq!(e.line, Some(4), "{e}");
    // In an attribute value template.
    let e = compile("avt", "<xsl:template match=\"/\"><r a=\"{$nope}\"/></xsl:template>")
        .err()
        .unwrap();
    assert!(e.message.contains("$nope"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}
