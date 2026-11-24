//! SVL-RND-036 (SoftConstraintShadowedInSameClass): Flags a `soft` constraint on
//! a single `rand` variable inside a class when the very same class also
//! declares an unconditional hard constraint referencing only that same
//! variable, rendering the `soft` default dead within its own class.

use std::collections::{HashMap, HashSet};

use crate::support::class_items::declarators;
use crate::support::constraints::constraint_blocks;
use crate::support::slang_ast::subtree;
use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn is_guarded_constraint_item(c: &Cst, item: NodeId, block: NodeId) -> bool {
    for anc in std::iter::successors(c.parent(item), |&a| c.parent(a)) {
        if anc == block {
            break;
        }
        if c.kind(anc) == ConstraintItem {
            if let Some(ft) = c.first_tok(anc) {
                if matches!(c.text_of(ft), "if" | "foreach") {
                    return true;
                }
            }
            if c.toks(anc).any(|t| c.text_of(t) == "->") {
                return true;
            }
        }
    }
    c.toks(item).any(|t| c.text_of(t) == "->")
}

fn referenced_rand_vars(c: &Cst, item: NodeId, rand_props: &HashSet<String>) -> HashSet<String> {
    let mut vars = HashSet::new();
    for sub in subtree(c, item) {
        if c.kind(sub) == NameExpr {
            let nm = c.tree.bare_text(sub);
            if rand_props.contains(&nm) {
                vars.insert(nm);
            }
        }
    }
    vars
}

pub(crate) fn soft_constraint_shadowed_in_same_class(c: &Cst, out: &mut Sink) {
    for cls in c.nodes(ClassDecl) {
        let Some(cls_tok) = c.first_tok(cls) else { continue };
        if is_library_tok(c, cls_tok) {
            continue;
        }
        let cname = c.tree.child(cls, Name).map(|n| c.tree.bare_text(n)).unwrap_or_default();

        // Collect `rand` / `randc` properties declared in `cls`.
        let mut rand_props = HashSet::new();
        for dd in c.tree.children(cls).filter(|&k| c.kind(k) == DataDecl) {
            let is_rand =
                c.toks(dd).take_while(|&t| c.text_of(t) != ";").any(|t| matches!(c.text_of(t), "rand" | "randc"));
            if !is_rand {
                continue;
            }
            for (_, name, _) in declarators(c, dd) {
                rand_props.insert(name);
            }
        }
        if rand_props.is_empty() {
            continue;
        }

        let mut hard_single: HashMap<String, String> = HashMap::new();
        let mut soft_items: Vec<(NodeId, String, String)> = Vec::new();
        for cb in constraint_blocks(c, cls) {
            let cb_name = c
                .parent(cb)
                .and_then(|cd| c.tree.child(cd, Name))
                .map(|n| c.tree.bare_text(n))
                .unwrap_or_else(|| "constraint".into());
            for item in subtree(c, cb) {
                if c.kind(item) != ConstraintItem {
                    continue;
                }
                let Some(ft) = c.first_tok(item) else { continue };
                let ft_txt = c.text_of(ft);
                if matches!(ft_txt, "if" | "foreach" | "solve" | "unique" | "disable") {
                    continue;
                }
                if is_guarded_constraint_item(c, item, cb) {
                    continue;
                }
                let is_soft = ft_txt == "soft";
                let vars = referenced_rand_vars(c, item, &rand_props);
                if vars.len() != 1 {
                    continue;
                }
                let v = vars.into_iter().next().unwrap();
                if is_soft {
                    soft_items.push((item, v, cb_name.clone()));
                } else {
                    hard_single.entry(v).or_insert_with(|| cb_name.clone());
                }
            }
        }

        let mut reported = HashSet::new();
        for (item, v, soft_blk) in soft_items {
            if let Some(hard_blk) = hard_single.get(&v) {
                if let Some(t0) = c.first_tok(item).filter(|&t| !is_library_tok(c, t)) {
                    if reported.insert((t0, v.clone())) {
                        out.add(
                            "SVL-RND-036",
                            "SoftConstraintShadowedInSameClass",
                            Severity::Warning,
                            format!(
                                "Soft constraint on '{v}' in '{cname}::{soft_blk}' is unconditionally overridden by hard constraint in '{cname}::{hard_blk}'. A hard constraint in the same class always wins over 'soft' (IEEE 1800 §18.5.14), making the soft default dead."
                            ),
                            c.loc(t0),
                        );
                    }
                }
            }
        }
    }
}
