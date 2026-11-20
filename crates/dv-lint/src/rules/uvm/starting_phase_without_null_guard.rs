//! UVM-SEQ-018 (StartingPhaseWithoutNullGuard): Flags dereferencing
//! `starting_phase` (`starting_phase.raise_objection(...)`,
//! `starting_phase.drop_objection(...)`) inside a `uvm_sequence` without an
//! enclosing `if (starting_phase != null)` guard. Sequences started manually
//! via `seq.start(seqr)` or as child sequences have `starting_phase == null`,
//! causing fatal null-handle crashes at runtime. Offers an `--unsafe-fixes`
//! guard rewrite on standalone statements.

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use dv_model::{Design, UvmKind};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn condition_guards_starting_phase(c: &Cst, if_stmt: NodeId) -> bool {
    // Extract condition expression of `IfStmt` (first child before the `then` statement).
    let Some(cond) = c.tree.children(if_stmt).next() else { return false };
    let txt = c.trimmed(cond);
    txt.contains("starting_phase")
}

pub(crate) fn starting_phase_without_null_guard(d: &Design, csts: &[Cst], out: &mut Sink) {
    let ix = crate::support::class_roles::Index::new(csts);

    for cls in d.classes() {
        if cls.is_library || cls.is_parameterized || cls.uvm != UvmKind::Sequence {
            continue;
        }
        for m in &cls.methods {
            if m.is_builtin || m.is_extern {
                continue;
            }
            let Some((c, decl)) = ix.decl(m.loc) else { continue };
            let Some(body) = c.tree.child(decl, Body) else { continue };

            // Skip routines that already feature an early-return guard (`if (starting_phase == null) return;`).
            let body_txt = c.trimmed(body);
            if body_txt.contains("starting_phase == null") && body_txt.contains("return") {
                continue;
            }

            for n in c.tree.preorder() {
                if c.kind(n) != MemberExpr {
                    continue;
                }
                if !std::iter::successors(Some(n), |&x| c.parent(x)).any(|x| x == body) {
                    continue;
                }
                let Some(first_t) = c.first_tok(n) else { continue };
                if is_library_tok(c, first_t) {
                    continue;
                }
                let Some(base) = c.tree.children(n).next() else { continue };
                let is_sp = match c.kind(base) {
                    NameExpr => c.tree.bare_text(base) == "starting_phase",
                    MemberExpr => c.trimmed(base).replace(' ', "") == "this.starting_phase",
                    _ => false,
                };
                if !is_sp {
                    continue;
                }

                // Check whether any enclosing `IfStmt` inside `body` guards `starting_phase` in its condition
                // (and we are in the `then` branch when condition is `!= null` / truthy, or `else` when `== null`).
                let guarded = std::iter::successors(c.parent(n), |&x| c.parent(x))
                    .take_while(|&x| x != body)
                    .any(|anc| c.kind(anc) == IfStmt && condition_guards_starting_phase(c, anc));
                if guarded {
                    continue;
                }

                let finding = out.add(
                    "UVM-SEQ-018",
                    "StartingPhaseWithoutNullGuard",
                    Severity::Warning,
                    format!(
                        "Unchecked dereference of 'starting_phase' in sequence '{}' method '{}'. Sub-sequences and sequences started via seq.start(seqr) leave 'starting_phase' null; guard with 'if (starting_phase != null)'.",
                        cls.name, m.name
                    ),
                    c.loc(first_t),
                );

                // Attach an `Unsafe` fix wrapping standalone `starting_phase.<call>(...);` statements.
                if let Some(fix) = standalone_stmt_guard_fix(c, n) {
                    finding.fixes.push(fix);
                }
            }
        }
    }
}

fn standalone_stmt_guard_fix(c: &Cst, member_expr: NodeId) -> Option<crate::fix::Fix> {
    let call = c.parent(member_expr).filter(|&p| c.kind(p) == CallExpr)?;
    let stmt = c.parent(call).filter(|&p| c.kind(p) == ExprStmt)?;
    let parent_kind = c.parent(stmt).map(|p| c.kind(p))?;
    if !matches!(parent_kind, Body | SeqBlock) {
        return None;
    }
    let (a, b) = c.tree.token_bounds(stmt)?;
    let (ka, kb) = (&c.tree.tokens[a as usize], &c.tree.tokens[b as usize]);
    if ka.from_macro || kb.from_macro || ka.origin.file != kb.origin.file {
        return None;
    }
    let src = c.tree.sources.get(ka.origin.file as usize)?;
    let start = ka.origin.offset as usize;
    let end = kb.origin.offset as usize + kb.text.len();
    if start >= end || end > src.len() || src[start..end].contains('\n') {
        return None;
    }
    let raw = c.file_fix(
        ka.origin.file,
        start,
        end,
        format!("if (starting_phase != null) {}", &src[start..end]),
        "Guard starting_phase dereference with 'if (starting_phase != null)'",
    );
    crate::fix::with_safety(Some(raw), crate::fix::FixSafety::Unsafe)
}
