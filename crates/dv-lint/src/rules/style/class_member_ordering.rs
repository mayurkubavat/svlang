//! SV-STY-024 (ClassMemberOrdering): Enforces canonical SystemVerilog / UVM
//! class member ordering inside multi-line classes:
//! 1. Parameters & typedefs (`parameter`, `localparam`, `typedef`)
//! 2. Properties / data declarations
//! 3. Constraints & covergroups
//! 4. Constructor (`new`)
//! 5. Methods (`function`, `task`, `extern`)

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn member_rank(c: &Cst, item: NodeId) -> Option<(u8, &'static str)> {
    match c.kind(item) {
        ParamDecl | TypedefDecl | ForwardTypedef => Some((0, "type/parameter declaration")),
        DataDecl => Some((1, "property declaration")),
        ConstraintDecl | ConstraintPrototype | CovergroupDecl => Some((2, "constraint/covergroup")),
        FunctionDecl | TaskDecl | MethodPrototype => {
            let name = crate::support::slang_ast::last_name(c, item);
            if name == "new" {
                Some((3, "constructor 'new'"))
            } else {
                Some((4, "method declaration"))
            }
        }
        _ => None,
    }
}

pub(crate) fn class_member_ordering(c: &Cst, out: &mut Sink) {
    let layout = crate::shared::tree_layout(c);
    for cls in c.nodes(ClassDecl) {
        let Some((t0, t1)) = c.tree.token_bounds(cls) else { continue };
        if is_library_tok(c, t0) {
            continue;
        }
        let Some((f0, l0, _)) = layout.source_token_pos(c.tree, t0) else { continue };
        let Some((f1, l1, _)) = layout.source_token_pos(c.tree, t1) else { continue };
        // Skip single-line compact class mocks in tests.
        if f0 != f1 || !c.tree.mains.contains(&f0) || l0 == l1 {
            continue;
        }
        let cname = c.tree.child(cls, Name).map(|n| c.tree.bare_text(n)).unwrap_or_else(|| "class".into());

        let mut max_seen: Option<(u8, &'static str)> = None;
        for item in c.tree.children(cls) {
            let Some(it0) = c.first_tok(item) else { continue };
            if is_library_tok(c, it0) {
                continue;
            }
            let Some((rank, label)) = member_rank(c, item) else { continue };
            if let Some((prev_rank, prev_label)) = max_seen {
                if rank < prev_rank {
                    out.add(
                        "SV-STY-024",
                        "ClassMemberOrdering",
                        Severity::Info,
                        format!(
                            "Class '{cname}' places {label} after {prev_label}. Order class members as: typedefs/parameters, properties, constraints/covergroups, constructor 'new', then methods."
                        ),
                        c.loc(it0),
                    );
                    break;
                }
            }
            if max_seen.is_none_or(|(pr, _)| rank > pr) {
                max_seen = Some((rank, label));
            }
        }
    }
}
