//! SV-PRF-001 (PassArrayByValue): Passing dynamic arrays, queues, associative
//! arrays, unpacked arrays, or unpacked structs by value (input) causes
//! expensive memory duplication on every call.

use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;
use svlang_model::{Design, Direction, TypeShape};

pub(crate) fn pass_array_by_value(d: &Design, csts: &[Cst], out: &mut Sink) {
    let ix = crate::support::class_roles::Index::new(csts);
    for cls in d.classes() {
        if cls.is_parameterized {
            continue;
        }
        for sub in &cls.methods {
            if sub.is_extern || sub.is_builtin {
                continue;
            }
            let can_fix_sub = !sub.is_virtual && !sub.declared_virtual;
            for (arg_idx, a) in sub.arguments.iter().enumerate() {
                if a.direction != Direction::In || a.shape == TypeShape::Other {
                    continue;
                }
                let mutated = sub.assignments.iter().any(|asg| asg.root_name == a.name);
                let fix = (can_fix_sub && !mutated)
                    .then(|| {
                        ix.decl(sub.loc).and_then(|(c, decl)| const_ref_fix(c, csts, decl, &sub.name, &a.name, arg_idx))
                    })
                    .flatten();
                out.add(
                    "SV-PRF-001",
                    "PassArrayByValue",
                    Severity::Warning,
                    format!(
                        "Argument '{}' of type '{}' in method '{}' is passed by value (input). Use 'const ref' instead.",
                        a.name, a.canonical_type, sub.name
                    ),
                    a.loc,
                )
                .fixes
                .extend(fix);
            }
        }
    }
}

fn const_ref_fix(
    c: &Cst,
    csts: &[Cst],
    decl: NodeId,
    method_name: &str,
    arg_name: &str,
    arg_idx: usize,
) -> Option<crate::fix::Fix> {
    // Skip if any call site in the compilation passes a non-lvalue expression (e.g. literal or pattern)
    // at `arg_idx`, because `const ref` requires an lvalue variable/member/element in IEEE 1800 §13.5.2.
    for cst in csts {
        for call in cst.nodes(CallExpr) {
            let Some(callee) = cst.tree.children(call).next() else { continue };
            let cname = match cst.kind(callee) {
                NameExpr => cst.tree.bare_text(callee),
                MemberExpr => {
                    cst.tree.token_bounds(callee).map(|(_, b)| cst.text_of(b).to_string()).unwrap_or_default()
                }
                _ => continue,
            };
            if cname != method_name {
                continue;
            }
            if let Some(al) = cst.tree.child(call, ArgList) {
                let args: Vec<NodeId> = cst.tree.children(al).filter(|&x| cst.kind(x) == Arg).collect();
                if let Some(&arg_node) = args.get(arg_idx) {
                    if let Some(expr) = cst.tree.children(arg_node).find(|&x| cst.kind(x) != Attribute) {
                        let expr = cst.strip(expr);
                        if !matches!(cst.kind(expr), NameExpr | MemberExpr | SelectExpr) {
                            return None;
                        }
                    }
                }
            }
        }
    }

    let list = c.tree.child(decl, PortList)?;
    for p in c.tree.children(list).filter(|&p| c.kind(p) == Port) {
        let decls: Vec<NodeId> = c.tree.children(p).filter(|&d| c.kind(d) == Declarator).collect();
        let [d_node] = decls.as_slice() else { continue };
        let nm = c.tree.child(*d_node, Name)?;
        if c.tree.bare_text(nm) != arg_name {
            continue;
        }
        // Skip ports with a default value (`byte data[] = ...`), since `const ref` defaults are restricted.
        if c.tok(*d_node, "=").is_some() {
            return None;
        }
        let dt = c.tree.child(p, DataType)?;
        let dt_tok = c.first_tok(dt)?;
        let raw_fix = if let Some(in_tok) = c.tok(p, "input") {
            c.token_fix(in_tok, in_tok, "const ref", "Pass read-only aggregate argument by 'const ref'")?
        } else {
            // Check no earlier sibling port carried a sticky direction onto an omitted-type port.
            let kt = &c.tree.tokens[dt_tok as usize];
            if kt.from_macro {
                return None;
            }
            c.file_fix(
                kt.origin.file,
                kt.origin.offset as usize,
                kt.origin.offset as usize,
                "const ref ",
                "Pass read-only aggregate argument by 'const ref'",
            )
        };
        return crate::fix::with_safety(Some(raw_fix), crate::fix::FixSafety::Unsafe);
    }
    None
}
