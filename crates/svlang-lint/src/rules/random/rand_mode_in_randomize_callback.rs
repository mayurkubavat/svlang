//! SVL-RND-035 (RandModeInRandomizeCallback): Mutating `.rand_mode(val)` or
//! `.constraint_mode(val)` inside `post_randomize()`.

use svlang::syntax::SyntaxKind::*;

use crate::expr_types::call_args;
use crate::support::class_items::{kids, user_code};
use crate::support::slang_ast::{decl_name, last_name, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn rand_mode_in_randomize_callback(c: &Cst, out: &mut Sink) {
    for cls in c.nodes(ClassDecl) {
        if !user_code(c, cls) {
            continue;
        }
        let cname = decl_name(c, cls);
        if cname.is_empty() {
            continue;
        }
        for f in c.tree.children(cls).filter(|&k| c.kind(k) == FunctionDecl) {
            if last_name(c, f) != "post_randomize" {
                continue;
            }
            for call in subtree(c, f) {
                if c.kind(call) != CallExpr {
                    continue;
                }
                // 0-arg `x.rand_mode()` is a pure query returning `int` (IEEE 1800 § 18.8); only 1-arg calls mutate mode!
                if call_args(c, call).is_empty() {
                    continue;
                }
                let Some(&callee) = kids(c, call).first() else { continue };
                let (recv, mname) = match c.kind(callee) {
                    NameExpr => ("this".to_string(), c.tree.bare_text(callee)),
                    MemberExpr => {
                        let r =
                            kids(c, callee).first().map(|&b| c.tree.bare_text(b)).unwrap_or_else(|| "this".to_string());
                        (r, c.last_text(callee).to_string())
                    }
                    _ => continue,
                };
                if matches!(mname.as_str(), "rand_mode" | "constraint_mode") {
                    if let Some(tok) = c.first_tok(call) {
                        out.add(
                            "SVL-RND-035",
                            "RandModeInRandomizeCallback",
                            Severity::Warning,
                            format!(
                                "'post_randomize()' in class '{}' calls '{}.{}()'. Solver randomization has already completed when post_randomize() runs, \
so changing rand_mode or constraint_mode here has no effect on the current transaction and silently mutates solver state for subsequent randomize() calls; move mode setup to pre_randomize().",
                                cname, recv, mname
                            ),
                            c.loc(tok),
                        );
                    }
                }
            }
        }
    }
}
