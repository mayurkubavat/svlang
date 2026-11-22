//! Bound SystemVerilog Assertions: [`sva_ir`] resolves concurrent
//! assertions, named `sequence` and `property` declarations, clocking events
//! and `disable iff` conditions of an elaborated design into typed temporal
//! IR ([`SvaBinder`], [`BoundProperty`]) whose boolean leaves are `sv-expr`
//! bound expressions.
//!
//! `sv-semantic` re-exports the module as `sv_semantic::sva_ir`.

use sv_expr::{expr, LocalBinding};
use sv_fxhash as fx;
use sv_ir::{elab_ir, Locator, RtlType};

pub mod sva_ir;

pub use sva_ir::{
    BoundProperty, CycleBound, CycleRange, PropExpr, PropKind, RepeatKind, SeqExpr, SvaBinder, SvaClock, SvaEdge,
};
