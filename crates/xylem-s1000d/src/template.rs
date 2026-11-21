//! Skeletons for new data modules.

use xylem_dom::{escape_attr, escape_text};

use crate::error::S1000dError;
use crate::ident::{Dmc, IssueDate, IssueInfo};
use crate::issue::{Issue, Markup, SchemaKind, available_kinds};

/// Tunable parts of a new data module.
#[derive(Debug, Clone)]
pub struct NewModuleOptions {
    /// Goes into `issueDate`.
    pub date: IssueDate,
    /// Issue and in-work number of the new module (default `001-00`).
    pub issue_info: IssueInfo,
    /// `securityClassification` (default `01`).
    pub security_classification: String,
    /// Enterprise (CAGE) code for responsible partner company and originator.
    pub enterprise_code: String,
    pub enterprise_name: String,
    /// Text of the applicability statement (default `All`).
    pub applic_text: String,
    /// The BREX referenced from the status section; `None` selects [`default_brex_dmc`].
    pub brex: Option<Dmc>,
}

impl NewModuleOptions {
    pub fn new(date: IssueDate) -> Self {
        Self {
            date,
            issue_info: IssueInfo::new("001", "00"),
            security_classification: "01".to_string(),
            enterprise_code: "00000".to_string(),
            enterprise_name: "Placeholder".to_string(),
            applic_text: "All".to_string(),
            brex: None,
        }
    }
}

/// The data module code of the default S1000D BREX for `issue` (modern issues only).
///
/// The system difference code letter differs between issues; verify it against the
/// BREX actually shipped with your S1000D package and override through
/// [`NewModuleOptions::brex`] if needed.
pub fn default_brex_dmc(issue: Issue) -> Option<Dmc> {
    let sdc = match issue {
        Issue::V4_0 | Issue::V4_0_1 => "A",
        Issue::V4_1 => "E",
        Issue::V4_2 => "F",
        Issue::V5_0 => "G",
        Issue::V6_0 => "H",
        _ => return None,
    };
    Some(Dmc {
        model_ident_code: "S1000D".into(),
        system_diff_code: sdc.into(),
        system_code: "04".into(),
        sub_system_code: "1".into(),
        sub_sub_system_code: "0".into(),
        assy_code: "0301".into(),
        disassy_code: "00".into(),
        disassy_code_variant: "A".into(),
        info_code: "022".into(),
        info_code_variant: "A".into(),
        item_location_code: "D".into(),
        learn_code: None,
        learn_event_code: None,
    })
}

/// Creates a new data module skeleton with default options.
///
/// `lang` is `(languageIsoCode, countryIsoCode)`, for example `("en", "US")`;
/// `date` becomes the `issueDate`. Fails with [`S1000dError::Unsupported`] when
/// `kind` is not in [`available_kinds`]`(issue)`.
pub fn new_data_module(
    issue: Issue,
    kind: SchemaKind,
    dmc: &Dmc,
    tech_name: &str,
    info_name: Option<&str>,
    lang: (&str, &str),
    date: IssueDate,
) -> Result<String, S1000dError> {
    new_data_module_with(
        issue,
        kind,
        dmc,
        tech_name,
        info_name,
        lang,
        &NewModuleOptions::new(date),
    )
}

/// Like [`new_data_module`] with explicit [`NewModuleOptions`].
pub fn new_data_module_with(
    issue: Issue,
    kind: SchemaKind,
    dmc: &Dmc,
    tech_name: &str,
    info_name: Option<&str>,
    lang: (&str, &str),
    options: &NewModuleOptions,
) -> Result<String, S1000dError> {
    if !available_kinds(issue).contains(&kind) {
        return Err(S1000dError::Unsupported { issue, kind });
    }
    let parts = Parts {
        issue,
        kind,
        dmc,
        tech_name,
        info_name,
        lang,
        options,
    };
    Ok(match issue.markup() {
        Markup::Modern => parts.modern(),
        Markup::Legacy => parts.legacy(),
    })
}

struct Parts<'a> {
    issue: Issue,
    kind: SchemaKind,
    dmc: &'a Dmc,
    tech_name: &'a str,
    info_name: Option<&'a str>,
    lang: (&'a str, &'a str),
    options: &'a NewModuleOptions,
}

fn text(s: &str) -> String {
    let mut out = String::new();
    escape_text(s, &mut out);
    out
}

fn attr_value(s: &str) -> String {
    let mut out = String::new();
    escape_attr(s, &mut out);
    out
}

impl Parts<'_> {
    fn dm_code_element(dmc: &Dmc) -> String {
        let mut s = format!(
            r#"<dmCode modelIdentCode="{}" systemDiffCode="{}" systemCode="{}" subSystemCode="{}" subSubSystemCode="{}" assyCode="{}" disassyCode="{}" disassyCodeVariant="{}" infoCode="{}" infoCodeVariant="{}" itemLocationCode="{}""#,
            attr_value(&dmc.model_ident_code),
            attr_value(&dmc.system_diff_code),
            attr_value(&dmc.system_code),
            attr_value(&dmc.sub_system_code),
            attr_value(&dmc.sub_sub_system_code),
            attr_value(&dmc.assy_code),
            attr_value(&dmc.disassy_code),
            attr_value(&dmc.disassy_code_variant),
            attr_value(&dmc.info_code),
            attr_value(&dmc.info_code_variant),
            attr_value(&dmc.item_location_code),
        );
        if let Some(learn) = &dmc.learn_code {
            s.push_str(&format!(r#" learnCode="{}""#, attr_value(learn)));
        }
        if let Some(event) = &dmc.learn_event_code {
            s.push_str(&format!(r#" learnEventCode="{}""#, attr_value(event)));
        }
        s.push_str("/>");
        s
    }

    // ------------------------------------------------------------ modern markup

    fn modern(&self) -> String {
        let o = self.options;
        let title = {
            let mut t = format!("<techName>{}</techName>", text(self.tech_name));
            if let Some(info) = self.info_name {
                t.push_str(&format!("<infoName>{}</infoName>", text(info)));
            }
            t
        };
        let brex = o.brex.clone().or_else(|| default_brex_dmc(self.issue));
        let brex_ref = brex
            .map(|b| {
                format!(
                    "<brexDmRef><dmRef><dmRefIdent>{}</dmRefIdent></dmRef></brexDmRef>\n",
                    Self::dm_code_element(&b)
                )
            })
            .unwrap_or_default();
        let enterprise = |element: &str| {
            format!(
                r#"<{element} enterpriseCode="{}"><enterpriseName>{}</enterpriseName></{element}>"#,
                attr_value(&o.enterprise_code),
                text(&o.enterprise_name)
            )
        };
        let status = format!(
            "<dmStatus>\n\
             <security securityClassification=\"{security}\"/>\n\
             {partner}\n\
             {originator}\n\
             <applic><displayText><simplePara>{applic}</simplePara></displayText></applic>\n\
             {brex_ref}\
             <qualityAssurance><unverified/></qualityAssurance>\n\
             </dmStatus>",
            security = attr_value(&o.security_classification),
            partner = enterprise("responsiblePartnerCompany"),
            originator = enterprise("originator"),
            applic = text(&o.applic_text),
        );
        let body = format!(
            "<dmodule xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"{url}\">\n\
             <identAndStatusSection>\n\
             <dmAddress>\n\
             <dmIdent>\n\
             {code}\n\
             <language languageIsoCode=\"{lang}\" countryIsoCode=\"{country}\"/>\n\
             <issueInfo issueNumber=\"{issue_no}\" inWork=\"{in_work}\"/>\n\
             </dmIdent>\n\
             <dmAddressItems>\n\
             <issueDate year=\"{y}\" month=\"{m}\" day=\"{d}\"/>\n\
             <dmTitle>{title}</dmTitle>\n\
             </dmAddressItems>\n\
             </dmAddress>\n\
             {status}\n\
             </identAndStatusSection>\n\
             <content>\n\
             {content}\n\
             </content>\n\
             </dmodule>",
            url = self.schema_url(),
            code = Self::dm_code_element(self.dmc),
            lang = attr_value(self.lang.0),
            country = attr_value(self.lang.1),
            issue_no = attr_value(&o.issue_info.issue_number),
            in_work = attr_value(&o.issue_info.in_work),
            y = attr_value(&o.date.year),
            m = attr_value(&o.date.month),
            d = attr_value(&o.date.day),
            content = self.modern_content(),
        );
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n{}\n",
            Self::reindent(&body)
        )
    }

    /// The raw template is flat; give it a readable nesting by element depth.
    fn reindent(flat: &str) -> String {
        let mut depth = 0usize;
        let mut out = Vec::new();
        for line in flat.lines() {
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if t.starts_with("</") {
                depth = depth.saturating_sub(1);
            }
            out.push(format!("{}{t}", "  ".repeat(depth)));
            // A line that opens an element without closing it nests what follows.
            if t.starts_with('<')
                && !t.starts_with("</")
                && !t.starts_with("<?")
                && !closes_itself(t)
            {
                depth += 1;
            }
        }
        out.join("\n")
    }

    fn schema_url(&self) -> String {
        format!(
            "http://www.s1000d.org/S1000D_{}/xml_schema_flat/{}.xsd",
            self.issue.schema_token(),
            self.kind.stem()
        )
    }

    fn modern_content(&self) -> &'static str {
        match self.kind {
            SchemaKind::Descript => {
                "<description>\n\
                 <levelledPara>\n\
                 <title>Placeholder title</title>\n\
                 <para>Placeholder paragraph.</para>\n\
                 </levelledPara>\n\
                 </description>"
            }
            SchemaKind::Proced => {
                "<procedure>\n\
                 <preliminaryRqmts>\n\
                 <reqCondGroup>\n\
                 <noConds/>\n\
                 </reqCondGroup>\n\
                 <reqSupportEquips>\n\
                 <noSupportEquips/>\n\
                 </reqSupportEquips>\n\
                 <reqSupplies>\n\
                 <noSupplies/>\n\
                 </reqSupplies>\n\
                 <reqSpares>\n\
                 <noSpares/>\n\
                 </reqSpares>\n\
                 <reqSafety>\n\
                 <noSafety/>\n\
                 </reqSafety>\n\
                 </preliminaryRqmts>\n\
                 <mainProcedure>\n\
                 <proceduralStep>\n\
                 <para>Placeholder step.</para>\n\
                 </proceduralStep>\n\
                 </mainProcedure>\n\
                 <closeRqmts>\n\
                 <reqCondGroup>\n\
                 <noConds/>\n\
                 </reqCondGroup>\n\
                 </closeRqmts>\n\
                 </procedure>"
            }
            SchemaKind::Fault => {
                "<faultIsolation>\n\
                 <isolationMainProcedure>\n\
                 <isolationStep>\n\
                 <action>Placeholder action.</action>\n\
                 </isolationStep>\n\
                 </isolationMainProcedure>\n\
                 </faultIsolation>"
            }
            SchemaKind::Crew => {
                "<crew>\n\
                 <crewRefCard>\n\
                 <title>Placeholder title</title>\n\
                 <crewDrill>\n\
                 <crewDrillStep>\n\
                 <para>Placeholder step.</para>\n\
                 </crewDrillStep>\n\
                 </crewDrill>\n\
                 </crewRefCard>\n\
                 </crew>"
            }
            SchemaKind::Ipd => {
                "<illustratedPartsCatalog>\n\
                 <catalogSeqNumber indenture=\"1\">\n\
                 <itemSeqNumber itemSeqNumberValue=\"001\">\n\
                 <partRef manufacturerCodeValue=\"00000\" partNumberValue=\"PLACEHOLDER\"/>\n\
                 <quantityPerNextHigherAssy>1</quantityPerNextHigherAssy>\n\
                 </itemSeqNumber>\n\
                 </catalogSeqNumber>\n\
                 </illustratedPartsCatalog>"
            }
            SchemaKind::Process => {
                "<process>\n\
                 <dmNode id=\"nod-0001\"/>\n\
                 </process>"
            }
            SchemaKind::Brex => {
                "<brex>\n\
                 <contextRules>\n\
                 <structureObjectRuleGroup>\n\
                 <structureObjectRule>\n\
                 <objectPath allowedObjectFlag=\"2\">//para</objectPath>\n\
                 <objectUse>Placeholder rule</objectUse>\n\
                 </structureObjectRule>\n\
                 </structureObjectRuleGroup>\n\
                 </contextRules>\n\
                 </brex>"
            }
            _ => "",
        }
    }

    // ------------------------------------------------------------ legacy markup

    fn legacy(&self) -> String {
        let o = self.options;
        let d = self.dmc;
        let (public_id, system_id) = match self.kind {
            SchemaKind::Proced => ("Procedural", "proced.dtd"),
            _ => ("Descriptive", "descript.dtd"),
        };
        let infoname = self
            .info_name
            .map(|i| format!("<infoname>{}</infoname>", text(i)))
            .unwrap_or_default();
        let avee = [
            ("modelic", &d.model_ident_code),
            ("sdc", &d.system_diff_code),
            ("chapnum", &d.system_code),
            ("section", &d.sub_system_code),
            ("subsect", &d.sub_sub_system_code),
            ("subject", &d.assy_code),
            ("discode", &d.disassy_code),
            ("discodev", &d.disassy_code_variant),
            ("incode", &d.info_code),
            ("incodev", &d.info_code_variant),
            ("itemloc", &d.item_location_code),
        ]
        .iter()
        .map(|(n, v)| format!("<{n}>{}</{n}>", text(v)))
        .collect::<String>();
        let flat = format!(
            "<dmodule>\n\
             <idstatus>\n\
             <dmaddres>\n\
             <dmc>\n\
             <avee>{avee}</avee>\n\
             </dmc>\n\
             <dmtitle><techname>{tech}</techname>{infoname}</dmtitle>\n\
             <issno issno=\"{issue_no}\" inwork=\"{in_work}\" type=\"new\"/>\n\
             <issdate year=\"{y}\" month=\"{m}\" day=\"{day}\"/>\n\
             <language language=\"{lang}\" country=\"{country}\"/>\n\
             </dmaddres>\n\
             <dmstatus>\n\
             <security class=\"{security}\"/>\n\
             <rpc rpcname=\"{name}\">{code}</rpc>\n\
             <orig origname=\"{name}\">{code}</orig>\n\
             <applic><model model=\"{applic}\"><version version=\"{applic}\"/></model></applic>\n\
             <qa><unverif/></qa>\n\
             </dmstatus>\n\
             </idstatus>\n\
             <content>\n\
             {content}\n\
             </content>\n\
             </dmodule>",
            tech = text(self.tech_name),
            issue_no = attr_value(&o.issue_info.issue_number),
            in_work = attr_value(&o.issue_info.in_work),
            y = attr_value(&o.date.year),
            m = attr_value(&o.date.month),
            day = attr_value(&o.date.day),
            lang = attr_value(self.lang.0),
            country = attr_value(self.lang.1),
            security = attr_value(&o.security_classification),
            name = attr_value(&o.enterprise_name),
            code = text(&o.enterprise_code),
            applic = attr_value(&o.applic_text),
            content = self.legacy_content(),
        );
        format!(
            "<?xml version=\"1.0\"?>\n\
             <!DOCTYPE dmodule PUBLIC \"-//S1000D//DTD {public_id} Data Module//EN\" \"{system_id}\" []>\n\
             <!-- S1000D issue {issue} -->\n\
             {}\n",
            Self::reindent(&flat),
            issue = self.issue,
        )
    }

    fn legacy_content(&self) -> &'static str {
        match self.kind {
            SchemaKind::Proced => {
                "<proced>\n\
                 <prelreqs>\n\
                 <reqconds>\n\
                 <noconds/>\n\
                 </reqconds>\n\
                 <reqpers>\n\
                 <nopers/>\n\
                 </reqpers>\n\
                 <supequi>\n\
                 <nosupeq/>\n\
                 </supequi>\n\
                 <supplies>\n\
                 <nosupply/>\n\
                 </supplies>\n\
                 <spares>\n\
                 <nospares/>\n\
                 </spares>\n\
                 <safety>\n\
                 <nosafety/>\n\
                 </safety>\n\
                 </prelreqs>\n\
                 <mainfunc>\n\
                 <step1>\n\
                 <para>Placeholder step.</para>\n\
                 </step1>\n\
                 </mainfunc>\n\
                 <closereqs>\n\
                 <reqconds>\n\
                 <noconds/>\n\
                 </reqconds>\n\
                 </closereqs>\n\
                 </proced>"
            }
            _ => {
                "<descript>\n\
                 <para0>\n\
                 <title>Placeholder title</title>\n\
                 <para>Placeholder paragraph.</para>\n\
                 </para0>\n\
                 </descript>"
            }
        }
    }
}

/// True when every element opened on `line` is also closed on it (`<a>text</a>`, `<a/>`).
fn closes_itself(line: &str) -> bool {
    let tags = line.matches('<').count();
    let closers = line.matches("</").count();
    let self_closing = line.matches("/>").count();
    tags - closers - self_closing == closers
}

#[cfg(test)]
mod tests;
