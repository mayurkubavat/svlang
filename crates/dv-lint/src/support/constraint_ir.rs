//! Semantic constraint-block analyzer across class hierarchies (`SV-RND-041`,
//! `SV-RND-042`, `SV-RND-043`, `SV-RND-044`).

use std::collections::{HashMap, HashSet};

use dv_model::Loc;
use svlang::syntax::{NodeId, SyntaxKind::*};

use crate::support::class_items::{declarators, kids, op, qualifiers};
use crate::support::constraints::{constraint_blocks, user_model_classes};
use crate::support::data_items::unparen;
use crate::support::slang_ast::{class_chain, last_name, subtree};
use crate::syntax::Cst;

#[derive(Clone, Debug)]
pub(crate) struct RelationalCycle {
    pub class_name: String,
    pub constraint_name: String,
    pub lhs: String,
    pub rhs: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct UnconstrainedForeach {
    pub class_name: String,
    pub array_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct DanglingDisableSoft {
    pub class_name: String,
    pub var_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct PigeonholeUnique {
    pub class_name: String,
    pub array_name: String,
    pub elem_count: u128,
    pub domain_size: u128,
    pub loc: Loc,
}

#[derive(Default)]
pub(crate) struct ConstraintFacts {
    pub relational_cycles: Vec<RelationalCycle>,
    pub unconstrained_foreach: Vec<UnconstrainedForeach>,
    pub dangling_disable_soft: Vec<DanglingDisableSoft>,
    pub pigeonhole_unique: Vec<PigeonholeUnique>,
}

pub(crate) fn facts(c: &Cst) -> std::sync::Arc<ConstraintFacts> {
    crate::shared::get("constraint_ir_facts", c.tree_key(), || analyze_cst(c))
}

fn analyze_cst(c: &Cst) -> ConstraintFacts {
    let mut facts = ConstraintFacts::default();
    let oracle = svlang::semantic::oracle::TypeOracle::new(c.all);
    for (cls, cls_name) in user_model_classes(c) {
        check_relational_cycles(c, cls, &cls_name, &mut facts.relational_cycles);
        check_unconstrained_foreach(c, &oracle, cls, &cls_name, &mut facts.unconstrained_foreach);
        check_dangling_disable_soft(c, cls, &cls_name, &mut facts.dangling_disable_soft);
        check_pigeonhole_unique(c, &oracle, cls, &cls_name, &mut facts.pigeonhole_unique);
    }
    facts
}

// ---- SV-RND-041: Multi-variable strict relational contradiction ----

fn check_relational_cycles(c: &Cst, cls: NodeId, cls_name: &str, out: &mut Vec<RelationalCycle>) {
    for cd in c.tree.children(cls).filter(|&d| c.kind(d) == ConstraintDecl) {
        let Some(cb) = c.tree.child(cd, ConstraintBlock) else { continue };
        let cname = last_name(c, cd);
        // Edge `(u, v, strict_weight, loc)` meaning `u + strict_weight <= v`.
        let mut edges: Vec<(String, String, u32, Loc)> = Vec::new();
        for item in c.tree.children(cb) {
            if c.kind(item) != ConstraintItem {
                continue;
            }
            // Skip `soft` constraints and conditional/implication/foreach/solve/unique/disable items.
            let item_own: Vec<&str> = c.toks(item).map(|t| c.text_of(t)).collect();
            if item_own.iter().any(|t| matches!(*t, "soft" | "if" | "->" | "foreach" | "solve" | "unique" | "disable"))
            {
                continue;
            }
            for e in kids(c, item) {
                collect_conjunct_edges(c, e, &mut edges);
            }
        }
        if edges.len() < 2 {
            continue;
        }
        let mut nodes: Vec<String> = Vec::new();
        let mut idx_of: HashMap<String, usize> = HashMap::new();
        for (u, v, _, _) in &edges {
            for s in [u, v] {
                if !idx_of.contains_key(s) {
                    let k = nodes.len();
                    idx_of.insert(s.clone(), k);
                    nodes.push(s.clone());
                }
            }
        }
        let n = nodes.len();
        if n == 0 || n > 64 {
            continue;
        }
        // Max strict weight on a directed path `i ->* j` (`None` = unreachable).
        let mut dist: Vec<Vec<Option<u32>>> = vec![vec![None; n]; n];
        for (u, v, w, loc) in &edges {
            let ui = idx_of[u];
            let vi = idx_of[v];
            // Check if adding `ui -> vi (w)` closes a strict cycle with already-known paths `vi ->* ui`!
            if ui == vi && *w >= 1 {
                out.push(RelationalCycle {
                    class_name: cls_name.to_string(),
                    constraint_name: cname.clone(),
                    lhs: u.clone(),
                    rhs: v.clone(),
                    loc: *loc,
                });
                break;
            }
            if let Some(back) = dist[vi][ui] {
                if back + *w >= 1 {
                    out.push(RelationalCycle {
                        class_name: cls_name.to_string(),
                        constraint_name: cname.clone(),
                        lhs: u.clone(),
                        rhs: v.clone(),
                        loc: *loc,
                    });
                    break;
                }
            }
            // Update transitive closure incrementally so the exact offending constraint edge is reported once!
            let prev = dist[ui][vi].unwrap_or(0);
            if dist[ui][vi].is_none() || *w > prev {
                dist[ui][vi] = Some(*w);
            }
            #[allow(clippy::needless_range_loop)]
            for src in 0..n {
                let d_su = if src == ui { Some(0) } else { dist[src][ui] };
                let Some(d1) = d_su else { continue };
                for dst in 0..n {
                    let d_vd = if vi == dst { Some(0) } else { dist[vi][dst] };
                    let Some(d2) = d_vd else { continue };
                    let cand = (d1 + *w + d2).min(2);
                    if dist[src][dst].is_none_or(|old| cand > old) {
                        dist[src][dst] = Some(cand);
                    }
                }
            }
        }
    }
}

fn simple_rel_term(c: &Cst, e: NodeId) -> Option<String> {
    let e = unparen(c, e);
    match c.kind(e) {
        NameExpr | MemberExpr => {
            let txt = c.tree.bare_text(e);
            (!txt.is_empty() && !txt.contains('(') && !txt.contains('[')).then_some(txt)
        }
        _ => None,
    }
}

fn collect_conjunct_edges(c: &Cst, e: NodeId, edges: &mut Vec<(String, String, u32, Loc)>) {
    let e = unparen(c, e);
    if c.kind(e) != BinaryExpr {
        return;
    }
    let ks = kids(c, e);
    let [l, r] = ks[..] else { return };
    let o = op(c, e);
    if o == "&&" {
        collect_conjunct_edges(c, l, edges);
        collect_conjunct_edges(c, r, edges);
        return;
    }
    if !matches!(o, "<" | "<=" | ">" | ">=" | "==") {
        return;
    }
    let (Some(lt), Some(rt)) = (simple_rel_term(c, l), simple_rel_term(c, r)) else {
        return;
    };
    let Some(tok) = c.first_tok(e) else { return };
    let loc = c.loc(tok);
    match o {
        "<" => edges.push((lt, rt, 1, loc)),
        "<=" => edges.push((lt, rt, 0, loc)),
        ">" => edges.push((rt, lt, 1, loc)),
        ">=" => edges.push((rt, lt, 0, loc)),
        "==" => {
            edges.push((lt.clone(), rt.clone(), 0, loc));
            edges.push((rt, lt, 0, loc));
        }
        _ => {}
    }
}

// ---- SV-RND-042: foreach over rand dynamic array/queue with unconstrained .size() ----

fn check_unconstrained_foreach(
    c: &Cst,
    oracle: &svlang::semantic::oracle::TypeOracle,
    cls: NodeId,
    cls_name: &str,
    out: &mut Vec<UnconstrainedForeach>,
) {
    let chain = class_chain(c, cls);
    // Collect rand dynamic-array / queue properties declared in `cls` or its bases.
    let mut dyn_arrays: HashSet<String> = HashSet::new();
    for &anc in &chain {
        for dd in c.tree.children(anc).filter(|&d| c.kind(d) == DataDecl) {
            let q = qualifiers(c, dd);
            if !q.contains(&"rand") {
                continue;
            }
            let dt = c.tree.child(dd, DataType);
            for (dec, name, _) in declarators(c, dd) {
                let ty = oracle.decl_type(c.index(), dt, Some(dec));
                if ty.text.ends_with("$[]") || ty.text.ends_with("$[$]") {
                    dyn_arrays.insert(name);
                }
            }
        }
    }
    if dyn_arrays.is_empty() {
        return;
    }
    // Collect arrays whose size is constrained in any constraint block in the chain,
    // OR allocated/populated in `new` / `pre_randomize`.
    let mut sized: HashSet<String> = HashSet::new();
    for &anc in &chain {
        for cb in constraint_blocks(c, anc) {
            for n in subtree(c, cb) {
                if c.kind(n) == MemberExpr && c.last_text(n) == "size" {
                    if let Some(&base) = kids(c, n).first() {
                        sized.insert(c.tree.bare_text(base));
                    }
                }
                if c.kind(n) == BinaryExpr && op(c, n) == "==" {
                    // `arr == '{...}` fixes array size too.
                    for k in kids(c, n) {
                        let txt = c.tree.bare_text(k);
                        if dyn_arrays.contains(&txt) {
                            sized.insert(txt);
                        }
                    }
                }
            }
        }
        for m in c.tree.children(anc).filter(|&d| c.kind(d) == FunctionDecl) {
            let mname = last_name(c, m);
            if !matches!(mname.as_str(), "new" | "pre_randomize") {
                continue;
            }
            for n in subtree(c, m) {
                if c.kind(n) == BinaryExpr && op(c, n) == "=" {
                    if let Some(&lhs) = kids(c, n).first() {
                        let txt = c.tree.bare_text(lhs);
                        if dyn_arrays.contains(&txt) {
                            sized.insert(txt);
                        }
                    }
                }
                if c.kind(n) == MemberExpr && matches!(c.last_text(n), "push_back" | "push_front" | "insert") {
                    if let Some(&base) = kids(c, n).first() {
                        sized.insert(c.tree.bare_text(base));
                    }
                }
            }
        }
    }
    let mut reported: HashSet<String> = HashSet::new();
    for cb in constraint_blocks(c, cls) {
        for item in subtree(c, cb) {
            if c.kind(item) != ConstraintItem {
                continue;
            }
            if c.first_tok(item).map(|t| c.text_of(t)) != Some("foreach") {
                continue;
            }
            // In sv-syntax, `foreach (arr[i]) { ... }` bumps `foreach`, `(`, `arr`, `[`, `i`, `]`, `)` as own tokens on `ConstraintItem`!
            let own: Vec<&str> = c.toks(item).map(|t| c.text_of(t)).collect();
            let base_txt = own
                .iter()
                .skip_while(|&&t| t != "(")
                .skip(1)
                .take_while(|&&t| !matches!(t, "[" | ")"))
                .copied()
                .collect::<String>();
            if dyn_arrays.contains(&base_txt) && !sized.contains(&base_txt) && reported.insert(base_txt.clone()) {
                if let Some(t) = c.first_tok(item) {
                    out.push(UnconstrainedForeach {
                        class_name: cls_name.to_string(),
                        array_name: base_txt,
                        loc: c.loc(t),
                    });
                }
            }
        }
    }
}

// ---- SV-RND-043: `disable soft x;` when no `soft` constraint mentions `x` ----

fn check_dangling_disable_soft(c: &Cst, cls: NodeId, cls_name: &str, out: &mut Vec<DanglingDisableSoft>) {
    let chain = class_chain(c, cls);
    // If `cls` or an ancestor has an `Extends` clause that `class_chain` couldn't resolve in this compile, bail out conservatively.
    for &anc in &chain {
        if let Some(h) = c.tree.child(anc, Header) {
            let has_ext = c
                .tree
                .children(h)
                .any(|e| c.kind(e) == Extends && c.first_tok(e).is_some_and(|t| c.text_of(t) == "extends"));
            if has_ext && anc == *chain.last().unwrap() {
                // Check if the root base is a UVM base (`uvm_*`), which declares zero `soft` constraints on user fields.
                let ext_txt = c
                    .tree
                    .children(h)
                    .find(|&e| c.kind(e) == Extends && c.first_tok(e).is_some_and(|t| c.text_of(t) == "extends"))
                    .map(|e| c.tree.bare_text(e))
                    .unwrap_or_default();
                if !ext_txt.contains("uvm_") {
                    return;
                }
            }
        }
    }
    let mut soft_idents: HashSet<String> = HashSet::new();
    for &anc in &chain {
        for cb in constraint_blocks(c, anc) {
            for item in subtree(c, cb) {
                if c.kind(item) != ConstraintItem {
                    continue;
                }
                let own_toks: Vec<&str> = c.toks(item).map(|t| c.text_of(t)).collect();
                if own_toks.contains(&"soft") && !own_toks.contains(&"disable") {
                    for sub in subtree(c, item) {
                        if c.kind(sub) == NameExpr {
                            if let Some(t) = c.first_tok(sub) {
                                soft_idents.insert(c.text_of(t).to_string());
                            }
                        } else if c.kind(sub) == MemberExpr {
                            soft_idents.insert(c.last_text(sub).to_string());
                            soft_idents.insert(c.tree.bare_text(sub));
                        }
                    }
                }
            }
        }
    }
    for cb in constraint_blocks(c, cls) {
        for item in subtree(c, cb) {
            if c.kind(item) != ConstraintItem {
                continue;
            }
            // In sv-syntax, `disable soft x;` bumps flat tokens `["disable", "soft", "x", ";"]` on `ConstraintItem`.
            let own_toks: Vec<u32> = c.toks(item).collect();
            let texts: Vec<&str> = own_toks.iter().map(|&t| c.text_of(t)).collect();
            if texts.first() == Some(&"disable") && texts.get(1) == Some(&"soft") {
                let vname: String = texts.iter().skip(2).take_while(|&&t| t != ";").copied().collect();
                let leaf = vname.rsplit('.').next().unwrap_or(&vname);
                if !vname.is_empty() && !soft_idents.contains(&vname) && !soft_idents.contains(leaf) {
                    if let Some(&t) = own_toks.first() {
                        out.push(DanglingDisableSoft {
                            class_name: cls_name.to_string(),
                            var_name: vname,
                            loc: c.loc(t),
                        });
                    }
                }
            }
        }
    }
}

// ---- SV-RND-044: `unique { arr }` where fixed array length > 2^W ----

fn check_pigeonhole_unique(
    c: &Cst,
    oracle: &svlang::semantic::oracle::TypeOracle,
    cls: NodeId,
    cls_name: &str,
    out: &mut Vec<PigeonholeUnique>,
) {
    // Map property name -> `(elem_count, domain_cardinality)` for fixed-size unpacked arrays of narrow integral types.
    let mut fixed_caps: HashMap<String, (u128, u128)> = HashMap::new();
    for &anc in &class_chain(c, cls) {
        for dd in c.tree.children(anc).filter(|&d| c.kind(d) == DataDecl) {
            let Some(dt) = c.tree.child(dd, DataType) else { continue };
            let elem_ty = oracle.data_type(c.index(), dt);
            let Some((w, _, _)) = elem_ty.integral else { continue };
            if w == 0 || w > 16 {
                continue;
            }
            let domain = 1u128 << w;
            for (dec, name, _) in declarators(c, dd) {
                let full_ty = oracle.decl_type(c.index(), Some(dt), Some(dec));
                if let Some(cnt) = parse_fixed_unpacked_count(&full_ty.text) {
                    if cnt >= 2 {
                        fixed_caps.entry(name).or_insert((cnt, domain));
                    }
                }
            }
        }
    }
    if fixed_caps.is_empty() {
        return;
    }
    for cb in constraint_blocks(c, cls) {
        for item in subtree(c, cb) {
            if c.kind(item) != ConstraintItem {
                continue;
            }
            if c.first_tok(item).map(|t| c.text_of(t)) != Some("unique") {
                continue;
            }
            // Extract identifiers inside `unique { ... }` (either inside a `ValueRangeList` child or flat tokens).
            let mut seen_arr = HashSet::new();
            for t in c.tree.token_range(item) {
                let txt = c.text_of(t);
                if matches!(txt, "unique" | "{" | "}" | "," | ";") {
                    continue;
                }
                if let Some(&(cnt, dom)) = fixed_caps.get(txt) {
                    if cnt > dom && seen_arr.insert(txt.to_string()) {
                        if let Some(tok) = c.first_tok(item) {
                            out.push(PigeonholeUnique {
                                class_name: cls_name.to_string(),
                                array_name: txt.to_string(),
                                elem_count: cnt,
                                domain_size: dom,
                                loc: c.loc(tok),
                            });
                        }
                    }
                }
            }
        }
    }
}

fn parse_fixed_unpacked_count(ty_text: &str) -> Option<u128> {
    let (_, dims) = ty_text.split_once("$[")?;
    let mut total = 1u128;
    for part in dims.split('[') {
        let range = part.strip_suffix(']')?;
        if range.is_empty() || range == "$" || range == "*" {
            return None;
        }
        let (a_s, b_s) = range.split_once(':')?;
        let a: i128 = a_s.parse().ok()?;
        let b: i128 = b_s.parse().ok()?;
        let len = a.abs_diff(b).checked_add(1)?;
        total = total.checked_mul(len)?;
    }
    Some(total)
}
