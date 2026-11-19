//! UVM-FAC-006 (IncompatibleFactoryOverride): Type-based UVM factory override
//! (`set_type_override_by_type`, `set_inst_override_by_type`, or
//! `T::type_id::set_type_override`) where the override class does not extend
//! the requested base class.

use std::collections::HashMap;

use dv_model::{ClassId, Design};
use svlang::syntax::{NodeId, SyntaxKind::*};

use crate::expr_types::call_args;
use crate::support::class_items::{kids, user_code};
use crate::syntax::Cst;
use crate::{Severity, Sink};

fn extract_get_type_class(c: &Cst, arg: NodeId) -> Option<String> {
    let txt = c.tree.bare_text(arg);
    let clean = txt.trim();
    if let Some(prefix) = clean.strip_suffix("::get_type()") {
        let cls = prefix.strip_suffix("::type_id").unwrap_or(prefix);
        let base = cls.split('#').next().unwrap_or(cls).rsplit("::").next().unwrap_or(cls).trim();
        return (!base.is_empty()).then(|| base.to_string());
    }
    None
}

fn extract_type_id_owner(c: &Cst, callee: NodeId) -> Option<(String, String)> {
    let txt = c.tree.bare_text(callee);
    let (head, method) = txt.rsplit_once("::")?;
    let cls = head.strip_suffix("::type_id")?;
    let base = cls.split('#').next().unwrap_or(cls).rsplit("::").next().unwrap_or(cls).trim();
    (!base.is_empty()).then(|| (base.to_string(), method.trim().to_string()))
}

pub(crate) fn incompatible_factory_override(d: &Design, csts: &[Cst], out: &mut Sink) {
    let mut by_name: HashMap<&str, ClassId> = HashMap::new();
    for cls in d.classes() {
        by_name.entry(cls.name.as_str()).or_insert(cls.id);
    }
    for c in csts {
        for call in c.nodes(CallExpr) {
            if !user_code(c, call) {
                continue;
            }
            let Some(&callee) = kids(c, call).first() else { continue };
            let args = call_args(c, call);
            let mut pair: Option<(String, String, NodeId)> = None;

            if let Some((owner_cls, mname)) = extract_type_id_owner(c, callee) {
                if matches!(mname.as_str(), "set_type_override" | "set_inst_override") {
                    if let Some(&a0) = args.first() {
                        if let Some(ovr_cls) = extract_get_type_class(c, a0) {
                            pair = Some((owner_cls, ovr_cls, a0));
                        }
                    }
                }
            } else {
                let mname = match c.kind(callee) {
                    NameExpr => c.tree.bare_text(callee),
                    MemberExpr | ScopedExpr => c.last_text(callee).to_string(),
                    _ => continue,
                };
                if mname == "set_type_override_by_type" && args.len() >= 2 {
                    if let (Some(base_c), Some(ovr_c)) =
                        (extract_get_type_class(c, args[0]), extract_get_type_class(c, args[1]))
                    {
                        pair = Some((base_c, ovr_c, args[1]));
                    }
                } else if mname == "set_inst_override_by_type" && args.len() >= 2 {
                    // Two overload orders exist in UVM:
                    //   uvm_component::set_inst_override_by_type(rel_path, orig_type, ovr_type)
                    //   uvm_factory::set_inst_override_by_type(orig_type, ovr_type, full_path)
                    if let (Some(base_c), Some(ovr_c)) =
                        (extract_get_type_class(c, args[0]), extract_get_type_class(c, args[1]))
                    {
                        pair = Some((base_c, ovr_c, args[1]));
                    } else if args.len() >= 3 {
                        if let (Some(base_c), Some(ovr_c)) =
                            (extract_get_type_class(c, args[1]), extract_get_type_class(c, args[2]))
                        {
                            pair = Some((base_c, ovr_c, args[2]));
                        }
                    }
                }
            }

            let Some((base_name, ovr_name, ovr_node)) = pair else { continue };
            if base_name == ovr_name {
                continue;
            }
            let (Some(&base_id), Some(&ovr_id)) = (by_name.get(base_name.as_str()), by_name.get(ovr_name.as_str()))
            else {
                continue;
            };
            let extends_base = ovr_id == base_id || d.ancestors(ovr_id).any(|anc| anc.id == base_id);
            if !extends_base {
                if let Some(tok) = c.first_tok(ovr_node).or_else(|| c.first_tok(call)) {
                    out.add(
                        "UVM-FAC-006",
                        "IncompatibleFactoryOverride",
                        Severity::Warning,
                        format!(
                            "Factory override replaces '{}' with '{}', but '{}' does not extend '{}'. \
Creating an instance via '{}::type_id::create()' will fail the $cast check at run time.",
                            base_name, ovr_name, ovr_name, base_name, base_name
                        ),
                        c.loc(tok),
                    );
                }
            }
        }
    }
}
