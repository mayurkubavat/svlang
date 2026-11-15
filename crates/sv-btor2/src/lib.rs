//! A svlang [`Netlist`] written as a [Btor2](https://github.com/Boolector/btor2tools)
//! transition system, the word-level input format of model checkers such as
//! rIC3, Pono, AVR and BtorMC.
//!
//! ```
//! use sv_netlist::{lower_trees, LowerConfig};
//!
//! let tree = sv_syntax::parse_source(
//!     "module cnt (input logic clk, rst, output logic [3:0] q);\n\
//!        always_ff @(posedge clk) if (rst) q <= 4'd0; else if (q != 4'd9) q <= q + 4'd1;\n\
//!        always @(posedge clk) in_range: assert (rst || q <= 4'd9);\n\
//!      endmodule\n",
//!     "cnt.sv",
//!     Default::default(),
//! );
//! let netlist = lower_trees(&[tree], &[], &[], &LowerConfig::default()).unwrap_netlist();
//! let model = sv_btor2::write(&netlist, &sv_btor2::Options::default()).unwrap();
//! assert!(model.btor2.lines().any(|l| l.ends_with(" in_range")));
//! ```
//!
//! With the `svlang` crate this is `svlang::btor2::write(&comp.lower_netlist(..).unwrap_netlist(), ..)`.
//!
//! The modelling choices, all of them the usual ones for formal tools:
//!
//! - one model step is one clock edge: every register, memory write port,
//!   sampled-value history and clocked check must use the same clock signal
//!   and edge, or the design is rejected;
//! - with [`Options::both_edges`], a design that uses both edges of one clock is written
//!   with two model steps per clock cycle. The clock, which must be a top-level input, is
//!   held to a level that starts low and toggles on every step. A register keeps its value
//!   except in the step that ends in its edge, where it takes its next value from that
//!   step; history, memory writes and clocked checks follow their own edge the same way.
//!   Other inputs are free on every step, so they may change on either edge. A design on
//!   one edge is written as above either way ([`Model::steps_per_cycle`] says which);
//! - with [`Options::clocks`], a design on several clocks is written the same way: each
//!   clock in use must be a top-level input declared there with its wave in model steps
//!   ([`Clock`]);
//! - with declared clocks, a register may be on a clock that is computed in the design: a
//!   gated clock (`clk & en`, also through a clock-gating latch), a divided one (the output
//!   of a register), any signal. Such a clock is read once in every model step, like any
//!   other signal: the register has the data of the step before when its clock has its
//!   edge between that step and this one. So it sees the registers on other clocks as they
//!   were before an edge they share (a divided clock's registers do not see what the
//!   dividing edge changed), and an enable that changes while the gated clock's source is
//!   high makes an edge. What happens inside a step, such as a glitch shorter than a step
//!   or the order of two changes in it, is not modelled. Sampled-value history on such a
//!   clock moves the same way, and a clocked check on it is made on what the edge sampled:
//!   it fails in the step after the one whose values break it. A memory write on a
//!   computed clock is rejected;
//! - a latch is transparent within a step: its value is its data where the data assigns
//!   it and, where the data leaves it alone, what it held at the end of the step before (a
//!   state named `<prefix>_latch_<n>`). A latch whose value depends on itself within a
//!   step, through its own data or through another latch, is rejected;
//! - an asynchronous reset acts synchronously (`next = reset ? value : next`);
//! - `(* anyconst *)` holds one free value for the whole trace, `(* anyseq *)`,
//!   cut points, black-box outputs and undriven bits are free on every step;
//! - an X or Z bit of a constant (`'x`, an out-of-range read) is any value:
//!   a free input bit on every step, like an undriven bit;
//! - an immediate assertion is a `bad` line named by its label, an immediate
//!   assumption a `constraint`;
//! - a concurrent property (`assert property`, `assume property`, `cover property`) is
//!   written through the check [`Netlist::compile_properties`] made for it, under its full
//!   hierarchical name (`top.u0.a_no_overflow`). One that has no check is **not** written,
//!   and [`Model::notes`] says how many were left out. A concurrent *assumption* (`assume
//!   property`, `restrict property`) cannot be left out without changing what the assertions
//!   mean, so a design with one that has no check is [`Fail::Unsupported`];
//! - a liveness property (`req |-> s_eventually gnt`, compiled to [`Netlist::liveness`]) is
//!   a `justice` line under its name: an infinite trace that makes it true again and again
//!   is a counterexample. A liveness assumption is a `fair` line;
//! - a cover is a `bad` line that is reached when the cover is hit, with
//!   [`Options::covers`]; without it covers are left out and noted;
//! - with [`Options::reset`], a reset condition is held true for the first model steps and
//!   false after them;
//! - `$past` and friends are history registers with no initial value, so a
//!   property that reads them is only meaningful once the history is real.
//!
//! Anything this crate does not model is [`Fail::Unsupported`] (not handled
//! yet) or [`Fail::Rejected`] (would need a guess), never approximated.

use std::collections::HashMap;
use std::fmt::Write as _;

use sv_netlist::{CheckKind, ClockSpec, FreeKind, MemId, NetOp, Netlist, ValueId, ValueKind};
use sv_semantic::{BitState, CVal, SampledKind};

/// Why a netlist could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// Not handled yet.
    Unsupported(String),
    /// Would need a guess about semantics: the design is rejected.
    Rejected(String),
}

/// The written model, and notes for the log.
#[derive(Debug, Clone)]
pub struct Model {
    /// The Btor2 text.
    pub btor2: String,
    /// What was left out or is worth telling the user (unchecked concurrent assertions, no assertions at all).
    pub notes: Vec<String>,
    /// Model steps in one cycle of the fastest clock the design uses: 1 for a design on one
    /// edge of one clock, and twice that clock's [`Clock::half_period`] (2 with
    /// [`Options::both_edges`] alone) for a design on both edges or on several clocks. A
    /// depth an engine reports is in model steps.
    pub steps_per_cycle: u32,
}

/// How to write the model.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// After the model, write [`Options::trace_marker`] on a line of its own and then one
    /// `output` line per named combinational net, so a tool can show those nets in a
    /// counterexample. A tool that hands the model to an engine that rejects `output`
    /// lines cuts the text at the marker. Off by default.
    pub trace_outputs: bool,
    /// The comment line that starts the trace section (`"; trace-outputs"` by default).
    pub trace_marker: String,
    /// Prefix of the names this crate invents: `<prefix>_x_<n>` for the free input behind an
    /// X or Z constant bit and `<prefix>_past_<n>` for a history register (`"svlang"` by default).
    pub name_prefix: String,
    /// Write each cover as a `bad` line that is reached when the cover is hit, named like an
    /// assertion. Btor2 has no cover line, so a tool that sets this tells covers from
    /// assertions by name (see [`Netlist::checks`]). Off by default: covers are left out and
    /// [`Model::notes`] says how many.
    pub covers: bool,
    /// Accept a design that uses both edges of its one clock, with two model steps per clock
    /// cycle (see the crate documentation). Off by default: such a design is rejected. A
    /// design that uses one edge is written the same either way.
    pub both_edges: bool,
    /// The clocks of a design with more than one, each a top-level input with a waveform in
    /// model steps (see [`Clock`] and the crate documentation). Empty by default: a design
    /// with two clocks is rejected. A design whose registers, memory writes, histories and
    /// clocked checks all use one edge of one clock is written with one step per edge of
    /// that clock whatever is declared here.
    pub clocks: Vec<Clock>,
    /// A reset condition the model holds true for its first steps and false ever after
    /// ([`Reset`]). `None` by default: the design's reset is an input like any other.
    pub reset: Option<Reset>,
}

/// A reset that the model applies: `value` is true in each of the first `steps` model steps
/// and false in every step after them (two `constraint` lines over a step counter state,
/// `<prefix>_step`). A tool that proves properties "after reset" declares its reset this way,
/// and leaves the attempts of the reset steps out of its properties
/// (`PropertyOptions::skip_cycles` in `sv-netlist`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reset {
    /// The condition "in reset", a value of the netlist (any bit set counts as true).
    pub value: ValueId,
    /// How many model steps it holds for, from the first.
    pub steps: u32,
}

/// A clock that the model drives: a top-level input held to a square wave.
///
/// One model step is one tick. The clock is low for `half_period` ticks, then high for
/// `half_period` ticks, and so on, with the whole wave moved `phase` ticks later (it runs as
/// if it had always been running, so with a phase the first ticks are the end of a period).
/// A clock with `half_period` 1 and `phase` 0 is low in the first step and toggles on every
/// step; one with `half_period` 2 is low for two steps, then high for two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clock {
    /// The input's name, as in [`Netlist::inputs`] (`"clk"`).
    pub input: String,
    /// Ticks the clock stays at one level (at least 1).
    pub half_period: u32,
    /// Ticks the wave is moved later.
    pub phase: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            trace_outputs: false,
            trace_marker: "; trace-outputs".into(),
            name_prefix: "svlang".into(),
            covers: false,
            both_edges: false,
            clocks: Vec::new(),
            reset: None,
        }
    }
}

impl std::fmt::Display for Fail {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Fail::Unsupported(e) => write!(f, "not supported yet: {e}"),
            Fail::Rejected(e) => write!(f, "not modelled: {e}"),
        }
    }
}

impl std::error::Error for Fail {}

/// Writes `nl` as Btor2.
///
/// # Errors
///
/// [`Fail`] when the netlist uses something the model cannot express exactly, such as
/// two clocks.
pub fn write(nl: &Netlist, options: &Options) -> Result<Model, Fail> {
    let mut w = Writer::new(nl, options);
    let computed;
    (w.waves, computed, w.reads_clocks) = w.check_clocks()?;
    w.computed = computed.into_iter().collect();
    w.run(options.trace_outputs)?;
    let steps_per_cycle = w.waves.iter().map(|c| 2 * c.half_period).min().unwrap_or(1);
    Ok(Model { btor2: w.text, notes: w.notes, steps_per_cycle })
}

/// Longest run of model steps after which the declared clocks' waves may repeat.
pub const MAX_CLOCK_CYCLE: u64 = 1 << 16;

/// A clock the model drives.
struct Wave {
    signal: ValueId,
    half_period: u32,
    phase: u32,
    /// Btor2 nodes: the clock's level in this step, and whether the step ends in its
    /// rising or its falling edge.
    level: u32,
    rise: u32,
    fall: u32,
}

/// History on a computed clock: the value's node, the gate's, the edge's, and the depth.
type HistoryKey = (u32, Option<u32>, u32, u32);

struct Writer<'n> {
    nl: &'n Netlist,
    opts: &'n Options,
    text: String,
    next: u32,
    sorts: HashMap<u32, u32>,
    array_sorts: HashMap<(u32, u32), u32>,
    consts: HashMap<(u32, String), u32>,
    /// Btor2 node of each netlist value.
    node: Vec<Option<u32>>,
    width: HashMap<u32, u32>,
    /// History registers: (value node, gate node, depth) -> state.
    history: HashMap<(u32, Option<u32>, u32), u32>,
    /// `next` lines to write last (sort, state, value).
    pending: Vec<(u32, u32, u32)>,
    /// Btor2 array state of each non-blasted memory, with its sort.
    mems: HashMap<u32, (u32, u32)>,
    notes: Vec<String>,
    /// The clocks the model drives, for a design on both edges or on several clocks: one
    /// model step is then one tick. Empty for a design on one edge of one clock.
    waves: Vec<Wave>,
    /// The design has registers, sampled-value functions or checks on a clock computed in
    /// it: such a clock is read in every model step.
    reads_clocks: bool,
    /// The state that holds a value from the step before, by the value's node.
    before: HashMap<u32, u32>,
    /// History on a computed clock: (value, gate, edge, depth) to the value's node.
    history_computed: HashMap<HistoryKey, (u32, u32)>,
    /// Registers on a clock computed in the design (indices into the netlist's states).
    computed: std::collections::HashSet<usize>,
    /// Each such register written: its state index, the state that holds its value from
    /// the step before, the state that holds its data from the step before, and its value.
    computed_regs: Vec<(usize, u32, u32, u32)>,
    /// The state that holds a computed clock's value in the step before, by clock value and
    /// whether the rising edge is the one looked for.
    clock_before: HashMap<(u32, bool), u32>,
    /// Latches whose value is being written (indices into the netlist's states).
    latch_open: std::collections::HashSet<usize>,
    /// Latches written so far.
    latches: u32,
    names: HashMap<String, u32>,
    /// Free inputs made for X and Z constant bits so far.
    unknowns: u32,
}

fn unsupported<T>(s: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Unsupported(s.into()))
}

fn rejected<T>(s: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Rejected(s.into()))
}

/// A Btor2 symbol: no spaces.
fn symbol(s: &str) -> String {
    s.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}

/// The bits of `c`, MSB first, X and Z as 0.
fn bits(c: &CVal, w: u32) -> String {
    (0..w)
        .rev()
        .map(|i| match c.bit_state(i) {
            BitState::One => '1',
            _ => '0',
        })
        .collect()
}

impl<'n> Writer<'n> {
    /// True for a signal cut whole by a cut point.
    fn cut(&self, name: &str) -> bool {
        self.nl.undriven.iter().any(|u| u.free == FreeKind::Cut && u.signal == name)
    }

    fn new(nl: &'n Netlist, opts: &'n Options) -> Self {
        Writer {
            nl,
            opts,
            text: String::new(),
            next: 1,
            sorts: HashMap::new(),
            array_sorts: HashMap::new(),
            consts: HashMap::new(),
            node: vec![None; nl.values.len()],
            width: HashMap::new(),
            history: HashMap::new(),
            pending: Vec::new(),
            mems: HashMap::new(),
            notes: Vec::new(),
            waves: Vec::new(),
            reads_clocks: false,
            before: HashMap::new(),
            history_computed: HashMap::new(),
            computed: Default::default(),
            computed_regs: Vec::new(),
            clock_before: HashMap::new(),
            latch_open: Default::default(),
            latches: 0,
            names: HashMap::new(),
            unknowns: 0,
        }
    }

    // ---- clocks

    /// The clocks the model has to drive. None when every clocked element is on one edge of
    /// one clock (one model step is then one edge of it). Otherwise each clock in use must
    /// be a top-level input with a wave: declared in [`Options::clocks`], or the design's
    /// only clock with [`Options::both_edges`].
    ///
    /// With declared clocks, a register may also be on a clock computed in the design (gated,
    /// divided): the second value returned lists those registers.
    fn check_clocks(&self) -> Result<(Vec<Wave>, Vec<usize>, bool), Fail> {
        let nl = self.nl;
        let mut uses: Vec<(ClockSpec, String)> = Vec::new();
        // Registers on a clock that is not a top-level input, when clocks are declared.
        let mut computed: Vec<usize> = Vec::new();
        let is_input = |v: ValueId| matches!(nl.value(v).kind, ValueKind::Input(_));
        for (i, s) in nl.states.iter().enumerate() {
            match s.clock {
                Some(c) if !self.opts.clocks.is_empty() && !is_input(c.signal) => computed.push(i),
                Some(c) => uses.push((c, s.name.clone())),
                // A latch has no clock: it is open or closed within a step.
                None if s.latch_enable.is_some() => {}
                None => return rejected(format!("register {} with no clock (not modelled)", s.name)),
            }
        }
        for m in &nl.memories {
            for p in &m.writes {
                match p.clock {
                    Some(c) => uses.push((c, m.name.clone())),
                    None => return rejected(format!("unclocked write to memory {} (not modelled)", m.name)),
                }
            }
        }
        // Sampled-value functions and checks on a computed clock, when clocks are declared.
        let mut others: Vec<ClockSpec> = Vec::new();
        let on_computed = |c: ClockSpec| !self.opts.clocks.is_empty() && !is_input(c.signal);
        for s in &nl.sampled {
            match s.clock {
                Some(c) if on_computed(c) => others.push(c),
                Some(c) => uses.push((c, "a sampled-value function".into())),
                None => {}
            }
        }
        for c in &nl.checks {
            match c.clock {
                Some(k) if on_computed(k) => others.push(k),
                Some(k) => uses.push((k, "an assertion".into())),
                None => {}
            }
        }
        // Everything on one edge of one clock, computed or not: one step is that edge.
        let all: Vec<ClockSpec> = uses
            .iter()
            .map(|(c, _)| *c)
            .chain(computed.iter().map(|&i| nl.states[i].clock.expect("a computed clock")))
            .chain(others.iter().copied())
            .collect();
        if all.iter().all(|c| *c == all[0]) {
            return Ok((Vec::new(), Vec::new(), false));
        }
        let reads_clocks = !computed.is_empty() || !others.is_empty();
        let mut waves: Vec<Wave> = Vec::new();
        if reads_clocks {
            // A computed clock is read in every model step, so the steps have to be those
            // of the declared clocks: all of those the design has are driven.
            for k in &self.opts.clocks {
                if k.half_period == 0 {
                    return rejected(format!("clock {} is declared with a half period of 0", k.input));
                }
                if let Some(p) = nl.inputs.iter().find(|p| p.name == k.input) {
                    waves.push(Wave {
                        signal: p.value,
                        half_period: k.half_period,
                        phase: k.phase,
                        level: 0,
                        rise: 0,
                        fall: 0,
                    });
                }
            }
        }
        let first_computed;
        let (c0, n0) = match uses.first() {
            Some(u) => u,
            None => {
                let name = computed.first().map_or("an assertion".to_string(), |&i| nl.states[i].name.clone());
                first_computed = (all[0], name);
                &first_computed
            }
        };
        let one_clock = !reads_clocks && uses.iter().all(|(c, _)| c.signal == c0.signal);
        for (c, n) in &uses {
            if waves.iter().any(|w| w.signal == c.signal) {
                continue;
            }
            // The model drives the clock, so it has to be a signal nothing else drives.
            let ValueKind::Input(i) = nl.value(c.signal).kind else {
                return rejected(if one_clock {
                    format!("blocks clocked on different edges of a clock that is not a top-level input ({n0})")
                } else if self.opts.clocks.is_empty() {
                    format!(
                        "more than one clock ({n0}, {n}), and the clock of {n} is not a top-level input (no clock is declared, so a computed clock is not modelled)"
                    )
                } else {
                    format!(
                        "{n} is written on a clock computed in the design: registers, sampled-value functions and assertions are modelled on such a clock, memory writes are not"
                    )
                });
            };
            let name = &nl.inputs[i].name;
            let (half_period, phase) = match self.opts.clocks.iter().find(|k| &k.input == name) {
                Some(k) if k.half_period == 0 => {
                    return rejected(format!("clock {name} is declared with a half period of 0"));
                }
                Some(k) => (k.half_period, k.phase),
                None if one_clock && self.opts.both_edges => (1, 0),
                None if one_clock => {
                    let other = uses.iter().find(|(k, _)| k.edge != c0.edge).map_or("", |(_, n)| n.as_str());
                    return rejected(format!(
                        "blocks clocked on different edges ({n0}, {other}); every register steps on each model step"
                    ));
                }
                None if self.opts.clocks.is_empty() => {
                    let other = uses.iter().find(|(k, _)| k.signal != c0.signal).map_or("", |(_, n)| n.as_str());
                    return rejected(format!(
                        "more than one clock ({n0}, {other}); every register steps on each model step"
                    ));
                }
                None => {
                    return rejected(format!(
                        "more than one clock, and {name} (the clock of {n}) is not a declared clock"
                    ));
                }
            };
            waves.push(Wave { signal: c.signal, half_period, phase, level: 0, rise: 0, fall: 0 });
        }
        if nl.sampled.iter().any(|s| s.clock.is_none() && s.kind != SampledKind::Sampled) {
            return unsupported(
                "a sampled-value function with no clock in a design on both clock edges or several clocks",
            );
        }
        // The waves repeat after the least common multiple of their periods.
        let mut cycle: u64 = 1;
        for w in &waves {
            let p = 2 * u64::from(w.half_period);
            let (mut a, mut b) = (cycle, p);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            cycle = cycle / a * p;
            if cycle > MAX_CLOCK_CYCLE {
                return unsupported(format!(
                    "clocks whose waves repeat only after more than {MAX_CLOCK_CYCLE} model steps"
                ));
            }
        }
        Ok((waves, computed, reads_clocks))
    }

    /// True for a clock computed in the design, in a model that reads such clocks.
    fn is_computed(&self, clock: Option<ClockSpec>) -> bool {
        self.reads_clocks && clock.is_some_and(|c| !self.waves.iter().any(|w| w.signal == c.signal))
    }

    /// True when the computed clock `clock` had its edge between the step before and this
    /// one. A state (`<prefix>_clk_<n>`) holds the clock's value in the step before; no
    /// edge is seen in the first step.
    fn computed_edge(&mut self, clock: ClockSpec) -> Result<u32, Fail> {
        // The clock's logic first, operands before users (it may be reached before it is
        // written).
        for v in self.cone(clock.signal) {
            self.value(v)?;
        }
        let now = self.value(clock.signal)?;
        let now = self.bool(now);
        let rising = clock.edge == sv_netlist::Edge::Posedge;
        let was = match self.clock_before.get(&(now, rising)) {
            Some(&s) => s,
            None => {
                let k = self.clock_before.len();
                // An initial value is declared before its state, as the format asks.
                let quiet = self.konst(if rising { "1" } else { "0" }.into());
                let s = self.state(1, &format!("{}_clk_{k}", self.opts.name_prefix));
                let srt = self.sort(1);
                self.line(format!("init {srt} {s} {quiet}"));
                self.pending.push((srt, s, now));
                self.clock_before.insert((now, rising), s);
                s
            }
        };
        Ok(if rising {
            let low = self.un("not", was);
            self.bin("and", low, now)
        } else {
            let low = self.un("not", now);
            self.bin("and", was, low)
        })
    }

    /// A state that holds the value of node `v` in the step before (`<prefix>_was_<n>`),
    /// starting at 0 when `zero`.
    fn value_before(&mut self, v: u32, zero: bool) -> u32 {
        if let Some(&s) = self.before.get(&v) {
            return s;
        }
        let w = self.w(v);
        let z = zero.then(|| self.num(w, 0));
        let s = self.state(w, &format!("{}_was_{}", self.opts.name_prefix, self.before.len()));
        let srt = self.sort(w);
        if let Some(z) = z {
            self.line(format!("init {srt} {s} {z}"));
        }
        self.pending.push((srt, s, v));
        self.before.insert(v, s);
        s
    }

    /// `$past(v, depth)` on a computed clock with the edge `edge`, gated by `gate`: the
    /// history moves when the clock had its edge since the step before, taking what `v`
    /// was then (the value the edge sampled). Like a register on such a clock, each level
    /// is a choice between a state and the level below as it was.
    fn past_computed(&mut self, v: u32, gate: Option<u32>, depth: u32, edge: u32) -> u32 {
        let mut shift = edge;
        if let Some(g) = gate {
            let was = self.value_before(g, false);
            shift = self.bin("and", edge, was);
        }
        let mut below = self.value_before(v, false);
        let mut value = v;
        for d in 1..=depth {
            let key = (v, gate, edge, d);
            let (state, shown) = match self.history_computed.get(&key) {
                Some(&made) => made,
                None => {
                    let w = self.w(v);
                    let k = self.history.len() + self.history_computed.len();
                    let state = self.state(w, &format!("{}_past_{k}", self.opts.name_prefix));
                    let shown = self.ite(shift, below, state);
                    let srt = self.sort(w);
                    self.pending.push((srt, state, shown));
                    self.history_computed.insert(key, (state, shown));
                    (state, shown)
                }
            };
            below = state;
            value = shown;
        }
        value
    }

    /// The value of register `sidx`, which is on a clock computed in the design.
    ///
    /// Such a clock is read once in every model step, like any other signal. The register
    /// has the data of the step before when its clock has its edge between that step and
    /// this one, and otherwise the value it had in the step before. So its value is not a
    /// state of the model but a choice between two: `<prefix>_reg_<n>` (the value before)
    /// and `<prefix>_data_<n>` (the data before), with `<prefix>_clk_<n>` holding the
    /// clock's value before.
    fn computed_register(&mut self, sidx: usize) -> Result<u32, Fail> {
        let st = &self.nl.states[sidx];
        let (w, name) = (st.width, st.name.clone());
        let clock = st.clock.expect("a register has a clock");
        if !self.latch_open.insert(sidx) {
            return rejected(format!("register {name}: its clock is computed from its own value within a step"));
        }
        let n = self.computed_regs.len();
        let before = self.state(w, &format!("{}_reg_{n}", self.opts.name_prefix));
        if let Some(init) = st.init.as_ref().filter(|c| c.known()) {
            let c = self.konst(bits(init, w));
            let srt = self.sort(w);
            self.line(format!("init {srt} {before} {c}"));
        }
        let data = self.state(w, &format!("{}_data_{n}", self.opts.name_prefix));
        let edge = self.computed_edge(clock)?;
        let value = self.ite(edge, data, before);
        self.computed_regs.push((sidx, before, data, value));
        self.latch_open.remove(&sidx);
        Ok(value)
    }

    /// Writes the tick counter and each clock's level and edges from it.
    fn clock_waves(&mut self) {
        if self.waves.is_empty() {
            return;
        }
        let mut cycle: u64 = 1;
        for w in &self.waves {
            let p = 2 * u64::from(w.half_period);
            let (mut a, mut b) = (cycle, p);
            while b != 0 {
                (a, b) = (b, a % b);
            }
            cycle = cycle / a * p;
        }
        // Wide enough for the counter plus one period, so the sums below do not wrap.
        let bits_for = |n: u64| (64 - n.leading_zeros()).max(1);
        let tw = bits_for(cycle - 1);
        let zero = self.num(tw, 0);
        let tick = self.state(tw, &format!("{}_clock_phase", self.opts.name_prefix));
        let srt = self.sort(tw);
        self.line(format!("init {srt} {tick} {zero}"));
        let last = self.num(tw, cycle - 1);
        let at_last = self.cmp("eq", tick, last);
        let one = self.num(tw, 1);
        let up = self.bin("add", tick, one);
        let next = self.ite(at_last, zero, up);
        self.pending.push((srt, tick, next));
        for i in 0..self.waves.len() {
            let (half, phase) = (u64::from(self.waves[i].half_period), u64::from(self.waves[i].phase));
            let period = 2 * half;
            let shift = (period - phase % period) % period;
            // Where this clock is in its period: (tick + shift) mod period.
            let w = bits_for(cycle + period);
            let wide = if w == tw { tick } else { self.op("uext", w, &[tick], &(w - tw).to_string()) };
            let at = if shift == 0 && period == cycle {
                wide
            } else {
                let k = self.num(w, shift);
                let sum = self.bin("add", wide, k);
                let p = self.num(w, period);
                self.bin("urem", sum, p)
            };
            let h = self.num(w, half);
            let level = self.cmp("ugte", at, h);
            let before_rise = self.num(w, half - 1);
            let rise = self.cmp("eq", at, before_rise);
            let before_fall = self.num(w, period - 1);
            let fall = self.cmp("eq", at, before_fall);
            (self.waves[i].level, self.waves[i].rise, self.waves[i].fall) = (level, rise, fall);
        }
        let fastest = self.waves.iter().map(|w| w.half_period).min().unwrap_or(1);
        if !self.computed.is_empty() {
            self.notes.push(format!(
                "{} register(s) are on clocks computed in the design: such a clock is read once in every model step",
                self.computed.len()
            ));
        }
        self.notes.push(if self.waves.len() == 1 {
            "the design uses both edges of its clock: one model step is half a clock cycle, the clock starts low".into()
        } else {
            format!(
                "the design has {} clocks: one model step is 1/{} of the fastest clock's cycle",
                self.waves.len(),
                2 * fastest
            )
        });
    }

    /// True in the model steps that end in the active edge of `clock`; `None` when every
    /// step does (a design on one edge of one clock).
    fn fire(&mut self, clock: Option<ClockSpec>) -> Option<u32> {
        let c = clock?;
        let w = self.waves.iter().find(|w| w.signal == c.signal)?;
        Some(match c.edge {
            sv_netlist::Edge::Posedge => w.rise,
            sv_netlist::Edge::Negedge => w.fall,
        })
    }

    /// `v` only in the steps `clock` fires.
    fn on_edge(&mut self, clock: Option<ClockSpec>, v: u32) -> u32 {
        match self.fire(clock) {
            Some(f) => self.bin("and", f, v),
            None => v,
        }
    }

    // ---- Btor2 lines

    fn line(&mut self, body: String) -> u32 {
        let id = self.next;
        self.next += 1;
        writeln!(self.text, "{id} {body}").unwrap();
        id
    }

    fn sort(&mut self, w: u32) -> u32 {
        if let Some(&s) = self.sorts.get(&w) {
            return s;
        }
        let s = self.line(format!("sort bitvec {w}"));
        self.sorts.insert(w, s);
        s
    }

    fn array_sort(&mut self, iw: u32, ew: u32) -> u32 {
        if let Some(&s) = self.array_sorts.get(&(iw, ew)) {
            return s;
        }
        let (i, e) = (self.sort(iw), self.sort(ew));
        let s = self.line(format!("sort array {i} {e}"));
        self.array_sorts.insert((iw, ew), s);
        s
    }

    /// A unique Btor2 symbol based on `name`.
    fn unique(&mut self, name: &str) -> String {
        let base = symbol(name);
        let n = self.names.entry(base.clone()).or_insert(0);
        *n += 1;
        if *n == 1 {
            base
        } else {
            format!("{base}#{}", *n - 1)
        }
    }

    fn op(&mut self, name: &str, w: u32, args: &[u32], extra: &str) -> u32 {
        let s = self.sort(w);
        let mut body = format!("{name} {s}");
        for a in args {
            write!(body, " {a}").unwrap();
        }
        if !extra.is_empty() {
            write!(body, " {extra}").unwrap();
        }
        let id = self.line(body);
        self.width.insert(id, w);
        id
    }

    fn input(&mut self, w: u32, name: &str) -> u32 {
        let name = self.unique(name);
        self.op("input", w, &[], &name)
    }

    fn state(&mut self, w: u32, name: &str) -> u32 {
        let name = self.unique(name);
        self.op("state", w, &[], &name)
    }

    fn konst(&mut self, bits: String) -> u32 {
        let w = bits.len() as u32;
        if let Some(&c) = self.consts.get(&(w, bits.clone())) {
            return c;
        }
        let c = self.op("const", w, &[], &bits);
        self.consts.insert((w, bits), c);
        c
    }

    fn num(&mut self, w: u32, v: u64) -> u32 {
        let b: String = (0..w).rev().map(|i| if i < 64 && (v >> i) & 1 == 1 { '1' } else { '0' }).collect();
        self.konst(b)
    }

    fn w(&self, n: u32) -> u32 {
        self.width[&n]
    }

    fn un(&mut self, name: &str, a: u32) -> u32 {
        let w = self.w(a);
        self.op(name, w, &[a], "")
    }

    fn bin(&mut self, name: &str, a: u32, b: u32) -> u32 {
        let w = self.w(a);
        self.op(name, w, &[a, b], "")
    }

    fn cmp(&mut self, name: &str, a: u32, b: u32) -> u32 {
        self.op(name, 1, &[a, b], "")
    }

    fn slice(&mut self, a: u32, hi: u32, lo: u32) -> u32 {
        self.op("slice", hi - lo + 1, &[a], &format!("{hi} {lo}"))
    }

    fn uext(&mut self, a: u32, w: u32) -> u32 {
        let aw = self.w(a);
        if aw == w {
            return a;
        }
        self.op("uext", w, &[a], &(w - aw).to_string())
    }

    fn ite(&mut self, c: u32, t: u32, e: u32) -> u32 {
        let w = self.w(t);
        self.op("ite", w, &[c, t, e], "")
    }

    /// A constant; its X and Z bits are free on every step.
    fn constant(&mut self, c: &CVal, w: u32) -> u32 {
        let known = self.konst(bits(c, w));
        let mask: String = (0..w)
            .rev()
            .map(|i| match c.bit_state(i) {
                BitState::X | BitState::Z => '1',
                _ => '0',
            })
            .collect();
        if !mask.contains('1') {
            return known;
        }
        let k = self.unknowns;
        self.unknowns += 1;
        let free = self.input(w, &format!("{}_x_{k}", self.opts.name_prefix));
        if !mask.contains('0') {
            return free;
        }
        let m = self.konst(mask);
        let picked = self.bin("and", free, m);
        self.bin("or", known, picked)
    }

    // ---- values

    fn value(&mut self, id: ValueId) -> Result<u32, Fail> {
        if let Some(n) = self.node[id.0 as usize] {
            return Ok(n);
        }
        let n = self.lower_value(id)?;
        self.node[id.0 as usize] = Some(n);
        Ok(n)
    }

    fn lower_value(&mut self, id: ValueId) -> Result<u32, Fail> {
        let nl = self.nl;
        let v = nl.value(id);
        let w = v.width;
        Ok(match &v.kind {
            ValueKind::Input(i) => {
                let name = format!("{}.{}", nl.top, nl.inputs[*i].name);
                let input = self.input(w, &name);
                // A clock the model drives is held to its wave.
                if let Some(level) = self.waves.iter().find(|k| k.signal == id).map(|k| k.level) {
                    let level = if w == 1 { level } else { self.op("uext", w, &[level], &(w - 1).to_string()) };
                    let same = self.cmp("eq", input, level);
                    self.line(format!("constraint {same}"));
                }
                input
            }
            ValueKind::Undriven(i) => {
                let u = &nl.undriven[*i];
                let whole =
                    u.lo == 0 && u.hi + 1 == w && nl.undriven.iter().filter(|x| x.signal == u.signal).count() == 1;
                let base = if whole { u.signal.clone() } else { format!("{}[{}:{}]", u.signal, u.hi, u.lo) };
                match u.free {
                    // Chosen once, then held: a state with no init whose
                    // next value is itself.
                    FreeKind::AnyConst => {
                        let s = self.state(w, &base);
                        let srt = self.sort(w);
                        self.pending.push((srt, s, s));
                        s
                    }
                    // A cut point (`stopat`): free on every step, under
                    // the signal's own name.
                    FreeKind::AnySeq | FreeKind::Cut => self.input(w, &base),
                    FreeKind::Undriven => self.input(w, &format!("{base}_undriven")),
                }
            }
            ValueKind::Const(c) => self.constant(c, w),
            ValueKind::StateOut(s) => {
                let st = &nl.states[s.0 as usize];
                // A cut register is not a state of the model: nothing
                // reads it (its readers got the free value).
                if self.cut(&st.name) {
                    return Ok(self.konst("0".repeat(w as usize)));
                }
                if st.latch_enable.is_some() {
                    return self.latch(s.0 as usize, id);
                }
                if self.computed.contains(&(s.0 as usize)) {
                    return self.computed_register(s.0 as usize);
                }
                self.state(w, &st.name)
            }
            ValueKind::MemRead { mem, addr } => {
                let a = self.value(*addr)?;
                let m = self.memory(*mem, self.w(a))?;
                self.op("read", w, &[m, a], "")
            }
            ValueKind::Sampled(i) => self.sampled(*i)?,
            ValueKind::Op { op, args } => {
                let mut a = Vec::with_capacity(args.len());
                for x in args {
                    a.push(self.value(*x)?);
                }
                self.netop(op, &a, args, w)?
            }
        })
    }

    fn netop(&mut self, op: &NetOp, a: &[u32], ids: &[ValueId], w: u32) -> Result<u32, Fail> {
        use NetOp::*;
        Ok(match op {
            Add => self.bin("add", a[0], a[1]),
            Sub => self.bin("sub", a[0], a[1]),
            Mul => self.bin("mul", a[0], a[1]),
            UDiv => self.bin("udiv", a[0], a[1]),
            SDiv => self.bin("sdiv", a[0], a[1]),
            UMod => self.bin("urem", a[0], a[1]),
            SMod => self.bin("srem", a[0], a[1]),
            Pow => {
                let e = self.nl.value(ids[1]);
                let ValueKind::Const(c) = &e.kind else {
                    return unsupported("non-constant power");
                };
                if !c.known() || c.i() < 0 || c.i() > 4096 {
                    return unsupported("power with a negative, unknown or large exponent");
                }
                let mut r = self.num(w, 1);
                for _ in 0..c.i() {
                    r = self.bin("mul", r, a[0]);
                }
                r
            }
            And => self.bin("and", a[0], a[1]),
            Or => self.bin("or", a[0], a[1]),
            Xor => self.bin("xor", a[0], a[1]),
            Xnor => self.bin("xnor", a[0], a[1]),
            Not => self.un("not", a[0]),
            Neg => self.un("neg", a[0]),
            RedAnd => self.op("redand", 1, &[a[0]], ""),
            RedOr => self.op("redor", 1, &[a[0]], ""),
            RedXor => self.op("redxor", 1, &[a[0]], ""),
            LogNot => {
                let b = self.bool(a[0]);
                self.un("not", b)
            }
            LogAnd => {
                let (x, y) = (self.bool(a[0]), self.bool(a[1]));
                self.bin("and", x, y)
            }
            LogOr => {
                let (x, y) = (self.bool(a[0]), self.bool(a[1]));
                self.bin("or", x, y)
            }
            Shl => self.shift("sll", a[0], a[1]),
            LShr => self.shift("srl", a[0], a[1]),
            AShr => self.shift("sra", a[0], a[1]),
            Eq | CaseEq => self.cmp("eq", a[0], a[1]),
            Neq | CaseNeq => self.cmp("neq", a[0], a[1]),
            WildEq { rhs } | WildNeq { rhs } => {
                let aw = self.w(a[0]);
                let mask: String = (0..aw)
                    .rev()
                    .map(|i| match rhs.bit_state(i) {
                        BitState::Zero | BitState::One => '1',
                        _ => '0',
                    })
                    .collect();
                let m = self.konst(mask);
                let r = self.konst(bits(rhs, aw));
                let x = self.bin("and", a[0], m);
                let y = self.bin("and", r, m);
                let name = if matches!(op, WildEq { .. }) { "eq" } else { "neq" };
                self.cmp(name, x, y)
            }
            ULt => self.cmp("ult", a[0], a[1]),
            SLt => self.cmp("slt", a[0], a[1]),
            ULe => self.cmp("ulte", a[0], a[1]),
            SLe => self.cmp("slte", a[0], a[1]),
            Mux => {
                let c = self.bool(a[0]);
                self.ite(c, a[1], a[2])
            }
            Concat => {
                let mut r = a[0];
                for &x in &a[1..] {
                    let rw = self.w(r) + self.w(x);
                    r = self.op("concat", rw, &[r, x], "");
                }
                r
            }
            Slice { hi, lo } => {
                if *lo == 0 && *hi + 1 == self.w(a[0]) {
                    a[0]
                } else {
                    self.slice(a[0], *hi, *lo)
                }
            }
            ZeroExt { width } => self.uext(a[0], *width),
            SignExt { width } => {
                let aw = self.w(a[0]);
                if aw == *width {
                    a[0]
                } else {
                    self.op("sext", *width, &[a[0]], &(width - aw).to_string())
                }
            }
            CastFourState { .. } => a[0],
            CountOnes => self.count_ones(a[0]),
            OneHot => {
                let c = self.count_ones(a[0]);
                let one = self.num(32, 1);
                self.cmp("eq", c, one)
            }
            OneHot0 => {
                let c = self.count_ones(a[0]);
                let one = self.num(32, 1);
                self.cmp("ulte", c, one)
            }
            IsUnknown => return rejected("$isunknown (X and Z are not modelled: values are two-state)"),
        })
    }

    /// A 1-bit truth value (operands of logical operators are already
    /// booleanized; this only guards against a wider one).
    fn bool(&mut self, a: u32) -> u32 {
        if self.w(a) == 1 {
            a
        } else {
            self.op("redor", 1, &[a], "")
        }
    }

    /// Btor2 shifts take an amount of the operand's width; a wider amount
    /// that does not fit shifts everything out.
    fn shift(&mut self, name: &str, a: u32, b: u32) -> u32 {
        let (aw, bw) = (self.w(a), self.w(b));
        if bw <= aw {
            let b = self.uext(b, aw);
            return self.bin(name, a, b);
        }
        let low = self.slice(b, aw - 1, 0);
        let shifted = self.bin(name, a, low);
        // Amount >= aw: everything shifted out (sign fill for sra, which a
        // shift by aw - 1 gives).
        let limit = self.num(bw, aw as u64 - 1);
        let over = self.cmp("ugt", b, limit);
        let out = if name == "sra" {
            let max = self.num(aw, aw as u64 - 1);
            self.bin("sra", a, max)
        } else {
            self.num(aw, 0)
        };
        self.ite(over, out, shifted)
    }

    fn count_ones(&mut self, a: u32) -> u32 {
        let w = self.w(a);
        let mut cnt = self.num(32, 0);
        for i in 0..w {
            let b = self.slice(a, i, i);
            let b = self.uext(b, 32);
            cnt = self.bin("add", cnt, b);
        }
        cnt
    }

    // ---- latches

    /// The value of latch `sidx`, whose output is `out`: its data with every path that
    /// leaves the latch alone reading what it held at the end of the step before.
    fn latch(&mut self, sidx: usize, out: ValueId) -> Result<u32, Fail> {
        let st = &self.nl.states[sidx];
        let (w, next, name) = (st.width, st.next, st.name.clone());
        if !self.latch_open.insert(sidx) {
            return rejected(format!(
                "latch {name}: its value depends on itself within a step (a loop through the open latch, or latches that feed each other)"
            ));
        }
        let held = self.state(w, &format!("{}_latch_{}", self.opts.name_prefix, self.latches));
        self.latches += 1;
        if let Some(init) = st.init.as_ref().filter(|c| c.known()) {
            let c = self.konst(bits(init, w));
            let srt = self.sort(w);
            self.line(format!("init {srt} {held} {c}"));
        }
        let mut keeps = HashMap::new();
        // The data's logic first, operands before users, so that writing it does not recurse
        // as deep as the logic is (values are normally written in netlist order, but a
        // latch is reached before the logic that feeds it).
        for v in self.cone(next) {
            if v != out && !self.keeps(v, out, &mut keeps) {
                self.value(v)?;
            }
        }
        let q = self.latch_data(next, out, held, &mut keeps)?;
        let srt = self.sort(w);
        self.pending.push((srt, held, q));
        self.latch_open.remove(&sidx);
        Ok(q)
    }

    /// The values `root` is computed from that are not written yet, operands before users.
    /// Stops at registers and latches.
    fn cone(&self, root: ValueId) -> Vec<ValueId> {
        let nl = self.nl;
        let mut order = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut stack = vec![(root, false)];
        while let Some((v, done)) = stack.pop() {
            if done {
                order.push(v);
                continue;
            }
            if self.node[v.0 as usize].is_some() || !seen.insert(v.0) {
                continue;
            }
            stack.push((v, true));
            match &nl.value(v).kind {
                ValueKind::Op { args, .. } => stack.extend(args.iter().map(|a| (*a, false))),
                ValueKind::MemRead { addr, .. } => stack.push((*addr, false)),
                ValueKind::Sampled(i) => {
                    let s = &nl.sampled[*i];
                    stack.push((s.expr, false));
                    stack.extend(s.gate.map(|g| (g, false)));
                }
                ValueKind::Input(_) | ValueKind::Const(_) | ValueKind::StateOut(_) | ValueKind::Undriven(_) => {}
            }
        }
        order
    }

    /// True when `v` passes the latch output `out` through unchanged on some path: `out`
    /// itself, or a mux, slice or concatenation of data that does.
    fn keeps(&self, v: ValueId, out: ValueId, memo: &mut HashMap<u32, bool>) -> bool {
        if v == out {
            return true;
        }
        if let Some(&k) = memo.get(&v.0) {
            return k;
        }
        let k = match &self.nl.value(v).kind {
            ValueKind::Op { op: NetOp::Mux, args } => args[1..].iter().any(|a| self.keeps(*a, out, memo)),
            ValueKind::Op { op: NetOp::Slice { .. } | NetOp::Concat | NetOp::CastFourState { .. }, args } => {
                args.iter().any(|a| self.keeps(*a, out, memo))
            }
            _ => false,
        };
        memo.insert(v.0, k);
        k
    }

    /// `v` in the data of a latch, with the latch's own output `out` read as `held` where
    /// the data only passes it through. Anything else that reads `out` reads the latch's
    /// value in this step, which [`Self::latch`] rejects as a loop.
    fn latch_data(&mut self, v: ValueId, out: ValueId, held: u32, memo: &mut HashMap<u32, bool>) -> Result<u32, Fail> {
        if v == out {
            return Ok(held);
        }
        if !self.keeps(v, out, memo) {
            return self.value(v);
        }
        let node = self.nl.value(v);
        let ValueKind::Op { op, args } = &node.kind else {
            return self.value(v);
        };
        let mut a = Vec::with_capacity(args.len());
        for (i, x) in args.iter().enumerate() {
            // A mux select is read, not passed through.
            let select = matches!(op, NetOp::Mux) && i == 0;
            a.push(if select { self.value(*x)? } else { self.latch_data(*x, out, held, memo)? });
        }
        self.netop(op, &a, args, node.width)
    }

    // ---- history

    /// `$past(v, depth)` gated by `gate`: a chain of `depth` registers that
    /// shift on every step the gate is on.
    fn past(&mut self, v: u32, gate: Option<u32>, depth: u32) -> u32 {
        let mut cur = v;
        for d in 1..=depth {
            let key = (v, gate, d);
            let s = match self.history.get(&key) {
                Some(&s) => s,
                None => {
                    let w = self.w(v);
                    let k = self.history.len();
                    let s = self.state(w, &format!("{}_past_{k}", self.opts.name_prefix));
                    let nxt = match gate {
                        Some(g) => self.ite(g, cur, s),
                        None => cur,
                    };
                    let srt = self.sort(w);
                    self.pending.push((srt, s, nxt));
                    self.history.insert(key, s);
                    s
                }
            };
            cur = s;
        }
        cur
    }

    fn sampled(&mut self, i: usize) -> Result<u32, Fail> {
        let s = &self.nl.sampled[i];
        let (kind, depth, gate_id, expr, clock) = (s.kind, s.depth, s.gate, s.expr, s.clock);
        let v = self.value(expr)?;
        let gate = match gate_id {
            Some(g) => {
                let g = self.value(g)?;
                Some(self.bool(g))
            }
            None => None,
        };
        if self.is_computed(clock) {
            let edge = self.computed_edge(clock.expect("a computed clock"))?;
            return Ok(match kind {
                SampledKind::Sampled => v,
                SampledKind::Past => self.past_computed(v, gate, depth.max(1), edge),
                SampledKind::Rose | SampledKind::Fell => {
                    let b = self.slice_lsb(v);
                    let p = self.past_computed(b, gate, 1, edge);
                    let (now, before) =
                        if kind == SampledKind::Rose { (b, self.un("not", p)) } else { (self.un("not", b), p) };
                    self.bin("and", now, before)
                }
                SampledKind::Stable | SampledKind::Changed => {
                    let p = self.past_computed(v, gate, 1, edge);
                    let name = if kind == SampledKind::Stable { "eq" } else { "neq" };
                    self.cmp(name, v, p)
                }
            });
        }
        // On both edges, history moves only at this function's own clock edge.
        let gate = match (self.fire(clock), gate) {
            (Some(f), Some(g)) => Some(self.bin("and", f, g)),
            (Some(f), None) => Some(f),
            (None, g) => g,
        };
        Ok(match kind {
            SampledKind::Sampled => v,
            SampledKind::Past => self.past(v, gate, depth.max(1)),
            SampledKind::Rose | SampledKind::Fell => {
                let b = self.slice_lsb(v);
                let p = self.past(b, gate, 1);
                let (now, before) =
                    if kind == SampledKind::Rose { (b, self.un("not", p)) } else { (self.un("not", b), p) };
                self.bin("and", now, before)
            }
            SampledKind::Stable | SampledKind::Changed => {
                let p = self.past(v, gate, 1);
                let name = if kind == SampledKind::Stable { "eq" } else { "neq" };
                self.cmp(name, v, p)
            }
        })
    }

    fn slice_lsb(&mut self, v: u32) -> u32 {
        if self.w(v) == 1 {
            v
        } else {
            self.slice(v, 0, 0)
        }
    }

    // ---- memories

    /// The array state of a memory the netlist kept (not blasted).
    fn memory(&mut self, id: MemId, iw: u32) -> Result<u32, Fail> {
        if let Some(&(m, _)) = self.mems.get(&id.0) {
            return Ok(m);
        }
        let mem = &self.nl.memories[id.0 as usize];
        if !mem.init.is_empty() {
            return unsupported(format!(
                "initial values of memory {} kept as an array (raise the blast limit)",
                mem.name
            ));
        }
        let s = self.array_sort(iw, mem.width);
        let name = self.unique(&mem.name);
        let m = self.line(format!("state {s} {name}"));
        self.mems.insert(id.0, (m, s));
        Ok(m)
    }

    /// A `bad` line for check `c`. An immediate check is named by its label, a compiled
    /// concurrent property by its full hierarchical name.
    fn bad(&mut self, cond: u32, c: &sv_netlist::Check) {
        let label = c.label.as_deref().map(|l| if c.immediate { l.rsplit('.').next().unwrap_or(l) } else { l });
        let mut body = format!("bad {cond}");
        if let Some(l) = label {
            write!(body, " {}", symbol(l)).unwrap();
        }
        self.line(body);
    }

    // ---- driver

    fn run(&mut self, trace_outputs: bool) -> Result<(), Fail> {
        let nl = self.nl;
        // The format wants an initial value declared before the state it initialises
        // (the reference parser refuses the other order), and a register can be
        // reached from logic long before its own turn below. So the constants go first.
        for st in &nl.states {
            if let Some(init) = st.init.as_ref().filter(|c| c.known()) {
                self.konst(bits(init, st.width));
            }
        }
        self.clock_waves();
        // Values in netlist order (topological), so names come out in
        // declaration order.
        for i in 0..nl.values.len() {
            self.value(ValueId(i as u32))?;
        }
        // Concurrent properties are written through the checks Netlist::compile_properties
        // made for them; the rest are left out, which is only safe for assertions and covers.
        let left_out: Vec<&sv_semantic::BoundProperty> = nl
            .properties
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                nl.property_checks.get(*i).copied().flatten().is_none()
                    && nl.property_liveness.get(*i).copied().flatten().is_none()
            })
            .map(|(_, p)| p)
            .collect();
        if !left_out.is_empty() {
            let assumes = left_out
                .iter()
                .any(|p| matches!(p.kind, sv_semantic::PropKind::Assume | sv_semantic::PropKind::Restrict));
            if assumes {
                return unsupported(
                    "concurrent assumptions in the RTL (assume or restrict property) that are not compiled to checks: leaving them out would make the model weaker than the design says",
                );
            }
            self.notes.push(format!(
                "{} concurrent assertion(s) or cover(s) in the RTL are not written (see Netlist::compile_properties)",
                left_out.len()
            ));
        }
        // Registers.
        for st in &nl.states {
            if self.cut(&st.name) {
                continue;
            }
            // A latch: its value and held state are written where it is read. So is a
            // register on a computed clock (its next values follow below).
            if st.latch_enable.is_some() || self.computed.contains(&(st.id.0 as usize)) {
                self.value(st.out)?;
                continue;
            }
            let s = self.value(st.out)?;
            // An initial value with X or Z bits: no init (any value).
            if let Some(init) = st.init.as_ref().filter(|c| c.known()) {
                let c = self.konst(bits(init, st.width));
                let srt = self.sort(st.width);
                self.line(format!("init {srt} {s} {c}"));
            }
            let mut n = self.value(st.next)?;
            if let Some(r) = st.async_reset {
                let sig = self.value(r.signal)?;
                let sig = self.bool(sig);
                let on = if r.active_high { sig } else { self.un("not", sig) };
                let val = self.value(r.value)?;
                n = self.ite(on, val, n);
            }
            if let Some(f) = self.fire(st.clock) {
                n = self.ite(f, n, s);
            }
            let srt = self.sort(st.width);
            self.pending.push((srt, s, n));
        }
        // A register on a computed clock: it keeps its value of this step, and its data of
        // this step (an asynchronous reset acts as part of the data, as for any register).
        for i in 0..self.computed_regs.len() {
            let (sidx, before, data, value) = self.computed_regs[i];
            let st = &nl.states[sidx];
            let mut n = self.value(st.next)?;
            if let Some(r) = st.async_reset {
                let sig = self.value(r.signal)?;
                let sig = self.bool(sig);
                let on = if r.active_high { sig } else { self.un("not", sig) };
                let val = self.value(r.value)?;
                n = self.ite(on, val, n);
            }
            let srt = self.sort(st.width);
            self.pending.push((srt, before, value));
            self.pending.push((srt, data, n));
        }
        // Memory writes: later ports win.
        for (k, mem) in nl.memories.iter().enumerate() {
            // Written but never read: nothing observes it.
            let Some(&(m, srt)) = self.mems.get(&(k as u32)) else {
                continue;
            };
            let mut cur = m;
            for p in &mem.writes {
                let a = self.value(p.addr)?;
                let d = self.value(p.data)?;
                let e = self.value(p.enable)?;
                let e = self.bool(e);
                let e = self.on_edge(p.clock, e);
                let wr = self.line(format!("write {srt} {cur} {a} {d}"));
                let pick = self.line(format!("ite {srt} {e} {wr} {cur}"));
                cur = pick;
            }
            self.pending.push((srt, m, cur));
        }
        // Checks.
        let mut bads = 0;
        let mut covers_left_out = 0;
        for c in &nl.checks {
            let en = self.value(c.enable)?;
            let en = self.bool(en);
            let ex = self.value(c.expr)?;
            let ex = self.bool(ex);
            if self.is_computed(c.clock) {
                // On a computed clock the edge is seen one step late, so the check is on
                // what the edge sampled: whether it failed (or was hit) in the step before.
                let edge = self.computed_edge(c.clock.expect("a computed clock"))?;
                let cover = c.kind == CheckKind::Cover;
                if cover && !self.opts.covers {
                    covers_left_out += 1;
                    continue;
                }
                let then = if cover { ex } else { self.un("not", ex) };
                let then = self.bin("and", en, then);
                let was = self.value_before(then, true);
                let at_edge = self.bin("and", edge, was);
                if c.kind == CheckKind::Assume {
                    let ok = self.un("not", at_edge);
                    self.line(format!("constraint {ok}"));
                } else {
                    self.bad(at_edge, c);
                    bads += usize::from(!cover);
                }
                continue;
            }
            // On both edges, a clocked check is looked at only at its own clock edge.
            let en = self.on_edge(c.clock, en);
            match c.kind {
                CheckKind::Assert => {
                    let nex = self.un("not", ex);
                    let bad = self.bin("and", en, nex);
                    self.bad(bad, c);
                    bads += 1;
                }
                CheckKind::Assume => {
                    let nen = self.un("not", en);
                    let ok = self.bin("or", nen, ex);
                    self.line(format!("constraint {ok}"));
                }
                CheckKind::Cover if self.opts.covers => {
                    let hit = self.bin("and", en, ex);
                    self.bad(hit, c);
                }
                CheckKind::Cover => covers_left_out += 1,
            }
        }
        // The reset: held for the first steps, off after them.
        if let Some(r) = self.opts.reset {
            let on = self.value(r.value)?;
            let on = self.bool(on);
            let off = self.un("not", on);
            if r.steps == 0 {
                self.line(format!("constraint {off}"));
            } else {
                let w = (32 - r.steps.leading_zeros()).max(1);
                let zero = self.num(w, 0);
                let step = self.state(w, &format!("{}_step", self.opts.name_prefix));
                let srt = self.sort(w);
                self.line(format!("init {srt} {step} {zero}"));
                let last = self.num(w, u64::from(r.steps));
                let early = self.cmp("ult", step, last);
                let one = self.num(w, 1);
                let up = self.bin("add", step, one);
                let next = self.ite(early, up, step);
                self.pending.push((srt, step, next));
                let held = self.ite(early, on, off);
                self.line(format!("constraint {held}"));
            }
        }
        // Liveness: a `justice` line that a trace hitting it infinitely often witnesses, or
        // a `fair` line that every such trace has to hit infinitely often.
        for l in &nl.liveness {
            let v = self.value(l.signal)?;
            let v = self.bool(v);
            if l.assume {
                self.line(format!("fair {v}"));
            } else {
                let mut body = format!("justice 1 {v}");
                if let Some(name) = &l.label {
                    write!(body, " {}", symbol(name)).unwrap();
                }
                self.line(body);
                bads += 1;
            }
        }
        if covers_left_out > 0 {
            self.notes.push(format!("{covers_left_out} cover(s) in the RTL are not written (see Options::covers)"));
        }
        // `next` lines last; history registers may still be added while
        // lowering them, so loop by index.
        let mut i = 0;
        while i < self.pending.len() {
            let (srt, s, n) = self.pending[i];
            self.line(format!("next {srt} {s} {n}"));
            i += 1;
        }
        if bads == 0 {
            self.notes.push("the design has no assertions".into());
        }
        if trace_outputs {
            self.text.push_str(&self.opts.trace_marker);
            self.text.push('\n');
            let mut named: Vec<(String, ValueId)> = Vec::new();
            for t in &nl.traces {
                match nl.value(t.value).kind {
                    ValueKind::Input(_) => {}
                    // A register is a named state already; a latch's value is not.
                    ValueKind::StateOut(s)
                        if nl.states[s.0 as usize].latch_enable.is_none()
                            && !self.computed.contains(&(s.0 as usize)) => {}
                    _ => named.push((t.name.clone(), t.value)),
                }
            }
            named.sort();
            named.dedup_by(|a, b| a.0 == b.0);
            for (name, v) in named {
                let n = self.value(v)?;
                let name = symbol(&name);
                self.line(format!("output {n} {name}"));
            }
        }
        Ok(())
    }
}
