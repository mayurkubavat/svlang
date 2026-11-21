//! Cached semantic & netlist facts for the Formal-Readiness rule pack (`SV-FPV-001..005`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use svlang::netlist::{CheckKind, LowerOutcome, NetOp, Netlist, PropertyOptions, ValueId, ValueKind};
use svlang::syntax::{NodeId, SyntaxKind};
use svlang::{PortDir, PropKind};
use svlang_model::Loc;

use crate::support::class_items::user_code;
use crate::support::slang_ast::subtree;
use crate::syntax::Cst;

#[derive(Clone, Debug)]
pub(crate) struct AssumeOutputOrStateHit {
    pub target_desc: String,
    pub loc: Loc,
    pub fix: Option<crate::fix::Fix>,
}

#[derive(Clone, Debug)]
pub(crate) struct ResetlessControlHit {
    pub state_name: String,
    pub sink_desc: String,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct MissingDisableIffHit {
    pub prop_name: String,
    pub loc: Loc,
    pub fix: Option<crate::fix::Fix>,
}

#[derive(Clone, Debug)]
pub(crate) struct WideComplexityHit {
    pub kind_desc: String,
    pub metric: u32,
    pub is_memory: bool,
    pub loc: Loc,
}

#[derive(Clone, Debug)]
pub(crate) struct BadAssumeHit {
    pub reason: String,
    pub loc: Loc,
}

#[derive(Default)]
pub(crate) struct FormalReadyFacts {
    pub assume_output_or_state: Vec<AssumeOutputOrStateHit>,
    pub resetless_control: Vec<ResetlessControlHit>,
    pub missing_disable_iff: Vec<MissingDisableIffHit>,
    pub wide_complexity: Vec<WideComplexityHit>,
    pub bad_assumes: Vec<BadAssumeHit>,
}

pub(crate) fn facts(c: &Cst) -> Arc<FormalReadyFacts> {
    crate::shared::get("formal_ready_facts", c.compile_key(), || compute(c))
}

fn own_toks<'a>(c: &'a Cst, n: NodeId) -> Vec<&'a str> {
    c.toks(n).map(|ti| c.text_of(ti)).collect()
}

fn is_clock_or_reset_name(name: &str) -> bool {
    let leaf = name.rsplit('.').next().unwrap_or(name).to_ascii_lowercase();
    leaf == "clk"
        || leaf.starts_with("clk_")
        || leaf.ends_with("_clk")
        || leaf.ends_with("_ck")
        || leaf.contains("clock")
        || leaf == "rst"
        || leaf == "reset"
        || leaf.starts_with("rst")
        || leaf.ends_with("_rst")
        || leaf.ends_with("_rstn")
        || leaf.ends_with("_rst_n")
        || (leaf.ends_with("_ni") && leaf.contains("rst"))
        || leaf.contains("reset")
}

fn map_sv_loc(path_to_file: &HashMap<String, u32>, nl: &Netlist, loc: svlang::Loc) -> Option<Loc> {
    if loc.line == 0 {
        return None;
    }
    let path = nl.files.get(loc.file as usize)?;
    let &file_id = path_to_file.get(path)?;
    Some(Loc { file: file_id, line: loc.line, column: loc.column.max(1), expansion: None })
}

fn compute(c: &Cst) -> FormalReadyFacts {
    let mut out = FormalReadyFacts::default();
    if c.nodes(SyntaxKind::ModuleDecl).is_empty() {
        return out;
    }
    let fs = crate::support::defines_packages::Files::new(c);
    let mut path_to_file: HashMap<String, u32> = HashMap::new();
    for (ti, tok) in c.tree.tokens.iter().enumerate() {
        if tok.from_macro || !crate::support::defines_packages::user_tok(c, &fs, ti as u32) {
            continue;
        }
        let p = fs.path(tok.origin.file).to_string();
        path_to_file.entry(p).or_insert_with(|| c.loc(ti as u32).file);
    }
    if path_to_file.is_empty() {
        return out;
    }

    collect_ast_assume_on_outputs(c, &mut out);
    let mut seen_fpv001: HashSet<(u32, u32)> =
        out.assume_output_or_state.iter().map(|h| (h.loc.file, h.loc.line)).collect();
    let mut seen_fpv002: HashSet<(u32, u32, String)> = HashSet::new();
    let mut seen_fpv003: HashSet<(u32, u32)> = HashSet::new();
    let mut seen_fpv004: HashSet<(u32, u32, String)> = HashSet::new();
    let mut seen_fpv005: HashSet<(u32, u32)> = HashSet::new();

    for (_tid, outcome) in crate::shared::netlist_facts(c).iter() {
        let LowerOutcome::Lowered(nl) = outcome else { continue };

        analyze_resetless_control(nl, &path_to_file, &mut seen_fpv002, &mut out);

        let has_formal = !nl.properties.is_empty() || !nl.checks.is_empty();
        if !has_formal {
            continue;
        }

        analyze_missing_disable_iff(c, nl, &path_to_file, &mut seen_fpv003, &mut out);
        analyze_wide_complexity(nl, &path_to_file, &mut seen_fpv004, &mut out);

        let mut compiled = nl.clone();
        if !compiled.properties.is_empty() {
            let _ = compiled.compile_properties(&PropertyOptions::default());
        }
        analyze_assumes_on_netlist(c, &compiled, &path_to_file, &mut seen_fpv001, &mut seen_fpv005, &mut out);
    }

    out
}

fn collect_ast_assume_on_outputs(c: &Cst, out: &mut FormalReadyFacts) {
    for m in c.nodes(SyntaxKind::ModuleDecl) {
        if !user_code(c, m) {
            continue;
        }
        let mut output_ports: HashSet<String> = HashSet::new();
        if let Some(pl) = c.tree.child(m, SyntaxKind::PortList) {
            let mut cur_dir: Option<PortDir> = None;
            for p in c.tree.children(pl) {
                if c.kind(p) != SyntaxKind::Port {
                    continue;
                }
                let toks = own_toks(c, p);
                if toks.contains(&"output") {
                    cur_dir = Some(PortDir::Output);
                } else if toks.contains(&"input") {
                    cur_dir = Some(PortDir::Input);
                } else if toks.contains(&"inout") {
                    cur_dir = Some(PortDir::Inout);
                } else if toks.contains(&"ref") {
                    cur_dir = Some(PortDir::Ref);
                }
                if cur_dir == Some(PortDir::Output) {
                    if let Some(nm) = c.tree.child(p, SyntaxKind::Name) {
                        let txt = c.text(nm).trim().to_string();
                        if !txt.is_empty() {
                            output_ports.insert(txt);
                        }
                    }
                }
            }
        }
        for n in subtree(c, m) {
            if c.kind(n) == SyntaxKind::DataDecl {
                let toks = own_toks(c, n);
                if toks.contains(&"output") {
                    for decl in c.tree.children(n).filter(|&ch| c.kind(ch) == SyntaxKind::Declarator) {
                        if let Some(nm) = c.tree.child(decl, SyntaxKind::Name) {
                            let txt = c.text(nm).trim().to_string();
                            if !txt.is_empty() {
                                output_ports.insert(txt);
                            }
                        }
                    }
                }
            }
        }
        if output_ports.is_empty() {
            continue;
        }
        for n in subtree(c, m) {
            if !matches!(c.kind(n), SyntaxKind::ConcurrentAssert | SyntaxKind::AssertStmt) {
                continue;
            }
            let own = own_toks(c, n);
            if !own.contains(&"assume") {
                continue;
            }
            let Some((t_lo, t_hi)) = c.tree.token_bounds(n) else { continue };
            let loc = c.loc(t_lo);
            let mut hit_port: Option<String> = None;
            for ti in t_lo..=t_hi {
                let txt = c.text_of(ti);
                if output_ports.contains(txt) {
                    if ti == t_lo && (ti + 1) <= t_hi && c.text_of(ti + 1) == ":" {
                        continue;
                    }
                    hit_port = Some(txt.to_string());
                    break;
                }
            }
            if let Some(port_name) = hit_port {
                let fix = build_assume_to_assert_fix(c, loc);
                out.assume_output_or_state.push(AssumeOutputOrStateHit {
                    target_desc: format!("module output port '{port_name}'"),
                    loc,
                    fix,
                });
            }
        }
    }
}

fn state_has_sync_reset(nl: &Netlist, state_next: ValueId, reset_inputs: &HashSet<ValueId>) -> bool {
    let vn = nl.value(state_next);
    let ValueKind::Op { op: NetOp::Mux, args } = &vn.kind else {
        return false;
    };
    if args.len() != 3 {
        return false;
    }
    let cond = args[0];
    let arm_t_const = matches!(nl.value(args[1]).kind, ValueKind::Const(_));
    let arm_f_const = matches!(nl.value(args[2]).kind, ValueKind::Const(_));
    if !arm_t_const && !arm_f_const {
        return false;
    }
    cone_touches_any(nl, cond, reset_inputs)
}

fn cone_touches_any(nl: &Netlist, root: ValueId, targets: &HashSet<ValueId>) -> bool {
    let mut stack = vec![root];
    let mut seen = HashSet::new();
    while let Some(v) = stack.pop() {
        if !seen.insert(v) {
            continue;
        }
        if targets.contains(&v) {
            return true;
        }
        match &nl.value(v).kind {
            ValueKind::Op { args, .. } => stack.extend(args.iter().copied()),
            ValueKind::Sampled(si) => stack.push(nl.sampled[*si].expr),
            _ => {}
        }
    }
    false
}

fn analyze_resetless_control(
    nl: &Netlist,
    path_to_file: &HashMap<String, u32>,
    seen: &mut HashSet<(u32, u32, String)>,
    out: &mut FormalReadyFacts,
) {
    let mut reset_inputs: HashSet<ValueId> = HashSet::new();
    for s in &nl.states {
        if let Some(ar) = s.async_reset {
            reset_inputs.insert(ar.signal);
        }
    }
    for (idx, inp) in nl.inputs.iter().enumerate() {
        if is_clock_or_reset_name(&inp.name) && !inp.name.to_ascii_lowercase().contains("clk") {
            for v in &nl.values {
                if v.kind == ValueKind::Input(idx) {
                    reset_inputs.insert(v.id);
                }
            }
        }
    }
    let has_any_reset = !reset_inputs.is_empty() || nl.states.iter().any(|s| s.init.is_some());
    if !has_any_reset {
        return;
    }

    let mut resetless_states: HashMap<ValueId, (String, svlang::Loc)> = HashMap::new();
    for s in &nl.states {
        if s.name.contains("._sva_") || s.async_reset.is_some() || s.init.is_some() {
            continue;
        }
        if state_has_sync_reset(nl, s.next, &reset_inputs) {
            continue;
        }
        resetless_states.insert(s.out, (s.name.clone(), s.loc));
    }
    if resetless_states.is_empty() {
        return;
    }

    let mut sinks: Vec<(ValueId, String)> = Vec::new();
    for o in &nl.outputs {
        if o.width == 1 {
            sinks.push((o.value, format!("1-bit control output '{}'", o.name)));
        }
    }
    for m in &nl.memories {
        for wp in &m.writes {
            sinks.push((wp.enable, format!("write-enable of memory '{}'", m.name)));
        }
    }
    for v in &nl.values {
        if let ValueKind::Op { op: NetOp::Mux, args } = &v.kind {
            if let Some(&sel) = args.first() {
                sinks.push((sel, "branch/mux control selector".to_string()));
            }
        }
    }

    for (sink_v, sink_desc) in sinks {
        let mut stack = vec![sink_v];
        let mut visited = HashSet::new();
        while let Some(v) = stack.pop() {
            if !visited.insert(v) {
                continue;
            }
            if let Some((sname, sloc)) = resetless_states.get(&v) {
                if let Some(loc) = map_sv_loc(path_to_file, nl, *sloc) {
                    if seen.insert((loc.file, loc.line, sname.clone())) {
                        out.resetless_control.push(ResetlessControlHit {
                            state_name: sname.clone(),
                            sink_desc: sink_desc.clone(),
                            loc,
                        });
                    }
                }
                continue;
            }
            if let ValueKind::Op { args, .. } = &nl.value(v).kind {
                stack.extend(args.iter().copied());
            }
        }
    }
}

fn infer_module_reset_expr(nl: &Netlist) -> Option<String> {
    for s in &nl.states {
        if let Some(ar) = s.async_reset {
            if let Some(raw) = nl.value(ar.signal).name.as_deref() {
                let leaf = raw.rsplit('.').next().unwrap_or(raw);
                if !leaf.is_empty() {
                    return Some(if ar.active_high { leaf.to_string() } else { format!("!{leaf}") });
                }
            }
        }
    }
    for inp in &nl.inputs {
        let low = inp.name.to_ascii_lowercase();
        if is_clock_or_reset_name(&inp.name) && !low.contains("clk") && !low.contains("clock") {
            let leaf = inp.name.rsplit('.').next().unwrap_or(&inp.name);
            let active_low =
                low.ends_with("_n") || low.ends_with("_ni") || low.ends_with("rstn") || low.ends_with("resetn");
            return Some(if active_low { format!("!{leaf}") } else { leaf.to_string() });
        }
    }
    None
}

fn build_assume_to_assert_fix(c: &Cst, loc: Loc) -> Option<crate::fix::Fix> {
    for (ti, tok) in c.tree.tokens.iter().enumerate() {
        if tok.from_macro || &*tok.text != "assume" {
            continue;
        }
        let tl = c.loc(ti as u32);
        if tl.file == loc.file && tl.line == loc.line {
            let raw =
                c.token_fix(ti as u32, ti as u32, "assert", "Replace 'assume' on DUT output/state with 'assert'")?;
            return crate::fix::with_safety(Some(raw), crate::fix::FixSafety::Unsafe).into_iter().next();
        }
    }
    None
}

fn build_disable_iff_fix(c: &Cst, loc: Loc, rst_expr: &str) -> Option<crate::fix::Fix> {
    for ca in c.nodes(SyntaxKind::ConcurrentAssert) {
        let Some((t_lo, t_hi)) = c.tree.token_bounds(ca) else { continue };
        let ca_loc = c.loc(t_lo);
        if ca_loc.file != loc.file || (loc.line < ca_loc.line || loc.line > c.loc(t_hi).line) {
            continue;
        }
        // Locate `property` `(` then optional `@(...)`.
        let mut prop_kw: Option<u32> = None;
        for ti in t_lo..=t_hi {
            if c.text_of(ti) == "property" {
                prop_kw = Some(ti);
                break;
            }
        }
        let p_ti = prop_kw?;
        let mut open_p: Option<u32> = None;
        for ti in (p_ti + 1)..=t_hi {
            if c.text_of(ti) == "(" {
                open_p = Some(ti);
                break;
            }
        }
        let open_ti = open_p?;
        let desc = format!("Insert 'disable iff ({rst_expr})'");
        if open_ti < t_hi && c.text_of(open_ti + 1) == "@" {
            let mut depth = 0i32;
            let mut saw_open = false;
            for ti in (open_ti + 2)..=t_hi {
                let txt = c.text_of(ti);
                if txt == "(" {
                    depth += 1;
                    saw_open = true;
                } else if txt == ")" {
                    depth -= 1;
                    if saw_open && depth == 0 {
                        let raw = c.token_fix(ti, ti, format!(") disable iff ({rst_expr})"), &desc)?;
                        return crate::fix::with_safety(Some(raw), crate::fix::FixSafety::Unsafe).into_iter().next();
                    }
                } else if !saw_open {
                    // `@clk` identifier without parentheses.
                    let raw = c.token_fix(ti, ti, format!("{txt} disable iff ({rst_expr})"), &desc)?;
                    return crate::fix::with_safety(Some(raw), crate::fix::FixSafety::Unsafe).into_iter().next();
                }
            }
        } else {
            let raw = c.token_fix(open_ti, open_ti, format!("(disable iff ({rst_expr}) "), &desc)?;
            return crate::fix::with_safety(Some(raw), crate::fix::FixSafety::Unsafe).into_iter().next();
        }
    }
    None
}

fn analyze_missing_disable_iff(
    c: &Cst,
    nl: &Netlist,
    path_to_file: &HashMap<String, u32>,
    seen: &mut HashSet<(u32, u32)>,
    out: &mut FormalReadyFacts,
) {
    let has_reset = nl.states.iter().any(|s| s.async_reset.is_some())
        || nl.inputs.iter().any(|i| is_clock_or_reset_name(&i.name) && !i.name.to_ascii_lowercase().contains("clk"));
    if !has_reset {
        return;
    }
    let rst_expr = infer_module_reset_expr(nl);
    for (idx, p) in nl.properties.iter().enumerate() {
        if !matches!(p.kind, PropKind::Assert | PropKind::Cover) || p.disable_iff.is_some() {
            continue;
        }
        let Some(loc) = map_sv_loc(path_to_file, nl, p.loc) else { continue };
        if seen.insert((loc.file, loc.line)) {
            let name = nl.property_names.get(idx).cloned().or_else(|| p.label.clone()).unwrap_or_default();
            let fix = rst_expr.as_deref().and_then(|re| build_disable_iff_fix(c, loc, re));
            out.missing_disable_iff.push(MissingDisableIffHit { prop_name: name, loc, fix });
        }
    }
}

fn analyze_wide_complexity(
    nl: &Netlist,
    path_to_file: &HashMap<String, u32>,
    seen: &mut HashSet<(u32, u32, String)>,
    out: &mut FormalReadyFacts,
) {
    for v in &nl.values {
        let ValueKind::Op { op, args } = &v.kind else { continue };
        let op_name = match op {
            NetOp::Mul => "multiplier ('*')",
            NetOp::UDiv | NetOp::SDiv => "divider ('/')",
            NetOp::UMod | NetOp::SMod => "modulo ('%')",
            _ => continue,
        };
        if args.len() == 2
            && !matches!(nl.value(args[0]).kind, ValueKind::Const(_))
            && !matches!(nl.value(args[1]).kind, ValueKind::Const(_))
        {
            if let Some(loc) = map_sv_loc(path_to_file, nl, v.loc) {
                if seen.insert((loc.file, loc.line, op_name.to_string())) {
                    out.wide_complexity.push(WideComplexityHit {
                        kind_desc: format!("{}-bit {op_name}", v.width),
                        metric: v.width,
                        is_memory: false,
                        loc,
                    });
                }
            }
        }
    }
    for m in &nl.memories {
        if let Some(loc) = map_sv_loc(path_to_file, nl, m.loc) {
            if seen.insert((loc.file, loc.line, m.name.clone())) {
                out.wide_complexity.push(WideComplexityHit {
                    kind_desc: format!("memory '{}' ({}x{}-bit)", m.name, m.depth, m.width),
                    metric: m.depth,
                    is_memory: true,
                    loc,
                });
            }
        }
    }
}

fn canonical_atom(nl: &Netlist, mut v: ValueId) -> (String, bool) {
    let mut pol = true;
    loop {
        match &nl.value(v).kind {
            ValueKind::Op { op: NetOp::LogNot | NetOp::Not, args } if args.len() == 1 && nl.value(v).width == 1 => {
                pol = !pol;
                v = args[0];
            }
            ValueKind::Op { op: NetOp::RedOr, args } if args.len() == 1 && nl.value(args[0]).width == 1 => {
                v = args[0];
            }
            ValueKind::Op { op: NetOp::Neq, args } if args.len() == 2 => {
                let (mut a, mut b) = (args[0].0, args[1].0);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                return (format!("eq:{a}:{b}"), !pol);
            }
            ValueKind::Op { op: NetOp::Eq, args } if args.len() == 2 => {
                if let ValueKind::Const(cv) = &nl.value(args[1]).kind {
                    if nl.value(args[0]).width == 1 && cv.known() {
                        if !cv.is_true() {
                            pol = !pol;
                        }
                        v = args[0];
                        continue;
                    }
                }
                let (mut a, mut b) = (args[0].0, args[1].0);
                if a > b {
                    std::mem::swap(&mut a, &mut b);
                }
                return (format!("eq:{a}:{b}"), pol);
            }
            _ => return (format!("v:{}", v.0), pol),
        }
    }
}

fn eval_bool_cone_over_bits(nl: &Netlist, root: ValueId) -> Option<(bool, bool)> {
    let mut leaves: Vec<ValueId> = Vec::new();
    let mut leaf_idx: HashMap<ValueId, usize> = HashMap::new();
    let mut order: Vec<ValueId> = Vec::new();
    let mut visited: HashSet<ValueId> = HashSet::new();

    fn dfs(
        nl: &Netlist,
        v: ValueId,
        leaves: &mut Vec<ValueId>,
        leaf_idx: &mut HashMap<ValueId, usize>,
        order: &mut Vec<ValueId>,
        visited: &mut HashSet<ValueId>,
    ) -> bool {
        if visited.contains(&v) {
            return true;
        }
        let vn = nl.value(v);
        if vn.width != 1 {
            return false;
        }
        match &vn.kind {
            ValueKind::Const(_) => {}
            ValueKind::Input(_) | ValueKind::StateOut(_) | ValueKind::Sampled(_) => {
                if let std::collections::hash_map::Entry::Vacant(e) = leaf_idx.entry(v) {
                    if leaves.len() >= 8 {
                        return false;
                    }
                    e.insert(leaves.len());
                    leaves.push(v);
                }
            }
            ValueKind::Op {
                op:
                    NetOp::LogNot
                    | NetOp::Not
                    | NetOp::RedOr
                    | NetOp::RedAnd
                    | NetOp::LogAnd
                    | NetOp::And
                    | NetOp::LogOr
                    | NetOp::Or
                    | NetOp::Xor
                    | NetOp::Xnor
                    | NetOp::Eq
                    | NetOp::Neq
                    | NetOp::Mux,
                args,
            } => {
                if !args.iter().all(|&a| dfs(nl, a, leaves, leaf_idx, order, visited)) {
                    return false;
                }
            }
            _ => return false,
        }
        visited.insert(v);
        order.push(v);
        true
    }

    if !dfs(nl, root, &mut leaves, &mut leaf_idx, &mut order, &mut visited) {
        return None;
    }
    let mut ever_true = false;
    let mut ever_false = false;
    let mut vals: HashMap<ValueId, bool> = HashMap::with_capacity(order.len());
    for mask in 0..(1usize << leaves.len()) {
        vals.clear();
        for (i, &lv) in leaves.iter().enumerate() {
            vals.insert(lv, ((mask >> i) & 1) != 0);
        }
        for &v in &order {
            if vals.contains_key(&v) {
                continue;
            }
            let b = match &nl.value(v).kind {
                ValueKind::Const(cv) => cv.known() && cv.is_true(),
                ValueKind::Op { op, args } => match op {
                    NetOp::LogNot | NetOp::Not => !vals[&args[0]],
                    NetOp::RedOr | NetOp::RedAnd => vals[&args[0]],
                    NetOp::LogAnd | NetOp::And => vals[&args[0]] && vals[&args[1]],
                    NetOp::LogOr | NetOp::Or => vals[&args[0]] || vals[&args[1]],
                    NetOp::Xor | NetOp::Neq => vals[&args[0]] ^ vals[&args[1]],
                    NetOp::Xnor | NetOp::Eq => vals[&args[0]] == vals[&args[1]],
                    NetOp::Mux => {
                        if vals[&args[0]] {
                            vals[&args[1]]
                        } else {
                            vals[&args[2]]
                        }
                    }
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            };
            vals.insert(v, b);
        }
        if vals[&root] {
            ever_true = true;
        } else {
            ever_false = true;
        }
        if ever_true && ever_false {
            break;
        }
    }
    Some((ever_true, ever_false))
}

fn analyze_assumes_on_netlist(
    c: &Cst,
    nl: &Netlist,
    path_to_file: &HashMap<String, u32>,
    seen_fpv001: &mut HashSet<(u32, u32)>,
    seen_fpv005: &mut HashSet<(u32, u32)>,
    out: &mut FormalReadyFacts,
) {
    let free_vals: HashSet<ValueId> = nl.undriven.iter().map(|u| u.value).collect();
    let mut unit_facts: HashMap<String, bool> = HashMap::new();

    for chk in &nl.checks {
        if chk.kind != CheckKind::Assume {
            continue;
        }
        let Some(loc) = map_sv_loc(path_to_file, nl, chk.loc) else { continue };
        let en_const_zero = matches!(&nl.value(chk.enable).kind, ValueKind::Const(cv) if cv.known() && !cv.is_true());
        if en_const_zero {
            continue;
        }
        let en_uncond = matches!(&nl.value(chk.enable).kind, ValueKind::Const(cv) if cv.known() && cv.is_true());

        let mut bad_reason: Option<String> = None;
        if let ValueKind::Const(cv) = &nl.value(chk.expr).kind {
            if !cv.is_true() {
                bad_reason = Some(
                    "evaluates to constant false ('0'), making the formal environment inconsistent (all assertions pass vacuously)"
                        .to_string(),
                );
            } else if en_uncond {
                bad_reason = Some("evaluates to constant true ('1') and constrains nothing".to_string());
            }
        } else if let Some((ever_t, ever_f)) = eval_bool_cone_over_bits(nl, chk.expr) {
            if !ever_t {
                bad_reason = Some(
                    "is unsatisfiable (contradictory boolean expression), making the formal environment inconsistent"
                        .to_string(),
                );
            } else if !ever_f && en_uncond {
                bad_reason = Some("is a boolean tautology (always true) and constrains nothing".to_string());
            }
        }
        if bad_reason.is_none() && en_uncond {
            let (key, pol) = canonical_atom(nl, chk.expr);
            if let Some(&prev_pol) = unit_facts.get(&key) {
                if prev_pol != pol {
                    bad_reason = Some(
                        "directly contradicts an earlier unconditional assumption on the same signal/expression"
                            .to_string(),
                    );
                }
            } else {
                unit_facts.insert(key, pol);
            }
        }
        if let Some(reason) = bad_reason {
            if seen_fpv005.insert((loc.file, loc.line)) {
                out.bad_assumes.push(BadAssumeHit { reason, loc });
            }
            continue;
        }

        let mut stack = vec![chk.expr];
        let mut visited = HashSet::new();
        let mut has_data_input = false;
        let mut user_state: Option<String> = None;
        while let Some(v) = stack.pop() {
            if !visited.insert(v) {
                continue;
            }
            if free_vals.contains(&v) {
                let is_sva_free = nl.undriven.iter().any(|u| u.value == v && u.signal.contains("._sva_"));
                if !is_sva_free {
                    has_data_input = true;
                }
            }
            match &nl.value(v).kind {
                ValueKind::Input(idx) => {
                    if !is_clock_or_reset_name(&nl.inputs[*idx].name) {
                        has_data_input = true;
                    }
                }
                ValueKind::StateOut(sid) => {
                    let sname = &nl.states[sid.0 as usize].name;
                    if !sname.contains("._sva_") && user_state.is_none() {
                        user_state = Some(sname.clone());
                    }
                }
                ValueKind::Sampled(si) => {
                    stack.push(nl.sampled[*si].expr);
                }
                ValueKind::Op { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                _ => {}
            }
        }
        if !has_data_input {
            if let Some(sname) = user_state {
                if seen_fpv001.insert((loc.file, loc.line)) {
                    let fix = build_assume_to_assert_fix(c, loc);
                    out.assume_output_or_state.push(AssumeOutputOrStateHit {
                        target_desc: format!("internal sequential state '{sname}'"),
                        loc,
                        fix,
                    });
                }
            }
        }
    }
}
