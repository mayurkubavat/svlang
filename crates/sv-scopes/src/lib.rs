//! Scopes and name checks on `sv-syntax` trees, before types or
//! elaboration: [`scopes`] is the symbol table of every design unit (builder
//! phase N1) and [`names`] the IEEE 1800 checks on declarations and names
//! that need only that table, with the [`names::Diag`] every syntax-level
//! check reports.
//!
//! `sv-semantic` re-exports both modules as `sv_semantic::scopes` and
//! `sv_semantic::names`.

use sv_fxhash as fx;

pub mod names;
pub mod scopes;
