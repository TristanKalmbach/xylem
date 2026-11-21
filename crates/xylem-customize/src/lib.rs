//! Xylem doctype packages and the XMAX customization importer.
//!
//! A *doctype package* is a folder with a `doctype.toml` manifest plus the
//! files it names (schema, stylesheets, entities). [`DoctypePackage`] loads
//! and saves one, [`DoctypeRegistry`] finds packages and picks one for an
//! opened document, and [`import_xmax`] converts an XMetaL XMAX
//! customization (`.ctm` folder or `.xac` archive) into a package.

mod batch;
mod behaviour;
mod css;
mod ctm;
mod error;
mod import;
mod package;
mod registry;
mod source;
mod template;
mod toolbar;
mod xdp;

pub use batch::{BatchEntry, ImportJob, discover, import_all, verify};
pub use behaviour::{
    Behaviour, DefinitionListDef, ElementPrettyPrint, IdGeneration, ImageDef, ListDef, PrettyPrint,
};
pub use error::{Error, Result};
pub use import::{ImportReport, import_xmax};
pub use package::{DoctypeInfo, DoctypePackage, Files, MANIFEST_NAME, MatchRules, ResolvedFiles};
pub use registry::DoctypeRegistry;
pub use template::{PLACEHOLDER_TARGET, Template, placeholder_text};
pub use toolbar::{Action, ToolbarEntry, default_toolbar};

#[cfg(test)]
mod tests;
