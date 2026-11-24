//! SV-BAN-001 (LegacyRegWireInTestbench): Discourages legacy Verilog-1995 'reg'
//! variable declarations in favor of IEEE 1800 'logic' (leaving 'wire' nets intact).

use crate::support::names::DIRECTIONS;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::Child;
use svlang::syntax::SyntaxKind::*;

pub(crate) fn legacy_reg_wire(c: &Cst, s: &mut Sink) {
    for n in c.nodes(DataDecl) {
        // Skip port declarations and struct/union members.
        // Note: `wire` is an active IEEE 1800-2023 net type (§6.6/§6.7) required
        // for multi-driver nets, inout ports, pullups, and bus interfaces — never flag `wire`.
        if c.toks(n).any(|t| DIRECTIONS.contains(&c.text_of(t))) || c.parent(n).is_some_and(|p| c.kind(p) == DataType) {
            continue;
        }
        let Some(ty) = c.tree.child(n, DataType) else { continue };
        if c.kids(n).first() != Some(&Child::Node(ty)) {
            continue;
        }
        let Some(first) = c.keyword(ty) else { continue };
        if c.text_of(first) == "reg" {
            s.add(
                "SV-BAN-001",
                "LegacyRegWireInTestbench",
                Severity::Info,
                "Legacy Verilog-1995 keyword 'reg' is discouraged in modern SystemVerilog (IEEE 1800 § 6.8). \
                 Prefer 'logic' for 4-state variables."
                    .into(),
                c.loc(first),
            );
        }
    }
}
