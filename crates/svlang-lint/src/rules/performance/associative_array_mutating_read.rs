//! SV-ASC-001 (AssociativeArrayMutatingRead): Flags unguarded associative-array
//! read lookups in relational comparisons without an `.exists(...)` check
//! (IEEE 1800-2023 §7.8.6).

use crate::support::procedural::start;
use crate::support::slang_ast::{ancestors, decl_name, kids};
use crate::syntax::Cst;
use crate::{Severity, Sink};
use std::collections::HashSet;
use svlang::syntax::NodeId;
use svlang::syntax::SyntaxKind::*;

fn is_assoc_index_type(idx: &str, user_types: &HashSet<String>) -> bool {
    if idx == "*" {
        return true;
    }
    let head = idx.split([' ', '\t', '[']).next().unwrap_or("");
    if matches!(
        head,
        "string" | "int" | "integer" | "byte" | "shortint" | "longint" | "bit" | "logic" | "reg" | "time" | "chandle"
    ) {
        return true;
    }
    let clean = idx.rsplit("::").next().unwrap_or(idx).trim();
    user_types.contains(clean)
}

/// Names declared with a true associative unpacked dimension (`[*]`, `[string]`,
/// `[int]`, `[my_key_t]`) — excluding fixed-size unpacked arrays `[4]` or `[DEPTH]`.
fn assoc_names(c: &Cst) -> std::rc::Rc<HashSet<String>> {
    thread_local! {
        static NAMES: crate::shared::CompileCache<HashSet<String>> = const { std::cell::RefCell::new(None) };
    }
    let key = c.compile_key();
    if let Some(v) = NAMES.with(|k| k.borrow().as_ref().filter(|e| e.0 == key).map(|e| e.1.clone())) {
        return v;
    }
    let mut user_types = HashSet::new();
    for (ti, t) in c.all.iter().enumerate() {
        let tc = Cst::new(t, ti, svlang::semantic::Locator::default());
        for k in [TypedefDecl, ClassDecl] {
            for n in tc.nodes(k) {
                let nm = decl_name(&tc, n);
                if !nm.is_empty() {
                    user_types.insert(nm);
                }
            }
        }
    }
    let mut out = HashSet::new();
    for t in c.all {
        for i in 0..t.nodes.len() {
            let d = NodeId(i as u32);
            if t.kind(d) != Declarator {
                continue;
            }
            let assoc = t.children(d).filter(|&k| t.kind(k) == Dim).any(|k| {
                let text = t.bare_text(k);
                let text = text.trim();
                if !(text.len() >= 3
                    && text.starts_with('[')
                    && text.ends_with(']')
                    && !text.contains(':')
                    && !text.contains('$'))
                {
                    return false;
                }
                let inner = text[1..text.len() - 1].trim();
                !inner.is_empty() && is_assoc_index_type(inner, &user_types)
            });
            if assoc {
                if let Some(nm) = t.child(d, Name) {
                    let name = t.bare_text(nm);
                    if !name.is_empty() {
                        out.insert(name);
                    }
                }
            }
        }
    }
    let v = std::rc::Rc::new(out);
    NAMES.with(|k| *k.borrow_mut() = Some((key, v.clone())));
    v
}

fn guarded_by_exists(c: &Cst, node: NodeId, name: &str) -> bool {
    let needle = format!("{name}.exists");
    for a in ancestors(c, node) {
        if matches!(c.kind(a), BinaryExpr | CondExpr | IfStmt) && c.tree.bare_text(a).contains(&needle) {
            return true;
        }
        if matches!(c.kind(a), FunctionDecl | TaskDecl | ProceduralBlock) {
            break;
        }
    }
    false
}

pub(crate) fn assoc_mutating_read(c: &Cst, out: &mut Sink) {
    let names = assoc_names(c);
    if names.is_empty() {
        return;
    }
    for e in c.nodes(BinaryExpr) {
        let op = c.keyword(e).map_or("", |k| c.text_of(k));
        if !matches!(op, "==" | "!=" | "===" | "!==" | "<" | "<=" | ">" | ">=") {
            continue;
        }
        if op == "<=" && c.parent(e).is_some_and(|p| c.kind(p) == ExprStmt) {
            continue;
        }
        for side in kids(c, e).into_iter().take(2) {
            if c.kind(side) != SelectExpr {
                continue;
            }
            let mut base = side;
            while c.kind(base) == SelectExpr {
                match kids(c, base).first() {
                    Some(&b) => base = b,
                    None => break,
                }
            }
            if c.kind(base) != NameExpr || c.toks(base).count() != 1 || !kids(c, base).is_empty() {
                continue;
            }
            let name = c.tree.bare_text(base);
            if !names.contains(&name) || guarded_by_exists(c, e, &name) {
                continue;
            }
            let Some(loc) = start(c, side) else { continue };
            out.add(
                "SV-ASC-001",
                "AssociativeArrayMutatingRead",
                Severity::Info,
                format!(
                    "Unguarded read lookup on associative array '{name}' in relational comparison. \
                     Per IEEE 1800 § 7.8.6, reading a nonexistent index returns the element type's default value and may issue a runtime warning, \
                     conflating missing keys with entries explicitly storing the default value. Guard with '{name}.exists(...)' when key presence matters."
                ),
                loc,
            );
        }
    }
}
