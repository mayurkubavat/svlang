//! SVL-COR-002 (NonShortCircuitLogical): Disallows using bitwise AND ('&') or
//! bitwise OR ('|') instead of logical short-circuiting operators ('&&', '||')
//! inside conditional statements when the RHS can fault or have side effects.

use crate::support::for_loops::{kids, op_text, predicates};
use crate::support::slang_ast::{ancestors, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

const BOOLEAN_OPS: &[&str] = &["==", "!=", "===", "!==", "==?", "!=?", "<", "<=", ">", ">=", "&&", "||", "->", "<->"];

fn is_boolean(c: &Cst, n: NodeId) -> bool {
    c.kind(n) == BinaryExpr && BOOLEAN_OPS.contains(&op_text(c, n))
}

fn is_one_bit_bool(c: &Cst, n: NodeId) -> bool {
    match c.kind(n) {
        ParenExpr => kids(c, n).first().is_some_and(|&i| is_one_bit_bool(c, i)),
        PrefixExpr => matches!(op_text(c, n), "!" | "&" | "|" | "^" | "~&" | "~|" | "~^" | "^~"),
        BinaryExpr if matches!(op_text(c, n), "&" | "|") => {
            let k = kids(c, n);
            k.len() == 2 && is_one_bit_bool(c, k[0]) && is_one_bit_bool(c, k[1])
        }
        _ => is_boolean(c, n),
    }
}

/// In synthesizable RTL (`always_comb`/`always_ff`/`always_latch`/`assign` inside `module`),
/// `(a == 1'b1) | (b == 1'b1)` where BOTH sides are 1-bit boolean predicates on pure scalar
/// wires/registers is a standard lowRISC/AscentLint-clean idiom with no width-masking,
/// null-deref, side-effect, division-by-zero, or dynamic index hazard.
fn is_safe_rtl_boolean_bitwise(c: &Cst, e: NodeId, lhs: NodeId, rhs: NodeId) -> bool {
    if !is_one_bit_bool(c, lhs) || !is_one_bit_bool(c, rhs) {
        return false;
    }
    let mut in_module = false;
    for a in ancestors(c, e) {
        match c.kind(a) {
            ClassDecl => return false,
            ProceduralBlock => {
                if c.toks(a).any(|t| matches!(c.text_of(t), "initial" | "final")) {
                    return false;
                }
            }
            ModuleDecl => in_module = true,
            _ => {}
        }
    }
    if !in_module {
        return false;
    }
    for n in subtree(c, rhs) {
        match c.kind(n) {
            CallExpr | MemberExpr | PostfixExpr => return false,
            PrefixExpr if matches!(op_text(c, n), "++" | "--") => return false,
            BinaryExpr if matches!(op_text(c, n), "/" | "%" | "=" | "+=" | "-=") => return false,
            SelectExpr => {
                // Constant bit/part select `sig[0]` or `sig[7:0]` is safe; variable index `arr[idx]` needs bounds guard.
                for idx in kids(c, n).into_iter().skip(1) {
                    if c.toks(idx).any(|t| {
                        let s = c.text_of(t);
                        s.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_')
                    }) {
                        return false;
                    }
                }
            }
            _ => {}
        }
    }
    true
}

pub(crate) fn non_short_circuit(c: &Cst, s: &mut Sink) {
    fn check(c: &Cst, e: NodeId, s: &mut Sink) {
        match c.kind(e) {
            BinaryExpr if matches!(op_text(c, e), "&" | "|") => {
                let k = kids(c, e);
                let boolean = k.iter().any(|&x| {
                    is_boolean(c, x)
                        || (c.kind(x) == ParenExpr && kids(c, x).first().is_some_and(|&i| is_boolean(c, i)))
                });
                let safe_rtl = k.len() == 2 && is_safe_rtl_boolean_bitwise(c, e, k[0], k[1]);
                if boolean && !safe_rtl {
                    let op = op_text(c, e);
                    let rep = if op == "&" { "&&" } else { "||" };
                    if let Some(t) = c.toks(e).next() {
                        s.add(
                            "SVL-COR-002",
                            "NonShortCircuitLogical",
                            Severity::Warning,
                            format!(
                                "Bitwise operator '{op}' used in conditional predicate where logical '{rep}' is expected. Bitwise operators do not short-circuit, unconditionally evaluating the right-hand side and risking null-pointer dereferences or index-out-of-bounds faults. Replace with '{rep}'."
                            ),
                            c.loc(t),
                        )
                        .fixes
                        .extend(crate::fix::with_safety(
                            c.token_fix(t, t, rep, &format!("Replace non-short-circuiting bitwise '{op}' with logical '{rep}'")),
                            crate::fix::FixSafety::Unsafe,
                        ));
                    }
                }
                for x in k {
                    check(c, x, s);
                }
            }
            ParenExpr => {
                if let Some(&i) = kids(c, e).first() {
                    check(c, i, s);
                }
            }
            _ => {}
        }
    }
    for p in predicates(c) {
        check(c, p, s);
    }
}
