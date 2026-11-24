//! SVL-COV-020 (MissingPerInstanceOption): Embedded `covergroup` in a `uvm_component`
//! subclass omits `option.per_instance = 1`, collapsing multi-agent instance
//! coverage into type-only coverage.

use svlang::syntax::SyntaxKind::*;
use svlang_model::Design;

use crate::support::class_items::user_code;
use crate::support::slang_ast::{decl_name, enclosing_class, subtree};
use crate::support::uvm_kinds::is_component;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn missing_per_instance_option(d: &Design, csts: &[Cst], out: &mut Sink) {
    let comp_names: std::collections::HashSet<&str> =
        d.classes().iter().filter(|cls| !cls.is_library && is_component(d, cls)).map(|cls| cls.name.as_str()).collect();
    if comp_names.is_empty() {
        return;
    }
    for c in csts {
        for cg in c.nodes(CovergroupDecl) {
            if !user_code(c, cg) {
                continue;
            }
            let Some(cls_node) = enclosing_class(c, cg) else { continue };
            let cname = decl_name(c, cls_node);
            if !comp_names.contains(cname.as_str()) {
                continue;
            }
            let has_per_inst = subtree(c, cg).into_iter().any(|n| {
                c.kind(n) == MemberExpr
                    && c.last_text(n) == "per_instance"
                    && c.tree.bare_text(n).starts_with("option.")
            });
            if !has_per_inst {
                let cg_name = decl_name(c, cg);
                let tok = c.tree.child(cg, Name).and_then(|nm| c.first_tok(nm)).or_else(|| c.first_tok(cg));
                if let Some(t) = tok {
                    out.add(
                        "SVL-COV-020",
                        "MissingPerInstanceOption",
                        Severity::Info,
                        format!(
                            "Covergroup '{}' in UVM component '{}' omits 'option.per_instance = 1;'. \
Without per_instance enabled, multiple instances of '{}' merge coverage into a single type bin table, hiding per-port or per-channel coverage holes.",
                            cg_name, cname, cname
                        ),
                        c.loc(t),
                    );
                }
            }
        }
    }
}
