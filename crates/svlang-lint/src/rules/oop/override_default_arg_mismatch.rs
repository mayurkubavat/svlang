//! SVL-OOP-014 (OverrideDefaultArgMismatch): Virtual method override in a derived
//! class changes or omits a default argument value declared on the base virtual
//! method (IEEE 1800 § 8.20 static-type default value trap).

use svlang::syntax::{NodeId, SyntaxKind::*};

use crate::support::class_items::{kids, user_code};
use crate::support::slang_ast::{class_chain, decl_name, last_name};
use crate::syntax::Cst;
use crate::{Severity, Sink};

/// Extracts `(arg_name, Option<normalized_default_expr>, port_loc_tok)` per formal port.
fn extract_port_defaults(c: &Cst, decl: NodeId) -> Vec<(String, Option<String>, u32)> {
    let Some(pl) = c.tree.child(decl, PortList) else { return Vec::new() };
    let mut out = Vec::new();
    for p in c.tree.children(pl).filter(|&k| c.kind(k) == Port) {
        for dec in c.tree.children(p).filter(|&k| c.kind(k) == Declarator) {
            let Some(nm) = c.tree.child(dec, Name) else { continue };
            let Some(tok) = c.first_tok(nm) else { continue };
            let aname = c.tree.bare_text(nm);
            let dflt = kids(c, dec)
                .into_iter()
                .find(|&k| !matches!(c.kind(k), Name | Dim))
                .map(|e| c.tree.bare_text(e).chars().filter(|ch| !ch.is_whitespace()).collect::<String>());
            out.push((aname, dflt, tok));
        }
    }
    out
}

fn find_method_decl(c: &Cst, cls: NodeId, mname: &str) -> Option<(NodeId, bool)> {
    for item in c.tree.children(cls) {
        if matches!(c.kind(item), FunctionDecl | TaskDecl | MethodPrototype) && last_name(c, item) == mname {
            let is_virt = c.toks(item).take_while(|&t| c.text_of(t) != "(").any(|t| c.text_of(t) == "virtual");
            return Some((item, is_virt));
        }
    }
    None
}

pub(crate) fn override_default_arg_mismatch(c: &Cst, out: &mut Sink) {
    for cls in c.nodes(ClassDecl) {
        if !user_code(c, cls) {
            continue;
        }
        let chain = class_chain(c, cls);
        if chain.len() < 2 {
            continue;
        }
        let cname = decl_name(c, cls);
        if cname.starts_with("uvm_") {
            continue;
        }
        for m in c.tree.children(cls).filter(|&k| matches!(c.kind(k), FunctionDecl | TaskDecl | MethodPrototype)) {
            let mname = last_name(c, m);
            if mname.is_empty() || mname == "new" {
                continue;
            }
            // Find nearest ancestor declaring `mname`, and check whether any ancestor declares it `virtual`.
            let mut nearest_base: Option<(NodeId, NodeId)> = None;
            let mut any_virtual = false;
            for &anc in &chain[1..] {
                if let Some((bm, bvirt)) = find_method_decl(c, anc, &mname) {
                    if nearest_base.is_none() {
                        nearest_base = Some((anc, bm));
                    }
                    if bvirt {
                        any_virtual = true;
                    }
                }
            }
            let Some((base_cls, base_m)) = nearest_base else { continue };
            if !any_virtual {
                continue;
            }
            let bname = decl_name(c, base_cls);
            // Skip standard UVM library base methods (`uvm_object::convert2string`, `print`, etc.) unless overridden default actually differs on user-defined bases,
            // wait: even on UVM bases, changing a default is bad, EXCEPT UVM macros never declare method args we walk (`user_code` + `!loc.expansion`!).
            if bname.starts_with("uvm_") {
                continue;
            }
            let drv_ports = extract_port_defaults(c, m);
            let base_ports = extract_port_defaults(c, base_m);
            if drv_ports.len() != base_ports.len() {
                // Parameter count mismatch is already covered by SVL-OOP-013.
                continue;
            }
            for ((d_arg, d_def, d_tok), (_b_arg, b_def, _)) in drv_ports.into_iter().zip(base_ports) {
                if c.loc(d_tok).expansion.is_some() {
                    continue;
                }
                if let Some(bd) = b_def {
                    match d_def {
                        None => {
                            out.add(
                                "SVL-OOP-014",
                                "OverrideDefaultArgMismatch",
                                Severity::Warning,
                                format!(
                                    "Override '{}::{}' omits default value '{}' for argument '{}' declared on base virtual method '{}::{}'. \
Calls through a '{}' handle will still supply '{}', whereas calls through '{}' require an explicit argument (IEEE 1800 § 8.20).",
                                    cname, mname, bd, d_arg, bname, mname, bname, bd, cname
                                ),
                                c.loc(d_tok),
                            );
                        }
                        Some(dd) if dd != bd => {
                            out.add(
                                "SVL-OOP-014",
                                "OverrideDefaultArgMismatch",
                                Severity::Warning,
                                format!(
                                    "Override '{}::{}' changes default value of argument '{}' from '{}' (in '{}::{}') to '{}'. \
In SystemVerilog (IEEE 1800 § 8.20), default arguments are bound statically from the handle type, so polymorphic calls via a '{}' handle silently pass '{}' instead of '{}'.",
                                    cname, mname, d_arg, bd, bname, mname, dd, bname, bd, dd
                                ),
                                c.loc(d_tok),
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
