//! XML fixtures shared by the unit tests.

/// A modern (4.1) descriptive data module.
pub(crate) const MODERN_DESCRIPT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<dmodule xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.s1000d.org/S1000D_4-1/xml_schema_flat/descript.xsd">
  <identAndStatusSection>
    <dmAddress>
      <dmIdent>
        <dmCode modelIdentCode="S1000DBIKE" systemDiffCode="AAA" systemCode="D00" subSystemCode="0" subSubSystemCode="0" assyCode="00" disassyCode="00" disassyCodeVariant="AA" infoCode="041" infoCodeVariant="A" itemLocationCode="A"/>
        <language languageIsoCode="en" countryIsoCode="US"/>
        <issueInfo issueNumber="003" inWork="01"/>
      </dmIdent>
      <dmAddressItems>
        <issueDate year="2024" month="03" day="09"/>
        <dmTitle><techName>Bicycle</techName><infoName>Description</infoName></dmTitle>
      </dmAddressItems>
    </dmAddress>
    <dmStatus>
      <security securityClassification="01"/>
      <brexDmRef><dmRef><dmRefIdent><dmCode modelIdentCode="S1000D" systemDiffCode="E" systemCode="04" subSystemCode="1" subSubSystemCode="0" assyCode="0301" disassyCode="00" disassyCodeVariant="A" infoCode="022" infoCodeVariant="A" itemLocationCode="D"/></dmRefIdent></dmRef></brexDmRef>
    </dmStatus>
  </identAndStatusSection>
  <content>
    <description>
      <levelledPara id="lp1">
        <title>Intro</title>
        <para>See <dmRef><dmRefIdent><dmCode modelIdentCode="S1000DBIKE" systemDiffCode="AAA" systemCode="D00" subSystemCode="0" subSubSystemCode="0" assyCode="00" disassyCode="00" disassyCodeVariant="AA" infoCode="040" infoCodeVariant="A" itemLocationCode="D"/><issueInfo issueNumber="002" inWork="00"/><language languageIsoCode="en" countryIsoCode="US"/></dmRefIdent></dmRef> and <pmRef><pmRefIdent><pmCode modelIdentCode="S1000DBIKE" pmIssuer="C0419" pmNumber="00001" pmVolume="00"/></pmRefIdent></pmRef>.</para>
        <figure><graphic infoEntityIdent="ICN-S1000DBIKE-AAA-DA10000-0-U8025-00100-A-04-1"/></figure>
      </levelledPara>
    </description>
  </content>
</dmodule>"#;

/// A legacy (3.0) descriptive data module with a DOCTYPE.
pub(crate) const LEGACY_DESCRIPT: &str = r#"<?xml version="1.0"?>
<!DOCTYPE dmodule PUBLIC "-//S1000D//DTD Descriptive Data Module//EN" "descript.dtd" [
<!ENTITY ICN-BIKE-0001 SYSTEM "ICN-BIKE-0001.CGM" NDATA cgm>
<!NOTATION cgm SYSTEM "CGM">
]>
<dmodule>
  <idstatus>
    <dmaddres>
      <dmc><avee><modelic>BIKE</modelic><sdc>A</sdc><chapnum>12</chapnum><section>3</section><subsect>4</subsect><subject>56</subject><discode>00</discode><discodev>A</discodev><incode>040</incode><incodev>A</incodev><itemloc>A</itemloc></avee></dmc>
      <dmtitle><techname>Brakes</techname><infoname>Description</infoname></dmtitle>
      <issno issno="002" inwork="00" type="new"/>
      <issdate year="2001" month="05" day="31"/>
      <language language="en" country="US"/>
    </dmaddres>
    <dmstatus/>
  </idstatus>
  <content>
    <descript>
      <para0><title>Brakes</title>
        <para>Refer to <refdm><dmc><avee><modelic>BIKE</modelic><sdc>A</sdc><chapnum>12</chapnum><section>0</section><subsect>0</subsect><subject>00</subject><discode>00</discode><discodev>A</discodev><incode>520</incode><incodev>A</incodev><itemloc>A</itemloc></avee></dmc><issno issno="001" inwork="00"/></refdm>.</para>
        <graphic boardno="ICN-BIKE-0001"/>
      </para0>
    </descript>
  </content>
</dmodule>"#;

/// A modern publication module.
pub(crate) const MODERN_PM: &str = r#"<pm xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" xsi:noNamespaceSchemaLocation="http://www.s1000d.org/S1000D_4-1/xml_schema_flat/pm.xsd">
  <identAndStatusSection>
    <pmAddress>
      <pmIdent>
        <pmCode modelIdentCode="S1000DBIKE" pmIssuer="C0419" pmNumber="00001" pmVolume="00"/>
        <language languageIsoCode="en" countryIsoCode="US"/>
        <issueInfo issueNumber="001" inWork="00"/>
      </pmIdent>
      <pmAddressItems>
        <issueDate year="2024" month="01" day="02"/>
        <pmTitle>Bike manual</pmTitle>
      </pmAddressItems>
    </pmAddress>
  </identAndStatusSection>
  <content/>
</pm>"#;
