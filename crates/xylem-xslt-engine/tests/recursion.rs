//! Endless or very deep recursion ends in an ordinary error, never in a
//! stack overflow (which would end a process that embeds the engine), and
//! deep but finite recursion still works.

use std::collections::HashMap;
use xylem_xslt_engine::{Stylesheet, transform_bytes};

fn run(name: &str, body: &str) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!("xylem-engine-recursion-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.xsl"));
    std::fs::write(
        &path,
        format!(
            r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:xs="http://www.w3.org/2001/XMLSchema" xmlns:f="urn:f" exclude-result-prefixes="xs f">{body}</xsl:stylesheet>"#
        ),
    )
    .unwrap();
    let ss = Stylesheet::compile(&path)?;
    transform_bytes(&ss, b"<a/>", &HashMap::new(), false).map(|o| o.text)
}

fn assert_depth_error(r: Result<String, String>) {
    let e = r.expect_err("endless recursion must fail");
    assert!(e.contains("nested template or function calls"), "{}", &e[..e.len().min(300)]);
}

#[test]
fn endless_named_template_recursion_is_an_error() {
    assert_depth_error(run(
        "named",
        r#"<xsl:template name="t"><x><xsl:call-template name="t"/></x></xsl:template>
           <xsl:template match="/"><xsl:call-template name="t"/></xsl:template>"#,
    ));
}

#[test]
fn endless_function_recursion_is_an_error() {
    assert_depth_error(run(
        "function",
        r#"<xsl:function name="f:f" as="xs:integer"><xsl:param name="n" as="xs:integer"/><xsl:sequence select="f:f($n + 1) + 1"/></xsl:function>
           <xsl:template match="/"><r><xsl:value-of select="f:f(0)"/></r></xsl:template>"#,
    ));
}

#[test]
fn endless_apply_templates_recursion_is_an_error() {
    assert_depth_error(run(
        "apply",
        r#"<xsl:template match="/"><x><xsl:apply-templates select="."/></x></xsl:template>"#,
    ));
}

#[test]
fn deep_finite_recursion_still_works() {
    let out = run(
        "deep",
        r#"<xsl:function name="f:sum" as="xs:integer"><xsl:param name="n" as="xs:integer"/><xsl:sequence select="if ($n = 0) then 0 else $n + f:sum($n - 1)"/></xsl:function>
           <xsl:template name="count"><xsl:param name="n"/><xsl:if test="$n > 0"><xsl:call-template name="count"><xsl:with-param name="n" select="$n - 1"/></xsl:call-template></xsl:if><xsl:if test="$n = 0">done</xsl:if></xsl:template>
           <xsl:template match="/"><r s="{f:sum(9000)}"><xsl:call-template name="count"><xsl:with-param name="n" select="9000"/></xsl:call-template></r></xsl:template>"#,
    )
    .unwrap();
    assert_eq!(out, r#"<r s="40504500">done</r>"#);
}
