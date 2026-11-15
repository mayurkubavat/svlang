//! A svlang [`Netlist`] written in the JSON netlist format of
//! [Yosys](https://yosyshq.net/yosys/) (`read_json` / `write_json`), using
//! Yosys's own word-level cells (`$add`, `$mux`, `$dff`, ...). Yosys can then
//! synthesize it, prove it equivalent to another netlist, draw it, or hand it
//! to SymbiYosys.
//!
//! ```
//! use sv_netlist::{lower_trees, LowerConfig};
//!
//! let tree = sv_syntax::parse_source(
//!     "module cnt (input logic clk, rst, output logic [3:0] q);\n\
//!        always_ff @(posedge clk) if (rst) q <= 4'd0; else q <= q + 4'd1;\n\
//!      endmodule\n",
//!     "cnt.sv",
//!     Default::default(),
//! );
//! let netlist = lower_trees(&[tree], &[], &[], &LowerConfig::default()).unwrap_netlist();
//! let model = sv_yosys::write(&netlist, &sv_yosys::Options::default()).unwrap();
//! assert!(model.json.contains("\"$dff\""));
//! // yosys -p 'read_json cnt.json; synth; stat'
//! ```
//!
//! The netlist is flat, so the file holds one module, named after the top
//! module, with the top module's ports. What is written:
//!
//! - every operator as the Yosys cell of the same meaning; slices,
//!   concatenations and extensions are wiring and make no cell;
//! - a register as `$dff`, with an asynchronous reset `$adff` (constant reset
//!   value) or `$aldff` (any value), a latch as `$dlatch` or `$adlatch`; an
//!   initial value is the `init` attribute of the register's wire;
//! - `$past` and the other sampled-value functions as registers on their clock;
//! - `(* anyconst *)` as `$anyconst`, `(* anyseq *)`, cut points and black-box
//!   outputs as `$anyseq`; an undriven bit is left undriven, and an X or Z
//!   constant bit is the constant `x` or `z`;
//! - a check as a `$assert`, `$assume` or `$cover` cell, named by its label;
//!   concurrent properties through the checks `Netlist::compile_properties`
//!   made; the others, and liveness obligations (`s_eventually`), are left out
//!   and [`Model::notes`] says how many;
//! - registers and the design's named signals as wires under their
//!   hierarchical names (`top.u0.count`), and the source location of each
//!   cell as its `src` attribute.
//!
//! Memories must be expanded into registers first
//! ([`LowerConfig::blast_memories_up_to`](sv_netlist::LowerConfig)). Anything
//! that cannot be written with its exact meaning is a [`Fail`].

use std::collections::HashSet;
use std::fmt::Write as _;

use sv_netlist::{CheckKind, ClockSpec, Edge, FreeKind, NetOp, Netlist, ValueId, ValueKind};
use sv_semantic::{BitState, CVal, Loc, SampledKind};

/// Why a netlist could not be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// Not handled yet.
    Unsupported(String),
    /// Has no exact Yosys form: the design is rejected.
    Rejected(String),
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

/// How to write the netlist.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// Write the `src` attribute (`file.sv:12.5`) on cells. On by default.
    pub src_attributes: bool,
    /// Write the checks as `$assert`, `$assume` and `$cover` cells. On by default; a
    /// synthesis flow that does not want them turns it off (or runs `chformal -remove`).
    pub checks: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { src_attributes: true, checks: true }
    }
}

/// The written netlist, and notes for the log.
#[derive(Debug, Clone)]
pub struct Model {
    /// The Yosys JSON text.
    pub json: String,
    /// What was left out or is worth telling the user.
    pub notes: Vec<String>,
}

/// Writes `nl` as a Yosys JSON netlist.
///
/// # Errors
///
/// [`Fail`] when the netlist holds something with no exact Yosys form here: a memory that
/// was not expanded, `$isunknown`, a power with a non-constant exponent.
pub fn write(nl: &Netlist, options: &Options) -> Result<Model, Fail> {
    let mut w = Writer::new(nl, options);
    w.run()?;
    Ok(Model { json: w.json(), notes: w.notes })
}

/// One bit of a signal: a constant or a net.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bit {
    Const(char),
    Net(u32),
}

type Bits = Vec<Bit>;

enum Param {
    Int(u32),
    /// A bit string, most significant bit first.
    Bits(String),
}

struct Cell {
    name: String,
    ty: &'static str,
    params: Vec<(&'static str, Param)>,
    /// Port, whether it is an output, and its bits.
    ports: Vec<(&'static str, bool, Bits)>,
    loc: Loc,
}

struct NetName {
    name: String,
    bits: Bits,
    init: Option<String>,
}

/// A history register to add once every value has its bits.
struct History {
    q: Bits,
    /// The data: a value's bits, or the bits of the stage before.
    d: Result<ValueId, Bits>,
    clock: ClockSpec,
    gate: Option<ValueId>,
    loc: Loc,
}

struct Writer<'n> {
    nl: &'n Netlist,
    opts: &'n Options,
    next_bit: u32,
    bits: Vec<Option<Bits>>,
    cells: Vec<Cell>,
    netnames: Vec<NetName>,
    history: Vec<History>,
    names: HashSet<String>,
    notes: Vec<String>,
}

fn unsupported<T>(s: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Unsupported(s.into()))
}

fn rejected<T>(s: impl Into<String>) -> Result<T, Fail> {
    Err(Fail::Rejected(s.into()))
}

fn const_bits(c: &CVal, w: u32) -> Bits {
    (0..w)
        .map(|i| {
            Bit::Const(match c.bit_state(i) {
                BitState::Zero => '0',
                BitState::One => '1',
                BitState::X => 'x',
                BitState::Z => 'z',
            })
        })
        .collect()
}

/// `bits` as a Yosys constant string, most significant bit first.
fn bit_string(bits: &[Bit]) -> Option<String> {
    bits.iter()
        .rev()
        .map(|b| match b {
            Bit::Const(c) => Some(*c),
            Bit::Net(_) => None,
        })
        .collect()
}

fn number(value: u64, w: u32) -> Bits {
    (0..w).map(|i| Bit::Const(if i < 64 && value >> i & 1 == 1 { '1' } else { '0' })).collect()
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_bits(bits: &[Bit]) -> String {
    let parts: Vec<String> = bits
        .iter()
        .map(|b| match b {
            Bit::Const(c) => format!("\"{c}\""),
            Bit::Net(n) => n.to_string(),
        })
        .collect();
    format!("[{}]", parts.join(", "))
}

impl<'n> Writer<'n> {
    fn new(nl: &'n Netlist, opts: &'n Options) -> Self {
        Writer {
            nl,
            opts,
            // Yosys keeps 0 and 1 for the constants.
            next_bit: 2,
            bits: vec![None; nl.values.len()],
            cells: Vec::new(),
            netnames: Vec::new(),
            history: Vec::new(),
            names: HashSet::new(),
            notes: Vec::new(),
        }
    }

    fn fresh(&mut self, w: u32) -> Bits {
        let first = self.next_bit;
        self.next_bit += w;
        (first..first + w).map(Bit::Net).collect()
    }

    /// `name`, or `name$2`, `name$3`, ... when it is taken.
    fn unique(&mut self, name: &str) -> String {
        let mut n = name.to_owned();
        let mut k = 2;
        while !self.names.insert(n.clone()) {
            n = format!("{name}${k}");
            k += 1;
        }
        n
    }

    fn get(&self, id: ValueId) -> Bits {
        self.bits[id.0 as usize].clone().expect("values are written in topological order")
    }

    /// A cell with one output `Y` of `w` bits; returns the output.
    fn cell(
        &mut self,
        ty: &'static str,
        params: Vec<(&'static str, Param)>,
        inputs: Vec<(&'static str, Bits)>,
        w: u32,
        loc: Loc,
    ) -> Bits {
        let y = self.fresh(w);
        let mut ports: Vec<(&'static str, bool, Bits)> = inputs.into_iter().map(|(n, b)| (n, false, b)).collect();
        ports.push(("Y", true, y.clone()));
        let name = format!("{ty}${}", self.cells.len());
        self.cells.push(Cell { name, ty, params, ports, loc });
        y
    }

    fn unary(&mut self, ty: &'static str, a: Bits, signed: bool, w: u32, loc: Loc) -> Bits {
        let params = vec![
            ("A_SIGNED", Param::Int(signed.into())),
            ("A_WIDTH", Param::Int(a.len() as u32)),
            ("Y_WIDTH", Param::Int(w)),
        ];
        self.cell(ty, params, vec![("A", a)], w, loc)
    }

    fn binary(&mut self, ty: &'static str, a: Bits, b: Bits, signed: bool, w: u32, loc: Loc) -> Bits {
        self.binary_signs(ty, a, b, (signed, signed), w, loc)
    }

    fn binary_signs(&mut self, ty: &'static str, a: Bits, b: Bits, signed: (bool, bool), w: u32, loc: Loc) -> Bits {
        let params = vec![
            ("A_SIGNED", Param::Int(signed.0.into())),
            ("A_WIDTH", Param::Int(a.len() as u32)),
            ("B_SIGNED", Param::Int(signed.1.into())),
            ("B_WIDTH", Param::Int(b.len() as u32)),
            ("Y_WIDTH", Param::Int(w)),
        ];
        self.cell(ty, params, vec![("A", a), ("B", b)], w, loc)
    }

    /// A 1-bit truth value.
    fn truth(&mut self, a: Bits, loc: Loc) -> Bits {
        if a.len() == 1 {
            a
        } else {
            self.unary("$reduce_bool", a, false, 1, loc)
        }
    }

    fn count_ones(&mut self, a: &[Bit], loc: Loc) -> Bits {
        let mut count = number(0, 32);
        for &b in a {
            count = self.binary("$add", count, vec![b], false, 32, loc);
        }
        count
    }

    fn netop(&mut self, op: &NetOp, ids: &[ValueId], w: u32, loc: Loc) -> Result<Bits, Fail> {
        use NetOp::*;
        let a: Vec<Bits> = ids.iter().map(|&i| self.get(i)).collect();
        let arg = |i: usize| a[i].clone();
        Ok(match op {
            Add => self.binary("$add", arg(0), arg(1), false, w, loc),
            Sub => self.binary("$sub", arg(0), arg(1), false, w, loc),
            Mul => self.binary("$mul", arg(0), arg(1), false, w, loc),
            UDiv => self.binary("$div", arg(0), arg(1), false, w, loc),
            SDiv => self.binary("$div", arg(0), arg(1), true, w, loc),
            UMod => self.binary("$mod", arg(0), arg(1), false, w, loc),
            SMod => self.binary("$mod", arg(0), arg(1), true, w, loc),
            Pow => {
                let ValueKind::Const(c) = &self.nl.value(ids[1]).kind else {
                    return unsupported("non-constant power");
                };
                if !c.known() || c.i() < 0 || c.i() > 4096 {
                    return unsupported("power with a negative, unknown or large exponent");
                }
                let mut r = number(1, w);
                for _ in 0..c.i() {
                    r = self.binary("$mul", r, arg(0), false, w, loc);
                }
                r
            }
            And => self.binary("$and", arg(0), arg(1), false, w, loc),
            Or => self.binary("$or", arg(0), arg(1), false, w, loc),
            Xor => self.binary("$xor", arg(0), arg(1), false, w, loc),
            Xnor => self.binary("$xnor", arg(0), arg(1), false, w, loc),
            Not => self.unary("$not", arg(0), false, w, loc),
            Neg => self.unary("$neg", arg(0), false, w, loc),
            RedAnd => self.unary("$reduce_and", arg(0), false, 1, loc),
            RedOr => self.unary("$reduce_or", arg(0), false, 1, loc),
            RedXor => self.unary("$reduce_xor", arg(0), false, 1, loc),
            LogNot => self.unary("$logic_not", arg(0), false, 1, loc),
            LogAnd => self.binary("$logic_and", arg(0), arg(1), false, 1, loc),
            LogOr => self.binary("$logic_or", arg(0), arg(1), false, 1, loc),
            Shl => self.binary("$shl", arg(0), arg(1), false, w, loc),
            LShr => self.binary("$shr", arg(0), arg(1), false, w, loc),
            // The amount is unsigned; the sign of the shifted value fills.
            AShr => self.binary_signs("$sshr", arg(0), arg(1), (true, false), w, loc),
            Eq => self.binary("$eq", arg(0), arg(1), false, 1, loc),
            Neq => self.binary("$ne", arg(0), arg(1), false, 1, loc),
            CaseEq => self.binary("$eqx", arg(0), arg(1), false, 1, loc),
            CaseNeq => self.binary("$nex", arg(0), arg(1), false, 1, loc),
            WildEq { rhs } | WildNeq { rhs } => {
                // Only the bits the pattern fixes are compared.
                let (mut left, mut right) = (Bits::new(), Bits::new());
                for (i, &b) in a[0].iter().enumerate() {
                    match rhs.bit_state(i as u32) {
                        BitState::Zero => (left.push(b), right.push(Bit::Const('0'))),
                        BitState::One => (left.push(b), right.push(Bit::Const('1'))),
                        BitState::X | BitState::Z => continue,
                    };
                }
                let eq = matches!(op, WildEq { .. });
                if left.is_empty() {
                    vec![Bit::Const(if eq { '1' } else { '0' })]
                } else {
                    self.binary(if eq { "$eq" } else { "$ne" }, left, right, false, 1, loc)
                }
            }
            ULt => self.binary("$lt", arg(0), arg(1), false, 1, loc),
            SLt => self.binary("$lt", arg(0), arg(1), true, 1, loc),
            ULe => self.binary("$le", arg(0), arg(1), false, 1, loc),
            SLe => self.binary("$le", arg(0), arg(1), true, 1, loc),
            Mux => {
                let s = self.truth(arg(0), loc);
                // $mux gives B when S is set.
                self.cell("$mux", vec![("WIDTH", Param::Int(w))], vec![("A", arg(2)), ("B", arg(1)), ("S", s)], w, loc)
            }
            // The first operand is the most significant.
            Concat => a.iter().rev().flatten().copied().collect(),
            Slice { hi, lo } => a[0][*lo as usize..=*hi as usize].to_vec(),
            ZeroExt { width } => {
                let mut r = arg(0);
                r.resize(*width as usize, Bit::Const('0'));
                r
            }
            SignExt { width } => {
                let mut r = arg(0);
                let sign = *r.last().expect("a value has at least one bit");
                r.resize(*width as usize, sign);
                r
            }
            CastFourState { .. } => arg(0),
            CountOnes => self.count_ones(&a[0], loc),
            OneHot => {
                let c = self.count_ones(&a[0], loc);
                self.binary("$eq", c, number(1, 32), false, 1, loc)
            }
            OneHot0 => {
                let c = self.count_ones(&a[0], loc);
                self.binary("$le", c, number(1, 32), false, 1, loc)
            }
            IsUnknown => return rejected("$isunknown has no Yosys cell"),
        })
    }

    /// True for a signal cut whole by a cut point.
    fn cut(&self, name: &str) -> bool {
        self.nl.undriven.iter().any(|u| u.free == FreeKind::Cut && u.signal == name)
    }

    /// A register chain `depth` deep behind `expr`, added to the netlist at the end.
    fn past(&mut self, expr: ValueId, w: u32, depth: u32, clock: ClockSpec, gate: Option<ValueId>, loc: Loc) -> Bits {
        let mut d: Result<ValueId, Bits> = Ok(expr);
        let mut q = Bits::new();
        for _ in 0..depth {
            q = self.fresh(w);
            self.history.push(History { q: q.clone(), d, clock, gate, loc });
            d = Err(q.clone());
        }
        q
    }

    fn sampled(&mut self, i: usize) -> Result<Bits, Fail> {
        let s = &self.nl.sampled[i];
        let (kind, expr, depth, gate, loc) = (s.kind, s.expr, s.depth, s.gate, s.loc);
        let w = self.nl.value(expr).width;
        if kind == SampledKind::Sampled {
            return Ok(self.get(expr));
        }
        let Some(clock) = s.clock else {
            return rejected("a sampled-value function ($past, $rose, ...) with no clock");
        };
        if kind == SampledKind::Past {
            return Ok(self.past(expr, w, depth.max(1), clock, gate, loc));
        }
        // The others compare the value now with the value one tick ago.
        let now = self.get(expr);
        let before = self.past(expr, w, 1, clock, gate, loc);
        Ok(match kind {
            SampledKind::Rose => {
                let not_before = self.unary("$not", vec![before[0]], false, 1, loc);
                self.binary("$and", vec![now[0]], not_before, false, 1, loc)
            }
            SampledKind::Fell => {
                let not_now = self.unary("$not", vec![now[0]], false, 1, loc);
                self.binary("$and", not_now, vec![before[0]], false, 1, loc)
            }
            SampledKind::Stable => self.binary("$eq", now, before, false, 1, loc),
            _ => self.binary("$ne", now, before, false, 1, loc),
        })
    }

    fn value(&mut self, id: ValueId) -> Result<Bits, Fail> {
        let nl = self.nl;
        let v = nl.value(id);
        let w = v.width;
        Ok(match &v.kind {
            ValueKind::Input(_) | ValueKind::StateOut(_) => self.fresh(w),
            ValueKind::Const(c) => const_bits(c, w),
            ValueKind::Undriven(i) => {
                let u = &nl.undriven[*i];
                let whole =
                    u.lo == 0 && u.hi + 1 == w && nl.undriven.iter().filter(|x| x.signal == u.signal).count() == 1;
                let base = if whole { u.signal.clone() } else { format!("{}[{}:{}]", u.signal, u.hi, u.lo) };
                let ty = match u.free {
                    FreeKind::AnyConst => Some("$anyconst"),
                    FreeKind::AnySeq | FreeKind::Cut => Some("$anyseq"),
                    FreeKind::Undriven => None,
                };
                let bits = match ty {
                    Some(ty) => self.cell(ty, vec![("WIDTH", Param::Int(w))], Vec::new(), w, u.loc),
                    None => self.fresh(w),
                };
                let name = self.unique(&base);
                self.netnames.push(NetName { name, bits: bits.clone(), init: None });
                bits
            }
            ValueKind::MemRead { mem, .. } => {
                return unsupported(format!(
                    "memory {} kept as an array (lower with blast_memories_up_to)",
                    nl.memories[mem.0 as usize].name
                ));
            }
            ValueKind::Sampled(i) => self.sampled(*i)?,
            ValueKind::Op { op, args } => {
                let bits = self.netop(op, args, w, v.loc)?;
                debug_assert_eq!(bits.len(), w as usize, "{op:?}");
                bits
            }
        })
    }

    fn clock_ports(&self, c: ClockSpec) -> (Bits, Param) {
        (self.get(c.signal), Param::Int((c.edge == Edge::Posedge).into()))
    }

    fn registers(&mut self) -> Result<(), Fail> {
        let nl = self.nl;
        for st in &nl.states {
            if self.cut(&st.name) {
                continue;
            }
            let (q, d, w) = (self.get(st.out), self.get(st.next), st.width);
            let mut params = vec![("WIDTH", Param::Int(w))];
            let mut ports: Vec<(&'static str, bool, Bits)> = Vec::new();
            let reset = match st.async_reset {
                Some(r) => {
                    let value = self.get(r.value);
                    Some((self.get(r.signal), Param::Int(r.active_high.into()), bit_string(&value), value))
                }
                None => None,
            };
            let ty = match (st.clock, st.latch_enable) {
                (Some(clock), None) => {
                    let (clk, polarity) = self.clock_ports(clock);
                    params.push(("CLK_POLARITY", polarity));
                    ports.push(("CLK", false, clk));
                    match reset {
                        None => "$dff",
                        Some((signal, polarity, Some(constant), _)) => {
                            params.push(("ARST_POLARITY", polarity));
                            params.push(("ARST_VALUE", Param::Bits(constant)));
                            ports.push(("ARST", false, signal));
                            "$adff"
                        }
                        // A reset to a value that is not a constant is an asynchronous load.
                        Some((signal, polarity, None, value)) => {
                            params.push(("ALOAD_POLARITY", polarity));
                            ports.push(("ALOAD", false, signal));
                            ports.push(("AD", false, value));
                            "$aldff"
                        }
                    }
                }
                (None, Some(enable)) => {
                    let en = self.get(enable);
                    let en = self.truth(en, st.loc);
                    params.push(("EN_POLARITY", Param::Int(1)));
                    ports.push(("EN", false, en));
                    match reset {
                        None => "$dlatch",
                        Some((signal, polarity, Some(constant), _)) => {
                            params.push(("ARST_POLARITY", polarity));
                            params.push(("ARST_VALUE", Param::Bits(constant)));
                            ports.push(("ARST", false, signal));
                            "$adlatch"
                        }
                        Some(_) => {
                            return unsupported(format!(
                                "latch {} with a reset to a value that is not constant",
                                st.name
                            ))
                        }
                    }
                }
                _ => return rejected(format!("state {} is neither a flip-flop nor a latch", st.name)),
            };
            ports.push(("D", false, d));
            ports.push(("Q", true, q.clone()));
            let name = self.unique(&format!("{ty}${}", st.name));
            self.cells.push(Cell { name, ty, params, ports, loc: st.loc });
            let init = st.init.as_ref().and_then(|c| bit_string(&const_bits(c, w)));
            let name = self.unique(&st.name);
            self.netnames.push(NetName { name, bits: q, init });
        }
        Ok(())
    }

    fn histories(&mut self) {
        for h in std::mem::take(&mut self.history) {
            let d = match h.d {
                Ok(v) => self.get(v),
                Err(bits) => bits,
            };
            let (clk, polarity) = self.clock_ports(h.clock);
            let mut params = vec![("WIDTH", Param::Int(h.q.len() as u32)), ("CLK_POLARITY", polarity)];
            let mut ports = vec![("CLK", false, clk)];
            let ty = match h.gate {
                Some(g) => {
                    let en = self.get(g);
                    let en = self.truth(en, h.loc);
                    params.push(("EN_POLARITY", Param::Int(1)));
                    ports.push(("EN", false, en));
                    "$dffe"
                }
                None => "$dff",
            };
            ports.push(("D", false, d));
            ports.push(("Q", true, h.q));
            let name = format!("{ty}$past${}", self.cells.len());
            self.cells.push(Cell { name, ty, params, ports, loc: h.loc });
        }
    }

    fn checks(&mut self) {
        let nl = self.nl;
        let live = nl.liveness.len();
        if live > 0 {
            self.notes.push(format!(
                "{live} liveness obligation{} not written (the Btor2 and AIGER writers take them)",
                if live == 1 { " is" } else { "s are" }
            ));
        }
        let compiled = |i: usize| {
            nl.property_checks.get(i).copied().flatten().is_some()
                || nl.property_liveness.get(i).copied().flatten().is_some()
        };
        let left_out = (0..nl.properties.len()).filter(|&i| !compiled(i)).count();
        if left_out > 0 {
            self.notes.push(format!(
                "{left_out} concurrent propert{} not written (not compiled by Netlist::compile_properties)",
                if left_out == 1 { "y is" } else { "ies are" }
            ));
        }
        if !self.opts.checks {
            if !nl.checks.is_empty() {
                self.notes.push(format!("{} check(s) left out (Options::checks is off)", nl.checks.len()));
            }
            return;
        }
        for c in &nl.checks {
            let ty = match c.kind {
                CheckKind::Assert => "$assert",
                CheckKind::Assume => "$assume",
                CheckKind::Cover => "$cover",
            };
            let (a, en) = (self.get(c.expr), self.get(c.enable));
            let (a, en) = (self.truth(a, c.loc), self.truth(en, c.loc));
            let name = match &c.label {
                Some(l) => self.unique(l),
                None => format!("{ty}${}", self.cells.len()),
            };
            self.cells.push(Cell {
                name,
                ty,
                params: Vec::new(),
                ports: vec![("A", false, a), ("EN", false, en)],
                loc: c.loc,
            });
        }
    }

    fn run(&mut self) -> Result<(), Fail> {
        let nl = self.nl;
        for p in nl.inputs.iter().map(|p| &p.name).chain(nl.outputs.iter().map(|p| &p.name)) {
            self.names.insert(p.clone());
        }
        for i in 0..nl.values.len() {
            let bits = self.value(ValueId(i as u32))?;
            self.bits[i] = Some(bits);
        }
        self.registers()?;
        self.histories();
        self.checks();
        // The design's own names for its signals.
        for t in &nl.traces {
            if self.names.contains(&t.name) {
                continue;
            }
            let bits = self.get(t.value);
            if bits.iter().all(|b| matches!(b, Bit::Const(_))) {
                continue;
            }
            self.names.insert(t.name.clone());
            self.netnames.push(NetName { name: t.name.clone(), bits, init: None });
        }
        Ok(())
    }

    fn src(&self, loc: Loc) -> Option<String> {
        if !self.opts.src_attributes || loc.line == 0 {
            return None;
        }
        let file = self.nl.files.get(loc.file as usize).filter(|f| !f.is_empty())?;
        Some(format!("{file}:{}.{}", loc.line, loc.column))
    }

    fn json(&self) -> String {
        let nl = self.nl;
        let mut s = String::new();
        let _ = writeln!(s, "{{\n  \"creator\": \"svlang sv-yosys {}\",\n  \"modules\": {{", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(s, "    {}: {{", json_string(&nl.top));
        s.push_str("      \"attributes\": {\"top\": \"00000000000000000000000000000001\"},\n");

        let mut ports = Vec::new();
        for p in &nl.inputs {
            ports.push(format!(
                "        {}: {{\"direction\": \"input\", \"bits\": {}}}",
                json_string(&p.name),
                json_bits(&self.get(p.value))
            ));
        }
        for p in &nl.outputs {
            ports.push(format!(
                "        {}: {{\"direction\": \"output\", \"bits\": {}}}",
                json_string(&p.name),
                json_bits(&self.get(p.value))
            ));
        }
        let _ = writeln!(s, "      \"ports\": {{\n{}\n      }},", ports.join(",\n"));

        let mut cells = Vec::new();
        for c in &self.cells {
            let params: Vec<String> = c
                .params
                .iter()
                .map(|(n, p)| match p {
                    Param::Int(v) => format!("\"{n}\": \"{v:032b}\""),
                    Param::Bits(b) => format!("\"{n}\": \"{b}\""),
                })
                .collect();
            let dirs: Vec<String> = c
                .ports
                .iter()
                .map(|(n, out, _)| format!("\"{n}\": \"{}\"", if *out { "output" } else { "input" }))
                .collect();
            let conns: Vec<String> = c.ports.iter().map(|(n, _, b)| format!("\"{n}\": {}", json_bits(b))).collect();
            let attrs = self.src(c.loc).map(|src| format!("\"src\": {}", json_string(&src))).unwrap_or_default();
            cells.push(format!(
                "        {}: {{\n          \"hide_name\": {},\n          \"type\": \"{}\",\n          \"parameters\": {{{}}},\n          \"attributes\": {{{attrs}}},\n          \"port_directions\": {{{}}},\n          \"connections\": {{{}}}\n        }}",
                json_string(&c.name),
                u8::from(c.name.starts_with('$')),
                c.ty,
                params.join(", "),
                dirs.join(", "),
                conns.join(", ")
            ));
        }
        let _ = writeln!(s, "      \"cells\": {{\n{}\n      }},", cells.join(",\n"));

        let mut nets = Vec::new();
        for n in &self.netnames {
            let attrs = n.init.as_ref().map(|i| format!("\"init\": \"{i}\"")).unwrap_or_default();
            nets.push(format!(
                "        {}: {{\"hide_name\": 0, \"bits\": {}, \"attributes\": {{{attrs}}}}}",
                json_string(&n.name),
                json_bits(&n.bits)
            ));
        }
        let _ = writeln!(s, "      \"netnames\": {{\n{}\n      }}", nets.join(",\n"));
        s.push_str("    }\n  }\n}\n");
        s
    }
}
