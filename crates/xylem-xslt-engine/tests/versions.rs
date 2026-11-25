//! Which stylesheets run as XSLT 2.0: by default as the reference
//! publishing service decides (a root `version` of exactly `2.0`), or with
//! `VersionRule::TwoAndAbove` every version from 2.0 up.

use std::collections::HashMap;
use xylem_xslt_engine::{Stylesheet, VersionRule, transform_bytes};

fn run(stylesheet: &str, rule: VersionRule) -> String {
    let dir = std::env::temp_dir().join(format!("xylem-engine-versions-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("s{}.xsl", stylesheet.len()));
    std::fs::write(&path, stylesheet).unwrap();
    let mut ss = Stylesheet::compile(&path).unwrap();
    ss.version_rule = rule;
    transform_bytes(&ss, b"<a/>", &HashMap::new(), false).unwrap().text
}

fn with_version(v: &str) -> String {
    format!(
        r#"<xsl:stylesheet version="{v}" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:template match="/"><r><xsl:value-of select="1 div 3"/></r></xsl:template></xsl:stylesheet>"#
    )
}

const XSLT1: &str = "<r>0.3333333333333333</r>";
const XSLT2: &str = "<r>0.333333333333333333</r>";

#[test]
fn only_version_2_0_runs_as_xslt_2_by_default() {
    for (v, want) in [("1.0", XSLT1), ("2.0", XSLT2), ("2", XSLT1), ("3.0", XSLT1)] {
        assert_eq!(run(&with_version(v), VersionRule::ReferenceHost), want, "version {v}");
    }
    let simplified = r#"<r xsl:version="2.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform"><xsl:value-of select="1 div 3"/></r>"#;
    assert_eq!(run(simplified, VersionRule::ReferenceHost), XSLT1);
}

#[test]
fn two_and_above_runs_later_versions_as_xslt_2() {
    for (v, want) in [("1.0", XSLT1), ("2.0", XSLT2), ("2", XSLT2), ("3.0", XSLT2)] {
        assert_eq!(run(&with_version(v), VersionRule::TwoAndAbove), want, "version {v}");
    }
}
