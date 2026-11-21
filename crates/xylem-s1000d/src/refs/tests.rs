use super::*;
use crate::testdata::{LEGACY_DESCRIPT, MODERN_DESCRIPT};

fn parse(xml: &str) -> Document {
    Document::from_str(xml).expect("fixture parses")
}

#[test]
fn modern_dm_refs() {
    let doc = parse(MODERN_DESCRIPT);
    let refs = find_dm_refs(&doc);
    // The BREX reference in dmStatus comes first, then the inline reference.
    assert_eq!(refs.len(), 2);
    assert_eq!(
        refs[0].dmc.to_string(),
        "DMC-S1000D-E-04-10-0301-00A-022A-D"
    );
    assert_eq!(refs[0].issue_info, None);
    let inline = &refs[1];
    assert_eq!(
        inline.dmc.to_string(),
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-040A-D"
    );
    assert_eq!(inline.issue_info.as_ref().unwrap().to_string(), "002-00");
    assert_eq!(inline.language.as_ref().unwrap().to_string(), "en-US");
    assert_eq!(doc.name(inline.node), Some("dmRef"));
    assert_eq!(
        inline.filename_pattern(),
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-040A-D_002-00_en-US.XML"
    );
    assert_eq!(
        inline.latest_filename_pattern(),
        "DMC-S1000DBIKE-AAA-D00-00-00-00AA-040A-D_*_en-US.XML"
    );
    assert_eq!(
        refs[0].filename_pattern(),
        "DMC-S1000D-E-04-10-0301-00A-022A-D_*_*.XML"
    );
}

#[test]
fn legacy_dm_refs() {
    let doc = parse(LEGACY_DESCRIPT);
    let refs = find_dm_refs(&doc);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].dmc.to_string(), "DMC-BIKE-A-12-00-00-00A-520A-A");
    assert_eq!(refs[0].issue_info.as_ref().unwrap().to_string(), "001-00");
    assert_eq!(
        refs[0].filename_pattern(),
        "DMC-BIKE-A-12-00-00-00A-520A-A_001-00_*.XML"
    );
}

#[test]
fn pm_refs() {
    let doc = parse(MODERN_DESCRIPT);
    let refs = find_pm_refs(&doc);
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].pmc.to_string(), "PMC-S1000DBIKE-C0419-00001-00");
    assert_eq!(
        refs[0].filename_pattern(),
        "PMC-S1000DBIKE-C0419-00001-00_*_*.XML"
    );
}

#[test]
fn glob_matching() {
    assert!(glob_match("DMC-A_*_en-US.XML", "dmc-a_001-00_EN-us.xml"));
    assert!(glob_match("A?C", "abc"));
    assert!(glob_match("*", ""));
    assert!(glob_match("a*b*c", "aXXbYYc"));
    assert!(!glob_match("a*b*c", "aXXbYY"));
    assert!(!glob_match("abc", "abcd"));
    assert!(!glob_match("a?", "a"));
    assert!(glob_match("a**", "a"));
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("xylem-s1000d-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn resolves_latest_issue_in_folder() {
    let tmp = TempDir::new("resolve");
    let code = "DMC-S1000DBIKE-AAA-D00-00-00-00AA-040A-D";
    for name in [
        format!("{code}_001-00_en-US.XML"),
        format!("{code}_002-00_en-US.XML"),
        format!("{code}_010-00_en-US.XML"),
        format!("{code}_010-01_en-US.XML"),
        format!("{code}_011-00_fr-CA.XML"),
        "DMC-OTHER-AAA-D00-00-00-00AA-040A-D_001-00_en-US.XML".to_string(),
    ] {
        std::fs::write(tmp.0.join(name), "<dmodule/>").unwrap();
    }
    let doc = parse(MODERN_DESCRIPT);
    let inline = find_dm_refs(&doc).remove(1);

    let latest = inline.find_latest_in_dir(&tmp.0).unwrap().unwrap();
    assert!(
        latest.ends_with(format!("{code}_010-01_en-US.XML")),
        "{latest:?}"
    );

    // The reference names issue 002 exactly.
    let exact = inline.find_in_dir(&tmp.0).unwrap();
    assert_eq!(exact.len(), 1);
    assert!(exact[0].ends_with(format!("{code}_002-00_en-US.XML")));

    let mut any_language = inline.clone();
    any_language.language = None;
    any_language.issue_info = None;
    let all = any_language.find_in_dir(&tmp.0).unwrap();
    assert_eq!(all.len(), 5);
    assert!(
        all.last()
            .unwrap()
            .ends_with(format!("{code}_011-00_fr-CA.XML"))
    );

    let mut missing = inline;
    missing.dmc.info_code = "999".into();
    assert_eq!(missing.find_latest_in_dir(&tmp.0).unwrap(), None);
    assert!(missing.find_latest_in_dir(&tmp.0.join("nope")).is_err());
}

#[test]
fn icn_refs_modern_and_legacy() {
    let modern = parse(MODERN_DESCRIPT);
    let icns = find_icn_refs(&modern);
    assert_eq!(icns.len(), 1);
    assert_eq!(
        icns[0].icn,
        "ICN-S1000DBIKE-AAA-DA10000-0-U8025-00100-A-04-1"
    );
    assert_eq!(icns[0].kind, IcnRefKind::InfoEntityIdent);
    assert_eq!(icns[0].file, None);

    let legacy = parse(LEGACY_DESCRIPT);
    let icns = find_icn_refs(&legacy);
    assert_eq!(icns.len(), 1);
    assert_eq!(icns[0].kind, IcnRefKind::BoardNo);
    assert_eq!(icns[0].file.as_deref(), Some("ICN-BIKE-0001.CGM"));
    assert_eq!(icns[0].notation.as_deref(), Some("cgm"));
    assert_eq!(legacy.name(icns[0].node), Some("graphic"));
}

#[test]
fn entity_declaration_parsing() {
    let subset = r#"
<!-- <!ENTITY commented SYSTEM "no.cgm"> -->
<!ENTITY ICN-1 SYSTEM "ICN-1.CGM" NDATA cgm>
<!ENTITY ICN-2 PUBLIC "-//pub//" "ICN-2.TIF" NDATA tif>
<!ENTITY % param "ignored">
<!ENTITY text "internal > value">
<!NOTATION cgm SYSTEM "CGM">
<!ENTITY ICN-3 SYSTEM 'a b.png'>
"#;
    let decls = parse_entity_declarations(subset);
    let names: Vec<_> = decls.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["ICN-1", "ICN-2", "text", "ICN-3"]);
    assert_eq!(decls[0].system_id.as_deref(), Some("ICN-1.CGM"));
    assert_eq!(decls[0].notation.as_deref(), Some("cgm"));
    assert_eq!(decls[1].public_id.as_deref(), Some("-//pub//"));
    assert_eq!(decls[1].system_id.as_deref(), Some("ICN-2.TIF"));
    assert_eq!(decls[2].system_id, None);
    assert_eq!(decls[3].system_id.as_deref(), Some("a b.png"));
    assert_eq!(decls[3].notation, None);
}
