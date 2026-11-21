use xylem_dom::Document;

use super::*;
use crate::brex::Brex;
use crate::detect::{DetectSource, detect};
use crate::ident::{DmIdent, DmTitle, Language};

fn bike() -> Dmc {
    Dmc::parse("DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A").unwrap()
}

fn date() -> IssueDate {
    IssueDate::new(2026, 11, 20)
}

fn build(issue: Issue, kind: SchemaKind) -> String {
    new_data_module(issue, kind, &bike(), "Front brake", Some("Description"), ("en", "US"), date())
        .unwrap_or_else(|e| panic!("{issue} {kind:?}: {e}"))
}

#[test]
fn every_supported_combination_is_well_formed_and_detectable() {
    for issue in Issue::ALL {
        for kind in available_kinds(issue) {
            let xml = build(issue, kind);
            let doc = Document::from_str(&xml).unwrap_or_else(|e| panic!("{issue} {kind:?}: {e}\n{xml}"));
            let info = detect(&doc).unwrap_or_else(|| panic!("{issue} {kind:?} not detected"));
            assert_eq!(info.issue, issue, "{issue} {kind:?}");
            assert_eq!(info.schema, kind, "{issue} {kind:?}");
            assert_eq!(info.root, "dmodule");
            let expected_source = if issue.is_legacy() { DetectSource::DocType } else { DetectSource::SchemaLocation };
            assert_eq!(info.source, expected_source);

            let ident = DmIdent::from_document(&doc).unwrap_or_else(|| panic!("{issue} {kind:?}: no ident"));
            assert_eq!(ident.dmc, bike());
            assert_eq!(ident.language, Some(Language::new("en", "US")));
            assert_eq!(ident.issue_info.as_ref().unwrap().to_string(), "001-00");
            assert_eq!(ident.issue_date.as_ref().unwrap().to_string(), "2026-11-20");
            assert_eq!(ident.title, Some(DmTitle::new("Front brake", Some("Description"))));
            assert_eq!(ident.csdb_filename().unwrap(), "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A_001-00_en-US.XML");
        }
    }
}

#[test]
fn modern_module_has_required_status_content() {
    let xml = build(Issue::V4_1, SchemaKind::Descript);
    assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<dmodule "));
    assert!(xml.contains(
        r#"xsi:noNamespaceSchemaLocation="http://www.s1000d.org/S1000D_4-1/xml_schema_flat/descript.xsd""#
    ));
    assert!(xml.contains(r#"<issueDate year="2026" month="11" day="20"/>"#));
    assert!(xml.contains(r#"<security securityClassification="01"/>"#));
    assert!(xml.contains(r#"<responsiblePartnerCompany enterpriseCode="00000">"#));
    assert!(xml.contains(r#"<originator enterpriseCode="00000">"#));
    assert!(xml.contains("<simplePara>All</simplePara>"));
    assert!(xml.contains("<qualityAssurance><unverified/></qualityAssurance>"));
    assert!(xml.contains("<brexDmRef>"));
    assert!(xml.contains("<para>Placeholder paragraph.</para>"));
    // Nested elements are indented.
    assert!(xml.contains("\n    <dmAddress>\n      <dmIdent>\n        <dmCode "));
    assert!(xml.ends_with("</dmodule>\n"));
}

#[test]
fn schema_location_matches_issue_and_kind() {
    for (issue, token) in [
        (Issue::V4_0, "4-0"),
        (Issue::V4_0_1, "4-0-1"),
        (Issue::V4_2, "4-2"),
        (Issue::V5_0, "5-0"),
        (Issue::V6_0, "6"),
    ] {
        let xml = build(issue, SchemaKind::Proced);
        assert!(xml.contains(&format!("S1000D_{token}/xml_schema_flat/proced.xsd")), "{issue}");
    }
}

#[test]
fn content_per_kind() {
    let proced = build(Issue::V4_1, SchemaKind::Proced);
    for needle in ["<preliminaryRqmts>", "<noConds/>", "<mainProcedure>", "<proceduralStep>", "<closeRqmts>"] {
        assert!(proced.contains(needle), "{needle}");
    }
    assert!(build(Issue::V4_1, SchemaKind::Fault).contains("<faultIsolation>"));
    assert!(build(Issue::V4_1, SchemaKind::Crew).contains("<crewDrillStep>"));
    assert!(build(Issue::V4_1, SchemaKind::Ipd).contains("<illustratedPartsCatalog>"));
    assert!(build(Issue::V4_1, SchemaKind::Process).contains("<process>"));
    let brex = Brex::from_document(&Document::from_str(&build(Issue::V4_1, SchemaKind::Brex)).unwrap());
    assert_eq!(brex.rules.len(), 1);
    assert_eq!(brex.rules[0].path, "//para");
}

#[test]
fn legacy_module_layout() {
    let xml = build(Issue::V3_0, SchemaKind::Descript);
    assert!(xml.contains(r#"<!DOCTYPE dmodule PUBLIC "-//S1000D//DTD Descriptive Data Module//EN" "descript.dtd" []>"#));
    assert!(xml.contains("<!-- S1000D issue 3.0 -->"));
    assert!(xml.contains("<idstatus>"));
    assert!(xml.contains("<chapnum>D00</chapnum>"));
    assert!(xml.contains(r#"<issno issno="001" inwork="00" type="new"/>"#));
    assert!(xml.contains(r#"<issdate year="2026" month="11" day="20"/>"#));
    assert!(xml.contains(r#"<security class="01"/>"#));
    assert!(xml.contains("<qa><unverif/></qa>"));
    assert!(xml.contains("<descript>"));
    assert!(!xml.contains("identAndStatusSection"));

    let proced = build(Issue::V2_2, SchemaKind::Proced);
    assert!(proced.contains(r#"PUBLIC "-//S1000D//DTD Procedural Data Module//EN" "proced.dtd""#));
    assert!(proced.contains("<!-- S1000D issue 2.2 -->"));
    assert!(proced.contains("<step1>"));
}

#[test]
fn unsupported_combinations_are_errors() {
    let err = new_data_module(Issue::V3_0, SchemaKind::Fault, &bike(), "x", None, ("en", "US"), date()).unwrap_err();
    assert!(matches!(err, S1000dError::Unsupported { issue: Issue::V3_0, kind: SchemaKind::Fault }));
    assert!(err.to_string().contains("3.0"));
    assert!(new_data_module(Issue::V4_1, SchemaKind::Pm, &bike(), "x", None, ("en", "US"), date()).is_err());
}

#[test]
fn text_is_escaped_and_info_name_is_optional() {
    let xml =
        new_data_module(Issue::V4_1, SchemaKind::Descript, &bike(), "Nuts & <bolts>", None, ("en", "US"), date())
            .unwrap();
    assert!(xml.contains("<techName>Nuts &amp; &lt;bolts&gt;</techName>"));
    assert!(!xml.contains("infoName"));
    let doc = Document::from_str(&xml).unwrap();
    assert_eq!(DmTitle::from_document(&doc).unwrap(), DmTitle::new("Nuts & <bolts>", None));
}

#[test]
fn options_override_defaults() {
    let mut options = NewModuleOptions::new(date());
    options.issue_info = IssueInfo::new("002", "03");
    options.security_classification = "02".into();
    options.enterprise_code = "C0419".into();
    options.enterprise_name = "Acme & Sons".into();
    options.applic_text = "Model A only".into();
    options.brex = Some(Dmc::parse("DMC-BIKE-A-00-00-00-00A-022A-D").unwrap());
    let xml = new_data_module_with(Issue::V4_2, SchemaKind::Descript, &bike(), "T", None, ("fr", "CA"), &options)
        .unwrap();
    assert!(xml.contains(r#"<issueInfo issueNumber="002" inWork="03"/>"#));
    assert!(xml.contains(r#"securityClassification="02""#));
    assert!(xml.contains(r#"enterpriseCode="C0419"><enterpriseName>Acme &amp; Sons</enterpriseName>"#));
    assert!(xml.contains("Model A only"));
    assert!(xml.contains(r#"modelIdentCode="BIKE""#));
    assert!(xml.contains(r#"<language languageIsoCode="fr" countryIsoCode="CA"/>"#));
    Document::from_str(&xml).unwrap();
}

#[test]
fn learn_codes_are_written_for_modern_issues() {
    let dmc = Dmc::parse("DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A-H11A").unwrap();
    let xml = new_data_module(Issue::V4_1, SchemaKind::Descript, &dmc, "T", None, ("en", "US"), date()).unwrap();
    let doc = Document::from_str(&xml).unwrap();
    assert_eq!(Dmc::from_document(&doc).unwrap(), dmc);
}

#[test]
fn default_brex_by_issue() {
    assert_eq!(default_brex_dmc(Issue::V4_1).unwrap().to_string(), "DMC-S1000D-E-04-10-0301-00A-022A-D");
    assert!(default_brex_dmc(Issue::V3_0).is_none());
    let legacy = build(Issue::V3_0, SchemaKind::Descript);
    assert!(!legacy.contains("brexDmRef"));
}
