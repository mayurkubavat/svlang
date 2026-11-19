//! UVM-SEQ-017 (PSequencerTypeMismatch): Sequence declaring `p_sequencer` of type
//! `SEQR` (via `` `uvm_declare_p_sequencer(SEQR) `` or explicit `SEQR p_sequencer;`)
//! started via `.start(seqr)` on an incompatible sequencer class type.

use std::collections::HashMap;

use dv_model::{ClassId, Design};
use svlang::syntax::SyntaxKind::*;

use crate::expr_types::call_args;
use crate::support::class_items::{kids, user_code};
use crate::support::slang_ast::expr_type;
use crate::syntax::Cst;
use crate::{Severity, Sink};

fn classes_compatible(d: &Design, a: ClassId, b: ClassId) -> bool {
    a == b || d.ancestors(a).any(|anc| anc.id == b) || d.ancestors(b).any(|anc| anc.id == a)
}

pub(crate) fn p_sequencer_type_mismatch(d: &Design, csts: &[Cst], out: &mut Sink) {
    let mut by_name: HashMap<&str, ClassId> = HashMap::new();
    for cls in d.classes() {
        by_name.entry(cls.name.as_str()).or_insert(cls.id);
    }
    // Map sequence class name -> required `p_sequencer` class id & name.
    let mut req_pseqr: HashMap<&str, (ClassId, String)> = HashMap::new();
    for cls in d.classes() {
        if cls.is_library {
            continue;
        }
        for prop in &cls.properties {
            if prop.name == "p_sequencer" {
                let raw = prop.ty.split('#').next().unwrap_or(&prop.ty).rsplit("::").next().unwrap_or(&prop.ty).trim();
                if let Some(&pid) = by_name.get(raw) {
                    req_pseqr.insert(cls.name.as_str(), (pid, raw.to_string()));
                }
            }
        }
    }
    if req_pseqr.is_empty() {
        return;
    }
    for c in csts {
        for call in c.nodes(CallExpr) {
            if !user_code(c, call) {
                continue;
            }
            let Some(&callee) = kids(c, call).first() else { continue };
            if c.kind(callee) != MemberExpr || c.last_text(callee) != "start" {
                continue;
            }
            let Some(&seq_recv) = kids(c, callee).first() else { continue };
            let args = call_args(c, call);
            let Some(&seqr_arg) = args.first() else { continue };
            if c.tree.bare_text(seqr_arg) == "null" {
                continue;
            }
            let Some(seq_ty) = expr_type(c, seq_recv) else { continue };
            let Some(actual_seqr_ty) = expr_type(c, seqr_arg) else { continue };
            if matches!(actual_seqr_ty.as_str(), "uvm_sequencer_base" | "uvm_sequencer" | "uvm_component") {
                // Base handle might point to the right subclass dynamically via $cast; only flag unrelated concrete sequencer lineages!
                continue;
            }
            // Look up `seq_ty` (or its base sequence classes) in `req_pseqr`:
            let Some(&seq_cid) = by_name.get(seq_ty.as_str()) else { continue };
            let expected = req_pseqr
                .get(seq_ty.as_str())
                .cloned()
                .or_else(|| d.ancestors(seq_cid).find_map(|anc| req_pseqr.get(anc.name.as_str()).cloned()));
            let Some((exp_seqr_cid, exp_seqr_name)) = expected else { continue };
            let Some(&act_seqr_cid) = by_name.get(actual_seqr_ty.as_str()) else { continue };
            if !classes_compatible(d, exp_seqr_cid, act_seqr_cid) {
                if let Some(tok) = c.first_tok(seqr_arg).or_else(|| c.first_tok(call)) {
                    out.add(
                        "UVM-SEQ-017",
                        "PSequencerTypeMismatch",
                        Severity::Warning,
                        format!(
                            "Sequence of type '{}' expects p_sequencer of type '{}', but is started on incompatible sequencer type '{}'. \
uvm_sequence_base::m_set_p_sequencer() will fail $cast at run time with UVM_FATAL.",
                            seq_ty, exp_seqr_name, actual_seqr_ty
                        ),
                        c.loc(tok),
                    );
                }
            }
        }
    }
}
