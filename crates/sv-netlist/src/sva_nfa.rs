//! Automaton-based SVA compiler (`[->m:n]`, `[=m:n]`, `##[m:$]`, `[*m:$]`, `strong(...)`,
//! and ranged `s_eventually`).
//!
//! Bounded SVA properties without `[->]`/`[=]`/`$` are compiled by `Matcher` in [`crate::sva`].
//! Properties that contain unbounded ranges (`$`), goto repetition (`[->]`), non-consecutive
//! repetition (`[=]`), or `strong(...)` / `s_eventually [m:$]` over sequences are compiled here
//! into exact finite-state monitors:
//! - Antecedents and `cover property` checks use bit-parallel Glushkov/Thompson NFAs (`SeqNfa`)
//!   with zero combinational loops (factoring nullable operands as `epsilon | S_+`).
//! - Unbounded `assert`/`assume` consequents use exact on-the-fly subset construction (`ConsequentDfa`)
//!   synthesized via memoized Shannon ROBDDs, plus witness-picker (`Assert`) and Miyano-Hayashi
//!   breakpoint (`Assume`) liveness monitors for `strong` / `s_eventually` obligations.

use std::collections::{BTreeMap, HashMap, VecDeque};

use sv_semantic::{BoundProperty, CVal, CycleBound, CycleRange, ExprId, Loc, PropExpr, PropKind, RepeatKind, SeqExpr};

use crate::ir::{CheckKind, ClockSpec, FreeKind, NetOp, Netlist, State, StateId, UndrivenSlice, ValueId, ValueKind};
use crate::sva::{
    as_seq, bounds_checked, live_terms, prop_name, too_large, too_long, Cond, Lookback, Matcher, Monitor, Plan,
    PropertyOptions, Term,
};

/// Maximum NFA state flip-flops for an existential monitor (antecedent or `cover`).
const MAX_NFA_STATES: usize = 256;

/// Maximum NFA states in a universal consequent before subset construction (`u64` mask).
const MAX_CONSEQUENT_NFA_STATES: usize = 64;

/// Maximum reachable active DFA states in a universal consequent.
const MAX_DFA_STATES: usize = 64;

/// Maximum boolean variables in a consequent subset-construction truth table (`2^10 = 1024`).
const MAX_DFA_VARS: usize = 10;

/// Returns `true` when `prop` requires the automaton compiler instead of bounded `Matcher`.
pub(crate) fn prop_needs_automaton(prop: &PropExpr) -> bool {
    match prop {
        PropExpr::Seq(s) | PropExpr::Weak(s) => seq_needs_automaton(s),
        PropExpr::Strong(s) => {
            // `strong(bounded_seq)` agrees with `bounded_seq` on infinite traces; unbounded needs liveness.
            seq_is_unbounded_len(s) || seq_needs_automaton(s)
        }
        PropExpr::OverlappedImplication { lhs, rhs } | PropExpr::NonOverlappedImplication { lhs, rhs } => {
            seq_needs_automaton(lhs) || rhs_needs_automaton(rhs)
        }
        PropExpr::Nexttime { prop: inner, .. } => prop_needs_automaton(inner),
        PropExpr::Eventually { strong, range, prop: inner } => {
            if !*strong {
                return false;
            }
            // Bare `s_eventually bool` (`[0:$]`) with bounded antecedent is handled by `plan_until`.
            if range.min == 0
                && range.max == CycleBound::Unbounded
                && matches!(as_seq(inner).as_deref(), Ok(SeqExpr::Bool(_)))
            {
                return false;
            }
            range.max == CycleBound::Unbounded || prop_needs_automaton(inner)
        }
        PropExpr::Always { range, prop: inner, .. } => {
            range.max != CycleBound::Unbounded && prop_needs_automaton(inner)
        }
        PropExpr::Until { .. } => false,
        _ => false,
    }
}

fn rhs_needs_automaton(rhs: &PropExpr) -> bool {
    if matches!(rhs, PropExpr::Until { .. }) {
        return false;
    }
    prop_needs_automaton(rhs)
}

pub(crate) fn seq_needs_automaton(s: &SeqExpr) -> bool {
    match s {
        SeqExpr::Bool(_) => false,
        SeqExpr::Delay { range, lhs, rhs } => {
            range.max == CycleBound::Unbounded
                || lhs.as_deref().is_some_and(seq_needs_automaton)
                || seq_needs_automaton(rhs)
        }
        SeqExpr::Fusion(a, b)
        | SeqExpr::Or(a, b)
        | SeqExpr::And(a, b)
        | SeqExpr::Intersect(a, b)
        | SeqExpr::Within(a, b) => seq_needs_automaton(a) || seq_needs_automaton(b),
        SeqExpr::Repeat { kind, range, seq } => {
            matches!(kind, RepeatKind::Goto | RepeatKind::NonConsecutive)
                || range.max == CycleBound::Unbounded
                || seq_needs_automaton(seq)
        }
        SeqExpr::Throughout { seq, .. } | SeqExpr::FirstMatch(seq) | SeqExpr::Clocked { seq, .. } => {
            seq_needs_automaton(seq)
        }
    }
}

fn seq_is_unbounded_len(s: &SeqExpr) -> bool {
    seq_needs_automaton(s)
}

/// Symbolic variable in [`GuardPool`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GVar {
    /// Start/injection signal into the sequence automaton at the current tick.
    Inject,
    /// Current-tick output of NFA state flip-flop `i`.
    NfaState(u16),
    /// Positive SVA truth of leaf `k` (`two_state(bool(val))`).
    PosLeaf(u16),
    /// Negative SVA truth of 4-state leaf `k` (`two_state(!bool(val))`, `0` on `'x`).
    NegLeaf(u16),
}

/// Hash-consed boolean formula node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Guard {
    False,
    True,
    Node(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum GNode {
    Var(GVar),
    Not(u32),
    And(Guard, Guard),
    Or(Guard, Guard),
}

#[derive(Clone, Default)]
pub(crate) struct GuardPool {
    nodes: Vec<GNode>,
    intern: HashMap<GNode, u32>,
    work: u32,
}

impl GuardPool {
    fn step(&mut self) -> Result<(), String> {
        self.work = self.work.saturating_add(1);
        if self.work > (1 << 22) {
            return Err(too_large());
        }
        Ok(())
    }

    fn mk(&mut self, n: GNode) -> Result<Guard, String> {
        self.step()?;
        if let Some(&id) = self.intern.get(&n) {
            return Ok(Guard::Node(id));
        }
        if self.nodes.len() >= (1 << 16) {
            return Err(too_large());
        }
        let id = self.nodes.len() as u32;
        self.nodes.push(n);
        self.intern.insert(n, id);
        Ok(Guard::Node(id))
    }

    fn var(&mut self, v: GVar) -> Result<Guard, String> {
        self.mk(GNode::Var(v))
    }

    fn not(&mut self, g: Guard) -> Result<Guard, String> {
        match g {
            Guard::False => Ok(Guard::True),
            Guard::True => Ok(Guard::False),
            Guard::Node(i) => {
                if let GNode::Not(inner) = self.nodes[i as usize] {
                    return Ok(Guard::Node(inner));
                }
                self.mk(GNode::Not(i))
            }
        }
    }

    fn and(&mut self, a: Guard, b: Guard) -> Result<Guard, String> {
        match (a, b) {
            (Guard::False, _) | (_, Guard::False) => Ok(Guard::False),
            (Guard::True, x) | (x, Guard::True) => Ok(x),
            _ if a == b => Ok(a),
            _ => {
                let (x, y) = if self.ord_key(a) <= self.ord_key(b) { (a, b) } else { (b, a) };
                self.mk(GNode::And(x, y))
            }
        }
    }

    fn or(&mut self, a: Guard, b: Guard) -> Result<Guard, String> {
        match (a, b) {
            (Guard::True, _) | (_, Guard::True) => Ok(Guard::True),
            (Guard::False, x) | (x, Guard::False) => Ok(x),
            _ if a == b => Ok(a),
            _ => {
                let (x, y) = if self.ord_key(a) <= self.ord_key(b) { (a, b) } else { (b, a) };
                self.mk(GNode::Or(x, y))
            }
        }
    }

    fn ord_key(&self, g: Guard) -> u32 {
        match g {
            Guard::False => 0,
            Guard::True => 1,
            Guard::Node(i) => i + 2,
        }
    }

    fn eval(&self, root: Guard, inject: bool, nfa_mask: u64, pos_bits: u16, neg_bits: u16) -> bool {
        match root {
            Guard::False => false,
            Guard::True => true,
            Guard::Node(idx) => {
                let mut vals = vec![false; (idx as usize) + 1];
                for i in 0..=(idx as usize) {
                    vals[i] = match self.nodes[i] {
                        GNode::Var(GVar::Inject) => inject,
                        GNode::Var(GVar::NfaState(s)) => ((nfa_mask >> s) & 1) != 0,
                        GNode::Var(GVar::PosLeaf(k)) => ((pos_bits >> k) & 1) != 0,
                        GNode::Var(GVar::NegLeaf(k)) => ((neg_bits >> k) & 1) != 0,
                        GNode::Not(a) => !vals[a as usize],
                        GNode::And(a, b) => {
                            let va = match a {
                                Guard::False => false,
                                Guard::True => true,
                                Guard::Node(j) => vals[j as usize],
                            };
                            va && match b {
                                Guard::False => false,
                                Guard::True => true,
                                Guard::Node(j) => vals[j as usize],
                            }
                        }
                        GNode::Or(a, b) => {
                            let va = match a {
                                Guard::False => false,
                                Guard::True => true,
                                Guard::Node(j) => vals[j as usize],
                            };
                            va || match b {
                                Guard::False => false,
                                Guard::True => true,
                                Guard::Node(j) => vals[j as usize],
                            }
                        }
                    };
                }
                vals[idx as usize]
            }
        }
    }
}

/// Leaf table mapping `ExprId` to compact leaf indices.
#[derive(Clone, Default)]
pub(crate) struct LeafTable {
    pub(crate) exprs: Vec<(ValueId, bool)>,
    by_val: HashMap<(ValueId, bool), u16>,
    /// Minimum tick offset from attempt start at which each leaf index can be evaluated.
    pub(crate) min_offset: Vec<u32>,
}

impl LeafTable {
    fn intern(&mut self, val: ValueId, needs_four_state_neg: bool, offset_lb: u32) -> Result<u16, String> {
        if let Some(&idx) = self.by_val.get(&(val, needs_four_state_neg)) {
            self.min_offset[idx as usize] = self.min_offset[idx as usize].min(offset_lb);
            return Ok(idx);
        }
        let idx = u16::try_from(self.exprs.len()).map_err(|_| too_large())?;
        self.exprs.push((val, needs_four_state_neg));
        self.min_offset.push(offset_lb);
        self.by_val.insert((val, needs_four_state_neg), idx);
        Ok(idx)
    }
}

/// Compiled sequence NFA fragment.
#[derive(Clone, Copy, Debug)]
struct Frag {
    /// Whether the sequence admits the empty match (`start == end`).
    accepts_empty: bool,
    /// Combinational signal true on the last tick of any non-empty match.
    end_now: Guard,
    /// Shortest non-empty match length in clock ticks (`None` if no non-empty match exists).
    min_nonempty_len: Option<u32>,
}

struct SeqNfaBuilder<'a> {
    nl: &'a Netlist,
    leaves: &'a [Option<ValueId>],
    pool: GuardPool,
    leaf_tab: LeafTable,
    /// Next-state formula for each allocated NFA flip-flop.
    state_next: Vec<Guard>,
    /// Stack of active `throughout` boolean guards.
    throughout_stack: Vec<Guard>,
    /// Maximum allowed NFA states.
    max_states: usize,
    /// Whether we are inside a repetition loop (disallows `first_match` inside loop body).
    in_loop: bool,
}

impl<'a> SeqNfaBuilder<'a> {
    fn new(nl: &'a Netlist, leaves: &'a [Option<ValueId>], max_states: usize) -> Self {
        Self {
            nl,
            leaves,
            pool: GuardPool::default(),
            leaf_tab: LeafTable::default(),
            state_next: Vec::new(),
            throughout_stack: Vec::new(),
            max_states,
            in_loop: false,
        }
    }

    fn alloc_reg(&mut self, next: Guard) -> Result<(usize, Guard), String> {
        if self.state_next.len() >= self.max_states {
            return Err(if self.max_states <= MAX_CONSEQUENT_NFA_STATES { too_large() } else { too_long() });
        }
        let idx = self.state_next.len();
        let gated_next = self.apply_throughout(next)?;
        self.state_next.push(gated_next);
        let out = self.pool.var(GVar::NfaState(idx as u16))?;
        Ok((idx, out))
    }

    fn apply_throughout(&mut self, mut g: Guard) -> Result<Guard, String> {
        for &th in &self.throughout_stack.clone() {
            g = self.pool.and(g, th)?;
        }
        Ok(g)
    }

    fn pos_leaf(&mut self, e: ExprId, min_off: u32) -> Result<Guard, String> {
        let v = self.leaves.get(e.0 as usize).copied().flatten().ok_or("an expression that was not lowered")?;
        let idx = self.leaf_tab.intern(v, false, min_off)?;
        self.pool.var(GVar::PosLeaf(idx))
    }

    fn pos_and_neg_leaf(&mut self, e: ExprId, min_off: u32) -> Result<(Guard, Guard), String> {
        let v = self.leaves.get(e.0 as usize).copied().flatten().ok_or("an expression that was not lowered")?;
        let four = self.nl.value(v).four_state;
        let idx = self.leaf_tab.intern(v, four, min_off)?;
        let pos = self.pool.var(GVar::PosLeaf(idx))?;
        let neg = if four { self.pool.var(GVar::NegLeaf(idx))? } else { self.pool.not(pos)? };
        Ok((pos, neg))
    }

    /// Delays `sig` by `[min_ticks ..= max_ticks]` (or `[min_ticks .. $]`) clock ticks:
    /// at offset `0`, returns `sig` combinationally; at offset `k >= 1`, shifts through `k` registers.
    fn delay_signal(&mut self, sig: Guard, min_ticks: u32, max_bound: CycleBound) -> Result<Guard, String> {
        if sig == Guard::False {
            return Ok(Guard::False);
        }
        match max_bound {
            CycleBound::Const(max_ticks) => {
                if max_ticks < min_ticks {
                    return Ok(Guard::False);
                }
                if max_ticks > crate::sva::MAX_PROPERTY_SPAN {
                    return Err(too_long());
                }
                let mut out = if min_ticks == 0 { sig } else { Guard::False };
                let mut cur = sig;
                for step in 1..=max_ticks {
                    let (_, q) = self.alloc_reg(cur)?;
                    cur = q;
                    if step >= min_ticks {
                        out = self.pool.or(out, cur)?;
                    }
                }
                Ok(out)
            }
            CycleBound::Unbounded => {
                if min_ticks > crate::sva::MAX_PROPERTY_SPAN {
                    return Err(too_long());
                }
                let mut cur = sig;
                for _ in 0..min_ticks {
                    let (_, q) = self.alloc_reg(cur)?;
                    cur = q;
                }
                // Sticky wait register: holds `1` on every subsequent tick after `cur` arrives.
                let (hold_idx, hold_q) = self.alloc_reg(Guard::False)?;
                let active = self.pool.or(cur, hold_q)?;
                let hold_next = self.apply_throughout(active)?;
                self.state_next[hold_idx] = hold_next;
                Ok(active)
            }
        }
    }

    fn build(&mut self, s: &SeqExpr, inject: Guard, min_off: u32, allow_first_match: bool) -> Result<Frag, String> {
        match s {
            SeqExpr::Bool(e) => {
                let b = self.pos_leaf(*e, min_off)?;
                let raw = self.pool.and(inject, b)?;
                let end_now = self.apply_throughout(raw)?;
                Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: Some(1) })
            }
            SeqExpr::Fusion(a, b) => {
                let fa = self.build(a, inject, min_off, allow_first_match)?;
                let Some(la) = fa.min_nonempty_len else {
                    return Ok(Frag { accepts_empty: false, end_now: Guard::False, min_nonempty_len: None });
                };
                // `empty ##0 s` and `s ##0 empty` never match: `b` starts on the shared last tick `la - 1` of `a`,
                // and only `b`'s non-empty matches (`end_now`) count!
                let fb = self.build(b, fa.end_now, min_off.saturating_add(la.saturating_sub(1)), allow_first_match)?;
                let min_len = fb.min_nonempty_len.map(|lb| la.saturating_add(lb).saturating_sub(1));
                Ok(Frag { accepts_empty: false, end_now: fb.end_now, min_nonempty_len: min_len })
            }
            SeqExpr::Delay { range, lhs: None, rhs } => {
                bounds_checked(range, "the delay")?;
                let delayed_inj = self.delay_signal(inject, range.min, range.max)?;
                let fr = self.build(rhs, delayed_inj, min_off.saturating_add(range.min), allow_first_match)?;
                let accepts_empty = range.min == 0 && fr.accepts_empty;
                // If `rhs` accepts empty and `range` includes `n >= 1`, `##n empty` ends on tick `from + n - 1`!
                let mut end_now = fr.end_now;
                let mut min_len = fr.min_nonempty_len.map(|lr| range.min.saturating_add(lr));
                if fr.accepts_empty {
                    let empty_tail = match range.max {
                        CycleBound::Const(0) => Guard::False,
                        CycleBound::Const(mx) => {
                            let lo = range.min.max(1) - 1;
                            self.delay_signal(inject, lo, CycleBound::Const(mx - 1))?
                        }
                        CycleBound::Unbounded => {
                            let lo = range.min.max(1) - 1;
                            self.delay_signal(inject, lo, CycleBound::Unbounded)?
                        }
                    };
                    if empty_tail != Guard::False {
                        end_now = self.pool.or(end_now, empty_tail)?;
                        let empty_len = range.min.max(1);
                        min_len = Some(min_len.map_or(empty_len, |m| m.min(empty_len)));
                    }
                }
                Ok(Frag { accepts_empty, end_now, min_nonempty_len: min_len })
            }
            SeqExpr::Delay { range, lhs: Some(lhs), rhs } => {
                bounds_checked(range, "the delay")?;
                let fl = self.build(lhs, inject, min_off, allow_first_match)?;
                let mut end_now = Guard::False;
                let mut min_len: Option<u32> = None;

                // 1. `n == 0` fusion arm (`lhs ##0 rhs`), when `range.min == 0`.
                if range.min == 0 && fl.min_nonempty_len.is_some() {
                    let la = fl.min_nonempty_len.unwrap_or(1);
                    let fb0 =
                        self.build(rhs, fl.end_now, min_off.saturating_add(la.saturating_sub(1)), allow_first_match)?;
                    end_now = self.pool.or(end_now, fb0.end_now)?;
                    if let Some(lb) = fb0.min_nonempty_len {
                        let l0 = la.saturating_add(lb).saturating_sub(1);
                        min_len = Some(min_len.map_or(l0, |m| m.min(l0)));
                    }
                }

                // 2. `n >= 1` concatenation arms (`end + n - 1`).
                let has_pos_delay = match range.max {
                    CycleBound::Const(mx) => mx >= 1,
                    CycleBound::Unbounded => true,
                };
                let mut accepts_empty = false;
                if has_pos_delay {
                    let lo_n = range.min.max(1);
                    let shift_max = match range.max {
                        CycleBound::Const(mx) => CycleBound::Const(mx - 1),
                        CycleBound::Unbounded => CycleBound::Unbounded,
                    };
                    // `e_pulse` fires at tick `end` (`t` for empty `lhs`, `t_l + 1` via 1 reg for non-empty `lhs`).
                    let reg_l = if fl.end_now != Guard::False {
                        let (_, q) = self.alloc_reg(fl.end_now)?;
                        q
                    } else {
                        Guard::False
                    };
                    let e_pulse = if fl.accepts_empty { self.pool.or(inject, reg_l)? } else { reg_l };
                    let start_r = self.delay_signal(e_pulse, lo_n - 1, shift_max)?;
                    let base_l = if fl.accepts_empty { 0 } else { fl.min_nonempty_len.unwrap_or(1) };
                    let rhs_off = min_off.saturating_add(base_l).saturating_add(lo_n - 1);
                    let fr = self.build(rhs, start_r, rhs_off, allow_first_match)?;
                    end_now = self.pool.or(end_now, fr.end_now)?;
                    if let Some(lr) = fr.min_nonempty_len {
                        if fl.accepts_empty || fl.min_nonempty_len.is_some() {
                            let cand = base_l.saturating_add(lo_n - 1).saturating_add(lr);
                            min_len = Some(min_len.map_or(cand, |m| m.min(cand)));
                        }
                    }

                    // Handle `rhs.accepts_empty` (`e = end + n - 1`):
                    if fr.accepts_empty {
                        if fl.accepts_empty && lo_n == 1 {
                            accepts_empty = true;
                        }
                        // Non-empty `lhs` + empty `rhs`: last tick is `t_l + (n - 1)`.
                        if fl.end_now != Guard::False {
                            let tail_ne = self.delay_signal(fl.end_now, lo_n - 1, shift_max)?;
                            end_now = self.pool.or(end_now, tail_ne)?;
                            if let Some(ll) = fl.min_nonempty_len {
                                let cand = ll.saturating_add(lo_n - 1);
                                min_len = Some(min_len.map_or(cand, |m| m.min(cand)));
                            }
                        }
                        // Empty `lhs` + empty `rhs` with `n >= 2`: last tick is `from + (n - 2)`.
                        if fl.accepts_empty {
                            let ge2 = match range.max {
                                CycleBound::Const(mx) => mx >= 2,
                                CycleBound::Unbounded => true,
                            };
                            if ge2 {
                                let lo2 = lo_n.max(2) - 2;
                                let max2 = match range.max {
                                    CycleBound::Const(mx) => CycleBound::Const(mx - 2),
                                    CycleBound::Unbounded => CycleBound::Unbounded,
                                };
                                let tail_e = self.delay_signal(inject, lo2, max2)?;
                                end_now = self.pool.or(end_now, tail_e)?;
                                let cand = lo_n.max(2) - 1;
                                min_len = Some(min_len.map_or(cand, |m| m.min(cand)));
                            }
                        }
                    }
                }
                Ok(Frag { accepts_empty, end_now, min_nonempty_len: min_len })
            }
            SeqExpr::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
                bounds_checked(range, "the repetition")?;
                if range.max == CycleBound::Const(0) {
                    return Ok(Frag { accepts_empty: true, end_now: Guard::False, min_nonempty_len: None });
                }
                let saved_loop = self.in_loop;
                self.in_loop = true;
                let probe = self.build_probe_nullable(seq)?;
                let eff_min = if probe { 1 } else { range.min.max(1) };
                let accepts_empty = range.min == 0 || probe;

                let res = match range.max {
                    CycleBound::Const(max_reps) => {
                        if max_reps > crate::sva::MAX_PROPERTY_SPAN {
                            self.in_loop = saved_loop;
                            return Err(too_long());
                        }
                        let mut end_now = Guard::False;
                        let mut min_len: Option<u32> = None;
                        let mut cur_inj = inject;
                        let mut cur_off = min_off;
                        for rep in 1..=max_reps {
                            let f = self.build(seq, cur_inj, cur_off, false)?;
                            let Some(step_l) = f.min_nonempty_len else {
                                break;
                            };
                            if rep >= eff_min {
                                end_now = self.pool.or(end_now, f.end_now)?;
                                let cand = step_l.saturating_mul(rep);
                                min_len = Some(min_len.map_or(cand, |m| m.min(cand)));
                            }
                            if rep < max_reps {
                                let (_, q) = self.alloc_reg(f.end_now)?;
                                cur_inj = q;
                                cur_off = cur_off.saturating_add(step_l);
                            }
                        }
                        Ok(Frag { accepts_empty, end_now, min_nonempty_len: min_len })
                    }
                    CycleBound::Unbounded => {
                        if eff_min > crate::sva::MAX_PROPERTY_SPAN {
                            self.in_loop = saved_loop;
                            return Err(too_long());
                        }
                        let mut cur_inj = inject;
                        let mut cur_off = min_off;
                        let mut total_min: u32 = 0;
                        // Build `eff_min - 1` mandatory non-empty prefix copies:
                        for _ in 1..eff_min {
                            let f = self.build(seq, cur_inj, cur_off, false)?;
                            let Some(step_l) = f.min_nonempty_len else {
                                self.in_loop = saved_loop;
                                return Ok(Frag { accepts_empty, end_now: Guard::False, min_nonempty_len: None });
                            };
                            total_min = total_min.saturating_add(step_l);
                            let (_, q) = self.alloc_reg(f.end_now)?;
                            cur_inj = q;
                            cur_off = cur_off.saturating_add(step_l);
                        }
                        // Build the repeating tail copy `seq_+` fed by `cur_inj | reg(tail.end_now)`.
                        // Because `tail.end_now` only matches non-empty runs (`>= 1` tick), feeding it
                        // back through `alloc_reg` has a 1-tick flip-flop boundary (no combinational loop!).
                        let (fb_idx, fb_q) = self.alloc_reg(Guard::False)?;
                        let loop_inj = self.pool.or(cur_inj, fb_q)?;
                        let f_tail = self.build(seq, loop_inj, cur_off, false)?;
                        let fb_next = self.apply_throughout(f_tail.end_now)?;
                        self.state_next[fb_idx] = fb_next;
                        let min_len = f_tail.min_nonempty_len.map(|l| total_min.saturating_add(l));
                        Ok(Frag { accepts_empty, end_now: f_tail.end_now, min_nonempty_len: min_len })
                    }
                };
                self.in_loop = saved_loop;
                res
            }
            SeqExpr::Repeat { kind: RepeatKind::Goto, range, seq } => {
                bounds_checked(range, "the goto repetition")?;
                if range.min == 0 {
                    return Err("goto repetition [->0] is illegal".into());
                }
                let SeqExpr::Bool(e) = seq.as_ref() else {
                    return Err("goto repetition [->] on a multi-cycle sequence".into());
                };
                let (b_pos, b_neg) = self.pos_and_neg_leaf(*e, min_off)?;
                match range.max {
                    CycleBound::Const(max_reps) => {
                        if max_reps > crate::sva::MAX_PROPERTY_SPAN {
                            return Err(too_long());
                        }
                        let mut end_now = Guard::False;
                        let mut cur_inj = inject;
                        for rep in 1..=max_reps {
                            let (wait_idx, wait_q) = self.alloc_reg(Guard::False)?;
                            let active = self.pool.or(cur_inj, wait_q)?;
                            let stay = self.pool.and(active, b_neg)?;
                            self.state_next[wait_idx] = self.apply_throughout(stay)?;
                            let hit_raw = self.pool.and(active, b_pos)?;
                            let hit = self.apply_throughout(hit_raw)?;
                            if rep >= range.min {
                                end_now = self.pool.or(end_now, hit)?;
                            }
                            if rep < max_reps {
                                let (_, next_q) = self.alloc_reg(hit)?;
                                cur_inj = next_q;
                            }
                        }
                        Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: Some(range.min) })
                    }
                    CycleBound::Unbounded => {
                        if range.min > crate::sva::MAX_PROPERTY_SPAN {
                            return Err(too_long());
                        }
                        let mut cur_inj = inject;
                        for _ in 1..range.min {
                            let (wait_idx, wait_q) = self.alloc_reg(Guard::False)?;
                            let active = self.pool.or(cur_inj, wait_q)?;
                            let stay = self.pool.and(active, b_neg)?;
                            self.state_next[wait_idx] = self.apply_throughout(stay)?;
                            let hit_raw = self.pool.and(active, b_pos)?;
                            let hit = self.apply_throughout(hit_raw)?;
                            let (_, next_q) = self.alloc_reg(hit)?;
                            cur_inj = next_q;
                        }
                        // On the `min`-th and all subsequent hits, `active` stays alive across both `b_neg` and `b_pos`
                        // (`(!b | b)` on 2-state, or `b_neg | b_pos` on 4-state so `'x` drops dead!).
                        let (tail_idx, tail_q) = self.alloc_reg(Guard::False)?;
                        let active = self.pool.or(cur_inj, tail_q)?;
                        let valid_bit = self.pool.or(b_pos, b_neg)?;
                        let keep = self.pool.and(active, valid_bit)?;
                        self.state_next[tail_idx] = self.apply_throughout(keep)?;
                        let hit_raw = self.pool.and(active, b_pos)?;
                        let end_now = self.apply_throughout(hit_raw)?;
                        Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: Some(range.min) })
                    }
                }
            }
            SeqExpr::Repeat { kind: RepeatKind::NonConsecutive, range, seq } => {
                bounds_checked(range, "the non-consecutive repetition")?;
                let SeqExpr::Bool(e) = seq.as_ref() else {
                    return Err("non-consecutive repetition [=] on a multi-cycle sequence".into());
                };
                let (b_pos, b_neg) = self.pos_and_neg_leaf(*e, min_off)?;
                let accepts_empty = range.min == 0;
                match range.max {
                    CycleBound::Const(max_reps) => {
                        if max_reps > crate::sva::MAX_PROPERTY_SPAN {
                            return Err(too_long());
                        }
                        let cnt = (max_reps as usize) + 1;
                        let mut regs = Vec::with_capacity(cnt);
                        for _ in 0..cnt {
                            regs.push(self.alloc_reg(Guard::False)?);
                        }
                        let mut pre = Vec::with_capacity(cnt);
                        pre.push(self.pool.or(inject, regs[0].1)?);
                        for &(_, q) in &regs[1..] {
                            pre.push(q);
                        }
                        let mut end_now = Guard::False;
                        for c in 0..cnt {
                            let stay = self.pool.and(pre[c], b_neg)?;
                            let post_raw = if c == 0 {
                                stay
                            } else {
                                let step = self.pool.and(pre[c - 1], b_pos)?;
                                self.pool.or(stay, step)?
                            };
                            let post = self.apply_throughout(post_raw)?;
                            self.state_next[regs[c].0] = post;
                            if (c as u32) >= range.min {
                                end_now = self.pool.or(end_now, post)?;
                            }
                        }
                        let min_len = Some(range.min.max(1));
                        Ok(Frag { accepts_empty, end_now, min_nonempty_len: min_len })
                    }
                    CycleBound::Unbounded => {
                        if range.min > crate::sva::MAX_PROPERTY_SPAN {
                            return Err(too_long());
                        }
                        if range.min == 0 {
                            // `b [= 0:$]` matches empty and every tick where `b` is a valid boolean (`b_pos | b_neg`).
                            let (idx, q) = self.alloc_reg(Guard::False)?;
                            let active = self.pool.or(inject, q)?;
                            let valid_bit = self.pool.or(b_pos, b_neg)?;
                            let step = self.pool.and(active, valid_bit)?;
                            let post = self.apply_throughout(step)?;
                            self.state_next[idx] = post;
                            return Ok(Frag { accepts_empty: true, end_now: post, min_nonempty_len: Some(1) });
                        }
                        // `b [= min:$]` (`min >= 1`): states `0 .. min` where state `min` absorbs `>= min` occurrences!
                        let cnt = (range.min as usize) + 1;
                        let mut regs = Vec::with_capacity(cnt);
                        for _ in 0..cnt {
                            regs.push(self.alloc_reg(Guard::False)?);
                        }
                        let mut pre = Vec::with_capacity(cnt);
                        pre.push(self.pool.or(inject, regs[0].1)?);
                        for &(_, q) in &regs[1..] {
                            pre.push(q);
                        }
                        let valid_bit = self.pool.or(b_pos, b_neg)?;
                        for c in 0..(cnt - 1) {
                            let stay = self.pool.and(pre[c], b_neg)?;
                            let post_raw = if c == 0 {
                                stay
                            } else {
                                let step = self.pool.and(pre[c - 1], b_pos)?;
                                self.pool.or(stay, step)?
                            };
                            self.state_next[regs[c].0] = self.apply_throughout(post_raw)?;
                        }
                        let top = cnt - 1;
                        let stay_top = self.pool.and(pre[top], valid_bit)?;
                        let enter_top = self.pool.and(pre[top - 1], b_pos)?;
                        let merged_top = self.pool.or(stay_top, enter_top)?;
                        let post_top = self.apply_throughout(merged_top)?;
                        self.state_next[regs[top].0] = post_top;
                        Ok(Frag { accepts_empty: false, end_now: post_top, min_nonempty_len: Some(range.min) })
                    }
                }
            }
            SeqExpr::Or(a, b) => {
                let fa = self.build(a, inject, min_off, allow_first_match)?;
                let fb = self.build(b, inject, min_off, allow_first_match)?;
                let end_now = self.pool.or(fa.end_now, fb.end_now)?;
                let min_len = match (fa.min_nonempty_len, fb.min_nonempty_len) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                };
                Ok(Frag { accepts_empty: fa.accepts_empty || fb.accepts_empty, end_now, min_nonempty_len: min_len })
            }
            SeqExpr::Throughout { cond, seq } => {
                let c_guard = self.pos_leaf(*cond, min_off)?;
                self.throughout_stack.push(c_guard);
                let f = self.build(seq, inject, min_off, allow_first_match);
                self.throughout_stack.pop();
                // IEEE 1800-2023 Annex F: `b throughout empty` holds vacuously (`empty` covers 0 ticks).
                f
            }
            SeqExpr::FirstMatch(inner) => {
                if !allow_first_match || self.in_loop {
                    return Err("`first_match` over an unbounded antecedent or loop".into());
                }
                let start_idx = self.state_next.len();
                let f = self.build(inner, inject, min_off, true)?;
                if f.accepts_empty {
                    // Empty match is strictly earlier than any non-empty match, so non-empty matches are suppressed!
                    for st in &mut self.state_next[start_idx..] {
                        *st = Guard::False;
                    }
                    return Ok(Frag { accepts_empty: true, end_now: Guard::False, min_nonempty_len: None });
                }
                let not_matched = self.pool.not(f.end_now)?;
                for st in &mut self.state_next[start_idx..] {
                    let cur = *st;
                    *st = self.pool.and(cur, not_matched)?;
                }
                Ok(f)
            }
            SeqExpr::And(..) => Err("sequence `and` with an unbounded operand".into()),
            SeqExpr::Intersect(..) => Err("sequence `intersect` with an unbounded operand".into()),
            SeqExpr::Within(..) => Err("sequence `within` with an unbounded operand".into()),
            SeqExpr::Clocked { .. } => Err("a sequence with its own clock".into()),
        }
    }

    fn build_probe_nullable(&self, s: &SeqExpr) -> Result<bool, String> {
        Ok(match s {
            SeqExpr::Bool(_) | SeqExpr::Fusion(..) => false,
            SeqExpr::Delay { range, lhs: None, rhs } => range.min == 0 && self.build_probe_nullable(rhs)?,
            SeqExpr::Delay { range, lhs: Some(l), rhs: r } => {
                let has_1 = match range.max {
                    CycleBound::Const(mx) => range.min <= 1 && mx >= 1,
                    CycleBound::Unbounded => range.min <= 1,
                };
                has_1 && self.build_probe_nullable(l)? && self.build_probe_nullable(r)?
            }
            SeqExpr::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
                range.min == 0 || self.build_probe_nullable(seq)?
            }
            SeqExpr::Repeat { kind: RepeatKind::Goto, .. } => false,
            SeqExpr::Repeat { kind: RepeatKind::NonConsecutive, range, .. } => range.min == 0,
            SeqExpr::Or(a, b) => self.build_probe_nullable(a)? || self.build_probe_nullable(b)?,
            SeqExpr::Throughout { seq, .. } | SeqExpr::FirstMatch(seq) => self.build_probe_nullable(seq)?,
            SeqExpr::And(a, b) | SeqExpr::Intersect(a, b) => {
                self.build_probe_nullable(a)? && self.build_probe_nullable(b)?
            }
            SeqExpr::Within(a, b) => self.build_probe_nullable(a)? && self.build_probe_nullable(b)?,
            SeqExpr::Clocked { .. } => return Err("a sequence with its own clock".into()),
        })
    }
}

/// Unbounded sequence extraction for consequents (extends [`as_seq`] with unbounded `eventually` / `s_eventually`).
fn as_consequent_seq(p: &PropExpr) -> Result<(SeqExpr, bool), String> {
    let delayed = |range: CycleRange, s: SeqExpr| SeqExpr::Delay { range, lhs: None, rhs: Box::new(s) };
    match p {
        PropExpr::Seq(s) | PropExpr::Weak(s) => Ok((s.clone(), false)),
        PropExpr::Strong(s) => Ok((s.clone(), true)),
        PropExpr::Nexttime { strong, cycles, prop } => {
            let (inner, inner_strong) = as_consequent_seq(prop)?;
            Ok((delayed(CycleRange::exact(*cycles), inner), *strong || inner_strong))
        }
        PropExpr::Eventually { strong, range, prop } => {
            if !*strong && range.max == CycleBound::Unbounded {
                return Err(prop_name(p).into());
            }
            let (inner, inner_strong) = as_consequent_seq(prop)?;
            Ok((delayed(*range, inner), *strong || inner_strong))
        }
        PropExpr::Always { range, prop, strong } => {
            let CycleBound::Const(last) = range.max else {
                return Err("`always` with no upper bound".into());
            };
            if last > crate::sva::MAX_PROPERTY_SPAN || last < range.min {
                return Err(too_long());
            }
            let (inner, inner_strong) = as_consequent_seq(prop)?;
            if range.min == last {
                return Ok((delayed(CycleRange::exact(range.min), inner), *strong || inner_strong));
            }
            // Fallback to `as_seq` when `always [m:n]` is bounded.
            Ok((as_seq(p)?.into_owned(), false))
        }
        other => Err(prop_name(other).into()),
    }
}

/// Compiled antecedent trigger specification in an [`AutomatonPlan`].
#[derive(Clone)]
pub(crate) enum AntecedentPlan {
    /// Property has no antecedent: starts at every warmed-up tick (`reached`).
    Always { reached: Option<ValueId> },
    /// Bounded antecedent compiled by `Matcher` (`starts: Vec<(at, Cond, first_tick)>`, `terms`).
    Bounded { terms: Vec<Term>, two_state: bool, starts: Vec<(u32, Cond, u32)>, non_overlapped: bool },
    /// Existential `SeqNfa` antecedent (`[->]`, `[=]`, `##[m:$]`, `[*m:$]`).
    Nfa {
        pool: GuardPool,
        leaf_tab: LeafTable,
        state_next: Vec<Guard>,
        accepts_empty: bool,
        end_now: Guard,
        reached: Option<ValueId>,
        non_overlapped: bool,
        warm_start: u32,
    },
}

/// Deterministic subset automaton compiled for a universal (`assert`/`assume`) consequent.
#[derive(Clone)]
pub(crate) struct ConsequentDfa {
    pub(crate) pool: GuardPool,
    pub(crate) leaf_tab: LeafTable,
    pub(crate) num_states: usize,
    pub(crate) init_dead: Guard,
    pub(crate) init_to: Vec<Guard>,
    pub(crate) state_dead: Vec<Guard>,
    pub(crate) state_to: Vec<Vec<Guard>>,
    pub(crate) has_dead: bool,
    pub(crate) strong: bool,
}

#[derive(Clone)]
pub(crate) enum ConsequentPlan {
    /// Existential NFA monitor for `PropKind::Cover` (including `:precondition` and `:witness` covers).
    CoverNfa {
        pool: GuardPool,
        leaf_tab: LeafTable,
        state_next: Vec<Guard>,
        hit_now: Guard,
        reached: Option<ValueId>,
        warm_start: u32,
    },
    /// Precondition cover of `AntecedentPlan` (`start_now & !disable_now`).
    PreconditionCover,
    /// `b until c` / `s_eventually c` after an NFA antecedent.
    Until { hold: Option<ValueId>, release: ValueId, with: bool, strong: bool },
    /// Subset-constructed universal safety (+ optional strong liveness) DFA.
    Dfa(ConsequentDfa),
}

#[derive(Clone)]
pub(crate) struct AutomatonPlan {
    pub(crate) antecedent: AntecedentPlan,
    pub(crate) consequent: ConsequentPlan,
    /// Full property expression + leaves saved so `related_cover(..., witness = true)` can build a cover NFA.
    pub(crate) witness_cover: Option<Box<ConsequentPlan>>,
}

/// Builds a compact ROBDD `Guard` from a truth table over `vars` (`tt.len() == 1 << vars.len()`),
/// treating `None` entries in `tt` as don't-cares!
fn robdd_from_table(
    pool: &mut GuardPool,
    vars: &[Guard],
    tt: &[Option<bool>],
    memo: &mut HashMap<(usize, Vec<u8>), Guard>,
) -> Result<Guard, String> {
    let mut has_true = false;
    let mut has_false = false;
    for &v in tt {
        match v {
            Some(true) => has_true = true,
            Some(false) => has_false = true,
            None => {}
        }
    }
    if !has_true {
        return Ok(Guard::False);
    }
    if !has_false {
        return Ok(Guard::True);
    }
    let key_bytes: Vec<u8> = tt
        .iter()
        .map(|x| match x {
            None => 2,
            Some(false) => 0,
            Some(true) => 1,
        })
        .collect();
    if let Some(&cached) = memo.get(&(vars.len(), key_bytes.clone())) {
        return Ok(cached);
    }
    let half = tt.len() / 2;
    // Bit `0` of assignment index corresponds to `vars[0]`, or MSB?
    // Let `vars[0]` be the MSB of the slice (`idx >= half` means `vars[0] == true`).
    let lo_slice = &tt[..half];
    let hi_slice = &tt[half..];
    let lo_g = robdd_from_table(pool, &vars[1..], lo_slice, memo)?;
    let hi_g = robdd_from_table(pool, &vars[1..], hi_slice, memo)?;
    let res = if lo_g == hi_g {
        lo_g
    } else {
        let v = vars[0];
        let nv = pool.not(v)?;
        let hi_term = pool.and(v, hi_g)?;
        let lo_term = pool.and(nv, lo_g)?;
        pool.or(hi_term, lo_term)?
    };
    memo.insert((vars.len(), key_bytes), res);
    Ok(res)
}

fn compile_consequent_dfa(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    rhs_seq: &SeqExpr,
    strong: bool,
    ant_min_len: u32,
) -> Result<(ConsequentDfa, u32), String> {
    let mut b = SeqNfaBuilder::new(nl, leaves, MAX_CONSEQUENT_NFA_STATES);
    let inj = b.pool.var(GVar::Inject)?;
    let frag = b.build(rhs_seq, inj, ant_min_len, true)?;
    if frag.accepts_empty {
        return Err("a sequence that admits an empty match, used as a property".into());
    }
    if frag.min_nonempty_len.is_none() || frag.end_now == Guard::False {
        return Err("a sequence that has no match, used as a property".into());
    }

    // Build variable list for ROBDD / truth-table enumeration:
    // Each leaf `k` contributes `PosLeaf(k)` and, if 4-state, `NegLeaf(k)`.
    let mut dfa_pool = GuardPool::default();
    let mut var_guards: Vec<Guard> = Vec::new();
    let mut var_roles: Vec<(u16, bool)> = Vec::new(); // (leaf_idx, is_neg)
    for (k, &(_, is_four_neg)) in b.leaf_tab.exprs.iter().enumerate() {
        let k16 = k as u16;
        var_guards.push(dfa_pool.var(GVar::PosLeaf(k16))?);
        var_roles.push((k16, false));
        if is_four_neg {
            var_guards.push(dfa_pool.var(GVar::NegLeaf(k16))?);
            var_roles.push((k16, true));
        }
    }
    let nvars = var_guards.len();
    if nvars > MAX_DFA_VARS {
        return Err(too_large());
    }
    let nassign = 1usize << nvars;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum OutTrans {
        DontCare,
        Matched,
        Dead,
        Next(usize),
    }

    // Decode assignment index `a` (where `var_guards[0]` is MSB `nvars - 1`):
    let decode = |a: usize| -> Option<(u16, u16)> {
        let mut pos_bits = 0u16;
        let mut neg_bits = 0u16;
        for (vi, &(leaf_k, is_neg)) in var_roles.iter().enumerate() {
            let bit = ((a >> (nvars - 1 - vi)) & 1) as u16;
            if is_neg {
                neg_bits |= bit << leaf_k;
            } else {
                pos_bits |= bit << leaf_k;
            }
        }
        // For 2-state leaves (`!is_four_neg`), `neg` is `!pos` (handled inside `b.pool` via `Not`).
        // For 4-state leaves (`is_four_neg`), `(pos=1, neg=1)` is impossible in hardware!
        for (k, &(_, is_four_neg)) in b.leaf_tab.exprs.iter().enumerate() {
            if is_four_neg && ((pos_bits >> k) & 1) != 0 && ((neg_bits >> k) & 1) != 0 {
                return None;
            }
        }
        Some((pos_bits, neg_bits))
    };

    let eval_step = |inject: bool, mask: u64, pos_bits: u16, neg_bits: u16| -> (bool, u64) {
        let matched = b.pool.eval(frag.end_now, inject, mask, pos_bits, neg_bits);
        if matched {
            return (true, 0);
        }
        let mut next_mask = 0u64;
        for (si, &sn) in b.state_next.iter().enumerate() {
            if b.pool.eval(sn, inject, mask, pos_bits, neg_bits) {
                next_mask |= 1u64 << si;
            }
        }
        (false, next_mask)
    };

    let mut mask_to_dfa: HashMap<u64, usize> = HashMap::new();
    let mut dfa_masks: Vec<u64> = Vec::new();
    let mut queue: VecDeque<usize> = VecDeque::new();

    let intern_mask =
        |m: u64, map: &mut HashMap<u64, usize>, vec: &mut Vec<u64>, q: &mut VecDeque<usize>| -> Result<usize, String> {
            if let Some(&id) = map.get(&m) {
                return Ok(id);
            }
            if vec.len() >= MAX_DFA_STATES {
                return Err(too_large());
            }
            let id = vec.len();
            vec.push(m);
            map.insert(m, id);
            q.push_back(id);
            Ok(id)
        };

    let mut init_trans = vec![OutTrans::DontCare; nassign];
    for (a, slot) in init_trans.iter_mut().enumerate() {
        let Some((pos_b, neg_b)) = decode(a) else { continue };
        let (matched, next_m) = eval_step(true, 0, pos_b, neg_b);
        *slot = if matched {
            OutTrans::Matched
        } else if next_m == 0 {
            OutTrans::Dead
        } else {
            OutTrans::Next(intern_mask(next_m, &mut mask_to_dfa, &mut dfa_masks, &mut queue)?)
        };
    }

    let mut state_trans: Vec<Vec<OutTrans>> = Vec::new();
    while let Some(d_id) = queue.pop_front() {
        let mask = dfa_masks[d_id];
        let mut row = vec![OutTrans::DontCare; nassign];
        for (a, slot) in row.iter_mut().enumerate() {
            let Some((pos_b, neg_b)) = decode(a) else { continue };
            let (matched, next_m) = eval_step(false, mask, pos_b, neg_b);
            *slot = if matched {
                OutTrans::Matched
            } else if next_m == 0 {
                OutTrans::Dead
            } else {
                OutTrans::Next(intern_mask(next_m, &mut mask_to_dfa, &mut dfa_masks, &mut queue)?)
            };
        }
        if d_id >= state_trans.len() {
            state_trans.resize(d_id + 1, Vec::new());
        }
        state_trans[d_id] = row;
    }

    let num_states = dfa_masks.len();
    let mut memo = HashMap::new();
    let mk_pred = |pool: &mut GuardPool,
                   memo: &mut HashMap<(usize, Vec<u8>), Guard>,
                   row: &[OutTrans],
                   target: OutTrans|
     -> Result<Guard, String> {
        let tt: Vec<Option<bool>> =
            row.iter().map(|&t| if t == OutTrans::DontCare { None } else { Some(t == target) }).collect();
        robdd_from_table(pool, &var_guards, &tt, memo)
    };

    let init_dead = mk_pred(&mut dfa_pool, &mut memo, &init_trans, OutTrans::Dead)?;
    let mut init_to = Vec::with_capacity(num_states);
    for d in 0..num_states {
        init_to.push(mk_pred(&mut dfa_pool, &mut memo, &init_trans, OutTrans::Next(d))?);
    }
    let mut state_dead = Vec::with_capacity(num_states);
    let mut state_to = Vec::with_capacity(num_states);
    for row in &state_trans {
        state_dead.push(mk_pred(&mut dfa_pool, &mut memo, row, OutTrans::Dead)?);
        let mut dsts = Vec::with_capacity(num_states);
        for d2 in 0..num_states {
            dsts.push(mk_pred(&mut dfa_pool, &mut memo, row, OutTrans::Next(d2))?);
        }
        state_to.push(dsts);
    }
    let has_dead = init_dead != Guard::False || state_dead.iter().any(|&g| g != Guard::False);

    Ok((
        ConsequentDfa {
            pool: dfa_pool,
            leaf_tab: b.leaf_tab,
            num_states,
            init_dead,
            init_to,
            state_dead,
            state_to,
            has_dead,
            strong,
        },
        frag.min_nonempty_len.unwrap_or(1),
    ))
}

fn compute_warm_start(nl: &Netlist, tab: &LeafTable, skip: u32, memo: &mut Lookback) -> u32 {
    let mut warm = skip;
    for (&(val, _), &min_off) in tab.exprs.iter().zip(&tab.min_offset) {
        let lb = nl.lookback(val, memo);
        warm = warm.max(lb.saturating_sub(min_off));
    }
    warm
}

fn build_cover_consequent(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    prop_expr: &PropExpr,
    reached: Option<ValueId>,
    skip: u32,
    memo: &mut Lookback,
) -> Result<(ConsequentPlan, u32), String> {
    let full_seq = match prop_expr {
        PropExpr::OverlappedImplication { lhs, rhs } => {
            let (rseq, _) = as_consequent_seq(rhs)?;
            SeqExpr::Fusion(Box::new(lhs.clone()), Box::new(rseq))
        }
        PropExpr::NonOverlappedImplication { lhs, rhs } => {
            let (rseq, _) = as_consequent_seq(rhs)?;
            SeqExpr::Delay { range: CycleRange::exact(1), lhs: Some(Box::new(lhs.clone())), rhs: Box::new(rseq) }
        }
        other => {
            let (rseq, _) = as_consequent_seq(other)?;
            rseq
        }
    };
    // Verify consequent legality per IEEE 1800-2023 §16.12.22:
    match prop_expr {
        PropExpr::OverlappedImplication { lhs, rhs } | PropExpr::NonOverlappedImplication { lhs, rhs } => {
            let overlapped = matches!(prop_expr, PropExpr::OverlappedImplication { .. });
            let mut lb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let linj = lb.pool.var(GVar::Inject)?;
            let lf = lb.build(lhs, linj, 0, false)?;
            let ant_has_match = if overlapped {
                lf.min_nonempty_len.is_some() && lf.end_now != Guard::False
            } else {
                lf.accepts_empty || (lf.min_nonempty_len.is_some() && lf.end_now != Guard::False)
            };
            if !ant_has_match {
                return Err("an antecedent that has no match".into());
            }
            let (rseq, _) = as_consequent_seq(rhs)?;
            let mut rb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let rinj = rb.pool.var(GVar::Inject)?;
            let rf = rb.build(&rseq, rinj, 0, false)?;
            if rf.accepts_empty {
                return Err("a sequence that admits an empty match, used as a property".into());
            }
            if rf.min_nonempty_len.is_none() || rf.end_now == Guard::False {
                return Err("a sequence that has no match, used as a property".into());
            }
        }
        _ => {
            let mut rb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let rinj = rb.pool.var(GVar::Inject)?;
            let rf = rb.build(&full_seq, rinj, 0, false)?;
            if rf.accepts_empty {
                return Err("a sequence that admits an empty match, used as a property".into());
            }
            if rf.min_nonempty_len.is_none() || rf.end_now == Guard::False {
                return Err("a sequence that has no match, used as a property".into());
            }
        }
    }

    let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
    let inj = b.pool.var(GVar::Inject)?;
    let frag = b.build(&full_seq, inj, 0, false)?;
    let mut warm_start = compute_warm_start(nl, &b.leaf_tab, skip, memo);
    if let Some(rv) = reached {
        warm_start = warm_start.max(nl.lookback(rv, memo));
    }
    Ok((
        ConsequentPlan::CoverNfa {
            pool: b.pool,
            leaf_tab: b.leaf_tab,
            state_next: b.state_next,
            hit_now: frag.end_now,
            reached,
            warm_start,
        },
        warm_start,
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_automaton(
    nl: &Netlist,
    prop: &BoundProperty,
    leaves: &[Option<ValueId>],
    reached: Option<ValueId>,
    clock: ClockSpec,
    disable: Option<ValueId>,
    opts: &PropertyOptions,
    memo: &mut Lookback,
) -> Result<Plan, String> {
    let skip = nl.skip_for(clock, opts);
    let disable_lb = disable.map_or(0, |d| nl.lookback(d, memo));
    let eff_skip = skip.max(disable_lb);

    if prop.kind == PropKind::Cover {
        if matches!(
            prop.expr,
            PropExpr::Until { .. } | PropExpr::OverlappedImplication { .. } | PropExpr::NonOverlappedImplication { .. }
        ) {
            if let PropExpr::Until { .. } = prop.expr {
                return Err("a cover of `until`".into());
            }
            if let PropExpr::OverlappedImplication { rhs, .. } | PropExpr::NonOverlappedImplication { rhs, .. } =
                &prop.expr
            {
                if matches!(**rhs, PropExpr::Until { .. }) {
                    return Err("a cover of `until`".into());
                }
                if matches!(**rhs, PropExpr::Eventually { strong: true, .. }) {
                    return Err("a cover of `s_eventually`".into());
                }
            }
        }
        if matches!(prop.expr, PropExpr::Eventually { strong: true, .. }) {
            return Err("a cover of `s_eventually`".into());
        }
        let (cover_plan, warm) = build_cover_consequent(nl, leaves, &prop.expr, reached, eff_skip, memo)?;
        return Ok(Plan {
            kind: PropKind::Cover,
            loc: prop.loc,
            clock,
            terms: Vec::new(),
            live: Vec::new(),
            two_state: false,
            enable: Cond::True,
            expr: Cond::True,
            disable,
            span: 0,
            warm,
            until: None,
            automaton: Some(AutomatonPlan {
                antecedent: AntecedentPlan::Always { reached },
                consequent: cover_plan,
                witness_cover: None,
            }),
        });
    }

    // `Assert`, `Assume`, or `Restrict`:
    let (ant_opt, rhs_expr) = match &prop.expr {
        PropExpr::OverlappedImplication { lhs, rhs } => (Some((lhs, true)), rhs.as_ref()),
        PropExpr::NonOverlappedImplication { lhs, rhs } => (Some((lhs, false)), rhs.as_ref()),
        other => (None, other),
    };

    let (antecedent, ant_min_ticks, mut warm_max) = match ant_opt {
        None => {
            let w = eff_skip.max(reached.map_or(0, |r| nl.lookback(r, memo)));
            (AntecedentPlan::Always { reached }, 0u32, w)
        }
        Some((lhs, overlapped)) => {
            if !seq_needs_automaton(lhs) {
                // Bounded antecedent: reuse `Matcher` so `first_match`, `intersect`, `and`, `within` work!
                let mut m = Matcher::new(leaves);
                let r_cond = match reached {
                    Some(v) => m.terms.leaf(0, v)?,
                    None => Cond::True,
                };
                let mut starts_map: BTreeMap<u32, Cond> = BTreeMap::new();
                for (end, cond) in m.matches(lhs, 0)? {
                    let at = match (overlapped, end) {
                        (true, 0) => continue,
                        (true, _) => end - 1,
                        (false, _) => end,
                    };
                    let c = m.terms.and(r_cond, cond)?;
                    m.add(&mut starts_map, at, c)?;
                }
                if starts_map.is_empty() {
                    return Err("an antecedent that has no match".into());
                }
                let terms = m.terms;
                let mut planned = Vec::new();
                let mut w_max = eff_skip;
                let min_at = *starts_map.keys().next().unwrap_or(&0);
                for (at, when) in starts_map {
                    if at > crate::sva::MAX_PROPERTY_SPAN {
                        return Err(too_long());
                    }
                    let mut hist = if disable.is_some() { at.saturating_add(disable_lb) } else { 0 };
                    for (i, used) in live_terms(&terms.nodes, when).into_iter().enumerate() {
                        if let (true, &Term::Leaf { at: tick, value }) = (used, &terms.nodes[i]) {
                            hist = hist.max((at - tick).saturating_add(nl.lookback(value, memo)));
                        }
                    }
                    let first = eff_skip.saturating_add(at).max(hist);
                    w_max = w_max.max(first);
                    planned.push((at, when, first));
                }
                (
                    AntecedentPlan::Bounded {
                        terms: terms.nodes,
                        two_state: terms.negated,
                        starts: planned,
                        non_overlapped: false, // `at` already accounts for `!overlapped`!
                    },
                    min_at,
                    w_max,
                )
            } else {
                let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
                let inj = b.pool.var(GVar::Inject)?;
                let frag = b.build(lhs, inj, 0, false)?;
                let ant_has_match = if overlapped {
                    frag.min_nonempty_len.is_some() && frag.end_now != Guard::False
                } else {
                    frag.accepts_empty || (frag.min_nonempty_len.is_some() && frag.end_now != Guard::False)
                };
                if !ant_has_match {
                    return Err("an antecedent that has no match".into());
                }
                let mut w = compute_warm_start(nl, &b.leaf_tab, eff_skip, memo);
                if let Some(rv) = reached {
                    w = w.max(nl.lookback(rv, memo));
                }
                let min_ticks = if overlapped {
                    frag.min_nonempty_len.unwrap_or(1).saturating_sub(1)
                } else if frag.accepts_empty {
                    0
                } else {
                    frag.min_nonempty_len.unwrap_or(1)
                };
                (
                    AntecedentPlan::Nfa {
                        pool: b.pool,
                        leaf_tab: b.leaf_tab,
                        state_next: b.state_next,
                        accepts_empty: !overlapped && frag.accepts_empty,
                        end_now: frag.end_now,
                        reached,
                        non_overlapped: !overlapped,
                        warm_start: w,
                    },
                    min_ticks,
                    w,
                )
            }
        }
    };

    // Check if consequent is `Until` (`b until c`, `s_eventually bool` after unbounded NFA antecedent):
    if let PropExpr::Until { strong, overlapping, lhs, rhs } = rhs_expr {
        let not_lowered = || "an expression that was not lowered".to_string();
        let leaf = |e: ExprId| leaves.get(e.0 as usize).copied().flatten();
        let boolean = |p: &PropExpr| match as_seq(p).as_deref() {
            Ok(SeqExpr::Bool(e)) => Ok(*e),
            _ => Err("`until` between properties that are not single booleans".to_string()),
        };
        let hold = leaf(boolean(lhs)?).ok_or_else(not_lowered)?;
        let release = leaf(boolean(rhs)?).ok_or_else(not_lowered)?;
        let rhs_lb = nl.lookback(hold, memo).max(nl.lookback(release, memo)).saturating_sub(ant_min_ticks);
        warm_max = warm_max.max(rhs_lb);
        let mut ant = antecedent;
        if let AntecedentPlan::Nfa { warm_start, .. } = &mut ant {
            *warm_start = (*warm_start).max(rhs_lb);
        }
        return Ok(Plan {
            kind: prop.kind,
            loc: prop.loc,
            clock,
            terms: Vec::new(),
            live: Vec::new(),
            two_state: false,
            enable: Cond::True,
            expr: Cond::True,
            disable,
            span: 0,
            warm: warm_max,
            until: None,
            automaton: Some(AutomatonPlan {
                antecedent: ant,
                consequent: ConsequentPlan::Until { hold: Some(hold), release, with: *overlapping, strong: *strong },
                witness_cover: None,
            }),
        });
    }

    let (rhs_seq, strong) = as_consequent_seq(rhs_expr)?;
    let (dfa, _) = compile_consequent_dfa(nl, leaves, &rhs_seq, strong, ant_min_ticks)?;
    let rhs_warm = compute_warm_start(nl, &dfa.leaf_tab, eff_skip, memo);
    warm_max = warm_max.max(rhs_warm);
    let mut ant = antecedent;
    match &mut ant {
        AntecedentPlan::Bounded { starts, .. } => {
            for (_, _, first) in starts {
                *first = (*first).max(rhs_warm);
            }
        }
        AntecedentPlan::Nfa { warm_start, .. } => {
            *warm_start = (*warm_start).max(rhs_warm);
        }
        AntecedentPlan::Always { .. } => {}
    }

    let witness_cover = if prop.kind == PropKind::Assert && !strong && (ant_opt.is_some() || reached.is_some()) {
        build_cover_consequent(nl, leaves, &prop.expr, reached, eff_skip, memo).ok().map(|(cp, w)| {
            warm_max = warm_max.max(w);
            Box::new(cp)
        })
    } else {
        None
    };

    Ok(Plan {
        kind: prop.kind,
        loc: prop.loc,
        clock,
        terms: Vec::new(),
        live: Vec::new(),
        two_state: false,
        enable: Cond::True,
        expr: Cond::True,
        disable,
        span: 0,
        warm: warm_max,
        until: None,
        automaton: Some(AutomatonPlan { antecedent: ant, consequent: ConsequentPlan::Dfa(dfa), witness_cover }),
    })
}

pub(crate) fn related_automaton_cover(p: &Plan, witness: bool) -> Option<Plan> {
    if p.kind != PropKind::Assert {
        return None;
    }
    let auto = p.automaton.as_ref()?;
    let has_precond = match &auto.antecedent {
        AntecedentPlan::Always { reached } => reached.is_some(),
        AntecedentPlan::Bounded { .. } | AntecedentPlan::Nfa { .. } => true,
    };
    if !has_precond {
        return None;
    }
    match &auto.consequent {
        ConsequentPlan::Dfa(dfa) if !dfa.strong => {}
        _ => return None,
    }
    if witness {
        let cov = auto.witness_cover.as_ref()?.as_ref().clone();
        Some(Plan {
            kind: PropKind::Cover,
            loc: p.loc,
            clock: p.clock,
            terms: Vec::new(),
            live: Vec::new(),
            two_state: false,
            enable: Cond::True,
            expr: Cond::True,
            disable: p.disable,
            span: 0,
            warm: p.warm,
            until: None,
            automaton: Some(AutomatonPlan {
                antecedent: AntecedentPlan::Always { reached: None },
                consequent: cov,
                witness_cover: None,
            }),
        })
    } else {
        Some(Plan {
            kind: PropKind::Cover,
            loc: p.loc,
            clock: p.clock,
            terms: Vec::new(),
            live: Vec::new(),
            two_state: false,
            enable: Cond::True,
            expr: Cond::True,
            disable: p.disable,
            span: 0,
            warm: p.warm,
            until: None,
            automaton: Some(AutomatonPlan {
                antecedent: auto.antecedent.clone(),
                consequent: ConsequentPlan::PreconditionCover,
                witness_cover: None,
            }),
        })
    }
}

struct GuardLowerEnv<'a> {
    inject: ValueId,
    nfa_states: &'a [ValueId],
    pos_leaves: &'a [ValueId],
    neg_leaves: &'a [Option<ValueId>],
}

fn emit_guard(nl: &mut Netlist, pool: &GuardPool, root: Guard, env: &GuardLowerEnv<'_>, loc: Loc) -> ValueId {
    match root {
        Guard::False => nl.sva_const(1, 0, loc),
        Guard::True => nl.sva_const(1, 1, loc),
        Guard::Node(target) => {
            let mut live = vec![false; (target as usize) + 1];
            live[target as usize] = true;
            for i in (0..=(target as usize)).rev() {
                if !live[i] {
                    continue;
                }
                match pool.nodes[i] {
                    GNode::Var(_) => {}
                    GNode::Not(a) => live[a as usize] = true,
                    GNode::And(a, b) | GNode::Or(a, b) => {
                        if let Guard::Node(ja) = a {
                            live[ja as usize] = true;
                        }
                        if let Guard::Node(jb) = b {
                            live[jb as usize] = true;
                        }
                    }
                }
            }
            let mut made: Vec<Option<ValueId>> = vec![None; (target as usize) + 1];
            for i in 0..=(target as usize) {
                if !live[i] {
                    continue;
                }
                let val_of = |nl: &mut Netlist, made: &[Option<ValueId>], g: Guard| match g {
                    Guard::False => nl.sva_const(1, 0, loc),
                    Guard::True => nl.sva_const(1, 1, loc),
                    Guard::Node(j) => made[j as usize].expect("operand emitted before parent"),
                };
                made[i] = Some(match pool.nodes[i] {
                    GNode::Var(GVar::Inject) => env.inject,
                    GNode::Var(GVar::NfaState(s)) => env.nfa_states[s as usize],
                    GNode::Var(GVar::PosLeaf(k)) => env.pos_leaves[k as usize],
                    GNode::Var(GVar::NegLeaf(k)) => env.neg_leaves[k as usize].expect("4-state neg leaf prepared"),
                    GNode::Not(a) => {
                        let va = made[a as usize].expect("operand emitted");
                        nl.sva_op(NetOp::LogNot, vec![va], 1, loc)
                    }
                    GNode::And(a, b) => {
                        let va = val_of(nl, &made, a);
                        let vb = val_of(nl, &made, b);
                        nl.sva_op(NetOp::LogAnd, vec![va, vb], 1, loc)
                    }
                    GNode::Or(a, b) => {
                        let va = val_of(nl, &made, a);
                        let vb = val_of(nl, &made, b);
                        nl.sva_op(NetOp::LogOr, vec![va, vb], 1, loc)
                    }
                });
            }
            made[target as usize].expect("target emitted")
        }
    }
}

fn prepare_leaves(nl: &mut Netlist, tab: &LeafTable, loc: Loc) -> (Vec<ValueId>, Vec<Option<ValueId>>) {
    let mut pos = Vec::with_capacity(tab.exprs.len());
    let mut neg = Vec::with_capacity(tab.exprs.len());
    for &(v, needs_four_neg) in &tab.exprs {
        let b = nl.sva_bool(v, loc);
        pos.push(nl.sva_two_state(b, loc));
        if needs_four_neg {
            let nb = nl.sva_op(NetOp::LogNot, vec![b], 1, loc);
            neg.push(Some(nl.sva_two_state(nb, loc)));
        } else {
            neg.push(None);
        }
    }
    (pos, neg)
}

fn alloc_sva_bit_state(nl: &mut Netlist, name: String, clock: ClockSpec, loc: Loc) -> (StateId, ValueId) {
    let id = StateId(nl.states.len() as u32);
    let out = nl.sva_value(1, false, ValueKind::StateOut(id), loc);
    nl.states.push(State {
        id,
        name,
        width: 1,
        signed: false,
        four_state: false,
        clock: Some(clock),
        async_reset: None,
        latch_enable: None,
        init: Some(CVal::new(1, false, 0)),
        next: out,
        out,
        loc,
    });
    (id, out)
}

fn warm_gate(nl: &mut Netlist, warm: u32, ticks: Option<(ValueId, u32)>, loc: Loc) -> Option<ValueId> {
    if warm == 0 {
        return None;
    }
    let (counter, width) = ticks.expect("tick counter planned for warm > 0");
    let from = nl.sva_const(width, warm, loc);
    Some(nl.sva_op(NetOp::ULe, vec![from, counter], 1, loc))
}

fn emit_antecedent_start(
    nl: &mut Netlist,
    p: &Plan,
    ant: &AntecedentPlan,
    off_now: Option<(ValueId, ValueId)>,
    ticks: Option<(ValueId, u32)>,
    prefix: &str,
) -> ValueId {
    let loc = p.loc;
    match ant {
        AntecedentPlan::Always { reached } => {
            let mut g = warm_gate(nl, p.warm, ticks, loc);
            if let Some(rv) = *reached {
                let rv_b = nl.sva_bool(rv, loc);
                let rb = nl.sva_two_state(rv_b, loc);
                g = nl.sva_and(g, rb, loc);
            }
            if let Some((_, off)) = off_now {
                g = nl.sva_and(g, off, loc);
            }
            g.unwrap_or_else(|| nl.sva_const(1, 1, loc))
        }
        AntecedentPlan::Bounded { terms, two_state, starts, non_overlapped } => {
            let tmp_plan = Plan {
                kind: p.kind,
                loc,
                clock: p.clock,
                terms: terms.clone(),
                live: Vec::new(),
                two_state: *two_state,
                enable: Cond::True,
                expr: Cond::True,
                disable: p.disable,
                span: 0,
                warm: p.warm,
                until: None,
                automaton: None,
            };
            let mut start = None;
            for &(at, when, first) in starts {
                let mut here = nl.sva_cond_at(&tmp_plan, when, at, loc);
                if let Some(wg) = warm_gate(nl, first, ticks, loc) {
                    here = nl.sva_and(here, wg, loc);
                }
                if let Some((d, off)) = off_now {
                    here = nl.sva_and(here, off, loc);
                    for back in 1..=at {
                        let was = nl.sva_past(d, back, p.clock, loc);
                        let was_off = nl.sva_op(NetOp::LogNot, vec![was], 1, loc);
                        here = nl.sva_and(here, was_off, loc);
                    }
                }
                let here = here.unwrap_or_else(|| nl.sva_const(1, 1, loc));
                start = Some(match start {
                    None => here,
                    Some(s) => nl.sva_op(NetOp::LogOr, vec![s, here], 1, loc),
                });
            }
            let raw = start.unwrap_or_else(|| nl.sva_const(1, 0, loc));
            if *non_overlapped {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_ant_pipe"), p.clock, loc);
                nl.states[sid.0 as usize].next = raw;
                match off_now {
                    Some((_, off)) => nl.sva_op(NetOp::LogAnd, vec![sq, off], 1, loc),
                    None => sq,
                }
            } else {
                raw
            }
        }
        AntecedentPlan::Nfa {
            pool,
            leaf_tab,
            state_next,
            accepts_empty,
            end_now,
            reached,
            non_overlapped,
            warm_start,
        } => {
            let mut inj_opt = warm_gate(nl, *warm_start, ticks, loc);
            if let Some(rv) = *reached {
                let rv_b = nl.sva_bool(rv, loc);
                let rb = nl.sva_two_state(rv_b, loc);
                inj_opt = nl.sva_and(inj_opt, rb, loc);
            }
            if let Some((_, off)) = off_now {
                inj_opt = nl.sva_and(inj_opt, off, loc);
            }
            let inject = inj_opt.unwrap_or_else(|| nl.sva_const(1, 1, loc));
            let (pos_leaves, neg_leaves) = prepare_leaves(nl, leaf_tab, loc);
            let mut state_ids = Vec::with_capacity(state_next.len());
            let mut state_outs = Vec::with_capacity(state_next.len());
            for si in 0..state_next.len() {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_ant_{si}"), p.clock, loc);
                state_ids.push(sid);
                state_outs.push(sq);
            }
            let env =
                GuardLowerEnv { inject, nfa_states: &state_outs, pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };
            for (si, &sn) in state_next.iter().enumerate() {
                let mut nxt = emit_guard(nl, pool, sn, &env, loc);
                if let Some((_, off)) = off_now {
                    nxt = nl.sva_op(NetOp::LogAnd, vec![nxt, off], 1, loc);
                }
                nl.states[state_ids[si].0 as usize].next = nxt;
            }
            let mut hit = emit_guard(nl, pool, *end_now, &env, loc);
            if let Some((_, off)) = off_now {
                hit = nl.sva_op(NetOp::LogAnd, vec![hit, off], 1, loc);
            }
            if *non_overlapped {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_ant_pipe"), p.clock, loc);
                nl.states[sid.0 as usize].next = hit;
                let mut delayed = match off_now {
                    Some((_, off)) => nl.sva_op(NetOp::LogAnd, vec![sq, off], 1, loc),
                    None => sq,
                };
                if *accepts_empty {
                    delayed = nl.sva_op(NetOp::LogOr, vec![delayed, inject], 1, loc);
                }
                delayed
            } else {
                hit
            }
        }
    }
}

pub(crate) fn emit_automaton(
    nl: &mut Netlist,
    p: &Plan,
    auto: &AutomatonPlan,
    ticks: Option<(ValueId, u32)>,
    nth: usize,
) -> (Monitor, CheckKind) {
    let loc = p.loc;
    let prefix = if nth == 0 { format!("{}._sva_nfa", nl.top) } else { format!("{}._sva_nfa_{nth}", nl.top) };
    let off_now = p.disable.map(|d| {
        let db = nl.sva_bool(d, loc);
        let d2 = nl.sva_two_state(db, loc);
        (d2, nl.sva_op(NetOp::LogNot, vec![d2], 1, loc))
    });

    match &auto.consequent {
        ConsequentPlan::CoverNfa { pool, leaf_tab, state_next, hit_now, reached, warm_start } => {
            let mut inj_opt = warm_gate(nl, *warm_start, ticks, loc);
            if let Some(rv) = *reached {
                let rv_b = nl.sva_bool(rv, loc);
                let rb = nl.sva_two_state(rv_b, loc);
                inj_opt = nl.sva_and(inj_opt, rb, loc);
            }
            if let Some((_, off)) = off_now {
                inj_opt = nl.sva_and(inj_opt, off, loc);
            }
            let inject = inj_opt.unwrap_or_else(|| nl.sva_const(1, 1, loc));
            let (pos_leaves, neg_leaves) = prepare_leaves(nl, leaf_tab, loc);
            let mut state_ids = Vec::with_capacity(state_next.len());
            let mut state_outs = Vec::with_capacity(state_next.len());
            for si in 0..state_next.len() {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_cov_{si}"), p.clock, loc);
                state_ids.push(sid);
                state_outs.push(sq);
            }
            let env =
                GuardLowerEnv { inject, nfa_states: &state_outs, pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };
            for (si, &sn) in state_next.iter().enumerate() {
                let mut nxt = emit_guard(nl, pool, sn, &env, loc);
                if let Some((_, off)) = off_now {
                    nxt = nl.sva_op(NetOp::LogAnd, vec![nxt, off], 1, loc);
                }
                nl.states[state_ids[si].0 as usize].next = nxt;
            }
            let mut hit = emit_guard(nl, pool, *hit_now, &env, loc);
            if let Some((_, off)) = off_now {
                hit = nl.sva_op(NetOp::LogAnd, vec![hit, off], 1, loc);
            }
            let one = nl.sva_const(1, 1, loc);
            (Monitor::Check(one, hit), CheckKind::Cover)
        }
        ConsequentPlan::PreconditionCover => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let one = nl.sva_const(1, 1, loc);
            (Monitor::Check(one, start), CheckKind::Cover)
        }
        ConsequentPlan::Until { hold, release, with, strong } => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let (pending_id, pending) = alloc_sva_bit_state(nl, format!("{prefix}_until"), p.clock, loc);
            let mut active = nl.sva_op(NetOp::LogOr, vec![start, pending], 1, loc);
            if let Some((_, off)) = off_now {
                active = nl.sva_op(NetOp::LogAnd, vec![active, off], 1, loc);
            }
            let rel_raw = nl.sva_bool(*release, loc);
            let rel_b = nl.sva_two_state(rel_raw, loc);
            let ended = nl.sva_op(NetOp::LogNot, vec![rel_b], 1, loc);
            let next = nl.sva_op(NetOp::LogAnd, vec![active, ended], 1, loc);
            nl.states[pending_id.0 as usize].next = next;
            let ck = if p.kind == PropKind::Assert { CheckKind::Assert } else { CheckKind::Assume };
            let Some(h_val) = *hold else {
                let off = off_now.map(|(_, o)| o);
                return (Monitor::Liveness(nl.sva_liveness(p, pending, active, ended, off, nth)), ck);
            };
            let hold_b = nl.sva_bool(h_val, loc);
            let expr = if *with { hold_b } else { nl.sva_op(NetOp::LogOr, vec![hold_b, rel_b], 1, loc) };
            if *strong {
                let off = off_now.map(|(_, o)| o);
                let sig = nl.sva_liveness(p, pending, active, ended, off, nth);
                (Monitor::Both(active, expr, sig), ck)
            } else {
                (Monitor::Check(active, expr), ck)
            }
        }
        ConsequentPlan::Dfa(dfa) => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let (pos_leaves, neg_leaves) = prepare_leaves(nl, &dfa.leaf_tab, loc);
            let dummy = nl.sva_const(1, 0, loc);
            let env =
                GuardLowerEnv { inject: dummy, nfa_states: &[], pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };

            let mut dfa_ids = Vec::with_capacity(dfa.num_states);
            let mut dfa_outs = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_dfa_{d}"), p.clock, loc);
                dfa_ids.push(sid);
                dfa_outs.push(sq);
            }

            // Pre-emit transition guards once:
            let init_dead_v = emit_guard(nl, &dfa.pool, dfa.init_dead, &env, loc);
            let mut init_to_v = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                init_to_v.push(emit_guard(nl, &dfa.pool, dfa.init_to[d], &env, loc));
            }
            let mut state_dead_v = Vec::with_capacity(dfa.num_states);
            let mut state_to_v = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                state_dead_v.push(emit_guard(nl, &dfa.pool, dfa.state_dead[d], &env, loc));
                let mut row = Vec::with_capacity(dfa.num_states);
                for d2 in 0..dfa.num_states {
                    row.push(emit_guard(nl, &dfa.pool, dfa.state_to[d][d2], &env, loc));
                }
                state_to_v.push(row);
            }

            // Wire DFA state `next`s:
            for d2 in 0..dfa.num_states {
                let mut acc = nl.sva_op(NetOp::LogAnd, vec![start, init_to_v[d2]], 1, loc);
                for d in 0..dfa.num_states {
                    if dfa.state_to[d][d2] != Guard::False {
                        let term = nl.sva_op(NetOp::LogAnd, vec![dfa_outs[d], state_to_v[d][d2]], 1, loc);
                        acc = nl.sva_op(NetOp::LogOr, vec![acc, term], 1, loc);
                    }
                }
                if let Some((_, off)) = off_now {
                    acc = nl.sva_op(NetOp::LogAnd, vec![acc, off], 1, loc);
                }
                nl.states[dfa_ids[d2].0 as usize].next = acc;
            }

            // Safety enable and failure expression:
            let mut any_active = start;
            for &sq in &dfa_outs {
                any_active = nl.sva_op(NetOp::LogOr, vec![any_active, sq], 1, loc);
            }
            if let Some((_, off)) = off_now {
                any_active = nl.sva_op(NetOp::LogAnd, vec![any_active, off], 1, loc);
            }

            let mut fail = nl.sva_op(NetOp::LogAnd, vec![start, init_dead_v], 1, loc);
            for d in 0..dfa.num_states {
                if dfa.state_dead[d] != Guard::False {
                    let f_d = nl.sva_op(NetOp::LogAnd, vec![dfa_outs[d], state_dead_v[d]], 1, loc);
                    fail = nl.sva_op(NetOp::LogOr, vec![fail, f_d], 1, loc);
                }
            }
            if let Some((_, off)) = off_now {
                fail = nl.sva_op(NetOp::LogAnd, vec![fail, off], 1, loc);
            }
            let ok_expr = nl.sva_op(NetOp::LogNot, vec![fail], 1, loc);

            let ck = if p.kind == PropKind::Assert { CheckKind::Assert } else { CheckKind::Assume };
            if !dfa.strong {
                return (Monitor::Check(any_active, ok_expr), ck);
            }

            // Strong / liveness obligation:
            let live_sig = emit_dfa_liveness(
                nl,
                p,
                dfa,
                start,
                &init_to_v,
                &state_to_v,
                &dfa_ids,
                off_now.map(|(_, o)| o),
                &prefix,
            );
            if dfa.has_dead {
                (Monitor::Both(any_active, ok_expr, live_sig), ck)
            } else {
                (Monitor::Liveness(live_sig), ck)
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_dfa_liveness(
    nl: &mut Netlist,
    p: &Plan,
    dfa: &ConsequentDfa,
    start: ValueId,
    init_to_v: &[ValueId],
    state_to_v: &[Vec<ValueId>],
    dfa_ids: &[StateId],
    off: Option<ValueId>,
    prefix: &str,
) -> ValueId {
    let loc = p.loc;
    if dfa.num_states == 0 {
        return nl.sva_const(1, 0, loc);
    }
    if p.kind == PropKind::Assert {
        // Nondeterministic witness picker: picks one non-immediately-matched attempt and watches
        // its single deterministic DFA run until it hits `Matched` (or `Dead` / `disable iff`).
        let pick = nl.sva_value(1, false, ValueKind::Undriven(nl.undriven.len()), loc);
        nl.undriven.push(UndrivenSlice {
            signal: format!("{prefix}_pick"),
            hi: 0,
            lo: 0,
            value: pick,
            loc,
            free: FreeKind::AnySeq,
        });
        let (used_id, used) = alloc_sva_bit_state(nl, format!("{prefix}_picked"), p.clock, loc);
        let mut watch_ids = Vec::with_capacity(dfa.num_states);
        let mut watch_outs = Vec::with_capacity(dfa.num_states);
        for d in 0..dfa.num_states {
            let (wid, wq) = alloc_sva_bit_state(nl, format!("{prefix}_watch_{d}"), p.clock, loc);
            watch_ids.push(wid);
            watch_outs.push(wq);
        }
        let mut init_stays = nl.sva_const(1, 0, loc);
        for &iv in init_to_v {
            init_stays = nl.sva_op(NetOp::LogOr, vec![init_stays, iv], 1, loc);
        }
        let fresh = nl.sva_op(NetOp::LogNot, vec![used], 1, loc);
        let can_take = nl.sva_op(NetOp::LogAnd, vec![start, init_stays], 1, loc);
        let take1 = nl.sva_op(NetOp::LogAnd, vec![pick, can_take], 1, loc);
        let mut take = nl.sva_op(NetOp::LogAnd, vec![take1, fresh], 1, loc);
        if let Some(off_v) = off {
            take = nl.sva_op(NetOp::LogAnd, vec![take, off_v], 1, loc);
        }
        nl.states[used_id.0 as usize].next = nl.sva_op(NetOp::LogOr, vec![used, take], 1, loc);

        for d2 in 0..dfa.num_states {
            let mut nxt = nl.sva_op(NetOp::LogAnd, vec![take, init_to_v[d2]], 1, loc);
            for d in 0..dfa.num_states {
                if dfa.state_to[d][d2] != Guard::False {
                    let step = nl.sva_op(NetOp::LogAnd, vec![watch_outs[d], state_to_v[d][d2]], 1, loc);
                    nxt = nl.sva_op(NetOp::LogOr, vec![nxt, step], 1, loc);
                }
            }
            if let Some(off_v) = off {
                nxt = nl.sva_op(NetOp::LogAnd, vec![nxt, off_v], 1, loc);
            }
            nl.states[watch_ids[d2].0 as usize].next = nxt;
        }

        let mut any_watch = watch_outs[0];
        for &wq in &watch_outs[1..] {
            any_watch = nl.sva_op(NetOp::LogOr, vec![any_watch, wq], 1, loc);
        }
        any_watch
    } else {
        // Miyano-Hayashi breakpoint construction for strong assumptions (`!any(brk)` infinitely often):
        let mut brk_ids = Vec::with_capacity(dfa.num_states);
        let mut brk_outs = Vec::with_capacity(dfa.num_states);
        for d in 0..dfa.num_states {
            let (bid, bq) = alloc_sva_bit_state(nl, format!("{prefix}_brk_{d}"), p.clock, loc);
            brk_ids.push(bid);
            brk_outs.push(bq);
        }
        let mut any_brk = brk_outs[0];
        for &bq in &brk_outs[1..] {
            any_brk = nl.sva_op(NetOp::LogOr, vec![any_brk, bq], 1, loc);
        }
        let brk_empty = nl.sva_op(NetOp::LogNot, vec![any_brk], 1, loc);
        for d2 in 0..dfa.num_states {
            let q_next = nl.states[dfa_ids[d2].0 as usize].next;
            let mut stepped = nl.sva_const(1, 0, loc);
            for d in 0..dfa.num_states {
                if dfa.state_to[d][d2] != Guard::False {
                    let s = nl.sva_op(NetOp::LogAnd, vec![brk_outs[d], state_to_v[d][d2]], 1, loc);
                    stepped = nl.sva_op(NetOp::LogOr, vec![stepped, s], 1, loc);
                }
            }
            if let Some(off_v) = off {
                stepped = nl.sva_op(NetOp::LogAnd, vec![stepped, off_v], 1, loc);
            }
            let reload = nl.sva_op(NetOp::LogAnd, vec![brk_empty, q_next], 1, loc);
            let keep = nl.sva_op(NetOp::LogAnd, vec![any_brk, stepped], 1, loc);
            nl.states[brk_ids[d2].0 as usize].next = nl.sva_op(NetOp::LogOr, vec![reload, keep], 1, loc);
        }
        brk_empty
    }
}
