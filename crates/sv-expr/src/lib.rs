//! Typed RTL expressions: [`expr`] binds SystemVerilog expressions against an
//! elaborated design (`sv_ir::ElaboratedDesign`) into a typed [`ExprArena`]
//! with IEEE 1800 §11.6 context-determined sizing, signedness propagation and
//! four-state constant folding ([`ExprBinder`]); [`expr_types`] holds the
//! §11.6 / §11.8 type rules it uses.
//!
//! `sv-semantic` re-exports both modules as `sv_semantic::expr` and
//! `sv_semantic::expr_types`.

use sv_bigint as bigint;
use sv_fxhash as fx;
use sv_ir::{elab_ir, Locator};

pub mod expr;
pub mod expr_types;

pub use expr::{
    BinaryOp, BoundExpr, ExprArena, ExprBinder, ExprDiag, ExprId, ExprKind, InsideItem, LocalBinding, LocalId,
    SampledKind, SysFunc, UnaryOp,
};
