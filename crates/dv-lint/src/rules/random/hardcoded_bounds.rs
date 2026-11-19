//! SV-RND-040 (HardcodedBounds): Flags non-`soft` literal constant bounds on
//! `rand` properties in base classes (`virtual` classes or classes extended by
//! another class in the design), which prevent derived classes from widening
//! or overriding stimulus ranges without disabling the constraint block.

use crate::support::class_roles::{user_classes, Index};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use dv_model::{ClassId, Design};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

/// Non-`randc` `rand` properties declared directly on `decl`. (`soft` is
/// illegal on `randc` variables per IEEE 1800-2023 §18.5.14.)
fn non_randc_rand_fields(c: &Cst, decl: NodeId) -> HashSet<String> {
    let mut out = HashSet::new();
    for item in c.items(decl) {
        if c.kind(item) != DataDecl {
            continue;
        }
        let has_rand = c.toks(item).any(|t| c.text_of(t) == "rand");
        let has_randc = c.toks(item).any(|t| c.text_of(t) == "randc");
        if !has_rand || has_randc {
            continue;
        }
        for d in c.tree.children(item).filter(|&n| c.kind(n) == Declarator) {
            if c.tree.children(d).any(|x| c.kind(x) == Dim) {
                continue;
            }
            if let Some(nm) = c.tree.child(d, Name) {
                out.insert(c.tree.bare_text(nm));
            }
        }
    }
    out
}

/// Whether `n` (after stripping parentheses and unary `+`/`-`) is an integer
/// literal, returning its text.
fn literal_bound(c: &Cst, n: NodeId) -> Option<String> {
    match c.kind(n) {
        ParenExpr => c.tree.children(n).next().and_then(|k| literal_bound(c, k)),
        PrefixExpr if c.toks(n).next().is_some_and(|t| matches!(c.text_of(t), "+" | "-")) => {
            c.tree.children(n).next().and_then(|k| literal_bound(c, k)).map(|_| c.trimmed(n))
        }
        Literal => {
            let txt = c.trimmed(n);
            let first = txt.chars().next()?;
            (first.is_ascii_digit() || first == '\'').then_some(txt)
        }
        _ => None,
    }
}

/// True when every bound in an `inside` range list uses raw integer literals.
fn has_literal_range_elem(c: &Cst, list: NodeId) -> bool {
    if c.kind(list) != RangeList {
        return false;
    }
    c.tree.children(list).any(|el| {
        if c.kind(el) == OpenRange {
            c.tree.children(el).any(|b| literal_bound(c, b).is_some())
        } else {
            literal_bound(c, el).is_some()
        }
    })
}

fn rand_target<'a>(c: &'a Cst, n: NodeId, fields: &'a HashSet<String>) -> Option<&'a str> {
    match c.kind(n) {
        ParenExpr => c.tree.children(n).next().and_then(|k| rand_target(c, k, fields)),
        NameExpr if c.tree.children(n).next().is_none() => {
            let name = c.tree.bare_text(n);
            fields.get(name.as_str()).map(String::as_str)
        }
        MemberExpr => {
            let obj = c.tree.children(n).next()?;
            if c.kind(obj) == NameExpr && c.tree.bare_text(obj) == "this" {
                fields.get(c.last_text(n)).map(String::as_str)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Inspects a non-`soft` expression for hardcoded literal bounds on `fields`.
fn check_hard_expr(c: &Cst, n: NodeId, cname: &str, bname: &str, fields: &HashSet<String>, out: &mut Sink) {
    match c.kind(n) {
        ParenExpr => {
            if let Some(k) = c.tree.children(n).next() {
                check_hard_expr(c, k, cname, bname, fields, out);
            }
        }
        BinaryExpr => {
            let op = c.toks(n).next().map(|t| c.text_of(t)).unwrap_or("");
            if op == "&&" {
                for k in c.tree.children(n) {
                    check_hard_expr(c, k, cname, bname, fields, out);
                }
                return;
            }
            if !matches!(op, "<" | "<=" | ">" | ">=" | "==") {
                return;
            }
            let kids: Vec<NodeId> = c.tree.children(n).collect();
            let [lhs, rhs] = kids[..] else { return };
            let matched = if let (Some(f), Some(lit)) = (rand_target(c, lhs, fields), literal_bound(c, rhs)) {
                // Skip non-negativity invariant `x >= 0` / `x > 0`.
                if matches!((op, lit.as_str()), (">=" | ">", "0")) {
                    None
                } else {
                    Some(f)
                }
            } else if let (Some(lit), Some(f)) = (literal_bound(c, lhs), rand_target(c, rhs, fields)) {
                if matches!((op, lit.as_str()), ("<=" | "<", "0")) {
                    None
                } else {
                    Some(f)
                }
            } else {
                None
            };
            if let Some(field) = matched {
                report(c, n, cname, bname, field, out);
            }
        }
        InsideExpr => {
            let kids: Vec<NodeId> = c.tree.children(n).collect();
            let [lhs, list] = kids[..] else { return };
            if let Some(field) = rand_target(c, lhs, fields) {
                if has_literal_range_elem(c, list) {
                    report(c, n, cname, bname, field, out);
                }
            }
        }
        _ => {}
    }
}

fn report(c: &Cst, n: NodeId, cname: &str, bname: &str, field: &str, out: &mut Sink) {
    let expr_txt = c.trimmed(n);
    let where_blk = if bname.is_empty() { String::new() } else { format!(" in constraint '{bname}'") };
    out.add(
        "SV-RND-040",
        "HardcodedBounds",
        Severity::Warning,
        format!(
            "Hard literal bound '{expr_txt}' on rand field '{field}' in base class '{cname}'{where_blk} prevents derived classes from widening or overriding the range without 'constraint_mode(0)'. Declare default bounds 'soft' or parameterize protocol limits."
        ),
        c.loc_of(n),
    );
}

fn walk_constraint_node(c: &Cst, n: NodeId, cname: &str, bname: &str, fields: &HashSet<String>, out: &mut Sink) {
    match c.kind(n) {
        ConstraintBlock => {
            for k in c.node_kids(n) {
                walk_constraint_node(c, k, cname, bname, fields, out);
            }
        }
        ConstraintItem => {
            let toks: Vec<&str> = c.toks(n).map(|t| c.text_of(t)).collect();
            if toks.iter().any(|t| matches!(*t, "soft" | "solve" | "unique" | "disable")) {
                return;
            }
            let kw = c.keyword(n).map(|t| c.text_of(t)).unwrap_or("");
            if kw == "if" || kw == "foreach" || toks.contains(&"->") {
                for k in c.node_kids(n) {
                    if matches!(c.kind(k), ConstraintItem | ConstraintBlock) {
                        walk_constraint_node(c, k, cname, bname, fields, out);
                    }
                }
                return;
            }
            if let Some(e) = c.tree.children(n).next() {
                check_hard_expr(c, e, cname, bname, fields, out);
            }
        }
        _ => {}
    }
}

pub(crate) fn hardcoded_bounds(d: &Design, csts: &[Cst], out: &mut Sink) {
    let ix = Index::new(csts);
    let uclasses: Vec<_> = user_classes(d).collect();
    let mut extended: HashSet<ClassId> = HashSet::new();
    for &cls in &uclasses {
        for anc in d.ancestors(cls.id) {
            extended.insert(anc.id);
        }
    }

    for cls in uclasses {
        let Some((c, decl)) = ix.decl(cls.loc) else { continue };
        if c.kind(decl) != ClassDecl {
            continue;
        }
        let is_virtual = c.toks(decl).any(|t| c.text_of(t) == "virtual");
        if !is_virtual && !extended.contains(&cls.id) {
            continue;
        }
        // `:final` classes (IEEE 1800-2023 §8.14) cannot be extended.
        let toks: Vec<&str> = c.toks(decl).map(|t| c.text_of(t)).collect();
        if toks.windows(2).any(|w| w == [":", "final"]) {
            continue;
        }
        let fields = non_randc_rand_fields(c, decl);
        if fields.is_empty() {
            continue;
        }
        for item in c.items(decl) {
            let bname = c.tree.child(item, Name).map(|n| c.tree.bare_text(n)).unwrap_or_default();
            match c.kind(item) {
                ConstraintDecl => {
                    if let Some(blk) = c.tree.child(item, ConstraintBlock) {
                        walk_constraint_node(c, blk, &cls.name, &bname, &fields, out);
                    }
                }
                ConstraintPrototype => {
                    let full = format!("{}::{bname}", cls.name);
                    let ext = ix.csts.iter().find_map(|k| {
                        k.first_named(ConstraintDecl, &full)
                            .and_then(|n| k.tree.child(n, ConstraintBlock).map(|b| (k, b)))
                    });
                    if let Some((kc, blk)) = ext {
                        walk_constraint_node(kc, blk, &cls.name, &bname, &fields, out);
                    }
                }
                _ => {}
            }
        }
    }
}
