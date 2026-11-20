//! SV-COV-021 (PerInstanceZeroOnClassCovergroup): Flags `option.per_instance = 0`
//! explicitly assigned inside a `covergroup` declared in a `class` or a
//! parameterized `module`/`interface`, which merges multi-instance coverage
//! holes into type-level totals.

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::SyntaxKind::*;

pub(crate) fn per_instance_zero_on_class_covergroup(c: &Cst, out: &mut Sink) {
    for cg in c.nodes(CovergroupDecl) {
        let in_class_or_param_unit = std::iter::successors(c.parent(cg), |&a| c.parent(a)).any(|a| match c.kind(a) {
            ClassDecl => true,
            ModuleDecl | InterfaceDecl => {
                c.tree.child(a, Header).is_some_and(|h| c.tree.child(h, ParamList).is_some())
                    || c.tree
                        .children(a)
                        .any(|k| c.kind(k) == ParamDecl && c.toks(k).any(|t| c.text_of(t) == "parameter"))
            }
            _ => false,
        });
        if !in_class_or_param_unit {
            continue;
        }
        let cg_name = c.tree.child(cg, Name).map(|n| c.tree.bare_text(n)).unwrap_or_else(|| "covergroup".into());
        for opt in c.subtree(cg).into_iter().filter(|&n| c.kind(n) == CoverageOption) {
            let Some(t0) = c.first_tok(opt) else { continue };
            if is_library_tok(c, t0) {
                continue;
            }
            let txt = c.trimmed(opt).replace([' ', '\t'], "");
            if txt == "option.per_instance=0;" || txt == "option.per_instance=1'b0;" || txt == "option.per_instance=0" {
                out.add(
                    "SV-COV-021",
                    "PerInstanceZeroOnClassCovergroup",
                    Severity::Warning,
                    format!(
                        "Covergroup '{cg_name}' explicitly sets 'option.per_instance = 0' inside a class or parameterized design unit. Disabling per-instance tracking collapses distinct agent or channel coverage holes into type-level aggregates; set 'option.per_instance = 1'."
                    ),
                    c.loc(t0),
                );
            }
        }
    }
}
