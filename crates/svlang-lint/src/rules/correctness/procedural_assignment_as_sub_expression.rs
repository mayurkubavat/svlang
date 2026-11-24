//! SV-COR-020 (ProceduralAssignmentAsSubExpression): Disallows embedded
//! assignment expressions used as sub-expressions within conditionals or
//! arithmetic operations.

use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::Child;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

/// True when `n` sits in the `for (init; cond; step)` init or step slot (IEEE 1800 §12.7.1),
/// rather than embedded inside `cond`!
fn in_for_init_or_step(c: &Cst, n: NodeId, lp: NodeId) -> bool {
    if c.keyword(lp).map(|k| c.text_of(k)) != Some("for") {
        return false;
    }
    let Some(tok) = c.first_tok(n) else { return false };
    let mut semis = Vec::new();
    let mut close_paren = None;
    let mut depth = 0i32;
    for ch in &c.tree.node(lp).children {
        if let Child::Token(t) = *ch {
            match c.text_of(t) {
                "(" => depth += 1,
                ")" => {
                    depth -= 1;
                    if depth == 0 {
                        close_paren = Some(t);
                        break;
                    }
                }
                ";" if depth == 1 => semis.push(t),
                _ => {}
            }
        }
    }
    // `for (init ; cond ; step )`: init is before `semis[0]`, step is between `semis[1]` and `close_paren`.
    if let ([s0, s1], Some(cp)) = (semis.as_slice(), close_paren) {
        return tok < *s0 || (tok > *s1 && tok < cp);
    }
    false
}

pub(crate) fn assignment_sub_expr(c: &Cst, s: &mut Sink) {
    for n in c.nodes(BinaryExpr) {
        let Some(op) = c.tok(n, "=").or_else(|| c.tok(n, "+=")).or_else(|| c.tok(n, "-=")) else { continue };
        let mut up = c.parent(n);
        while let Some(p) = up.filter(|&p| c.kind(p) == ParenExpr) {
            up = c.parent(p);
        }
        let standalone = up.is_some_and(|p| match c.kind(p) {
            // `assign a = b, c = d;` is a list of standalone assignments.
            ExprStmt | CoverageOption | ContinuousAssign => true,
            LoopStmt => in_for_init_or_step(c, n, p),
            _ => crate::support::slang_ast::ancestors(c, p).find(|&a| c.kind(a) == LoopStmt).is_some_and(|lp| {
                in_for_init_or_step(c, n, lp) && !matches!(c.kind(p), BinaryExpr | CondExpr | CallExpr)
            }),
        });
        if standalone {
            continue;
        }
        let text = c.text(n);
        let last = text.rsplit('\n').next().unwrap_or("");
        let expr = last.trim_matches([' ', '\t', '\r']);
        s.add(
            "SV-COR-020",
            "ProceduralAssignmentAsSubExpression",
            Severity::Warning,
            format!(
                "Assignment expression '{expr}' is embedded as a sub-expression. In SystemVerilog (IEEE 1800 § 10.4), \
                 assignments should stand alone as dedicated statements rather than being embedded in conditionals \
                 or arithmetic expressions, which creates subtle order-of-evaluation race bugs and degrades readability."
            ),
            c.loc(op),
        );
    }
}
