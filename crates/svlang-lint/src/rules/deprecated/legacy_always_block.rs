//! SVL-DEP-003 (LegacyAlwaysBlock): Flags RTL-style sensitivity-list `always @(...)`
//! blocks that should use IEEE 1800 `always_comb`, `always_ff`, or `always_latch`,
//! while exempting valid testbench `#delay` clock generators and multi-wait processes.

use crate::support::procedural::is_stmt;
use crate::support::slang_ast::subtree;
use crate::syntax::{procedural_blocks, Cst};
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;

pub(crate) fn legacy_always(c: &Cst, s: &mut Sink) {
    for (pb, kw) in procedural_blocks(c) {
        if c.text_of(kw) != "always" {
            continue;
        }
        // Per IEEE 1800-2023 §9.2.2, `always_comb`, `always_ff`, and `always_latch`
        // cannot replace `#delay` clock generators (`always #5 clk = ~clk;`), bare
        // `always begin ... end` BFM loops without a top-level `@(...)` sensitivity
        // list, or behavioral processes containing internal `#`, `@`, or `wait`
        // timing controls inside their body.
        let Some(timed) = c.tree.children(pb).find(|&k| c.kind(k) == TimedStmt) else {
            continue;
        };
        let Some(tc) = c.tree.child(timed, TimingControl) else {
            continue;
        };
        if c.first_tok(tc).map(|t| c.text_of(t)) != Some("@") {
            continue;
        }
        let Some(body) = c.tree.children(timed).find(|&k| is_stmt(c.kind(k))) else {
            continue;
        };
        let has_body_timing = subtree(c, body)
            .into_iter()
            .any(|n| n != body && matches!(c.kind(n), TimedStmt | TimingControl | TimedExpr | WaitStmt));
        if has_body_timing {
            continue;
        }
        s.add(
            "SVL-DEP-003",
            "LegacyAlwaysBlock",
            Severity::Warning,
            "RTL sensitivity-list 'always @(...)' block should use an IEEE 1800 § 9.2.2 intent-specific block. \
             General 'always @(...)' blocks do not enforce single-driver checks and permit unintended latch inference or incomplete sensitivity lists. \
             Use 'always_comb' for combinational logic, 'always_ff @(posedge clk ...)' for sequential logic, or 'always_latch' for transparent latches."
                .into(),
            c.loc(kw),
        );
    }
}
