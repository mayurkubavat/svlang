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

use sv_semantic::{
    BoundProperty, CVal, CycleBound, CycleRange, ExprId, Loc, PropExpr, PropKind, RepeatKind, SeqExpr, SvaEdge,
};

use crate::ir::{
    CheckKind, ClockSpec, Edge, FreeKind, NetOp, Netlist, State, StateId, UndrivenSlice, ValueId, ValueKind,
};
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
        PropExpr::Not(inner) => not_needs_automaton(inner),
        PropExpr::AcceptOn { .. } => true,
        PropExpr::FollowedBy { lhs, rhs, .. } => seq_needs_automaton(lhs) || fb_rhs_needs_automaton(rhs),
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
            if range.max == CycleBound::Unbounded {
                matches!(**inner, PropExpr::Not(_)) && as_bool_formula(inner, false).is_ok()
            } else {
                prop_needs_automaton(inner)
            }
        }
        PropExpr::And(a, b) | PropExpr::Or(a, b) => prop_needs_automaton(a) || prop_needs_automaton(b),
        PropExpr::Implies(a, b) => prop_needs_automaton(a) || not_needs_automaton(a) || prop_needs_automaton(b),
        PropExpr::Iff(a, b) => {
            prop_needs_automaton(a) || not_needs_automaton(a) || prop_needs_automaton(b) || not_needs_automaton(b)
        }
        PropExpr::IfElse { then_prop, else_prop, .. } => {
            prop_needs_automaton(then_prop) || else_prop.as_deref().is_some_and(prop_needs_automaton)
        }
        PropExpr::Until { lhs, rhs, .. } => {
            let single_bool = |p: &PropExpr| matches!(as_seq(p).as_deref(), Ok(SeqExpr::Bool(_)));
            !(single_bool(lhs) && single_bool(rhs))
                && as_bool_formula(lhs, false).is_ok()
                && as_bool_formula(rhs, false).is_ok()
        }
        _ => false,
    }
}

fn not_needs_automaton(p: &PropExpr) -> bool {
    match p {
        PropExpr::Seq(s) | PropExpr::Weak(s) | PropExpr::Strong(s) => seq_needs_automaton(s),
        PropExpr::Not(inner) => not_needs_automaton(inner),
        PropExpr::AcceptOn { .. } | PropExpr::Until { .. } => true,
        PropExpr::And(a, b) | PropExpr::Or(a, b) => not_needs_automaton(a) || not_needs_automaton(b),
        PropExpr::Implies(a, b) => not_needs_automaton(a) || prop_needs_automaton(a) || not_needs_automaton(b),
        PropExpr::Iff(a, b) => {
            prop_needs_automaton(a) || not_needs_automaton(a) || prop_needs_automaton(b) || not_needs_automaton(b)
        }
        PropExpr::IfElse { then_prop, else_prop, .. } => {
            not_needs_automaton(then_prop) || else_prop.as_deref().is_some_and(not_needs_automaton)
        }
        PropExpr::OverlappedImplication { lhs, rhs }
        | PropExpr::NonOverlappedImplication { lhs, rhs }
        | PropExpr::FollowedBy { lhs, rhs, .. } => seq_needs_automaton(lhs) || not_needs_automaton(rhs),
        PropExpr::Nexttime { prop: inner, .. } => not_needs_automaton(inner),
        PropExpr::Eventually { range, prop: inner, .. } | PropExpr::Always { range, prop: inner, .. } => {
            range.max == CycleBound::Unbounded || not_needs_automaton(inner)
        }
        _ => false,
    }
}

fn fb_rhs_needs_automaton(rhs: &PropExpr) -> bool {
    match rhs {
        PropExpr::Until { .. } => true,
        PropExpr::Eventually { strong: true, range, .. } if range.max == CycleBound::Unbounded => true,
        _ => prop_needs_automaton(rhs),
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

/// Leaf table mapping `ExprId` (`ValueId`) to compact leaf indices.
#[derive(Clone, Default)]
pub(crate) struct LeafTable {
    pub(crate) exprs: Vec<(ValueId, bool)>,
    by_val: HashMap<ValueId, u16>,
    /// Minimum tick offset from attempt start at which each leaf index can be evaluated.
    pub(crate) min_offset: Vec<u32>,
}

impl LeafTable {
    fn intern(&mut self, val: ValueId, needs_four_state_neg: bool, offset_lb: u32) -> Result<u16, String> {
        if let Some(&idx) = self.by_val.get(&val) {
            self.exprs[idx as usize].1 |= needs_four_state_neg;
            self.min_offset[idx as usize] = self.min_offset[idx as usize].min(offset_lb);
            return Ok(idx);
        }
        let idx = u16::try_from(self.exprs.len()).map_err(|_| too_large())?;
        self.exprs.push((val, needs_four_state_neg));
        self.min_offset.push(offset_lb);
        self.by_val.insert(val, idx);
        Ok(idx)
    }
}

/// Internal sequence AST enriched with negated boolean leaves, negated `throughout` guards,
/// and embedded complement DFAs (`NotWeak`) produced by property negation normal form.
#[derive(Clone, Debug)]
enum NfaSeq {
    Bool(ExprId),
    NotBool(ExprId),
    Delay { range: CycleRange, lhs: Option<Box<NfaSeq>>, rhs: Box<NfaSeq> },
    Fusion(Box<NfaSeq>, Box<NfaSeq>),
    Repeat { kind: RepeatKind, range: CycleRange, seq: Box<NfaSeq> },
    Or(Box<NfaSeq>, Box<NfaSeq>),
    Throughout { cond: ExprId, negated: bool, seq: Box<NfaSeq> },
    FirstMatch(Box<NfaSeq>),
    NotWeak(Box<NfaSeq>),
    And(Box<NfaSeq>, Box<NfaSeq>),
    Intersect(Box<NfaSeq>, Box<NfaSeq>),
    Within(Box<NfaSeq>, Box<NfaSeq>),
    Clocked,
}

impl NfaSeq {
    fn from_seq(s: &SeqExpr) -> Self {
        match s {
            SeqExpr::Bool(e) => Self::Bool(*e),
            SeqExpr::Delay { range, lhs, rhs } => Self::Delay {
                range: *range,
                lhs: lhs.as_deref().map(|l| Box::new(Self::from_seq(l))),
                rhs: Box::new(Self::from_seq(rhs)),
            },
            SeqExpr::Fusion(a, b) => Self::Fusion(Box::new(Self::from_seq(a)), Box::new(Self::from_seq(b))),
            SeqExpr::Repeat { kind, range, seq } => {
                Self::Repeat { kind: *kind, range: *range, seq: Box::new(Self::from_seq(seq)) }
            }
            SeqExpr::Or(a, b) => Self::Or(Box::new(Self::from_seq(a)), Box::new(Self::from_seq(b))),
            SeqExpr::Throughout { cond, seq } => {
                Self::Throughout { cond: *cond, negated: false, seq: Box::new(Self::from_seq(seq)) }
            }
            SeqExpr::FirstMatch(inner) => Self::FirstMatch(Box::new(Self::from_seq(inner))),
            SeqExpr::And(a, b) => Self::And(Box::new(Self::from_seq(a)), Box::new(Self::from_seq(b))),
            SeqExpr::Intersect(a, b) => Self::Intersect(Box::new(Self::from_seq(a)), Box::new(Self::from_seq(b))),
            SeqExpr::Within(a, b) => Self::Within(Box::new(Self::from_seq(a)), Box::new(Self::from_seq(b))),
            SeqExpr::Clocked { .. } => Self::Clocked,
        }
    }

    fn has_unbounded_wait(&self) -> bool {
        match self {
            Self::Bool(_) | Self::NotBool(_) | Self::Clocked => false,
            Self::Delay { range, lhs, rhs } => {
                range.max == CycleBound::Unbounded
                    || lhs.as_deref().is_some_and(Self::has_unbounded_wait)
                    || rhs.has_unbounded_wait()
            }
            Self::Fusion(a, b) | Self::Or(a, b) | Self::And(a, b) | Self::Intersect(a, b) | Self::Within(a, b) => {
                a.has_unbounded_wait() || b.has_unbounded_wait()
            }
            Self::Repeat { kind, range, seq } => {
                matches!(kind, RepeatKind::Goto | RepeatKind::NonConsecutive)
                    || range.max == CycleBound::Unbounded
                    || seq.has_unbounded_wait()
            }
            Self::Throughout { seq, .. } | Self::FirstMatch(seq) | Self::NotWeak(seq) => seq.has_unbounded_wait(),
        }
    }
}

/// 2-state boolean formula over property leaf expressions (used by `until` and its negations).
#[derive(Clone, Debug)]
pub(crate) enum BoolFormula {
    Const(bool),
    Leaf(ExprId),
    Not(Box<BoolFormula>),
    And(Box<BoolFormula>, Box<BoolFormula>),
}

/// Enclosing abort specification (`accept_on` / `sync_accept_on` vs `reject_on` / `sync_reject_on`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct AbortSpec {
    pub(crate) reject: bool,
    pub(crate) cond: ExprId,
}

///Compiled sequence NFA fragment.
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

    fn import_dfa_guard(
        &mut self,
        src_pool: &GuardPool,
        root: Guard,
        pos_map: &[Guard],
        neg_map: &[Option<Guard>],
    ) -> Result<Guard, String> {
        match root {
            Guard::False => Ok(Guard::False),
            Guard::True => Ok(Guard::True),
            Guard::Node(idx) => {
                let mut mapped = vec![Guard::False; (idx as usize) + 1];
                for i in 0..=(idx as usize) {
                    mapped[i] = match src_pool.nodes[i] {
                        GNode::Var(GVar::PosLeaf(k)) => pos_map[k as usize],
                        GNode::Var(GVar::NegLeaf(k)) => neg_map[k as usize].expect("4-state neg mapped"),
                        GNode::Var(GVar::Inject | GVar::NfaState(_)) => unreachable!("DFA guard has only leaf vars"),
                        GNode::Not(a) => self.pool.not(mapped[a as usize])?,
                        GNode::And(a, b) => {
                            let ga = match a {
                                Guard::False => Guard::False,
                                Guard::True => Guard::True,
                                Guard::Node(j) => mapped[j as usize],
                            };
                            let gb = match b {
                                Guard::False => Guard::False,
                                Guard::True => Guard::True,
                                Guard::Node(j) => mapped[j as usize],
                            };
                            self.pool.and(ga, gb)?
                        }
                        GNode::Or(a, b) => {
                            let ga = match a {
                                Guard::False => Guard::False,
                                Guard::True => Guard::True,
                                Guard::Node(j) => mapped[j as usize],
                            };
                            let gb = match b {
                                Guard::False => Guard::False,
                                Guard::True => Guard::True,
                                Guard::Node(j) => mapped[j as usize],
                            };
                            self.pool.or(ga, gb)?
                        }
                    };
                }
                Ok(mapped[idx as usize])
            }
        }
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

    fn build(&mut self, s: &NfaSeq, inject: Guard, min_off: u32, allow_first_match: bool) -> Result<Frag, String> {
        match s {
            NfaSeq::Bool(e) => {
                let b = self.pos_leaf(*e, min_off)?;
                let raw = self.pool.and(inject, b)?;
                let end_now = self.apply_throughout(raw)?;
                Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: Some(1) })
            }
            NfaSeq::NotBool(e) => {
                let pos = self.pos_leaf(*e, min_off)?;
                let b = self.pool.not(pos)?;
                let raw = self.pool.and(inject, b)?;
                let end_now = self.apply_throughout(raw)?;
                Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: Some(1) })
            }
            NfaSeq::NotWeak(inner) => {
                let (dfa, _) =
                    compile_consequent_dfa(self.nl, self.leaves, inner, DfaMode::Pos { strong: false }, &[], min_off)?;
                let mut pos_map = Vec::with_capacity(dfa.leaf_tab.exprs.len());
                let mut neg_map = Vec::with_capacity(dfa.leaf_tab.exprs.len());
                for (&(val, is_four), &off) in dfa.leaf_tab.exprs.iter().zip(&dfa.leaf_tab.min_offset) {
                    let k = self.leaf_tab.intern(val, is_four, off)?;
                    let pg = self.pool.var(GVar::PosLeaf(k))?;
                    pos_map.push(pg);
                    if is_four {
                        neg_map.push(Some(self.pool.var(GVar::NegLeaf(k))?));
                    } else {
                        neg_map.push(None);
                    }
                }
                let mut regs = Vec::with_capacity(dfa.num_states);
                for _ in 0..dfa.num_states {
                    regs.push(self.alloc_reg(Guard::False)?);
                }
                let init_dead_g = self.import_dfa_guard(&dfa.pool, dfa.init_dead, &pos_map, &neg_map)?;
                let mut init_to_g = Vec::with_capacity(dfa.num_states);
                for d in 0..dfa.num_states {
                    init_to_g.push(self.import_dfa_guard(&dfa.pool, dfa.init_to[d], &pos_map, &neg_map)?);
                }
                let mut state_dead_g = Vec::with_capacity(dfa.num_states);
                let mut state_to_g = Vec::with_capacity(dfa.num_states);
                for d in 0..dfa.num_states {
                    state_dead_g.push(self.import_dfa_guard(&dfa.pool, dfa.state_dead[d], &pos_map, &neg_map)?);
                    let mut row = Vec::with_capacity(dfa.num_states);
                    for d2 in 0..dfa.num_states {
                        row.push(self.import_dfa_guard(&dfa.pool, dfa.state_to[d][d2], &pos_map, &neg_map)?);
                    }
                    state_to_g.push(row);
                }
                for d2 in 0..dfa.num_states {
                    let mut nxt = self.pool.and(inject, init_to_g[d2])?;
                    for d in 0..dfa.num_states {
                        if state_to_g[d][d2] != Guard::False {
                            let step = self.pool.and(regs[d].1, state_to_g[d][d2])?;
                            nxt = self.pool.or(nxt, step)?;
                        }
                    }
                    self.state_next[regs[d2].0] = self.apply_throughout(nxt)?;
                }
                let mut dead_hit = self.pool.and(inject, init_dead_g)?;
                for d in 0..dfa.num_states {
                    if state_dead_g[d] != Guard::False {
                        let d_hit = self.pool.and(regs[d].1, state_dead_g[d])?;
                        dead_hit = self.pool.or(dead_hit, d_hit)?;
                    }
                }
                let end_now = self.apply_throughout(dead_hit)?;
                let min_len = if end_now == Guard::False { None } else { Some(1) };
                Ok(Frag { accepts_empty: false, end_now, min_nonempty_len: min_len })
            }
            NfaSeq::Fusion(a, b) => {
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
            NfaSeq::Delay { range, lhs: None, rhs } => {
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
            NfaSeq::Delay { range, lhs: Some(lhs), rhs } => {
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
            NfaSeq::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
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
            NfaSeq::Repeat { kind: RepeatKind::Goto, range, seq } => {
                bounds_checked(range, "the goto repetition")?;
                if range.min == 0 {
                    return Err("goto repetition [->0] is illegal".into());
                }
                let NfaSeq::Bool(e) = seq.as_ref() else {
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
            NfaSeq::Repeat { kind: RepeatKind::NonConsecutive, range, seq } => {
                bounds_checked(range, "the non-consecutive repetition")?;
                let NfaSeq::Bool(e) = seq.as_ref() else {
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
                            let (idx, q) = self.alloc_reg(Guard::False)?;
                            let active = self.pool.or(inject, q)?;
                            let valid_bit = self.pool.or(b_pos, b_neg)?;
                            let step = self.pool.and(active, valid_bit)?;
                            let post = self.apply_throughout(step)?;
                            self.state_next[idx] = post;
                            return Ok(Frag { accepts_empty: true, end_now: post, min_nonempty_len: Some(1) });
                        }
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
            NfaSeq::Or(a, b) => {
                let fa = self.build(a, inject, min_off, allow_first_match)?;
                let fb = self.build(b, inject, min_off, allow_first_match)?;
                let end_now = self.pool.or(fa.end_now, fb.end_now)?;
                let min_len = match (fa.min_nonempty_len, fb.min_nonempty_len) {
                    (Some(x), Some(y)) => Some(x.min(y)),
                    (x, y) => x.or(y),
                };
                Ok(Frag { accepts_empty: fa.accepts_empty || fb.accepts_empty, end_now, min_nonempty_len: min_len })
            }
            NfaSeq::Throughout { cond, negated, seq } => {
                let pos = self.pos_leaf(*cond, min_off)?;
                let c_guard = if *negated { self.pool.not(pos)? } else { pos };
                self.throughout_stack.push(c_guard);
                let f = self.build(seq, inject, min_off, allow_first_match);
                self.throughout_stack.pop();
                f
            }
            NfaSeq::FirstMatch(inner) => {
                if !allow_first_match || self.in_loop {
                    return Err("`first_match` over an unbounded antecedent or loop".into());
                }
                let start_idx = self.state_next.len();
                let f = self.build(inner, inject, min_off, true)?;
                if f.accepts_empty {
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
            NfaSeq::And(..) => Err("sequence `and` with an unbounded operand".into()),
            NfaSeq::Intersect(..) => Err("sequence `intersect` with an unbounded operand".into()),
            NfaSeq::Within(..) => Err("sequence `within` with an unbounded operand".into()),
            NfaSeq::Clocked => Err("a sequence with its own clock".into()),
        }
    }

    fn build_probe_nullable(&self, s: &NfaSeq) -> Result<bool, String> {
        Ok(match s {
            NfaSeq::Bool(_) | NfaSeq::NotBool(_) | NfaSeq::NotWeak(_) | NfaSeq::Fusion(..) => false,
            NfaSeq::Delay { range, lhs: None, rhs } => range.min == 0 && self.build_probe_nullable(rhs)?,
            NfaSeq::Delay { range, lhs: Some(l), rhs: r } => {
                let has_1 = match range.max {
                    CycleBound::Const(mx) => range.min <= 1 && mx >= 1,
                    CycleBound::Unbounded => range.min <= 1,
                };
                has_1 && self.build_probe_nullable(l)? && self.build_probe_nullable(r)?
            }
            NfaSeq::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
                range.min == 0 || self.build_probe_nullable(seq)?
            }
            NfaSeq::Repeat { kind: RepeatKind::Goto, .. } => false,
            NfaSeq::Repeat { kind: RepeatKind::NonConsecutive, range, .. } => range.min == 0,
            NfaSeq::Or(a, b) => self.build_probe_nullable(a)? || self.build_probe_nullable(b)?,
            NfaSeq::Throughout { seq, .. } | NfaSeq::FirstMatch(seq) => self.build_probe_nullable(seq)?,
            NfaSeq::And(a, b) | NfaSeq::Intersect(a, b) => {
                self.build_probe_nullable(a)? && self.build_probe_nullable(b)?
            }
            NfaSeq::Within(a, b) => self.build_probe_nullable(a)? && self.build_probe_nullable(b)?,
            NfaSeq::Clocked => return Err("a sequence with its own clock".into()),
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum ValFormula {
    Const(bool),
    Leaf(ValueId),
    Not(Box<ValFormula>),
    And(Box<ValFormula>, Box<ValFormula>),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ValAbortSpec {
    pub(crate) reject: bool,
    pub(crate) val: ValueId,
}

impl ValFormula {
    fn from_bool_formula(bf: &BoolFormula, leaves: &[Option<ValueId>]) -> Result<Self, String> {
        let not_lowered = || "an expression that was not lowered".to_string();
        match bf {
            BoolFormula::Const(c) => Ok(Self::Const(*c)),
            BoolFormula::Leaf(e) => {
                let v = leaves.get(e.0 as usize).copied().flatten().ok_or_else(not_lowered)?;
                Ok(Self::Leaf(v))
            }
            BoolFormula::Not(x) => Ok(Self::Not(Box::new(Self::from_bool_formula(x, leaves)?))),
            BoolFormula::And(a, b) => Ok(Self::And(
                Box::new(Self::from_bool_formula(a, leaves)?),
                Box::new(Self::from_bool_formula(b, leaves)?),
            )),
        }
    }

    fn max_lookback(&self, nl: &Netlist, memo: &mut Lookback) -> u32 {
        match self {
            Self::Const(_) => 0,
            Self::Leaf(v) => nl.lookback(*v, memo),
            Self::Not(x) => x.max_lookback(nl, memo),
            Self::And(a, b) => a.max_lookback(nl, memo).max(b.max_lookback(nl, memo)),
        }
    }

    fn emit(&self, nl: &mut Netlist, loc: Loc) -> ValueId {
        match self {
            Self::Const(c) => nl.sva_const(1, u32::from(*c), loc),
            Self::Leaf(v) => {
                let b = nl.sva_bool(*v, loc);
                nl.sva_two_state(b, loc)
            }
            Self::Not(x) => {
                let xv = x.emit(nl, loc);
                nl.sva_op(NetOp::LogNot, vec![xv], 1, loc)
            }
            Self::And(a, b) => {
                let av = a.emit(nl, loc);
                let bv = b.emit(nl, loc);
                nl.sva_op(NetOp::LogAnd, vec![av, bv], 1, loc)
            }
        }
    }
}

/// Negation Normal Form property IR for `sva_nfa`.
#[derive(Clone, Debug)]
enum NormProp {
    Seq {
        seq: NfaSeq,
        strong: bool,
    },
    NotSeq {
        seq: NfaSeq,
        negated_weak: bool,
    },
    Implication {
        overlapped: bool,
        orig_lhs: Option<SeqExpr>,
        nfa_lhs: NfaSeq,
        rhs: Box<NormProp>,
        neg_rhs: Option<Box<NormProp>>,
    },
    FollowedBy {
        overlapped: bool,
        nfa_lhs: NfaSeq,
        rhs: Box<NormProp>,
    },
    And(Box<NormProp>, Box<NormProp>),
    Or(Box<NormProp>, Box<NormProp>),
    Delay {
        range: CycleRange,
        strong: bool,
        inner: Box<NormProp>,
    },
    Until {
        strong: bool,
        overlapping: bool,
        hold: Option<BoolFormula>,
        release: BoolFormula,
    },
    Abort {
        reject: bool,
        cond: ExprId,
        prop: Box<NormProp>,
    },
}

fn bf_or(a: BoolFormula, b: BoolFormula) -> BoolFormula {
    BoolFormula::Not(Box::new(BoolFormula::And(
        Box::new(BoolFormula::Not(Box::new(a))),
        Box::new(BoolFormula::Not(Box::new(b))),
    )))
}

fn as_bool_formula(p: &PropExpr, neg: bool) -> Result<BoolFormula, String> {
    match p {
        PropExpr::Not(inner) => as_bool_formula(inner, !neg),
        PropExpr::Seq(SeqExpr::Bool(e)) | PropExpr::Weak(SeqExpr::Bool(e)) | PropExpr::Strong(SeqExpr::Bool(e)) => {
            let leaf = BoolFormula::Leaf(*e);
            Ok(if neg { BoolFormula::Not(Box::new(leaf)) } else { leaf })
        }
        PropExpr::And(a, b) => {
            if !neg {
                Ok(BoolFormula::And(Box::new(as_bool_formula(a, false)?), Box::new(as_bool_formula(b, false)?)))
            } else {
                Ok(bf_or(as_bool_formula(a, true)?, as_bool_formula(b, true)?))
            }
        }
        PropExpr::Or(a, b) => {
            if !neg {
                Ok(bf_or(as_bool_formula(a, false)?, as_bool_formula(b, false)?))
            } else {
                Ok(BoolFormula::And(Box::new(as_bool_formula(a, true)?), Box::new(as_bool_formula(b, true)?)))
            }
        }
        PropExpr::Implies(a, b) => {
            if !neg {
                Ok(bf_or(as_bool_formula(a, true)?, as_bool_formula(b, false)?))
            } else {
                Ok(BoolFormula::And(Box::new(as_bool_formula(a, false)?), Box::new(as_bool_formula(b, true)?)))
            }
        }
        PropExpr::Iff(a, b) => {
            let ap = as_bool_formula(a, false)?;
            let an = as_bool_formula(a, true)?;
            let bp = as_bool_formula(b, false)?;
            let bn = as_bool_formula(b, true)?;
            if !neg {
                let both = BoolFormula::And(Box::new(ap), Box::new(bp));
                let neither = BoolFormula::And(Box::new(an), Box::new(bn));
                Ok(bf_or(both, neither))
            } else {
                let l1 = BoolFormula::And(Box::new(ap), Box::new(bn));
                let l2 = BoolFormula::And(Box::new(an), Box::new(bp));
                Ok(bf_or(l1, l2))
            }
        }
        PropExpr::IfElse { cond, then_prop, else_prop } => {
            let c_pos = BoolFormula::Leaf(*cond);
            let c_neg = BoolFormula::Not(Box::new(BoolFormula::Leaf(*cond)));
            let t_pos = as_bool_formula(then_prop, false)?;
            let then_branch = bf_or(c_neg.clone(), t_pos);
            let full = match else_prop {
                Some(ep) => {
                    let e_pos = as_bool_formula(ep, false)?;
                    let else_branch = bf_or(c_pos, e_pos);
                    BoolFormula::And(Box::new(then_branch), Box::new(else_branch))
                }
                None => then_branch,
            };
            Ok(if neg { BoolFormula::Not(Box::new(full)) } else { full })
        }
        _ => Err("`until` between properties that are not single booleans".into()),
    }
}

fn delay_norm_prop(np: NormProp, range: CycleRange, strong: bool) -> Result<NormProp, String> {
    bounds_checked(&range, "eventually")?;
    match np {
        NormProp::Seq { seq, strong: inner_strong } => Ok(NormProp::Seq {
            seq: NfaSeq::Delay { range, lhs: None, rhs: Box::new(seq) },
            strong: strong || inner_strong,
        }),
        NormProp::NotSeq { seq, negated_weak } if range.max == CycleBound::Const(range.min) => {
            Ok(NormProp::NotSeq { seq: NfaSeq::Delay { range, lhs: None, rhs: Box::new(seq) }, negated_weak })
        }
        NormProp::NotSeq { seq, negated_weak: true } => {
            let nw = NfaSeq::NotWeak(Box::new(seq));
            Ok(NormProp::Seq { seq: NfaSeq::Delay { range, lhs: None, rhs: Box::new(nw) }, strong })
        }
        NormProp::Implication { .. }
        | NormProp::FollowedBy { .. }
        | NormProp::And(..)
        | NormProp::Or(..)
        | NormProp::Delay { .. } => Ok(NormProp::Delay { range, strong, inner: Box::new(np) }),
        NormProp::Until { .. } => Err("`until`".into()),
        _ => Err("a property operator inside a temporal range".into()),
    }
}

fn norm_prop(p: &PropExpr, neg: bool, default_strong: bool, allow_imp: bool) -> Result<NormProp, String> {
    match p {
        PropExpr::Not(inner) => norm_prop(inner, !neg, default_strong, true),
        PropExpr::And(a, b) => {
            let na = norm_prop(a, neg, default_strong, true)?;
            let nb = norm_prop(b, neg, default_strong, true)?;
            Ok(if !neg { NormProp::And(Box::new(na), Box::new(nb)) } else { NormProp::Or(Box::new(na), Box::new(nb)) })
        }
        PropExpr::Or(a, b) => {
            let na = norm_prop(a, neg, default_strong, true)?;
            let nb = norm_prop(b, neg, default_strong, true)?;
            Ok(if !neg { NormProp::Or(Box::new(na), Box::new(nb)) } else { NormProp::And(Box::new(na), Box::new(nb)) })
        }
        PropExpr::Implies(a, b) => {
            let na = norm_prop(a, !neg, default_strong, true)?;
            let nb = norm_prop(b, neg, default_strong, true)?;
            Ok(if !neg { NormProp::Or(Box::new(na), Box::new(nb)) } else { NormProp::And(Box::new(na), Box::new(nb)) })
        }
        PropExpr::Iff(a, b) => {
            let ap = norm_prop(a, false, default_strong, true)?;
            let an = norm_prop(a, true, default_strong, true)?;
            let bp = norm_prop(b, false, default_strong, true)?;
            let bn = norm_prop(b, true, default_strong, true)?;
            if !neg {
                Ok(NormProp::Or(
                    Box::new(NormProp::And(Box::new(ap), Box::new(bp))),
                    Box::new(NormProp::And(Box::new(an), Box::new(bn))),
                ))
            } else {
                Ok(NormProp::Or(
                    Box::new(NormProp::And(Box::new(ap), Box::new(bn))),
                    Box::new(NormProp::And(Box::new(an), Box::new(bp))),
                ))
            }
        }
        PropExpr::IfElse { cond, then_prop, else_prop } => {
            if !neg {
                let t_pos = norm_prop(then_prop, false, default_strong, true)?;
                let t_neg = norm_prop(then_prop, true, default_strong, true).ok().map(Box::new);
                let then_imp = NormProp::Implication {
                    overlapped: true,
                    orig_lhs: Some(SeqExpr::Bool(*cond)),
                    nfa_lhs: NfaSeq::Bool(*cond),
                    rhs: Box::new(t_pos),
                    neg_rhs: t_neg,
                };
                match else_prop {
                    None => Ok(then_imp),
                    Some(ep) => {
                        let e_pos = norm_prop(ep, false, default_strong, true)?;
                        let e_neg = norm_prop(ep, true, default_strong, true).ok().map(Box::new);
                        let else_imp = NormProp::Implication {
                            overlapped: true,
                            orig_lhs: None,
                            nfa_lhs: NfaSeq::NotBool(*cond),
                            rhs: Box::new(e_pos),
                            neg_rhs: e_neg,
                        };
                        Ok(NormProp::And(Box::new(then_imp), Box::new(else_imp)))
                    }
                }
            } else {
                let t_neg = norm_prop(then_prop, true, default_strong, true)?;
                let then_fb =
                    NormProp::FollowedBy { overlapped: true, nfa_lhs: NfaSeq::Bool(*cond), rhs: Box::new(t_neg) };
                match else_prop {
                    None => Ok(then_fb),
                    Some(ep) => {
                        let e_neg = norm_prop(ep, true, default_strong, true)?;
                        let else_fb = NormProp::FollowedBy {
                            overlapped: true,
                            nfa_lhs: NfaSeq::NotBool(*cond),
                            rhs: Box::new(e_neg),
                        };
                        Ok(NormProp::Or(Box::new(then_fb), Box::new(else_fb)))
                    }
                }
            }
        }
        PropExpr::Seq(s) => {
            let is_strong = default_strong;
            if !neg {
                Ok(NormProp::Seq { seq: NfaSeq::from_seq(s), strong: is_strong })
            } else if let SeqExpr::Bool(e) = s {
                Ok(NormProp::Seq { seq: NfaSeq::NotBool(*e), strong: false })
            } else {
                Ok(NormProp::NotSeq { seq: NfaSeq::from_seq(s), negated_weak: !is_strong })
            }
        }
        PropExpr::Strong(s) => {
            if !neg {
                Ok(NormProp::Seq { seq: NfaSeq::from_seq(s), strong: true })
            } else if let SeqExpr::Bool(e) = s {
                Ok(NormProp::Seq { seq: NfaSeq::NotBool(*e), strong: false })
            } else {
                Ok(NormProp::NotSeq { seq: NfaSeq::from_seq(s), negated_weak: false })
            }
        }
        PropExpr::Weak(s) => {
            if !neg {
                Ok(NormProp::Seq { seq: NfaSeq::from_seq(s), strong: false })
            } else if let SeqExpr::Bool(e) = s {
                Ok(NormProp::Seq { seq: NfaSeq::NotBool(*e), strong: false })
            } else {
                Ok(NormProp::NotSeq { seq: NfaSeq::from_seq(s), negated_weak: true })
            }
        }
        PropExpr::OverlappedImplication { lhs, rhs } | PropExpr::NonOverlappedImplication { lhs, rhs } => {
            if !allow_imp {
                return Err("an implication inside another property operator".into());
            }
            let overlapped = matches!(p, PropExpr::OverlappedImplication { .. });
            if !neg {
                let neg_rhs = norm_prop(rhs, true, default_strong, false).ok().map(Box::new);
                Ok(NormProp::Implication {
                    overlapped,
                    orig_lhs: Some(lhs.clone()),
                    nfa_lhs: NfaSeq::from_seq(lhs),
                    rhs: Box::new(norm_prop(rhs, false, default_strong, false)?),
                    neg_rhs,
                })
            } else {
                Ok(NormProp::FollowedBy {
                    overlapped,
                    nfa_lhs: NfaSeq::from_seq(lhs),
                    rhs: Box::new(norm_prop(rhs, true, default_strong, false)?),
                })
            }
        }
        PropExpr::FollowedBy { overlapped, lhs, rhs } => {
            if !neg {
                Ok(NormProp::FollowedBy {
                    overlapped: *overlapped,
                    nfa_lhs: NfaSeq::from_seq(lhs),
                    rhs: Box::new(norm_prop(rhs, false, default_strong, false)?),
                })
            } else {
                let neg_rhs = norm_prop(rhs, false, default_strong, false).ok().map(Box::new);
                Ok(NormProp::Implication {
                    overlapped: *overlapped,
                    orig_lhs: Some(lhs.clone()),
                    nfa_lhs: NfaSeq::from_seq(lhs),
                    rhs: Box::new(norm_prop(rhs, true, default_strong, false)?),
                    neg_rhs,
                })
            }
        }
        PropExpr::AcceptOn { reject, cond, prop: inner, .. } => {
            let eff_reject = if neg { !*reject } else { *reject };
            Ok(NormProp::Abort {
                reject: eff_reject,
                cond: *cond,
                prop: Box::new(norm_prop(inner, neg, default_strong, allow_imp)?),
            })
        }
        PropExpr::Nexttime { strong, cycles, prop: inner } => {
            if *cycles > crate::sva::MAX_PROPERTY_SPAN {
                return Err(too_long());
            }
            let eff_strong = if neg { !*strong } else { *strong };
            let inner_np = norm_prop(inner, neg, default_strong, true)?;
            delay_norm_prop(inner_np, CycleRange::exact(*cycles), eff_strong)
        }
        PropExpr::Eventually { strong, range, prop: inner } => {
            if !neg {
                if !*strong && range.max == CycleBound::Unbounded {
                    return Err(prop_name(p).into());
                }
                if *strong && range.min == 0 && range.max == CycleBound::Unbounded {
                    if let Ok(bf) = as_bool_formula(inner, false) {
                        return Ok(NormProp::Until { strong: true, overlapping: false, hold: None, release: bf });
                    }
                }
                let inner_np = norm_prop(inner, false, default_strong, false)?;
                delay_norm_prop(inner_np, *range, *strong)
            } else {
                // `not (s_eventually [range] inner)` = `always [range] (not inner)`:
                bounds_checked(range, "eventually")?;
                match range.max {
                    CycleBound::Unbounded => {
                        let inner_pos = norm_prop(inner, false, true, false)?;
                        match inner_pos {
                            NormProp::Seq { seq, .. } => Ok(NormProp::NotSeq {
                                seq: NfaSeq::Delay { range: *range, lhs: None, rhs: Box::new(seq) },
                                negated_weak: false,
                            }),
                            NormProp::Until { .. } => Err("`until`".into()),
                            _ => Err(prop_name(p).into()),
                        }
                    }
                    CycleBound::Const(_) => {
                        let s = NfaSeq::from_seq(as_seq(p)?.as_ref());
                        Ok(NormProp::NotSeq { seq: s, negated_weak: !default_strong })
                    }
                }
            }
        }
        PropExpr::Always { strong, range, prop: inner } => {
            if !neg {
                let CycleBound::Const(last) = range.max else {
                    if range.min > crate::sva::MAX_PROPERTY_SPAN {
                        return Err(too_long());
                    }
                    if let Ok(bf) = as_bool_formula(inner, false) {
                        if range.min == 0 {
                            return Ok(NormProp::Until {
                                strong: false,
                                overlapping: false,
                                hold: Some(bf),
                                release: BoolFormula::Const(false),
                            });
                        }
                        match bf {
                            BoolFormula::Leaf(e) => {
                                return Ok(NormProp::NotSeq {
                                    seq: NfaSeq::Delay { range: *range, lhs: None, rhs: Box::new(NfaSeq::NotBool(e)) },
                                    negated_weak: false,
                                });
                            }
                            BoolFormula::Not(nb) => {
                                if let BoolFormula::Leaf(e) = *nb {
                                    return Ok(NormProp::NotSeq {
                                        seq: NfaSeq::Delay { range: *range, lhs: None, rhs: Box::new(NfaSeq::Bool(e)) },
                                        negated_weak: false,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                    return Err("`always` with no upper bound".into());
                };
                if last > crate::sva::MAX_PROPERTY_SPAN || last < range.min {
                    return Err(too_long());
                }
                if range.min == last {
                    let inner_np = norm_prop(inner, false, default_strong, false)?;
                    return delay_norm_prop(inner_np, CycleRange::exact(range.min), *strong);
                }
                let s = NfaSeq::from_seq(as_seq(p)?.as_ref());
                Ok(NormProp::Seq { seq: s, strong: default_strong })
            } else {
                // `not (always [range] inner)` = `s_eventually [range] (not inner)`:
                bounds_checked(range, "always")?;
                let eff_strong = !*strong;
                if !eff_strong && range.max == CycleBound::Unbounded {
                    return Err("`eventually`".into());
                }
                if eff_strong && range.min == 0 && range.max == CycleBound::Unbounded {
                    if let Ok(bf) = as_bool_formula(inner, true) {
                        return Ok(NormProp::Until { strong: true, overlapping: false, hold: None, release: bf });
                    }
                }
                let inner_np = norm_prop(inner, true, default_strong, false)?;
                delay_norm_prop(inner_np, *range, eff_strong)
            }
        }
        PropExpr::Until { strong, overlapping, lhs, rhs } => {
            let l_bf = as_bool_formula(lhs, false)?;
            let r_bf = as_bool_formula(rhs, false)?;
            if !neg {
                Ok(NormProp::Until { strong: *strong, overlapping: *overlapping, hold: Some(l_bf), release: r_bf })
            } else {
                let nl = BoolFormula::Not(Box::new(l_bf));
                let nr = BoolFormula::Not(Box::new(r_bf));
                if !*overlapping {
                    let both_neg = BoolFormula::And(Box::new(nl), Box::new(nr.clone()));
                    Ok(NormProp::Until { strong: !*strong, overlapping: true, hold: Some(nr), release: both_neg })
                } else {
                    Ok(NormProp::Until { strong: !*strong, overlapping: false, hold: Some(nr), release: nl })
                }
            }
        }
        other => Err(prop_name(other).into()),
    }
}

fn peel_aborts(mut np: NormProp) -> (Vec<AbortSpec>, NormProp) {
    let mut aborts = Vec::new();
    while let NormProp::Abort { reject, cond, prop } = np {
        aborts.push(AbortSpec { reject, cond });
        np = *prop;
    }
    (aborts, np)
}

fn to_pos_seq(np: NormProp) -> Result<(NfaSeq, bool), String> {
    match np {
        NormProp::Seq { seq, strong } => Ok((seq, strong)),
        NormProp::NotSeq { seq, negated_weak: true } => Ok((NfaSeq::NotWeak(Box::new(seq)), true)),
        NormProp::NotSeq { seq, negated_weak: false } if !seq.has_unbounded_wait() => {
            Ok((NfaSeq::NotWeak(Box::new(seq)), false))
        }
        NormProp::Until { hold, .. } => Err(if hold.is_some() { "`until`" } else { "`s_eventually`" }.into()),
        _ => Err("nested property operator with unbounded liveness or abort".into()),
    }
}

fn reduce_norm_prop(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    np: NormProp,
    for_cover: bool,
) -> Result<NormProp, String> {
    match np {
        NormProp::FollowedBy { overlapped, nfa_lhs, rhs } => {
            reduce_followed_by(nl, leaves, overlapped, nfa_lhs, *rhs, for_cover)
        }
        NormProp::Implication { overlapped, nfa_lhs, neg_rhs: Some(nr), .. } => {
            let bad = reduce_followed_by(nl, leaves, overlapped, nfa_lhs, *nr, for_cover)?;
            match bad {
                NormProp::Seq { seq, strong } => Ok(NormProp::NotSeq { seq, negated_weak: !strong }),
                _ => Err("an implication inside another property operator".into()),
            }
        }
        NormProp::Delay { range, strong, inner } => {
            let red = reduce_norm_prop(nl, leaves, *inner, for_cover)?;
            delay_norm_prop(red, range, strong)
        }
        NormProp::And(a, b) => {
            let ra = reduce_norm_prop(nl, leaves, *a, for_cover)?;
            let rb = reduce_norm_prop(nl, leaves, *b, for_cover)?;
            match (ra, rb) {
                (NormProp::NotSeq { seq: s1, negated_weak: nw1 }, NormProp::NotSeq { seq: s2, negated_weak: nw2 })
                    if (nw1 && s1.has_unbounded_wait()) == (nw2 && s2.has_unbounded_wait()) =>
                {
                    let eff_nw = nw1 && s1.has_unbounded_wait();
                    Ok(NormProp::NotSeq { seq: NfaSeq::Or(Box::new(s1), Box::new(s2)), negated_weak: eff_nw })
                }
                (x, y) => {
                    let (s1, st1) = to_pos_seq(x)?;
                    let (s2, st2) = to_pos_seq(y)?;
                    Ok(NormProp::Seq { seq: NfaSeq::And(Box::new(s1), Box::new(s2)), strong: st1 || st2 })
                }
            }
        }
        NormProp::Or(a, b) => {
            let ra = reduce_norm_prop(nl, leaves, *a, for_cover)?;
            let rb = reduce_norm_prop(nl, leaves, *b, for_cover)?;
            match (ra, rb) {
                (NormProp::Seq { seq: s1, strong: st1 }, NormProp::Seq { seq: s2, strong: st2 }) => {
                    Ok(NormProp::Seq { seq: NfaSeq::Or(Box::new(s1), Box::new(s2)), strong: st1 || st2 })
                }
                (NormProp::NotSeq { seq: s1, negated_weak: nw1 }, NormProp::NotSeq { seq: s2, negated_weak: nw2 })
                    if (nw1 && s1.has_unbounded_wait()) == (nw2 && s2.has_unbounded_wait()) =>
                {
                    let eff_nw = nw1 && s1.has_unbounded_wait();
                    Ok(NormProp::NotSeq { seq: NfaSeq::And(Box::new(s1), Box::new(s2)), negated_weak: eff_nw })
                }
                (x, y) => {
                    let (s1, st1) = to_pos_seq(x)?;
                    let (s2, st2) = to_pos_seq(y)?;
                    Ok(NormProp::Seq { seq: NfaSeq::Or(Box::new(s1), Box::new(s2)), strong: st1 || st2 })
                }
            }
        }
        other => Ok(other),
    }
}

/// Resolves `FollowedBy` chains over sequences/complement-DFAs into a single `NormProp::Seq`.
fn reduce_followed_by(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    overlapped: bool,
    nfa_lhs: NfaSeq,
    rhs: NormProp,
    for_cover: bool,
) -> Result<NormProp, String> {
    let mut lb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
    let linj = lb.pool.var(GVar::Inject)?;
    let lf = lb.build(&nfa_lhs, linj, 0, false)?;
    let ant_has_match = if overlapped {
        lf.min_nonempty_len.is_some() && lf.end_now != Guard::False
    } else {
        lf.accepts_empty || (lf.min_nonempty_len.is_some() && lf.end_now != Guard::False)
    };
    if !ant_has_match {
        return Err("an antecedent that has no match".into());
    }

    let resolved_rhs = match rhs {
        NormProp::FollowedBy { overlapped: inner_ov, nfa_lhs: inner_l, rhs: inner_r } => {
            reduce_followed_by(nl, leaves, inner_ov, inner_l, *inner_r, for_cover)?
        }
        NormProp::Implication { .. } | NormProp::And(..) | NormProp::Or(..) | NormProp::Delay { .. } => {
            reduce_norm_prop(nl, leaves, rhs, for_cover)?
        }
        NormProp::Until { hold: Some(_), .. } if for_cover => {
            return Err("a cover of `until`".into());
        }
        // In `cover property ((a ##1 b) #-# s_eventually done)` (or `assert`), `Until { hold: None, release: Leaf(e) }`
        // is `strong(##[0:$] e)`!
        NormProp::Until { strong: true, overlapping: false, hold: None, release: BoolFormula::Leaf(e) } => {
            NormProp::Seq {
                seq: NfaSeq::Delay { range: CycleRange::unbounded(0), lhs: None, rhs: Box::new(NfaSeq::Bool(e)) },
                strong: true,
            }
        }
        NormProp::Until { strong: true, overlapping: false, hold: None, release: BoolFormula::Not(inner) }
            if matches!(*inner, BoolFormula::Leaf(_)) =>
        {
            let BoolFormula::Leaf(e) = *inner else { unreachable!() };
            NormProp::Seq {
                seq: NfaSeq::Delay { range: CycleRange::unbounded(0), lhs: None, rhs: Box::new(NfaSeq::NotBool(e)) },
                strong: true,
            }
        }
        other => other,
    };

    let (r_seq, r_strong) = match resolved_rhs {
        NormProp::Seq { seq, strong } => (seq, strong),
        NormProp::NotSeq { seq, negated_weak: true } => (NfaSeq::NotWeak(Box::new(seq)), true),
        NormProp::NotSeq { seq, negated_weak: false } if !seq.has_unbounded_wait() => {
            (NfaSeq::NotWeak(Box::new(seq)), false)
        }
        NormProp::Until { hold, .. } => {
            return Err(if hold.is_some() {
                "followed-by (#-#, #=#) before `until`".into()
            } else {
                "followed-by (#-#, #=#) before `s_eventually`".into()
            });
        }
        _ => return Err("followed-by (#-#, #=#) with nested abort or implication".into()),
    };

    let mut rb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
    let rinj = rb.pool.var(GVar::Inject)?;
    let rf = rb.build(&r_seq, rinj, 0, true)?;
    if rf.accepts_empty {
        return Err("a sequence that admits an empty match, used as a property".into());
    }
    if rf.min_nonempty_len.is_none() || rf.end_now == Guard::False {
        return Err("a sequence that has no match, used as a property".into());
    }

    let combined = if overlapped {
        NfaSeq::Fusion(Box::new(nfa_lhs.clone()), Box::new(r_seq))
    } else {
        NfaSeq::Delay { range: CycleRange::exact(1), lhs: Some(Box::new(nfa_lhs.clone())), rhs: Box::new(r_seq) }
    };
    let eff_strong = r_strong || nfa_lhs.has_unbounded_wait();
    Ok(NormProp::Seq { seq: combined, strong: eff_strong })
}

/// Compiled antecedent trigger specification in an [`AutomatonPlan`].
#[derive(Clone)]
pub(crate) enum AntecedentPlan {
    /// Property has no antecedent: starts at every warmed-up tick (`reached`).
    Always { reached: Option<ValueId> },
    /// Bounded antecedent compiled by `Matcher` (`starts: Vec<(at, Cond, first_tick)>`, `terms`).
    Bounded { terms: Vec<Term>, two_state: bool, starts: Vec<(u32, Cond, u32)>, non_overlapped: bool },
    /// Existential `SeqNfa` antecedent (`[->]`, `[=]`, `##[m:$]`, `[*m:$]`, or abort-guarded antecedent).
    Nfa {
        pool: GuardPool,
        leaf_tab: LeafTable,
        state_next: Vec<Guard>,
        accepts_empty: bool,
        end_now: Guard,
        reached: Option<ValueId>,
        non_overlapped: bool,
        pipe_abort_free: Vec<ValueId>,
        warm_start: u32,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DfaMode {
    Pos { strong: bool },
    NegWeak,
    NegStrong,
    AntReject,
}

/// Deterministic subset automaton compiled for a consequent (or abort-guarded antecedent).
#[derive(Clone)]
pub(crate) struct ConsequentDfa {
    pub(crate) pool: GuardPool,
    pub(crate) leaf_tab: LeafTable,
    pub(crate) num_states: usize,
    pub(crate) init_matched: Guard,
    pub(crate) init_dead: Guard,
    pub(crate) init_to: Vec<Guard>,
    pub(crate) state_matched: Vec<Guard>,
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
    /// Deterministic DFA monitor for `PropKind::Cover` when `accept_on` or `not` is present.
    CoverDfa(ConsequentDfa),
    /// Precondition cover of `AntecedentPlan` (`start_now & !disable_now`).
    PreconditionCover,
    /// `b until c` / `s_eventually c` (with optional enclosing aborts).
    Until { hold: Option<ValFormula>, release: ValFormula, aborts: Vec<ValAbortSpec>, with: bool, strong: bool },
    /// Subset-constructed universal safety (+ optional strong liveness) DFA.
    Dfa(ConsequentDfa),
}

#[derive(Clone)]
pub(crate) struct AutomatonPlan {
    pub(crate) antecedent: AntecedentPlan,
    pub(crate) consequent: ConsequentPlan,
    /// Optional antecedent rejection DFA when an outer `reject_on` / `sync_reject_on` wraps `lhs |-> rhs`.
    pub(crate) ant_reject: Option<(ConsequentDfa, Option<ValueId>)>,
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

fn dfa_has_cycle(init_to: &[Guard], state_to: &[Vec<Guard>]) -> bool {
    let n = state_to.len();
    if n == 0 {
        return false;
    }
    let mut reachable = vec![false; n];
    let mut q = VecDeque::new();
    for (d, &g) in init_to.iter().enumerate() {
        if g != Guard::False {
            reachable[d] = true;
            q.push_back(d);
        }
    }
    while let Some(u) = q.pop_front() {
        for (v, &g) in state_to[u].iter().enumerate() {
            if g != Guard::False && !reachable[v] {
                reachable[v] = true;
                q.push_back(v);
            }
        }
    }
    // DFS cycle check on reachable subgraph (`0 = unvisited`, `1 = on stack`, `2 = done`):
    let mut color = vec![0u8; n];
    for start in 0..n {
        if !reachable[start] || color[start] != 0 {
            continue;
        }
        let mut stack = vec![(start, 0usize)];
        color[start] = 1;
        while let Some((u, next_v)) = stack.last_mut() {
            let u_val = *u;
            if *next_v < n {
                let v = *next_v;
                *next_v += 1;
                if state_to[u_val][v] == Guard::False || !reachable[v] {
                    continue;
                }
                if color[v] == 1 {
                    return true;
                }
                if color[v] == 0 {
                    color[v] = 1;
                    stack.push((v, 0));
                }
            } else {
                color[u_val] = 2;
                stack.pop();
            }
        }
    }
    false
}

fn compile_consequent_dfa(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    rhs_seq: &NfaSeq,
    mode: DfaMode,
    aborts: &[AbortSpec],
    ant_min_len: u32,
) -> Result<(ConsequentDfa, u32), String> {
    let mut b = SeqNfaBuilder::new(nl, leaves, MAX_CONSEQUENT_NFA_STATES);
    let inj = b.pool.var(GVar::Inject)?;
    let frag = b.build(rhs_seq, inj, ant_min_len, mode != DfaMode::AntReject)?;
    if mode != DfaMode::AntReject {
        if frag.accepts_empty {
            return Err("a sequence that admits an empty match, used as a property".into());
        }
        if frag.min_nonempty_len.is_none() || frag.end_now == Guard::False {
            return Err("a sequence that has no match, used as a property".into());
        }
    }

    let mut abort_leaves = Vec::with_capacity(aborts.len());
    for ab in aborts {
        let v = leaves.get(ab.cond.0 as usize).copied().flatten().ok_or("an expression that was not lowered")?;
        let idx = b.leaf_tab.intern(v, false, ant_min_len)?;
        abort_leaves.push((idx, ab.reject));
    }

    let mut dfa_pool = GuardPool::default();
    let mut var_guards: Vec<Guard> = Vec::new();
    let mut var_roles: Vec<(u16, bool)> = Vec::new();
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
        for (k, &(_, is_four_neg)) in b.leaf_tab.exprs.iter().enumerate() {
            if is_four_neg && ((pos_bits >> k) & 1) != 0 && ((neg_bits >> k) & 1) != 0 {
                return None;
            }
        }
        Some((pos_bits, neg_bits))
    };

    let valid_assigns: Vec<Option<(u16, u16)>> = (0..nassign).map(decode).collect();

    let eval_raw = |inject: bool, mask: u64, pos_bits: u16, neg_bits: u16| -> (bool, u64) {
        let matched = b.pool.eval(frag.end_now, inject, mask, pos_bits, neg_bits);
        if matched && mode != DfaMode::AntReject {
            return (true, 0);
        }
        let mut next_mask = 0u64;
        for (si, &sn) in b.state_next.iter().enumerate() {
            if b.pool.eval(sn, inject, mask, pos_bits, neg_bits) {
                next_mask |= 1u64 << si;
            }
        }
        (matched, next_mask)
    };

    // Prune dead-end NFA masks that can never reach `frag.end_now` on any future suffix:
    let mut raw_seen: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut raw_q: VecDeque<u64> = VecDeque::new();
    let mut raw_edges: HashMap<u64, Vec<(bool, u64)>> = HashMap::new();
    for &(pos_b, neg_b) in valid_assigns.iter().flatten() {
        let (_, nm) = eval_raw(true, 0, pos_b, neg_b);
        if nm != 0 && raw_seen.len() < MAX_DFA_STATES * 4 && raw_seen.insert(nm) {
            raw_q.push_back(nm);
        }
    }
    while let Some(m) = raw_q.pop_front() {
        let mut outs = Vec::new();
        for &(pos_b, neg_b) in valid_assigns.iter().flatten() {
            let (hit, nm) = eval_raw(false, m, pos_b, neg_b);
            outs.push((hit, nm));
            if nm != 0 && raw_seen.len() < MAX_DFA_STATES * 4 && raw_seen.insert(nm) {
                raw_q.push_back(nm);
            }
        }
        raw_edges.insert(m, outs);
    }
    let mut completable: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut changed = true;
    while changed {
        changed = false;
        for (&m, outs) in &raw_edges {
            if completable.contains(&m) {
                continue;
            }
            if outs.iter().any(|&(hit, nm)| hit || (nm != 0 && completable.contains(&nm))) {
                completable.insert(m);
                changed = true;
            }
        }
    }
    let filter_mask = |nm: u64| -> u64 {
        if nm == 0 || !raw_edges.contains_key(&nm) || completable.contains(&nm) {
            nm
        } else {
            0
        }
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

    let classify = |inject: bool,
                    mask: u64,
                    pos_b: u16,
                    neg_b: u16,
                    map: &mut HashMap<u64, usize>,
                    vec: &mut Vec<u64>,
                    q: &mut VecDeque<usize>|
     -> Result<OutTrans, String> {
        for &(leaf_k, is_reject) in &abort_leaves {
            if ((pos_b >> leaf_k) & 1) != 0 {
                return Ok(if is_reject { OutTrans::Dead } else { OutTrans::Matched });
            }
        }
        let (matched, raw_nm) = eval_raw(inject, mask, pos_b, neg_b);
        let next_m = filter_mask(raw_nm);
        Ok(match mode {
            DfaMode::Pos { .. } => {
                if matched {
                    OutTrans::Matched
                } else if next_m == 0 {
                    OutTrans::Dead
                } else {
                    OutTrans::Next(intern_mask(next_m, map, vec, q)?)
                }
            }
            DfaMode::NegWeak | DfaMode::NegStrong => {
                if matched {
                    OutTrans::Dead
                } else if next_m == 0 {
                    OutTrans::Matched
                } else {
                    OutTrans::Next(intern_mask(next_m, map, vec, q)?)
                }
            }
            DfaMode::AntReject => {
                if next_m == 0 {
                    OutTrans::Matched
                } else {
                    OutTrans::Next(intern_mask(next_m, map, vec, q)?)
                }
            }
        })
    };

    let mut init_trans = vec![OutTrans::DontCare; nassign];
    for (a, slot) in init_trans.iter_mut().enumerate() {
        let Some((pos_b, neg_b)) = valid_assigns[a] else { continue };
        *slot = classify(true, 0, pos_b, neg_b, &mut mask_to_dfa, &mut dfa_masks, &mut queue)?;
    }

    let mut state_trans: Vec<Vec<OutTrans>> = Vec::new();
    while let Some(d_id) = queue.pop_front() {
        let mask = dfa_masks[d_id];
        let mut row = vec![OutTrans::DontCare; nassign];
        for (a, slot) in row.iter_mut().enumerate() {
            let Some((pos_b, neg_b)) = valid_assigns[a] else { continue };
            *slot = classify(false, mask, pos_b, neg_b, &mut mask_to_dfa, &mut dfa_masks, &mut queue)?;
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

    let init_matched = mk_pred(&mut dfa_pool, &mut memo, &init_trans, OutTrans::Matched)?;
    let init_dead = mk_pred(&mut dfa_pool, &mut memo, &init_trans, OutTrans::Dead)?;
    let mut init_to = Vec::with_capacity(num_states);
    for d in 0..num_states {
        init_to.push(mk_pred(&mut dfa_pool, &mut memo, &init_trans, OutTrans::Next(d))?);
    }
    let mut state_matched = Vec::with_capacity(num_states);
    let mut state_dead = Vec::with_capacity(num_states);
    let mut state_to = Vec::with_capacity(num_states);
    for row in &state_trans {
        state_matched.push(mk_pred(&mut dfa_pool, &mut memo, row, OutTrans::Matched)?);
        state_dead.push(mk_pred(&mut dfa_pool, &mut memo, row, OutTrans::Dead)?);
        let mut dsts = Vec::with_capacity(num_states);
        for d2 in 0..num_states {
            dsts.push(mk_pred(&mut dfa_pool, &mut memo, row, OutTrans::Next(d2))?);
        }
        state_to.push(dsts);
    }
    let has_dead = init_dead != Guard::False || state_dead.iter().any(|&g| g != Guard::False);
    let wants_strong = matches!(mode, DfaMode::Pos { strong: true } | DfaMode::NegWeak);
    let eff_strong = wants_strong && dfa_has_cycle(&init_to, &state_to);

    Ok((
        ConsequentDfa {
            pool: dfa_pool,
            leaf_tab: b.leaf_tab,
            num_states,
            init_matched,
            init_dead,
            init_to,
            state_matched,
            state_dead,
            state_to,
            has_dead,
            strong: eff_strong,
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

fn wrap_abort_free(mut seq: NfaSeq, aborts: &[AbortSpec]) -> NfaSeq {
    for ab in aborts {
        seq = NfaSeq::Throughout { cond: ab.cond, negated: true, seq: Box::new(seq) };
    }
    seq
}

fn build_cover_consequent(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    prop_expr: &PropExpr,
    reached: Option<ValueId>,
    skip: u32,
    memo: &mut Lookback,
) -> Result<(AntecedentPlan, ConsequentPlan, u32), String> {
    let norm = norm_prop(prop_expr, false, true, true)?;
    let (outer_aborts, body) = peel_aborts(norm);

    if let NormProp::Until { hold, .. } = &body {
        return Err(if hold.is_some() { "a cover of `until`" } else { "a cover of `s_eventually`" }.into());
    }

    // Implication in `cover property` (`lhs |-> rhs` / `lhs |=> rhs`):
    // If there are NO outer `accept_on` aborts (only optional `reject_on`), `lhs |-> rhs` in `cover`
    // is covered iff `lhs #-# rhs` holds! If an outer `accept_on` wraps `(lhs |-> rhs)`, evaluate
    // `lhs` as an antecedent or `FollowedBy` depending on operator:
    if let NormProp::Implication { overlapped, nfa_lhs, rhs, .. } = body {
        let (inner_aborts, rhs_body) = peel_aborts(*rhs);
        if let NormProp::Until { hold, .. } = &rhs_body {
            return Err(if hold.is_some() { "a cover of `until`" } else { "a cover of `s_eventually`" }.into());
        }
        let all_aborts: Vec<AbortSpec> = outer_aborts.iter().chain(&inner_aborts).copied().collect();
        let has_accept = all_aborts.iter().any(|a| !a.reject);
        if !has_accept && matches!(rhs_body, NormProp::Seq { .. }) {
            let NormProp::Seq { seq: r_seq, .. } = rhs_body else { unreachable!() };
            let r_guarded = wrap_abort_free(r_seq, &inner_aborts);
            let combined_np = reduce_followed_by(
                nl,
                leaves,
                overlapped,
                nfa_lhs,
                NormProp::Seq { seq: r_guarded, strong: true },
                true,
            )?;
            let NormProp::Seq { seq: full_seq, .. } = combined_np else { unreachable!() };
            let full_guarded = wrap_abort_free(full_seq, &outer_aborts);
            let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let inj = b.pool.var(GVar::Inject)?;
            let frag = b.build(&full_guarded, inj, 0, false)?;
            let mut warm_start = compute_warm_start(nl, &b.leaf_tab, skip, memo);
            if let Some(rv) = reached {
                warm_start = warm_start.max(nl.lookback(rv, memo));
            }
            return Ok((
                AntecedentPlan::Always { reached },
                ConsequentPlan::CoverNfa {
                    pool: b.pool,
                    leaf_tab: b.leaf_tab,
                    state_next: b.state_next,
                    hit_now: frag.end_now,
                    reached,
                    warm_start,
                },
                warm_start,
            ));
        }
        // General implication cover with `accept_on` or `NotSeq` consequent:
        let mut lb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
        let linj = lb.pool.var(GVar::Inject)?;
        let lf_raw = lb.build(&nfa_lhs, linj, 0, false)?;
        let ant_has_match = if overlapped {
            lf_raw.min_nonempty_len.is_some() && lf_raw.end_now != Guard::False
        } else {
            lf_raw.accepts_empty || (lf_raw.min_nonempty_len.is_some() && lf_raw.end_now != Guard::False)
        };
        if !ant_has_match {
            return Err("an antecedent that has no match".into());
        }
        let guarded_lhs = wrap_abort_free(nfa_lhs, &outer_aborts);
        let mut ab = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
        let ainj = ab.pool.var(GVar::Inject)?;
        let af = ab.build(&guarded_lhs, ainj, 0, false)?;
        let mut ant_warm = compute_warm_start(nl, &ab.leaf_tab, skip, memo);
        if let Some(rv) = reached {
            ant_warm = ant_warm.max(nl.lookback(rv, memo));
        }
        let min_ticks = if overlapped {
            af.min_nonempty_len.unwrap_or(1).saturating_sub(1)
        } else if af.accepts_empty {
            0
        } else {
            af.min_nonempty_len.unwrap_or(1)
        };
        let pipe_abort_free: Vec<ValueId> = outer_aborts
            .iter()
            .map(|a| leaves.get(a.cond.0 as usize).copied().flatten().ok_or("an expression that was not lowered"))
            .collect::<Result<_, _>>()?;
        let (rhs_dfa, _) = compile_cover_rhs_dfa(nl, leaves, rhs_body, &all_aborts, min_ticks)?;
        let rhs_warm = compute_warm_start(nl, &rhs_dfa.leaf_tab, skip, memo);
        let warm = ant_warm.max(rhs_warm);
        let ant = AntecedentPlan::Nfa {
            pool: ab.pool,
            leaf_tab: ab.leaf_tab,
            state_next: ab.state_next,
            accepts_empty: !overlapped && af.accepts_empty,
            end_now: af.end_now,
            reached,
            non_overlapped: !overlapped,
            pipe_abort_free,
            warm_start: warm,
        };
        return Ok((ant, ConsequentPlan::CoverDfa(rhs_dfa), warm));
    }

    let resolved = reduce_norm_prop(nl, leaves, body, true)?;

    let has_accept = outer_aborts.iter().any(|a| !a.reject);
    if !has_accept {
        if let NormProp::Seq { seq, .. } = resolved {
            let mut rb = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let rinj = rb.pool.var(GVar::Inject)?;
            let rf = rb.build(&seq, rinj, 0, false)?;
            if rf.accepts_empty {
                return Err("a sequence that admits an empty match, used as a property".into());
            }
            if rf.min_nonempty_len.is_none() || rf.end_now == Guard::False {
                return Err("a sequence that has no match, used as a property".into());
            }
            let guarded = wrap_abort_free(seq, &outer_aborts);
            let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
            let inj = b.pool.var(GVar::Inject)?;
            let frag = b.build(&guarded, inj, 0, false)?;
            let mut warm_start = compute_warm_start(nl, &b.leaf_tab, skip, memo);
            if let Some(rv) = reached {
                warm_start = warm_start.max(nl.lookback(rv, memo));
            }
            return Ok((
                AntecedentPlan::Always { reached },
                ConsequentPlan::CoverNfa {
                    pool: b.pool,
                    leaf_tab: b.leaf_tab,
                    state_next: b.state_next,
                    hit_now: frag.end_now,
                    reached,
                    warm_start,
                },
                warm_start,
            ));
        }
    }

    let (dfa, _) = compile_cover_rhs_dfa(nl, leaves, resolved, &outer_aborts, 0)?;
    let mut warm_start = compute_warm_start(nl, &dfa.leaf_tab, skip, memo);
    if let Some(rv) = reached {
        warm_start = warm_start.max(nl.lookback(rv, memo));
    }
    Ok((AntecedentPlan::Always { reached }, ConsequentPlan::CoverDfa(dfa), warm_start))
}

fn compile_cover_rhs_dfa(
    nl: &Netlist,
    leaves: &[Option<ValueId>],
    body: NormProp,
    aborts: &[AbortSpec],
    min_ticks: u32,
) -> Result<(ConsequentDfa, u32), String> {
    let body = reduce_norm_prop(nl, leaves, body, true)?;
    let (seq, mode) = match body {
        NormProp::Seq { seq, strong } => (seq, DfaMode::Pos { strong }),
        NormProp::NotSeq { seq, negated_weak } => {
            (seq, if negated_weak { DfaMode::NegWeak } else { DfaMode::NegStrong })
        }
        NormProp::Until { hold, .. } => {
            return Err(if hold.is_some() { "a cover of `until`" } else { "a cover of `s_eventually`" }.into());
        }
        _ => return Err("unsupported operator in `cover property`".into()),
    };
    let (dfa, min_len) = compile_consequent_dfa(nl, leaves, &seq, mode, aborts, min_ticks)?;
    if dfa.init_matched == Guard::False
        && dfa.state_matched.iter().all(|&g| g == Guard::False)
        && dfa_has_cycle(&dfa.init_to, &dfa.state_to)
    {
        return Err("a cover of an unbounded safety invariant with no finite witness".into());
    }
    Ok((dfa, min_len))
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
        let (ant, cover_plan, warm) = build_cover_consequent(nl, leaves, &prop.expr, reached, eff_skip, memo)?;
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
                antecedent: ant,
                consequent: cover_plan,
                ant_reject: None,
                witness_cover: None,
            }),
        });
    }

    // `Assert`, `Assume`, or `Restrict`:
    let norm = norm_prop(&prop.expr, false, false, true)?;
    let (outer_aborts, top_body) = peel_aborts(norm);

    let mut ant_reject: Option<(ConsequentDfa, Option<ValueId>)> = None;
    let mut has_ant = false;
    let (antecedent, ant_min_ticks, mut warm_max, eff_aborts, rhs_norm) = match top_body {
        NormProp::Implication { overlapped, orig_lhs, nfa_lhs, rhs, .. } => {
            has_ant = true;
            let (inner_aborts, rhs_inner) = peel_aborts(*rhs);
            let combined_aborts: Vec<AbortSpec> = outer_aborts.iter().chain(&inner_aborts).copied().collect();

            if outer_aborts.is_empty() && orig_lhs.as_ref().is_some_and(|s| !seq_needs_automaton(s)) {
                let lhs = orig_lhs.as_ref().unwrap();
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
                        non_overlapped: false,
                    },
                    min_at,
                    w_max,
                    combined_aborts,
                    rhs_inner,
                )
            } else {
                // Verify raw `nfa_lhs` has at least one match:
                let mut check_b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
                let cinj = check_b.pool.var(GVar::Inject)?;
                let cfrag = check_b.build(&nfa_lhs, cinj, 0, false)?;
                let ant_has_match = if overlapped {
                    cfrag.min_nonempty_len.is_some() && cfrag.end_now != Guard::False
                } else {
                    cfrag.accepts_empty || (cfrag.min_nonempty_len.is_some() && cfrag.end_now != Guard::False)
                };
                if !ant_has_match {
                    return Err("an antecedent that has no match".into());
                }

                // If any outer abort is a `reject_on` / `sync_reject_on`, build the in-flight antecedent
                // rejection DFA so `c` rejects immediately at `attempt_start` or while `lhs` is completable!
                if outer_aborts.iter().any(|a| a.reject) {
                    let (rej_dfa, _) =
                        compile_consequent_dfa(nl, leaves, &nfa_lhs, DfaMode::AntReject, &outer_aborts, 0)?;
                    ant_reject = Some((rej_dfa, reached));
                }

                let guarded_lhs = wrap_abort_free(nfa_lhs, &outer_aborts);
                let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
                let inj = b.pool.var(GVar::Inject)?;
                let frag = b.build(&guarded_lhs, inj, 0, false)?;
                let mut w = compute_warm_start(nl, &b.leaf_tab, eff_skip, memo);
                if let Some(rv) = reached {
                    w = w.max(nl.lookback(rv, memo));
                }
                if let Some((ref r_dfa, _)) = ant_reject {
                    w = w.max(compute_warm_start(nl, &r_dfa.leaf_tab, eff_skip, memo));
                }
                let min_ticks = if overlapped {
                    cfrag.min_nonempty_len.unwrap_or(1).saturating_sub(1)
                } else if cfrag.accepts_empty {
                    0
                } else {
                    cfrag.min_nonempty_len.unwrap_or(1)
                };
                // For `|=>`, only outer `accept_on` suppresses `ant_pipe`; outer `reject_on` lets `rhs` start
                // so `rhs`'s own `outer_aborts` immediately rejects on that tick!
                let pipe_abort_free: Vec<ValueId> = outer_aborts
                    .iter()
                    .filter(|a| !a.reject)
                    .map(|a| {
                        leaves.get(a.cond.0 as usize).copied().flatten().ok_or("an expression that was not lowered")
                    })
                    .collect::<Result<_, _>>()?;
                (
                    AntecedentPlan::Nfa {
                        pool: b.pool,
                        leaf_tab: b.leaf_tab,
                        state_next: b.state_next,
                        accepts_empty: !overlapped && frag.accepts_empty,
                        end_now: frag.end_now,
                        reached,
                        non_overlapped: !overlapped,
                        pipe_abort_free,
                        warm_start: w,
                    },
                    min_ticks,
                    w,
                    combined_aborts,
                    rhs_inner,
                )
            }
        }
        other => {
            let w = eff_skip.max(reached.map_or(0, |r| nl.lookback(r, memo)));
            (AntecedentPlan::Always { reached }, 0u32, w, outer_aborts, other)
        }
    };

    // Unwrap any inner aborts on `rhs_norm` (in case `rhs_norm` wasn't an `Implication` above):
    let (extra_aborts, rhs_core) = peel_aborts(rhs_norm);
    let mut all_aborts = eff_aborts;
    all_aborts.extend(extra_aborts);

    // Handle 1-tick boolean `FollowedBy` before `Until` (e.g., `not (a |-> b until c)` = `a #-# ((!c) s_until_with ...)`):
    if let NormProp::FollowedBy { overlapped: true, ref nfa_lhs, ref rhs } = rhs_core {
        if matches!(nfa_lhs, NfaSeq::Bool(_) | NfaSeq::NotBool(_)) && matches!(**rhs, NormProp::Until { .. }) {
            let NormProp::Until { strong, overlapping, ref hold, ref release } = **rhs else { unreachable!() };
            let prefix_bf = match *nfa_lhs {
                NfaSeq::Bool(e) => BoolFormula::Leaf(e),
                NfaSeq::NotBool(e) => BoolFormula::Not(Box::new(BoolFormula::Leaf(e))),
                _ => unreachable!(),
            };
            // `prefix #-# (h until r)`: at start tick `prefix` must hold; we enforce `prefix` at `start`
            // via an `AntReject`-style 0-state check (`reject_on(!prefix)` on `start` only), or by
            // building a 1-tick `ant_reject` when `ant_reject.is_none()`!
            if ant_reject.is_none() && matches!(antecedent, AntecedentPlan::Always { .. }) {
                let mut rej_aborts = all_aborts.clone();
                let _ = prefix_bf;
                // Build `NotWeak(nfa_lhs)`? Even simpler: `nfa_lhs` as `Pos` 1-tick `ConsequentDfa`!
                // On `nfa_lhs` (`Bool` or `NotBool`), `num_states == 0`, `init_dead` fires iff `nfa_lhs` fails
                // at `start` (respecting `all_aborts`!), so `ant_reject = Some((prefix_dfa, reached))`!
                let (prefix_dfa, _) = compile_consequent_dfa(
                    nl,
                    leaves,
                    nfa_lhs,
                    DfaMode::Pos { strong: false },
                    &rej_aborts,
                    ant_min_ticks,
                )?;
                rej_aborts.clear();
                warm_max = warm_max.max(compute_warm_start(nl, &prefix_dfa.leaf_tab, eff_skip, memo));
                let hold_vf = hold.as_ref().map(|h| ValFormula::from_bool_formula(h, leaves)).transpose()?;
                let rel_vf = ValFormula::from_bool_formula(release, leaves)?;
                let val_aborts: Vec<ValAbortSpec> = all_aborts
                    .iter()
                    .map(|a| {
                        let v = leaves
                            .get(a.cond.0 as usize)
                            .copied()
                            .flatten()
                            .ok_or_else(|| "an expression that was not lowered".to_string())?;
                        Ok(ValAbortSpec { reject: a.reject, val: v })
                    })
                    .collect::<Result<_, String>>()?;
                let mut rhs_lb =
                    hold_vf.as_ref().map_or(0, |h| h.max_lookback(nl, memo)).max(rel_vf.max_lookback(nl, memo));
                for va in &val_aborts {
                    rhs_lb = rhs_lb.max(nl.lookback(va.val, memo));
                }
                warm_max = warm_max.max(rhs_lb.saturating_sub(ant_min_ticks));
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
                        antecedent,
                        consequent: ConsequentPlan::Until {
                            hold: hold_vf,
                            release: rel_vf,
                            aborts: val_aborts,
                            with: overlapping,
                            strong,
                        },
                        ant_reject: Some((prefix_dfa, reached)),
                        witness_cover: None,
                    }),
                });
            }
        }
    }

    let resolved_rhs = reduce_norm_prop(nl, leaves, rhs_core, false)?;

    // Check if consequent is `Until` (`b until c`, `s_eventually bool`, or their negations/aborts):
    if let NormProp::Until { strong, overlapping, hold, release } = resolved_rhs {
        let hold_vf = hold.as_ref().map(|h| ValFormula::from_bool_formula(h, leaves)).transpose()?;
        let rel_vf = ValFormula::from_bool_formula(&release, leaves)?;
        let val_aborts: Vec<ValAbortSpec> = all_aborts
            .iter()
            .map(|a| {
                let v = leaves
                    .get(a.cond.0 as usize)
                    .copied()
                    .flatten()
                    .ok_or_else(|| "an expression that was not lowered".to_string())?;
                Ok(ValAbortSpec { reject: a.reject, val: v })
            })
            .collect::<Result<_, String>>()?;
        let mut rhs_lb = hold_vf.as_ref().map_or(0, |h| h.max_lookback(nl, memo)).max(rel_vf.max_lookback(nl, memo));
        for va in &val_aborts {
            rhs_lb = rhs_lb.max(nl.lookback(va.val, memo));
        }
        let adj_lb = rhs_lb.saturating_sub(ant_min_ticks);
        warm_max = warm_max.max(adj_lb);
        let mut ant = antecedent;
        match &mut ant {
            AntecedentPlan::Bounded { starts, .. } => {
                for (_, _, first) in starts {
                    *first = (*first).max(adj_lb);
                }
            }
            AntecedentPlan::Nfa { warm_start, .. } => {
                *warm_start = (*warm_start).max(adj_lb);
            }
            AntecedentPlan::Always { .. } => {}
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
                consequent: ConsequentPlan::Until {
                    hold: hold_vf,
                    release: rel_vf,
                    aborts: val_aborts,
                    with: overlapping,
                    strong,
                },
                ant_reject,
                witness_cover: None,
            }),
        });
    }

    let (rhs_seq, dfa_mode) = match resolved_rhs {
        NormProp::Seq { seq, strong } => (seq, DfaMode::Pos { strong }),
        NormProp::NotSeq { seq, negated_weak } => {
            (seq, if negated_weak { DfaMode::NegWeak } else { DfaMode::NegStrong })
        }
        _ => return Err("unsupported property operator".into()),
    };

    let (dfa, _) = compile_consequent_dfa(nl, leaves, &rhs_seq, dfa_mode, &all_aborts, ant_min_ticks)?;
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

    let witness_cover =
        if prop.kind == PropKind::Assert && !dfa.strong && ant_reject.is_none() && (has_ant || reached.is_some()) {
            build_cover_consequent(nl, leaves, &prop.expr, reached, eff_skip, memo).ok().and_then(|(c_ant, cp, w)| {
                if matches!(c_ant, AntecedentPlan::Always { .. }) {
                    warm_max = warm_max.max(w);
                    Some(Box::new(cp))
                } else {
                    None
                }
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
        automaton: Some(AutomatonPlan {
            antecedent: ant,
            consequent: ConsequentPlan::Dfa(dfa),
            ant_reject,
            witness_cover,
        }),
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
                ant_reject: None,
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
                ant_reject: None,
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
            pipe_abort_free,
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
                for &ab_v in pipe_abort_free {
                    let ab_raw = nl.sva_bool(ab_v, loc);
                    let ab_b = nl.sva_two_state(ab_raw, loc);
                    let not_ab = nl.sva_op(NetOp::LogNot, vec![ab_b], 1, loc);
                    delayed = nl.sva_op(NetOp::LogAnd, vec![delayed, not_ab], 1, loc);
                }
                if *accepts_empty {
                    let mut emp = inject;
                    for &ab_v in pipe_abort_free {
                        let ab_raw = nl.sva_bool(ab_v, loc);
                        let ab_b = nl.sva_two_state(ab_raw, loc);
                        let not_ab = nl.sva_op(NetOp::LogNot, vec![ab_b], 1, loc);
                        emp = nl.sva_op(NetOp::LogAnd, vec![emp, not_ab], 1, loc);
                    }
                    delayed = nl.sva_op(NetOp::LogOr, vec![delayed, emp], 1, loc);
                }
                delayed
            } else {
                hit
            }
        }
    }
}

#[allow(clippy::type_complexity)]
fn emit_dfa_safety(
    nl: &mut Netlist,
    p: &Plan,
    dfa: &ConsequentDfa,
    start: ValueId,
    off_now: Option<(ValueId, ValueId)>,
    state_prefix: &str,
) -> (ValueId, ValueId, ValueId, Vec<ValueId>, Vec<Vec<ValueId>>, Vec<StateId>) {
    let loc = p.loc;
    let (pos_leaves, neg_leaves) = prepare_leaves(nl, &dfa.leaf_tab, loc);
    let dummy = nl.sva_const(1, 0, loc);
    let env = GuardLowerEnv { inject: dummy, nfa_states: &[], pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };

    let mut dfa_ids = Vec::with_capacity(dfa.num_states);
    let mut dfa_outs = Vec::with_capacity(dfa.num_states);
    for d in 0..dfa.num_states {
        let (sid, sq) = alloc_sva_bit_state(nl, format!("{state_prefix}_{d}"), p.clock, loc);
        dfa_ids.push(sid);
        dfa_outs.push(sq);
    }

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
    (any_active, fail, ok_expr, init_to_v, state_to_v, dfa_ids)
}

fn emit_abort_signals(nl: &mut Netlist, aborts: &[ValAbortSpec], loc: Loc) -> (Option<ValueId>, Option<ValueId>) {
    if aborts.is_empty() {
        return (None, None);
    }
    let mut accept_acc: Option<ValueId> = None;
    let mut reject_acc: Option<ValueId> = None;
    let mut none_yet: Option<ValueId> = None;
    for ab in aborts {
        let c_raw = nl.sva_bool(ab.val, loc);
        let c = nl.sva_two_state(c_raw, loc);
        let trig = match none_yet {
            None => c,
            Some(ny) => nl.sva_op(NetOp::LogAnd, vec![ny, c], 1, loc),
        };
        if ab.reject {
            reject_acc = Some(match reject_acc {
                None => trig,
                Some(prev) => nl.sva_op(NetOp::LogOr, vec![prev, trig], 1, loc),
            });
        } else {
            accept_acc = Some(match accept_acc {
                None => trig,
                Some(prev) => nl.sva_op(NetOp::LogOr, vec![prev, trig], 1, loc),
            });
        }
        let not_c = nl.sva_op(NetOp::LogNot, vec![c], 1, loc);
        none_yet = Some(match none_yet {
            None => not_c,
            Some(ny) => nl.sva_op(NetOp::LogAnd, vec![ny, not_c], 1, loc),
        });
    }
    (accept_acc, reject_acc)
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

    let base_res = match &auto.consequent {
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
            return (Monitor::Check(one, hit), CheckKind::Cover);
        }
        ConsequentPlan::CoverDfa(dfa) => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let (pos_leaves, neg_leaves) = prepare_leaves(nl, &dfa.leaf_tab, loc);
            let dummy = nl.sva_const(1, 0, loc);
            let env =
                GuardLowerEnv { inject: dummy, nfa_states: &[], pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };
            let mut dfa_ids = Vec::with_capacity(dfa.num_states);
            let mut dfa_outs = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_cov_dfa_{d}"), p.clock, loc);
                dfa_ids.push(sid);
                dfa_outs.push(sq);
            }
            let init_matched_v = emit_guard(nl, &dfa.pool, dfa.init_matched, &env, loc);
            let mut init_to_v = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                init_to_v.push(emit_guard(nl, &dfa.pool, dfa.init_to[d], &env, loc));
            }
            let mut state_matched_v = Vec::with_capacity(dfa.num_states);
            let mut state_to_v = Vec::with_capacity(dfa.num_states);
            for d in 0..dfa.num_states {
                state_matched_v.push(emit_guard(nl, &dfa.pool, dfa.state_matched[d], &env, loc));
                let mut row = Vec::with_capacity(dfa.num_states);
                for d2 in 0..dfa.num_states {
                    row.push(emit_guard(nl, &dfa.pool, dfa.state_to[d][d2], &env, loc));
                }
                state_to_v.push(row);
            }
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
            let mut hit = nl.sva_op(NetOp::LogAnd, vec![start, init_matched_v], 1, loc);
            for d in 0..dfa.num_states {
                if dfa.state_matched[d] != Guard::False {
                    let h_d = nl.sva_op(NetOp::LogAnd, vec![dfa_outs[d], state_matched_v[d]], 1, loc);
                    hit = nl.sva_op(NetOp::LogOr, vec![hit, h_d], 1, loc);
                }
            }
            if let Some((_, off)) = off_now {
                hit = nl.sva_op(NetOp::LogAnd, vec![hit, off], 1, loc);
            }
            let one = nl.sva_const(1, 1, loc);
            return (Monitor::Check(one, hit), CheckKind::Cover);
        }
        ConsequentPlan::PreconditionCover => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let one = nl.sva_const(1, 1, loc);
            return (Monitor::Check(one, start), CheckKind::Cover);
        }
        ConsequentPlan::Until { hold, release, aborts, with, strong } => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let (pending_id, pending) = alloc_sva_bit_state(nl, format!("{prefix}_until"), p.clock, loc);
            let mut active = nl.sva_op(NetOp::LogOr, vec![start, pending], 1, loc);
            if let Some((_, off)) = off_now {
                active = nl.sva_op(NetOp::LogAnd, vec![active, off], 1, loc);
            }
            let raw_rel = release.emit(nl, loc);
            let (acc_early, rej_early) = emit_abort_signals(nl, aborts, loc);
            let mut rel_b = match rej_early {
                None => raw_rel,
                Some(rej) => {
                    let not_rej = nl.sva_op(NetOp::LogNot, vec![rej], 1, loc);
                    nl.sva_op(NetOp::LogAnd, vec![not_rej, raw_rel], 1, loc)
                }
            };
            if let Some(acc) = acc_early {
                rel_b = nl.sva_op(NetOp::LogOr, vec![acc, rel_b], 1, loc);
            }
            let ended = nl.sva_op(NetOp::LogNot, vec![rel_b], 1, loc);
            let next = nl.sva_op(NetOp::LogAnd, vec![active, ended], 1, loc);
            nl.states[pending_id.0 as usize].next = next;
            let ck = if p.kind == PropKind::Assert { CheckKind::Assert } else { CheckKind::Assume };
            let hold_opt: Option<ValueId> = match (hold, rej_early) {
                (None, None) => None,
                (None, Some(rej)) => {
                    let not_rej = nl.sva_op(NetOp::LogNot, vec![rej], 1, loc);
                    Some(match acc_early {
                        None => not_rej,
                        Some(acc) => nl.sva_op(NetOp::LogOr, vec![acc, not_rej], 1, loc),
                    })
                }
                (Some(hf), rej_opt) => {
                    let raw_h = hf.emit(nl, loc);
                    let mut hb = match rej_opt {
                        None => raw_h,
                        Some(rej) => {
                            let not_rej = nl.sva_op(NetOp::LogNot, vec![rej], 1, loc);
                            nl.sva_op(NetOp::LogAnd, vec![not_rej, raw_h], 1, loc)
                        }
                    };
                    if let Some(acc) = acc_early {
                        hb = nl.sva_op(NetOp::LogOr, vec![acc, hb], 1, loc);
                    }
                    Some(hb)
                }
            };
            match hold_opt {
                None => {
                    let off = off_now.map(|(_, o)| o);
                    (Monitor::Liveness(nl.sva_liveness(p, pending, active, ended, off, nth)), ck)
                }
                Some(hold_b) => {
                    let expr = if *with || hold.is_none() {
                        hold_b
                    } else {
                        nl.sva_op(NetOp::LogOr, vec![hold_b, rel_b], 1, loc)
                    };
                    if *strong || hold.is_none() {
                        let off = off_now.map(|(_, o)| o);
                        let sig = nl.sva_liveness(p, pending, active, ended, off, nth);
                        (Monitor::Both(active, expr, sig), ck)
                    } else {
                        (Monitor::Check(active, expr), ck)
                    }
                }
            }
        }
        ConsequentPlan::Dfa(dfa) => {
            let start = emit_antecedent_start(nl, p, &auto.antecedent, off_now, ticks, &prefix);
            let (any_active, _, ok_expr, init_to_v, state_to_v, dfa_ids) =
                emit_dfa_safety(nl, p, dfa, start, off_now, &format!("{prefix}_dfa"));
            let ck = if p.kind == PropKind::Assert { CheckKind::Assert } else { CheckKind::Assume };
            if !dfa.strong {
                (Monitor::Check(any_active, ok_expr), ck)
            } else {
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
    };

    let Some((rej_dfa, rej_reached)) = &auto.ant_reject else {
        return base_res;
    };
    let rej_start = emit_antecedent_start(
        nl,
        p,
        &AntecedentPlan::Always { reached: *rej_reached },
        off_now,
        ticks,
        &format!("{prefix}_arej"),
    );
    let (rej_active, rej_fail, _, _, _, _) =
        emit_dfa_safety(nl, p, rej_dfa, rej_start, off_now, &format!("{prefix}_arej_dfa"));
    let not_rej_fail = nl.sva_op(NetOp::LogNot, vec![rej_fail], 1, loc);
    let (mon, ck) = base_res;
    let combined_mon = match mon {
        Monitor::Check(en, ok) => {
            let new_en = nl.sva_op(NetOp::LogOr, vec![en, rej_active], 1, loc);
            let not_en = nl.sva_op(NetOp::LogNot, vec![en], 1, loc);
            let base_ok = nl.sva_op(NetOp::LogOr, vec![not_en, ok], 1, loc);
            let new_ok = nl.sva_op(NetOp::LogAnd, vec![base_ok, not_rej_fail], 1, loc);
            Monitor::Check(new_en, new_ok)
        }
        Monitor::Liveness(sig) => Monitor::Both(rej_active, not_rej_fail, sig),
        Monitor::Both(en, ok, sig) => {
            let new_en = nl.sva_op(NetOp::LogOr, vec![en, rej_active], 1, loc);
            let not_en = nl.sva_op(NetOp::LogNot, vec![en], 1, loc);
            let base_ok = nl.sva_op(NetOp::LogOr, vec![not_en, ok], 1, loc);
            let new_ok = nl.sva_op(NetOp::LogAnd, vec![base_ok, not_rej_fail], 1, loc);
            Monitor::Both(new_en, new_ok, sig)
        }
    };
    (combined_mon, ck)
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_sticky_always_trigger(
    nl: &mut Netlist,
    prop: &BoundProperty,
    antecedent: Option<(&SeqExpr, bool)>,
    shift: u32,
    leaves: &[Option<ValueId>],
    reached: Option<ValueId>,
    opts: &PropertyOptions,
    memo: &mut Lookback,
) -> Result<ValueId, String> {
    let clock_node =
        prop.clock.as_ref().ok_or("a property with no clock (no clocking event and no default clocking)")?;
    if clock_node.iff.is_some() {
        return Err("a clocking event with `iff`".into());
    }
    let edge = match clock_node.edge {
        SvaEdge::Posedge => Edge::Posedge,
        SvaEdge::Negedge => Edge::Negedge,
        SvaEdge::AnyEdge => return Err("a clocking event on both edges".into()),
    };
    let not_lowered = || "an expression that was not lowered".to_string();
    let leaf = |e: ExprId| leaves.get(e.0 as usize).copied().flatten();
    let signal = leaf(clock_node.signal).ok_or_else(not_lowered)?;
    let clock = ClockSpec { signal, edge };
    let disable = match prop.disable_iff {
        Some(d) => Some(leaf(d).ok_or_else(not_lowered)?),
        None => None,
    };
    let loc = prop.loc;
    let skip = nl.skip_for(clock, opts);
    let disable_lb = disable.map_or(0, |d| nl.lookback(d, memo));
    let eff_skip = skip.max(disable_lb);
    let off_val = disable.map(|d| {
        let b = nl.sva_bool(d, loc);
        nl.sva_op(NetOp::LogNot, vec![b], 1, loc)
    });

    let tag = nl.states.iter().filter(|s| s.name.contains("._sva_sticky")).count();
    let prefix = format!("{}._sva_sticky_{tag}", nl.top);

    let alloc_warm_ok = |nl: &mut Netlist, warm: u32| -> Option<ValueId> {
        if warm == 0 {
            return None;
        }
        let width = (32 - warm.leading_zeros()).max(1);
        let id = StateId(nl.states.len() as u32);
        let out = nl.sva_value(width, false, ValueKind::StateOut(id), loc);
        let top = nl.sva_const(width, warm, loc);
        let below = nl.sva_op(NetOp::ULt, vec![out, top], 1, loc);
        let step = if width == 1 { below } else { nl.sva_op(NetOp::ZeroExt { width }, vec![below], width, loc) };
        let next = nl.sva_op(NetOp::Add, vec![out, step], width, loc);
        nl.states.push(State {
            id,
            name: format!("{prefix}_tick"),
            width,
            signed: false,
            four_state: false,
            clock: Some(clock),
            async_reset: None,
            latch_enable: None,
            init: Some(CVal::new(width, false, 0)),
            next,
            out,
            loc,
        });
        let from = nl.sva_const(width, warm, loc);
        Some(nl.sva_op(NetOp::ULe, vec![from, out], 1, loc))
    };

    let (mut step_trig, total_shift) = match antecedent {
        Some((ant_seq, overlapped)) => {
            let nfa_lhs = NfaSeq::from_seq(ant_seq);
            let (pool, leaf_tab, state_next, frag) = {
                let mut b = SeqNfaBuilder::new(nl, leaves, MAX_NFA_STATES);
                let inj = b.pool.var(GVar::Inject)?;
                let frag = b.build(&nfa_lhs, inj, 0, false)?;
                (b.pool, b.leaf_tab, b.state_next, frag)
            };
            let ant_has_match = if overlapped {
                frag.min_nonempty_len.is_some() && frag.end_now != Guard::False
            } else {
                frag.accepts_empty || (frag.min_nonempty_len.is_some() && frag.end_now != Guard::False)
            };
            if !ant_has_match {
                return Err("an antecedent that has no match".into());
            }
            let mut warm = compute_warm_start(nl, &leaf_tab, eff_skip, memo);
            if let Some(rv) = reached {
                warm = warm.max(nl.lookback(rv, memo));
            }
            let mut inject = match reached {
                Some(rv) => {
                    let rb = nl.sva_bool(rv, loc);
                    nl.sva_two_state(rb, loc)
                }
                None => nl.sva_const(1, 1, loc),
            };
            if let Some(wok) = alloc_warm_ok(nl, warm) {
                inject = nl.sva_op(NetOp::LogAnd, vec![inject, wok], 1, loc);
            }
            if let Some(off) = off_val {
                inject = nl.sva_op(NetOp::LogAnd, vec![inject, off], 1, loc);
            }
            let (pos_leaves, neg_leaves) = prepare_leaves(nl, &leaf_tab, loc);
            let mut nfa_ids = Vec::with_capacity(state_next.len());
            let mut nfa_outs = Vec::with_capacity(state_next.len());
            for k in 0..state_next.len() {
                let (sid, out) = alloc_sva_bit_state(nl, format!("{prefix}_ant_{k}"), clock, loc);
                nfa_ids.push(sid);
                nfa_outs.push(out);
            }
            let env = GuardLowerEnv { inject, nfa_states: &nfa_outs, pos_leaves: &pos_leaves, neg_leaves: &neg_leaves };
            for (k, &nxt_g) in state_next.iter().enumerate() {
                let mut nxt_v = emit_guard(nl, &pool, nxt_g, &env, loc);
                if let Some(off) = off_val {
                    nxt_v = nl.sva_op(NetOp::LogAnd, vec![nxt_v, off], 1, loc);
                }
                nl.states[nfa_ids[k].0 as usize].next = nxt_v;
            }
            let mut end_v = emit_guard(nl, &pool, frag.end_now, &env, loc);
            if !overlapped && frag.accepts_empty {
                end_v = nl.sva_op(NetOp::LogOr, vec![end_v, inject], 1, loc);
            }
            if let Some(off) = off_val {
                end_v = nl.sva_op(NetOp::LogAnd, vec![end_v, off], 1, loc);
            }
            (end_v, shift.saturating_add(u32::from(!overlapped)))
        }
        None => {
            let warm = eff_skip.max(reached.map_or(0, |r| nl.lookback(r, memo)));
            let mut trig = match reached {
                Some(rv) => {
                    let rb = nl.sva_bool(rv, loc);
                    nl.sva_two_state(rb, loc)
                }
                None => nl.sva_const(1, 1, loc),
            };
            if let Some(wok) = alloc_warm_ok(nl, warm) {
                trig = nl.sva_op(NetOp::LogAnd, vec![trig, wok], 1, loc);
            }
            if let Some(off) = off_val {
                trig = nl.sva_op(NetOp::LogAnd, vec![trig, off], 1, loc);
            }
            (trig, shift)
        }
    };

    for step in 0..total_shift {
        let (sid, sq) = alloc_sva_bit_state(nl, format!("{prefix}_d_{step}"), clock, loc);
        let mut nxt = step_trig;
        if let Some(off) = off_val {
            nxt = nl.sva_op(NetOp::LogAnd, vec![nxt, off], 1, loc);
        }
        nl.states[sid.0 as usize].next = nxt;
        step_trig = if let Some(off) = off_val { nl.sva_op(NetOp::LogAnd, vec![sq, off], 1, loc) } else { sq };
    }

    let (sticky_id, sticky_q) = alloc_sva_bit_state(nl, format!("{prefix}_on"), clock, loc);
    let raw_active = nl.sva_op(NetOp::LogOr, vec![step_trig, sticky_q], 1, loc);
    let active =
        if let Some(off) = off_val { nl.sva_op(NetOp::LogAnd, vec![raw_active, off], 1, loc) } else { raw_active };
    nl.states[sticky_id.0 as usize].next = active;
    Ok(active)
}
