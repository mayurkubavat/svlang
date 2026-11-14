//! A svlang [`Netlist`] as an and-inverter graph, written in
//! [AIGER](https://fmv.jku.at/aiger/) 1.9 for bit-level model checkers and
//! logic tools such as rIC3, ABC, nuXmv and the HWMCC solvers.
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
//! let model = sv_aiger::lower(&netlist, &sv_aiger::Options::default()).unwrap();
//! assert_eq!(model.aig.bads.len(), 1);
//! let binary: Vec<u8> = model.aig.to_binary(); // counter.aig
//! let ascii: String = model.aig.to_ascii(); // counter.aag
//! assert!(ascii.starts_with("aag "));
//! # let _ = binary;
//! ```
//!
//! The design is first written as the Btor2 transition system of `sv-btor2`,
//! with that crate's modelling choices (one step per edge of a single clock,
//! asynchronous resets acting synchronously, free values for undriven and X
//! bits, immediate assertions as bad states and immediate assumptions as
//! constraints), and then bit-blasted: every register bit is a latch, every
//! input bit an input. Memories must be expanded into registers first
//! ([`LowerConfig::blast_memories_up_to`](sv_netlist::LowerConfig)); anything
//! that cannot be written exactly is an [`Error`].
//!
//! Literals are AIGER-style: `2 * var + negated`; variable 0 is constant
//! false, so literal 0 is false and 1 is true. Every AND node's inputs are
//! smaller variables, so node order is a topological order.

pub mod aiger;
mod blast;
mod err;
mod model;
mod parse;
mod words;

pub use err::Error;

use sv_netlist::Netlist;

/// How to lower a netlist to an AIG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Options {
    /// Prefix of the names invented for free inputs and history registers (`"svlang"` by default).
    pub name_prefix: String,
}

impl Default for Options {
    fn default() -> Self {
        Self { name_prefix: "svlang".into() }
    }
}

/// The bit-level model, and notes for the log.
#[derive(Debug, Clone)]
pub struct Model {
    /// The and-inverter graph: write it with [`Aig::to_binary`] or [`Aig::to_ascii`].
    pub aig: Aig,
    /// What was left out or is worth telling the user (see `sv_btor2::Model::notes`).
    pub notes: Vec<String>,
}

/// Lowers `nl` to an and-inverter graph.
///
/// # Errors
///
/// [`Error`] when the netlist cannot be modelled exactly: more than one clock, a latch that depends on itself,
/// a concurrent assumption, a memory that was not expanded, or a register whose initial
/// value is not a constant.
pub fn lower(nl: &Netlist, options: &Options) -> Result<Model, Error> {
    let mut o = sv_btor2::Options::default();
    o.name_prefix.clone_from(&options.name_prefix);
    let m = sv_btor2::write(nl, &o).map_err(|e| Error(e.to_string()))?;
    let aig = from_btor2(&m.btor2)?;
    Ok(Model { aig, notes: m.notes })
}

/// Bit-blasts Btor2 text (bit-vectors only, no arrays, no liveness) into an AIG.
///
/// # Errors
///
/// [`Error`] when the text is not well-formed Btor2, uses arrays or liveness, or has a
/// state whose initial value is not a constant (AIGER cannot express it).
pub fn from_btor2(text: &str) -> Result<Aig, Error> {
    let parsed = parse::parse(text).map_err(|e| Error(e.to_string()))?;
    let aig = blast::from_btor2(&parsed)?;
    if let Some(l) = aig.latches.iter().find(|l| matches!(l.init, Init::Expr(_))) {
        let name = aig.names.get(&var(l.lit)).cloned().unwrap_or_else(|| format!("latch {}", l.lit));
        return Err(Error(format!("the initial value of {name} is not a constant, which AIGER cannot express")));
    }
    Ok(aig)
}

use std::collections::HashMap;
use std::hash::BuildHasherDefault;

pub type Lit = u32;
pub const FALSE: Lit = 0;
pub const TRUE: Lit = 1;

#[inline]
pub fn var(l: Lit) -> u32 {
    l >> 1
}
#[inline]
pub fn neg(l: Lit) -> Lit {
    l ^ 1
}
#[inline]
pub fn is_neg(l: Lit) -> bool {
    l & 1 == 1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Node {
    Const,
    Input,
    Latch,
    And(Lit, Lit),
}

/// A latch's value in the first frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Init {
    Zero,
    One,
    /// Any value (AIGER 1.9 uninitialized latch).
    Free,
    /// The value of a combinational literal in frame 0 (Btor2 `init` with a
    /// non-constant value); not expressible in AIGER.
    Expr(Lit),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latch {
    /// The latch's own (positive) literal.
    pub lit: Lit,
    pub next: Lit,
    pub init: Init,
}

/// One rewrite step of [`Aig::and`]'s two-level rules.
enum Step {
    Done(Lit),
    Again(Lit, Lit),
}

/// Deterministic hashing (fixed keys), so runs are reproducible.
type Fixed = BuildHasherDefault<std::collections::hash_map::DefaultHasher>;

#[derive(Debug, Clone, Default)]
pub struct Aig {
    pub nodes: Vec<Node>,
    pub inputs: Vec<Lit>,
    pub latches: Vec<Latch>,
    pub bads: Vec<Lit>,
    /// The label of each entry of `bads` (empty when it has none), written as `b<k>` symbols.
    pub bad_names: Vec<String>,
    pub constraints: Vec<Lit>,
    /// Names for inputs and latches, by variable (when known).
    pub names: HashMap<u32, String, Fixed>,
    /// Named Btor2 `output` lines (bits LSB first), kept only so a trace
    /// replay can show combinational nets; engines and preprocessing
    /// ignore them.
    pub outputs: Vec<(String, Vec<Lit>)>,
    strash: HashMap<(Lit, Lit), Lit, Fixed>,
}

impl Aig {
    pub fn new() -> Self {
        Aig { nodes: vec![Node::Const], ..Default::default() }
    }

    pub fn num_vars(&self) -> u32 {
        self.nodes.len() as u32
    }

    pub fn num_ands(&self) -> usize {
        self.nodes.iter().filter(|n| matches!(n, Node::And(..))).count()
    }

    pub fn input(&mut self) -> Lit {
        let l = 2 * self.num_vars();
        self.nodes.push(Node::Input);
        self.inputs.push(l);
        l
    }

    /// A new latch; set its next state with [`Aig::set_next`].
    pub fn latch(&mut self, init: Init) -> Lit {
        let l = 2 * self.num_vars();
        self.nodes.push(Node::Latch);
        self.latches.push(Latch { lit: l, next: FALSE, init });
        l
    }

    pub fn set_next(&mut self, latch: Lit, next: Lit) {
        let l = self.latches.iter_mut().find(|x| x.lit == latch).expect("not a latch");
        l.next = next;
    }

    /// AND with constant folding and structural hashing.
    pub fn and(&mut self, mut a: Lit, mut b: Lit) -> Lit {
        loop {
            if a == FALSE || b == FALSE || a == neg(b) {
                return FALSE;
            }
            if a == TRUE || a == b {
                return b;
            }
            if b == TRUE {
                return a;
            }
            // Two-level rules (as rIC3's aig-rs comb_simplify), with `o`
            // one fanin and `i = (x & y)` or `!(x & y)` the other:
            //   o & (o & y) = (o & y)      o & !(o & y) = o & !y
            //   o & (!o & y) = 0           o & !(!o & y) = o
            match self.two_level(a, b).or_else(|| self.two_level(b, a)) {
                Some(Step::Done(l)) => return l,
                Some(Step::Again(x, y)) => (a, b) = (x, y),
                None => break,
            }
        }
        let key = if a < b { (a, b) } else { (b, a) };
        if let Some(&l) = self.strash.get(&key) {
            return l;
        }
        let l = 2 * self.num_vars();
        self.nodes.push(Node::And(key.0, key.1));
        self.strash.insert(key, l);
        l
    }

    fn two_level(&self, o: Lit, i: Lit) -> Option<Step> {
        let Node::And(x, y) = self.nodes[var(i) as usize] else {
            return None;
        };
        let positive = !is_neg(i);
        if x == o || y == o {
            let other = if x == o { y } else { x };
            return Some(if positive { Step::Done(i) } else { Step::Again(o, neg(other)) });
        }
        if x == neg(o) || y == neg(o) {
            return Some(Step::Done(if positive { FALSE } else { o }));
        }
        None
    }

    /// Add an AND node exactly as given (readers that must keep variable
    /// numbering); still recorded for structural hashing.
    pub(crate) fn raw_and(&mut self, a: Lit, b: Lit) -> Lit {
        let l = 2 * self.num_vars();
        self.nodes.push(Node::And(a, b));
        self.strash.entry(if a < b { (a, b) } else { (b, a) }).or_insert(l);
        l
    }

    pub fn or(&mut self, a: Lit, b: Lit) -> Lit {
        neg(self.and(neg(a), neg(b)))
    }

    pub fn xor(&mut self, a: Lit, b: Lit) -> Lit {
        if a == FALSE {
            return b;
        }
        if b == FALSE {
            return a;
        }
        if a == TRUE {
            return neg(b);
        }
        if b == TRUE {
            return neg(a);
        }
        if a == b {
            return FALSE;
        }
        if a == neg(b) {
            return TRUE;
        }
        let x = self.and(a, neg(b));
        let y = self.and(neg(a), b);
        self.or(x, y)
    }

    pub fn ite(&mut self, c: Lit, a: Lit, b: Lit) -> Lit {
        if c == TRUE || a == b {
            return a;
        }
        if c == FALSE {
            return b;
        }
        let x = self.and(c, a);
        let y = self.and(neg(c), b);
        self.or(x, y)
    }

    pub fn and_all(&mut self, ls: &[Lit]) -> Lit {
        ls.iter().fold(TRUE, |acc, &l| self.and(acc, l))
    }

    pub fn or_all(&mut self, ls: &[Lit]) -> Lit {
        ls.iter().fold(FALSE, |acc, &l| self.or(acc, l))
    }

    pub fn is_latch(&self, l: Lit) -> bool {
        matches!(self.nodes.get(var(l) as usize), Some(Node::Latch))
    }

    /// Binary AIGER 1.9 (`.aig`), the form ABC and rIC3 read fastest.
    pub fn to_binary(&self) -> Vec<u8> {
        aiger::write_binary(self)
    }

    /// ASCII AIGER 1.9 (`.aag`).
    pub fn to_ascii(&self) -> String {
        aiger::write_ascii(self)
    }

    pub fn summary(&self) -> String {
        format!(
            "inputs {} latches {} ands {} bads {} constraints {}",
            self.inputs.len(),
            self.latches.len(),
            self.num_ands(),
            self.bads.len(),
            self.constraints.len()
        )
    }

    /// Evaluate every node for one frame, given input and latch values.
    pub fn eval(&self, mut leaf: impl FnMut(Lit) -> bool) -> Vec<bool> {
        let mut v = vec![false; self.nodes.len()];
        for (i, n) in self.nodes.iter().enumerate() {
            v[i] = match *n {
                Node::Const => false,
                Node::Input | Node::Latch => leaf(2 * i as u32),
                Node::And(a, b) => lit_val(&v, a) && lit_val(&v, b),
            };
        }
        v
    }
}

#[inline]
pub fn lit_val(vals: &[bool], l: Lit) -> bool {
    vals[var(l) as usize] ^ is_neg(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_and_hashing() {
        let mut g = Aig::new();
        let (a, b) = (g.input(), g.input());
        let x = g.and(a, b);
        assert_eq!(g.and(b, a), x);
        assert_eq!(g.and(a, neg(a)), FALSE);
        assert_eq!(g.and(a, TRUE), a);
        let y = g.xor(a, b);
        let vals = g.eval(|l| l == a);
        assert!(lit_val(&vals, y) && !lit_val(&vals, x));
    }
}
