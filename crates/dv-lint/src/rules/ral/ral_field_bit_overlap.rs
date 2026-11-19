//! RAL-FLD-001 (RalFieldBitOverlap): Detects overlapping `[lsb_pos : lsb_pos + size - 1]`
//! bit slices across `uvm_reg_field::configure(parent, size, lsb_pos, ...)` calls
//! inside the same `uvm_reg` class.

use std::collections::HashMap;

use svlang::syntax::SyntaxKind::*;

use crate::expr_types::call_args;
use crate::support::class_items::{kids, user_code};
use crate::support::constraints::{const_int, param_values};
use crate::support::slang_ast::{decl_name, last_name, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn ral_field_bit_overlap(c: &Cst, out: &mut Sink) {
    let params = param_values(c);
    for cls in c.nodes(ClassDecl) {
        if !user_code(c, cls) {
            continue;
        }
        let cname = decl_name(c, cls);
        if cname.is_empty() {
            continue;
        }
        // Check routines named `build` inside classes extending `uvm_reg` (or holding `.configure(this, ...)`).
        for f in c.tree.children(cls).filter(|&k| c.kind(k) == FunctionDecl) {
            if last_name(c, f) != "build" {
                continue;
            }
            // Recorded fields: `(field_name, lo, hi)`
            let mut fields: Vec<(String, i64, i64)> = Vec::new();
            let mut class_params: HashMap<String, svlang::syntax::NodeId> = params.clone();
            for pd in c.tree.children(cls).filter(|&k| c.kind(k) == ParamDecl) {
                for (dcl, pname, _) in crate::support::class_items::declarators(c, pd) {
                    if let Some(&e) =
                        kids(c, dcl).iter().rev().find(|&&x| crate::support::sema_facts::is_expr(c.kind(x)))
                    {
                        class_params.insert(pname, e);
                    }
                }
            }
            for call in subtree(c, f) {
                if c.kind(call) != CallExpr {
                    continue;
                }
                let Some(&callee) = kids(c, call).first() else { continue };
                if c.kind(callee) != MemberExpr || c.last_text(callee) != "configure" {
                    continue;
                }
                let Some(&recv) = kids(c, callee).first() else { continue };
                let fname = c.tree.bare_text(recv);
                // Skip array-indexed fields inside loops (`f[i].configure(...)`) unless constant index!
                if fname.contains('[') {
                    continue;
                }
                let args = call_args(c, call);
                // uvm_reg_field::configure(uvm_reg parent, int unsigned size, int unsigned lsb_pos, ...)
                if args.len() < 4 {
                    continue;
                }
                let parent_txt = c.tree.bare_text(args[0]);
                if parent_txt != "this" {
                    continue;
                }
                let Some(size) = const_int(c, args[1], &class_params, 0) else { continue };
                let Some(lsb) = const_int(c, args[2], &class_params, 0) else { continue };
                if size <= 0 || lsb < 0 {
                    continue;
                }
                let lo = lsb;
                let hi = lsb + size - 1;
                if let Some((prev_name, prev_lo, prev_hi)) =
                    fields.iter().find(|(_, plo, phi)| lo <= *phi && *plo <= hi)
                {
                    if let Some(tok) = c.first_tok(call) {
                        out.add(
                            "RAL-FLD-001",
                            "RalFieldBitOverlap",
                            Severity::Warning,
                            format!(
                                "Register field '{}' [{}:{}] in register '{}' overlaps field '{}' [{}:{}]. \
Overlapping uvm_reg_field bit slices corrupt frontdoor pack/unpack masks and register mirror prediction.",
                                fname, hi, lo, cname, prev_name, prev_hi, prev_lo
                            ),
                            c.loc(tok),
                        );
                    }
                } else {
                    fields.push((fname, lo, hi));
                }
            }
        }
    }
}
