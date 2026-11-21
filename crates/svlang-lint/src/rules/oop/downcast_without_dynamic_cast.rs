//! SV-OOP-011 (DowncastWithoutDynamicCast): Flags static casting syntax (e.g.
//! `ExtendedClass'(base_handle)`) used on class handles instead of `$cast()`.

use crate::support::ast::unparen;
use crate::support::class_items::{decl_name, own};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;
use svlang_model::Design;

const ROLE_SUFFIX_HINTS: &[&str] =
    &["_item", "_seq", "_driver", "_monitor", "_scoreboard", "_env", "_component", "_object"];

const VALUE_TYPE_SUFFIXES: &[&str] =
    &["_t", "_e", "_enum", "_s", "_u", "_mode", "_state", "_op", "_type", "_kind", "_struct", "_union"];

/// Strips leading package scope (`pkg::`) and specialization (`#(...)`) so
/// `uvm_pkg::uvm_bitstream_t` and `my_seq#(int)` resolve to their bare type name.
fn bare_type_name(ty: &str) -> &str {
    let before_params = ty.split('#').next().unwrap_or(ty).trim();
    before_params.rsplit("::").next().unwrap_or(before_params).trim()
}

/// True when `rhs` is syntactically an integral/arithmetic/string/concatenation
/// value expression that can never evaluate to a class handle.
fn is_non_handle_operand(c: &Cst, rhs: NodeId) -> bool {
    let e = unparen(c, rhs);
    matches!(c.kind(e), Literal | ConcatExpr | AssignPattern | BinaryExpr | PrefixExpr | PostfixExpr)
}

pub(crate) fn downcast(d: &Design, csts: &[Cst], s: &mut Sink) {
    let mut known: HashSet<String> =
        d.classes().iter().filter(|k| !k.is_parameterized).map(|k| k.name.clone()).collect();
    let uvm_classes = crate::uvm_api::for_version(None).map(|p| &p.classes);
    let mut typedefs: HashSet<String> = HashSet::new();
    let mut pending = Vec::new();
    for c in csts {
        for n in c.tree.preorder() {
            match c.kind(n) {
                TypedefDecl => {
                    if let Some(nm) = c.tree.child(n, Name) {
                        typedefs.insert(c.tree.bare_text(nm));
                    }
                }
                ClassDecl => {
                    if let Some((name, _)) = decl_name(c, n) {
                        if !name.is_empty() {
                            known.insert(name);
                        }
                    }
                }
                CastExpr if own(c, n).contains(&"(") => {
                    let mut ch = c.tree.children(n);
                    let Some(left) = ch.next() else { continue };
                    // `void'(f());` as a statement is a void-casted call, not a class cast.
                    if c.tree.bare_text(left) == "void" && c.parent(n).is_some_and(|p| c.kind(p) == ExprStmt) {
                        continue;
                    }
                    if ch.next().is_some_and(|rhs| is_non_handle_operand(c, rhs)) {
                        continue;
                    }
                    let ty = c.trimmed(left);
                    let Some(at) = c.first_tok(n) else { continue };
                    pending.push((ty, c.loc(at)));
                }
                _ => {}
            }
        }
    }
    for (ty, loc) in pending {
        let base = bare_type_name(&ty);
        if base.is_empty() {
            continue;
        }
        let is_known_class = known.contains(&ty) || known.contains(base);
        if !is_known_class {
            if typedefs.contains(&ty)
                || typedefs.contains(base)
                || VALUE_TYPE_SUFFIXES.iter().any(|suf| base.ends_with(suf))
            {
                continue;
            }
            let looks_like_class = if base.starts_with("uvm_") {
                uvm_classes.is_some_and(|cls| cls.contains(base))
            } else {
                ROLE_SUFFIX_HINTS.iter().any(|h| base.ends_with(h) || base.contains(h))
            };
            if !looks_like_class {
                continue;
            }
        }
        s.add(
            "SV-OOP-011",
            "DowncastWithoutDynamicCast",
            Severity::Error,
            format!(
                "Explicit static cast '{ty}'(...)' used on class handle. \
                 In SystemVerilog (IEEE 1800 \u{a7} 8.16), downcasting class handles requires dynamic casting via \
                 '$cast(target, source)' to verify polymorphic type compatibility at runtime. \
                 Static class casting bypasses runtime type safety and causes simulator aborts. Replace with '$cast()'."
            ),
            loc,
        );
    }
}
