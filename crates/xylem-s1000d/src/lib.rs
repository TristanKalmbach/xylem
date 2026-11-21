//! S1000D support for Xylem, covering issues 2.0 through 6.0.
//!
//! * [`detect`] identifies the issue and kind of a document.
//! * [`Dmc`], [`Pmc`], [`IssueInfo`], [`Language`], [`DmTitle`] read and write
//!   module identification for both the legacy (2.x/3.0) and modern (4.x+) markup,
//!   and build CSDB file names.
//! * [`find_dm_refs`], [`find_pm_refs`] and [`find_icn_refs`] locate references and
//!   [`DmRef::find_latest_in_dir`] resolves them against a folder.
//! * [`Brex`] and [`check_brex`] parse and enforce business rules.
//! * [`new_data_module`] and [`new_publication_module`] create skeletons for new modules.
//! * [`scan_dir`] and [`check_references`] treat a folder as a project (CSDB).
//! * [`filter_document`] evaluates applicability against chosen property values.

mod applic;
mod brex;
mod detect;
mod error;
mod ident;
mod issue;
mod pattern;
mod project;
mod refs;
mod template;
mod xmlutil;

#[cfg(test)]
mod testdata;

pub use brex::{
    AllowedFlag, Brex, BrexRule, BrexViolation, NotationRule, ObjectValue, Severity, SnsRule,
    ValueForm, ValueMatcher, check_brex,
};
pub use detect::{DetectSource, S1000dInfo, detect};
pub use error::S1000dError;
pub use ident::{
    DmIdent, DmTitle, Dmc, IssueDate, IssueInfo, Language, PmIdent, Pmc, csdb_filename,
};
pub use issue::{Issue, Markup, SchemaKind, available_kinds};
pub use pattern::Pattern;
pub use refs::{
    DmRef, EntityDecl, IcnRef, IcnRefKind, PmRef, entity_declarations, find_dm_refs, find_files,
    find_icn_refs, find_pm_refs, glob_match, parse_entity_declarations,
};
pub use template::{
    NewModuleOptions, default_brex_dmc, new_data_module, new_data_module_with,
    new_publication_module,
};

pub use applic::{
    ApplicChoice, ApplicFilterResult, ApplicProperty, PropertyKey, Truth, applic_properties,
    evaluate as evaluate_applic, filter_document, module_applic, values_match,
};
pub use project::{
    Finding, FindingKind, ModuleRef, ModuleType, Project, ProjectModule, ScanProgress,
    check_references, compare_issue, scan_dir, scan_file, xml_files,
};
