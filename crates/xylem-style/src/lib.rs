//! CSS engine that computes per-node styles for XML documents the way
//! XMetaL's tags-on view does.
//!
//! ```
//! use xylem_style::{Display, StyleEngine, Stylesheet};
//!
//! let sheet = Stylesheet::parse("para { display: block; margin-bottom: .5em } title:before { content: 'Title: ' }");
//! let engine = StyleEngine::new(sheet);
//! let doc = xylem_dom::parse("<doc><title>x</title><para>y</para></doc>").unwrap();
//! let styles = engine.compute(&doc);
//! let para = doc.descendants(doc.root()).find(|&n| doc.name(n) == Some("para")).unwrap();
//! assert_eq!(styles.get(para).display, Display::Block);
//! assert_eq!(styles.get(para).margin.bottom, 8.0);
//! ```
//!
//! Supported: the selectors, properties and generated-content features listed
//! in the crate README of the repository; see [`Stylesheet`], [`StyleEngine`],
//! [`StyleMap`] and [`ComputedStyle`].

mod counter;
mod decl;
mod engine;
mod selector;
mod sheet;
mod token;
mod types;
mod value;

pub use counter::format_counter;
pub use decl::{ContentItem, ContentValue};
pub use engine::{StyleEngine, StyleMap};
pub use sheet::{ResolvedImport, SheetFlags, Stylesheet};
pub use types::{
    Border, BorderStyle, Borders, Color, ComputedStyle, Display, Generated, ListStyle, Sides,
    TextAlign, TextTransform, VerticalAlign, WhiteSpace,
};
