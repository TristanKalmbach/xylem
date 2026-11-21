use super::*;
use crate::testdata::{LEGACY_DESCRIPT, MODERN_DESCRIPT, MODERN_PM};

fn doc(xml: &str) -> Document {
    Document::from_str(xml).expect("parses")
}

fn modern_with_location(url: &str, content: &str) -> String {
    format!(
        r#"<dmodule xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="{url}"><identAndStatusSection/><content>{content}</content></dmodule>"#
    )
}

#[test]
fn schema_location_gives_issue_and_kind() {
    let info = detect(&doc(MODERN_DESCRIPT)).unwrap();
    assert_eq!(info.issue, Issue::V4_1);
    assert_eq!(info.schema, SchemaKind::Descript);
    assert_eq!(info.root, "dmodule");
    assert_eq!(info.source, DetectSource::SchemaLocation);

    for (token, kind, stem, issue) in [
        ("4-0", SchemaKind::Proced, "proced", Issue::V4_0),
        ("4-0-1", SchemaKind::Fault, "fault", Issue::V4_0_1),
        ("4-2", SchemaKind::Ipd, "ipd", Issue::V4_2),
        ("5-0", SchemaKind::Brex, "brex", Issue::V5_0),
        ("6", SchemaKind::Crew, "crew", Issue::V6_0),
        ("6-0", SchemaKind::Process, "process", Issue::V6_0),
        ("4-1", SchemaKind::ApplicCrossRefTable, "appliccrossreftable", Issue::V4_1),
        ("4-1", SchemaKind::Comrep, "comrep", Issue::V4_1),
    ] {
        let url = format!("http://www.s1000d.org/S1000D_{token}/xml_schema_flat/{stem}.xsd");
        let info = detect(&doc(&modern_with_location(&url, ""))).unwrap();
        assert_eq!((info.issue, info.schema), (issue, kind), "{url}");
    }
}

#[test]
fn schema_location_variants() {
    let xml = r#"<dmodule xmlns:x="http://www.w3.org/2001/XMLSchema-instance" x:schemaLocation="urn:ns http://www.s1000d.org/S1000D_5-0/xml_schema_flat/dml.xsd"><identAndStatusSection/></dmodule>"#;
    let info = detect(&doc(xml)).unwrap();
    assert_eq!((info.issue, info.schema), (Issue::V5_0, SchemaKind::Dml));
}

#[test]
fn publication_module_and_other_roots() {
    let info = detect(&doc(MODERN_PM)).unwrap();
    assert_eq!(info.schema, SchemaKind::Pm);
    assert_eq!(info.issue, Issue::V4_1);
    assert_eq!(info.root, "pm");

    let dml = r#"<dml><identAndStatusSection/></dml>"#;
    let info = detect(&doc(dml)).unwrap();
    assert_eq!(info.schema, SchemaKind::Dml);
    assert_eq!(info.source, DetectSource::Structure);
}

#[test]
fn doctype_public_id_gives_legacy_kind() {
    let info = detect(&doc(LEGACY_DESCRIPT)).unwrap();
    assert_eq!(info.schema, SchemaKind::Descript);
    assert_eq!(info.source, DetectSource::DocType);
    assert_eq!(info.issue, Issue::V3_0);
    assert_eq!(info.root, "dmodule");

    for (pubid, kind) in [
        ("-//S1000D//DTD Procedural Data Module//EN", SchemaKind::Proced),
        ("-//S1000D//DTD Fault Isolation Data Module//EN", SchemaKind::Fault),
        ("-//S1000D//DTD Illustrated Parts Data Module//EN", SchemaKind::Ipd),
        ("-//S1000D//DTD Crew Data Module//EN", SchemaKind::Crew),
    ] {
        let xml = format!(r#"<!DOCTYPE dmodule PUBLIC "{pubid}" "x.dtd"><dmodule><idstatus/><content/></dmodule>"#);
        assert_eq!(detect(&doc(&xml)).unwrap().schema, kind, "{pubid}");
    }
}

#[test]
fn doctype_issue_from_public_id_system_id_or_comment() {
    let by_pubid = r#"<!DOCTYPE dmodule PUBLIC "-//S1000D//DTD Descriptive Data Module Issue 2.2//EN" "descript.dtd"><dmodule><idstatus/></dmodule>"#;
    assert_eq!(detect(&doc(by_pubid)).unwrap().issue, Issue::V2_2);

    let by_sysid = r#"<!DOCTYPE dmodule SYSTEM "S1000D_2-3/dtd/proced.dtd"><dmodule><idstatus/></dmodule>"#;
    let info = detect(&doc(by_sysid)).unwrap();
    assert_eq!((info.issue, info.schema), (Issue::V2_3, SchemaKind::Proced));

    let by_comment = r#"<!DOCTYPE dmodule PUBLIC "-//S1000D//DTD Descriptive Data Module//EN" "descript.dtd">
<!-- S1000D issue 2.1 -->
<dmodule><idstatus/></dmodule>"#;
    assert_eq!(detect(&doc(by_comment)).unwrap().issue, Issue::V2_1);
}

#[test]
fn structural_fallback_for_modern_and_legacy() {
    let modern = r#"<dmodule><identAndStatusSection/><content><procedure/></content></dmodule>"#;
    let info = detect(&doc(modern)).unwrap();
    assert_eq!(info.schema, SchemaKind::Proced);
    assert_eq!(info.source, DetectSource::Structure);
    assert!(!info.issue.is_legacy());

    let legacy = r#"<dmodule><idstatus/><content><proced/></content></dmodule>"#;
    let info = detect(&doc(legacy)).unwrap();
    assert_eq!(info.schema, SchemaKind::Proced);
    assert!(info.issue.is_legacy());

    let unknown_content = r#"<dmodule><identAndStatusSection/><content><mystery/></content></dmodule>"#;
    assert_eq!(detect(&doc(unknown_content)).unwrap().schema, SchemaKind::Unknown);
}

#[test]
fn non_s1000d_documents_are_rejected() {
    assert!(detect(&doc("<html><body/></html>")).is_none());
    assert!(detect(&doc("<dmodule><foo/></dmodule>")).is_none());
    assert!(detect(&Document::new()).is_none());
    let foreign = r#"<book xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="book.xsd"/>"#;
    assert!(detect(&doc(foreign)).is_none(), "non-S1000D schema must not be claimed");
}

#[test]
fn schema_url_parsing() {
    assert_eq!(
        parse_schema_url("http://www.s1000d.org/S1000D_4-0-1/xml_schema_flat/descript.xsd"),
        (Some(Issue::V4_0_1), Some(SchemaKind::Descript))
    );
    assert_eq!(parse_schema_url("file:///C:/schemas/DESCRIPT.XSD"), (None, Some(SchemaKind::Descript)));
    assert_eq!(parse_schema_url("http://example.com/other.xsd"), (None, None));
}
