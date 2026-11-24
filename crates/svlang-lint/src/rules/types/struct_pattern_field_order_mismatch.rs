//! SVL-TYP-007 (StructPatternFieldOrderMismatch): Flags named `struct`
//! assignment patterns (`'{b: 1, a: 2}`) whose explicit field keys appear out
//! of the target `typedef struct` declaration order, or omit declared fields
//! without a `default:` fallback, catching copy-paste field transpositions and
//! partial-initialization bugs.

use std::collections::{HashMap, HashSet};

use crate::support::trivia::is_library_tok;
use crate::syntax::Cst;
use crate::{Severity, Sink};
use svlang::syntax::Child;
use svlang::syntax::SyntaxKind::*;

const TYPE_KEYWORDS: &[&str] = &[
    "bit",
    "logic",
    "reg",
    "byte",
    "shortint",
    "int",
    "longint",
    "integer",
    "time",
    "real",
    "shortreal",
    "realtime",
    "string",
    "chandle",
    "event",
];

fn clean_type_name(raw: &str) -> &str {
    let base = raw.split('[').next().unwrap_or(raw).trim();
    let base = base.split('#').next().unwrap_or(base).trim();
    base.rsplit("::").next().unwrap_or(base).trim()
}

pub(crate) fn struct_pattern_field_order_mismatch(c: &Cst, out: &mut Sink) {
    // 1. Index `typedef struct [packed] { ... } name_t;` declarations across all trees in the compile.
    let mut struct_defs: HashMap<String, Vec<String>> = HashMap::new();
    let trees: Vec<&svlang::syntax::Tree> = if c.all.is_empty() { vec![c.tree] } else { c.all.iter().collect() };
    for tr in &trees {
        for id in tr.preorder() {
            if tr.kind(id) != TypedefDecl {
                continue;
            }
            let Some(nm) = tr.child(id, Name) else { continue };
            let Some(dt) = tr.child(id, DataType) else { continue };
            let first_kw = tr.token_bounds(dt).map(|(a, _)| &*tr.tokens[a as usize].text).unwrap_or("");
            if first_kw != "struct" {
                continue;
            }
            let mut fields = Vec::new();
            for member in tr.children(dt).filter(|&k| tr.kind(k) == DataDecl) {
                for decl in tr.children(member).filter(|&d| tr.kind(d) == Declarator) {
                    if let Some(fnm) = tr.child(decl, Name) {
                        fields.push(tr.bare_text(fnm));
                    }
                }
            }
            if fields.len() >= 2 {
                struct_defs.entry(tr.bare_text(nm)).or_insert(fields);
            }
        }
    }
    if struct_defs.is_empty() {
        return;
    }

    let types = crate::expr_types::expr_types(c);

    for pat in c.nodes(AssignPattern) {
        let Some(first_t) = c.first_tok(pat) else { continue };
        if is_library_tok(c, first_t) {
            continue;
        }
        // Resolve target struct typedef name from context:
        // - `s_t x = '{...};` (`Declarator` -> parent `DataDecl` -> `DataType`, when `Declarator` has no unpacked `Dim`)
        // - `x = '{...};` (`BinaryExpr` `=` LHS in `expr_types`)
        // - `s_t'('{...})` or `s_t'{...}` (`CastExpr`)
        let target_type: Option<String> = match c.parent(pat) {
            Some(d) if c.kind(d) == Declarator && c.tree.child(d, Dim).is_none() => c
                .parent(d)
                .and_then(|dd| c.tree.child(dd, DataType))
                .map(|dt| clean_type_name(&c.trimmed(dt)).to_string()),
            Some(be) if c.kind(be) == BinaryExpr && c.tok(be, "=").is_some() => c
                .tree
                .children(be)
                .next()
                .filter(|&lhs| lhs != pat)
                .and_then(|lhs| types.get(&(c.index(), lhs)))
                .filter(|ty| !ty.text.contains('['))
                .map(|ty| clean_type_name(&ty.text).to_string()),
            Some(ce) if c.kind(ce) == CastExpr => {
                c.tree.children(ce).next().filter(|&k| k != pat).map(|k| clean_type_name(&c.trimmed(k)).to_string())
            }
            _ => None,
        };
        let Some(tname) = target_type else { continue };
        let Some(decl_fields) = struct_defs.get(&tname) else { continue };

        // Parse top-level comma-separated entries inside `AssignPattern`.
        let mut explicit_keys: Vec<(String, u32)> = Vec::new();
        let mut has_default_or_type_key = false;
        let mut any_colon = false;

        let kids = c.kids(pat);
        let mut seg_start = 0usize;
        for idx in 0..=kids.len() {
            let is_boundary =
                idx == kids.len() || matches!(kids[idx], Child::Token(t) if matches!(c.text_of(t), "," | "}"));
            if !is_boundary {
                continue;
            }
            let seg = &kids[seg_start..idx];
            seg_start = idx + 1;
            let colon_pos = seg.iter().position(|ch| matches!(*ch, Child::Token(t) if c.text_of(t) == ":"));
            let Some(cp) = colon_pos else { continue };
            any_colon = true;
            let before = &seg[..cp];
            if before.iter().any(|ch| matches!(*ch, Child::Token(t) if c.text_of(t) == "default")) {
                has_default_or_type_key = true;
                continue;
            }
            let key_node = before.iter().find_map(|ch| match *ch {
                Child::Node(n) => Some(n),
                Child::Token(_) => None,
            });
            let Some(kn) = key_node else { continue };
            let Some((ka, kb)) = c.tree.token_bounds(kn) else { continue };
            if ka != kb {
                has_default_or_type_key = true;
                continue;
            }
            let ktxt = c.text_of(ka);
            if TYPE_KEYWORDS.contains(&ktxt) || !ktxt.starts_with(|ch: char| ch.is_ascii_alphabetic() || ch == '_') {
                has_default_or_type_key = true;
                continue;
            }
            explicit_keys.push((ktxt.to_string(), ka));
        }

        if !any_colon || explicit_keys.is_empty() {
            continue;
        }

        let pos_map: HashMap<&str, usize> = decl_fields.iter().enumerate().map(|(i, f)| (f.as_str(), i)).collect();
        if explicit_keys.iter().any(|(k, _)| !pos_map.contains_key(k.as_str())) {
            // Unknown member is already a compiler error; avoid duplicate lint noise.
            continue;
        }

        // Check monotonic declaration order of explicit field keys.
        let mut prev_pos: Option<(usize, &str)> = None;
        let mut order_flagged = false;
        for (k, ktok) in &explicit_keys {
            let cur_pos = pos_map[k.as_str()];
            if let Some((pp, pk)) = prev_pos {
                if cur_pos < pp {
                    out.add(
                        "SVL-TYP-007",
                        "StructPatternFieldOrderMismatch",
                        Severity::Warning,
                        format!(
                            "Field '{k}' in struct '{tname}' assignment pattern appears after '{pk}', out of declared struct member order ({}). Keep pattern keys in declaration order to prevent field transposition bugs.",
                            decl_fields.join(", ")
                        ),
                        c.loc(*ktok),
                    );
                    order_flagged = true;
                    break;
                }
            }
            prev_pos = Some((cur_pos, k.as_str()));
        }

        if !order_flagged && !has_default_or_type_key {
            let provided: HashSet<&str> = explicit_keys.iter().map(|(k, _)| k.as_str()).collect();
            let missing: Vec<&str> =
                decl_fields.iter().map(String::as_str).filter(|f| !provided.contains(*f)).collect();
            if !missing.is_empty() {
                out.add(
                    "SVL-TYP-007",
                    "StructPatternFieldOrderMismatch",
                    Severity::Warning,
                    format!(
                        "Struct '{tname}' assignment pattern omits member(s) [{}] without a 'default:' key.",
                        missing.join(", ")
                    ),
                    c.loc(first_t),
                );
            }
        }
    }
}
