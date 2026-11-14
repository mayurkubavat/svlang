//! Concurrent properties compiled to checks: [`Netlist::compile_properties`].
//!
//! A property in [`Netlist::properties`] is a tree over boolean leaves. This pass turns the
//! ones it can express exactly into a [`Check`] (with `immediate == false`) over sampled
//! history, so that a back end which already handles immediate checks and `$past` checks
//! concurrent assertions too. What it cannot express is left alone and reported as
//! [`PropertyOutcome::Unsupported`], naming the operator: nothing is approximated.
//!
//! The subset is the fixed-length one:
//!
//! ```text
//! property := seq | seq |-> seq | seq |=> seq        (weak(seq) and strong(seq) as seq)
//! seq      := b | seq ##N seq | ##N seq | seq [*N]   (N a constant, [*N] with N >= 1)
//! ```
//!
//! Such a sequence has one possible match, of a known length, so an attempt that starts at
//! clock tick `t` is decided at tick `t + span`: the check is placed there and reads each
//! boolean through `$past` with the distance back to its own tick.
//!
//! - **assert**: when every antecedent boolean held, every consequent boolean must hold.
//! - **assume** / **restrict**: the same condition, as a [`CheckKind::Assume`].
//! - **cover**: [`CheckKind::Cover`] of antecedent and consequent together (a vacuous match
//!   of an implication is not a cover).
//! - **`disable iff (d)`**: the attempt is dropped when `d` was true at any tick from its
//!   start to its end.
//! - **In procedural code**: a property under a condition (`if (en) assert property (...)`
//!   in a clocked block) starts an attempt only at a tick where the condition holds, as if
//!   the condition led its antecedent ([`Netlist::property_enables`]).
//! - **The first ticks**: an attempt is checked only once everything it reads is a sampled
//!   value, that is from tick `max(skip_cycles + span, history)`, where `history` also counts
//!   `$past`, `$rose`, `$fell`, `$stable` and `$changed` inside the booleans. One saturating
//!   tick counter per clock, initially 0, provides this.

use sv_semantic::{
    BoundProperty, CVal, CycleBound, CycleRange, ExprId, Loc, PropExpr, PropKind, RepeatKind, SampledKind, SeqExpr,
    SvaEdge,
};

use crate::ir::{
    Check, CheckKind, ClockSpec, Edge, NetOp, Netlist, SampledNode, State, StateId, ValueId, ValueKind, ValueNode,
};

/// Longest property, in clock ticks from its first boolean to its last, that is compiled.
/// Each tick costs history registers, so a longer one is reported as unsupported.
pub const MAX_PROPERTY_SPAN: u32 = 1024;

/// How [`Netlist::compile_properties`] compiles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PropertyOptions {
    /// Attempts that start in the first `skip_cycles` clock ticks are not checked. A tool that
    /// holds the design in reset for the first ticks of every trace sets this to that length.
    pub skip_cycles: u32,
}

/// What [`Netlist::compile_properties`] did with one property of [`Netlist::properties`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropertyOutcome {
    /// Compiled: `Netlist::checks[check]` decides it.
    Compiled {
        /// Index into [`Netlist::checks`].
        check: usize,
    },
    /// Not in the compiled subset; the text names what is not.
    Unsupported(String),
}

/// A property on one time line: every boolean with its tick, counted from the first.
struct Flat {
    antecedent: Vec<(u32, ExprId)>,
    consequent: Vec<(u32, ExprId)>,
    /// Tick of the last boolean.
    span: u32,
}

fn exact(range: &CycleRange, what: &str) -> Result<u32, String> {
    match range.max {
        CycleBound::Const(n) if n == range.min => Ok(n),
        CycleBound::Const(n) => Err(format!("{what} range [{}:{n}]", range.min)),
        CycleBound::Unbounded => Err(format!("{what} range [{}:$]", range.min)),
    }
}

fn too_long() -> String {
    format!("a property longer than {MAX_PROPERTY_SPAN} clock ticks")
}

/// Places the booleans of `s`, which starts at tick `start`. Returns the tick of its last one.
fn flatten_seq(s: &SeqExpr, start: u32, out: &mut Vec<(u32, ExprId)>) -> Result<u32, String> {
    if start > MAX_PROPERTY_SPAN {
        return Err(too_long());
    }
    match s {
        SeqExpr::Bool(e) => {
            out.push((start, *e));
            Ok(start)
        }
        SeqExpr::Delay { range, lhs, rhs } => {
            let n = exact(range, "the delay")?;
            let at = match lhs {
                Some(l) => flatten_seq(l, start, out)?.saturating_add(n),
                None => start.saturating_add(n),
            };
            flatten_seq(rhs, at, out)
        }
        SeqExpr::Fusion(a, b) => {
            let end = flatten_seq(a, start, out)?;
            flatten_seq(b, end, out)
        }
        SeqExpr::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
            let n = exact(range, "the repetition")?;
            if n == 0 {
                return Err("the empty repetition [*0]".into());
            }
            let mut at = start;
            let mut end = start;
            for _ in 0..n {
                end = flatten_seq(seq, at, out)?;
                at = end.saturating_add(1);
            }
            Ok(end)
        }
        SeqExpr::Repeat { kind: RepeatKind::Goto, .. } => Err("the goto repetition [->]".into()),
        SeqExpr::Repeat { kind: RepeatKind::NonConsecutive, .. } => Err("the non-consecutive repetition [=]".into()),
        SeqExpr::And(..) => Err("sequence `and`".into()),
        SeqExpr::Or(..) => Err("sequence `or`".into()),
        SeqExpr::Intersect(..) => Err("`intersect`".into()),
        SeqExpr::Within(..) => Err("`within`".into()),
        SeqExpr::Throughout { .. } => Err("`throughout`".into()),
        SeqExpr::FirstMatch(..) => Err("`first_match`".into()),
        SeqExpr::Clocked { .. } => Err("a sequence with its own clock".into()),
        #[allow(unreachable_patterns)]
        _ => Err("a sequence operator".into()),
    }
}

/// The sequence a property that is only a sequence stands for.
fn as_seq(p: &PropExpr) -> Result<&SeqExpr, String> {
    match p {
        // On an infinite trace a fixed-length sequence always ends, so weak and strong agree.
        PropExpr::Seq(s) | PropExpr::Weak(s) | PropExpr::Strong(s) => Ok(s),
        other => Err(prop_name(other).into()),
    }
}

fn prop_name(p: &PropExpr) -> &'static str {
    match p {
        PropExpr::Seq(_) | PropExpr::Weak(_) | PropExpr::Strong(_) => "a sequence",
        PropExpr::Not(_) => "property `not`",
        PropExpr::And(..) => "property `and`",
        PropExpr::Or(..) => "property `or`",
        PropExpr::Implies(..) => "`implies`",
        PropExpr::Iff(..) => "property `iff`",
        PropExpr::OverlappedImplication { .. } | PropExpr::NonOverlappedImplication { .. } => {
            "an implication inside another property operator"
        }
        PropExpr::FollowedBy { .. } => "followed-by (#-#, #=#)",
        PropExpr::Nexttime { .. } => "`nexttime`",
        PropExpr::Always { .. } => "`always`",
        PropExpr::Eventually { .. } => "`eventually`",
        PropExpr::Until { .. } => "`until`",
        PropExpr::IfElse { .. } => "property `if`",
        PropExpr::AcceptOn { .. } => "`accept_on` / `reject_on`",
        PropExpr::Clocked { .. } => "a property with its own clock inside",
        #[allow(unreachable_patterns)]
        _ => "a property operator",
    }
}

fn flatten(p: &PropExpr) -> Result<Flat, String> {
    let mut antecedent = Vec::new();
    let mut consequent = Vec::new();
    let span = match p {
        PropExpr::OverlappedImplication { lhs, rhs } => {
            let end = flatten_seq(lhs, 0, &mut antecedent)?;
            flatten_seq(as_seq(rhs)?, end, &mut consequent)?
        }
        PropExpr::NonOverlappedImplication { lhs, rhs } => {
            let end = flatten_seq(lhs, 0, &mut antecedent)?;
            flatten_seq(as_seq(rhs)?, end.saturating_add(1), &mut consequent)?
        }
        other => flatten_seq(as_seq(other)?, 0, &mut consequent)?,
    };
    if span > MAX_PROPERTY_SPAN {
        return Err(too_long());
    }
    Ok(Flat { antecedent, consequent, span })
}

/// A flattened property with its leaves as netlist values.
struct Plan {
    index: usize,
    clock: ClockSpec,
    antecedent: Vec<(u32, ValueId)>,
    consequent: Vec<(u32, ValueId)>,
    disable: Option<ValueId>,
    span: u32,
    /// First tick at which the check may fire.
    warm: u32,
}

/// A tick counter to make: its clock, the highest tick it must count to, and once made its
/// value and width.
type TickCounter = (ClockSpec, u32, Option<(ValueId, u32)>);

/// Working state of [`Netlist::lookback`], kept across calls.
struct Lookback {
    /// The answer for each value already visited.
    memo: Vec<Option<u32>>,
    /// Values whose operands are on the stack.
    open: Vec<bool>,
}

impl Netlist {
    /// How many clock ticks back `v` reads through sampled-value functions.
    fn lookback(&self, v: ValueId, memo: &mut Lookback) -> u32 {
        // Iterative: a property can read through a deep cone of logic.
        let Lookback { memo, open } = memo;
        let mut stack = vec![v.0 as usize];
        let mut deps: Vec<usize> = Vec::new();
        while let Some(&i) = stack.last() {
            if memo[i].is_some() {
                stack.pop();
                continue;
            }
            deps.clear();
            let mut own = 0;
            match &self.values[i].kind {
                ValueKind::Op { args, .. } => deps.extend(args.iter().map(|a| a.0 as usize)),
                ValueKind::MemRead { addr, .. } => deps.push(addr.0 as usize),
                ValueKind::Sampled(si) => {
                    let s = &self.sampled[*si];
                    own = match s.kind {
                        SampledKind::Sampled => 0,
                        SampledKind::Past => s.depth,
                        _ => 1,
                    };
                    deps.push(s.expr.0 as usize);
                    deps.extend(s.gate.map(|g| g.0 as usize));
                }
                ValueKind::Input(_) | ValueKind::Const(_) | ValueKind::StateOut(_) | ValueKind::Undriven(_) => {}
            }
            if !open[i] {
                open[i] = true;
                let before = stack.len();
                stack.extend(deps.iter().copied().filter(|&d| memo[d].is_none() && !open[d]));
                if stack.len() > before {
                    continue;
                }
            }
            let deepest = deps.iter().filter_map(|&d| memo[d]).max().unwrap_or(0);
            memo[i] = Some(own.saturating_add(deepest));
            stack.pop();
        }
        memo[v.0 as usize].unwrap_or(0)
    }

    fn plan(
        &self,
        index: usize,
        prop: &BoundProperty,
        opts: &PropertyOptions,
        memo: &mut Lookback,
    ) -> Result<Plan, String> {
        let clock =
            prop.clock.as_ref().ok_or("a property with no clock (no clocking event and no default clocking)")?;
        if clock.iff.is_some() {
            return Err("a clocking event with `iff`".into());
        }
        let edge = match clock.edge {
            SvaEdge::Posedge => Edge::Posedge,
            SvaEdge::Negedge => Edge::Negedge,
            SvaEdge::AnyEdge => return Err("a clocking event on both edges".into()),
        };
        let not_lowered = || "an expression that was not lowered".to_string();
        let signal = self.prop_expr_value(clock.signal).ok_or_else(not_lowered)?;
        let flat = flatten(&prop.expr)?;
        let leaves = |terms: &[(u32, ExprId)]| -> Result<Vec<(u32, ValueId)>, String> {
            terms.iter().map(|&(at, e)| Ok((at, self.prop_expr_value(e).ok_or_else(not_lowered)?))).collect()
        };
        let mut antecedent = leaves(&flat.antecedent)?;
        // In procedural code under a condition: the attempt starts only when it is reached.
        if let Some(reached) = self.property_enables.get(index).copied().flatten() {
            antecedent.insert(0, (0, reached));
        }
        let consequent = leaves(&flat.consequent)?;
        let disable = match prop.disable_iff {
            Some(d) => Some(self.prop_expr_value(d).ok_or_else(not_lowered)?),
            None => None,
        };
        let span = flat.span;
        let mut history = 0u32;
        for &(at, v) in antecedent.iter().chain(&consequent) {
            history = history.max((span - at).saturating_add(self.lookback(v, memo)));
        }
        if let Some(d) = disable {
            history = history.max(span.saturating_add(self.lookback(d, memo)));
        }
        let warm = opts.skip_cycles.saturating_add(span).max(history);
        Ok(Plan { index, clock: ClockSpec { signal, edge }, antecedent, consequent, disable, span, warm })
    }

    fn sva_value(&mut self, width: u32, four_state: bool, kind: ValueKind, loc: Loc) -> ValueId {
        let id = ValueId(self.values.len() as u32);
        self.values.push(ValueNode { id, width, signed: false, four_state, kind, name: None, loc });
        id
    }

    fn sva_op(&mut self, op: NetOp, args: Vec<ValueId>, width: u32, loc: Loc) -> ValueId {
        let four = args.iter().any(|a| self.value(*a).four_state);
        self.sva_value(width, four, ValueKind::Op { op, args }, loc)
    }

    fn sva_const(&mut self, width: u32, v: u32, loc: Loc) -> ValueId {
        self.sva_value(width, false, ValueKind::Const(CVal::new(width, false, i64::from(v))), loc)
    }

    /// `v` as one bit: true when any bit is set.
    fn sva_bool(&mut self, v: ValueId, loc: Loc) -> ValueId {
        if self.value(v).width == 1 {
            v
        } else {
            self.sva_op(NetOp::RedOr, vec![v], 1, loc)
        }
    }

    /// `v` as it was `depth` ticks of `clock` ago.
    fn sva_past(&mut self, v: ValueId, depth: u32, clock: ClockSpec, loc: Loc) -> ValueId {
        if depth == 0 {
            return v;
        }
        let (width, four) = (self.value(v).width, self.value(v).four_state);
        let out = self.sva_value(width, four, ValueKind::Sampled(self.sampled.len()), loc);
        self.sampled.push(SampledNode {
            kind: SampledKind::Past,
            expr: v,
            depth,
            clock: Some(clock),
            gate: None,
            out,
            loc,
        });
        out
    }

    /// The conjunction of `acc` and `v` (`None` is true).
    fn sva_and(&mut self, acc: Option<ValueId>, v: ValueId, loc: Loc) -> Option<ValueId> {
        Some(match acc {
            None => v,
            Some(a) => self.sva_op(NetOp::LogAnd, vec![a, v], 1, loc),
        })
    }

    /// Every boolean of `terms`, each read back from tick `span` to its own tick.
    fn sva_terms(
        &mut self,
        terms: &[(u32, ValueId)],
        span: u32,
        clock: ClockSpec,
        mut acc: Option<ValueId>,
        loc: Loc,
    ) -> Option<ValueId> {
        for &(at, v) in terms {
            let b = self.sva_bool(v, loc);
            let p = self.sva_past(b, span - at, clock, loc);
            acc = self.sva_and(acc, p, loc);
        }
        acc
    }

    /// A tick counter on `clock` that starts at 0 and stops at `max`.
    fn sva_tick_counter(&mut self, clock: ClockSpec, max: u32, nth: usize, loc: Loc) -> (ValueId, u32) {
        let width = (32 - max.leading_zeros()).max(1);
        let id = StateId(self.states.len() as u32);
        let out = self.sva_value(width, false, ValueKind::StateOut(id), loc);
        let top = self.sva_const(width, max, loc);
        let below = self.sva_op(NetOp::ULt, vec![out, top], 1, loc);
        let step = if width == 1 { below } else { self.sva_op(NetOp::ZeroExt { width }, vec![below], width, loc) };
        let next = self.sva_op(NetOp::Add, vec![out, step], width, loc);
        let name = if nth == 0 { format!("{}._sva_tick", self.top) } else { format!("{}._sva_tick_{nth}", self.top) };
        self.states.push(State {
            id,
            name,
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
        (out, width)
    }

    /// Compiles every property of [`Self::properties`] that is in the fixed-length subset into
    /// a [`Check`] (see the [module documentation](crate::sva) for the subset and the exact
    /// meaning), and returns what happened to each, in the order of [`Self::properties`].
    /// [`Self::property_checks`] records the same result.
    ///
    /// A property that is already compiled keeps its check. Properties stay in
    /// [`Self::properties`] either way. Calling this may re-index [`Self::values`].
    pub fn compile_properties(&mut self, opts: &PropertyOptions) -> Vec<PropertyOutcome> {
        if self.property_checks.len() != self.properties.len() {
            self.property_checks.resize(self.properties.len(), None);
        }
        let mut outcomes: Vec<Option<PropertyOutcome>> = vec![None; self.properties.len()];
        let mut plans = Vec::new();
        let mut memo = Lookback { memo: vec![None; self.values.len()], open: vec![false; self.values.len()] };
        for (i, prop) in self.properties.iter().enumerate() {
            if let Some(check) = self.property_checks[i] {
                outcomes[i] = Some(PropertyOutcome::Compiled { check });
                continue;
            }
            match self.plan(i, prop, opts, &mut memo) {
                Ok(p) => plans.push(p),
                Err(why) => outcomes[i] = Some(PropertyOutcome::Unsupported(why)),
            }
        }

        // One tick counter per clock, as wide as its latest first check needs.
        let mut counters: Vec<TickCounter> = Vec::new();
        for p in plans.iter().filter(|p| p.warm > 0) {
            match counters.iter_mut().find(|c| c.0 == p.clock) {
                Some(c) => c.1 = c.1.max(p.warm),
                None => counters.push((p.clock, p.warm, None)),
            }
        }

        for p in &plans {
            let prop = &self.properties[p.index];
            let (kind, loc) = (prop.kind, prop.loc);
            let mut guard = None;
            if p.warm > 0 {
                let nth = counters.iter().position(|c| c.0 == p.clock).expect("counter planned above");
                let (ticks, width) = match counters[nth].2 {
                    Some(made) => made,
                    None => {
                        let made = self.sva_tick_counter(p.clock, counters[nth].1, nth, loc);
                        counters[nth].2 = Some(made);
                        made
                    }
                };
                let first = self.sva_const(width, p.warm, loc);
                guard = Some(self.sva_op(NetOp::ULe, vec![first, ticks], 1, loc));
            }
            if let Some(d) = p.disable {
                let d = self.sva_bool(d, loc);
                for back in 0..=p.span {
                    let was = self.sva_past(d, back, p.clock, loc);
                    let off = self.sva_op(NetOp::LogNot, vec![was], 1, loc);
                    guard = self.sva_and(guard, off, loc);
                }
            }
            let (kind, enable, expr) = match kind {
                PropKind::Cover => {
                    let all = self.sva_terms(&p.antecedent, p.span, p.clock, None, loc);
                    let all = self.sva_terms(&p.consequent, p.span, p.clock, all, loc);
                    (CheckKind::Cover, guard, all)
                }
                PropKind::Assert | PropKind::Assume | PropKind::Restrict => {
                    let enable = self.sva_terms(&p.antecedent, p.span, p.clock, guard, loc);
                    let body = self.sva_terms(&p.consequent, p.span, p.clock, None, loc);
                    let kind = if kind == PropKind::Assert { CheckKind::Assert } else { CheckKind::Assume };
                    (kind, enable, body)
                }
            };
            let one = |nl: &mut Netlist, v: Option<ValueId>| v.unwrap_or_else(|| nl.sva_const(1, 1, loc));
            let enable = one(self, enable);
            let expr = one(self, expr);
            let check = self.checks.len();
            self.checks.push(Check {
                kind,
                label: self.property_names.get(p.index).cloned().or_else(|| self.properties[p.index].label.clone()),
                immediate: false,
                enable,
                expr,
                clock: Some(p.clock),
                loc,
            });
            self.property_checks[p.index] = Some(check);
            outcomes[p.index] = Some(PropertyOutcome::Compiled { check });
        }
        if !plans.is_empty() {
            self.toposort();
        }
        outcomes.into_iter().map(|o| o.expect("every property has an outcome")).collect()
    }
}
