//! SVA-RST-001 (DisableIffReset): Concurrent assertions and property
//! declarations should include a 'disable iff (...)' reset guard (unless
//! covered by `default disable iff` or the paired property/assertion, since
//! nested `disable iff` is illegal per IEEE 1800-2023 §16.12.1).

use crate::support::slang_ast::{ancestors, subtree};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn has_disable(c: &Cst, n: NodeId) -> bool {
    subtree(c, n).into_iter().any(|k| c.kind(k) == DisableIff)
}

fn scope_has_default_disable(c: &Cst, n: NodeId) -> bool {
    for scope in ancestors(c, n) {
        if matches!(c.kind(scope), ModuleDecl | InterfaceDecl | CheckerDecl | ProgramDecl) {
            let toks = c.tree.token_range(scope);
            for win in toks.windows(3) {
                if c.text_of(win[0]) == "default" && c.text_of(win[1]) == "disable" && c.text_of(win[2]) == "iff" {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn disable_iff_reset(c: &Cst, s: &mut Sink) {
    let mut guarded_props: HashSet<String> = HashSet::new();
    let mut unguarded_props: HashSet<String> = HashSet::new();
    let mut caller_guarded_props: HashSet<String> = HashSet::new();

    for n in c.nodes(PropertyDecl) {
        let Some(name) = c.tree.child(n, Name).map(|nm| c.tree.bare_text(nm)) else { continue };
        if has_disable(c, n) {
            guarded_props.insert(name);
        } else {
            unguarded_props.insert(name);
        }
    }
    for n in c.nodes(ConcurrentAssert) {
        if has_disable(c, n) {
            for t in c.toks(n) {
                let s = c.text_of(t);
                if unguarded_props.contains(s) {
                    caller_guarded_props.insert(s.to_string());
                }
            }
        }
    }

    for n in c.nodes(PropertyDecl) {
        let Some(name_tok) = c.tree.child(n, Name).and_then(|nm| c.first_tok(nm)) else { continue };
        let pname = c.text_of(name_tok);
        if c.tree.child(n, PropertyExpr).is_none()
            || has_disable(c, n)
            || caller_guarded_props.contains(pname)
            || scope_has_default_disable(c, n)
        {
            continue;
        }
        s.add(
            "SVA-RST-001",
            "DisableIffReset",
            Severity::Warning,
            format!(
                "Property declaration '{pname}' lacks a 'disable iff (...)' reset gating condition. \
                 Add 'disable iff (!rst_n)' on either the property or a 'default disable iff' in the enclosing module."
            ),
            c.loc(name_tok),
        );
    }
    for n in c.nodes(ConcurrentAssert) {
        let Some(kw) = c.keyword(n) else { continue };
        let k = c.text_of(kw);
        if !matches!(k, "assert" | "assume")
            || c.tree.child(n, PropertyExpr).is_none()
            || has_disable(c, n)
            || scope_has_default_disable(c, n)
        {
            continue;
        }
        // Per IEEE 1800-2023 §16.12.1, nested `disable iff` is illegal: skip if this assertion
        // instantiates a named property declared in the file (either already guarded, or already
        // reported once on its `property` declaration above).
        let refs_declared_prop = c.toks(n).any(|t| {
            let id = c.text_of(t);
            guarded_props.contains(id) || unguarded_props.contains(id)
        });
        if refs_declared_prop {
            continue;
        }
        s.add(
            "SVA-RST-001",
            "DisableIffReset",
            Severity::Warning,
            format!(
                "Inline concurrent '{k} property' lacks a 'disable iff (...)' reset gating condition. \
                 Add a 'disable iff' clause or 'default disable iff' to gate checking while under reset."
            ),
            c.loc(kw),
        );
    }
}
