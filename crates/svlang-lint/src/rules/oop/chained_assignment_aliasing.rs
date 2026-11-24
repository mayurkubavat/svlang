//! SV-OOP-012 (ChainedAssignmentAliasing): Disallows chained assignment
//! expressions (`h1 = h2 = h3;`) on class object handles.

use crate::support::class_items::{declarators, kids, op};
use crate::support::slang_ast::{ancestors, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

const VALUE_HEADS: &[&str] = &[
    "int",
    "integer",
    "bit",
    "logic",
    "reg",
    "byte",
    "shortint",
    "longint",
    "time",
    "real",
    "shortreal",
    "realtime",
    "string",
    "enum",
    "struct",
    "union",
    "chandle",
    "event",
];

fn handle_vars_in_scope(c: &Cst, scope: NodeId) -> HashSet<String> {
    let mut handles = HashSet::new();
    for dd in subtree(c, scope).into_iter().filter(|&n| c.kind(n) == DataDecl) {
        let Some(dt) = c.tree.child(dd, DataType) else { continue };
        let Some(first) = c.first_tok(dt).map(|t| c.text_of(t)) else { continue };
        if VALUE_HEADS.contains(&first) {
            continue;
        }
        let raw = c.tree.bare_text(dt);
        let base =
            raw.split('#').next().unwrap_or(&raw).rsplit("::").next().unwrap_or(&raw).trim().to_ascii_lowercase();
        if base.is_empty()
            || VALUE_HEADS.contains(&base.as_str())
            || ["_t", "_e", "_enum", "_s", "_u"].iter().any(|sfx| base.ends_with(sfx))
        {
            continue;
        }
        for (dec, name, _) in declarators(c, dd) {
            if c.tree.child(dec, Dim).is_none() {
                handles.insert(name);
            }
        }
    }
    handles
}

pub(crate) fn chained_assignment(c: &Cst, s: &mut Sink) {
    let is_assign = |n: NodeId| c.kind(n) == BinaryExpr && op(c, n) == "=";
    for n in c.nodes(BinaryExpr) {
        if !is_assign(n) {
            continue;
        }
        // Skip inner node of a 3+-way chain so `a = b = c` reports once on the outer assignment.
        if c.parent(n).is_some_and(is_assign) {
            continue;
        }
        let k = kids(c, n);
        if !k.iter().take(2).any(|&x| is_assign(x)) {
            continue;
        }
        let Some(&lhs) = k.first() else { continue };
        let lhs_name = c.tree.bare_text(lhs);
        let Some(scope) = ancestors(c, n)
            .find(|&a| matches!(c.kind(a), ClassDecl | ModuleDecl | PackageDecl | FunctionDecl | TaskDecl))
        else {
            continue;
        };
        let handles = handle_vars_in_scope(c, scope);
        if !handles.contains(&lhs_name) {
            continue;
        }
        let Some(at) = c.toks(n).next() else { continue };
        s.add(
            "SV-OOP-012",
            "ChainedAssignmentAliasing",
            Severity::Warning,
            "Chained class-handle assignment expression detected (e.g. 'h1 = h2 = h3;'). \
             Chained assignments alias multiple handles to the same underlying object instance without deep-copy isolation. \
             Assign or clone each handle in a separate statement."
                .into(),
            c.loc(at),
        );
    }
}
