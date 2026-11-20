//! UVM-PHS-013 (VirtualInterfaceAccessInBuildPhase): Flags reading or driving
//! signals through a virtual interface property (`vif.<sig>`) inside a
//! `uvm_component`'s constructor `new()` or `build_phase()` (or non-virtual
//! helper routines called directly from them). Virtual interface handles are
//! fetched in `build_phase`, bound in `connect_phase`, and driven/sampled in
//! runtime task phases (`MG-2.2`).

use std::collections::HashSet;

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use dv_model::Design;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

pub(crate) fn virtual_interface_access_in_build_phase(d: &Design, csts: &[Cst], out: &mut Sink) {
    let ix = crate::support::class_roles::Index::new(csts);

    for cls in d.classes() {
        if cls.is_library || cls.is_parameterized || !cls.uvm.is_component() {
            continue;
        }
        // Collect virtual interface member names declared on `cls` or its ancestors.
        let mut vif_names: HashSet<String> = HashSet::new();
        for p in &cls.properties {
            if p.is_virtual_interface {
                vif_names.insert(p.name.clone());
            }
        }
        for anc in d.ancestors(cls.id) {
            for p in &anc.properties {
                if p.is_virtual_interface {
                    vif_names.insert(p.name.clone());
                }
            }
        }
        if vif_names.is_empty() {
            continue;
        }

        // Check `new()` and `build_phase()`, plus non-virtual helper methods on `this` called directly from them.
        let mut target_methods = Vec::new();
        let mut helper_names = HashSet::new();
        for m in &cls.methods {
            if m.is_builtin || m.is_extern {
                continue;
            }
            if m.name == "new" || m.name == "build_phase" {
                target_methods.push((m, None::<String>));
                for call in &m.calls {
                    if !call.is_system
                        && !call.callee_builtin
                        && !call.via_super
                        && (call.receiver.is_empty() || call.receiver == "this")
                        && call.name != "new"
                        && call.name != "build_phase"
                    {
                        helper_names.insert((call.name.clone(), m.name.clone()));
                    }
                }
            }
        }
        for (hname, from_phase) in helper_names {
            if let Some(hm) =
                cls.methods.iter().find(|m| !m.is_builtin && !m.is_extern && !m.is_virtual && m.name == hname)
            {
                target_methods.push((hm, Some(from_phase)));
            }
        }

        for (m, via_phase) in target_methods {
            let Some((c, decl)) = ix.decl(m.loc) else { continue };
            let Some(body) = c.tree.child(decl, Body) else { continue };

            // Exclude local variables that shadow a `vif_names` property.
            let shadowed: HashSet<String> =
                m.locals.iter().map(|l| l.name.clone()).chain(m.arguments.iter().map(|a| a.name.clone())).collect();

            for n in c.tree.preorder() {
                if c.kind(n) != MemberExpr {
                    continue;
                }
                // Check `n` is inside `body`.
                if !std::iter::successors(Some(n), |&x| c.parent(x)).any(|x| x == body) {
                    continue;
                }
                let Some(mtok) = c.first_tok(n) else { continue };
                if is_library_tok(c, mtok) {
                    continue;
                }
                let kids: Vec<NodeId> = c.tree.children(n).collect();
                let Some(&base) = kids.first() else { continue };
                // Match `vif.<sig>`, `this.vif.<sig>`, or `vif[idx].<sig>`.
                let vif_ident: Option<String> = match c.kind(base) {
                    NameExpr => Some(c.tree.bare_text(base)),
                    SelectExpr => {
                        c.tree.children(base).next().filter(|&b| c.kind(b) == NameExpr).map(|b| c.tree.bare_text(b))
                    }
                    MemberExpr => {
                        let bkids: Vec<NodeId> = c.tree.children(base).collect();
                        if bkids.first().is_some_and(|&r| c.tree.bare_text(r) == "this") {
                            c.tree.token_bounds(base).map(|(_, bt)| c.text_of(bt).to_string())
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                let Some(vname) = vif_ident else { continue };
                if vname == "this" || shadowed.contains(&vname) || !vif_names.contains(&vname) {
                    continue;
                }
                // If `base` was `this` (`this.vif`), `n` is the handle `this.vif` itself — only flag when
                // `n` is the base of an outer `MemberExpr` (`this.vif.<sig>`), so `this.vif = ...` and
                // `config_db::get(..., this.vif)` remain allowed!
                if c.kind(base) == NameExpr && c.tree.bare_text(base) == "this" {
                    continue;
                }
                let Some((_, sig_tok)) = c.tree.token_bounds(n) else { continue };
                let sig_name = c.text_of(sig_tok);
                let context_desc = match &via_phase {
                    Some(ph) => format!("helper '{}' (called from '{ph}')", m.name),
                    None => format!("'{}'", m.name),
                };
                out.add(
                    "UVM-PHS-013",
                    "VirtualInterfaceAccessInBuildPhase",
                    Severity::Warning,
                    format!(
                        "Virtual interface '{vname}' member '{sig_name}' is accessed in {context_desc} of component '{}'. Virtual interface signals must not be driven or sampled before connect_phase / run_phase.",
                        cls.name
                    ),
                    c.loc(sig_tok),
                );
            }
        }
    }
}
