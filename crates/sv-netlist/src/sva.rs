//! Concurrent properties compiled to checks: [`Netlist::compile_properties`].
//!
//! A property in [`Netlist::properties`] is a tree over boolean leaves. This pass turns the
//! ones it can express exactly into a [`Check`] (with `immediate == false`) over sampled
//! history, so that a back end which already handles immediate checks and `$past` checks
//! concurrent assertions too. What it cannot express is left alone and reported as
//! [`PropertyOutcome::Unsupported`], naming the operator: nothing is approximated.
//!
//! The subset is the bounded one: every sequence in it has a longest match.
//!
//! ```text
//! property := seq | seq |-> seq | seq |=> seq        (weak(seq) and strong(seq) as seq)
//! seq      := b
//!           | seq ##N seq | seq ##[M:N] seq | ##N seq | ##[M:N] seq
//!           | seq [*N] | seq [*M:N]
//!           | seq or seq | seq and seq | seq intersect seq | seq within seq
//!           | b throughout seq | first_match(seq)
//! ```
//!
//! `M` and `N` are constants, and `M` may be 0. Not in the subset, and reported by name:
//! a range that ends at `$` (with `[*]` and `[+]`), goto `[->]` and non-consecutive `[=]`
//! repetition, a sequence with its own clock, and every property operator other than one
//! implication at the top (`not`, property `and` and `or` over implications, `if`, `until`,
//! `always`, `eventually`, `nexttime`, `#-#`, `accept_on`, ...).
//!
//! A bounded sequence has finitely many possible matches, each of a known length. The pass
//! enumerates them as IEEE 1800-2023 annex F defines a match: for the attempt that starts at
//! clock tick `t`, it builds for each sequence and each tick a match can end at the
//! condition that such a match exists, over the booleans at their ticks. `or` joins the
//! matches of its operands; `intersect` keeps those that end together; `and` those where
//! both operands match, ending with the later one; `within` a match of the right operand
//! with a match of the left one inside it; `throughout` those with the boolean true at each
//! of their ticks; `first_match` those with no match ending earlier. A repetition from 0
//! has the empty match, which joins as 16.9.2.1 says: `empty ##0 s` and `s ##0 empty` do
//! not match, `empty ##n s` is `##(n-1) s`, and `s ##n empty` is `s ##(n-1) 1`.
//!
//! The attempt is decided at tick `t + span`, where `span` is the distance to the last tick
//! any of its matches reads: the check is placed there and reads each boolean through
//! `$past` with the distance back to its own tick. A boolean that is unknown is not true.
//!
//! - **assert**: for every match of the antecedent, the consequent has at least one match
//!   that starts at the tick the antecedent's match ends (`|->`) or one tick later (`|=>`).
//!   With no antecedent: the sequence has at least one match from tick `t`. A failure is
//!   reported at tick `t + span`, also when an earlier tick already settled it.
//! - **assume** / **restrict**: the same condition, as a [`CheckKind::Assume`].
//! - **cover**: [`CheckKind::Cover`] of some match of the antecedent whose consequent has a
//!   match (a vacuous match of an implication is not a cover).
//! - **The empty match at the top**: an empty match of the antecedent starts no consequent
//!   with `|->`, and one at tick `t` with `|=>` (`s |=> p` is `s ##1 1 |-> p`). A sequence
//!   used as a property or a consequent that admits an empty match or has no match at all,
//!   and an antecedent with no match that starts a consequent, are illegal by 16.12.22 and
//!   are reported as unsupported.
//! - **`disable iff (d)`**: the attempt is dropped when `d` was true at any tick from `t` to
//!   `t + span`.
//! - **In procedural code**: a property under a condition (`if (en) assert property (...)`
//!   in a clocked block) starts an attempt only at a tick where the condition holds, as if
//!   the condition led its antecedent ([`Netlist::property_enables`]).
//! - **The first ticks**: an attempt is checked only once everything it reads is a sampled
//!   value, that is from tick `max(skip_cycles + span, history)`, where `history` also counts
//!   `$past`, `$rose`, `$fell`, `$stable` and `$changed` inside the booleans. One saturating
//!   tick counter per clock, initially 0, provides this.
//! - **Size**: a property that reads further than [`MAX_PROPERTY_SPAN`] ticks, or needs more
//!   than [`MAX_PROPERTY_TERMS`] conditions, is reported as unsupported.

use std::collections::{BTreeMap, BTreeSet, HashMap};

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

/// Most conditions (booleans at a tick and the `and`, `or` and `not` over them) one property
/// may take. Ranges multiply the matches to tell apart, so a property that needs more is
/// reported as unsupported.
pub const MAX_PROPERTY_TERMS: u32 = 1 << 16;

/// How [`Netlist::compile_properties`] compiles.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PropertyOptions {
    /// Attempts that start in the first `skip_cycles` clock ticks are not checked. A tool that
    /// holds the design in reset for the first ticks of every trace sets this to that length.
    pub skip_cycles: u32,
    /// `skip_cycles` for the properties on one clock and edge, where a design has several
    /// clocks and the first ticks of each cover a different stretch of time. A property on
    /// a clock and edge with no entry here uses [`Self::skip_cycles`].
    pub skip_by_clock: Vec<ClockSkip>,
}

/// [`PropertyOptions::skip_cycles`] for the properties on one clock and edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockSkip {
    /// The clock, a top-level input, by its name in [`Netlist::inputs`].
    pub input: String,
    /// The edge: `true` for properties on the rising edge, `false` for the falling edge.
    pub posedge: bool,
    /// Attempts that start in this many first ticks of that clock edge are not checked.
    pub ticks: u32,
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

/// A condition over the booleans of one attempt: a constant or a node of [`Terms`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Cond {
    False,
    True,
    Node(u32),
}

/// One node of [`Terms`]. Operands have smaller indices than the node.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Term {
    /// The value at clock tick `at`, counted from the start of the attempt.
    Leaf {
        at: u32,
        value: ValueId,
    },
    Not(u32),
    And(Cond, Cond),
    Or(Cond, Cond),
}

/// The conditions of one property, each built once.
#[derive(Default)]
struct Terms {
    nodes: Vec<Term>,
    index: HashMap<Term, u32>,
    /// Steps taken so far, to stop a property that is too large to enumerate.
    work: u32,
    /// Whether a negation was built: the booleans then have to be two-state.
    negated: bool,
}

impl Terms {
    /// Counts one step of the enumeration.
    fn step(&mut self) -> Result<(), String> {
        self.work += 1;
        if self.work > MAX_PROPERTY_TERMS.saturating_mul(64) {
            return Err(too_large());
        }
        Ok(())
    }

    fn node(&mut self, t: Term) -> Result<Cond, String> {
        self.step()?;
        if let Some(&i) = self.index.get(&t) {
            return Ok(Cond::Node(i));
        }
        if self.nodes.len() >= MAX_PROPERTY_TERMS as usize {
            return Err(too_large());
        }
        let i = self.nodes.len() as u32;
        self.nodes.push(t);
        self.index.insert(t, i);
        Ok(Cond::Node(i))
    }

    fn leaf(&mut self, at: u32, value: ValueId) -> Result<Cond, String> {
        self.node(Term::Leaf { at, value })
    }

    fn and(&mut self, a: Cond, b: Cond) -> Result<Cond, String> {
        match (a, b) {
            (Cond::False, _) | (_, Cond::False) => Ok(Cond::False),
            (Cond::True, x) | (x, Cond::True) => Ok(x),
            _ if a == b => Ok(a),
            _ => self.node(Term::And(a, b)),
        }
    }

    fn or(&mut self, a: Cond, b: Cond) -> Result<Cond, String> {
        match (a, b) {
            (Cond::True, _) | (_, Cond::True) => Ok(Cond::True),
            (Cond::False, x) | (x, Cond::False) => Ok(x),
            _ if a == b => Ok(a),
            _ => self.node(Term::Or(a, b)),
        }
    }

    fn not(&mut self, a: Cond) -> Result<Cond, String> {
        match a {
            Cond::True => Ok(Cond::False),
            Cond::False => Ok(Cond::True),
            Cond::Node(i) => {
                self.negated = true;
                self.node(Term::Not(i))
            }
        }
    }
}

/// The matches of a sequence from one start tick: for each tick just after a match ends, when
/// that match exists. A key equal to the start tick is the empty match. No condition is
/// [`Cond::False`].
type Ends = BTreeMap<u32, Cond>;

/// A constant finite range, as `(min, max)`.
fn bounds(range: &CycleRange, what: &str) -> Result<(u32, u32), String> {
    match range.max {
        CycleBound::Const(n) if n >= range.min => Ok((range.min, n)),
        CycleBound::Const(n) => Err(format!("{what} range [{}:{n}] with its bounds reversed", range.min)),
        CycleBound::Unbounded => Err(format!("{what} range [{}:$]", range.min)),
    }
}

fn too_long() -> String {
    format!("a property longer than {MAX_PROPERTY_SPAN} clock ticks")
}

fn too_large() -> String {
    format!("a property of more than {MAX_PROPERTY_TERMS} terms")
}

/// Enumerates the matches of bounded sequences, following the tight satisfaction relation of
/// IEEE 1800-2023 annex F with a match as the ticks `start .. end` (`end` not included).
struct Matcher<'a> {
    nl: &'a Netlist,
    terms: Terms,
}

impl Matcher<'_> {
    fn add(&mut self, ends: &mut Ends, end: u32, cond: Cond) -> Result<(), String> {
        if cond == Cond::False {
            return Ok(());
        }
        let had = ends.get(&end).copied().unwrap_or(Cond::False);
        let both = self.terms.or(had, cond)?;
        ends.insert(end, both);
        Ok(())
    }

    fn leaf(&mut self, at: u32, e: ExprId) -> Result<Cond, String> {
        if at > MAX_PROPERTY_SPAN {
            return Err(too_long());
        }
        let value = self.nl.prop_expr_value(e).ok_or("an expression that was not lowered")?;
        self.terms.leaf(at, value)
    }

    /// `lhs ##[min:max] rhs` from tick `start`, given the matches of `lhs` (`None`: no left
    /// operand).
    fn delay(&mut self, lhs: Option<Ends>, start: u32, min: u32, max: u32, rhs: &SeqExpr) -> Result<Ends, String> {
        // Where `rhs` starts, with the condition to get there: whatever `rhs` matches there,
        // and only when its match is not empty (`##0` joins the two on one shared tick).
        let mut starts: BTreeMap<u32, (Cond, Cond)> = BTreeMap::new();
        let Some(lhs) = lhs else {
            // `##n rhs` is `1[*n] ##1 rhs`.
            for n in min..=max {
                self.terms.step()?;
                let at = start.saturating_add(n);
                if at > MAX_PROPERTY_SPAN + 1 {
                    return Err(too_long());
                }
                starts.insert(at, (Cond::True, Cond::False));
            }
            return self.after(starts, rhs);
        };
        for (&end, &cond) in &lhs {
            for n in min..=max {
                self.terms.step()?;
                let (at, fused) = match n {
                    // An empty left match has no tick to share.
                    0 if end == start => continue,
                    0 => (end - 1, true),
                    _ => (end.saturating_add(n - 1), false),
                };
                if at > MAX_PROPERTY_SPAN + 1 {
                    return Err(too_long());
                }
                let slot = starts.entry(at).or_insert((Cond::False, Cond::False));
                if fused {
                    slot.1 = self.terms.or(slot.1, cond)?;
                } else {
                    slot.0 = self.terms.or(slot.0, cond)?;
                }
            }
        }
        self.after(starts, rhs)
    }

    /// The matches of `rhs` from each tick of `starts`, under that tick's conditions.
    fn after(&mut self, starts: BTreeMap<u32, (Cond, Cond)>, rhs: &SeqExpr) -> Result<Ends, String> {
        let mut out = Ends::new();
        for (at, (any, fused)) in starts {
            let either = self.terms.or(any, fused)?;
            for (end, cond) in self.matches(rhs, at)? {
                let reach = if end > at { either } else { any };
                let both = self.terms.and(reach, cond)?;
                self.add(&mut out, end, both)?;
            }
        }
        Ok(out)
    }

    /// The matches of `s` that start at tick `start`.
    fn matches(&mut self, s: &SeqExpr, start: u32) -> Result<Ends, String> {
        self.terms.step()?;
        let mut out = Ends::new();
        match s {
            SeqExpr::Bool(e) => {
                let cond = self.leaf(start, *e)?;
                out.insert(start + 1, cond);
            }
            SeqExpr::Delay { range, lhs, rhs } => {
                let (min, max) = bounds(range, "the delay")?;
                return match lhs {
                    Some(l) => {
                        let l = self.matches(l, start)?;
                        self.delay(Some(l), start, min, max, rhs)
                    }
                    None => self.delay(None, start, min, max, rhs),
                };
            }
            SeqExpr::Fusion(a, b) => {
                let a = self.matches(a, start)?;
                return self.delay(Some(a), start, 0, 0, b);
            }
            SeqExpr::Repeat { kind: RepeatKind::Consecutive, range, seq } => {
                let (min, max) = bounds(range, "the repetition")?;
                // The matches of `count` repetitions, starting with none: the empty match.
                let mut reps = Ends::from([(start, Cond::True)]);
                for count in 0..=max {
                    self.terms.step()?;
                    if count >= min {
                        for (&end, &cond) in &reps {
                            self.add(&mut out, end, cond)?;
                        }
                    }
                    if count == max {
                        break;
                    }
                    let mut next = Ends::new();
                    for (&at, &so_far) in &reps {
                        for (end, cond) in self.matches(seq, at)? {
                            let both = self.terms.and(so_far, cond)?;
                            self.add(&mut next, end, both)?;
                        }
                    }
                    // More repetitions add nothing once a count has no match, or the same
                    // matches as the count before it (when the operand's match is empty).
                    if next.is_empty() || (next == reps && count >= min) {
                        break;
                    }
                    reps = next;
                }
            }
            SeqExpr::Repeat { kind: RepeatKind::Goto, .. } => return Err("the goto repetition [->]".into()),
            SeqExpr::Repeat { kind: RepeatKind::NonConsecutive, .. } => {
                return Err("the non-consecutive repetition [=]".into())
            }
            SeqExpr::Or(a, b) => {
                out = self.matches(a, start)?;
                for (end, cond) in self.matches(b, start)? {
                    self.add(&mut out, end, cond)?;
                }
            }
            SeqExpr::Intersect(a, b) => {
                let a = self.matches(a, start)?;
                for (end, cond) in self.matches(b, start)? {
                    if let Some(&other) = a.get(&end) {
                        let both = self.terms.and(other, cond)?;
                        self.add(&mut out, end, both)?;
                    }
                }
            }
            SeqExpr::And(a, b) => {
                // Both match from `start`; the match ends where the later of the two ends.
                let a = self.matches(a, start)?;
                let b = self.matches(b, start)?;
                let ends: BTreeSet<u32> = a.keys().chain(b.keys()).copied().collect();
                let (mut a_by, mut b_by) = (Cond::False, Cond::False);
                for end in ends {
                    let a_here = a.get(&end).copied().unwrap_or(Cond::False);
                    let b_here = b.get(&end).copied().unwrap_or(Cond::False);
                    a_by = self.terms.or(a_by, a_here)?;
                    b_by = self.terms.or(b_by, b_here)?;
                    let a_last = self.terms.and(a_here, b_by)?;
                    let b_last = self.terms.and(b_here, a_by)?;
                    let cond = self.terms.or(a_last, b_last)?;
                    self.add(&mut out, end, cond)?;
                }
            }
            SeqExpr::Within(a, b) => {
                // A match of `b` with a match of `a` somewhere inside it.
                let b = self.matches(b, start)?;
                let Some(&last) = b.keys().next_back() else { return Ok(out) };
                let mut inner = Vec::new();
                for at in start..=last {
                    inner.push(self.matches(a, at)?);
                }
                for (end, cond) in b {
                    let mut inside = Cond::False;
                    for from in inner.iter().take((end - start) as usize + 1) {
                        for (_, &c) in from.range(..=end) {
                            inside = self.terms.or(inside, c)?;
                        }
                    }
                    let both = self.terms.and(cond, inside)?;
                    self.add(&mut out, end, both)?;
                }
            }
            SeqExpr::Throughout { cond, seq } => {
                // The condition holds at every tick of the match.
                let (mut held, mut upto) = (Cond::True, start);
                for (end, matched) in self.matches(seq, start)? {
                    while upto < end {
                        let here = self.leaf(upto, *cond)?;
                        held = self.terms.and(held, here)?;
                        upto += 1;
                    }
                    let both = self.terms.and(held, matched)?;
                    self.add(&mut out, end, both)?;
                }
            }
            SeqExpr::FirstMatch(s) => {
                // A match with no match that ends before it.
                let mut earlier = Cond::False;
                for (end, cond) in self.matches(s, start)? {
                    let none = self.terms.not(earlier)?;
                    let first = self.terms.and(cond, none)?;
                    self.add(&mut out, end, first)?;
                    earlier = self.terms.or(earlier, cond)?;
                }
            }
            SeqExpr::Clocked { .. } => return Err("a sequence with its own clock".into()),
            #[allow(unreachable_patterns)]
            _ => return Err("a sequence operator".into()),
        }
        Ok(out)
    }

    /// When the sequence property `s` holds from tick `start`: some match of it exists.
    /// Also returns the tick of the last boolean any match reads.
    fn holds(&mut self, s: &SeqExpr, start: u32) -> Result<(Cond, u32), String> {
        let ends = self.matches(s, start)?;
        // IEEE 1800-2023 16.12.22: a sequence used as a property admits no empty match and
        // has to have a match.
        if ends.contains_key(&start) {
            return Err("a sequence that admits an empty match, used as a property".into());
        }
        let Some(&last) = ends.keys().next_back() else {
            return Err("a sequence that has no match, used as a property".into());
        };
        let mut any = Cond::False;
        for cond in ends.into_values() {
            any = self.terms.or(any, cond)?;
        }
        Ok((any, last - 1))
    }
}

/// The sequence a property that is only a sequence stands for.
fn as_seq(p: &PropExpr) -> Result<&SeqExpr, String> {
    match p {
        // On an infinite trace a bounded sequence always ends, so weak and strong agree.
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

/// What a property requires of one attempt, as conditions of a [`Matcher`].
struct Shape {
    /// For each tick at which the consequent has to start: when the antecedent ends there,
    /// and when the consequent then has a match.
    obligations: Vec<(Cond, Cond)>,
    /// Tick of the last boolean, counted from the start of the attempt.
    span: u32,
}

impl Matcher<'_> {
    /// `reached` is the condition under which an attempt starts at all.
    fn shape(&mut self, p: &PropExpr, reached: Cond) -> Result<Shape, String> {
        // Where the consequent starts, and when.
        let mut starts: BTreeMap<u32, Cond> = BTreeMap::new();
        let consequent = match p {
            PropExpr::OverlappedImplication { lhs, rhs } | PropExpr::NonOverlappedImplication { lhs, rhs } => {
                let overlapped = matches!(p, PropExpr::OverlappedImplication { .. });
                for (end, cond) in self.matches(lhs, 0)? {
                    // `|->` starts the consequent at the last tick of a match, so an empty
                    // match starts none. `s |=> p` is `s ##1 1 |-> p`.
                    let at = match (overlapped, end) {
                        (true, 0) => continue,
                        (true, _) => end - 1,
                        (false, _) => end,
                    };
                    let cond = self.terms.and(reached, cond)?;
                    self.add(&mut starts, at, cond)?;
                }
                if starts.is_empty() {
                    return Err("an antecedent that has no match".into());
                }
                as_seq(rhs)?
            }
            other => {
                starts.insert(0, reached);
                as_seq(other)?
            }
        };
        let mut obligations = Vec::new();
        let mut span = 0;
        for (at, when) in starts {
            let (holds, last) = self.holds(consequent, at)?;
            span = span.max(last);
            obligations.push((when, holds));
        }
        if span > MAX_PROPERTY_SPAN {
            return Err(too_long());
        }
        Ok(Shape { obligations, span })
    }
}

/// A property as conditions over its leaves, ready to be made into a check.
struct Plan {
    index: usize,
    clock: ClockSpec,
    terms: Vec<Term>,
    /// The terms the check reads.
    live: Vec<bool>,
    /// Whether the booleans have to be made two-state (a condition is negated).
    two_state: bool,
    /// The check's enable (before the warm-up and `disable iff`) and its expression.
    enable: Cond,
    expr: Cond,
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
        let mut m = Matcher { nl: self, terms: Terms::default() };
        // In procedural code under a condition: the attempt starts only when it is reached.
        let reached = match self.property_enables.get(index).copied().flatten() {
            Some(v) => m.terms.leaf(0, v)?,
            None => Cond::True,
        };
        let Shape { obligations, span } = m.shape(&prop.expr, reached)?;
        let mut terms = m.terms;
        let (enable, expr) = match (prop.kind, &obligations[..]) {
            (PropKind::Cover, _) => {
                let mut any = Cond::False;
                for &(when, holds) in &obligations {
                    let both = terms.and(when, holds)?;
                    any = terms.or(any, both)?;
                }
                (Cond::True, any)
            }
            (_, &[(when, holds)]) => (when, holds),
            _ => {
                // Checked when any obligation is due; each one that is due has to hold.
                let (mut any, mut all) = (Cond::False, Cond::True);
                for &(when, holds) in &obligations {
                    any = terms.or(any, when)?;
                    let idle = terms.not(when)?;
                    let met = terms.or(idle, holds)?;
                    all = terms.and(all, met)?;
                }
                (any, all)
            }
        };
        // The terms the check reads: operands come before the terms that use them.
        let mut live = vec![false; terms.nodes.len()];
        for root in [enable, expr] {
            if let Cond::Node(i) = root {
                live[i as usize] = true;
            }
        }
        for i in (0..terms.nodes.len()).rev() {
            if !live[i] {
                continue;
            }
            let (a, b) = match terms.nodes[i] {
                Term::Leaf { .. } => continue,
                Term::Not(a) => (Cond::Node(a), Cond::True),
                Term::And(a, b) | Term::Or(a, b) => (a, b),
            };
            for operand in [a, b] {
                if let Cond::Node(j) = operand {
                    live[j as usize] = true;
                }
            }
        }
        let disable = match prop.disable_iff {
            Some(d) => Some(self.prop_expr_value(d).ok_or_else(not_lowered)?),
            None => None,
        };
        let mut history = 0u32;
        for (i, t) in terms.nodes.iter().enumerate() {
            if let (true, &Term::Leaf { at, value }) = (live[i], t) {
                history = history.max((span - at).saturating_add(self.lookback(value, memo)));
            }
        }
        if let Some(d) = disable {
            history = history.max(span.saturating_add(self.lookback(d, memo)));
        }
        let skip = match self.value(signal).kind {
            ValueKind::Input(i) => opts
                .skip_by_clock
                .iter()
                .find(|k| k.input == self.inputs[i].name && k.posedge == (edge == Edge::Posedge))
                .map(|k| k.ticks),
            _ => None,
        }
        .unwrap_or(opts.skip_cycles);
        let warm = skip.saturating_add(span).max(history);
        Ok(Plan {
            index,
            clock: ClockSpec { signal, edge },
            terms: terms.nodes,
            live,
            two_state: terms.negated,
            enable,
            expr,
            disable,
            span,
            warm,
        })
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

    /// The conditions `p.enable` and `p.expr` as values at tick `p.span` of an attempt, each
    /// boolean read back to its own tick (`None` is true).
    fn sva_conds(&mut self, p: &Plan, loc: Loc) -> (Option<ValueId>, Option<ValueId>) {
        let mut made: Vec<Option<ValueId>> = vec![None; p.terms.len()];
        for (i, term) in p.terms.iter().enumerate() {
            if !p.live[i] {
                continue;
            }
            // Operands are never constants: `Terms` folds those away.
            let of = |made: &[Option<ValueId>], c: Cond| match c {
                Cond::Node(j) => made[j as usize].expect("an operand is made before its use"),
                Cond::False | Cond::True => unreachable!("constant operand"),
            };
            made[i] = Some(match *term {
                Term::Leaf { at, value } => {
                    let mut b = self.sva_bool(value, loc);
                    // Under a negation an unknown boolean has to count as false, as it does
                    // everywhere else by not being true.
                    if p.two_state && self.value(b).four_state {
                        let cast = ValueKind::Op { op: NetOp::CastFourState { four_state: false }, args: vec![b] };
                        b = self.sva_value(1, false, cast, loc);
                    }
                    self.sva_past(b, p.span - at, p.clock, loc)
                }
                Term::Not(a) => self.sva_op(NetOp::LogNot, vec![of(&made, Cond::Node(a))], 1, loc),
                Term::And(a, b) => self.sva_op(NetOp::LogAnd, vec![of(&made, a), of(&made, b)], 1, loc),
                Term::Or(a, b) => self.sva_op(NetOp::LogOr, vec![of(&made, a), of(&made, b)], 1, loc),
            });
        }
        let mut root = |c: Cond| match c {
            Cond::True => None,
            Cond::Node(i) => made[i as usize],
            Cond::False => Some(self.sva_const(1, 0, loc)),
        };
        let enable = root(p.enable);
        let expr = root(p.expr);
        (enable, expr)
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

    /// Compiles every property of [`Self::properties`] that is in the bounded subset into
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
            let (enable, expr) = self.sva_conds(p, loc);
            let enable = match (guard, enable) {
                (Some(g), Some(e)) => self.sva_and(Some(g), e, loc),
                (g, e) => g.or(e),
            };
            let kind = match kind {
                PropKind::Cover => CheckKind::Cover,
                PropKind::Assert => CheckKind::Assert,
                PropKind::Assume | PropKind::Restrict => CheckKind::Assume,
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
