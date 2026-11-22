//! The plain data types the svlang semantic passes share, with no pass of
//! their own: [`elab_ir`] is the elaborated design ([`ElaboratedDesign`],
//! its [`Instance`]s, [`Symbol`]s, [`RtlType`]s and [`ScopeNode`] tree),
//! [`cval`] the four-state constant value [`CVal`] with its IEEE 1800
//! operators, [`loc`] source positions ([`Loc`], [`Locator`]) and
//! [`type_info`] the declared type of a name ([`TypeInfo`]).
//!
//! `sv-semantic` fills these in and re-exports every item at its old path
//! (`sv_semantic::elab_ir`, `sv_semantic::CVal`, `sv_semantic::Loc`,
//! `sv_semantic::Locator`, `sv_semantic::oracle::TypeInfo`).

use sv_bigint as bigint;
use sv_fxhash as fx;

pub mod cval;
pub mod elab_ir;
pub mod loc;
pub mod type_info;

pub use cval::{BitState, CVal};
pub use elab_ir::{
    ElabItem, ElabItemKind, ElaboratedDesign, Instance, InstanceId, PackageScope, PackedField, PortConn, PortDir,
    RtlType, ScopeNode, ScopeSignal, Symbol, SymbolId, SymbolKind,
};
pub use loc::{Loc, Locator};
pub use type_info::TypeInfo;
