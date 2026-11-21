//! S1000D issues and the data module / document kinds (schemas).

use std::fmt;

/// An S1000D specification issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Issue {
    V2_0,
    V2_1,
    V2_2,
    V2_3,
    V3_0,
    V4_0,
    V4_0_1,
    V4_1,
    V4_2,
    V5_0,
    V6_0,
}

impl Issue {
    pub const ALL: [Issue; 11] = [
        Issue::V2_0,
        Issue::V2_1,
        Issue::V2_2,
        Issue::V2_3,
        Issue::V3_0,
        Issue::V4_0,
        Issue::V4_0_1,
        Issue::V4_1,
        Issue::V4_2,
        Issue::V5_0,
        Issue::V6_0,
    ];

    /// Dotted issue number, e.g. `4.0.1`.
    pub fn as_str(self) -> &'static str {
        match self {
            Issue::V2_0 => "2.0",
            Issue::V2_1 => "2.1",
            Issue::V2_2 => "2.2",
            Issue::V2_3 => "2.3",
            Issue::V3_0 => "3.0",
            Issue::V4_0 => "4.0",
            Issue::V4_0_1 => "4.0.1",
            Issue::V4_1 => "4.1",
            Issue::V4_2 => "4.2",
            Issue::V5_0 => "5.0",
            Issue::V6_0 => "6.0",
        }
    }

    /// Parses `4.1`, `4-1`, `4_0_1` or a bare major number such as `6`.
    pub fn parse(text: &str) -> Option<Issue> {
        let numbers: Vec<u32> = text
            .trim()
            .split(['.', '-', '_'])
            .map(|p| p.parse::<u32>().ok())
            .collect::<Option<Vec<_>>>()?;
        Self::from_numbers(&numbers)
    }

    pub(crate) fn from_numbers(n: &[u32]) -> Option<Issue> {
        Some(match n {
            [2] | [2, 0] => Issue::V2_0,
            [2, 1] => Issue::V2_1,
            [2, 2] => Issue::V2_2,
            [2, 3] => Issue::V2_3,
            [3] | [3, 0] => Issue::V3_0,
            [4] | [4, 0] | [4, 0, 0] => Issue::V4_0,
            [4, 0, 1] => Issue::V4_0_1,
            [4, 1] | [4, 1, 0] => Issue::V4_1,
            [4, 2] | [4, 2, 0] => Issue::V4_2,
            [5] | [5, 0] | [5, 0, 0] => Issue::V5_0,
            [6] | [6, 0] | [6, 0, 0] => Issue::V6_0,
            _ => return None,
        })
    }

    /// True for the DTD-based issues (2.x and 3.0), which use the legacy
    /// `dmodule/idstatus/dmaddres` markup.
    pub fn is_legacy(self) -> bool {
        self < Issue::V4_0
    }

    /// The markup family of this issue.
    pub fn markup(self) -> Markup {
        if self.is_legacy() { Markup::Legacy } else { Markup::Modern }
    }

    /// The directory token in the official schema URLs (`S1000D_4-1`).
    pub fn schema_token(self) -> &'static str {
        match self {
            Issue::V4_0 => "4-0",
            Issue::V4_0_1 => "4-0-1",
            Issue::V4_1 => "4-1",
            Issue::V4_2 => "4-2",
            Issue::V5_0 => "5-0",
            Issue::V6_0 => "6",
            Issue::V2_0 => "2-0",
            Issue::V2_1 => "2-1",
            Issue::V2_2 => "2-2",
            Issue::V2_3 => "2-3",
            Issue::V3_0 => "3-0",
        }
    }

    /// Whether data module codes may carry learn code / learn event code.
    pub fn supports_learn_codes(self) -> bool {
        self >= Issue::V4_1
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Which family of element names a document uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Markup {
    /// Issues 2.x and 3.0: `idstatus`, `dmaddres`, `dmc/avee`, `issno`, ...
    Legacy,
    /// Issues 4.0 onward: `identAndStatusSection`, `dmAddress`, `dmCode`, ...
    Modern,
}

/// The kind of S1000D data module or document, named after its schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaKind {
    Descript,
    Proced,
    Fault,
    Ipd,
    Crew,
    Process,
    Brex,
    Comrep,
    ApplicCrossRefTable,
    CondCrossRefTable,
    PrdCrossRefTable,
    Checklist,
    Schedul,
    Learning,
    Wrngdata,
    Wrngflds,
    Container,
    Pm,
    Dml,
    Ddn,
    Comment,
    Update,
    ScormContentPackage,
    Unknown,
}

impl SchemaKind {
    /// The schema (or DTD) file stem, e.g. `appliccrossreftable`.
    pub fn stem(self) -> &'static str {
        match self {
            SchemaKind::Descript => "descript",
            SchemaKind::Proced => "proced",
            SchemaKind::Fault => "fault",
            SchemaKind::Ipd => "ipd",
            SchemaKind::Crew => "crew",
            SchemaKind::Process => "process",
            SchemaKind::Brex => "brex",
            SchemaKind::Comrep => "comrep",
            SchemaKind::ApplicCrossRefTable => "appliccrossreftable",
            SchemaKind::CondCrossRefTable => "condcrossreftable",
            SchemaKind::PrdCrossRefTable => "prdcrossreftable",
            SchemaKind::Checklist => "checklist",
            SchemaKind::Schedul => "schedul",
            SchemaKind::Learning => "learning",
            SchemaKind::Wrngdata => "wrngdata",
            SchemaKind::Wrngflds => "wrngflds",
            SchemaKind::Container => "container",
            SchemaKind::Pm => "pm",
            SchemaKind::Dml => "dml",
            SchemaKind::Ddn => "ddn",
            SchemaKind::Comment => "comment",
            SchemaKind::Update => "update",
            SchemaKind::ScormContentPackage => "scormcontentpackage",
            SchemaKind::Unknown => "",
        }
    }

    /// Looks a kind up by schema file stem (case-insensitive).
    pub fn from_stem(stem: &str) -> Option<SchemaKind> {
        const KINDS: [SchemaKind; 23] = [
            SchemaKind::Descript,
            SchemaKind::Proced,
            SchemaKind::Fault,
            SchemaKind::Ipd,
            SchemaKind::Crew,
            SchemaKind::Process,
            SchemaKind::Brex,
            SchemaKind::Comrep,
            SchemaKind::ApplicCrossRefTable,
            SchemaKind::CondCrossRefTable,
            SchemaKind::PrdCrossRefTable,
            SchemaKind::Checklist,
            SchemaKind::Schedul,
            SchemaKind::Learning,
            SchemaKind::Wrngdata,
            SchemaKind::Wrngflds,
            SchemaKind::Container,
            SchemaKind::Pm,
            SchemaKind::Dml,
            SchemaKind::Ddn,
            SchemaKind::Comment,
            SchemaKind::Update,
            SchemaKind::ScormContentPackage,
        ];
        KINDS.into_iter().find(|k| k.stem().eq_ignore_ascii_case(stem))
    }

    /// A friendly name for menus and status lines.
    pub fn display_name(self) -> &'static str {
        match self {
            SchemaKind::Descript => "Descriptive",
            SchemaKind::Proced => "Procedural",
            SchemaKind::Fault => "Fault isolation / reporting",
            SchemaKind::Ipd => "Illustrated parts data",
            SchemaKind::Crew => "Crew / operator information",
            SchemaKind::Process => "Process",
            SchemaKind::Brex => "Business rules exchange (BREX)",
            SchemaKind::Comrep => "Common information repository",
            SchemaKind::ApplicCrossRefTable => "Applicability cross-reference table",
            SchemaKind::CondCrossRefTable => "Conditions cross-reference table",
            SchemaKind::PrdCrossRefTable => "Products cross-reference table",
            SchemaKind::Checklist => "Checklist",
            SchemaKind::Schedul => "Maintenance schedule",
            SchemaKind::Learning => "Learning",
            SchemaKind::Wrngdata => "Wiring data",
            SchemaKind::Wrngflds => "Wiring fault isolation",
            SchemaKind::Container => "Container",
            SchemaKind::Pm => "Publication module",
            SchemaKind::Dml => "Data management list",
            SchemaKind::Ddn => "Data dispatch note",
            SchemaKind::Comment => "Comment",
            SchemaKind::Update => "Data module update",
            SchemaKind::ScormContentPackage => "SCORM content package",
            SchemaKind::Unknown => "Unknown S1000D document",
        }
    }

    /// True for kinds that are data modules (root element `dmodule`).
    pub fn is_data_module(self) -> bool {
        !matches!(
            self,
            SchemaKind::Pm
                | SchemaKind::Dml
                | SchemaKind::Ddn
                | SchemaKind::Comment
                | SchemaKind::Update
                | SchemaKind::ScormContentPackage
                | SchemaKind::Unknown
        )
    }

    /// The element directly inside `content` that identifies the kind, per markup family.
    pub(crate) fn content_element(self, markup: Markup) -> Option<&'static str> {
        match markup {
            Markup::Modern => Some(match self {
                SchemaKind::Descript => "description",
                SchemaKind::Proced => "procedure",
                SchemaKind::Fault => "faultIsolation",
                SchemaKind::Ipd => "illustratedPartsCatalog",
                SchemaKind::Crew => "crew",
                SchemaKind::Process => "process",
                SchemaKind::Brex => "brex",
                SchemaKind::Comrep => "commonRepository",
                SchemaKind::ApplicCrossRefTable => "applicCrossRefTable",
                SchemaKind::CondCrossRefTable => "condCrossRefTable",
                SchemaKind::PrdCrossRefTable => "prdCrossRefTable",
                SchemaKind::Checklist => "checkList",
                SchemaKind::Learning => "learning",
                _ => return None,
            }),
            Markup::Legacy => Some(match self {
                SchemaKind::Descript => "descript",
                SchemaKind::Proced => "proced",
                SchemaKind::Fault => "fault",
                SchemaKind::Ipd => "ipd",
                SchemaKind::Crew => "crew",
                SchemaKind::Brex => "brex",
                SchemaKind::Wrngdata => "wrngdata",
                SchemaKind::Wrngflds => "wrngflds",
                _ => return None,
            }),
        }
    }

    /// Maps the first child of `content` to a kind.
    pub(crate) fn from_content_element(name: &str, markup: Markup) -> Option<SchemaKind> {
        let by_name = |k: SchemaKind| k.content_element(markup) == Some(name);
        const CANDIDATES: [SchemaKind; 13] = [
            SchemaKind::Descript,
            SchemaKind::Proced,
            SchemaKind::Fault,
            SchemaKind::Ipd,
            SchemaKind::Crew,
            SchemaKind::Process,
            SchemaKind::Brex,
            SchemaKind::Comrep,
            SchemaKind::ApplicCrossRefTable,
            SchemaKind::CondCrossRefTable,
            SchemaKind::PrdCrossRefTable,
            SchemaKind::Checklist,
            SchemaKind::Learning,
        ];
        let found = CANDIDATES.into_iter().find(|&k| by_name(k));
        found.or(match (name, markup) {
            ("faultReporting", Markup::Modern) => Some(SchemaKind::Fault),
            ("checklist", Markup::Modern) => Some(SchemaKind::Checklist),
            ("wrngdata", Markup::Legacy) => Some(SchemaKind::Wrngdata),
            ("wrngflds", Markup::Legacy) => Some(SchemaKind::Wrngflds),
            _ => None,
        })
    }
}

/// The kinds a new data module can be created as for `issue`.
pub fn available_kinds(issue: Issue) -> Vec<SchemaKind> {
    if issue.is_legacy() {
        vec![SchemaKind::Descript, SchemaKind::Proced]
    } else {
        vec![
            SchemaKind::Descript,
            SchemaKind::Proced,
            SchemaKind::Fault,
            SchemaKind::Crew,
            SchemaKind::Ipd,
            SchemaKind::Process,
            SchemaKind::Brex,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_text_round_trip() {
        for issue in Issue::ALL {
            assert_eq!(Issue::parse(issue.as_str()), Some(issue));
            assert_eq!(Issue::parse(&issue.as_str().replace('.', "-")), Some(issue));
        }
        assert_eq!(Issue::parse("6"), Some(Issue::V6_0));
        assert_eq!(Issue::parse("4_0_1"), Some(Issue::V4_0_1));
        assert_eq!(Issue::parse("9.9"), None);
        assert_eq!(Issue::parse("x"), None);
    }

    #[test]
    fn ordering_and_markup() {
        assert!(Issue::V3_0 < Issue::V4_0);
        assert!(Issue::V4_0 < Issue::V4_0_1);
        assert!(Issue::V2_3.is_legacy());
        assert_eq!(Issue::V4_0.markup(), Markup::Modern);
        assert!(Issue::V4_1.supports_learn_codes());
        assert!(!Issue::V4_0_1.supports_learn_codes());
    }

    #[test]
    fn kinds() {
        assert_eq!(SchemaKind::from_stem("AppliCCrossRefTable"), Some(SchemaKind::ApplicCrossRefTable));
        assert_eq!(SchemaKind::from_stem("nope"), None);
        assert_eq!(SchemaKind::Descript.display_name(), "Descriptive");
        assert_eq!(available_kinds(Issue::V3_0), [SchemaKind::Descript, SchemaKind::Proced]);
        assert_eq!(available_kinds(Issue::V4_1).len(), 7);
        assert_eq!(
            SchemaKind::from_content_element("illustratedPartsCatalog", Markup::Modern),
            Some(SchemaKind::Ipd)
        );
        assert_eq!(SchemaKind::from_content_element("descript", Markup::Legacy), Some(SchemaKind::Descript));
        assert_eq!(SchemaKind::from_content_element("descript", Markup::Modern), None);
    }
}
