//! Data module and publication module identification: codes, issue info,
//! language, dates and titles, for both legacy (2.x/3.0) and modern (4.x+) markup.

use std::fmt;
use std::str::FromStr;

use xylem_dom::{Document, NodeId};

use crate::error::S1000dError;
use crate::issue::Markup;
use crate::xmlutil::{attr, child, descendant, ensure_child, path, set_text, strip_prefix_ci, text};

// ------------------------------------------------------------------ locating

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModuleKind {
    Data,
    Publication,
}

/// Where the identification elements of a document live.
pub(crate) struct Sections {
    pub markup: Markup,
    pub module: ModuleKind,
    /// `dmIdent` / `pmIdent`, or `dmaddres` / `pmaddres` in legacy markup.
    pub ident: NodeId,
    /// `dmAddressItems` / `pmAddressItems`; the same node as `ident` in legacy markup.
    pub items: Option<NodeId>,
}

pub(crate) fn sections(doc: &Document) -> Option<Sections> {
    let root = doc.document_element()?;
    if let Some(status) = child(doc, root, "identAndStatusSection") {
        if let Some(addr) = child(doc, status, "dmAddress") {
            return Some(Sections {
                markup: Markup::Modern,
                module: ModuleKind::Data,
                ident: child(doc, addr, "dmIdent")?,
                items: child(doc, addr, "dmAddressItems"),
            });
        }
        if let Some(addr) = child(doc, status, "pmAddress") {
            return Some(Sections {
                markup: Markup::Modern,
                module: ModuleKind::Publication,
                ident: child(doc, addr, "pmIdent")?,
                items: child(doc, addr, "pmAddressItems"),
            });
        }
    }
    if let Some(addr) = path(doc, root, &["idstatus", "dmaddres"]) {
        return Some(Sections { markup: Markup::Legacy, module: ModuleKind::Data, ident: addr, items: Some(addr) });
    }
    if let Some(addr) = descendant(doc, root, "pmaddres") {
        return Some(Sections {
            markup: Markup::Legacy,
            module: ModuleKind::Publication,
            ident: addr,
            items: Some(addr),
        });
    }
    None
}

fn missing(name: &str) -> S1000dError {
    S1000dError::MissingElement(name.to_string())
}

// ------------------------------------------------------------------ Dmc

/// A data module code, with every component kept as written (leading zeros included).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Dmc {
    pub model_ident_code: String,
    pub system_diff_code: String,
    pub system_code: String,
    pub sub_system_code: String,
    pub sub_sub_system_code: String,
    pub assy_code: String,
    pub disassy_code: String,
    pub disassy_code_variant: String,
    pub info_code: String,
    pub info_code_variant: String,
    pub item_location_code: String,
    /// Issue 4.1 and later.
    pub learn_code: Option<String>,
    /// Issue 4.1 and later.
    pub learn_event_code: Option<String>,
}

impl Dmc {
    /// The eleven mandatory components paired with their modern attribute name
    /// and legacy `avee` child element name.
    fn fields_mut(&mut self) -> [(&'static str, &'static str, &mut String); 11] {
        [
            ("modelIdentCode", "modelic", &mut self.model_ident_code),
            ("systemDiffCode", "sdc", &mut self.system_diff_code),
            ("systemCode", "chapnum", &mut self.system_code),
            ("subSystemCode", "section", &mut self.sub_system_code),
            ("subSubSystemCode", "subsect", &mut self.sub_sub_system_code),
            ("assyCode", "subject", &mut self.assy_code),
            ("disassyCode", "discode", &mut self.disassy_code),
            ("disassyCodeVariant", "discodev", &mut self.disassy_code_variant),
            ("infoCode", "incode", &mut self.info_code),
            ("infoCodeVariant", "incodev", &mut self.info_code_variant),
            ("itemLocationCode", "itemloc", &mut self.item_location_code),
        ]
    }

    /// The code without the `DMC-` prefix, e.g. `S1000DBIKE-AAA-D00-00-00-00AA-041A-A`.
    pub fn code(&self) -> String {
        let mut s = format!(
            "{}-{}-{}-{}{}-{}-{}{}-{}{}-{}",
            self.model_ident_code,
            self.system_diff_code,
            self.system_code,
            self.sub_system_code,
            self.sub_sub_system_code,
            self.assy_code,
            self.disassy_code,
            self.disassy_code_variant,
            self.info_code,
            self.info_code_variant,
            self.item_location_code
        );
        if let Some(learn) = &self.learn_code {
            s.push('-');
            s.push_str(learn);
            s.push_str(self.learn_event_code.as_deref().unwrap_or_default());
        }
        s
    }

    /// Parses `DMC-S1000DBIKE-AAA-D00-00-00-00AA-041A-A` (the prefix is optional).
    pub fn parse(text: &str) -> Result<Dmc, S1000dError> {
        let bad = |why: &str| S1000dError::InvalidCode(format!("'{text}': {why}"));
        let trimmed = text.trim();
        let body = strip_prefix_ci(trimmed, "DMC-").unwrap_or(trimmed);
        let parts: Vec<&str> = body.split('-').collect();
        if parts.len() != 8 && parts.len() != 9 {
            return Err(bad("expected 8 or 9 groups separated by '-'"));
        }
        if parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric())) {
            return Err(bad("groups must be non-empty letters and digits"));
        }
        let [mic, sdc, sys, subs, assy, dis, info, item] = parts[..8] else { unreachable!() };
        if subs.len() != 2 {
            return Err(bad("sub-system and sub-sub-system must be two characters"));
        }
        if dis.len() < 3 {
            return Err(bad("disassembly code and variant need at least three characters"));
        }
        if info.len() != 4 {
            return Err(bad("information code and variant must be four characters"));
        }
        let mut dmc = Dmc {
            model_ident_code: mic.to_string(),
            system_diff_code: sdc.to_string(),
            system_code: sys.to_string(),
            sub_system_code: subs[..1].to_string(),
            sub_sub_system_code: subs[1..].to_string(),
            assy_code: assy.to_string(),
            disassy_code: dis[..2].to_string(),
            disassy_code_variant: dis[2..].to_string(),
            info_code: info[..3].to_string(),
            info_code_variant: info[3..].to_string(),
            item_location_code: item.to_string(),
            learn_code: None,
            learn_event_code: None,
        };
        if let Some(learn) = parts.get(8) {
            if learn.len() != 4 {
                return Err(bad("learn code and event code must be four characters"));
            }
            dmc.learn_code = Some(learn[..3].to_string());
            dmc.learn_event_code = Some(learn[3..].to_string());
        }
        Ok(dmc)
    }

    /// Extracts the code from a CSDB file name such as `DMC-...-A_001-00_en-US.XML`.
    pub fn from_filename(name: &str) -> Result<Dmc, S1000dError> {
        let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
        Dmc::parse(base.split('_').next().unwrap_or(base))
    }

    /// Reports components that break the S1000D length and character rules.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        let mut check = |name: &str, value: &str, min: usize, max: usize| {
            let n = value.chars().count();
            if n < min || n > max {
                problems.push(format!("{name} '{value}' must be {min} to {max} characters"));
            } else if !value.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()) {
                problems.push(format!("{name} '{value}' may only contain A-Z and 0-9"));
            }
        };
        check("modelIdentCode", &self.model_ident_code, 2, 14);
        check("systemDiffCode", &self.system_diff_code, 1, 4);
        check("systemCode", &self.system_code, 2, 3);
        check("subSystemCode", &self.sub_system_code, 1, 1);
        check("subSubSystemCode", &self.sub_sub_system_code, 1, 1);
        check("assyCode", &self.assy_code, 2, 4);
        check("disassyCode", &self.disassy_code, 2, 2);
        check("disassyCodeVariant", &self.disassy_code_variant, 1, 3);
        check("infoCode", &self.info_code, 3, 3);
        check("infoCodeVariant", &self.info_code_variant, 1, 1);
        check("itemLocationCode", &self.item_location_code, 1, 1);
        if let Some(l) = &self.learn_code {
            check("learnCode", l, 3, 3);
        }
        if let Some(l) = &self.learn_event_code {
            check("learnEventCode", l, 1, 1);
        }
        problems
    }

    /// Reads a modern `dmCode` element.
    pub fn from_dm_code_element(doc: &Document, el: NodeId) -> Option<Dmc> {
        let mut dmc = Dmc::default();
        for (name, _, slot) in dmc.fields_mut() {
            *slot = attr(doc, el, name)?;
        }
        dmc.learn_code = attr(doc, el, "learnCode");
        dmc.learn_event_code = attr(doc, el, "learnEventCode");
        Some(dmc)
    }

    /// Reads legacy `modelic`/`sdc`/... elements found anywhere below `container`.
    pub fn from_legacy_container(doc: &Document, container: NodeId) -> Option<Dmc> {
        let mut dmc = Dmc::default();
        for (_, name, slot) in dmc.fields_mut() {
            *slot = text(doc, descendant(doc, container, name)?);
        }
        Some(dmc)
    }

    /// Reads the code identifying a data module from its own identification section.
    pub fn from_document(doc: &Document) -> Option<Dmc> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Data)?;
        match s.markup {
            Markup::Modern => Dmc::from_dm_code_element(doc, child(doc, s.ident, "dmCode")?),
            Markup::Legacy => Dmc::from_legacy_container(doc, child(doc, s.ident, "dmc")?),
        }
    }

    /// Writes the code into the document's identification section.
    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Data).ok_or_else(|| missing("dmAddress"))?;
        let mut copy = self.clone();
        match s.markup {
            Markup::Modern => {
                let el = child(doc, s.ident, "dmCode").ok_or_else(|| missing("dmCode"))?;
                for (name, _, value) in copy.fields_mut() {
                    doc.set_attr(el, name, value.clone());
                }
                set_optional_attr(doc, el, "learnCode", self.learn_code.as_deref());
                set_optional_attr(doc, el, "learnEventCode", self.learn_event_code.as_deref());
            }
            Markup::Legacy => {
                let dmc = child(doc, s.ident, "dmc").ok_or_else(|| missing("dmc"))?;
                let holder = child(doc, dmc, "avee").unwrap_or(dmc);
                for (_, name, value) in copy.fields_mut() {
                    let el = ensure_child(doc, holder, name);
                    set_text(doc, el, value);
                }
            }
        }
        Ok(())
    }
}

fn set_optional_attr(doc: &mut Document, el: NodeId, name: &str, value: Option<&str>) {
    match value {
        Some(v) => doc.set_attr(el, name, v),
        None => doc.remove_attr(el, name),
    }
}

impl fmt::Display for Dmc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DMC-{}", self.code())
    }
}

impl FromStr for Dmc {
    type Err = S1000dError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Dmc::parse(s)
    }
}

// ------------------------------------------------------------------ Pmc

/// A publication module code.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Pmc {
    pub model_ident_code: String,
    pub pm_issuer: String,
    pub pm_number: String,
    pub pm_volume: String,
}

impl Pmc {
    fn fields_mut(&mut self) -> [(&'static str, &'static str, &mut String); 4] {
        [
            ("modelIdentCode", "modelic", &mut self.model_ident_code),
            ("pmIssuer", "pmissuer", &mut self.pm_issuer),
            ("pmNumber", "pmnumber", &mut self.pm_number),
            ("pmVolume", "pmvolume", &mut self.pm_volume),
        ]
    }

    /// The code without the `PMC-` prefix.
    pub fn code(&self) -> String {
        format!("{}-{}-{}-{}", self.model_ident_code, self.pm_issuer, self.pm_number, self.pm_volume)
    }

    /// Parses `PMC-S1000DBIKE-C0419-00001-00` (the prefix is optional).
    pub fn parse(text: &str) -> Result<Pmc, S1000dError> {
        let trimmed = text.trim();
        let body = strip_prefix_ci(trimmed, "PMC-").unwrap_or(trimmed);
        let parts: Vec<&str> = body.split('-').collect();
        if parts.len() != 4 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric())) {
            return Err(S1000dError::InvalidCode(format!("'{text}': expected PMC-model-issuer-number-volume")));
        }
        Ok(Pmc {
            model_ident_code: parts[0].to_string(),
            pm_issuer: parts[1].to_string(),
            pm_number: parts[2].to_string(),
            pm_volume: parts[3].to_string(),
        })
    }

    /// Reads a modern `pmCode` element.
    pub fn from_pm_code_element(doc: &Document, el: NodeId) -> Option<Pmc> {
        let mut pmc = Pmc::default();
        for (name, _, slot) in pmc.fields_mut() {
            *slot = attr(doc, el, name)?;
        }
        Some(pmc)
    }

    /// Reads the code from a publication module's identification section.
    pub fn from_document(doc: &Document) -> Option<Pmc> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Publication)?;
        match s.markup {
            Markup::Modern => Pmc::from_pm_code_element(doc, child(doc, s.ident, "pmCode")?),
            Markup::Legacy => {
                let container = child(doc, s.ident, "pmc")?;
                let mut pmc = Pmc::default();
                for (_, name, slot) in pmc.fields_mut() {
                    *slot = text(doc, descendant(doc, container, name)?);
                }
                Some(pmc)
            }
        }
    }

    /// Writes the code into the document's identification section.
    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc)
            .filter(|s| s.module == ModuleKind::Publication)
            .ok_or_else(|| missing("pmAddress"))?;
        let mut copy = self.clone();
        match s.markup {
            Markup::Modern => {
                let el = child(doc, s.ident, "pmCode").ok_or_else(|| missing("pmCode"))?;
                for (name, _, value) in copy.fields_mut() {
                    doc.set_attr(el, name, value.clone());
                }
            }
            Markup::Legacy => {
                let holder = child(doc, s.ident, "pmc").ok_or_else(|| missing("pmc"))?;
                for (_, name, value) in copy.fields_mut() {
                    let el = ensure_child(doc, holder, name);
                    set_text(doc, el, value);
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for Pmc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PMC-{}", self.code())
    }
}

impl FromStr for Pmc {
    type Err = S1000dError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Pmc::parse(s)
    }
}

// ------------------------------------------------------------------ IssueInfo

/// Issue number and in-work number, e.g. `001` and `00`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueInfo {
    pub issue_number: String,
    pub in_work: String,
}

impl IssueInfo {
    pub fn new(issue_number: impl Into<String>, in_work: impl Into<String>) -> Self {
        Self { issue_number: issue_number.into(), in_work: in_work.into() }
    }

    /// Parses `001-00`.
    pub fn parse(text: &str) -> Option<IssueInfo> {
        let (issue, in_work) = text.trim().split_once('-')?;
        let ok = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        (ok(issue) && ok(in_work)).then(|| IssueInfo::new(issue, in_work))
    }

    /// Reads an `issueInfo` (modern) or `issno` (legacy) element.
    pub fn from_element(doc: &Document, el: NodeId, markup: Markup) -> Option<IssueInfo> {
        match markup {
            Markup::Modern => Some(IssueInfo::new(attr(doc, el, "issueNumber")?, attr(doc, el, "inWork")?)),
            Markup::Legacy => Some(IssueInfo::new(
                attr(doc, el, "issno")?,
                attr(doc, el, "inwork").unwrap_or_else(|| "00".to_string()),
            )),
        }
    }

    pub fn from_document(doc: &Document) -> Option<IssueInfo> {
        let s = sections(doc)?;
        match s.markup {
            Markup::Modern => IssueInfo::from_element(doc, child(doc, s.ident, "issueInfo")?, s.markup),
            Markup::Legacy => IssueInfo::from_element(doc, child(doc, s.ident, "issno")?, s.markup),
        }
    }

    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc).ok_or_else(|| missing("dmAddress"))?;
        match s.markup {
            Markup::Modern => {
                let el = child(doc, s.ident, "issueInfo").ok_or_else(|| missing("issueInfo"))?;
                doc.set_attr(el, "issueNumber", self.issue_number.clone());
                doc.set_attr(el, "inWork", self.in_work.clone());
            }
            Markup::Legacy => {
                let el = child(doc, s.ident, "issno").ok_or_else(|| missing("issno"))?;
                doc.set_attr(el, "issno", self.issue_number.clone());
                doc.set_attr(el, "inwork", self.in_work.clone());
            }
        }
        Ok(())
    }
}

impl fmt::Display for IssueInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.issue_number, self.in_work)
    }
}

// ------------------------------------------------------------------ Language

/// Language and country ISO codes, e.g. `en` and `US`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Language {
    pub language_iso_code: String,
    pub country_iso_code: String,
}

impl Language {
    pub fn new(language: impl Into<String>, country: impl Into<String>) -> Self {
        Self { language_iso_code: language.into(), country_iso_code: country.into() }
    }

    /// Parses `en-US`.
    pub fn parse(text: &str) -> Option<Language> {
        let (l, c) = text.trim().split_once('-')?;
        (!l.is_empty() && !c.is_empty()).then(|| Language::new(l, c))
    }

    /// Reads a `language` element (either attribute naming scheme).
    pub fn from_element(doc: &Document, el: NodeId) -> Option<Language> {
        let language = attr(doc, el, "languageIsoCode").or_else(|| attr(doc, el, "language"))?;
        let country = attr(doc, el, "countryIsoCode").or_else(|| attr(doc, el, "country"))?;
        Some(Language::new(language, country))
    }

    pub fn from_document(doc: &Document) -> Option<Language> {
        let s = sections(doc)?;
        let el = match s.markup {
            Markup::Modern => child(doc, s.ident, "language")?,
            Markup::Legacy => descendant(doc, s.ident, "language")?,
        };
        Language::from_element(doc, el)
    }

    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc).ok_or_else(|| missing("dmAddress"))?;
        let el = match s.markup {
            Markup::Modern => child(doc, s.ident, "language"),
            Markup::Legacy => descendant(doc, s.ident, "language"),
        }
        .ok_or_else(|| missing("language"))?;
        let (lang_attr, country_attr) = if s.markup == Markup::Legacy && doc.attr(el, "languageIsoCode").is_none() {
            ("language", "country")
        } else {
            ("languageIsoCode", "countryIsoCode")
        };
        doc.set_attr(el, lang_attr, self.language_iso_code.clone());
        doc.set_attr(el, country_attr, self.country_iso_code.clone());
        Ok(())
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.language_iso_code, self.country_iso_code)
    }
}

// ------------------------------------------------------------------ IssueDate

/// An issue date; components are kept as strings to preserve leading zeros.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IssueDate {
    pub year: String,
    pub month: String,
    pub day: String,
}

impl IssueDate {
    pub fn new(year: u32, month: u32, day: u32) -> Self {
        Self { year: format!("{year:04}"), month: format!("{month:02}"), day: format!("{day:02}") }
    }

    /// Today's date (UTC), derived from the system clock.
    pub fn today() -> Self {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        Self::from_unix_days((secs / 86_400) as i64)
    }

    /// Civil date of a day count since 1970-01-01.
    pub(crate) fn from_unix_days(days: i64) -> Self {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        IssueDate::new(year as u32, month as u32, day as u32)
    }

    fn element(doc: &Document, s: &Sections) -> Option<NodeId> {
        let name = if s.markup == Markup::Modern { "issueDate" } else { "issdate" };
        child(doc, s.items?, name)
    }

    pub fn from_document(doc: &Document) -> Option<IssueDate> {
        let s = sections(doc)?;
        let el = Self::element(doc, &s)?;
        Some(IssueDate { year: attr(doc, el, "year")?, month: attr(doc, el, "month")?, day: attr(doc, el, "day")? })
    }

    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc).ok_or_else(|| missing("dmAddress"))?;
        let el = Self::element(doc, &s).ok_or_else(|| missing("issueDate"))?;
        doc.set_attr(el, "year", self.year.clone());
        doc.set_attr(el, "month", self.month.clone());
        doc.set_attr(el, "day", self.day.clone());
        Ok(())
    }
}

impl fmt::Display for IssueDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.year, self.month, self.day)
    }
}

// ------------------------------------------------------------------ DmTitle

/// A data module title: tech name and optional info name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DmTitle {
    pub tech_name: String,
    pub info_name: Option<String>,
}

impl DmTitle {
    pub fn new(tech_name: impl Into<String>, info_name: Option<&str>) -> Self {
        Self { tech_name: tech_name.into(), info_name: info_name.map(str::to_string) }
    }

    fn element(doc: &Document, s: &Sections) -> Option<NodeId> {
        let (container, name) = match s.markup {
            Markup::Modern => (s.items?, "dmTitle"),
            Markup::Legacy => (s.ident, "dmtitle"),
        };
        child(doc, container, name)
    }

    fn child_names(markup: Markup) -> (&'static str, &'static str) {
        match markup {
            Markup::Modern => ("techName", "infoName"),
            Markup::Legacy => ("techname", "infoname"),
        }
    }

    pub fn from_document(doc: &Document) -> Option<DmTitle> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Data)?;
        let title = Self::element(doc, &s)?;
        let (tech, info) = Self::child_names(s.markup);
        Some(DmTitle {
            tech_name: text(doc, child(doc, title, tech)?),
            info_name: child(doc, title, info).map(|e| text(doc, e)),
        })
    }

    /// Updates the title, creating or removing `infoName` as needed.
    pub fn write_to_document(&self, doc: &mut Document) -> Result<(), S1000dError> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Data).ok_or_else(|| missing("dmAddress"))?;
        let title = Self::element(doc, &s).ok_or_else(|| missing("dmTitle"))?;
        let (tech, info) = Self::child_names(s.markup);
        let tech_el = ensure_child(doc, title, tech);
        set_text(doc, tech_el, &self.tech_name);
        match &self.info_name {
            Some(name) => {
                let el = ensure_child(doc, title, info);
                set_text(doc, el, name);
            }
            None => {
                if let Some(el) = child(doc, title, info) {
                    doc.detach(el);
                }
            }
        }
        Ok(())
    }
}

impl fmt::Display for DmTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.info_name {
            Some(info) => write!(f, "{} - {}", self.tech_name, info),
            None => f.write_str(&self.tech_name),
        }
    }
}

// ------------------------------------------------------------------ whole identifications

/// Builds `<code>_<issue>-<inwork>_<lang>-<country>.XML`.
pub fn csdb_filename(code: &str, issue: &IssueInfo, language: &Language) -> String {
    format!("{code}_{issue}_{language}.XML")
}

/// Everything identifying a data module.
#[derive(Debug, Clone, PartialEq)]
pub struct DmIdent {
    pub dmc: Dmc,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
    pub issue_date: Option<IssueDate>,
    pub title: Option<DmTitle>,
}

impl DmIdent {
    /// Reads the identification of a data module; `None` when it has no data module code.
    pub fn from_document(doc: &Document) -> Option<DmIdent> {
        Some(DmIdent {
            dmc: Dmc::from_document(doc)?,
            issue_info: IssueInfo::from_document(doc),
            language: Language::from_document(doc),
            issue_date: IssueDate::from_document(doc),
            title: DmTitle::from_document(doc),
        })
    }

    /// The standard CSDB file name, when issue and language are known.
    pub fn csdb_filename(&self) -> Option<String> {
        Some(csdb_filename(&self.dmc.to_string(), self.issue_info.as_ref()?, self.language.as_ref()?))
    }
}

/// Everything identifying a publication module.
#[derive(Debug, Clone, PartialEq)]
pub struct PmIdent {
    pub pmc: Pmc,
    pub issue_info: Option<IssueInfo>,
    pub language: Option<Language>,
    pub issue_date: Option<IssueDate>,
    pub title: Option<String>,
}

impl PmIdent {
    pub fn from_document(doc: &Document) -> Option<PmIdent> {
        let s = sections(doc).filter(|s| s.module == ModuleKind::Publication)?;
        let title_name = if s.markup == Markup::Modern { "pmTitle" } else { "pmtitle" };
        let title = child(doc, s.items?, title_name).map(|e| text(doc, e));
        Some(PmIdent {
            pmc: Pmc::from_document(doc)?,
            issue_info: IssueInfo::from_document(doc),
            language: Language::from_document(doc),
            issue_date: IssueDate::from_document(doc),
            title,
        })
    }

    pub fn csdb_filename(&self) -> Option<String> {
        Some(csdb_filename(&self.pmc.to_string(), self.issue_info.as_ref()?, self.language.as_ref()?))
    }
}

#[cfg(test)]
mod tests;
