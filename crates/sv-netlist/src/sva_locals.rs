//! Lowering of IEEE 1800-2023 §16.10 SVA sequence and property local variables.
//!
//! Two exact strategies work together:
//! 1. **Bounded `$past` path substitution (`try_desugar_bounded_locals`)**: enumerates
//!    the bounded cycle paths (`<= 32` threads, `<= 64` cycles) of sequences carrying
//!    [`SeqExpr::MatchItems`], propagating an `env: FxMap<LocalId, ExprId>` of `$past`-shifted
//!    expressions. Handles `assert`, `assume`, `cover`, and `restrict`, declaration
//!    initializers, multiple reassignments (`v = a`, `v += b`, `v++`), and overlapping
//!    in-flight transactions with zero extra state registers beyond `$past`.
//! 2. **Symbolic prophecy constants (`try_desugar_prophecy_locals`)**: for unbounded
//!    temporal operators (`##[1:$]`, `[*0:$]`, `[->1]`, `until`, `s_eventually`, `always`),
//!    verifies single-assignment existential/universal polarity (`Assert` antecedent or
//!    positive `Cover` sequence) and rewrites `(seq, v = rhs)` into `seq ##0 (v_sym == rhs)`
//!    backed by a free constant register (`init: None, next: v_sym`), proving arbitrary-depth
//!    and out-of-order pipelined scoreboards soundly and completely.

use std::collections::{HashMap, HashSet};

use sv_semantic::elab_ir::RtlType;
use sv_semantic::expr::{BinaryOp, ExprArena, ExprId, ExprKind, InsideItem, LocalId, SampledKind, UnaryOp};
use sv_semantic::model::Loc;
use sv_semantic::sva_ir::{BoundProperty, CycleBound, CycleRange, PropExpr, PropKind, RepeatKind, SeqExpr};
use sv_semantic::CVal;

use crate::builder::{BuildKind, NetBuilder};
use crate::expr_lower::ObjVal;
use crate::ir::{State, StateId};

const MAX_LOCAL_THREADS: usize = 32;
const MAX_LOCAL_DELAY: u32 = 64;

#[derive(Clone)]
struct SeqThread {
    delay: u32,
    guard: ExprId,
    env: HashMap<LocalId, ExprId>,
}

/// Allocates a free symbolic constant register (`next(s) = s`, uninitialized `init`) in `builder`
/// for each `(LocalId, RtlType)` returned by [`try_desugar_prophecy_locals`].
pub(crate) fn alloc_prophecy_states(
    builder: &mut NetBuilder,
    vars: &[(LocalId, RtlType)],
    loc: Loc,
    out_map: &mut sv_semantic::fx::FxMap<LocalId, ObjVal>,
) -> Vec<StateId> {
    let mut created = Vec::with_capacity(vars.len());
    for (lid, ty) in vars {
        let Some((w, signed, four)) = ty.integral_shape() else { continue };
        let sid = StateId(builder.states.len() as u32);
        let name = format!("{}._sva_loc_{}", builder.top, lid.0);
        let out_val = builder.alloc_raw(w, signed, four, BuildKind::StateOut(sid), Some(name.clone()), loc);
        builder.states.push(State {
            id: sid,
            name,
            width: w,
            signed,
            four_state: four,
            clock: None,
            async_reset: None,
            init: None,
            next: out_val,
            latch_enable: None,
            out: out_val,
            loc,
        });
        out_map.insert(*lid, ObjVal::Packed(out_val));
        created.push(sid);
    }
    created
}

/// Desugars SVA local variables on `prop` if present, returning any symbolic constant
/// `(LocalId, RtlType)` registers that must be allocated when Pass B (prophecy) was used.
/// If neither Pass A nor Pass B applies, leaves `SeqExpr::MatchItems` intact so property
/// compilation reports a clean per-property `Unsupported` reason.
pub(crate) fn desugar_property_locals(arena: &mut ExprArena, prop: &mut BoundProperty) -> Vec<(LocalId, RtlType)> {
    if prop.sva_locals.is_empty() {
        return Vec::new();
    }
    let sva_ids: HashSet<LocalId> = prop.sva_locals.iter().map(|(id, _, _)| *id).collect();
    if try_desugar_bounded_locals(arena, prop, &sva_ids) {
        prop.sva_locals.clear();
        return Vec::new();
    }
    if let Some(vars) = try_desugar_prophecy_locals(arena, prop) {
        prop.sva_locals.clear();
        return vars;
    }
    Vec::new()
}

fn try_desugar_bounded_locals(arena: &mut ExprArena, prop: &mut BoundProperty, sva_ids: &HashSet<LocalId>) -> bool {
    let mut env0 = HashMap::new();
    for (lid, _, init_opt) in &prop.sva_locals {
        if let Some(ie) = init_opt {
            env0.insert(*lid, *ie);
        }
    }
    let mut candidate = prop.expr.clone();
    if !rewrite_bounded_prop(arena, &mut candidate, &env0, sva_ids, prop.loc) {
        return false;
    }
    prop.expr = candidate;
    true
}

fn rewrite_bounded_prop(
    arena: &mut ExprArena,
    prop: &mut PropExpr,
    env0: &HashMap<LocalId, ExprId>,
    sva_ids: &HashSet<LocalId>,
    loc: Loc,
) -> bool {
    match prop {
        PropExpr::Clocked { prop: inner, .. } | PropExpr::AcceptOn { prop: inner, .. } => {
            rewrite_bounded_prop(arena, inner, env0, sva_ids, loc)
        }
        PropExpr::Always { range, prop: inner, .. } => {
            // Even under unbounded `always (ant |-> cons)`, a bounded inner implication
            // desugars locally per attempt!
            let _ = range;
            rewrite_bounded_prop(arena, inner, env0, sva_ids, loc)
        }
        PropExpr::OverlappedImplication { lhs, rhs } => {
            let (l, r) = (lhs.clone(), (**rhs).clone());
            desugar_implication(arena, prop, l, r, 0, env0, sva_ids, loc)
        }
        PropExpr::NonOverlappedImplication { lhs, rhs } => {
            let (l, r) = (lhs.clone(), (**rhs).clone());
            desugar_implication(arena, prop, l, r, 1, env0, sva_ids, loc)
        }
        PropExpr::Seq(s) | PropExpr::Strong(s) | PropExpr::Weak(s) => {
            let s_cloned = s.clone();
            let is_strong = matches!(prop, PropExpr::Strong(_));
            let is_weak = matches!(prop, PropExpr::Weak(_));
            let one = arena.alloc_const(CVal::new(1, false, 1), false, loc);
            let init = vec![SeqThread { delay: 0, guard: one, env: env0.clone() }];
            let Some(threads) = enum_seq_threads(arena, &s_cloned, init, sva_ids, loc) else {
                return false;
            };
            if threads.is_empty() {
                return false;
            }
            let mut acc: Option<SeqExpr> = None;
            for th in threads {
                let term = delayed_bool_seq(th.delay, th.guard);
                acc = Some(match acc {
                    None => term,
                    Some(prev) => SeqExpr::Or(Box::new(prev), Box::new(term)),
                });
            }
            let built = acc.unwrap();
            *prop = if is_strong {
                PropExpr::Strong(built)
            } else if is_weak {
                PropExpr::Weak(built)
            } else {
                PropExpr::Seq(built)
            };
            true
        }
        PropExpr::And(a, b) => {
            rewrite_bounded_prop(arena, a, env0, sva_ids, loc) && rewrite_bounded_prop(arena, b, env0, sva_ids, loc)
        }
        PropExpr::Or(a, b) => {
            rewrite_bounded_prop(arena, a, env0, sva_ids, loc) && rewrite_bounded_prop(arena, b, env0, sva_ids, loc)
        }
        PropExpr::IfElse { cond, then_prop, else_prop } => {
            let Some(c_sub) = subst_expr(arena, *cond, env0, sva_ids, 0) else {
                return false;
            };
            *cond = c_sub;
            if !rewrite_bounded_prop(arena, then_prop, env0, sva_ids, loc) {
                return false;
            }
            if let Some(ep) = else_prop {
                if !rewrite_bounded_prop(arena, ep, env0, sva_ids, loc) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn desugar_implication(
    arena: &mut ExprArena,
    out_prop: &mut PropExpr,
    lhs: SeqExpr,
    rhs: PropExpr,
    rhs_offset: u32,
    env0: &HashMap<LocalId, ExprId>,
    sva_ids: &HashSet<LocalId>,
    loc: Loc,
) -> bool {
    let one = arena.alloc_const(CVal::new(1, false, 1), false, loc);
    let init = vec![SeqThread { delay: 0, guard: one, env: env0.clone() }];
    let Some(threads) = enum_seq_threads(arena, &lhs, init, sva_ids, loc) else {
        return false;
    };
    if threads.is_empty() {
        return false;
    }
    let mut conj: Option<PropExpr> = None;
    for th in threads {
        let Some(rhs_sub) = subst_consequent_prop(arena, &rhs, &th.env, sva_ids, rhs_offset, loc) else {
            return false;
        };
        let ant_seq = delayed_bool_seq(th.delay, th.guard);
        let imp = if rhs_offset == 0 {
            PropExpr::OverlappedImplication { lhs: ant_seq, rhs: Box::new(rhs_sub) }
        } else {
            PropExpr::NonOverlappedImplication { lhs: ant_seq, rhs: Box::new(rhs_sub) }
        };
        conj = Some(match conj {
            None => imp,
            Some(prev) => PropExpr::And(Box::new(prev), Box::new(imp)),
        });
    }
    *out_prop = conj.unwrap();
    true
}

fn delayed_bool_seq(delay: u32, guard: ExprId) -> SeqExpr {
    if delay == 0 {
        SeqExpr::Bool(guard)
    } else {
        SeqExpr::Delay { range: CycleRange::exact(delay), lhs: None, rhs: Box::new(SeqExpr::Bool(guard)) }
    }
}

fn subst_consequent_prop(
    arena: &mut ExprArena,
    prop: &PropExpr,
    env: &HashMap<LocalId, ExprId>,
    sva_ids: &HashSet<LocalId>,
    offset: u32,
    loc: Loc,
) -> Option<PropExpr> {
    if offset > MAX_LOCAL_DELAY {
        return None;
    }
    match prop {
        PropExpr::Seq(s) => Some(PropExpr::Seq(subst_consequent_seq(arena, s, env, sva_ids, offset, loc)?)),
        PropExpr::Strong(s) => Some(PropExpr::Strong(subst_consequent_seq(arena, s, env, sva_ids, offset, loc)?)),
        PropExpr::Weak(s) => Some(PropExpr::Weak(subst_consequent_seq(arena, s, env, sva_ids, offset, loc)?)),
        PropExpr::Not(inner) => {
            Some(PropExpr::Not(Box::new(subst_consequent_prop(arena, inner, env, sva_ids, offset, loc)?)))
        }
        PropExpr::And(a, b) => Some(PropExpr::And(
            Box::new(subst_consequent_prop(arena, a, env, sva_ids, offset, loc)?),
            Box::new(subst_consequent_prop(arena, b, env, sva_ids, offset, loc)?),
        )),
        PropExpr::Or(a, b) => Some(PropExpr::Or(
            Box::new(subst_consequent_prop(arena, a, env, sva_ids, offset, loc)?),
            Box::new(subst_consequent_prop(arena, b, env, sva_ids, offset, loc)?),
        )),
        PropExpr::Implies(a, b) => Some(PropExpr::Implies(
            Box::new(subst_consequent_prop(arena, a, env, sva_ids, offset, loc)?),
            Box::new(subst_consequent_prop(arena, b, env, sva_ids, offset, loc)?),
        )),
        PropExpr::Iff(a, b) => Some(PropExpr::Iff(
            Box::new(subst_consequent_prop(arena, a, env, sva_ids, offset, loc)?),
            Box::new(subst_consequent_prop(arena, b, env, sva_ids, offset, loc)?),
        )),
        PropExpr::Nexttime { strong, cycles, prop: inner } => Some(PropExpr::Nexttime {
            strong: *strong,
            cycles: *cycles,
            prop: Box::new(subst_consequent_prop(arena, inner, env, sva_ids, offset.checked_add(*cycles)?, loc)?),
        }),
        PropExpr::IfElse { cond, then_prop, else_prop } => {
            let c_sub = subst_expr(arena, *cond, env, sva_ids, offset)?;
            let tp = subst_consequent_prop(arena, then_prop, env, sva_ids, offset, loc)?;
            let ep = match else_prop {
                Some(e) => Some(Box::new(subst_consequent_prop(arena, e, env, sva_ids, offset, loc)?)),
                None => None,
            };
            Some(PropExpr::IfElse { cond: c_sub, then_prop: Box::new(tp), else_prop: ep })
        }
        PropExpr::OverlappedImplication { lhs, rhs } => {
            // Nested bounded implication inside consequent (`##[1:4] (rsp && id == tag) |-> (data == d)`).
            let shifted_env = shift_env(arena, env, offset, loc);
            let mut tmp = prop.clone();
            if desugar_implication(arena, &mut tmp, lhs.clone(), (**rhs).clone(), 0, &shifted_env, sva_ids, loc) {
                Some(tmp)
            } else {
                None
            }
        }
        PropExpr::NonOverlappedImplication { lhs, rhs } => {
            let shifted_env = shift_env(arena, env, offset, loc);
            let mut tmp = prop.clone();
            if desugar_implication(arena, &mut tmp, lhs.clone(), (**rhs).clone(), 1, &shifted_env, sva_ids, loc) {
                Some(tmp)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn subst_consequent_seq(
    arena: &mut ExprArena,
    seq: &SeqExpr,
    env: &HashMap<LocalId, ExprId>,
    sva_ids: &HashSet<LocalId>,
    offset: u32,
    loc: Loc,
) -> Option<SeqExpr> {
    if !seq_mentions_locals(arena, seq, sva_ids) {
        return Some(seq.clone());
    }
    let shifted_env = shift_env(arena, env, offset, loc);
    let one = arena.alloc_const(CVal::new(1, false, 1), false, loc);
    let init = vec![SeqThread { delay: 0, guard: one, env: shifted_env }];
    let threads = enum_seq_threads(arena, seq, init, sva_ids, loc)?;
    if threads.is_empty() {
        return None;
    }
    let mut acc: Option<SeqExpr> = None;
    for th in threads {
        let term = delayed_bool_seq(th.delay, th.guard);
        acc = Some(match acc {
            None => term,
            Some(prev) => SeqExpr::Or(Box::new(prev), Box::new(term)),
        });
    }
    acc
}

fn enum_seq_threads(
    arena: &mut ExprArena,
    seq: &SeqExpr,
    in_threads: Vec<SeqThread>,
    sva_ids: &HashSet<LocalId>,
    loc: Loc,
) -> Option<Vec<SeqThread>> {
    if in_threads.len() > MAX_LOCAL_THREADS {
        return None;
    }
    match seq {
        SeqExpr::Bool(b) => {
            let mut out = Vec::with_capacity(in_threads.len());
            for mut th in in_threads {
                let b_sub = subst_expr(arena, *b, &th.env, sva_ids, 0)?;
                th.guard = and_bool_expr(arena, th.guard, b_sub, loc);
                out.push(th);
            }
            Some(out)
        }
        SeqExpr::MatchItems { seq: inner, assigns } => {
            let mut mid = enum_seq_threads(arena, inner, in_threads, sva_ids, loc)?;
            for th in &mut mid {
                for (lid, rhs) in assigns {
                    let val = subst_expr(arena, *rhs, &th.env, sva_ids, 0)?;
                    th.env.insert(*lid, val);
                }
            }
            Some(mid)
        }
        SeqExpr::Fusion(a, b) => {
            let mid = enum_seq_threads(arena, a, in_threads, sva_ids, loc)?;
            enum_seq_threads(arena, b, mid, sva_ids, loc)
        }
        SeqExpr::Delay { range, lhs, rhs } => {
            let CycleBound::Const(max_d) = range.max else { return None };
            if max_d < range.min || max_d > MAX_LOCAL_DELAY {
                return None;
            }
            let lhs_threads = match lhs {
                Some(l) => enum_seq_threads(arena, l, in_threads, sva_ids, loc)?,
                None => in_threads,
            };
            let mut out = Vec::new();
            for th in &lhs_threads {
                for d in range.min..=max_d {
                    let next_delay = th.delay.checked_add(d)?;
                    if next_delay > MAX_LOCAL_DELAY {
                        return None;
                    }
                    let shifted = SeqThread {
                        delay: next_delay,
                        guard: shift_past(arena, th.guard, d, loc),
                        env: shift_env(arena, &th.env, d, loc),
                    };
                    let mut branch = enum_seq_threads(arena, rhs, vec![shifted], sva_ids, loc)?;
                    if out.len() + branch.len() > MAX_LOCAL_THREADS {
                        return None;
                    }
                    out.append(&mut branch);
                }
            }
            Some(out)
        }
        SeqExpr::Or(a, b) => {
            let mut la = enum_seq_threads(arena, a, in_threads.clone(), sva_ids, loc)?;
            let mut lb = enum_seq_threads(arena, b, in_threads, sva_ids, loc)?;
            if la.len() + lb.len() > MAX_LOCAL_THREADS {
                return None;
            }
            la.append(&mut lb);
            Some(la)
        }
        SeqExpr::Repeat { kind: RepeatKind::Consecutive, range, seq: inner } => {
            let CycleBound::Const(max_r) = range.max else { return None };
            if max_r < range.min || range.min == 0 || max_r > 16 {
                return None;
            }
            let mut out = Vec::new();
            for reps in range.min..=max_r {
                let mut cur = in_threads.clone();
                for step in 0..reps {
                    if step > 0 {
                        for th in &mut cur {
                            th.delay = th.delay.checked_add(1)?;
                            if th.delay > MAX_LOCAL_DELAY {
                                return None;
                            }
                            th.guard = shift_past(arena, th.guard, 1, loc);
                            th.env = shift_env(arena, &th.env, 1, loc);
                        }
                    }
                    cur = enum_seq_threads(arena, inner, cur, sva_ids, loc)?;
                }
                if out.len() + cur.len() > MAX_LOCAL_THREADS {
                    return None;
                }
                out.append(&mut cur);
            }
            Some(out)
        }
        SeqExpr::FirstMatch(inner) => {
            let mut out = Vec::new();
            for base_th in in_threads {
                let base_delay = base_th.delay;
                let mut sub = enum_seq_threads(arena, inner, vec![base_th], sva_ids, loc)?;
                sub.sort_by_key(|t| t.delay);
                let mut earlier: Vec<(u32, ExprId)> = Vec::new();
                for mut th in sub {
                    let rel = th.delay.saturating_sub(base_delay);
                    let mut g = th.guard;
                    for &(e_rel, e_guard) in &earlier {
                        if e_rel < rel {
                            let prev_at_now = shift_past(arena, e_guard, rel - e_rel, loc);
                            let not_prev = arena.alloc(
                                ExprKind::Unary { op: UnaryOp::LogNot, arg: prev_at_now },
                                RtlType::Integral { width: 1, signed: false, four_state: false },
                                None,
                                loc,
                            );
                            g = and_bool_expr(arena, g, not_prev, loc);
                        }
                    }
                    earlier.push((rel, th.guard));
                    th.guard = g;
                    if out.len() >= MAX_LOCAL_THREADS {
                        return None;
                    }
                    out.push(th);
                }
            }
            Some(out)
        }
        _ => None,
    }
}

fn shift_env(arena: &mut ExprArena, env: &HashMap<LocalId, ExprId>, d: u32, loc: Loc) -> HashMap<LocalId, ExprId> {
    if d == 0 {
        return env.clone();
    }
    env.iter().map(|(&k, &v)| (k, shift_past(arena, v, d, loc))).collect()
}

fn shift_past(arena: &mut ExprArena, eid: ExprId, k: u32, loc: Loc) -> ExprId {
    if k == 0 || arena.get(eid).cval.is_some() {
        return eid;
    }
    let e = arena.get(eid).clone();
    if let ExprKind::Sampled {
        kind: SampledKind::Past,
        expr: inner,
        depth,
        gate: None,
        clock: None,
        clock_posedge: None,
    } = e.kind
    {
        return arena.alloc(
            ExprKind::Sampled {
                kind: SampledKind::Past,
                expr: inner,
                depth: depth + k,
                gate: None,
                clock: None,
                clock_posedge: None,
            },
            e.ty,
            None,
            loc,
        );
    }
    arena.alloc(
        ExprKind::Sampled {
            kind: SampledKind::Past,
            expr: eid,
            depth: k,
            gate: None,
            clock: None,
            clock_posedge: None,
        },
        e.ty,
        None,
        loc,
    )
}

fn and_bool_expr(arena: &mut ExprArena, a: ExprId, b: ExprId, loc: Loc) -> ExprId {
    if arena.get(a).cval.as_ref().is_some_and(|c| c.known() && c.is_true()) {
        return b;
    }
    if arena.get(b).cval.as_ref().is_some_and(|c| c.known() && c.is_true()) {
        return a;
    }
    arena.alloc(
        ExprKind::Binary { op: BinaryOp::LogAnd, lhs: a, rhs: b },
        RtlType::Integral { width: 1, signed: false, four_state: false },
        None,
        loc,
    )
}

fn expr_mentions_locals(arena: &ExprArena, eid: ExprId, sva_ids: &HashSet<LocalId>) -> bool {
    match &arena.get(eid).kind {
        ExprKind::Local(lid) => sva_ids.contains(lid),
        ExprKind::Symbol(_) | ExprKind::Const(_) | ExprKind::UnbasedUnsized(_) => false,
        ExprKind::Unary { arg, .. }
        | ExprKind::Replicate { expr: arg, .. }
        | ExprKind::Member { base: arg, .. }
        | ExprKind::PartSelect { base: arg, .. }
        | ExprKind::Resize { arg, .. }
        | ExprKind::CastSign { arg, .. }
        | ExprKind::CastFourState { arg, .. } => expr_mentions_locals(arena, *arg, sva_ids),
        ExprKind::Binary { lhs, rhs, .. }
        | ExprKind::BitSelect { base: lhs, index: rhs, .. }
        | ExprKind::IndexedPartSelect { base: lhs, base_idx: rhs, .. }
        | ExprKind::ArrayIndex { base: lhs, index: rhs, .. } => {
            expr_mentions_locals(arena, *lhs, sva_ids) || expr_mentions_locals(arena, *rhs, sva_ids)
        }
        ExprKind::Cond { cond, then_val, else_val } => {
            expr_mentions_locals(arena, *cond, sva_ids)
                || expr_mentions_locals(arena, *then_val, sva_ids)
                || expr_mentions_locals(arena, *else_val, sva_ids)
        }
        ExprKind::Inside { expr, items } => {
            expr_mentions_locals(arena, *expr, sva_ids)
                || items.iter().any(|it| match it {
                    InsideItem::Single(x) => expr_mentions_locals(arena, *x, sva_ids),
                    InsideItem::Range(l, h) => {
                        expr_mentions_locals(arena, *l, sva_ids) || expr_mentions_locals(arena, *h, sva_ids)
                    }
                })
        }
        ExprKind::Concat(parts) | ExprKind::SysCall { args: parts, .. } | ExprKind::FuncCall { args: parts, .. } => {
            parts.iter().any(|&p| expr_mentions_locals(arena, p, sva_ids))
        }
        ExprKind::Sampled { expr, gate, clock, .. } => {
            expr_mentions_locals(arena, *expr, sva_ids)
                || gate.is_some_and(|g| expr_mentions_locals(arena, g, sva_ids))
                || clock.is_some_and(|c| expr_mentions_locals(arena, c, sva_ids))
        }
        ExprKind::AssignPattern(entries) => entries.iter().any(|(_, x)| expr_mentions_locals(arena, *x, sva_ids)),
    }
}

fn seq_mentions_locals(arena: &ExprArena, seq: &SeqExpr, sva_ids: &HashSet<LocalId>) -> bool {
    match seq {
        SeqExpr::Bool(e) => expr_mentions_locals(arena, *e, sva_ids),
        SeqExpr::MatchItems { .. } => true,
        SeqExpr::Delay { lhs, rhs, .. } => {
            lhs.as_ref().is_some_and(|l| seq_mentions_locals(arena, l, sva_ids))
                || seq_mentions_locals(arena, rhs, sva_ids)
        }
        SeqExpr::Fusion(a, b)
        | SeqExpr::And(a, b)
        | SeqExpr::Or(a, b)
        | SeqExpr::Intersect(a, b)
        | SeqExpr::Within(a, b) => seq_mentions_locals(arena, a, sva_ids) || seq_mentions_locals(arena, b, sva_ids),
        SeqExpr::Repeat { seq: inner, .. } | SeqExpr::FirstMatch(inner) | SeqExpr::Clocked { seq: inner, .. } => {
            seq_mentions_locals(arena, inner, sva_ids)
        }
        SeqExpr::Throughout { cond, seq: inner } => {
            expr_mentions_locals(arena, *cond, sva_ids) || seq_mentions_locals(arena, inner, sva_ids)
        }
    }
}

fn subst_expr(
    arena: &mut ExprArena,
    eid: ExprId,
    env: &HashMap<LocalId, ExprId>,
    sva_ids: &HashSet<LocalId>,
    past_shift: u32,
) -> Option<ExprId> {
    let e = arena.get(eid).clone();
    if !expr_mentions_locals(arena, eid, sva_ids) {
        return Some(shift_past(arena, eid, past_shift, e.loc));
    }
    let loc = e.loc;
    let ty = e.ty.clone();
    match e.kind {
        ExprKind::Local(lid) => {
            if sva_ids.contains(&lid) {
                let bound = *env.get(&lid)?;
                Some(shift_past(arena, bound, past_shift, loc))
            } else {
                Some(eid)
            }
        }
        ExprKind::Symbol(_) | ExprKind::Const(_) | ExprKind::UnbasedUnsized(_) => {
            Some(shift_past(arena, eid, past_shift, loc))
        }
        ExprKind::Unary { op, arg } => {
            let a = subst_expr(arena, arg, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Unary { op, arg: a }, ty, None, loc))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let l = subst_expr(arena, lhs, env, sva_ids, past_shift)?;
            let r = subst_expr(arena, rhs, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Binary { op, lhs: l, rhs: r }, ty, None, loc))
        }
        ExprKind::Cond { cond, then_val, else_val } => {
            let c = subst_expr(arena, cond, env, sva_ids, past_shift)?;
            let tv = subst_expr(arena, then_val, env, sva_ids, past_shift)?;
            let ev = subst_expr(arena, else_val, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Cond { cond: c, then_val: tv, else_val: ev }, ty, None, loc))
        }
        ExprKind::Inside { expr, items } => {
            let x = subst_expr(arena, expr, env, sva_ids, past_shift)?;
            let mut ni = Vec::with_capacity(items.len());
            for it in items {
                ni.push(match it {
                    InsideItem::Single(v) => InsideItem::Single(subst_expr(arena, v, env, sva_ids, past_shift)?),
                    InsideItem::Range(l, h) => InsideItem::Range(
                        subst_expr(arena, l, env, sva_ids, past_shift)?,
                        subst_expr(arena, h, env, sva_ids, past_shift)?,
                    ),
                });
            }
            Some(arena.alloc(ExprKind::Inside { expr: x, items: ni }, ty, None, loc))
        }
        ExprKind::Concat(parts) => {
            let mut np = Vec::with_capacity(parts.len());
            for p in parts {
                np.push(subst_expr(arena, p, env, sva_ids, past_shift)?);
            }
            Some(arena.alloc(ExprKind::Concat(np), ty, None, loc))
        }
        ExprKind::Replicate { count, expr } => {
            let x = subst_expr(arena, expr, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Replicate { count, expr: x }, ty, None, loc))
        }
        ExprKind::Member { base, field, packed_slice } => {
            let b = subst_expr(arena, base, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Member { base: b, field, packed_slice }, ty, None, loc))
        }
        ExprKind::BitSelect { base, index, bounds, elem_width } => {
            let b = subst_expr(arena, base, env, sva_ids, past_shift)?;
            let i = subst_expr(arena, index, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::BitSelect { base: b, index: i, bounds, elem_width }, ty, None, loc))
        }
        ExprKind::PartSelect { base, msb, lsb, bounds, elem_width } => {
            let b = subst_expr(arena, base, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::PartSelect { base: b, msb, lsb, bounds, elem_width }, ty, None, loc))
        }
        ExprKind::IndexedPartSelect { base, base_idx, width, ascending, bounds, elem_width } => {
            let b = subst_expr(arena, base, env, sva_ids, past_shift)?;
            let idx = subst_expr(arena, base_idx, env, sva_ids, past_shift)?;
            Some(arena.alloc(
                ExprKind::IndexedPartSelect { base: b, base_idx: idx, width, ascending, bounds, elem_width },
                ty,
                None,
                loc,
            ))
        }
        ExprKind::ArrayIndex { base, index, dim } => {
            let b = subst_expr(arena, base, env, sva_ids, past_shift)?;
            let idx = subst_expr(arena, index, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::ArrayIndex { base: b, index: idx, dim }, ty, None, loc))
        }
        ExprKind::Resize { arg, width, signed } => {
            let a = subst_expr(arena, arg, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::Resize { arg: a, width, signed }, ty, None, loc))
        }
        ExprKind::CastSign { arg, signed } => {
            let a = subst_expr(arena, arg, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::CastSign { arg: a, signed }, ty, None, loc))
        }
        ExprKind::CastFourState { arg, four_state } => {
            let a = subst_expr(arena, arg, env, sva_ids, past_shift)?;
            Some(arena.alloc(ExprKind::CastFourState { arg: a, four_state }, ty, None, loc))
        }
        ExprKind::SysCall { func, args } => {
            let mut na = Vec::with_capacity(args.len());
            for a in args {
                na.push(subst_expr(arena, a, env, sva_ids, past_shift)?);
            }
            Some(arena.alloc(ExprKind::SysCall { func, args: na }, ty, None, loc))
        }
        ExprKind::FuncCall { callee, callee_inst, callee_scope_prefix, name, args } => {
            let mut na = Vec::with_capacity(args.len());
            for a in args {
                na.push(subst_expr(arena, a, env, sva_ids, past_shift)?);
            }
            Some(arena.alloc(
                ExprKind::FuncCall { callee, callee_inst, callee_scope_prefix, name, args: na },
                ty,
                None,
                loc,
            ))
        }
        ExprKind::Sampled { kind: SampledKind::Past, expr, depth, gate: None, clock: None, clock_posedge: None } => {
            subst_expr(arena, expr, env, sva_ids, past_shift.checked_add(depth)?)
        }
        ExprKind::Sampled { .. } | ExprKind::AssignPattern(_) => None,
    }
}

/// Pass B: single-assignment symbolic constant prophecy reduction for unbounded temporal properties.
fn try_desugar_prophecy_locals(arena: &mut ExprArena, prop: &mut BoundProperty) -> Option<Vec<(LocalId, RtlType)>> {
    if prop.sva_locals.iter().any(|(_, _, init)| init.is_some()) {
        return None;
    }
    let mut write_counts: HashMap<LocalId, usize> = HashMap::new();
    let loc = prop.loc;
    let ty_by_id: HashMap<LocalId, RtlType> = prop.sva_locals.iter().map(|(id, ty, _)| (*id, ty.clone())).collect();

    let mut expr_clone = prop.expr.clone();
    match prop.kind {
        PropKind::Assert => {
            if !rewrite_assert_prophecy(arena, &mut expr_clone, &ty_by_id, &mut write_counts, loc) {
                return None;
            }
        }
        PropKind::Cover => {
            if !rewrite_cover_prophecy(arena, &mut expr_clone, &ty_by_id, &mut write_counts, loc) {
                return None;
            }
        }
        PropKind::Assume | PropKind::Restrict => return None,
    }

    if write_counts.values().any(|&n| n != 1) || write_counts.len() != prop.sva_locals.len() {
        return None;
    }
    prop.expr = expr_clone;
    Some(prop.sva_locals.iter().map(|(id, ty, _)| (*id, ty.clone())).collect())
}

fn rewrite_assert_prophecy(
    arena: &mut ExprArena,
    prop: &mut PropExpr,
    ty_by_id: &HashMap<LocalId, RtlType>,
    writes: &mut HashMap<LocalId, usize>,
    loc: Loc,
) -> bool {
    match prop {
        PropExpr::Clocked { prop: inner, .. }
        | PropExpr::AcceptOn { prop: inner, .. }
        | PropExpr::Always { prop: inner, .. } => rewrite_assert_prophecy(arena, inner, ty_by_id, writes, loc),
        PropExpr::OverlappedImplication { lhs, rhs } | PropExpr::NonOverlappedImplication { lhs, rhs } => {
            if prop_has_match_items(rhs) {
                return false;
            }
            rewrite_seq_prophecy(arena, lhs, ty_by_id, writes, loc)
        }
        _ => false,
    }
}

fn rewrite_cover_prophecy(
    arena: &mut ExprArena,
    prop: &mut PropExpr,
    ty_by_id: &HashMap<LocalId, RtlType>,
    writes: &mut HashMap<LocalId, usize>,
    loc: Loc,
) -> bool {
    match prop {
        PropExpr::Clocked { prop: inner, .. } | PropExpr::AcceptOn { prop: inner, .. } => {
            rewrite_cover_prophecy(arena, inner, ty_by_id, writes, loc)
        }
        PropExpr::Seq(s) | PropExpr::Strong(s) | PropExpr::Weak(s) => {
            rewrite_seq_prophecy(arena, s, ty_by_id, writes, loc)
        }
        PropExpr::FollowedBy { lhs, rhs, .. } => {
            rewrite_seq_prophecy(arena, lhs, ty_by_id, writes, loc)
                && (!prop_has_match_items(rhs) || rewrite_cover_prophecy(arena, rhs, ty_by_id, writes, loc))
        }
        _ => false,
    }
}

pub(crate) fn prop_has_match_items(prop: &PropExpr) -> bool {
    match prop {
        PropExpr::Seq(s) | PropExpr::Strong(s) | PropExpr::Weak(s) => seq_has_match_items(s),
        PropExpr::Not(p)
        | PropExpr::Nexttime { prop: p, .. }
        | PropExpr::Always { prop: p, .. }
        | PropExpr::Eventually { prop: p, .. }
        | PropExpr::AcceptOn { prop: p, .. }
        | PropExpr::Clocked { prop: p, .. } => prop_has_match_items(p),
        PropExpr::And(a, b)
        | PropExpr::Or(a, b)
        | PropExpr::Implies(a, b)
        | PropExpr::Iff(a, b)
        | PropExpr::Until { lhs: a, rhs: b, .. } => prop_has_match_items(a) || prop_has_match_items(b),
        PropExpr::OverlappedImplication { lhs, rhs }
        | PropExpr::NonOverlappedImplication { lhs, rhs }
        | PropExpr::FollowedBy { lhs, rhs, .. } => seq_has_match_items(lhs) || prop_has_match_items(rhs),
        PropExpr::IfElse { then_prop, else_prop, .. } => {
            prop_has_match_items(then_prop) || else_prop.as_ref().is_some_and(|e| prop_has_match_items(e))
        }
    }
}

fn seq_has_match_items(seq: &SeqExpr) -> bool {
    match seq {
        SeqExpr::Bool(_) => false,
        SeqExpr::MatchItems { .. } => true,
        SeqExpr::Delay { lhs, rhs, .. } => {
            lhs.as_ref().is_some_and(|l| seq_has_match_items(l)) || seq_has_match_items(rhs)
        }
        SeqExpr::Fusion(a, b)
        | SeqExpr::And(a, b)
        | SeqExpr::Or(a, b)
        | SeqExpr::Intersect(a, b)
        | SeqExpr::Within(a, b) => seq_has_match_items(a) || seq_has_match_items(b),
        SeqExpr::Repeat { seq: inner, .. }
        | SeqExpr::FirstMatch(inner)
        | SeqExpr::Throughout { seq: inner, .. }
        | SeqExpr::Clocked { seq: inner, .. } => seq_has_match_items(inner),
    }
}

fn rewrite_seq_prophecy(
    arena: &mut ExprArena,
    seq: &mut SeqExpr,
    ty_by_id: &HashMap<LocalId, RtlType>,
    writes: &mut HashMap<LocalId, usize>,
    loc: Loc,
) -> bool {
    match seq {
        SeqExpr::Bool(_) => true,
        SeqExpr::MatchItems { seq: inner, assigns } => {
            if !rewrite_seq_prophecy(arena, inner, ty_by_id, writes, loc) {
                return false;
            }
            let mut eq_guard: Option<ExprId> = None;
            for (lid, rhs) in assigns.iter() {
                // Self-referential update `v = v + 1` cannot be represented as a single prophecy equality.
                if expr_mentions_locals(arena, *rhs, &HashSet::from([*lid])) {
                    return false;
                }
                *writes.entry(*lid).or_insert(0) += 1;
                let Some(ty) = ty_by_id.get(lid).cloned() else { return false };
                let lref = arena.alloc(ExprKind::Local(*lid), ty, None, loc);
                let eq = arena.alloc(
                    ExprKind::Binary { op: BinaryOp::Eq, lhs: lref, rhs: *rhs },
                    RtlType::Integral { width: 1, signed: false, four_state: false },
                    None,
                    loc,
                );
                eq_guard = Some(match eq_guard {
                    None => eq,
                    Some(prev) => and_bool_expr(arena, prev, eq, loc),
                });
            }
            let inner_taken = (**inner).clone();
            *seq = match eq_guard {
                Some(g) => SeqExpr::Fusion(Box::new(inner_taken), Box::new(SeqExpr::Bool(g))),
                None => inner_taken,
            };
            true
        }
        SeqExpr::Delay { lhs, rhs, .. } => {
            if let Some(l) = lhs {
                if !rewrite_seq_prophecy(arena, l, ty_by_id, writes, loc) {
                    return false;
                }
            }
            rewrite_seq_prophecy(arena, rhs, ty_by_id, writes, loc)
        }
        SeqExpr::Fusion(a, b)
        | SeqExpr::And(a, b)
        | SeqExpr::Or(a, b)
        | SeqExpr::Intersect(a, b)
        | SeqExpr::Within(a, b) => {
            rewrite_seq_prophecy(arena, a, ty_by_id, writes, loc)
                && rewrite_seq_prophecy(arena, b, ty_by_id, writes, loc)
        }
        SeqExpr::Repeat { range, seq: inner, .. } => {
            // A loop writing a local variable on multiple ticks cannot bind a single constant.
            if seq_has_match_items(inner) && range.max != CycleBound::Const(1) {
                return false;
            }
            rewrite_seq_prophecy(arena, inner, ty_by_id, writes, loc)
        }
        SeqExpr::FirstMatch(inner) | SeqExpr::Throughout { seq: inner, .. } | SeqExpr::Clocked { seq: inner, .. } => {
            rewrite_seq_prophecy(arena, inner, ty_by_id, writes, loc)
        }
    }
}
