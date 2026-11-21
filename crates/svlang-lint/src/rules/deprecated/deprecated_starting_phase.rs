//! UVM-DEP-005 (DeprecatedStartingPhase): Flags direct reads or writes of the
//! deprecated `starting_phase` member on `uvm_sequence_base` (IEEE 1800.2-2020
//! §14.2.2), offering safe `--fix` rewrites to `get_starting_phase()` and
//! `set_starting_phase(...)`.

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn in_sequence_class(c: &Cst, n: NodeId) -> bool {
    std::iter::successors(c.parent(n), |&a| c.parent(a))
        .find(|&a| c.kind(a) == ClassDecl)
        .and_then(|cls| crate::support::uvm_syntax::base_name_node(c, cls))
        .is_some_and(|bt| {
            let base = c.tree.bare_text(bt);
            base.contains("seq") || base.contains("uvm_sequence")
        })
}

fn is_assign_op(s: &str) -> bool {
    matches!(s, "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<=" | ">>=" | "<<<=" | ">>>=")
}

fn is_write_lhs(c: &Cst, expr: NodeId) -> bool {
    c.parent(expr).is_some_and(|p| match c.kind(p) {
        PostfixExpr => true,
        PrefixExpr => c.toks(p).any(|t| matches!(c.text_of(t), "++" | "--")),
        BinaryExpr => c.tree.children(p).next() == Some(expr) && c.toks(p).any(|t| is_assign_op(c.text_of(t))),
        _ => false,
    })
}

pub(crate) fn deprecated_starting_phase(c: &Cst, out: &mut Sink) {
    let types = crate::expr_types::expr_types(c);
    for e in c.nodes(NameExpr) {
        let Some((t0, t1)) = c.tree.token_bounds(e) else { continue };
        if t0 != t1 || c.text_of(t0) != "starting_phase" || is_library_tok(c, t0) {
            continue;
        }
        if !in_sequence_class(c, e) {
            continue;
        }
        let assign_parent = c.parent(e).filter(|&p| {
            c.kind(p) == BinaryExpr
                && c.parent(p).map(|gp| c.kind(gp)) == Some(ExprStmt)
                && c.tree.children(p).next() == Some(e)
                && c.tok(p, "=").is_some()
        });
        let fix = if let Some(asgn) = assign_parent {
            let kids: Vec<NodeId> = c.tree.children(asgn).collect();
            if kids.len() == 2 {
                let rhs_text = c.trimmed(kids[1]);
                c.tree.token_bounds(asgn).and_then(|(a, b)| {
                    c.token_fix(
                        a,
                        b,
                        format!("set_starting_phase({rhs_text})"),
                        "Replace 'starting_phase = ...' with 'set_starting_phase(...)'",
                    )
                })
            } else {
                None
            }
        } else if !is_write_lhs(c, e) {
            c.token_fix(t0, t0, "get_starting_phase()", "Replace 'starting_phase' with 'get_starting_phase()'")
        } else {
            None
        };

        out.add(
            "UVM-DEP-005",
            "DeprecatedStartingPhase",
            Severity::Warning,
            "Direct access to 'starting_phase' in UVM sequences is deprecated by IEEE 1800.2-2020 §14.2.2. \
Use 'get_starting_phase()', 'set_starting_phase()', or 'set_automatic_phase_objection(1)' instead."
                .into(),
            c.loc(t0),
        )
        .fixes
        .extend(fix);
    }

    for m in c.nodes(MemberExpr) {
        let Some((a, b)) = c.tree.token_bounds(m) else { continue };
        if a == b || c.text_of(b) != "starting_phase" || is_library_tok(c, b) {
            continue;
        }
        let Some(base_node) = c.tree.children(m).next() else { continue };
        let base_is_seq = if c.text_of(a) == "this" || c.text_of(a) == "super" {
            in_sequence_class(c, m)
        } else {
            types
                .get(&(c.index(), base_node))
                .is_some_and(|ty| ty.text.contains("seq") || ty.text.contains("uvm_sequence"))
                || c.trimmed(base_node).ends_with("seq")
        };
        if !base_is_seq {
            continue;
        }
        let assign_parent = c.parent(m).filter(|&p| {
            c.kind(p) == BinaryExpr
                && c.parent(p).map(|gp| c.kind(gp)) == Some(ExprStmt)
                && c.tree.children(p).next() == Some(m)
                && c.tok(p, "=").is_some()
        });
        let base_text = c.trimmed(base_node);
        let fix = if let Some(asgn) = assign_parent {
            let kids: Vec<NodeId> = c.tree.children(asgn).collect();
            if kids.len() == 2 {
                let rhs_text = c.trimmed(kids[1]);
                c.tree.token_bounds(asgn).and_then(|(x, y)| {
                    c.token_fix(
                        x,
                        y,
                        format!("{base_text}.set_starting_phase({rhs_text})"),
                        "Replace '.starting_phase = ...' with '.set_starting_phase(...)'",
                    )
                })
            } else {
                None
            }
        } else if !is_write_lhs(c, m) {
            c.token_fix(
                a,
                b,
                format!("{base_text}.get_starting_phase()"),
                "Replace '.starting_phase' with '.get_starting_phase()'",
            )
        } else {
            None
        };
        out.add(
            "UVM-DEP-005",
            "DeprecatedStartingPhase",
            Severity::Warning,
            "Direct access to '.starting_phase' on a UVM sequence is deprecated by IEEE 1800.2-2020 §14.2.2. \
Use '.get_starting_phase()' or '.set_starting_phase()' instead."
                .into(),
            c.loc(b),
        )
        .fixes
        .extend(fix);
    }
}
