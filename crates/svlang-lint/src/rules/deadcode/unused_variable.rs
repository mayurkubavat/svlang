//! SV-DED-001 (UnusedVariable): Local variables declared in tasks or functions
//! should be referenced.

use crate::support::handles::body_refs_if_good;
use crate::support::slang_ast::kids;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

/// The classes slang's model lists: non-generic classes of `$unit`,
/// packages and other such classes (not of modules, not generic).
fn model_classes(c: &Cst) -> Vec<NodeId> {
    let generic = |k: NodeId| c.tree.child(k, Header).and_then(|h| c.tree.child(h, ParamList)).is_some();
    c.nodes(ClassDecl)
        .into_iter()
        .filter(|&k| !generic(k))
        .filter(|&k| {
            crate::support::slang_ast::ancestors(c, k)
                .all(|a| matches!(c.kind(a), SourceFile | PackageDecl) || (c.kind(a) == ClassDecl && !generic(a)))
        })
        .collect()
}

pub(crate) fn unused_variable(c: &Cst, out: &mut Sink) {
    for cls in model_classes(c) {
        for m in kids(c, cls).into_iter().filter(|&m| matches!(c.kind(m), FunctionDecl | TaskDecl)) {
            let Some(body) = c.tree.child(m, Body) else { continue };
            let mut locals = Vec::new();
            for d in kids(c, body).into_iter().filter(|&d| c.kind(d) == DataDecl) {
                let first = c.first_tok(d).map(|t| c.text_of(t)).unwrap_or("");
                if matches!(first, "input" | "output" | "inout" | "ref" | "typedef" | "localparam" | "parameter") {
                    continue;
                }
                for decl in c.tree.children(d).filter(|&x| c.kind(x) == Declarator) {
                    if let Some(nm) = c.tree.child(decl, Name) {
                        locals.push((decl, nm, c.tree.bare_text(nm)));
                    }
                }
            }
            if locals.is_empty() {
                continue;
            }
            let used_nodes = body_refs_if_good(c, body);
            let fallback_names: HashSet<String> =
                if used_nodes.is_none() { syntactic_body_names(c, body) } else { HashSet::new() };
            let mname = last_name_of(c, m);
            for (decl, nm, name) in locals {
                if let Some(ref used) = used_nodes {
                    if used.contains(&decl) {
                        continue;
                    }
                } else if fallback_names.contains(&name) {
                    continue;
                }
                let Some(loc) = c.first_tok(nm).map(|t| c.loc(t)) else { continue };
                out.add(
                    "SV-DED-001",
                    "UnusedVariable",
                    Severity::Info,
                    format!("Local variable '{name}' in method '{mname}' is declared but never referenced."),
                    loc,
                );
            }
        }
    }
}

/// Fallback when semantic binding in `body` fails due to uncompiled imports:
/// records every identifier referenced outside `Declarator`s in `body` so
/// clearly used local variables are not falsely reported as unused.
fn syntactic_body_names(c: &Cst, body: NodeId) -> HashSet<String> {
    let mut names = HashSet::new();
    let in_decl_name = |e: NodeId| {
        crate::support::slang_ast::ancestors(c, e).take_while(|&a| a != body).any(|a| {
            c.kind(a) == Declarator
                && c.tree.child(a, Name).is_some_and(|nm| {
                    c.tree
                        .token_bounds(nm)
                        .zip(c.tree.token_bounds(e))
                        .is_some_and(|((a0, a1), (b0, b1))| a0 <= b0 && b1 <= a1)
                })
        })
    };
    for e in crate::support::slang_ast::subtree(c, body) {
        if c.kind(e) == NameExpr && !in_decl_name(e) {
            if let Some(t) = c.first_tok(e) {
                names.insert(c.text_of(t).to_string());
            }
        } else if c.kind(e) == LoopStmt && c.keyword(e).is_some_and(|k| c.text_of(k) == "foreach") {
            let toks: Vec<u32> = c.toks(e).collect();
            if let Some(&t) = toks.get(2).filter(|_| toks.get(1).is_some_and(|&p| c.text_of(p) == "(")) {
                names.insert(c.text_of(t).to_string());
            }
        } else if c.kind(e) == ScopedExpr {
            names.insert(c.last_text(e).to_string());
        }
    }
    names
}

fn last_name_of(c: &Cst, m: NodeId) -> String {
    crate::support::slang_ast::last_name(c, m)
}
