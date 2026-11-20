//! SV-TYP-012 (UnsignedLoopDowncountHazard): Flags `for` loops whose loop
//! control variable is declared with an unsigned integral type (`int unsigned`,
//! `bit [...]`, `logic [...]`, `byte unsigned`) and tested with `>= 0` (or
//! `0 <= i`) while decrementing, creating a non-terminating wrap-around loop.

use crate::support::for_loops::keyword_is;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;
use svlang::syntax::{Child, NodeId};

fn is_unsigned_type_tokens(toks: &[&str]) -> bool {
    if toks.contains(&"signed") {
        return false;
    }
    if toks.contains(&"unsigned") {
        return true;
    }
    toks.iter().any(|t| matches!(*t, "bit" | "logic" | "reg"))
}

fn unsigned_init_vars(c: &Cst, init: &[NodeId]) -> Vec<String> {
    let mut out = Vec::new();
    for &n in init {
        if c.kind(n) != DataDecl {
            continue;
        }
        let Some(dt) = c.tree.child(n, DataType) else { continue };
        let dt_toks: Vec<&str> = c.toks(dt).map(|t| c.text_of(t)).collect();
        if !is_unsigned_type_tokens(&dt_toks) {
            continue;
        }
        for d in c.tree.children(n).filter(|&d| c.kind(d) == Declarator) {
            if let Some(nm) = c.tree.child(d, Name) {
                out.push(c.tree.bare_text(nm));
            }
        }
    }
    out
}

fn cond_has_ge_zero(c: &Cst, cond: NodeId, var: &str) -> bool {
    let cond = c.strip(cond);
    if c.kind(cond) == BinaryExpr {
        let kids: Vec<NodeId> = c.tree.children(cond).collect();
        if kids.len() == 2 {
            let lhs = c.trimmed(c.strip(kids[0]));
            let rhs = c.trimmed(c.strip(kids[1]));
            let txt = c.trimmed(cond);
            if txt.contains(">=")
                && lhs == var
                && (rhs == "0" || rhs.ends_with("'d0") || rhs.ends_with("'b0") || rhs.ends_with("'h0"))
            {
                return true;
            }
            if txt.contains("<=")
                && rhs == var
                && (lhs == "0" || lhs.ends_with("'d0") || lhs.ends_with("'b0") || lhs.ends_with("'h0"))
            {
                return true;
            }
        }
        for k in kids {
            if cond_has_ge_zero(c, k, var) {
                return true;
            }
        }
    }
    false
}

fn step_decrements(c: &Cst, steps: &[NodeId], var: &str) -> bool {
    for &step in steps {
        let txt = c.trimmed(step).replace(' ', "");
        if txt == format!("{var}--")
            || txt == format!("--{var}")
            || txt.starts_with(&format!("{var}-="))
            || txt == format!("{var}={var}-1")
            || txt == format!("{var}={var}-1'b1")
        {
            return true;
        }
    }
    false
}

fn for_header_parts(c: &Cst, l: NodeId) -> (Vec<NodeId>, Option<NodeId>, Vec<NodeId>) {
    let mut semis = 0u32;
    let mut paren_depth = 0i32;
    let mut init = Vec::new();
    let mut cond = None;
    let mut steps = Vec::new();
    for k in c.kids(l) {
        match *k {
            Child::Token(t) => match c.text_of(t) {
                "(" => paren_depth += 1,
                ")" => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        break;
                    }
                }
                ";" if paren_depth == 1 => semis += 1,
                _ => {}
            },
            Child::Node(n) if paren_depth >= 1 => match semis {
                0 => init.push(n),
                1 => cond = Some(n),
                2 => steps.push(n),
                _ => {}
            },
            _ => {}
        }
    }
    (init, cond, steps)
}

pub(crate) fn unsigned_loop_downcount_hazard(c: &Cst, out: &mut Sink) {
    for l in c.nodes(LoopStmt) {
        let Some(t0) = c.first_tok(l) else { continue };
        if is_library_tok(c, t0) || !keyword_is(c, l, "for") {
            continue;
        }
        let (init, Some(cond), steps) = for_header_parts(c, l) else { continue };
        for var in unsigned_init_vars(c, &init) {
            if cond_has_ge_zero(c, cond, &var) && step_decrements(c, &steps, &var) {
                out.add(
                    "SV-TYP-012",
                    "UnsignedLoopDowncountHazard",
                    Severity::Error,
                    format!(
                        "Unsigned loop variable '{var}' is tested with '>= 0' while decrementing. An unsigned value is always '>= 0', causing wrap-around underflow and an infinite loop."
                    ),
                    c.loc(t0),
                );
            }
        }
    }
}
