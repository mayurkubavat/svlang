//! Format-neutral, word-level RTL netlist IR.
//!
//! This module defines the data structures at the boundary between SystemVerilog
//! front-end elaboration and downstream formal verification, synthesis,
//! equivalence-checking, and lint consumers.
//!
//! Structural invariants of [`Netlist`]:
//! - [`Netlist::values`] is topologically ordered. Every [`ValueKind::Op`] node
//!   appears strictly after all operand [`ValueId`]s in `args`. Every
//!   [`ValueKind::MemRead`] node appears strictly after its `addr` and after any
//!   combinational [`MemWritePort`] (`clock.is_none()`) on the same [`Memory`].
//!   Every [`ValueKind::Sampled`] node appears strictly after `expr` except when
//!   `kind == SampledKind::Past` and `depth > 0`, which acts as a sequential state
//!   boundary.
//! - Four-state `x` and `z` bits are preserved in [`CVal`] constants and
//!   [`ValueNode::four_state`] metadata rather than coerced to `0`.
//! - Asynchronous resets ([`AsyncReset`]) and constant initial values
//!   ([`State::init`] and [`Memory::init`]) remain structural metadata separate
//!   from [`State::next`].
//! - Sampled-value functions (`$past`, `$rose`, `$fell`, `$stable`, `$changed`,
//!   `$sampled`) remain explicit [`SampledNode`] entries with their resolved clock
//!   and enable gate.

use sv_semantic::{BoundProperty, CVal, ElaboratedDesign, ExprArena, ExprId, Loc, SampledKind, SymbolId};

/// Dense zero-based identifier indexing [`Netlist::values`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(
    /// Index into [`Netlist::values`] (`0 .. values.len() as u32`).
    pub u32,
);

/// Dense zero-based identifier indexing [`Netlist::states`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId(
    /// Index into [`Netlist::states`] (`0 .. states.len() as u32`).
    pub u32,
);

/// Dense zero-based identifier indexing [`Netlist::memories`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemId(
    /// Index into [`Netlist::memories`] (`0 .. memories.len() as u32`).
    pub u32,
);

/// Active clock edge polarity on a [`State`], [`MemWritePort`], [`SampledNode`], or [`Check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Edge {
    /// Rising edge (`0` to `1` transition on the 1-bit clock signal).
    Posedge,
    /// Falling edge (`1` to `0` transition on the 1-bit clock signal).
    Negedge,
}

impl Edge {
    /// Returns the SystemVerilog keyword (`"posedge"` or `"negedge"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            Edge::Posedge => "posedge",
            Edge::Negedge => "negedge",
        }
    }
}

/// Clock signal and active edge governing a sequential element or sampled check.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockSpec {
    /// 1-bit clock value in [`Netlist::values`]. Multi-bit edge expressions extract LSB bit `0`.
    pub signal: ValueId,
    /// Active clock edge polarity.
    pub edge: Edge,
}

/// Asynchronous reset specification on a flip-flop [`State`] element.
///
/// `State::next` holds only the synchronous datapath expression. `AsyncReset` is
/// kept separate so consumers can model asynchronous preemption directly or convert
/// it to synchronous logic according to their target semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AsyncReset {
    /// 1-bit reset signal in [`Netlist::values`].
    pub signal: ValueId,
    /// `true` for active-high (`posedge rst` with `if (rst)`), `false` for active-low (`negedge rst_n` with `if (!rst_n)`).
    pub active_high: bool,
    /// Value of width `State::width` loaded asynchronously while reset is asserted.
    pub value: ValueId,
}

/// Word-level combinational operator in [`ValueKind::Op`].
///
/// Every `NetOp` specifies its exact `args` arity, operand width requirements, and
/// result width and signedness invariants. Physical bit positions are always
/// `0`-based from the least-significant bit (`0 <= lo <= hi < width`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetOp {
    /// Modular addition `args[0] + args[1]`.
    /// Arity: `2`. Both operands and the result have identical width `W`, wrapping modulo `2^W`.
    /// Any `x`/`z` bit in either operand makes the entire `W`-bit result `all_x`.
    Add,
    /// Modular subtraction `args[0] - args[1]`.
    /// Arity: `2`. Both operands and the result have identical width `W`, wrapping modulo `2^W`.
    /// Any `x`/`z` bit in either operand makes the entire `W`-bit result `all_x`.
    Sub,
    /// Modular multiplication `args[0] * args[1]`.
    /// Arity: `2`. Both operands and the result have identical width `W` (low `W` product bits).
    /// Any `x`/`z` bit in either operand makes the entire `W`-bit result `all_x`.
    Mul,
    /// Unsigned integer division `args[0] / args[1]` truncating toward zero.
    /// Arity: `2`. Both operands and the result have width `W`.
    /// Division by `0` or any `x`/`z` bit yields `all_x` in 4-state mode (`0` when coerced to 2-state).
    UDiv,
    /// Signed two's-complement division `args[0] / args[1]` truncating toward zero.
    /// Arity: `2`. Both operands and the result have width `W`.
    /// Division by `0` yields `all_x` in 4-state mode; `INT_MIN / -1` wraps to `INT_MIN` modulo `2^W`.
    SDiv,
    /// Unsigned remainder `args[0] % args[1]`.
    /// Arity: `2`. Both operands and the result have width `W`.
    /// Modulo by `0` or any `x`/`z` bit yields `all_x` in 4-state mode.
    UMod,
    /// Signed two's-complement remainder `args[0] % args[1]` (sign follows dividend `args[0]`).
    /// Arity: `2`. Both operands and the result have width `W`.
    /// Modulo by `0` or any `x`/`z` bit yields `all_x` in 4-state mode.
    SMod,
    /// Integer exponentiation `args[0] ** args[1]` (emitted only when `args[1]` is constant or folded).
    /// Arity: `2`. `args[0]` and the result have width `W`; `args[1]` has its self-determined width.
    Pow,
    /// Bitwise AND `args[0] & args[1]` (`0 & x == 0`).
    /// Arity: `2`. Both operands and the result have identical width `W`.
    And,
    /// Bitwise inclusive OR `args[0] | args[1]` (`1 | x == 1`).
    /// Arity: `2`. Both operands and the result have identical width `W`.
    Or,
    /// Bitwise exclusive OR `args[0] ^ args[1]`.
    /// Arity: `2`. Both operands and the result have identical width `W`.
    Xor,
    /// Bitwise exclusive NOR `args[0] ~^ args[1]`.
    /// Arity: `2`. Both operands and the result have identical width `W`.
    Xnor,
    /// Bitwise complement `~args[0]`.
    /// Arity: `1`. Operand and result have identical width `W`.
    Not,
    /// Two's-complement negation `-args[0]` modulo `2^W`.
    /// Arity: `1`. Operand and result have identical width `W`.
    Neg,
    /// Reduction AND `&args[0]`.
    /// Arity: `1`. Operand has width `>= 1`; result is `1`-bit unsigned (`0` if any bit is `0`).
    RedAnd,
    /// Reduction OR `|args[0]`.
    /// Arity: `1`. Operand has width `>= 1`; result is `1`-bit unsigned (`1` if any bit is `1`).
    RedOr,
    /// Reduction XOR `^args[0]`.
    /// Arity: `1`. Operand has width `>= 1`; result is `1`-bit unsigned.
    RedXor,
    /// Logical negation `!args[0]`.
    /// Arity: `1`. Operand has width `1` (booleanized prior to emission); result is `1`-bit unsigned.
    LogNot,
    /// Logical conjunction `args[0] && args[1]` (`0 && x == 0`).
    /// Arity: `2`. Both operands and the result are `1`-bit unsigned.
    LogAnd,
    /// Logical disjunction `args[0] || args[1]` (`1 || x == 1`).
    /// Arity: `2`. Both operands and the result are `1`-bit unsigned.
    LogOr,
    /// Logical left shift `args[0] << args[1]`, filling vacated low bits with `0`.
    /// Arity: `2`. `args[0]` and the result have width `W`; `args[1]` is treated as unsigned of any width `>= 1`.
    /// Shifts `>= W` produce `0`; unknown `x`/`z` bits in `args[1]` produce `all_x`.
    Shl,
    /// Logical right shift `args[0] >> args[1]`, filling vacated high bits with `0`.
    /// Arity: `2`. `args[0]` and the result have width `W`; `args[1]` is treated as unsigned of any width `>= 1`.
    /// Shifts `>= W` produce `0`; unknown `x`/`z` bits in `args[1]` produce `all_x`.
    LShr,
    /// Arithmetic right shift `args[0] >>> args[1]`, filling vacated high bits with sign bit `args[0][W - 1]`.
    /// Arity: `2`. `args[0]` and the result have width `W`; `args[1]` is treated as unsigned of any width `>= 1`.
    /// Shifts `>= W` replicate `args[0][W - 1]` across all `W` bits.
    AShr,
    /// Four-state logical equality `args[0] == args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned (`1'bx` when ambiguous under `x`/`z`).
    Eq,
    /// Four-state logical inequality `args[0] != args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned (`1'b1` if any known bit position differs).
    Neq,
    /// Exact 4-state case equality `args[0] === args[1]` comparing `0`, `1`, `x`, and `z` bit-for-bit.
    /// Arity: `2`. Both operands have equal width `W`; result is always 2-state `1`-bit unsigned (`four_state == false`).
    CaseEq,
    /// Exact 4-state case inequality `args[0] !== args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is always 2-state `1`-bit unsigned (`four_state == false`).
    CaseNeq,
    /// Wildcard equality `args[0] ==? rhs` where `x` and `z` bits in `rhs` act as don't-cares.
    /// Arity: `1`. `args[0]` and `rhs` have equal width `W`; result is `1`-bit unsigned.
    WildEq {
        /// Constant right-hand pattern of width `args[0].width`.
        rhs: CVal,
    },
    /// Wildcard inequality `args[0] !=? rhs` where `x` and `z` bits in `rhs` act as don't-cares.
    /// Arity: `1`. `args[0]` and `rhs` have equal width `W`; result is `1`-bit unsigned.
    WildNeq {
        /// Constant right-hand pattern of width `args[0].width`.
        rhs: CVal,
    },
    /// Unsigned less-than `args[0] < args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned.
    ULt,
    /// Signed two's-complement less-than `args[0] < args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned.
    SLt,
    /// Unsigned less-than-or-equal `args[0] <= args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned.
    ULe,
    /// Signed two's-complement less-than-or-equal `args[0] <= args[1]`.
    /// Arity: `2`. Both operands have equal width `W`; result is `1`-bit unsigned.
    SLe,
    /// Ternary multiplexer `args[0] ? args[1] : args[2]`.
    /// Arity: `3`. `args[0]` is `1`-bit; `args[1]`, `args[2]`, and the result have identical width `W`.
    /// When `args[0]` is `x`/`z`, resolves to `args[1]` if `args[1]` and `args[2]` are known and equal, else `all_x`.
    Mux,
    /// Bit concatenation `{args[0], args[1], ...}` ordered most-significant part first (`args[0]` at MSB, last arg at LSB `0`).
    /// Arity: `>= 1`. Result width equals the sum of operand widths.
    Concat,
    /// Constant physical bit-range extraction `args[0][hi : lo]`.
    /// Arity: `1`. Satisfies `0 <= lo <= hi < args[0].width`; result width is `hi - lo + 1`.
    /// Also used with `lo == 0 && hi == width - 1` when reinterpreting signedness without changing bit width.
    Slice {
        /// Inclusive physical MSB bit offset (`0`-based from LSB).
        hi: u32,
        /// Inclusive physical LSB bit offset (`0`-based from LSB).
        lo: u32,
    },
    /// Unsigned zero-extension of `args[0]` to `width` bits.
    /// Arity: `1`. Satisfies `width >= args[0].width`; result width equals `width`.
    ZeroExt {
        /// Target bit width (`>= args[0].width`).
        width: u32,
    },
    /// Two's-complement sign-extension of `args[0]` replicating `args[0][args[0].width - 1]` up to `width` bits.
    /// Arity: `1`. Satisfies `width >= args[0].width`; result width equals `width`.
    SignExt {
        /// Target bit width (`>= args[0].width`).
        width: u32,
    },
    /// Explicit 2-state / 4-state domain conversion (`bit'(x)` sets `four_state: false`, mapping `x`/`z` bits to `0`).
    /// Arity: `1`. Operand and result have identical width `W`.
    CastFourState {
        /// Target `four_state` flag (`false` maps unknown bits to `0`).
        four_state: bool,
    },
    /// Population count `$countones(args[0])` counting bits equal to `1` (`0`, `x`, and `z` bits are not counted).
    /// Arity: `1`. Result is `32`-bit signed 2-state (`s32.2`).
    CountOnes,
    /// One-hot check `$onehot(args[0])` (`true` iff exactly one bit is `1` and no unknown bits make count ambiguous).
    /// Arity: `1`. Result is `1`-bit unsigned 2-state (`u1.2`).
    OneHot,
    /// At-most-one-hot check `$onehot0(args[0])` (`true` iff at most one bit is `1`).
    /// Arity: `1`. Result is `1`-bit unsigned 2-state (`u1.2`).
    OneHot0,
    /// Unknown-bit detector `$isunknown(args[0])` (`1` iff any bit of `args[0]` is `x` or `z`).
    /// Arity: `1`. Result is `1`-bit unsigned 2-state (`u1.2`). Emitted only when [`LowerConfig::allow_four_state_ops`] is `true`.
    IsUnknown,
}

/// Kind of a [`ValueNode`] in [`Netlist::values`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueKind {
    /// Primary top-level input port (indexes [`Netlist::inputs`]).
    Input(usize),
    /// Constant bit-vector with 4-state `x`/`z` bits preserved.
    Const(CVal),
    /// Current-state output of sequential element [`StateId`].
    StateOut(StateId),
    /// Word-level combinational operation.
    Op {
        /// Combinational operator.
        op: NetOp,
        /// Operand values in [`Netlist::values`].
        args: Vec<ValueId>,
    },
    /// Combinational read port on [`MemId`] at zero-based word offset `addr` (`sv_index - lo_idx`).
    ///
    /// Out-of-bounds or unknown `addr` values read `all_x` when `four_state == true` and `0`
    /// when `four_state == false` (guarded by explicit bounds muxing emitted by the lowerer).
    MemRead {
        /// Target memory array in [`Netlist::memories`].
        mem: MemId,
        /// Zero-based word offset (`0 .. depth - 1`).
        addr: ValueId,
    },
    /// Sampled-value expression output (indexes [`Netlist::sampled`]).
    Sampled(usize),
    /// Undriven bit-slice free value (indexes [`Netlist::undriven`]).
    Undriven(usize),
}

/// One typed word-level value in [`Netlist::values`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValueNode {
    /// Dense ID (`values[id.0 as usize]`).
    pub id: ValueId,
    /// Bit width (`>= 1`).
    pub width: u32,
    /// Signedness metadata (`true` when two's-complement signed).
    pub signed: bool,
    /// `true` when capable of carrying 4-state (`x` or `z`) values.
    pub four_state: bool,
    /// Defining computation or leaf source.
    pub kind: ValueKind,
    /// Optional hierarchical signal name (`"top.u0.sum"`).
    pub name: Option<String>,
    /// Originating source location.
    pub loc: Loc,
}

/// Top-level input port (scalarized if declared as an aggregate struct or unpacked array).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortInput {
    /// Port name (`"clk"`, `"req.valid"`, or `"in_arr[0]"`).
    pub name: String,
    /// Bit width (`>= 1`).
    pub width: u32,
    /// Signedness.
    pub signed: bool,
    /// Four-state flag.
    pub four_state: bool,
    /// Corresponding [`ValueKind::Input`] node in [`Netlist::values`].
    pub value: ValueId,
    /// Declaration location.
    pub loc: Loc,
}

/// Top-level output port (scalarized if declared as an aggregate struct or unpacked array).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortOutput {
    /// Port name (`"q"`, `"resp.data"`, or `"out_arr[1]"`).
    pub name: String,
    /// Bit width (`>= 1`).
    pub width: u32,
    /// Signedness.
    pub signed: bool,
    /// Four-state flag.
    pub four_state: bool,
    /// Driven value in [`Netlist::values`].
    pub value: ValueId,
    /// Declaration location.
    pub loc: Loc,
}

/// Sequential edge-triggered flip-flop or level-sensitive latch in [`Netlist::states`].
///
/// Exactly one of the two storage modes holds:
/// - **Edge-triggered flip-flop**: `clock` is `Some(_)`, `latch_enable` is `None`. On each
///   active `clock` edge (when `async_reset` is not asserted), the state updates to `next`
///   sampled immediately before the clock edge. If `async_reset` is `Some(ar)` and
///   `ar.signal == ar.active_high`, the state asynchronously takes `ar.value`; `next`
///   contains only the synchronous branch without duplicating the asynchronous reset mux.
/// - **Level-sensitive latch**: `clock` is `None`, `async_reset` is `None`, and
///   `latch_enable` is `Some(en)`. While `en` is `1`, the latch is transparent and
///   tracks `next`; while `en` is `0`, it holds `out` (`next` also muxes `out` on the
///   disabled paths).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    /// Dense state ID (`states[id.0 as usize]`).
    pub id: StateId,
    /// Hierarchical register or slice name (`"top.q"` or `"top.q[3:0]"`).
    pub name: String,
    /// Bit width (`>= 1`).
    pub width: u32,
    /// Signedness.
    pub signed: bool,
    /// Four-state flag.
    pub four_state: bool,
    /// Governing clock and active edge (`Some` for flip-flops, `None` for latches).
    pub clock: Option<ClockSpec>,
    /// Optional asynchronous reset specification (`None` for purely synchronous flip-flops and latches).
    pub async_reset: Option<AsyncReset>,
    /// Transparent enable (`Some(en)` for `always_latch`, `None` for flip-flops).
    pub latch_enable: Option<ValueId>,
    /// Optional declaration or `initial`-block constant value.
    pub init: Option<CVal>,
    /// Synchronous next-state (or latched data) value in [`Netlist::values`] of width `width`.
    pub next: ValueId,
    /// Current-state [`ValueKind::StateOut`] node in [`Netlist::values`] of width `width`.
    pub out: ValueId,
    /// Source location of the sequential driver.
    pub loc: Loc,
}

/// Write port on a [`Memory`] array.
///
/// `addr` uses zero-based physical word offsets (`0 .. depth - 1`, computed as
/// `sv_index - lo_idx`). Out-of-bounds or unknown `x`/`z` addresses force `enable`
/// to `2`-state `1'b0` so invalid indices never corrupt valid words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemWritePort {
    /// Governing clock (`Some` for synchronous NBA writes; `None` if unclocked).
    pub clock: Option<ClockSpec>,
    /// Zero-based word offset (`0 .. depth - 1`).
    pub addr: ValueId,
    /// Word data to write (`width == mem.width`).
    pub data: ValueId,
    /// 1-bit write-enable condition incorporating path reachability and bounds checks.
    pub enable: ValueId,
    /// Source location of the write assignment.
    pub loc: Loc,
}

/// Word-level memory lowered from a module-internal 1-D unpacked array.
///
/// Word offsets are zero-based (`offset = sv_index - lo_idx`, so `lo_idx` maps to `0`
/// and `hi_idx` maps to `depth - 1` regardless of whether the SystemVerilog array was
/// declared `[0:D-1]` or `[D-1:0]`). Clocked write ports follow read-old-data NBA
/// semantics on active clock edges, and `writes` is ordered from earliest to latest
/// statement so later write ports win when two enabled ports target the same address
/// on the same clock edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memory {
    /// Dense memory ID (`memories[id.0 as usize]`).
    pub id: MemId,
    /// Hierarchical array name (`"top.u_fifo.mem"`).
    pub name: String,
    /// Word bit width (`>= 1`).
    pub width: u32,
    /// Number of words (`hi_idx - lo_idx + 1 >= 1`).
    pub depth: u32,
    /// Minimum declared SystemVerilog index (`min(left, right)`).
    pub lo_idx: i128,
    /// Maximum declared SystemVerilog index (`max(left, right)`).
    pub hi_idx: i128,
    /// Word signedness.
    pub signed: bool,
    /// Word 4-state flag.
    pub four_state: bool,
    /// Constant initial word values `(zero_based_word_offset, value)` from declaration or `initial` blocks.
    pub init: Vec<(u32, CVal)>,
    /// Ordered write ports (last enabled port targeting the same address wins).
    pub writes: Vec<MemWritePort>,
    /// Declaration source location.
    pub loc: Loc,
}

/// Sampled-value function node (`$past`, `$rose`, `$fell`, `$stable`, `$changed`, `$sampled`).
///
/// Semantics across cycles:
/// - On each active edge of `clock`, when `gate` is `None` or evaluates to `1'b1`, `expr`
///   is sampled into a shift history of depth `depth`. When `gate` is `0`, the history holds
///   its previous values.
/// - Before `depth` active gated clock ticks have occurred (including cycle `0`), historical
///   samples default to `all_x` in 4-state interpretation (`0` in 2-state backends).
/// - Per IEEE 1800-2023 §16.9.3, `SampledKind::Rose` and `SampledKind::Fell` inspect **LSB bit `0`**
///   of `expr` (`expr[0]`), whereas `SampledKind::Stable` and `SampledKind::Changed` compare the
///   full width of `expr`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampledNode {
    /// Sampled function kind.
    pub kind: SampledKind,
    /// Monitored value in [`Netlist::values`].
    pub expr: ValueId,
    /// Cycle lookback depth (`>= 1` for `$past`, `1` for `$rose`/`$fell`/`$stable`/`$changed`, `0` for `$sampled`).
    pub depth: u32,
    /// Resolved sampling clock (`Some` when inferred from enclosing clocked block, explicit clocking event, or `default clocking`).
    pub clock: Option<ClockSpec>,
    /// Optional 1-bit clock-enable gate (`$past(expr, depth, gate)`).
    pub gate: Option<ValueId>,
    /// Output [`ValueKind::Sampled`] node in [`Netlist::values`].
    pub out: ValueId,
    /// Source location.
    pub loc: Loc,
}

/// Verification directive kind on an immediate [`Check`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckKind {
    /// Safety assertion (`enable` implies `expr` must hold).
    Assert,
    /// Environment assumption (`enable` implies `expr` is constrained to hold).
    Assume,
    /// Reachability cover target (`enable && expr` is reachable).
    Cover,
}

impl CheckKind {
    /// Returns the SystemVerilog keyword (`"assert"`, `"assume"`, or `"cover"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            CheckKind::Assert => "assert",
            CheckKind::Assume => "assume",
            CheckKind::Cover => "cover",
        }
    }
}

/// Immediate procedural or module-level verification check (`assert`, `assume`, `cover`).
///
/// Evaluation rules:
/// - For [`CheckKind::Assert`] and [`CheckKind::Assume`], the obligation is `enable -> expr`
///   (vacuously satisfied when `enable` is `0`).
/// - For [`CheckKind::Cover`], the witness condition is `enable && expr`.
/// - When `clock` is `Some(clk)`, the check is evaluated on active edges of `clk` using
///   pre-update signal values; when `clock` is `None`, the check is combinational and must
///   hold in every step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
    /// Directive kind.
    pub kind: CheckKind,
    /// Unique hierarchical check name (`"top.u0.lbl"`, `"top._assert_1"`, `"top.a_loop_2"`),
    /// or `None` for manually constructed anonymous checks.
    pub label: Option<String>,
    /// `true` for immediate assertions (`assert (...)`), `false` if lowered from a concurrent assertion.
    pub immediate: bool,
    /// 1-bit path-condition enable reaching this check (`1'b1` when unconditional).
    pub enable: ValueId,
    /// 1-bit checked condition in [`Netlist::values`].
    pub expr: ValueId,
    /// Enclosing procedural clock when placed inside a clocked block (or task called from one).
    pub clock: Option<ClockSpec>,
    /// Source location.
    pub loc: Loc,
}

/// A liveness obligation over infinite traces, made from a property such as
/// `req |-> s_eventually gnt`.
///
/// `signal` is one bit. For an assertion (`assume == false`) a trace on which `signal` is true
/// at infinitely many clock ticks is a counterexample: the pass builds `signal` so that it
/// stays true from some tick on exactly when one attempt is never met. For an assumption
/// (`assume == true`) only traces on which `signal` is true at infinitely many ticks count
/// (a fairness constraint).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Liveness {
    /// The property's name ([`Netlist::property_names`], or the name given for a property
    /// bound from text).
    pub label: Option<String>,
    /// `true` for `assume` / `restrict`: a fairness constraint, not a target.
    pub assume: bool,
    /// The 1-bit value in [`Netlist::values`].
    pub signal: ValueId,
    /// The property's clock.
    pub clock: ClockSpec,
    /// Source location.
    pub loc: Loc,
}

/// How a [`UndrivenSlice`] is free.
///
/// The formal attributes `(* anyseq *)` and `(* anyconst *)` on a declaration
/// (as Yosys reads them) state the intent: any value on every step, or any value
/// chosen once and held for the whole trace.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FreeKind {
    /// Nothing drives it (no attribute): consumers usually make it a free input.
    #[default]
    Undriven,
    /// `(* anyseq *)`: an unconstrained value on every step.
    AnySeq,
    /// `(* anyconst *)`: an unconstrained value that never changes.
    AnyConst,
    /// A cut point ([`LowerConfig::cut_points`]): the signal's drivers are ignored and
    /// it is an unconstrained value on every step.
    Cut,
}

impl FreeKind {
    /// The attribute name (`"anyseq"`, `"anyconst"`), `"cut"` for a cut point, or `""` for a
    /// plain undriven slice.
    pub const fn as_str(self) -> &'static str {
        match self {
            FreeKind::Undriven => "",
            FreeKind::AnySeq => "anyseq",
            FreeKind::AnyConst => "anyconst",
            FreeKind::Cut => "cut",
        }
    }
}

/// Undriven physical bit-interval on an internal signal or output port.
///
/// Formal backends typically bind each `UndrivenSlice` to an unconstrained primary input,
/// while lint tools report undriven slices directly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UndrivenSlice {
    /// Hierarchical signal path (`"top.w"`).
    pub signal: String,
    /// Inclusive physical MSB bit offset (`0`-based).
    pub hi: u32,
    /// Inclusive physical LSB bit offset (`0`-based).
    pub lo: u32,
    /// Corresponding [`ValueKind::Undriven`] node in [`Netlist::values`].
    pub value: ValueId,
    /// Declaration location of `signal`.
    pub loc: Loc,
    /// Plain undriven, or free by a formal attribute.
    pub free: FreeKind,
}

/// Named hierarchical signal binding for waveform counterexample traces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceSignal {
    /// Full hierarchical path (`"top.u0.cnt"`).
    pub name: String,
    /// Bit width (`>= 1`).
    pub width: u32,
    /// Signedness.
    pub signed: bool,
    /// Four-state flag.
    pub four_state: bool,
    /// Value node in [`Netlist::values`].
    pub value: ValueId,
    /// Declaration location.
    pub loc: Loc,
}

/// Lowered word-level RTL netlist and bound SVA properties for one elaborated top module.
#[derive(Clone, Debug)]
pub struct Netlist {
    /// Top module name.
    pub top: String,
    /// Source file table indexed by [`Loc::file`] (`files[0]` is `""`).
    pub files: Vec<String>,
    /// Topologically sorted word-level value DAG.
    pub values: Vec<ValueNode>,
    /// Primary top-level inputs.
    pub inputs: Vec<PortInput>,
    /// Primary top-level outputs.
    pub outputs: Vec<PortOutput>,
    /// Sequential flip-flops and latches.
    pub states: Vec<State>,
    /// Unpacked-array memories (expandable via [`Self::blast_memories`]).
    pub memories: Vec<Memory>,
    /// Sampled-value function nodes (`$past`, `$rose`, `$fell`, `$stable`, `$changed`, `$sampled`).
    pub sampled: Vec<SampledNode>,
    /// Immediate `assert`, `assume`, and `cover` checks with path-condition enables.
    pub checks: Vec<Check>,
    /// Undriven signal bit-slices.
    pub undriven: Vec<UndrivenSlice>,
    /// Named hierarchical signals for trace generation.
    pub traces: Vec<TraceSignal>,
    /// Expression arena backing [`Self::properties`].
    pub sva_arena: ExprArena,
    /// Bound concurrent SVA properties (`assert property`, `assume property`, `cover property`, `restrict property`).
    pub properties: Vec<BoundProperty>,
    /// Lowered [`ValueId`] for each boolean, clock, or reset [`ExprId`] in [`Self::sva_arena`] referenced by [`Self::properties`].
    pub prop_expr_values: Vec<Option<ValueId>>,
    /// For each property of [`Self::properties`] that stands in procedural code under a
    /// condition (`always @(posedge clk) if (en) assert property (...)`), the 1-bit path
    /// condition that reaches it: an attempt starts only at a clock tick where it is true
    /// (IEEE 1800-2023 §16.14.6). `None` for a property that is always reached.
    pub property_enables: Vec<Option<ValueId>>,
    /// The words of each read-only memory that [`Self::blast_memories`] turned into
    /// constants, named like the array's elements (`"top.tab[3]"`).
    pub table_words: Vec<TraceSignal>,
    /// Full hierarchical name of each property of [`Self::properties`] (`"top.u0.a_no_overflow"`,
    /// `"top.g[1].a_x"`). A property with no label is named `_assert_<n>`, `_assume_<n>`,
    /// `_cover_<n>` or `_restrict_<n>` in its instance, counted from 1 over the design.
    pub property_names: Vec<String>,
    /// For each property of [`Self::properties`], the [`Self::checks`] index that
    /// [`Self::compile_properties`] made for it (`None` until then, or when it is not in the
    /// compiled subset).
    pub property_checks: Vec<Option<usize>>,
    /// Liveness obligations that [`Self::compile_properties`] made from properties such as
    /// `req |-> s_eventually gnt`.
    pub liveness: Vec<Liveness>,
    /// For each property of [`Self::properties`], the [`Self::liveness`] index made for it.
    pub property_liveness: Vec<Option<usize>>,
    /// Sorted `(SymbolId, ValueId)` map for every elaborated packed/scalar symbol in the design.
    pub symbol_values: Vec<(SymbolId, ValueId)>,
}

/// Configuration controlling RTL netlist lowering.
#[derive(Clone, Debug)]
pub struct LowerConfig {
    /// Optional explicit top module name when multiple top modules exist.
    pub top: Option<String>,
    /// Maximum static loop unroll iterations per loop (`65_536` by default).
    pub max_loop_iters: usize,
    /// When `Some(max_depth)`, expands eligible memories with `depth <= max_depth` via [`Netlist::blast_memories`].
    pub blast_memories_up_to: Option<usize>,
    /// When `false` (default, matching 2-state model checkers), rejects `$isunknown` and 4-state non-constant `casez`/`casex` items with [`LowerOutcome::Unsupported`].
    pub allow_four_state_ops: bool,
    /// Cut points, by full hierarchical path (`"top.u0.cnt"`): each named signal is
    /// disconnected from its drivers and becomes a free value on every step
    /// ([`FreeKind::Cut`] in [`Netlist::undriven`]). Everything that reads the signal sees
    /// the free value; its drivers (a register, an assign, an instance output) are still
    /// lowered but no longer reach it. This is an over-approximation: a property proved
    /// with cut points holds on the design, a counterexample may use values the design
    /// cannot produce. A path that names an aggregate cuts every packed part of it; a path
    /// that names no packed signal is an error.
    /// A cut register keeps its [`State`] in the netlist, with nothing reading it: a back end
    /// can drop the states named by a `Cut` slice.
    pub cut_points: Vec<String>,
    /// Black boxes by module name: every instance of a named module keeps its ports, and
    /// nothing inside it is lowered (its body, its child instances, its assertions). Its
    /// output ports are free values on every step ([`FreeKind::Cut`]); its input ports are
    /// still connected and read by nothing. Like a cut point this is an over-approximation.
    /// The module must still elaborate. A name that matches no instance is an error, and so
    /// is the top module.
    pub black_box_modules: Vec<String>,
    /// Black boxes by instance path (`"top.u_core.u_mem"`): as [`Self::black_box_modules`],
    /// for the named instances only.
    pub black_box_instances: Vec<String>,
    /// What to do with an immediate assertion in an `initial` block that is not a
    /// constant (a time-zero check of a signal, such as OpenTitan's `ASSERT_INIT_NET`).
    /// `false` (default) reports it as [`LowerOutcome::Unsupported`]; `true` leaves it
    /// out of the netlist, as formal tools that ignore `initial` blocks do. The design's
    /// behavior is the same either way: only that one check is not made. Constant
    /// assertions are decided at lowering in both settings.
    pub skip_initial_checks: bool,
    /// What `$isunknown(x)` is while [`Self::allow_four_state_ops`] is `false`. `false`
    /// (default) reports it as [`LowerOutcome::Unsupported`]; `true` lowers it as the
    /// constant 0, since a two-state model has no unknown value, as formal tools do. A
    /// check such as `assert (!$isunknown(x))` then always holds: it says nothing about
    /// the design. Ignored when `allow_four_state_ops` is `true`.
    pub isunknown_is_false: bool,
    /// When `Some(min_width)`, replaces variable-by-variable `Mul`/`UDiv`/`SDiv`/`UMod`/`SMod`
    /// operations of width `>= min_width` with unconstrained [`FreeKind::Cut`] values (`-bbox_mul`).
    pub bbox_mul_width: Option<u32>,
    /// When `Some(min_words)`, abstracts unpacked-array memories with `depth >= min_words` by
    /// replacing their [`ValueKind::MemRead`] ports with unconstrained [`FreeKind::Cut`] values (`-bbox_a`).
    pub bbox_array_words: Option<usize>,
}

impl Default for LowerConfig {
    fn default() -> Self {
        Self {
            top: None,
            max_loop_iters: 65_536,
            blast_memories_up_to: None,
            allow_four_state_ops: false,
            cut_points: Vec::new(),
            black_box_modules: Vec::new(),
            black_box_instances: Vec::new(),
            skip_initial_checks: false,
            isunknown_is_false: false,
            bbox_mul_width: None,
            bbox_array_words: None,
        }
    }
}

/// Fail-closed diagnostic produced during RTL netlist lowering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetlistDiag {
    /// Source location (`1`-based line and byte column).
    pub loc: Loc,
    /// Resolved source file path.
    pub file: String,
    /// Diagnostic message.
    pub message: String,
    /// `true` when the construct is legal SystemVerilog outside the supported synthesizable/formal RTL subset.
    pub unsupported: bool,
}

impl std::fmt::Display for NetlistDiag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let kind = if self.unsupported { "unsupported" } else { "error" };
        write!(f, "{}:{}:{}: {kind}: {}", self.file, self.loc.line, self.loc.column, self.message)
    }
}

/// Tri-state fail-closed result of [`crate::lower_design`] and [`crate::lower_trees`].
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum LowerOutcome {
    /// Lowering succeeded with zero errors or unsupported constructs.
    Lowered(Netlist),
    /// Design is legal SystemVerilog, but uses an unsupported construct (`$isunknown` in 2-state mode, switch-level gates, unbounded loop).
    Unsupported(Vec<NetlistDiag>),
    /// Design contains syntax, semantic, or RTL legality errors (undeclared identifier, overlapping bit drivers, combinational loop, mixed blocking/NBA clocked drivers).
    Error(Vec<NetlistDiag>),
}

impl LowerOutcome {
    /// Unwraps [`LowerOutcome::Lowered`], or panics with formatted diagnostics.
    ///
    /// # Panics
    ///
    /// Panics if `self` is [`LowerOutcome::Unsupported`] or [`LowerOutcome::Error`].
    pub fn unwrap_netlist(self) -> Netlist {
        match self {
            LowerOutcome::Lowered(n) => n,
            LowerOutcome::Unsupported(ds) | LowerOutcome::Error(ds) => {
                let text: Vec<String> = ds.iter().map(ToString::to_string).collect();
                panic!("expected Lowered netlist, got:\n{}", text.join("\n"));
            }
        }
    }

    /// Borrows diagnostics when `self` is [`LowerOutcome::Unsupported`] or [`LowerOutcome::Error`].
    pub fn diagnostics(&self) -> &[NetlistDiag] {
        match self {
            LowerOutcome::Lowered(_) => &[],
            LowerOutcome::Unsupported(d) | LowerOutcome::Error(d) => d,
        }
    }
}

impl Netlist {
    /// Borrows a [`ValueNode`] by [`ValueId`].
    ///
    /// # Panics
    ///
    /// Panics if `id.0 as usize >= self.values.len()`.
    #[inline]
    pub fn value(&self, id: ValueId) -> &ValueNode {
        &self.values[id.0 as usize]
    }

    /// Returns the lowered [`ValueId`] corresponding to `id` in [`Self::sva_arena`] when `id` is a
    /// boolean, clock, or reset expression referenced by [`Self::properties`].
    #[inline]
    pub fn prop_expr_value(&self, id: ExprId) -> Option<ValueId> {
        self.prop_expr_values.get(id.0 as usize).copied().flatten()
    }

    /// Looks up the lowered [`ValueId`] of an elaborated packed or scalar [`SymbolId`] via binary search.
    #[inline]
    pub fn symbol_value(&self, sym: SymbolId) -> Option<ValueId> {
        self.symbol_values.binary_search_by_key(&sym.0, |(s, _)| s.0).ok().map(|idx| self.symbol_values[idx].1)
    }

    /// Lowers all boolean, clock, and reset [`ExprId`]s referenced by an externally bound
    /// [`BoundProperty`] (such as one produced by `Compilation::bind_property_in_scope`) into
    /// [`Self::values`] in topological order.
    ///
    /// Returns `Ok(pairs)` mapping each lowered [`ExprId`] in `arena` to its topologically sorted
    /// [`ValueId`] in `self.values`. Note that calling this method may re-index [`Self::values`]
    /// (updating [`Self::prop_expr_values`], [`Self::symbol_values`], ports, states, and checks
    /// consistently). On error or unsupported construct, `self` is left unmodified and `Err(diags)`
    /// is returned.
    pub fn lower_external_property(
        &mut self,
        design: &ElaboratedDesign,
        arena: &ExprArena,
        prop: &BoundProperty,
        allow_four_state_ops: bool,
    ) -> Result<Vec<(ExprId, ValueId)>, Vec<NetlistDiag>> {
        crate::lower::lower_external_property_on_netlist(self, design, arena, prop, allow_four_state_ops)
    }

    /// Expands eligible [`Memory`] arrays with `depth as usize <= max_depth`:
    /// - Read-only memories (`writes.is_empty()`) expand into pure combinational mux trees over
    ///   [`Memory::init`] (and out-of-bounds default) with zero [`State`] registers.
    /// - Single-clock synchronous memories (where every [`MemWritePort`] has `clock.is_some()` and
    ///   shares the exact same [`ClockSpec`]) expand into `depth` word [`State`] registers with
    ///   write-enable mux chains on `next` and read mux trees replacing [`ValueKind::MemRead`].
    /// - Memories with `depth as usize > max_depth`, unclocked write ports, or multi-clock write
    ///   ports remain in [`Self::memories`].
    ///
    /// Preserves the topological ordering of [`Self::values`] and remaps [`Self::prop_expr_values`]
    /// and [`Self::symbol_values`].
    pub fn blast_memories(&mut self, max_depth: usize) {
        enum BlastedMem {
            Kept(MemId),
            Rom { width: u32, depth: u32, signed: bool, four_state: bool, init: Vec<(u32, CVal)> },
            Regs { depth: u32, outs: Vec<ValueId> },
        }

        let can_blast = |m: &Memory| -> bool {
            if (m.depth as usize) > max_depth {
                return false;
            }
            if m.writes.is_empty() {
                return true;
            }
            let Some(first_clk) = m.writes[0].clock else {
                return false;
            };
            m.writes.iter().all(|wp| wp.clock == Some(first_clk))
        };

        if !self.memories.iter().any(can_blast) {
            return;
        }

        let mut actions: Vec<Option<BlastedMem>> = (0..self.memories.len()).map(|_| None).collect();
        let mut kept_memories: Vec<Memory> = Vec::new();

        let old_memories = std::mem::take(&mut self.memories);
        for mem in old_memories {
            let mid_idx = mem.id.0 as usize;
            if !can_blast(&mem) {
                let new_id = MemId(kept_memories.len() as u32);
                actions[mid_idx] = Some(BlastedMem::Kept(new_id));
                let mut m = mem;
                m.id = new_id;
                kept_memories.push(m);
                continue;
            }

            if mem.writes.is_empty() {
                // Each word under the name of its array element, so that a property bound
                // after lowering can still read `mem[i]` (kept apart from the traces: a
                // table of constants is not a waveform).
                for w_idx in 0..mem.depth {
                    let cv = mem.init.iter().find(|(i, _)| *i == w_idx).map(|(_, c)| c.clone()).unwrap_or_else(|| {
                        if mem.four_state {
                            CVal::all_x(mem.width, mem.signed)
                        } else {
                            CVal::new(mem.width, mem.signed, 0)
                        }
                    });
                    let value = self.push_const(cv, mem.four_state, mem.loc);
                    self.table_words.push(TraceSignal {
                        name: format!("{}[{}]", mem.name, mem.lo_idx + i128::from(w_idx)),
                        width: mem.width,
                        signed: mem.signed,
                        four_state: mem.four_state,
                        value,
                        loc: mem.loc,
                    });
                }
                actions[mid_idx] = Some(BlastedMem::Rom {
                    width: mem.width,
                    depth: mem.depth,
                    signed: mem.signed,
                    four_state: mem.four_state,
                    init: mem.init,
                });
                continue;
            }

            let clk = mem.writes[0].clock;
            let req_addr_w = crate::expr_lower::addr_bits(mem.depth);
            let mut outs = Vec::with_capacity(mem.depth as usize);
            for w_idx in 0..mem.depth {
                let state_id = StateId(self.states.len() as u32);
                let out_val = ValueId(self.values.len() as u32);
                let word_name = format!("{}[{}]", mem.name, mem.lo_idx + (w_idx as i128));
                self.values.push(ValueNode {
                    id: out_val,
                    width: mem.width,
                    signed: mem.signed,
                    four_state: mem.four_state,
                    kind: ValueKind::StateOut(state_id),
                    name: Some(word_name.clone()),
                    loc: mem.loc,
                });

                let mut cur = out_val;
                for wp in &mem.writes {
                    let wp_addr_node = self.value(wp.addr);
                    let addr_w = wp_addr_node.width;
                    let addr_four = wp_addr_node.four_state;
                    let cmp_w = addr_w.max(req_addr_w);
                    let addr_cmp = if addr_w < cmp_w {
                        self.push_op(NetOp::ZeroExt { width: cmp_w }, vec![wp.addr], cmp_w, false, addr_four, wp.loc)
                    } else {
                        wp.addr
                    };
                    let idx_const = self.push_const(CVal::new(cmp_w, false, w_idx as i128), false, wp.loc);
                    let eq_val = self.push_op(NetOp::Eq, vec![addr_cmp, idx_const], 1, false, addr_four, wp.loc);
                    let en_four = self.value(wp.enable).four_state;
                    let hit_four = en_four || addr_four;
                    let hit = self.push_op(NetOp::And, vec![wp.enable, eq_val], 1, false, hit_four, wp.loc);
                    let data_four = self.value(wp.data).four_state;
                    let cur_four = self.value(cur).four_state;
                    let mux_four = mem.four_state || hit_four || data_four || cur_four;
                    cur = self.push_op(NetOp::Mux, vec![hit, wp.data, cur], mem.width, mem.signed, mux_four, wp.loc);
                }
                if !mem.four_state && self.value(cur).four_state {
                    cur = self.push_op(
                        NetOp::CastFourState { four_state: false },
                        vec![cur],
                        mem.width,
                        mem.signed,
                        false,
                        mem.loc,
                    );
                }

                let init_cv = mem.init.iter().find(|(i, _)| *i == w_idx).map(|(_, c)| c.clone());
                // Named like an element of the array, so that a property bound after
                // lowering can still read `mem[i]`.
                self.traces.push(TraceSignal {
                    name: word_name.clone(),
                    width: mem.width,
                    signed: mem.signed,
                    four_state: mem.four_state,
                    value: out_val,
                    loc: mem.loc,
                });
                self.states.push(State {
                    id: state_id,
                    name: word_name,
                    width: mem.width,
                    signed: mem.signed,
                    four_state: mem.four_state,
                    clock: clk,
                    async_reset: None,
                    latch_enable: None,
                    init: init_cv,
                    next: cur,
                    out: out_val,
                    loc: mem.loc,
                });
                outs.push(out_val);
            }
            actions[mid_idx] = Some(BlastedMem::Regs { depth: mem.depth, outs });
        }
        self.memories = kept_memories;

        let orig_len = self.values.len();
        for vi in 0..orig_len {
            let (mid_opt, addr_val, vw, vs, v4, vloc) = match &self.values[vi].kind {
                ValueKind::MemRead { mem, addr } => (
                    Some(*mem),
                    *addr,
                    self.values[vi].width,
                    self.values[vi].signed,
                    self.values[vi].four_state,
                    self.values[vi].loc,
                ),
                _ => (None, ValueId(0), 0, false, false, Loc::default()),
            };
            let Some(mid) = mid_opt else { continue };
            let Some(action) = actions[mid.0 as usize].as_ref() else { continue };
            match action {
                BlastedMem::Kept(new_mid) => {
                    self.values[vi].kind = ValueKind::MemRead { mem: *new_mid, addr: addr_val };
                }
                BlastedMem::Rom { width, depth, signed, four_state, init } => {
                    let mw = *width;
                    let md = *depth;
                    let ms = *signed;
                    let m4 = *four_state;
                    let init_clone = init.clone();
                    let req_addr_w = crate::expr_lower::addr_bits(md);
                    let addr_w = self.value(addr_val).width;
                    let addr_four = self.value(addr_val).four_state;
                    let cmp_w = addr_w.max(req_addr_w);
                    let addr_cmp = if addr_w < cmp_w {
                        self.push_op(NetOp::ZeroExt { width: cmp_w }, vec![addr_val], cmp_w, false, addr_four, vloc)
                    } else {
                        addr_val
                    };
                    let mut word_vals = Vec::with_capacity(md as usize);
                    for k in 0..md {
                        let cv = init_clone
                            .iter()
                            .find(|(idx, _)| *idx == k)
                            .map(|(_, c)| c.clone())
                            .unwrap_or_else(|| if m4 { CVal::all_x(mw, ms) } else { CVal::new(mw, ms, 0) });
                        word_vals.push(self.push_const(cv, m4, vloc));
                    }
                    let oob_cv = if v4 { CVal::all_x(vw, vs) } else { CVal::new(vw, vs, 0) };
                    let mut acc = self.push_const(oob_cv, v4, vloc);
                    for (k, &w_val) in word_vals.iter().enumerate().rev() {
                        let k_const = self.push_const(CVal::new(cmp_w, false, k as i128), false, vloc);
                        let eq = self.push_op(NetOp::Eq, vec![addr_cmp, k_const], 1, false, addr_four, vloc);
                        let mux_four = addr_four || m4 || self.value(acc).four_state;
                        acc = self.push_op(NetOp::Mux, vec![eq, w_val, acc], vw, vs, mux_four, vloc);
                    }
                    if !v4 && self.value(acc).four_state {
                        acc = self.push_op(NetOp::CastFourState { four_state: false }, vec![acc], vw, vs, false, vloc);
                    }
                    self.values[vi].kind = self.values[acc.0 as usize].kind.clone();
                }
                BlastedMem::Regs { depth, outs } => {
                    let md = *depth;
                    let outs_clone = outs.clone();
                    let req_addr_w = crate::expr_lower::addr_bits(md);
                    let addr_w = self.value(addr_val).width;
                    let addr_four = self.value(addr_val).four_state;
                    let cmp_w = addr_w.max(req_addr_w);
                    let addr_cmp = if addr_w < cmp_w {
                        self.push_op(NetOp::ZeroExt { width: cmp_w }, vec![addr_val], cmp_w, false, addr_four, vloc)
                    } else {
                        addr_val
                    };
                    let oob_cv = if v4 { CVal::all_x(vw, vs) } else { CVal::new(vw, vs, 0) };
                    let mut acc = self.push_const(oob_cv, v4, vloc);
                    for (k, &w_out) in outs_clone.iter().enumerate().rev() {
                        let k_const = self.push_const(CVal::new(cmp_w, false, k as i128), false, vloc);
                        let eq = self.push_op(NetOp::Eq, vec![addr_cmp, k_const], 1, false, addr_four, vloc);
                        let w_four = self.value(w_out).four_state;
                        let mux_four = addr_four || w_four || self.value(acc).four_state;
                        acc = self.push_op(NetOp::Mux, vec![eq, w_out, acc], vw, vs, mux_four, vloc);
                    }
                    if !v4 && self.value(acc).four_state {
                        acc = self.push_op(NetOp::CastFourState { four_state: false }, vec![acc], vw, vs, false, vloc);
                    }
                    self.values[vi].kind = self.values[acc.0 as usize].kind.clone();
                }
            }
        }

        self.toposort();
    }

    /// Abstracts variable-by-variable multiplier, divider, and modulo operations (`*`, `/`, `%`)
    /// whose result or operand bit width is `>= min_width` into unconstrained [`FreeKind::Cut`]
    /// values (`elaborate -bbox_mul`). Returns the number of arithmetic cones boxed.
    pub fn black_box_multipliers(&mut self, min_width: u32) -> usize {
        let mut boxed = 0usize;
        for vi in 0..self.values.len() {
            let should_box = match &self.values[vi].kind {
                ValueKind::Op { op: NetOp::Mul | NetOp::UDiv | NetOp::SDiv | NetOp::UMod | NetOp::SMod, args } => {
                    let max_w = self.values[vi].width.max(args.iter().map(|&a| self.value(a).width).max().unwrap_or(0));
                    max_w >= min_width && args.iter().all(|&a| !matches!(self.value(a).kind, ValueKind::Const(_)))
                }
                _ => false,
            };
            if !should_box {
                continue;
            }
            let uid = self.undriven.len();
            let sig = self.values[vi].name.clone().unwrap_or_else(|| format!("{}._bbox_mul_{boxed}", self.top));
            let w = self.values[vi].width;
            let vid = self.values[vi].id;
            let loc = self.values[vi].loc;
            self.undriven.push(UndrivenSlice {
                signal: sig.clone(),
                hi: w - 1,
                lo: 0,
                value: vid,
                loc,
                free: FreeKind::Cut,
            });
            self.values[vi].kind = ValueKind::Undriven(uid);
            if self.values[vi].name.is_none() {
                self.values[vi].name = Some(sig);
            }
            boxed += 1;
        }
        if boxed > 0 {
            self.toposort();
        }
        boxed
    }

    /// Abstracts [`Memory`] arrays with `(depth as usize) >= min_words` by replacing all of their
    /// [`ValueKind::MemRead`] ports with unconstrained [`FreeKind::Cut`] values and removing the
    /// boxed memories (`elaborate -bbox_a`). Returns the number of memories boxed.
    pub fn black_box_memories(&mut self, min_words: usize) -> usize {
        if !self.memories.iter().any(|m| (m.depth as usize) >= min_words) {
            return 0;
        }
        let mut remap: Vec<Option<MemId>> = vec![None; self.memories.len()];
        let mut mem_names: Vec<String> = vec![String::new(); self.memories.len()];
        let mut kept: Vec<Memory> = Vec::new();
        let mut boxed = 0usize;
        for mut mem in std::mem::take(&mut self.memories) {
            let idx = mem.id.0 as usize;
            mem_names[idx] = mem.name.clone();
            if (mem.depth as usize) >= min_words {
                boxed += 1;
            } else {
                let new_id = MemId(kept.len() as u32);
                mem.id = new_id;
                remap[idx] = Some(new_id);
                kept.push(mem);
            }
        }
        self.memories = kept;

        let mut read_k = 0usize;
        for vi in 0..self.values.len() {
            let mem_idx = match &mut self.values[vi].kind {
                ValueKind::MemRead { mem, .. } => {
                    let old = mem.0 as usize;
                    if let Some(new_mid) = remap[old] {
                        *mem = new_mid;
                        None
                    } else {
                        Some(old)
                    }
                }
                _ => None,
            };
            let Some(old_idx) = mem_idx else { continue };
            let uid = self.undriven.len();
            let sig =
                self.values[vi].name.clone().unwrap_or_else(|| format!("{}._bbox_rd_{read_k}", mem_names[old_idx]));
            read_k += 1;
            let w = self.values[vi].width;
            let vid = self.values[vi].id;
            let loc = self.values[vi].loc;
            self.undriven.push(UndrivenSlice {
                signal: sig.clone(),
                hi: w - 1,
                lo: 0,
                value: vid,
                loc,
                free: FreeKind::Cut,
            });
            self.values[vi].kind = ValueKind::Undriven(uid);
            if self.values[vi].name.is_none() {
                self.values[vi].name = Some(sig);
            }
        }
        self.toposort();
        boxed
    }

    fn alloc_post_reset_gate(&mut self, skip_cycles: u32) -> ValueId {
        let mut gate = self.push_const(CVal::new(1, false, 1), false, Loc::default());
        if skip_cycles == 0 {
            return gate;
        }
        let clk = self
            .states
            .iter()
            .find_map(|s| s.clock)
            .or_else(|| self.checks.iter().find_map(|c| c.clock))
            .or_else(|| self.inputs.first().map(|p| ClockSpec { signal: p.value, edge: Edge::Posedge }));
        let Some(clock) = clk else {
            return gate;
        };
        for step in 0..skip_cycles {
            let sid = StateId(self.states.len() as u32);
            let out_val = ValueId(self.values.len() as u32);
            let name = format!("{}._cov_post_reset_{step}", self.top);
            self.values.push(ValueNode {
                id: out_val,
                width: 1,
                signed: false,
                four_state: false,
                kind: ValueKind::StateOut(sid),
                name: Some(name.clone()),
                loc: Loc::default(),
            });
            self.states.push(State {
                id: sid,
                name,
                width: 1,
                signed: false,
                four_state: false,
                clock: Some(clock),
                async_reset: None,
                latch_enable: None,
                init: Some(CVal::new(1, false, 0)),
                next: gate,
                out: out_val,
                loc: Loc::default(),
            });
            gate = out_val;
        }
        gate
    }

    /// Synthesizes paired true/false [`CheckKind::Cover`] targets (`<top>._cov_branch_<k>_true`
    /// and `<top>._cov_branch_<k>_false`) on every source-located non-constant [`NetOp::Mux`]
    /// condition in the netlist (`check_cov -type branch`), gated off for the first `skip_steps`
    /// model steps when a reset sequence is active. Returns the number of covers added.
    pub fn add_branch_covers(&mut self, skip_steps: u32) -> usize {
        let mut candidates: Vec<(ValueId, Loc)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for v in &self.values {
            let ValueKind::Op { op: NetOp::Mux, args } = &v.kind else { continue };
            if args.len() != 3 || v.loc.line == 0 {
                continue;
            }
            let cond = args[0];
            if matches!(self.value(cond).kind, ValueKind::Const(_)) {
                continue;
            }
            if seen.insert((cond, v.loc.file, v.loc.line)) {
                candidates.push((cond, v.loc));
            }
        }
        if candidates.is_empty() {
            return 0;
        }
        let enable = self.alloc_post_reset_gate(skip_steps);
        let mut added = 0usize;
        for (idx, (cond, loc)) in candidates.into_iter().enumerate() {
            let cond_b = if self.value(cond).width == 1 {
                cond
            } else {
                let four = self.value(cond).four_state;
                self.push_op(NetOp::RedOr, vec![cond], 1, false, four, loc)
            };
            let four = self.value(cond_b).four_state;
            let neg = self.push_op(NetOp::Not, vec![cond_b], 1, false, four, loc);
            self.checks.push(Check {
                kind: CheckKind::Cover,
                immediate: true,
                label: Some(format!("{}._cov_branch_{idx}_true", self.top)),
                enable,
                expr: cond_b,
                clock: None,
                loc,
            });
            self.checks.push(Check {
                kind: CheckKind::Cover,
                immediate: true,
                label: Some(format!("{}._cov_branch_{idx}_false", self.top)),
                enable,
                expr: neg,
                clock: None,
                loc,
            });
            added += 2;
        }
        self.toposort();
        added
    }

    /// Synthesizes `0 -> 1` (`_rise`) and `1 -> 0` (`_fall`) [`CheckKind::Cover`] targets on
    /// register bits in [`Self::states`] (`check_cov -type toggle`, capped at 32 bits per register
    /// or word-level `next != out` for wider buses), gated off for the first `skip_steps` model
    /// steps when a reset sequence is active. Returns the number of covers added.
    pub fn add_toggle_covers(&mut self, skip_steps: u32) -> usize {
        if self.states.is_empty() {
            return 0;
        }
        let snapshot: Vec<State> = self
            .states
            .iter()
            .filter(|st| {
                !st.name.contains("._cov_post_reset_")
                    && !self.undriven.iter().any(|u| u.free == FreeKind::Cut && u.signal == st.name)
            })
            .cloned()
            .collect();
        if snapshot.is_empty() {
            return 0;
        }
        let enable = self.alloc_post_reset_gate(skip_steps);
        let mut added = 0usize;
        for st in snapshot {
            let (name, width, four, clock, out, next, loc) =
                (st.name, st.width, st.four_state, st.clock, st.out, st.next, st.loc);
            let safe: String =
                name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '_' }).collect();
            if width <= 32 {
                for bit in 0..width {
                    let (ob, nb) = if width == 1 {
                        (out, next)
                    } else {
                        let o = self.push_op(NetOp::Slice { hi: bit, lo: bit }, vec![out], 1, false, four, loc);
                        let n = self.push_op(NetOp::Slice { hi: bit, lo: bit }, vec![next], 1, false, four, loc);
                        (o, n)
                    };
                    let not_o = self.push_op(NetOp::Not, vec![ob], 1, false, four, loc);
                    let not_n = self.push_op(NetOp::Not, vec![nb], 1, false, four, loc);
                    let rise = self.push_op(NetOp::And, vec![not_o, nb], 1, false, four, loc);
                    let fall = self.push_op(NetOp::And, vec![ob, not_n], 1, false, four, loc);
                    let suffix = if width == 1 { String::new() } else { format!("_{bit}") };
                    self.checks.push(Check {
                        kind: CheckKind::Cover,
                        immediate: true,
                        label: Some(format!("{safe}._cov_toggle{suffix}_rise")),
                        enable,
                        expr: rise,
                        clock,
                        loc,
                    });
                    self.checks.push(Check {
                        kind: CheckKind::Cover,
                        immediate: true,
                        label: Some(format!("{safe}._cov_toggle{suffix}_fall")),
                        enable,
                        expr: fall,
                        clock,
                        loc,
                    });
                    added += 2;
                }
            } else {
                let chg = self.push_op(NetOp::Neq, vec![out, next], 1, false, four, loc);
                self.checks.push(Check {
                    kind: CheckKind::Cover,
                    immediate: true,
                    label: Some(format!("{safe}._cov_toggle_chg")),
                    enable,
                    expr: chg,
                    clock,
                    loc,
                });
                added += 1;
            }
        }
        self.toposort();
        added
    }

    fn push_const(&mut self, cval: CVal, four_state: bool, loc: Loc) -> ValueId {
        let id = ValueId(self.values.len() as u32);
        let w = cval.w.max(1);
        let s = cval.s;
        self.values.push(ValueNode {
            id,
            width: w,
            signed: s,
            four_state,
            kind: ValueKind::Const(cval),
            name: None,
            loc,
        });
        id
    }

    fn push_op(
        &mut self,
        op: NetOp,
        args: Vec<ValueId>,
        width: u32,
        signed: bool,
        four_state: bool,
        loc: Loc,
    ) -> ValueId {
        let id = ValueId(self.values.len() as u32);
        self.values.push(ValueNode {
            id,
            width,
            signed,
            four_state,
            kind: ValueKind::Op { op, args },
            name: None,
            loc,
        });
        id
    }

    /// Re-sorts [`Self::values`] into topological order using an iterative heap stack and
    /// compacts unreachable temporary nodes, remapping every [`ValueId`] across `inputs`,
    /// `outputs`, `states`, `memories`, `sampled`, `checks`, `undriven`, `traces`,
    /// `prop_expr_values`, and `symbol_values`.
    pub fn toposort(&mut self) {
        let n = self.values.len();
        let mut state = vec![0u8; n];
        let mut order: Vec<usize> = Vec::with_capacity(n);
        let mut stack: Vec<(usize, bool)> = Vec::new();

        let visit_root = |root: usize,
                          values: &[ValueNode],
                          memories: &[Memory],
                          sampled: &[SampledNode],
                          state: &mut [u8],
                          stack: &mut Vec<(usize, bool)>,
                          order: &mut Vec<usize>| {
            if state[root] == 2 {
                return;
            }
            stack.push((root, false));
            while let Some((idx, post)) = stack.pop() {
                if post {
                    if state[idx] != 2 {
                        state[idx] = 2;
                        order.push(idx);
                    }
                    continue;
                }
                if state[idx] != 0 {
                    continue;
                }
                state[idx] = 1;
                stack.push((idx, true));
                match &values[idx].kind {
                    ValueKind::Input(_) | ValueKind::Const(_) | ValueKind::StateOut(_) | ValueKind::Undriven(_) => {}
                    ValueKind::Op { args, .. } => {
                        for &a in args.iter().rev() {
                            let ai = a.0 as usize;
                            if state[ai] == 0 {
                                stack.push((ai, false));
                            }
                        }
                    }
                    ValueKind::MemRead { mem, addr } => {
                        for wp in memories[mem.0 as usize].writes.iter().rev() {
                            if wp.clock.is_none() {
                                for d in [wp.enable, wp.data, wp.addr] {
                                    let di = d.0 as usize;
                                    if state[di] == 0 {
                                        stack.push((di, false));
                                    }
                                }
                            }
                        }
                        let ai = addr.0 as usize;
                        if state[ai] == 0 {
                            stack.push((ai, false));
                        }
                    }
                    ValueKind::Sampled(si) => {
                        let sn = &sampled[*si];
                        if sn.kind != SampledKind::Past || sn.depth == 0 {
                            let ei = sn.expr.0 as usize;
                            if state[ei] == 0 {
                                stack.push((ei, false));
                            }
                        }
                    }
                }
            }
        };

        for p in &self.inputs {
            visit_root(
                p.value.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }
        for s in &self.states {
            visit_root(
                s.out.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }
        for u in &self.undriven {
            visit_root(
                u.value.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }
        for o in &self.outputs {
            visit_root(
                o.value.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }
        for s in &self.states {
            if let Some(clk) = s.clock {
                visit_root(
                    clk.signal.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
            if let Some(ar) = s.async_reset {
                visit_root(
                    ar.signal.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
                visit_root(
                    ar.value.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
            if let Some(le) = s.latch_enable {
                visit_root(
                    le.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
            visit_root(
                s.next.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }
        for m in &self.memories {
            for wp in &m.writes {
                if let Some(clk) = wp.clock {
                    visit_root(
                        clk.signal.0 as usize,
                        &self.values,
                        &self.memories,
                        &self.sampled,
                        &mut state,
                        &mut stack,
                        &mut order,
                    );
                }
                visit_root(
                    wp.addr.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
                visit_root(
                    wp.data.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
                visit_root(
                    wp.enable.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
        }
        for sn in &self.sampled {
            visit_root(
                sn.out.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
            visit_root(
                sn.expr.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
            if let Some(clk) = sn.clock {
                visit_root(
                    clk.signal.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
            if let Some(g) = sn.gate {
                visit_root(
                    g.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
        }
        for c in &self.checks {
            visit_root(
                c.enable.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
            visit_root(
                c.expr.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
            if let Some(clk) = c.clock {
                visit_root(
                    clk.signal.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
        }
        for v in self.prop_expr_values.iter().flatten() {
            visit_root(v.0 as usize, &self.values, &self.memories, &self.sampled, &mut state, &mut stack, &mut order);
        }
        for &(_, v) in &self.symbol_values {
            visit_root(v.0 as usize, &self.values, &self.memories, &self.sampled, &mut state, &mut stack, &mut order);
        }
        for v in self.property_enables.iter().flatten() {
            visit_root(v.0 as usize, &self.values, &self.memories, &self.sampled, &mut state, &mut stack, &mut order);
        }
        for l in &self.liveness {
            for v in [l.signal, l.clock.signal] {
                visit_root(
                    v.0 as usize,
                    &self.values,
                    &self.memories,
                    &self.sampled,
                    &mut state,
                    &mut stack,
                    &mut order,
                );
            }
        }
        for tr in self.traces.iter().chain(&self.table_words) {
            visit_root(
                tr.value.0 as usize,
                &self.values,
                &self.memories,
                &self.sampled,
                &mut state,
                &mut stack,
                &mut order,
            );
        }

        let mut remap = vec![ValueId(0); n];
        let mut new_values = Vec::with_capacity(order.len());
        for (new_idx, &old_idx) in order.iter().enumerate() {
            let new_id = ValueId(new_idx as u32);
            remap[old_idx] = new_id;
            let mut node = self.values[old_idx].clone();
            node.id = new_id;
            if let ValueKind::Op { args, .. } = &mut node.kind {
                for a in args {
                    *a = remap[a.0 as usize];
                }
            } else if let ValueKind::MemRead { addr, .. } = &mut node.kind {
                *addr = remap[addr.0 as usize];
            }
            new_values.push(node);
        }
        self.values = new_values;

        let map_v = |v: &mut ValueId| *v = remap[v.0 as usize];
        for p in &mut self.inputs {
            map_v(&mut p.value);
        }
        for o in &mut self.outputs {
            map_v(&mut o.value);
        }
        for s in &mut self.states {
            if let Some(clk) = &mut s.clock {
                map_v(&mut clk.signal);
            }
            if let Some(ar) = &mut s.async_reset {
                map_v(&mut ar.signal);
                map_v(&mut ar.value);
            }
            if let Some(le) = &mut s.latch_enable {
                map_v(le);
            }
            map_v(&mut s.next);
            map_v(&mut s.out);
        }
        for m in &mut self.memories {
            for wp in &mut m.writes {
                if let Some(clk) = &mut wp.clock {
                    map_v(&mut clk.signal);
                }
                map_v(&mut wp.addr);
                map_v(&mut wp.data);
                map_v(&mut wp.enable);
            }
        }
        for sn in &mut self.sampled {
            map_v(&mut sn.expr);
            if let Some(clk) = &mut sn.clock {
                map_v(&mut clk.signal);
            }
            if let Some(g) = &mut sn.gate {
                map_v(g);
            }
            map_v(&mut sn.out);
        }
        for c in &mut self.checks {
            map_v(&mut c.enable);
            map_v(&mut c.expr);
            if let Some(clk) = &mut c.clock {
                map_v(&mut clk.signal);
            }
        }
        for u in &mut self.undriven {
            map_v(&mut u.value);
        }
        for tr in self.traces.iter_mut().chain(&mut self.table_words) {
            map_v(&mut tr.value);
        }
        for pv in self.prop_expr_values.iter_mut().flatten() {
            map_v(pv);
        }
        for pv in self.property_enables.iter_mut().flatten() {
            map_v(pv);
        }
        for l in &mut self.liveness {
            map_v(&mut l.signal);
            map_v(&mut l.clock.signal);
        }
        for (_, sv) in &mut self.symbol_values {
            map_v(sv);
        }
    }
}
