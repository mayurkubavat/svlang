//! RTL, FSM, Clock/Reset, and CDC semantic analyzer (`SV-FSM-002`, `SV-FSM-003`,
//! `SV-CDC-001`, `SV-RTL-001`, `SV-RTL-002`, `SV-RTL-003`, `SV-RTL-004`).

use std::collections::{HashMap, HashSet, VecDeque};

use svlang::syntax::{Child, NodeId, SyntaxKind::*};
use svlang_model::Loc;

use crate::support::class_items::{declarators, kids, op, user_code};
use crate::support::procedural::{is_stmt, items};
use crate::support::sema_facts::ast_ok;
use crate::support::slang_ast::{decl_name, subtree};
use crate::syntax::Cst;

#[derive(Clone, Debug)]
pub(crate) struct FsmStateFinding {
    pub module_name: String,
    pub state_var: String,
    pub state_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct CdcCrossing {
    pub module_name: String,
    pub src_reg: String,
    pub src_clk: String,
    pub dst_clk: String,
    pub width: u32,
    pub combinational: bool,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct ResetPolarityMismatch {
    pub module_name: String,
    pub reset_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct PartialAlwaysFfReset {
    pub module_name: String,
    pub unreset_regs: Vec<String>,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct CombClockGate {
    pub module_name: String,
    pub clk_name: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct PortWidthMismatch {
    pub inst_name: String,
    pub def_name: String,
    pub port_name: String,
    pub formal_width: u32,
    pub actual_width: u32,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct InferredLatch {
    pub message: String,
    pub loc: Loc,
}

#[derive(Default)]
pub(crate) struct RtlFsmCdcFacts {
    pub fsm_unreachable: Vec<FsmStateFinding>,
    pub fsm_deadlock: Vec<FsmStateFinding>,
    pub cdc_crossings: Vec<CdcCrossing>,
    pub mixed_reset_polarity: Vec<ResetPolarityMismatch>,
    pub partial_ff_reset: Vec<PartialAlwaysFfReset>,
    pub comb_clock_gate: Vec<CombClockGate>,
    pub port_width_mismatches: Vec<PortWidthMismatch>,
    pub inferred_latches: Vec<InferredLatch>,
}

pub(crate) fn facts(c: &Cst) -> std::sync::Arc<RtlFsmCdcFacts> {
    crate::shared::get("rtl_fsm_cdc_facts", c.tree_key(), || analyze_cst(c))
}

fn analyze_cst(c: &Cst) -> RtlFsmCdcFacts {
    let mut facts = RtlFsmCdcFacts::default();
    let oracle = svlang::semantic::oracle::TypeOracle::new(c.all);
    for m in c.nodes(ModuleDecl) {
        if !user_code(c, m) || !ast_ok(c, m) {
            continue;
        }
        let mname = decl_name(c, m);
        if mname.is_empty() {
            continue;
        }
        analyze_module_clock_reset_cdc(c, &oracle, m, &mname, &mut facts);
        analyze_module_fsms(c, &oracle, m, &mname, &mut facts);
    }
    analyze_port_widths(c, &mut facts.port_width_mismatches);
    analyze_inferred_latches(c, &mut facts.inferred_latches);
    facts
}

// ---- Clock, Reset & CDC Extraction ----

#[derive(Clone, Debug)]
struct SensEdge {
    posedge: bool,
    signal: String,
    tok: u32,
}

fn proc_sens_edges(c: &Cst, pb: NodeId) -> Option<(NodeId, Vec<SensEdge>, NodeId)> {
    let kw = c.first_tok(pb).map(|t| c.text_of(t)).unwrap_or("");
    if !matches!(kw, "always_ff" | "always") {
        return None;
    }
    let timed = c.tree.children(pb).find(|&k| c.kind(k) == TimedStmt)?;
    let ec = c.tree.child(timed, TimingControl)?;
    let body = c.tree.children(timed).find(|&k| is_stmt(c.kind(k)))?;
    let toks: Vec<u32> = c.tree.token_range(ec);
    let mut edges = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let txt = c.text_of(toks[i]);
        if matches!(txt, "posedge" | "negedge") {
            let pos = txt == "posedge";
            // Find the signal identifier right after posedge/negedge (skipping `(` if any).
            let mut j = i + 1;
            while j < toks.len() && c.text_of(toks[j]) == "(" {
                j += 1;
            }
            if j < toks.len() {
                let sig = c.text_of(toks[j]);
                if sig.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic() || ch == '_') {
                    edges.push(SensEdge { posedge: pos, signal: sig.to_string(), tok: toks[j] });
                }
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    if edges.is_empty() {
        return None;
    }
    Some((ec, edges, body))
}

fn nba_targets_in(c: &Cst, root: NodeId) -> Vec<String> {
    let mut out = Vec::new();
    for n in subtree(c, root) {
        if c.kind(n) == BinaryExpr
            && c.keyword(n).map(|k| c.text_of(k)) == Some("<=")
            && c.parent(n).is_some_and(|p| c.kind(p) == ExprStmt)
        {
            if let Some(&lhs) = kids(c, n).first() {
                if let Some(id) = base_ident(c, lhs) {
                    if !out.contains(&id) {
                        out.push(id);
                    }
                }
            }
        }
    }
    out
}

fn base_ident(c: &Cst, mut e: NodeId) -> Option<String> {
    loop {
        match c.kind(e) {
            ParenExpr | SelectExpr | MemberExpr => e = *kids(c, e).first()?,
            NameExpr => return c.first_tok(e).map(|t| c.text_of(t).to_string()),
            _ => return None,
        }
    }
}

fn split_async_reset_body(c: &Cst, body: NodeId, rst_sig: &str) -> Option<(NodeId, NodeId)> {
    let top = match c.kind(body) {
        IfStmt => body,
        SeqBlock => {
            let stmts = items(c, body);
            if stmts.len() == 1 && c.kind(stmts[0]) == IfStmt {
                stmts[0]
            } else {
                return None;
            }
        }
        _ => return None,
    };
    let ks = kids(c, top);
    let cond = *ks.first()?;
    if !c.tree.bare_text(cond).contains(rst_sig) {
        return None;
    }
    let then_s = *ks.iter().find(|&&k| is_stmt(c.kind(k)))?;
    let else_cl = c.tree.child(top, ElseClause)?;
    let else_s = c.tree.children(else_cl).find(|&k| is_stmt(c.kind(k)))?;
    Some((then_s, else_s))
}

fn analyze_module_clock_reset_cdc(
    c: &Cst,
    oracle: &svlang::semantic::oracle::TypeOracle,
    m: NodeId,
    mname: &str,
    facts: &mut RtlFsmCdcFacts,
) {
    // Map signal -> packed width in module `m`.
    let mut widths: HashMap<String, u32> = HashMap::new();
    for dd in c.tree.children(m).filter(|&k| c.kind(k) == DataDecl) {
        let dt = c.tree.child(dd, DataType);
        for (dec, name, _) in declarators(c, dd) {
            let ty = oracle.decl_type(c.index(), dt, Some(dec));
            if let Some((w, _, _)) = ty.integral {
                widths.insert(name, w);
            }
        }
    }
    if let Some(h) = c.tree.child(m, Header) {
        if let Some(pl) = c.tree.child(h, PortList) {
            let mut prev_dt = None;
            for p in c.tree.children(pl).filter(|&k| c.kind(k) == Port) {
                if let Some(dt) = c.tree.child(p, DataType) {
                    prev_dt = Some(dt);
                } else if c.toks(p).next().is_some() {
                    prev_dt = None;
                }
                for (dec, name, _) in declarators(c, p) {
                    let ty = oracle.decl_type(c.index(), prev_dt, Some(dec));
                    if let Some((w, _, _)) = ty.integral {
                        widths.insert(name, w);
                    }
                }
            }
        }
    }

    // Collect combinationally gated signals (`assign gclk = clk & en;` or in `always_comb`).
    let mut comb_gated: HashSet<String> = HashSet::new();
    for ca in c.tree.children(m).filter(|&k| c.kind(k) == ContinuousAssign) {
        for b in kids(c, ca) {
            if c.kind(b) == BinaryExpr && op(c, b) == "=" {
                let ks = kids(c, b);
                if let (Some(&lhs), Some(&rhs)) = (ks.first(), ks.get(1)) {
                    if is_nontrivial_comb_expr(c, rhs) {
                        if let Some(id) = base_ident(c, lhs) {
                            comb_gated.insert(id);
                        }
                    }
                }
            }
        }
    }
    for pb in c.tree.children(m).filter(|&k| c.kind(k) == ProceduralBlock) {
        if c.first_tok(pb).map(|t| c.text_of(t)) == Some("always_comb") {
            for n in subtree(c, pb) {
                if c.kind(n) == BinaryExpr && op(c, n) == "=" {
                    let ks = kids(c, n);
                    if let (Some(&lhs), Some(&rhs)) = (ks.first(), ks.get(1)) {
                        if is_nontrivial_comb_expr(c, rhs) {
                            if let Some(id) = base_ident(c, lhs) {
                                comb_gated.insert(id);
                            }
                        }
                    }
                }
            }
        }
    }

    // Inspect clocked blocks:
    let mut rst_polarities: HashMap<String, (bool, Loc)> = HashMap::new();
    let mut reported_rst: HashSet<String> = HashSet::new();
    // reg_name -> clock_name (None if driven in >1 clock domain)
    let mut reg_clock: HashMap<String, Option<String>> = HashMap::new();
    let mut clocked_blocks: Vec<(String, Option<String>, NodeId)> = Vec::new();

    for pb in c.tree.children(m).filter(|&k| c.kind(k) == ProceduralBlock) {
        let is_always_ff = c.first_tok(pb).map(|t| c.text_of(t)) == Some("always_ff");
        let Some((_ec, edges, body)) = proc_sens_edges(c, pb) else { continue };
        let clk_edge = &edges[0];
        if is_always_ff && comb_gated.contains(&clk_edge.signal) {
            facts.comb_clock_gate.push(CombClockGate {
                module_name: mname.to_string(),
                clk_name: clk_edge.signal.clone(),
                loc: c.loc(clk_edge.tok),
            });
        }
        let rst_opt = edges.get(1).map(|e| e.signal.clone());
        if let Some(rst_edge) = edges.get(1) {
            match rst_polarities.get(&rst_edge.signal) {
                None => {
                    rst_polarities.insert(rst_edge.signal.clone(), (rst_edge.posedge, c.loc(rst_edge.tok)));
                }
                Some(&(prev_pos, _))
                    if prev_pos != rst_edge.posedge && reported_rst.insert(rst_edge.signal.clone()) =>
                {
                    facts.mixed_reset_polarity.push(ResetPolarityMismatch {
                        module_name: mname.to_string(),
                        reset_name: rst_edge.signal.clone(),
                        loc: c.loc(rst_edge.tok),
                    });
                }
                _ => {}
            }
            if is_always_ff {
                if let Some((rst_branch, clk_branch)) = split_async_reset_body(c, body, &rst_edge.signal) {
                    let rst_set: HashSet<String> = nba_targets_in(c, rst_branch).into_iter().collect();
                    let clk_targets = nba_targets_in(c, clk_branch);
                    if !rst_set.is_empty() {
                        let missing: Vec<String> = clk_targets.into_iter().filter(|r| !rst_set.contains(r)).collect();
                        if !missing.is_empty() {
                            if let Some(pt) = c.first_tok(pb) {
                                facts.partial_ff_reset.push(PartialAlwaysFfReset {
                                    module_name: mname.to_string(),
                                    unreset_regs: missing,
                                    loc: c.loc(pt),
                                });
                            }
                        }
                    }
                }
            }
        }
        let driven = nba_targets_in(c, body);
        for r in driven {
            match reg_clock.get(&r) {
                None => {
                    reg_clock.insert(r, Some(clk_edge.signal.clone()));
                }
                Some(Some(prev)) if prev != &clk_edge.signal => {
                    reg_clock.insert(r, None);
                }
                _ => {}
            }
        }
        clocked_blocks.push((clk_edge.signal.clone(), rst_opt, body));
    }

    // Check CDC crossings across distinct clocks in `m`:
    // First identify valid 1-bit 2-FF synchronizer first-stage flops in each destination clock domain!
    // A flop `s1` in `dst_clk` is a valid sync stage-1 for 1-bit `src` in `src_clk` if:
    //   1. `s1 <= src;` is a direct 1-bit NBA with no combinational logic, AND
    //   2. Every read of `s1` in `dst_clk` is a direct 1-bit NBA `s2 <= s1;` into a second flop `s2`!
    let distinct_clocks: HashSet<&str> = clocked_blocks.iter().map(|(clk, _, _)| clk.as_str()).collect();
    if distinct_clocks.len() < 2 {
        return;
    }

    // Also trace simple combinational wires (`assign w = expr;`) back to source registers!
    let mut comb_sources: HashMap<String, (Vec<String>, bool)> = HashMap::new();
    for ca in c.tree.children(m).filter(|&k| c.kind(k) == ContinuousAssign) {
        for b in kids(c, ca) {
            if c.kind(b) == BinaryExpr && op(c, b) == "=" {
                let ks = kids(c, b);
                if let (Some(&lhs), Some(&rhs)) = (ks.first(), ks.get(1)) {
                    if let Some(id) = base_ident(c, lhs) {
                        let comb = is_nontrivial_comb_expr(c, rhs);
                        let mut srcs = Vec::new();
                        for sub in subtree(c, rhs) {
                            if c.kind(sub) == NameExpr {
                                if let Some(t) = c.first_tok(sub) {
                                    srcs.push(c.text_of(t).to_string());
                                }
                            }
                        }
                        comb_sources.insert(id, (srcs, comb));
                    }
                }
            }
        }
    }

    // Pre-collect how every signal is read in each `dst_clk`: whether `s1` is ONLY read as plain RHS of `s2 <= s1`.
    let mut non_sync_stage2_reads: HashSet<(String, String)> = HashSet::new();
    let mut sync_stage2_reads: HashSet<(String, String)> = HashSet::new();
    for (dst_clk, rst_opt, body) in &clocked_blocks {
        let active_body =
            rst_opt.as_deref().and_then(|r| split_async_reset_body(c, *body, r)).map_or(*body, |(_, clk_b)| clk_b);
        for n in subtree(c, active_body) {
            if c.kind(n) != NameExpr {
                continue;
            }
            let Some(t) = c.first_tok(n) else { continue };
            let sig = c.text_of(t);
            // Is `n` the LHS of an NBA?
            if c.parent(n).is_some_and(|p| c.kind(p) == BinaryExpr && kids(c, p).first() == Some(&n)) {
                continue;
            }
            let is_direct_nba_rhs = c.parent(n).is_some_and(|p| {
                c.kind(p) == BinaryExpr
                    && c.keyword(p).map(|k| c.text_of(k)) == Some("<=")
                    && kids(c, p).get(1) == Some(&n)
            });
            if is_direct_nba_rhs {
                sync_stage2_reads.insert((dst_clk.clone(), sig.to_string()));
            } else {
                non_sync_stage2_reads.insert((dst_clk.clone(), sig.to_string()));
            }
        }
    }

    let mut reported_cdc: HashSet<(String, String)> = HashSet::new();
    for (dst_clk, rst_opt, body) in &clocked_blocks {
        let active_body =
            rst_opt.as_deref().and_then(|r| split_async_reset_body(c, *body, r)).map_or(*body, |(_, clk_b)| clk_b);
        for n in subtree(c, active_body) {
            if c.kind(n) != NameExpr {
                continue;
            }
            // Skip LHS of assignment!
            let mut cur = n;
            let mut is_lhs = false;
            while let Some(p) = c.parent(cur) {
                if c.kind(p) == BinaryExpr && matches!(op(c, p), "=" | "<=") && kids(c, p).first() == Some(&cur) {
                    is_lhs = true;
                    break;
                }
                if matches!(c.kind(p), SelectExpr | MemberExpr) && kids(c, p).first() == Some(&cur) {
                    cur = p;
                    continue;
                }
                break;
            }
            if is_lhs {
                continue;
            }
            let Some(tok) = c.first_tok(n) else { continue };
            let read_name = c.text_of(tok);
            // Resolve direct register or combinational net fed by cross-clock register:
            let mut candidates: Vec<(String, bool)> = vec![(read_name.to_string(), false)];
            if let Some((srcs, comb)) = comb_sources.get(read_name) {
                for s in srcs {
                    candidates.push((s.clone(), *comb));
                }
            }
            let direct_nba_lhs = c.parent(n).and_then(|p| {
                (c.kind(p) == BinaryExpr
                    && c.keyword(p).map(|k| c.text_of(k)) == Some("<=")
                    && kids(c, p).get(1) == Some(&n))
                .then(|| kids(c, p).first().and_then(|&l| base_ident(c, l)))
                .flatten()
            });

            for (src_reg, via_comb) in candidates {
                let Some(Some(src_clk)) = reg_clock.get(&src_reg) else { continue };
                if src_clk == dst_clk {
                    continue;
                }
                let w = widths.get(&src_reg).copied().unwrap_or(1);
                let in_comb_expr = via_comb || direct_nba_lhs.is_none();
                // Allow a clean 1-bit 2-FF synchronizer (`s1 <= src_reg; s2 <= s1;`) or gray-coded pointer (`*gray*`).
                if w == 1 && !in_comb_expr {
                    if let Some(ref s1) = direct_nba_lhs {
                        let has_s2 = sync_stage2_reads.contains(&(dst_clk.clone(), s1.clone()));
                        let bad_use = non_sync_stage2_reads.contains(&(dst_clk.clone(), s1.clone()));
                        if has_s2 && !bad_use {
                            continue;
                        }
                    }
                }
                if w > 1 && !in_comb_expr && src_reg.to_ascii_lowercase().contains("gray") {
                    continue;
                }
                if reported_cdc.insert((src_reg.clone(), dst_clk.clone())) {
                    facts.cdc_crossings.push(CdcCrossing {
                        module_name: mname.to_string(),
                        src_reg,
                        src_clk: src_clk.clone(),
                        dst_clk: dst_clk.clone(),
                        width: w,
                        combinational: in_comb_expr,
                        loc: c.loc(tok),
                    });
                }
            }
        }
    }
}

fn is_nontrivial_comb_expr(c: &Cst, e: NodeId) -> bool {
    let e = crate::support::data_items::unparen(c, e);
    matches!(c.kind(e), BinaryExpr | CondExpr | ConcatExpr)
        || (c.kind(e) == PrefixExpr
            && c.keyword(e).is_some_and(|k| matches!(c.text_of(k), "&" | "|" | "^" | "~&" | "~|" | "~^")))
}

// ---- FSM Extraction & Reachability / Deadlock ----

fn collect_enum_defs(all: &[svlang::syntax::Tree]) -> HashMap<String, Vec<String>> {
    let mut defs: HashMap<String, Vec<String>> = HashMap::new();
    for (ti, t) in all.iter().enumerate() {
        let tc = Cst::new(t, ti, svlang::semantic::Locator::default());
        for td in tc.nodes(TypedefDecl) {
            let Some(dt) = tc.tree.child(td, DataType) else { continue };
            if let Some(mems) = enum_declarator_names(&tc, dt) {
                let tname = decl_name(&tc, td);
                if !tname.is_empty() && mems.len() >= 2 {
                    defs.entry(tname).or_insert(mems);
                }
            }
        }
    }
    defs
}

fn enum_declarator_names(c: &Cst, dt: NodeId) -> Option<Vec<String>> {
    if c.first_tok(dt).map(|t| c.text_of(t)) != Some("enum") {
        return None;
    }
    let mut out = Vec::new();
    for dec in c.tree.children(dt).filter(|&k| c.kind(k) == Declarator) {
        if let Some(nm) = c.tree.child(dec, Name) {
            let s = c.tree.bare_text(nm);
            if !s.is_empty() {
                out.push(s);
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

fn analyze_module_fsms(
    c: &Cst,
    _oracle: &svlang::semantic::oracle::TypeOracle,
    m: NodeId,
    mname: &str,
    facts: &mut RtlFsmCdcFacts,
) {
    let enum_defs = collect_enum_defs(c.all);
    // Find variables in `m` whose declared type is an `enum` (typedef or inline) with >= 2 states.
    let mut enum_vars: HashMap<String, (Vec<String>, Loc, Option<String>)> = HashMap::new();
    for dd in c.tree.children(m).filter(|&k| c.kind(k) == DataDecl) {
        let Some(dt) = c.tree.child(dd, DataType) else { continue };
        let members_opt = enum_declarator_names(c, dt).or_else(|| {
            let raw = strip_pkg_prefix(&c.tree.bare_text(dt));
            enum_defs.get(&raw).cloned()
        });
        let Some(members) = members_opt else { continue };
        if members.len() < 2 {
            continue;
        }
        for (dec, name, tok) in declarators(c, dd) {
            if c.tree.child(dec, Dim).is_some() {
                continue;
            }
            let init_state = kids(c, dec)
                .into_iter()
                .find(|&k| !matches!(c.kind(k), Name | Dim))
                .map(|e| strip_pkg_prefix(&c.tree.bare_text(e)))
                .filter(|s| members.contains(s));
            enum_vars.insert(name, (members.clone(), c.loc(tok), init_state));
        }
    }
    if enum_vars.is_empty() {
        return;
    }

    // Find `case (state_var)` blocks in `m`:
    for cs in subtree(c, m) {
        if c.kind(cs) != CaseStmt {
            continue;
        }
        let Some(sel) = case_selector(c, cs) else { continue };
        let sel_name = c.tree.bare_text(sel);
        let Some((members, decl_loc, init_state)) = enum_vars.get(&sel_name) else { continue };
        let member_set: HashSet<&str> = members.iter().map(String::as_str).collect();

        // Determine the companion `next_state` variable (if two-process FSM: `state_q <= state_d;`)
        // by finding any other variable of the same enum assigned inside `cs`, plus `sel_name` itself!
        let mut target_vars: HashSet<String> = HashSet::from([sel_name.clone()]);
        for (other_var, (other_mems, _, _)) in &enum_vars {
            if other_mems == members && assigns_var_in(c, cs, other_var) {
                target_vars.insert(other_var.clone());
            }
        }

        // Collect reset states assigned to `sel_name` anywhere in clocked reset branches of `m`:
        let mut reset_states: HashSet<String> = HashSet::new();
        if let Some(init_s) = init_state {
            reset_states.insert(init_s.clone());
        }
        for pb in c.tree.children(m).filter(|&k| c.kind(k) == ProceduralBlock) {
            for ifs in subtree(c, pb) {
                if c.kind(ifs) != IfStmt {
                    continue;
                }
                let ks = kids(c, ifs);
                let Some(&cond) = ks.first() else { continue };
                let cond_l = c.tree.bare_text(cond).to_ascii_lowercase();
                if cond_l.contains("rst") || cond_l.contains("reset") {
                    if let Some(&then_s) = ks.iter().find(|&&k| is_stmt(c.kind(k))) {
                        for st in assigned_enum_values(c, then_s, &sel_name, &member_set).unwrap_or_default() {
                            reset_states.insert(st);
                        }
                    }
                }
            }
        }

        // Parse arms of `case (sel_name)`:
        let Some(arms) = parse_case_arms(c, cs) else { continue };
        let mut adj: HashMap<String, HashSet<String>> = HashMap::new();
        let mut all_constant = true;
        for (labels, is_default, arm_stmt) in &arms {
            let mut dsts: HashSet<String> = HashSet::new();
            for tv in &target_vars {
                match assigned_enum_values(c, *arm_stmt, tv, &member_set) {
                    Some(vs) => dsts.extend(vs),
                    None => {
                        all_constant = false;
                        break;
                    }
                }
            }
            if !all_constant {
                break;
            }
            if *is_default {
                for mem in members {
                    if !arms.iter().any(|(ls, _, _)| ls.contains(mem)) {
                        adj.entry(mem.clone()).or_default().extend(dsts.iter().cloned());
                    }
                }
            } else {
                for lbl in labels {
                    if member_set.contains(lbl.as_str()) {
                        adj.entry(lbl.clone()).or_default().extend(dsts.iter().cloned());
                    }
                }
            }
        }
        if !all_constant {
            continue;
        }
        // Also collect direct synchronous transitions outside `cs` (excluding reset branches and default self-hold `state_d = state_q`).
        let mut extra_targets: HashSet<String> = HashSet::new();
        for pb in c.tree.children(m).filter(|&k| c.kind(k) == ProceduralBlock) {
            for n in subtree(c, pb) {
                if c.kind(n) == BinaryExpr && matches!(op(c, n), "=" | "<=") {
                    if crate::support::slang_ast::ancestors(c, n).any(|a| {
                        a == cs
                            || (c.kind(a) == IfStmt
                                && kids(c, a).first().is_some_and(|&cond| {
                                    let cl = c.tree.bare_text(cond).to_ascii_lowercase();
                                    cl.contains("rst") || cl.contains("reset")
                                }))
                    }) {
                        continue;
                    }
                    let ks = kids(c, n);
                    if let (Some(&lhs), Some(&rhs)) = (ks.first(), ks.get(1)) {
                        let lhs_txt = c.tree.bare_text(lhs);
                        if target_vars.contains(&lhs_txt) {
                            let rtxt = strip_pkg_prefix(&c.tree.bare_text(rhs));
                            if member_set.contains(rtxt.as_str()) {
                                extra_targets.insert(rtxt);
                            }
                        }
                    }
                }
            }
        }

        // 1. Reachability from `reset_states` (only when at least 1 reset/init state is known):
        let mut reachable: HashSet<String> = reset_states.clone();
        reachable.extend(extra_targets.iter().cloned());
        if !reset_states.is_empty() {
            let mut q: VecDeque<String> = reachable.iter().cloned().collect();
            while let Some(u) = q.pop_front() {
                if let Some(nexts) = adj.get(&u) {
                    for v in nexts {
                        if reachable.insert(v.clone()) {
                            q.push_back(v.clone());
                        }
                    }
                }
            }
            for mem in members {
                let up = mem.to_ascii_uppercase();
                if !reachable.contains(mem) && !["UNUSED", "RSVD", "RESERVED", "XXX"].iter().any(|k| up.contains(k)) {
                    facts.fsm_unreachable.push(FsmStateFinding {
                        module_name: mname.to_string(),
                        state_var: sel_name.clone(),
                        state_name: mem.clone(),
                        loc: *decl_loc,
                    });
                }
            }
        }

        // 2. Deadlock states: non-reset reachable state whose explicit case arm exists and only transitions to itself
        // (or assigns nothing at all when two-process default holds `next_state = state;`!).
        let case_tok = c.first_tok(cs).map(|t| c.loc(t)).unwrap_or(*decl_loc);
        for (labels, is_default, _) in &arms {
            if *is_default {
                continue;
            }
            for lbl in labels {
                if !member_set.contains(lbl.as_str()) || reset_states.contains(lbl) {
                    continue;
                }
                if !reset_states.is_empty() && !reachable.contains(lbl) {
                    continue;
                }
                let up = lbl.to_ascii_uppercase();
                if ["DONE", "FIN", "END", "ERR", "FAULT", "HALT", "DEAD", "TRAP", "STOP", "LOCK", "ABORT", "FAIL"]
                    .iter()
                    .any(|k| up.contains(k))
                {
                    continue;
                }
                let nexts = adj.get(lbl);
                let trapped = match nexts {
                    None => true,
                    Some(set) => set.iter().all(|d| d == lbl),
                };
                if trapped && extra_targets.is_empty() {
                    facts.fsm_deadlock.push(FsmStateFinding {
                        module_name: mname.to_string(),
                        state_var: sel_name.clone(),
                        state_name: lbl.clone(),
                        loc: case_tok,
                    });
                }
            }
        }
    }
}

fn strip_pkg_prefix(s: &str) -> String {
    s.rsplit("::").next().unwrap_or(s).trim().to_string()
}

fn case_selector(c: &Cst, cs: NodeId) -> Option<NodeId> {
    let mut opened = false;
    for ch in &c.tree.node(cs).children {
        match *ch {
            Child::Token(t) if c.text_of(t) == "(" => opened = true,
            Child::Node(n) if opened => return Some(n),
            _ => {}
        }
    }
    None
}

fn parse_case_arms(c: &Cst, cs: NodeId) -> Option<Vec<(Vec<String>, bool, NodeId)>> {
    let mut opened = false;
    let mut sel_seen = false;
    let mut labels = Vec::new();
    let mut is_default = false;
    let mut arms = Vec::new();
    for ch in &c.tree.node(cs).children {
        match *ch {
            Child::Token(t) => match c.text_of(t) {
                "(" => opened = true,
                "default" if sel_seen => is_default = true,
                _ => {}
            },
            Child::Node(_n) if opened && !sel_seen => {
                sel_seen = true;
            }
            Child::Node(n) if sel_seen => {
                if is_stmt(c.kind(n)) {
                    arms.push((std::mem::take(&mut labels), std::mem::take(&mut is_default), n));
                } else {
                    labels.push(strip_pkg_prefix(&c.tree.bare_text(n)));
                }
            }
            _ => {}
        }
    }
    Some(arms)
}

fn assigns_var_in(c: &Cst, root: NodeId, var: &str) -> bool {
    subtree(c, root).into_iter().any(|n| {
        c.kind(n) == BinaryExpr
            && matches!(op(c, n), "=" | "<=")
            && kids(c, n).first().is_some_and(|&l| c.tree.bare_text(l) == var)
    })
}

/// Returns `Some(states)` assigned to `target_var` in `stmt`, or `None` if a dynamic/computed
/// non-enum expression is assigned to `target_var`.
fn assigned_enum_values(c: &Cst, stmt: NodeId, target_var: &str, members: &HashSet<&str>) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for n in subtree(c, stmt) {
        if c.kind(n) == BinaryExpr && matches!(op(c, n), "=" | "<=") {
            let ks = kids(c, n);
            let [lhs, rhs] = ks[..] else { continue };
            if c.tree.bare_text(lhs) != target_var {
                continue;
            }
            if !extract_rhs_states(c, rhs, target_var, members, &mut out) {
                return None;
            }
        }
    }
    Some(out)
}

fn extract_rhs_states(c: &Cst, rhs: NodeId, target_var: &str, members: &HashSet<&str>, out: &mut Vec<String>) -> bool {
    let rhs = crate::support::data_items::unparen(c, rhs);
    if c.kind(rhs) == CondExpr {
        let ks = kids(c, rhs);
        if ks.len() == 3 {
            return extract_rhs_states(c, ks[1], target_var, members, out)
                && extract_rhs_states(c, ks[2], target_var, members, out);
        }
        return false;
    }
    let txt = strip_pkg_prefix(&c.tree.bare_text(rhs));
    if members.contains(txt.as_str()) {
        if !out.contains(&txt) {
            out.push(txt);
        }
        return true;
    }
    txt == target_var
}

// ---- SV-RTL-004: Elaborated Port Connection Width Mismatch ----

fn analyze_port_widths(c: &Cst, out: &mut Vec<PortWidthMismatch>) {
    let elab = crate::shared::elaboration(c);
    let design = &elab.design;
    if design.instances.is_empty() {
        return;
    }
    let mut seen_sites: HashSet<(u32, u32, u32, String)> = HashSet::new();
    for inst in &design.instances {
        let Some(parent_id) = inst.parent else { continue };
        if inst.is_checker || inst.port_conns.is_empty() {
            continue;
        }
        for conn in &inst.port_conns {
            let Some((expr_tree, expr_node)) = conn.expr else { continue };
            if expr_tree != c.index() || !user_code(c, expr_node) {
                continue;
            }
            let Some(tok) = c.first_tok(expr_node) else { continue };
            let loc = c.loc(tok);
            if loc.expansion.is_some() {
                continue;
            }
            let raw_txt = c.tree.bare_text(expr_node);
            if svlang::semantic::expr_types::is_unbased_unsized_tok(raw_txt.trim()) {
                continue;
            }
            let port_sid = match &conn.port_name {
                Some(pname) => inst.ports.iter().copied().find(|&sid| design.symbols[sid.0 as usize].name == *pname),
                None => inst.ports.get(conn.arg_index).copied(),
            };
            let Some(port_sid) = port_sid else { continue };
            let formal_sym = &design.symbols[port_sid.0 as usize];
            if !formal_sym.ty.is_packed() {
                continue;
            }
            let Some(formal_w) = formal_sym.ty.bit_width() else { continue };
            let mut arena = svlang::ExprArena::new();
            let mut binder =
                svlang::ExprBinder::new(c.all, design, parent_id, &conn.scope_prefix, &conn.gen_consts, &mut arena);
            let Some(bound_id) = binder.bind_self(expr_tree, expr_node) else { continue };
            let actual_ty = &arena.get(bound_id).ty;
            if !actual_ty.is_packed() {
                continue;
            }
            let Some(actual_w) = actual_ty.bit_width() else { continue };
            if formal_w == actual_w {
                continue;
            }
            let pname = formal_sym.name.clone();
            if seen_sites.insert((loc.file, loc.line, loc.column, pname.clone())) {
                out.push(PortWidthMismatch {
                    inst_name: inst.name.clone(),
                    def_name: inst.definition.clone(),
                    port_name: pname,
                    formal_width: formal_w,
                    actual_width: actual_w,
                    loc,
                });
            }
        }
    }
}

fn analyze_inferred_latches(c: &Cst, out: &mut Vec<InferredLatch>) {
    // Only run if this CST actually declares at least one `always_comb` in user code!
    let has_user_comb = c
        .nodes(ProceduralBlock)
        .into_iter()
        .any(|pb| user_code(c, pb) && c.first_tok(pb).map(|t| c.text_of(t)) == Some("always_comb"));
    if !has_user_comb {
        return;
    }
    let fs = crate::support::defines_packages::Files::new(c);
    // Map file path -> Cst file index (from the first non-macro token in that file).
    let mut path_to_file: HashMap<String, u32> = HashMap::new();
    for (ti, tok) in c.tree.tokens.iter().enumerate() {
        if tok.from_macro || !crate::support::defines_packages::user_tok(c, &fs, ti as u32) {
            continue;
        }
        let p = fs.path(tok.origin.file).to_string();
        path_to_file.entry(p).or_insert_with(|| c.loc(ti as u32).file);
    }
    if path_to_file.is_empty() {
        return;
    }
    let mut seen = HashSet::new();
    for (_tid, outcome) in crate::shared::netlist_facts(c).iter() {
        if let svlang::netlist::LowerOutcome::Error(diags) = outcome {
            for d in diags {
                if !d.message.contains("inferred latch") {
                    continue;
                }
                let Some(&file_id) = path_to_file.get(&d.file) else { continue };
                if seen.insert((file_id, d.loc.line, d.message.clone())) {
                    out.push(InferredLatch {
                        message: d.message.clone(),
                        loc: Loc { file: file_id, line: d.loc.line, column: d.loc.column.max(1), expansion: None },
                    });
                }
            }
        }
    }
}
