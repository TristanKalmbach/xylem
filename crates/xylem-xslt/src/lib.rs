//! Checking XSLT stylesheets: does a changed stylesheet still produce the
//! same output?
//!
//! A [`Suite`] lists test inputs (with any output a publishing service saved
//! for them). [`check`] runs the stylesheet under test on each input through
//! a [`Host`] (Xylem's built-in engine, or optionally a program such as
//! `tools/saxon-dotnet` that runs stylesheets the way the publishing service
//! does) and compares the result with a
//! [`Baseline`]: the saved output, the stylesheet as last committed to git,
//! or another stylesheet. [`compare`] ignores differences that are not real
//! (generated IDs, a byte order mark) and describes the rest.

pub mod analyze;
pub mod check;
pub mod compare;
pub mod git;
pub mod host;
pub mod navigate;
pub mod profile;
pub mod proof;
pub mod report;
pub mod stylesheet;
pub mod suite;
pub mod xpath;

pub use analyze::{Analysis, Applied, Finding, Rule, Severity, analyze, apply, revert};
pub use check::{
    Baseline, CheckEnv, CheckReport, EngineComparison, Outcome, ProfileReport, TestProfile,
    TestResult, check, compare_engines, profile,
};
pub use compare::{Comparison, Difference, compare};
pub use host::Host;
pub use profile::{Profile, TemplateTime};
pub use suite::{Suite, Test};
