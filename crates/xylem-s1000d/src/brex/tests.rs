use super::*;

fn parse(xml: &str) -> Document {
    Document::from_str(xml).expect("fixture parses")
}

const BREX_XML: &str = r#"<dmodule>
<identAndStatusSection><dmAddress><dmIdent>
  <dmCode modelIdentCode="TEST" systemDiffCode="A" systemCode="00" subSystemCode="0" subSubSystemCode="0" assyCode="00" disassyCode="00" disassyCodeVariant="A" infoCode="022" infoCodeVariant="A" itemLocationCode="D"/>
</dmIdent></dmAddress></identAndStatusSection>
<content><brex>
 <contextRules>
  <structureObjectRuleGroup>
   <structureObjectRule id="no-font">
     <objectPath allowedObjectFlag="0">//font</objectPath>
     <objectUse>The font element must not be used.</objectUse>
   </structureObjectRule>
   <structureObjectRule>
     <objectPath allowedObjectFlag="1">/dmodule/content/description/levelledPara/title</objectPath>
     <objectUse>Every module needs a title.</objectUse>
   </structureObjectRule>
   <structureObjectRule>
     <objectPath allowedObjectFlag="1">//warning</objectPath>
     <objectUse>A warning is required.</objectUse>
   </structureObjectRule>
   <structureObjectRule id="sec">
     <objectPath allowedObjectFlag="2">//@securityClassification</objectPath>
     <objectUse>Unknown security classification.</objectUse>
     <objectValue valueForm="single" valueAllowed="01|02|03">Unclassified to confidential</objectValue>
   </structureObjectRule>
   <structureObjectRule id="level">
     <objectPath allowedObjectFlag="2">//@level</objectPath>
     <objectUse>Level out of range.</objectUse>
     <objectValue valueForm="range" valueAllowed="1~3|7">Levels</objectValue>
   </structureObjectRule>
   <structureObjectRule id="ident">
     <objectPath allowedObjectFlag="2">//para/@id</objectPath>
     <objectUse>Paragraph ids are lower case letters and digits.</objectUse>
     <objectValue valueForm="pattern" valueAllowed="[a-z]+[0-9]*"/>
   </structureObjectRule>
   <structureObjectRule id="emph">
     <objectPath allowedObjectFlag="0">//@emphasis</objectPath>
     <objectUse>Do not use the bad emphasis.</objectUse>
     <objectValue valueForm="single" valueAllowed="bad"/>
   </structureObjectRule>
   <structureObjectRule id="broken">
     <objectPath allowedObjectFlag="0">//[</objectPath>
   </structureObjectRule>
   <structureObjectRule id="letters">
     <objectPath allowedObjectFlag="2">//code</objectPath>
     <objectUse>Letter code.</objectUse>
     <objectValue valueForm="range" valueAllowed="a~c"/>
   </structureObjectRule>
  </structureObjectRuleGroup>
 </contextRules>
 <snsRules>
  <snsSystem><snsCode>D</snsCode><snsTitle>Vehicle</snsTitle>
    <snsSubSystem><snsCode>0</snsCode><snsTitle>General</snsTitle>
      <snsSubSubSystem><snsCode>1</snsCode><snsTitle>Frame</snsTitle></snsSubSubSystem>
    </snsSubSystem>
  </snsSystem>
 </snsRules>
 <notationRuleList>
  <notationRule><notationName>cgm</notationName><notationUse>Vector graphics</notationUse></notationRule>
  <notationRule><notationName>jpeg</notationName></notationRule>
 </notationRuleList>
</brex></content></dmodule>"#;

const LEGACY_BREX_XML: &str = r#"<dmodule><idstatus/><content><brex>
 <structrules>
  <objrule>
   <objpath allowed="0">//blink</objpath>
   <objuse>No blinking text.</objuse>
  </objrule>
  <objrule>
   <objpath allowed="2">//@width</objpath>
   <objuse>Width out of range.</objuse>
   <objval valtype="range" val1="10" val2="20"/>
   <objval valtype="single" val1="99"/>
  </objrule>
 </structrules>
</brex></content></dmodule>"#;

fn violations_for(rule_id: &str, v: &[BrexViolation]) -> Vec<BrexViolation> {
    v.iter().filter(|x| x.rule_id == rule_id).cloned().collect()
}

#[test]
fn parses_modern_brex() {
    let brex = Brex::from_document(&parse(BREX_XML));
    assert_eq!(brex.rules.len(), 9);
    assert_eq!(brex.dmc.as_ref().unwrap().to_string(), "DMC-TEST-A-00-00-00-00A-022A-D");

    let font = &brex.rules[0];
    assert_eq!(font.id, "no-font");
    assert_eq!(font.path, "//font");
    assert_eq!(font.flag, AllowedFlag::Prohibited);
    assert_eq!(font.use_text, "The font element must not be used.");
    assert!(font.values.is_empty());

    assert_eq!(brex.rules[1].id, "rule-2", "rules without id are numbered by position");
    assert_eq!(brex.rules[1].flag, AllowedFlag::Required);

    let sec = &brex.rules[3];
    assert_eq!(sec.flag, AllowedFlag::Optional);
    assert_eq!(sec.values.len(), 1);
    assert_eq!(sec.values[0].form, ValueForm::Single);
    assert_eq!(sec.values[0].allowed, "01|02|03");
    assert_eq!(sec.values[0].description, "Unclassified to confidential");

    assert_eq!(brex.rules[4].values[0].form, ValueForm::Range);
    assert_eq!(brex.rules[5].values[0].form, ValueForm::Pattern);
}

#[test]
fn parses_sns_and_notation_rules() {
    let brex = Brex::from_document(&parse(BREX_XML));
    let sns: Vec<_> = brex.sns_rules.iter().map(|r| (r.code.as_str(), r.title.as_str(), r.level.as_str())).collect();
    assert_eq!(
        sns,
        [("D", "Vehicle", "snsSystem"), ("D-0", "General", "snsSubSystem"), ("D-0-1", "Frame", "snsSubSubSystem")]
    );
    assert_eq!(brex.notation_rules.len(), 2);
    assert_eq!(brex.notation_rules[0].name, "cgm");
    assert_eq!(brex.notation_rules[0].description, "Vector graphics");
    assert_eq!(brex.notation_rules[1].name, "jpeg");
}

#[test]
fn parses_legacy_brex() {
    let brex = Brex::from_document(&parse(LEGACY_BREX_XML));
    assert_eq!(brex.rules.len(), 2);
    assert_eq!(brex.rules[0].flag, AllowedFlag::Prohibited);
    assert_eq!(brex.rules[0].path, "//blink");
    assert_eq!(brex.rules[1].values.len(), 2);
    assert_eq!(brex.rules[1].values[0].form, ValueForm::Range);
    assert_eq!(brex.rules[1].values[0].allowed, "10~20");
    assert_eq!(brex.rules[1].values[1].allowed, "99");

    let ok = parse(r#"<dmodule><content><para width="15"/><para width="99"/></content></dmodule>"#);
    assert!(check_brex(&ok, &brex).is_empty());
    let bad = parse(r#"<dmodule><content><blink/><para width="25"/></content></dmodule>"#);
    let v = check_brex(&bad, &brex);
    assert_eq!(v.len(), 2, "{v:?}");
    assert_eq!(v[0].message, "No blinking text. (/dmodule/content/blink)");
    assert!(v[1].message.contains("width"), "{}", v[1].message);
}

#[test]
fn clean_document_has_no_violations_except_unevaluable_rules() {
    let brex = Brex::from_document(&parse(BREX_XML));
    let doc = parse(
        r#"<dmodule><content><description><levelledPara securityClassification="02"><title>T</title>
           <warning/><para id="abc12" level="2"/><para id="x"/><code>b</code></levelledPara></description></content></dmodule>"#,
    );
    let v = check_brex(&doc, &brex);
    assert_eq!(v.len(), 1, "{v:?}");
    assert_eq!(v[0].rule_id, "broken");
    assert_eq!(v[0].severity, Severity::Warning);
    assert!(v[0].node.is_none());
}

#[test]
fn prohibited_objects_are_flagged() {
    let brex = Brex::from_document(&parse(BREX_XML));
    let doc = parse(r#"<dmodule><content><description><font/><levelledPara><title/><warning/><font/></levelledPara></description></content></dmodule>"#);
    let v = violations_for("no-font", &check_brex(&doc, &brex));
    assert_eq!(v.len(), 2);
    assert!(v.iter().all(|x| x.severity == Severity::Error && x.node.is_some()));
    assert_eq!(doc.name(v[0].node.unwrap()), Some("font"));
    assert!(v[0].message.starts_with("The font element must not be used."));
}

#[test]
fn required_objects_must_exist() {
    let brex = Brex::from_document(&parse(BREX_XML));
    let doc = parse(r#"<dmodule><content><description><levelledPara/></description></content></dmodule>"#);
    let all = check_brex(&doc, &brex);
    let title = violations_for("rule-2", &all);
    assert_eq!(title.len(), 1);
    assert_eq!(title[0].node, None);
    assert_eq!(title[0].message, "Every module needs a title.");
    assert_eq!(violations_for("rule-3", &all).len(), 1);
}

#[test]
fn value_sets_are_enforced_on_attributes() {
    let brex = Brex::from_document(&parse(BREX_XML));
    let doc = parse(
        r#"<dmodule><content><description><levelledPara securityClassification="06"><title/><warning/>
           <para id="Bad_Id" level="5"/><para id="ok7" level="7"/><para level="0"/><code>d</code><code>a</code></levelledPara>
           <para emphasis="bad"/><para emphasis="good"/></description></content></dmodule>"#,
    );
    let all = check_brex(&doc, &brex);

    let sec = violations_for("sec", &all);
    assert_eq!(sec.len(), 1);
    assert_eq!(doc.name(sec[0].node.unwrap()), Some("levelledPara"));
    assert!(sec[0].message.contains("securityClassification"), "{}", sec[0].message);
    assert!(sec[0].message.contains("value '06'"), "{}", sec[0].message);

    assert_eq!(violations_for("level", &all).len(), 2, "5 and 0 are outside 1~3|7");
    let ident = violations_for("ident", &all);
    assert_eq!(ident.len(), 1);
    assert!(ident[0].message.contains("Bad_Id"));
    assert_eq!(violations_for("letters", &all).len(), 1, "only 'd' is outside a~c");

    let emph = violations_for("emph", &all);
    assert_eq!(emph.len(), 1, "only the prohibited value is reported");
    assert!(emph[0].message.contains("value 'bad'"));
}

#[test]
fn invalid_rules_are_reported_as_warnings() {
    let xml = r#"<dmodule><content><brex><contextRules><structureObjectRuleGroup>
      <structureObjectRule id="a"><objectPath allowedObjectFlag="2">//x</objectPath>
        <objectValue valueForm="pattern" valueAllowed="(oops"/></structureObjectRule>
      <structureObjectRule id="b"><objectPath allowedObjectFlag="2">count(//x)</objectPath></structureObjectRule>
    </structureObjectRuleGroup></contextRules></brex></content></dmodule>"#;
    let brex = Brex::from_document(&parse(xml));
    let v = check_brex(&parse("<dmodule><x>1</x></dmodule>"), &brex);
    let a = violations_for("a", &v);
    assert_eq!(a.len(), 1, "{v:?}");
    assert_eq!(a[0].severity, Severity::Warning);
    assert!(a[0].message.contains("invalid pattern"));
    let b = violations_for("b", &v);
    assert_eq!(b.len(), 1);
    assert!(b[0].message.contains("node-set"), "{}", b[0].message);
}

#[test]
fn value_matching_forms() {
    let single = ObjectValue { form: ValueForm::Single, allowed: "A | B|C".into(), description: String::new() };
    assert!(single.allows("B"));
    assert!(single.allows(" C "));
    assert!(!single.allows("D"));

    let numeric = ObjectValue { form: ValueForm::Range, allowed: "001~010|100".into(), description: String::new() };
    assert!(numeric.allows("5"), "ranges compare numerically");
    assert!(numeric.allows("010"));
    assert!(!numeric.allows("011"));
    assert!(numeric.allows("100"));
    assert!(!numeric.allows("x"));

    let letters = ObjectValue { form: ValueForm::Range, allowed: "a~c".into(), description: String::new() };
    assert!(letters.allows("b"));
    assert!(!letters.allows("d"));

    let pattern = ObjectValue { form: ValueForm::Pattern, allowed: "[A-Z]{2}[0-9]{2}|none".into(), description: String::new() };
    assert!(pattern.allows("AB12"));
    assert!(pattern.allows("none"));
    assert!(!pattern.allows("AB123"));
    let broken = ObjectValue { form: ValueForm::Pattern, allowed: "(".into(), description: String::new() };
    assert!(!broken.allows("x"));
    assert!(broken.matcher().is_err());
}

#[test]
fn merge_combines_rule_sets() {
    let mut base = Brex::from_document(&parse(LEGACY_BREX_XML));
    let other = Brex::from_document(&parse(LEGACY_BREX_XML));
    base.merge(other);
    assert_eq!(base.rules.len(), 4);
    let ids: Vec<_> = base.rules.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, ["rule-1", "rule-2", "rule-3", "rule-4"]);
}

#[test]
fn loads_from_a_file() {
    let path = std::env::temp_dir().join(format!("xylem-s1000d-brex-{}.xml", std::process::id()));
    std::fs::write(&path, BREX_XML).unwrap();
    let loaded = Brex::load(&path);
    let _ = std::fs::remove_file(&path);
    assert_eq!(loaded.unwrap().rules.len(), 9);
    assert!(matches!(Brex::load(path.with_extension("missing")), Err(S1000dError::Io(_))));

    let bad = std::env::temp_dir().join(format!("xylem-s1000d-brex-bad-{}.xml", std::process::id()));
    std::fs::write(&bad, "<unclosed>").unwrap();
    let err = Brex::load(&bad);
    let _ = std::fs::remove_file(&bad);
    assert!(matches!(err, Err(S1000dError::Xml(_))));
}
