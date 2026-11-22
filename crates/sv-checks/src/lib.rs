//! The semantic checks that need only `sv-syntax` trees and the scopes of
//! `sv-scopes`, not constant evaluation or elaboration: one module per
//! family, each with a `check` function that returns [`names::Diag`]s.
//! `sv-semantic` runs them (with its own `typed` checks and elaboration)
//! in `SemaChecks` and re-exports every module at `sv_semantic::<module>`.
//!
//! [`hier`] takes the elaborated hierarchy as lines of text, so it needs
//! no elaboration types either.

use sv_fxhash as fx;
use sv_scopes::{names, scopes};

pub mod bind;
pub mod calls;
pub mod checks;
pub mod classes;
pub mod hier;
pub mod iclass;
pub mod order;
pub mod sva;
pub mod udp;
