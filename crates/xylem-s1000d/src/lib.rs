//! S1000D support for Xylem, covering issues 2.0 through 6.0.
//!
//! * [`detect`] identifies the issue and kind of a document.
//! * [`Dmc`], [`Pmc`], [`IssueInfo`], [`Language`], [`DmTitle`] read and write
//!   module identification for both the legacy (2.x/3.0) and modern (4.x+) markup,
//!   and build CSDB file names.
//! * [`find_dm_refs`], [`find_pm_refs`] and [`find_icn_refs`] locate references and
//!   [`DmRef::find_latest_in_dir`] resolves them against a folder.
//! * [`Brex`] and [`check_brex`] parse and enforce business rules.
//! * [`new_data_module`] creates skeletons for new modules.

mod brex;
mod detect;
mod error;
mod ident;
mod issue;
mod pattern;
mod refs;
mod template;
mod xmlutil;

#[cfg(test)]
mod testdata;

pub use brex::{
    AllowedFlag, Brex, BrexRule, BrexViolation, NotationRule, ObjectValue, Severity, SnsRule, ValueForm,
    ValueMatcher, check_brex,
};
pub use detect::{DetectSource, S1000dInfo, detect};
pub use error::S1000dError;
pub use ident::{DmIdent, DmTitle, Dmc, IssueDate, IssueInfo, Language, PmIdent, Pmc, csdb_filename};
pub use issue::{Issue, Markup, SchemaKind, available_kinds};
pub use pattern::Pattern;
pub use refs::{
    DmRef, EntityDecl, IcnRef, IcnRefKind, PmRef, entity_declarations, find_dm_refs, find_files, find_icn_refs,
    find_pm_refs, glob_match, parse_entity_declarations,
};
pub use template::{NewModuleOptions, default_brex_dmc, new_data_module, new_data_module_with};
