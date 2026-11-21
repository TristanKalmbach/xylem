use super::*;
use crate::testdata::{LEGACY_DESCRIPT, MODERN_DESCRIPT, MODERN_PM};

fn parse(xml: &str) -> Document {
    Document::from_str(xml).expect("fixture parses")
}

const BIKE: &str = "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A";

#[test]
fn dmc_parse_and_format_round_trip() {
    let dmc = Dmc::parse(BIKE).unwrap();
    assert_eq!(dmc.model_ident_code, "S1000DBIKE");
    assert_eq!(dmc.system_diff_code, "AAA");
    assert_eq!(dmc.system_code, "D00");
    assert_eq!(
        (
            dmc.sub_system_code.as_str(),
            dmc.sub_sub_system_code.as_str()
        ),
        ("0", "0")
    );
    assert_eq!(dmc.assy_code, "00");
    assert_eq!(
        (dmc.disassy_code.as_str(), dmc.disassy_code_variant.as_str()),
        ("00", "AA")
    );
    assert_eq!(
        (dmc.info_code.as_str(), dmc.info_code_variant.as_str()),
        ("041", "A")
    );
    assert_eq!(dmc.item_location_code, "A");
    assert_eq!(dmc.learn_code, None);
    assert_eq!(dmc.to_string(), BIKE);
    assert!(dmc.validate().is_empty(), "{:?}", dmc.validate());
    assert_eq!(BIKE.parse::<Dmc>().unwrap(), dmc);
    assert_eq!(Dmc::parse(&BIKE[4..]).unwrap(), dmc, "prefix is optional");
    assert_eq!(
        Dmc::parse("dmc-S1000DBIKE-AAA-D00-00-00-00AA-041A-A").unwrap(),
        dmc
    );
}

#[test]
fn dmc_with_learn_codes() {
    let dmc = Dmc::parse("DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A-H11A").unwrap();
    assert_eq!(dmc.learn_code.as_deref(), Some("H11"));
    assert_eq!(dmc.learn_event_code.as_deref(), Some("A"));
    assert_eq!(
        dmc.to_string(),
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A-H11A"
    );
}

#[test]
fn dmc_rejects_malformed_codes() {
    for bad in [
        "",
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A",
        "DMC-S1000DBIKE-AAA-D00-0-00-00AA-041A-A",
        "DMC-S1000DBIKE-AAA-D00-00-00-00-041A-A",
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-04A-A",
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A-H1",
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A!",
        "DMC-S1000DBIKE--D00-00-00-00AA-041A-A",
    ] {
        assert!(Dmc::parse(bad).is_err(), "{bad}");
    }
}

#[test]
fn dmc_from_filename() {
    let dmc =
        Dmc::from_filename("C:\\csdb\\DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A_003-01_en-US.XML")
            .unwrap();
    assert_eq!(dmc.to_string(), BIKE);
}

#[test]
fn dmc_validation_reports_problems() {
    let mut dmc = Dmc::parse(BIKE).unwrap();
    dmc.model_ident_code = "s".into();
    dmc.info_code = "04".into();
    let problems = dmc.validate();
    assert_eq!(problems.len(), 2, "{problems:?}");
}

#[test]
fn modern_document_ident() {
    let doc = parse(MODERN_DESCRIPT);
    let ident = DmIdent::from_document(&doc).unwrap();
    // The brexDmRef inside dmStatus must not be mistaken for the module's own code.
    assert_eq!(ident.dmc.to_string(), BIKE);
    assert_eq!(ident.issue_info, Some(IssueInfo::new("003", "01")));
    assert_eq!(ident.language, Some(Language::new("en", "US")));
    assert_eq!(
        ident
            .issue_date
            .as_ref()
            .map(ToString::to_string)
            .as_deref(),
        Some("2024-03-09")
    );
    assert_eq!(
        ident.title,
        Some(DmTitle::new("Bicycle", Some("Description")))
    );
    assert_eq!(
        ident.csdb_filename().unwrap(),
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A_003-01_en-US.XML"
    );
    assert!(Pmc::from_document(&doc).is_none());
}

#[test]
fn legacy_document_ident() {
    let doc = parse(LEGACY_DESCRIPT);
    let ident = DmIdent::from_document(&doc).unwrap();
    assert_eq!(ident.dmc.to_string(), "DMC-BIKE-A-12-34-56-00A-040A-A");
    assert_eq!(ident.dmc.system_code, "12");
    assert_eq!(ident.dmc.sub_system_code, "3");
    assert_eq!(ident.dmc.sub_sub_system_code, "4");
    assert_eq!(ident.dmc.assy_code, "56");
    assert_eq!(ident.issue_info, Some(IssueInfo::new("002", "00")));
    assert_eq!(ident.language, Some(Language::new("en", "US")));
    assert_eq!(
        ident.title,
        Some(DmTitle::new("Brakes", Some("Description")))
    );
    assert_eq!(
        ident.csdb_filename().unwrap(),
        "DMC-BIKE-A-12-34-56-00A-040A-A_002-00_en-US.XML"
    );
}

#[test]
fn modern_writes_update_attributes_and_round_trip() {
    let mut doc = parse(MODERN_DESCRIPT);
    let mut dmc = Dmc::from_document(&doc).unwrap();
    dmc.info_code = "040".into();
    dmc.learn_code = Some("H11".into());
    dmc.learn_event_code = Some("A".into());
    dmc.write_to_document(&mut doc).unwrap();
    IssueInfo::new("004", "00")
        .write_to_document(&mut doc)
        .unwrap();
    Language::new("fr", "CA")
        .write_to_document(&mut doc)
        .unwrap();
    IssueDate::new(2025, 1, 5)
        .write_to_document(&mut doc)
        .unwrap();
    DmTitle::new("Frein", None)
        .write_to_document(&mut doc)
        .unwrap();

    let reparsed = parse(&doc.to_xml());
    let ident = DmIdent::from_document(&reparsed).unwrap();
    assert_eq!(ident.dmc, dmc);
    assert_eq!(ident.issue_info, Some(IssueInfo::new("004", "00")));
    assert_eq!(ident.language, Some(Language::new("fr", "CA")));
    assert_eq!(ident.issue_date.unwrap().to_string(), "2025-01-05");
    assert_eq!(ident.title, Some(DmTitle::new("Frein", None)));

    // Clearing the learn codes removes the attributes again.
    let mut plain = ident.dmc.clone();
    plain.learn_code = None;
    plain.learn_event_code = None;
    plain.write_to_document(&mut doc).unwrap();
    assert!(!doc.to_xml().contains("learnCode"));
}

#[test]
fn legacy_writes_update_elements() {
    let mut doc = parse(LEGACY_DESCRIPT);
    let mut dmc = Dmc::from_document(&doc).unwrap();
    dmc.model_ident_code = "TRIKE".into();
    dmc.write_to_document(&mut doc).unwrap();
    IssueInfo::new("005", "02")
        .write_to_document(&mut doc)
        .unwrap();
    Language::new("de", "DE")
        .write_to_document(&mut doc)
        .unwrap();
    DmTitle::new("Bremsen", Some("Beschreibung"))
        .write_to_document(&mut doc)
        .unwrap();
    DmTitle::new("Bremsen", None)
        .write_to_document(&mut doc)
        .unwrap();
    let reparsed = parse(&doc.to_xml());
    let ident = DmIdent::from_document(&reparsed).unwrap();
    assert_eq!(ident.dmc.model_ident_code, "TRIKE");
    assert_eq!(ident.issue_info, Some(IssueInfo::new("005", "02")));
    assert_eq!(ident.language, Some(Language::new("de", "DE")));
    assert_eq!(ident.title, Some(DmTitle::new("Bremsen", None)));
    assert!(doc.to_xml().contains(r#"language="de" country="DE""#));
}

#[test]
fn publication_module_ident() {
    let doc = parse(MODERN_PM);
    let pm = PmIdent::from_document(&doc).unwrap();
    assert_eq!(pm.pmc.to_string(), "PMC-S1000DBIKE-C0419-00001-00");
    assert_eq!(
        pm.csdb_filename().unwrap(),
        "PMC-S1000DBIKE-C0419-00001-00_001-00_en-US.XML"
    );
    assert_eq!(pm.title.as_deref(), Some("Bike manual"));
    assert!(Dmc::from_document(&doc).is_none());

    let mut doc = doc;
    Pmc::parse("PMC-S1000DBIKE-C0419-00002-01")
        .unwrap()
        .write_to_document(&mut doc)
        .unwrap();
    assert_eq!(Pmc::from_document(&doc).unwrap().pm_number, "00002");
    IssueInfo::new("002", "00")
        .write_to_document(&mut doc)
        .unwrap();
    assert_eq!(
        IssueInfo::from_document(&doc).unwrap().to_string(),
        "002-00"
    );
}

#[test]
fn pmc_parsing() {
    assert!(Pmc::parse("PMC-A-B-C").is_err());
    assert_eq!(
        Pmc::parse("S1-ABCDE-00001-00").unwrap().to_string(),
        "PMC-S1-ABCDE-00001-00"
    );
}

#[test]
fn writes_fail_without_structure() {
    let mut doc = parse("<dmodule><content/></dmodule>");
    assert!(
        Dmc::parse(BIKE)
            .unwrap()
            .write_to_document(&mut doc)
            .is_err()
    );
    assert!(DmIdent::from_document(&doc).is_none());
}

#[test]
fn issue_info_and_language_text() {
    assert_eq!(
        IssueInfo::parse("001-00"),
        Some(IssueInfo::new("001", "00"))
    );
    assert_eq!(IssueInfo::parse("1-x"), None);
    assert_eq!(Language::parse("en-GB"), Some(Language::new("en", "GB")));
    assert_eq!(Language::parse("en"), None);
}

#[test]
fn civil_dates_from_unix_days() {
    assert_eq!(IssueDate::from_unix_days(0).to_string(), "1970-01-01");
    assert_eq!(IssueDate::from_unix_days(19_782).to_string(), "2024-02-29");
    assert_eq!(IssueDate::from_unix_days(20_443).to_string(), "2025-12-21");
    let today = IssueDate::today();
    assert!(today.year.as_str() >= "2025");
}
