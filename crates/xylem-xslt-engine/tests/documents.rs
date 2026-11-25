//! `document()` and `doc()` when a file is missing or malformed, matched
//! to the reference publishing service: XSLT 1.0 fails, XSLT 2.0
//! `document()` recovers with an empty sequence and a warning, `doc()`
//! fails, and `document('')` is the calling module.

use std::collections::HashMap;
use std::path::Path;
use xylem_xslt_engine::{Stylesheet, transform_bytes};

fn run(dir: &Path, version: &str, body: &str) -> Result<xylem_xslt_engine::Output, String> {
    let xsl = dir.join(format!("s{}.xsl", version.replace('.', "")));
    std::fs::write(
        &xsl,
        format!(
            r#"<xsl:stylesheet version="{version}" xmlns:xsl="http://www.w3.org/1999/XSL/Transform" xmlns:my="urn:my" exclude-result-prefixes="my"><my:data><v>embedded</v></my:data><xsl:template match="/"><r>{body}</r></xsl:template></xsl:stylesheet>"#
        ),
    )
    .unwrap();
    let ss = Stylesheet::compile(&xsl)?;
    transform_bytes(&ss, b"<a/>", &HashMap::new(), false)
}

#[test]
fn unreadable_documents_behave_as_the_reference_does() {
    let dir = std::env::temp_dir().join(format!("xylem-engine-docs-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bad.xml"), "<a><b>").unwrap();
    let copy_missing = r#"<xsl:copy-of select="document('missing.xml')"/>"#;

    let e = run(&dir, "1.0", copy_missing).err().unwrap();
    assert!(
        e.starts_with("An error occurred while loading document 'missing.xml'.")
            && e.contains("Could not find file"),
        "{e}"
    );

    let o = run(&dir, "2.0", copy_missing).unwrap();
    assert_eq!(o.text, "<r />");
    assert_eq!(o.warnings.len(), 1);
    assert!(
        o.warnings[0].starts_with("FODC0002: document('missing.xml')"),
        "{:?}",
        o.warnings
    );

    let e = run(&dir, "2.0", r#"<xsl:copy-of select="doc('missing.xml')"/>"#)
        .err()
        .unwrap();
    assert!(e.starts_with("FODC0002: doc('missing.xml')"), "{e}");

    for v in ["1.0", "2.0"] {
        assert!(run(&dir, v, r#"<xsl:copy-of select="document('bad.xml')"/>"#).is_err());
        let o = run(
            &dir,
            v,
            r#"<xsl:value-of select="document('')/*/my:data/v"/>"#,
        )
        .unwrap();
        assert_eq!(o.text, "<r>embedded</r>", "version {v}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_2_0_run_with_only_secondary_results_fails_to_publish() {
    let dir = std::env::temp_dir().join(format!(
        "xylem-engine-only-secondary-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let only = r#"<xsl:result-document href="out.xml"><x/></xsl:result-document>"#;
    // XSLT 2.0: no principal result next to a secondary one.
    let xsl = dir.join("s.xsl");
    std::fs::write(
        &xsl,
        format!(r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/">{only}</xsl:template></xsl:stylesheet>"#),
    )
    .unwrap();
    let ss = Stylesheet::compile(&xsl).unwrap();
    let o = transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap();
    assert!(
        o.published()
            .err()
            .unwrap()
            .contains("Object reference not set")
    );
    // A result-document without href opens the principal result, even
    // when it writes nothing: that publishes (empty).
    std::fs::write(
        &xsl,
        format!(r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/">{only}<xsl:result-document/></xsl:template></xsl:stylesheet>"#),
    )
    .unwrap();
    let ss = Stylesheet::compile(&xsl).unwrap();
    let o = transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap();
    assert_eq!(o.published().unwrap(), "");
    // An empty result alone is fine, as on the host.
    std::fs::write(
        &xsl,
        r#"<xsl:stylesheet version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"/></xsl:stylesheet>"#,
    )
    .unwrap();
    let ss = Stylesheet::compile(&xsl).unwrap();
    let o = transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap();
    assert_eq!(o.published().unwrap(), "");
    let _ = std::fs::remove_dir_all(&dir);
}
