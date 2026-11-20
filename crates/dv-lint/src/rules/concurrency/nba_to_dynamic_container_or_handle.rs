//! SV-CON-017 (NbaToDynamicContainerOrHandle): Flags non-blocking assignments
//! (`<=`) targeting local or module-scope dynamic arrays, queues, associative
//! arrays, `string` variables, or class handles (and elements of dynamically
//! sized containers), which are illegal or race-prone under IEEE 1800 §10.4.2.

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

pub(crate) fn nba_to_dynamic_container_or_handle(c: &Cst, out: &mut Sink) {
    let types = crate::expr_types::expr_types(c);
    // Collect local / module `DataDecl` variable declarations that are dynamic arrays,
    // queues, associative arrays, strings, or class handles (excluding class properties,
    // which SV-PRF-009 already checks).
    let mut dynamic_decls: std::collections::HashMap<(NodeId, String), String> = std::collections::HashMap::new();
    for dd in c.nodes(DataDecl) {
        let Some(scope) = std::iter::successors(c.parent(dd), |&a| c.parent(a)).find(|&a| {
            matches!(
                c.kind(a),
                ClassDecl | FunctionDecl | TaskDecl | SeqBlock | ParBlock | ModuleDecl | InterfaceDecl | ProgramDecl
            )
        }) else {
            continue;
        };
        if c.kind(scope) == ClassDecl {
            continue;
        }
        let Some(dt) = c.tree.child(dd, DataType) else { continue };
        let dt_txt = c.trimmed(dt);
        if dt_txt.starts_with("virtual ") {
            continue;
        }
        let is_string = dt_txt == "string";
        for d in c.tree.children(dd).filter(|&x| c.kind(x) == Declarator) {
            let Some(nm) = c.tree.child(d, Name) else { continue };
            let vname = c.tree.bare_text(nm);
            let mut has_dyn_dim = false;
            for dim in c.tree.children(d).filter(|&x| c.kind(x) == Dim) {
                let dtxt = c.trimmed(dim).replace(' ', "");
                if dtxt == "[]"
                    || dtxt == "[$]"
                    || dtxt.starts_with("[$:")
                    || dtxt == "[*]"
                    || dtxt == "[string]"
                    || dtxt == "[int]"
                {
                    has_dyn_dim = true;
                }
            }
            if is_string || has_dyn_dim {
                let full = if has_dyn_dim { format!("{dt_txt}[]") } else { dt_txt.clone() };
                dynamic_decls.insert((scope, vname), full);
            }
        }
    }

    for n in c.nodes(BinaryExpr) {
        if c.parent(n).map(|p| c.kind(p)) != Some(ExprStmt) {
            continue;
        }
        let Some(op) = c.tok(n, "<=") else { continue };
        if is_library_tok(c, op) {
            continue;
        }
        // Skip functions, where SV-LANG-005 already flags all non-interface `<=`.
        if std::iter::successors(c.parent(n), |&a| c.parent(a)).any(|a| c.kind(a) == FunctionDecl) {
            continue;
        }
        let Some(lhs) = c.tree.children(n).next() else { continue };
        let lhs = c.strip(lhs);
        // Only check plain local/module variable targets (`NameExpr` or `SelectExpr` on `NameExpr`)
        // so class member `this.prop <= ...` / `prop <= ...` stays owned by SV-PRF-009.
        let base_expr = if c.kind(lhs) == SelectExpr { c.tree.children(lhs).next().unwrap_or(lhs) } else { lhs };
        if c.kind(base_expr) != NameExpr {
            continue;
        }
        let vname = c.trimmed(base_expr);
        // Skip if `vname` is a class property of an enclosing class and not shadowed locally.
        let mut matched_ty: Option<String> = None;
        for anc in std::iter::successors(c.parent(n), |&a| c.parent(a)) {
            if let Some(ty) = dynamic_decls.get(&(anc, vname.clone())) {
                matched_ty = Some(ty.clone());
                break;
            }
            if c.kind(anc) == ClassDecl {
                break;
            }
        }
        if matched_ty.is_none() {
            // Also check `expr_types` for automatic local class handles declared inside tasks/modules.
            let not_in_class = !std::iter::successors(c.parent(n), |&a| c.parent(a)).any(|a| c.kind(a) == ClassDecl);
            if let Some(ty) = types.get(&(c.index(), base_expr)) {
                let t = ty.text.as_str();
                if not_in_class && (t == "string" || t.ends_with("[$]") || t.ends_with("[]")) {
                    matched_ty = Some(ty.text.clone());
                }
            }
        }
        let Some(ty_desc) = matched_ty else { continue };
        out.add(
            "SV-CON-017",
            "NbaToDynamicContainerOrHandle",
            Severity::Error,
            format!(
                "Non-blocking assignment '<=' to dynamically sized variable '{vname}' ('{ty_desc}'). IEEE 1800 §10.4.2 prohibits non-blocking assignments to dynamically sized variables, strings, queues, and dynamic arrays; use blocking '=' instead."
            ),
            c.loc(op),
        )
        .fixes
        .extend(c.token_fix(op, op, "=", "Replace '<=' with blocking '='"));
    }
}
