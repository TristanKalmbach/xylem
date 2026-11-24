//! Checking XSLT stylesheets: does a changed stylesheet still produce the
//! same output?
//!
//! A [`Suite`] lists test inputs (with any output a publishing service saved
//! for them). [`check`] runs the stylesheet under test on each input through
//! a [`Host`] (a program that runs stylesheets the way the publishing service
//! does, see `tools/saxon-dotnet`) and compares the result with a
//! [`Baseline`]: the saved output, the stylesheet as last committed to git,
//! or another stylesheet. [`compare`] ignores differences that are not real
//! (generated IDs, a byte order mark) and describes the rest.

pub mod check;
pub mod compare;
pub mod git;
pub mod host;
pub mod suite;

pub use check::{Baseline, CheckEnv, CheckReport, Outcome, TestResult, check};
pub use compare::{Comparison, Difference, compare};
pub use host::Host;
pub use suite::{Suite, Test};
