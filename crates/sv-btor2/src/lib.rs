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
//! - a cover is a `bad` line that is reached when the cover is hit, with
//!   [`Options::covers`]; without it covers are left out and noted;
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
    /// Model steps in one clock cycle: 1, or 2 for a design on both edges of its clock
    /// written with [`Options::both_edges`]. A depth an engine reports is in model steps.
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
}

impl Default for Options {
    fn default() -> Self {
        Self {
            trace_outputs: false,
            trace_marker: "; trace-outputs".into(),
            name_prefix: "svlang".into(),
            covers: false,
            both_edges: false,
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
    w.half_cycle = w.check_clocks()?;
    w.run(options.trace_outputs)?;
    let steps_per_cycle = if w.half_cycle.is_some() { 2 } else { 1 };
    Ok(Model { btor2: w.text, notes: w.notes, steps_per_cycle })
}

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
    /// The clock of a design on both edges: one model step is half a cycle of it.
    half_cycle: Option<ValueId>,
    /// With `half_cycle`: the clock level in this step (a state, 0 first, toggling).
    phase: Option<u32>,
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
            half_cycle: None,
            phase: None,
            latch_open: Default::default(),
            latches: 0,
            names: HashMap::new(),
            unknowns: 0,
        }
    }

    // ---- clocks

    /// Every clocked element on one clock and edge: one model step is one
    /// edge of that clock. With [`Options::both_edges`], one clock on both
    /// edges is accepted too: the clock is returned, and one model step is
    /// then half a cycle of it.
    fn check_clocks(&self) -> Result<Option<ValueId>, Fail> {
        let nl = self.nl;
        let mut uses: Vec<(ClockSpec, String)> = Vec::new();
        for s in &nl.states {
            match s.clock {
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
        for s in &nl.sampled {
            if let Some(c) = s.clock {
                uses.push((c, "a sampled-value function".into()));
            }
        }
        for c in &nl.checks {
            if let Some(k) = c.clock {
                uses.push((k, "an assertion".into()));
            }
        }
        let Some((c0, n0)) = uses.first() else {
            return Ok(None);
        };
        let mut both = false;
        for (c, n) in &uses {
            if c.signal != c0.signal {
                return rejected(format!("more than one clock ({n0}, {n}); every register steps on each model step"));
            }
            if c.edge != c0.edge {
                if !self.opts.both_edges {
                    return rejected(format!(
                        "blocks clocked on different edges ({n0}, {n}); every register steps on each model step"
                    ));
                }
                both = true;
            }
        }
        if !both {
            return Ok(None);
        }
        // The clock is driven by the model, so it has to be a signal nothing else drives.
        if !matches!(nl.value(c0.signal).kind, ValueKind::Input(_)) {
            return rejected(format!(
                "blocks clocked on different edges of a clock that is not a top-level input ({n0})"
            ));
        }
        if nl.sampled.iter().any(|s| s.clock.is_none() && s.kind != SampledKind::Sampled) {
            return unsupported("a sampled-value function with no clock in a design on both clock edges");
        }
        Ok(Some(c0.signal))
    }

    /// True in the model steps that end in the active edge of `clock`; `None` when every
    /// step does (a design on one edge).
    fn fire(&mut self, clock: Option<ClockSpec>) -> Option<u32> {
        let phase = self.phase?;
        Some(match clock?.edge {
            sv_netlist::Edge::Posedge => self.un("not", phase),
            sv_netlist::Edge::Negedge => phase,
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
                // The clock of a design on both edges: low in the first step, then toggling.
                if let (Some(phase), true) = (self.phase, self.half_cycle == Some(id)) {
                    let level = if w == 1 { phase } else { self.op("uext", w, &[phase], &(w - 1).to_string()) };
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
        if self.half_cycle.is_some() {
            let phase = self.state(1, &format!("{}_clock_phase", self.opts.name_prefix));
            let zero = self.konst("0".into());
            let srt = self.sort(1);
            self.line(format!("init {srt} {phase} {zero}"));
            let high = self.un("not", phase);
            self.pending.push((srt, phase, high));
            self.phase = Some(phase);
            self.notes.push(
                "the design uses both edges of its clock: one model step is half a clock cycle, the clock starts low"
                    .into(),
            );
        }
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
            .filter(|(i, _)| nl.property_checks.get(*i).copied().flatten().is_none())
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
            // A latch: its value and held state are written where it is read.
            if st.latch_enable.is_some() {
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
            // On both edges, a clocked check is looked at only at its own clock edge.
            let en = self.on_edge(c.clock, en);
            let ex = self.value(c.expr)?;
            let ex = self.bool(ex);
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
                    ValueKind::StateOut(s) if nl.states[s.0 as usize].latch_enable.is_none() => {}
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
