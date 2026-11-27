//! Bit-interval driver resolution, tri-state bus synthesis, and hierarchical `inout` / `alias`
//! net equivalence merging (IEEE 1800-2023 §6.5 / §9.2.2 / §10.3 / §23.3.3).
//!
//! Supports disjoint bit-slice / struct-member drivers across separate `always_ff` blocks,
//! `assign` statements, or child module output ports (`multi_driver_ff`, `multi_driver_struct`),
//! resolves multi-driver tri-state nets (`wire`/`tri`, `tri0`, `tri1`, `pullup`/`pulldown`),
//! splits top-level `inout` ports into `io$in` + `(io$oe, io$out)` (`io = io$oe ? io$out : io$in`),
//! merges internal hierarchical `inout` ports and `alias` nets via bit-level union-find without
//! combinational feedback loops, rejects illegal variable / non-tristate overlaps fail-closed,
//! and emits [`UndrivenSlice`] nodes for undriven bit ranges.

use sv_semantic::fx::{FxMap, FxSet};
use sv_semantic::{CVal, Int, Loc};

use crate::builder::{BuildKind, NetBuilder};
use crate::ir::{FreeKind, NetOp, PortOutput, TraceSignal, TristateBus, UndrivenSlice, ValueId};

/// Origin category of a bit-slice driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DriverKind {
    /// Top-level or hierarchical input port connection.
    InputPort,
    /// Child module output port connection.
    ChildOutput,
    /// Black-box `inout` port cut-point driver (`(bb$oe, bb$out)`).
    BlackBoxInout,
    /// `assign lhs = rhs;` or gate output.
    ContinuousAssign,
    /// `always_comb` or `always @*` procedural block.
    AlwaysComb,
    /// `always_latch` procedural block.
    AlwaysLatch,
    /// Clocked `always_ff` or `always @(posedge/negedge ...)` block.
    AlwaysFf,
}

impl DriverKind {
    /// Human-readable description for multi-driver diagnostics.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            DriverKind::InputPort => "input port",
            DriverKind::ChildOutput => "instance output port",
            DriverKind::BlackBoxInout => "black-box inout port",
            DriverKind::ContinuousAssign => "continuous assignment",
            DriverKind::AlwaysComb => "always_comb block",
            DriverKind::AlwaysLatch => "always_latch block",
            DriverKind::AlwaysFf => "clocked procedural block",
        }
    }

    /// `true` for procedural drivers (`always_ff`, `always_comb`, `always_latch`).
    pub(crate) const fn is_procedural(self) -> bool {
        matches!(self, DriverKind::AlwaysComb | DriverKind::AlwaysLatch | DriverKind::AlwaysFf)
    }
}

/// Tri-state enable condition on a [`BitSliceDriver`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TriEnable {
    /// Unconditionally active non-tristate driver (`en == '1`).
    Always,
    /// Pure high-impedance driver (`assign bus = 'z;`, `en == '0`).
    Never,
    /// Uniform 1-bit output-enable across `[lo ..= hi]`.
    Bit(ValueId),
    /// Per-bit output-enable mask of width `hi - lo + 1` (`1` = active, `0` = high-Z).
    Mask(ValueId),
}

/// One driven contiguous physical bit interval `[lo ..= hi]` (`0 <= lo <= hi < width`).
#[derive(Clone, Debug)]
pub(crate) struct BitSliceDriver {
    /// Inclusive physical LSB offset (`0`-based).
    pub(crate) lo: u32,
    /// Inclusive physical MSB offset (`0`-based, `width == hi - lo + 1`).
    pub(crate) hi: u32,
    /// Driven value of width `hi - lo + 1`.
    pub(crate) value: ValueId,
    /// Tri-state enable condition (`Always` for ordinary drivers).
    pub(crate) en: TriEnable,
    /// Driver category.
    pub(crate) kind: DriverKind,
    /// Source location of this driver.
    pub(crate) loc: Loc,
}

/// Top-level `inout` port split metadata attached to a [`SignalSlot`].
#[derive(Clone, Debug)]
pub(crate) struct TopInoutSplit {
    /// Top-level port relative name (`"io"`, `"pkt.io"`, or `"io_arr[0]"`).
    pub(crate) port_rel_name: String,
    /// External drive primary input [`ValueId`] (`<port>$in`).
    pub(crate) in_val: ValueId,
}

/// Bidirectional net equivalence between `[lo_a .. lo_a + width - 1]` of `sidx_a` and
/// `[lo_b .. lo_b + width - 1]` of `sidx_b` (from internal `inout` ports or `alias`).
#[derive(Clone, Debug)]
pub(crate) struct NetEquiv {
    pub(crate) sidx_a: usize,
    pub(crate) lo_a: u32,
    pub(crate) sidx_b: usize,
    pub(crate) lo_b: u32,
    pub(crate) width: u32,
    pub(crate) loc: Loc,
}

/// Per-signal driver accumulator for a flattened packed signal or scalarized struct member.
#[derive(Clone, Debug)]
pub(crate) struct SignalSlot {
    /// Full hierarchical name (`"top.u0.q"` or `"top.pkt.valid"`).
    pub(crate) name: String,
    /// Bit width (`>= 1`).
    pub(crate) width: u32,
    /// Signedness.
    pub(crate) signed: bool,
    /// Four-state flag.
    pub(crate) four_state: bool,
    /// `true` when declared as a net (`wire`, `tri`, ...) rather than a variable (`logic`, `reg`, ...).
    pub(crate) is_net: bool,
    /// Pre-allocated placeholder [`ValueId`] read by combinational/sequential consumers.
    pub(crate) placeholder: ValueId,
    /// Optional constant initial value (`logic q = 1'b1` or `initial q = 1'b1`).
    pub(crate) init: Option<CVal>,
    /// Declaration source location.
    pub(crate) loc: Loc,
    /// Registered bit-interval drivers across all blocks/ports.
    pub(crate) drivers: Vec<BitSliceDriver>,
    /// `(* anyseq *)` / `(* anyconst *)` on the declaration: how its undriven bits are free.
    pub(crate) free: FreeKind,
    /// Optional weak pull resistor (`Some(true)` for `pullup`/`tri1`, `Some(false)` for `pulldown`/`tri0`).
    pub(crate) pull: Option<bool>,
    /// Present when this slot is a top-level `inout` port leaf.
    pub(crate) top_inout: Option<TopInoutSplit>,
}

impl SignalSlot {
    /// Appends an unconditional bit-interval driver `[lo ..= hi]`.
    pub(crate) fn add_driver(&mut self, lo: u32, hi: u32, value: ValueId, kind: DriverKind, loc: Loc) {
        debug_assert!(lo <= hi && hi < self.width);
        self.drivers.push(BitSliceDriver { lo, hi, value, en: TriEnable::Always, kind, loc });
    }

    /// Appends a tri-state bit-interval driver `[lo ..= hi]` with enable `en`.
    pub(crate) fn add_tristate_driver(
        &mut self,
        lo: u32,
        hi: u32,
        value: ValueId,
        en: TriEnable,
        kind: DriverKind,
        loc: Loc,
    ) {
        debug_assert!(lo <= hi && hi < self.width);
        self.drivers.push(BitSliceDriver { lo, hi, value, en, kind, loc });
    }
}

/// Resolves bit-interval drivers and `NetEquiv` classes for every [`SignalSlot`], checking for
/// illegal overlaps, synthesizing tri-state bus muxes & top-level `(io$oe, io$out)` splits,
/// creating [`UndrivenSlice`]s for undriven bit gaps, and binding `slot.placeholder`.
pub(crate) fn resolve_all_signals(
    slots: &[SignalSlot],
    net_equivs: &[NetEquiv],
    builder: &mut NetBuilder,
    prop_values: &[Option<ValueId>],
) {
    let read = read_values(builder, prop_values);
    let mut in_equiv = vec![false; slots.len()];
    for eq in net_equivs {
        if eq.width == 0 || eq.sidx_a >= slots.len() || eq.sidx_b >= slots.len() {
            continue;
        }
        if eq.sidx_a == eq.sidx_b && eq.lo_a == eq.lo_b {
            continue;
        }
        if eq.lo_a + eq.width <= slots[eq.sidx_a].width && eq.lo_b + eq.width <= slots[eq.sidx_b].width {
            in_equiv[eq.sidx_a] = true;
            in_equiv[eq.sidx_b] = true;
        }
    }

    for (sidx, slot) in slots.iter().enumerate() {
        if !in_equiv[sidx] {
            resolve_one_signal(slot, builder, &read);
        }
    }

    if in_equiv.iter().any(|&b| b) {
        resolve_equiv_signals(slots, &in_equiv, net_equivs, builder, &read);
    }
}

/// Every value something reads: operator operands, aliases, memory ports, register
/// next states, resets and clocks, sampled-value inputs, checks, outputs and SVA leaves
/// (trace bindings do not count).
fn read_values(builder: &NetBuilder, prop_values: &[Option<ValueId>]) -> FxSet<u32> {
    let mut r: FxSet<u32> = FxSet::default();
    for n in &builder.nodes {
        match &n.kind {
            BuildKind::Alias(v) => {
                r.insert(v.0);
            }
            BuildKind::Op { args, .. } => r.extend(args.iter().map(|a| a.0)),
            BuildKind::MemRead { addr, .. } => {
                r.insert(addr.0);
            }
            _ => {}
        }
    }
    for s in &builder.states {
        r.insert(s.next.0);
        if let Some(c) = s.clock {
            r.insert(c.signal.0);
        }
        if let Some(a) = s.async_reset {
            r.extend([a.signal.0, a.value.0]);
        }
        if let Some(e) = s.latch_enable {
            r.insert(e.0);
        }
    }
    for m in &builder.memories {
        for w in &m.writes {
            r.extend([w.addr.0, w.data.0, w.enable.0]);
            if let Some(c) = w.clock {
                r.insert(c.signal.0);
            }
        }
    }
    for s in &builder.sampled {
        r.insert(s.expr.0);
        if let Some(g) = s.gate {
            r.insert(g.0);
        }
        if let Some(c) = s.clock {
            r.insert(c.signal.0);
        }
    }
    for c in &builder.checks {
        r.extend([c.enable.0, c.expr.0]);
        if let Some(k) = c.clock {
            r.insert(k.signal.0);
        }
    }
    r.extend(builder.outputs.iter().map(|o| o.value.0));
    r.extend(prop_values.iter().flatten().map(|v| v.0));
    r
}

fn resolve_one_signal(slot: &SignalSlot, builder: &mut NetBuilder, read: &FxSet<u32>) {
    // A cut point: whatever drives the signal, its readers get a free value.
    if slot.free == FreeKind::Cut {
        let uval = alloc_cut_slice(builder, &slot.name, slot.width, slot.signed, slot.four_state, slot.loc);
        builder.bind_placeholder(slot.placeholder, uval);
        if let Some(ti) = &slot.top_inout {
            let z = builder.alloc_const(CVal::new(slot.width, false, 0), false, slot.loc);
            emit_top_inout_outputs(builder, slot, &ti.port_rel_name, z, uval);
        }
        return;
    }

    // Net legality: procedural blocks cannot drive a `wire` net.
    if slot.is_net {
        for d in &slot.drivers {
            if d.kind.is_procedural() {
                builder.err(d.loc, format!("procedural {} cannot drive net '{}'", d.kind.as_str(), slot.name));
                return;
            }
        }
    }

    if slot.free != FreeKind::Undriven && !slot.drivers.is_empty() {
        builder.err(
            slot.drivers[0].loc,
            format!("'{}' is declared (* {} *) but is also driven", slot.name, slot.free.as_str()),
        );
        return;
    }

    // Top-level `inout` port: split into `io$in` + `(io$oe, io$out)` -> `io = io$oe ? io$out : io$in`.
    if let Some(ti) = &slot.top_inout {
        if let Some((oe_mask, out_masked, resolved)) = resolve_top_inout_segment(slot, ti.in_val, builder) {
            emit_top_inout_outputs(builder, slot, &ti.port_rel_name, oe_mask, out_masked);
            builder.bind_placeholder(slot.placeholder, resolved);
        }
        return;
    }

    // Any tri-state driver (`en != Always`) takes the interval-partitioned tri-state bus resolver.
    if slot.drivers.iter().any(|d| d.en != TriEnable::Always) {
        if !slot.is_net && slot.drivers.len() > 1 {
            // Multiple overlapping drivers on a variable remain illegal even if tri-stated;
            // disjoint slice tri-state drivers on a 4-state output variable (`bufif1`) are allowed.
            for i in 0..slot.drivers.len() {
                for j in (i + 1)..slot.drivers.len() {
                    let (a, b) = (&slot.drivers[i], &slot.drivers[j]);
                    if a.lo <= b.hi && b.lo <= a.hi {
                        let first_file = builder.file_of(a.loc);
                        let bits = fmt_overlap_bits(slot.width, a.lo.max(b.lo), a.hi.min(b.hi));
                        builder.err(
                            b.loc,
                            format!(
                                "overlapping drivers on '{}{bits}': driven by {} and {} (first driven at {}:{}:{})",
                                slot.name,
                                b.kind.as_str(),
                                a.kind.as_str(),
                                first_file,
                                a.loc.line,
                                a.loc.column
                            ),
                        );
                        return;
                    }
                }
            }
        }
        if let Some(resolved) = resolve_internal_tristate_slot(slot, slot.width, builder) {
            builder.bind_placeholder(slot.placeholder, resolved);
        }
        return;
    }

    // Standard non-tristate fast path (preserves 100% identical node order on all existing designs).
    let unobserved = !read.contains(&slot.placeholder.0) && slot.drivers.iter().all(|d| d.kind.is_procedural());
    if unobserved && slot.drivers.len() > 1 {
        let d = &slot.drivers[0];
        let v = builder.resize_val(d.value, d.hi - d.lo + 1, false, false, d.loc);
        let v = builder.resize_val(v, slot.width, false, slot.signed, d.loc);
        builder.bind_placeholder(slot.placeholder, v);
        return;
    }

    let mut had_overlap = false;
    for i in 0..slot.drivers.len() {
        for j in (i + 1)..slot.drivers.len() {
            let a = &slot.drivers[i];
            let b = &slot.drivers[j];
            if a.lo <= b.hi && b.lo <= a.hi {
                had_overlap = true;
                let ov_lo = a.lo.max(b.lo);
                let ov_hi = a.hi.min(b.hi);
                let bits = fmt_overlap_bits(slot.width, ov_lo, ov_hi);
                let first_file = builder.file_of(a.loc);
                if slot.is_net && !a.kind.is_procedural() && !b.kind.is_procedural() {
                    builder.unsup(
                        b.loc,
                        format!(
                            "net '{}{bits}' has more than one driver (first driven at {}:{}:{}); resolving drivers is not supported",
                            slot.name, first_file, a.loc.line, a.loc.column
                        ),
                    );
                    continue;
                }
                builder.err(
                    b.loc,
                    format!(
                        "overlapping drivers on '{}{bits}': driven by {} and {} (first driven at {}:{}:{})",
                        slot.name,
                        b.kind.as_str(),
                        a.kind.as_str(),
                        first_file,
                        a.loc.line,
                        a.loc.column
                    ),
                );
            }
        }
    }
    if had_overlap {
        return;
    }

    if slot.drivers.is_empty() {
        if let Some(init_cv) = &slot.init {
            let c = builder.alloc_const(init_cv.cast(slot.width, slot.signed), slot.four_state, slot.loc);
            builder.bind_placeholder(slot.placeholder, c);
        } else if let Some(pull_high) = slot.pull {
            let fill = if pull_high { Int::from(-1) } else { Int::from(0) };
            let c = builder.alloc_const(CVal::new(slot.width, slot.signed, fill), slot.four_state, slot.loc);
            builder.bind_placeholder(slot.placeholder, c);
        } else {
            let uid = builder.undriven.len();
            let uval = builder.alloc_raw(
                slot.width,
                slot.signed,
                slot.four_state,
                BuildKind::Undriven(uid),
                Some(slot.name.clone()),
                slot.loc,
            );
            builder.undriven.push(UndrivenSlice {
                signal: slot.name.clone(),
                hi: slot.width - 1,
                lo: 0,
                value: uval,
                loc: slot.loc,
                free: slot.free,
            });
            builder.bind_placeholder(slot.placeholder, uval);
        }
        return;
    }

    if slot.drivers.len() == 1 && slot.drivers[0].lo == 0 && slot.drivers[0].hi + 1 == slot.width {
        let dval = builder.resize_val(slot.drivers[0].value, slot.width, false, slot.signed, slot.drivers[0].loc);
        builder.bind_placeholder(slot.placeholder, dval);
        return;
    }

    let mut sorted = slot.drivers.clone();
    sorted.sort_by_key(|d| d.lo);

    let mut lsb_to_msb_parts: Vec<ValueId> = Vec::new();
    let mut cursor = 0u32;
    for d in &sorted {
        if cursor < d.lo {
            lsb_to_msb_parts.push(make_gap_slice(slot, cursor, d.lo - 1, builder));
        }
        let dw = (d.hi - d.lo) + 1;
        let part = builder.resize_val(d.value, dw, false, false, d.loc);
        lsb_to_msb_parts.push(part);
        cursor = d.hi + 1;
    }
    if cursor < slot.width {
        lsb_to_msb_parts.push(make_gap_slice(slot, cursor, slot.width - 1, builder));
    }

    lsb_to_msb_parts.reverse();
    let full = builder.alloc_op(NetOp::Concat, lsb_to_msb_parts, slot.width, slot.signed, slot.four_state, slot.loc);
    builder.bind_placeholder(slot.placeholder, full);
}

fn fmt_overlap_bits(width: u32, ov_lo: u32, ov_hi: u32) -> String {
    if width == 1 {
        String::new()
    } else if ov_lo == ov_hi {
        format!("[{ov_lo}]")
    } else {
        format!("[{ov_hi}:{ov_lo}]")
    }
}

fn alloc_cut_slice(
    builder: &mut NetBuilder,
    name: &str,
    width: u32,
    signed: bool,
    four_state: bool,
    loc: Loc,
) -> ValueId {
    let uid = builder.undriven.len();
    let uval = builder.alloc_raw(width, signed, four_state, BuildKind::Undriven(uid), Some(name.to_string()), loc);
    builder.undriven.push(UndrivenSlice {
        signal: name.to_string(),
        hi: width - 1,
        lo: 0,
        value: uval,
        loc,
        free: FreeKind::Cut,
    });
    uval
}

fn emit_top_inout_outputs(
    builder: &mut NetBuilder,
    slot: &SignalSlot,
    port_rel_name: &str,
    oe_mask: ValueId,
    out_masked: ValueId,
) {
    let w = slot.width;
    let oe_val = builder.resize_val(oe_mask, w, false, false, slot.loc);
    let out_val = builder.resize_val(out_masked, w, false, slot.signed, slot.loc);
    let (_, _, oe_f4) = builder.node_shape(oe_val);
    let (_, _, out_f4) = builder.node_shape(out_val);

    let oe_port = format!("{port_rel_name}$oe");
    let out_port = format!("{port_rel_name}$out");
    builder.outputs.push(PortOutput {
        name: oe_port,
        width: w,
        signed: false,
        four_state: oe_f4,
        value: oe_val,
        loc: slot.loc,
    });
    builder.outputs.push(PortOutput {
        name: out_port,
        width: w,
        signed: slot.signed,
        four_state: out_f4,
        value: out_val,
        loc: slot.loc,
    });

    builder.traces.push(TraceSignal {
        name: format!("{}$oe", slot.name),
        width: w,
        signed: false,
        four_state: oe_f4,
        value: oe_val,
        loc: slot.loc,
    });
    builder.traces.push(TraceSignal {
        name: format!("{}$out", slot.name),
        width: w,
        signed: slot.signed,
        four_state: out_f4,
        value: out_val,
        loc: slot.loc,
    });
}

fn partition_boundaries(width: u32, drivers: &[BitSliceDriver]) -> Vec<(u32, u32)> {
    let mut pts = Vec::with_capacity(drivers.len() * 2 + 2);
    pts.push(0);
    pts.push(width);
    for d in drivers {
        pts.push(d.lo);
        pts.push((d.hi + 1).min(width));
    }
    pts.sort_unstable();
    pts.dedup();
    let mut out = Vec::with_capacity(pts.len().saturating_sub(1));
    for win in pts.windows(2) {
        if win[0] < win[1] {
            out.push((win[0], win[1] - 1));
        }
    }
    out
}

fn slice_val_u(builder: &mut NetBuilder, val: ValueId, lo: u32, w: u32, loc: Loc) -> ValueId {
    let (vw, _, vf4) = builder.node_shape(val);
    if lo == 0 && w == vw {
        builder.resize_val(val, w, false, false, loc)
    } else {
        builder.alloc_op(NetOp::Slice { hi: lo + w - 1, lo }, vec![val], w, false, vf4, loc)
    }
}

fn slice_tri_enable(builder: &mut NetBuilder, en: TriEnable, lo: u32, w: u32, loc: Loc) -> TriEnable {
    match en {
        TriEnable::Always => TriEnable::Always,
        TriEnable::Never => TriEnable::Never,
        TriEnable::Bit(b) => TriEnable::Bit(b),
        TriEnable::Mask(m) => {
            let s = slice_val_u(builder, m, lo, w, loc);
            if w == 1 {
                TriEnable::Bit(s)
            } else {
                TriEnable::Mask(s)
            }
        }
    }
}

fn replicate_bit(builder: &mut NetBuilder, bit: ValueId, w: u32, loc: Loc) -> ValueId {
    let b1 = builder.alloc_bool(bit, loc);
    if w == 1 {
        return b1;
    }
    let (_, _, f4) = builder.node_shape(b1);
    builder.alloc_op(NetOp::Concat, vec![b1; w as usize], w, false, f4, loc)
}

struct ActiveBusCalc {
    oe_mask: ValueId,
    oe_1b: Option<ValueId>,
    driven_raw: ValueId,
    out_masked: ValueId,
    conflict: Option<ValueId>,
}

/// Computes active enable, driven value, masked output `driven_raw & oe_mask`, and pairwise
/// contention mask (`ones & zeros`) across `active` drivers covering `[sub_lo ..= sub_hi]`.
fn compute_active_bus(
    sig_name: &str,
    full_width: u32,
    sub_lo: u32,
    sub_hi: u32,
    active: &[(ValueId, TriEnable, DriverKind, Loc)],
    builder: &mut NetBuilder,
    loc: Loc,
) -> Option<ActiveBusCalc> {
    let sub_w = sub_hi - sub_lo + 1;
    if active.len() >= 2 && active.iter().all(|(_, en, _, _)| *en == TriEnable::Always) {
        let first = &active[0];
        let second = &active[1];
        let first_file = builder.file_of(first.3);
        let bits = fmt_overlap_bits(full_width, sub_lo, sub_hi);
        builder.unsup(
            second.3,
            format!(
                "net '{sig_name}{bits}' has more than one driver (first driven at {}:{}:{}); resolving drivers is not supported",
                first_file, first.3.line, first.3.column
            ),
        );
        return None;
    }

    if active.is_empty() {
        let z_w = builder.alloc_const(CVal::new(sub_w, false, 0), false, loc);
        let z_1 = builder.alloc_const(CVal::new(1, false, 0), false, loc);
        return Some(ActiveBusCalc {
            oe_mask: z_w,
            oe_1b: Some(z_1),
            driven_raw: z_w,
            out_masked: z_w,
            conflict: None,
        });
    }

    if active.len() == 1 {
        let (dval, en, _, dloc) = active[0];
        let du = builder.resize_val(dval, sub_w, false, false, dloc);
        let (_, _, df4) = builder.node_shape(du);
        return Some(match en {
            TriEnable::Always => {
                let ones_w = builder.alloc_const(CVal::new(sub_w, false, -1), false, dloc);
                let one_1 = builder.alloc_const(CVal::new(1, false, 1), false, dloc);
                ActiveBusCalc { oe_mask: ones_w, oe_1b: Some(one_1), driven_raw: du, out_masked: du, conflict: None }
            }
            TriEnable::Never => unreachable!("filtered out"),
            TriEnable::Bit(eb) => {
                let eb1 = builder.alloc_bool(eb, dloc);
                let (_, _, ef4) = builder.node_shape(eb1);
                let oe_mask = replicate_bit(builder, eb1, sub_w, dloc);
                let zero_w = builder.alloc_const(CVal::new(sub_w, false, 0), false, dloc);
                let out_masked = builder.alloc_op(NetOp::Mux, vec![eb1, du, zero_w], sub_w, false, df4 || ef4, dloc);
                ActiveBusCalc { oe_mask, oe_1b: Some(eb1), driven_raw: du, out_masked, conflict: None }
            }
            TriEnable::Mask(em) => {
                let em_u = builder.resize_val(em, sub_w, false, false, dloc);
                let (_, _, ef4) = builder.node_shape(em_u);
                let out_masked = builder.alloc_op(NetOp::And, vec![em_u, du], sub_w, false, df4 || ef4, dloc);
                ActiveBusCalc {
                    oe_mask: em_u,
                    oe_1b: (sub_w == 1).then_some(em_u),
                    driven_raw: du,
                    out_masked,
                    conflict: None,
                }
            }
        });
    }

    // Multi-driver tri-state resolution (`k >= 2`):
    // `ones = OR_i (mask_i & d_i)`, `zeros = OR_i (mask_i & ~d_i)`, `conflict = ones & zeros`.
    let all_scalar_en = active.iter().all(|(_, en, _, _)| matches!(en, TriEnable::Always | TriEnable::Bit(_)));
    let mut ones_acc: Option<ValueId> = None;
    let mut zeros_acc: Option<ValueId> = None;
    let mut mask_acc: Option<ValueId> = None;
    let mut bit_en_acc: Option<ValueId> = None;

    for &(dval, en, _, dloc) in active {
        let du = builder.resize_val(dval, sub_w, false, false, dloc);
        let (_, _, df4) = builder.node_shape(du);
        let not_du = builder.alloc_op(NetOp::Not, vec![du], sub_w, false, df4, dloc);
        let (mi, bi_opt) = match en {
            TriEnable::Always => {
                let ones_w = builder.alloc_const(CVal::new(sub_w, false, -1), false, dloc);
                let one_1 = builder.alloc_const(CVal::new(1, false, 1), false, dloc);
                (ones_w, Some(one_1))
            }
            TriEnable::Bit(eb) => {
                let eb1 = builder.alloc_bool(eb, dloc);
                (replicate_bit(builder, eb1, sub_w, dloc), Some(eb1))
            }
            TriEnable::Mask(em) => {
                let em_u = builder.resize_val(em, sub_w, false, false, dloc);
                (em_u, (sub_w == 1).then_some(em_u))
            }
            TriEnable::Never => continue,
        };
        let (_, _, mf4) = builder.node_shape(mi);
        let f4 = df4 || mf4;
        let term_1 = builder.alloc_op(NetOp::And, vec![mi, du], sub_w, false, f4, dloc);
        let term_0 = builder.alloc_op(NetOp::And, vec![mi, not_du], sub_w, false, f4, dloc);

        ones_acc = Some(match ones_acc {
            None => term_1,
            Some(prev) => {
                let pf = builder.node_shape(prev).2;
                builder.alloc_op(NetOp::Or, vec![prev, term_1], sub_w, false, pf || f4, dloc)
            }
        });
        zeros_acc = Some(match zeros_acc {
            None => term_0,
            Some(prev) => {
                let pf = builder.node_shape(prev).2;
                builder.alloc_op(NetOp::Or, vec![prev, term_0], sub_w, false, pf || f4, dloc)
            }
        });
        mask_acc = Some(match mask_acc {
            None => mi,
            Some(prev) => {
                let pf = builder.node_shape(prev).2;
                builder.alloc_op(NetOp::Or, vec![prev, mi], sub_w, false, pf || mf4, dloc)
            }
        });
        if all_scalar_en {
            if let Some(bi) = bi_opt {
                bit_en_acc = Some(match bit_en_acc {
                    None => bi,
                    Some(prev) => builder.alloc_or1(prev, bi, dloc),
                });
            }
        }
    }

    let ones = ones_acc.unwrap();
    let zeros = zeros_acc.unwrap();
    let any_en = mask_acc.unwrap();
    let f_oz = builder.node_shape(ones).2 || builder.node_shape(zeros).2;
    let conflict = builder.alloc_op(NetOp::And, vec![ones, zeros], sub_w, false, f_oz, loc);
    let not_conf = builder.alloc_op(NetOp::Not, vec![conflict], sub_w, false, f_oz, loc);
    let clean_ones = builder.alloc_op(NetOp::And, vec![ones, not_conf], sub_w, false, f_oz, loc);
    let x_contend = builder.alloc_const(CVal::all_x(sub_w, false), true, loc);
    let x_masked = builder.alloc_op(NetOp::And, vec![x_contend, conflict], sub_w, false, true, loc);
    let driven_raw = builder.alloc_op(NetOp::Or, vec![clean_ones, x_masked], sub_w, false, true, loc);
    let out_masked = builder.alloc_op(NetOp::And, vec![driven_raw, any_en], sub_w, false, true, loc);

    let (oe_mask, oe_1b) = if let Some(b1) = bit_en_acc {
        (replicate_bit(builder, b1, sub_w, loc), Some(b1))
    } else {
        (any_en, (sub_w == 1).then_some(any_en))
    };

    Some(ActiveBusCalc { oe_mask, oe_1b, driven_raw, out_masked, conflict: Some(conflict) })
}

fn mux_with_fallback(
    builder: &mut NetBuilder,
    calc: &ActiveBusCalc,
    fallback: ValueId,
    sub_w: u32,
    preserve_z_per_bit: bool,
    loc: Loc,
) -> ValueId {
    let fb_u = builder.resize_val(fallback, sub_w, false, false, loc);
    let f4 = builder.node_shape(calc.driven_raw).2 || builder.node_shape(fb_u).2 || builder.node_shape(calc.oe_mask).2;
    if let Some(oe1) = calc.oe_1b {
        return builder.alloc_op(NetOp::Mux, vec![oe1, calc.driven_raw, fb_u], sub_w, false, f4, loc);
    }
    if preserve_z_per_bit {
        let mut bits_msb = Vec::with_capacity(sub_w as usize);
        for k in (0..sub_w).rev() {
            let eb = slice_val_u(builder, calc.oe_mask, k, 1, loc);
            let db = slice_val_u(builder, calc.driven_raw, k, 1, loc);
            let zb = slice_val_u(builder, fb_u, k, 1, loc);
            bits_msb.push(builder.alloc_op(NetOp::Mux, vec![eb, db, zb], 1, false, true, loc));
        }
        return builder.alloc_op(NetOp::Concat, bits_msb, sub_w, false, true, loc);
    }
    let (_, _, mf4) = builder.node_shape(calc.oe_mask);
    let not_oe = builder.alloc_op(NetOp::Not, vec![calc.oe_mask], sub_w, false, mf4, loc);
    let fb_masked = builder.alloc_op(NetOp::And, vec![not_oe, fb_u], sub_w, false, f4, loc);
    builder.alloc_op(NetOp::Or, vec![calc.out_masked, fb_masked], sub_w, false, f4, loc)
}

fn resolve_top_inout_segment(
    slot: &SignalSlot,
    in_val: ValueId,
    builder: &mut NetBuilder,
) -> Option<(ValueId, ValueId, ValueId)> {
    let intervals = partition_boundaries(slot.width, &slot.drivers);
    let mut oe_parts_lsb = Vec::with_capacity(intervals.len());
    let mut out_parts_lsb = Vec::with_capacity(intervals.len());
    let mut res_parts_lsb = Vec::with_capacity(intervals.len());

    for (sub_lo, sub_hi) in intervals {
        let sub_w = sub_hi - sub_lo + 1;
        let in_sub = slice_val_u(builder, in_val, sub_lo, sub_w, slot.loc);
        let mut active = Vec::new();
        for d in &slot.drivers {
            if d.lo <= sub_hi && sub_lo <= d.hi && d.en != TriEnable::Never {
                let off = sub_lo - d.lo;
                let dv = slice_val_u(builder, d.value, off, sub_w, d.loc);
                let den = slice_tri_enable(builder, d.en, off, sub_w, d.loc);
                active.push((dv, den, d.kind, d.loc));
            }
        }
        let calc = compute_active_bus(&slot.name, slot.width, sub_lo, sub_hi, &active, builder, slot.loc)?;
        if !active.is_empty() {
            builder.tristate_buses.push(TristateBus {
                signal: slot.name.clone(),
                full_width: slot.width,
                hi: sub_hi,
                lo: sub_lo,
                any_enable: calc.oe_1b.unwrap_or(calc.oe_mask),
                conflict: calc.conflict,
                has_pull: slot.pull.is_some(),
                is_top_inout: true,
                loc: slot.loc,
            });
        }
        let res_sub = mux_with_fallback(builder, &calc, in_sub, sub_w, false, slot.loc);
        oe_parts_lsb.push(calc.oe_mask);
        out_parts_lsb.push(calc.out_masked);
        res_parts_lsb.push(res_sub);
    }

    let join = |mut lsb: Vec<ValueId>, signed: bool, four: bool, b: &mut NetBuilder| -> ValueId {
        if lsb.len() == 1 {
            b.resize_val(lsb[0], slot.width, false, signed, slot.loc)
        } else {
            lsb.reverse();
            let any_f4 = four || lsb.iter().any(|&x| b.node_shape(x).2);
            b.alloc_op(NetOp::Concat, lsb, slot.width, signed, any_f4, slot.loc)
        }
    };
    let oe_full = join(oe_parts_lsb, false, false, builder);
    let out_full = join(out_parts_lsb, slot.signed, slot.four_state, builder);
    let res_full = join(res_parts_lsb, slot.signed, slot.four_state, builder);
    Some((oe_full, out_full, res_full))
}

fn resolve_internal_tristate_slot(slot: &SignalSlot, full_sig_w: u32, builder: &mut NetBuilder) -> Option<ValueId> {
    let intervals = partition_boundaries(slot.width, &slot.drivers);
    let mut res_parts_lsb = Vec::with_capacity(intervals.len());

    for (sub_lo, sub_hi) in intervals {
        let sub_w = sub_hi - sub_lo + 1;
        let mut touched_by_z = false;
        let mut active = Vec::new();
        for d in &slot.drivers {
            if d.lo <= sub_hi && sub_lo <= d.hi {
                if d.en == TriEnable::Never {
                    touched_by_z = true;
                } else {
                    let off = sub_lo - d.lo;
                    let dv = slice_val_u(builder, d.value, off, sub_w, d.loc);
                    let den = slice_tri_enable(builder, d.en, off, sub_w, d.loc);
                    active.push((dv, den, d.kind, d.loc));
                }
            }
        }
        if active.is_empty() && !touched_by_z {
            res_parts_lsb.push(make_gap_slice(slot, sub_lo, sub_hi, builder));
            continue;
        }
        let calc = compute_active_bus(&slot.name, full_sig_w, sub_lo, sub_hi, &active, builder, slot.loc)?;
        let (fallback, is_z_fallback) = if let Some(init_cv) = &slot.init {
            (builder.alloc_const(init_cv.slice(sub_lo, sub_w, false), slot.four_state, slot.loc), false)
        } else if let Some(pull_high) = slot.pull {
            let fill = if pull_high { Int::from(-1) } else { Int::from(0) };
            (builder.alloc_const(CVal::new(sub_w, false, fill), slot.four_state, slot.loc), false)
        } else {
            (builder.alloc_const(CVal::all_z(sub_w, false), true, slot.loc), true)
        };
        if active.is_empty() {
            res_parts_lsb.push(fallback);
            continue;
        }
        if active.iter().any(|(_, en, _, _)| *en != TriEnable::Always) {
            builder.tristate_buses.push(TristateBus {
                signal: slot.name.clone(),
                full_width: full_sig_w,
                hi: sub_hi,
                lo: sub_lo,
                any_enable: calc.oe_1b.unwrap_or(calc.oe_mask),
                conflict: calc.conflict,
                has_pull: slot.pull.is_some(),
                is_top_inout: false,
                loc: slot.loc,
            });
        }
        let res_sub = mux_with_fallback(builder, &calc, fallback, sub_w, is_z_fallback, slot.loc);
        res_parts_lsb.push(res_sub);
    }

    let mut full = if res_parts_lsb.len() == 1 {
        builder.resize_val(res_parts_lsb[0], slot.width, false, slot.signed, slot.loc)
    } else {
        res_parts_lsb.reverse();
        let any_f4 = slot.four_state || res_parts_lsb.iter().any(|&x| builder.node_shape(x).2);
        builder.alloc_op(NetOp::Concat, res_parts_lsb, slot.width, slot.signed, any_f4, slot.loc)
    };
    if !slot.four_state && builder.node_shape(full).2 {
        full = builder.alloc_op(
            NetOp::CastFourState { four_state: false },
            vec![full],
            slot.width,
            slot.signed,
            false,
            slot.loc,
        );
    }
    Some(full)
}

fn resolve_equiv_signals(
    slots: &[SignalSlot],
    in_equiv: &[bool],
    net_equivs: &[NetEquiv],
    builder: &mut NetBuilder,
    read: &FxSet<u32>,
) {
    // 1. Validate procedural / free attribute legality on all participating slots up front.
    let mut slot_base: Vec<Option<u32>> = vec![None; slots.len()];
    let mut total_bits = 0u32;
    for (sidx, slot) in slots.iter().enumerate() {
        if !in_equiv[sidx] {
            continue;
        }
        if slot.is_net {
            for d in &slot.drivers {
                if d.kind.is_procedural() {
                    builder.err(d.loc, format!("procedural {} cannot drive net '{}'", d.kind.as_str(), slot.name));
                    return;
                }
            }
        }
        if slot.free != FreeKind::Undriven && slot.free != FreeKind::Cut && !slot.drivers.is_empty() {
            builder.err(
                slot.drivers[0].loc,
                format!("'{}' is declared (* {} *) but is also driven", slot.name, slot.free.as_str()),
            );
            return;
        }
        slot_base[sidx] = Some(total_bits);
        total_bits += slot.width;
    }

    // 2. Union-Find over participating bit positions.
    let mut parent: Vec<u32> = (0..total_bits).collect();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            let gp = parent[parent[x as usize] as usize];
            parent[x as usize] = gp;
            x = gp;
        }
        x
    }
    for eq in net_equivs {
        let (Some(ba), Some(bb)) =
            (slot_base.get(eq.sidx_a).copied().flatten(), slot_base.get(eq.sidx_b).copied().flatten())
        else {
            continue;
        };
        if eq.lo_a + eq.width > slots[eq.sidx_a].width || eq.lo_b + eq.width > slots[eq.sidx_b].width {
            continue;
        }
        if eq.sidx_a == eq.sidx_b && eq.lo_a < eq.lo_b + eq.width && eq.lo_b < eq.lo_a + eq.width {
            if eq.lo_a != eq.lo_b {
                builder.unsup(eq.loc, "self-overlapping 'alias' on the same net is not supported");
                return;
            }
            continue;
        }
        for k in 0..eq.width {
            let ra = find(&mut parent, ba + eq.lo_a + k);
            let rb = find(&mut parent, bb + eq.lo_b + k);
            if ra != rb {
                let (lo_r, hi_r) = if ra < rb { (ra, rb) } else { (rb, ra) };
                parent[hi_r as usize] = lo_r;
            }
        }
    }

    let mut members_by_root: FxMap<u32, Vec<(usize, u32)>> = FxMap::default();
    for (sidx, base_opt) in slot_base.iter().enumerate() {
        let Some(base) = *base_opt else { continue };
        for bit in 0..slots[sidx].width {
            let r = find(&mut parent, base + bit);
            members_by_root.entry(r).or_default().push((sidx, bit));
        }
    }
    let mut ordered_roots: Vec<(u32, Vec<(usize, u32)>)> = members_by_root.into_iter().collect();
    for (_, mems) in &mut ordered_roots {
        mems.sort_unstable();
    }
    ordered_roots.sort_by_key(|(_, mems)| mems[0]);

    // 3. Coalesce contiguous bit runs sharing identical slot alignment (`(sidx, prev_bit + 1)`).
    struct CoalescedGroup {
        width: u32,
        members: Vec<(usize, u32)>,
    }
    let mut groups: Vec<CoalescedGroup> = Vec::new();
    for (_, mems) in ordered_roots {
        let can_merge = groups.last().is_some_and(|prev| {
            prev.members.len() == mems.len()
                && prev.members.iter().zip(&mems).all(|(&(ps, pb), &(cs, cb))| ps == cs && cb == pb + prev.width)
        });
        if can_merge {
            groups.last_mut().unwrap().width += 1;
        } else {
            groups.push(CoalescedGroup { width: 1, members: mems });
        }
    }

    // 4. Resolve each coalesced equivalence group once and fan out to all member slot slices.
    let mut slot_parts: FxMap<usize, Vec<(u32, u32, ValueId)>> = FxMap::default();
    let mut top_io_parts: FxMap<usize, Vec<(u32, u32, ValueId, ValueId)>> = FxMap::default();

    for grp in &groups {
        let gw = grp.width;
        let rep_idx = grp
            .members
            .iter()
            .position(|&(s, _)| slots[s].top_inout.is_some())
            .or_else(|| grp.members.iter().position(|&(s, _)| slots[s].free == FreeKind::Cut))
            .unwrap_or(0);
        let (rep_sidx, rep_lo) = grp.members[rep_idx];
        let rep_slot = &slots[rep_sidx];
        let seg_name = if rep_lo == 0 && gw == rep_slot.width {
            rep_slot.name.clone()
        } else if gw == 1 {
            format!("{}[{rep_lo}]", rep_slot.name)
        } else {
            format!("{}[{}:{rep_lo}]", rep_slot.name, rep_lo + gw - 1)
        };

        let any_cut = grp.members.iter().any(|&(s, _)| slots[s].free == FreeKind::Cut);
        if any_cut {
            let cut_v = alloc_cut_slice(builder, &seg_name, gw, false, rep_slot.four_state, rep_slot.loc);
            let z_w = builder.alloc_const(CVal::new(gw, false, 0), false, rep_slot.loc);
            for &(ms, mlo) in &grp.members {
                slot_parts.entry(ms).or_default().push((mlo, gw, cut_v));
                if slots[ms].top_inout.is_some() {
                    top_io_parts.entry(ms).or_default().push((mlo, gw, z_w, cut_v));
                }
            }
            continue;
        }

        let merged_pull = grp.members.iter().find_map(|&(s, _)| slots[s].pull);
        let merged_free = grp
            .members
            .iter()
            .map(|&(s, _)| slots[s].free)
            .find(|&f| f != FreeKind::Undriven)
            .unwrap_or(FreeKind::Undriven);
        let merged_four = grp.members.iter().any(|&(s, _)| slots[s].four_state);
        let merged_is_net = grp.members.iter().any(|&(s, _)| slots[s].is_net);

        let mut syn_slot = SignalSlot {
            name: seg_name,
            width: gw,
            signed: false,
            four_state: merged_four,
            is_net: merged_is_net,
            placeholder: rep_slot.placeholder,
            init: None,
            loc: rep_slot.loc,
            drivers: Vec::new(),
            free: merged_free,
            pull: merged_pull,
            top_inout: None,
        };

        for &(ms, mlo) in &grp.members {
            let mslot = &slots[ms];
            let mhi = mlo + gw - 1;
            if syn_slot.init.is_none() {
                if let Some(icv) = &mslot.init {
                    syn_slot.init = Some(icv.slice(mlo, gw, false));
                }
            }
            for d in &mslot.drivers {
                if d.lo <= mhi && mlo <= d.hi {
                    let ov_lo = d.lo.max(mlo);
                    let ov_hi = d.hi.min(mhi);
                    let sub_w = ov_hi - ov_lo + 1;
                    let off = ov_lo - d.lo;
                    let dv = slice_val_u(builder, d.value, off, sub_w, d.loc);
                    let den = slice_tri_enable(builder, d.en, off, sub_w, d.loc);
                    syn_slot.add_tristate_driver(ov_lo - mlo, ov_hi - mlo, dv, den, d.kind, d.loc);
                }
            }
        }

        let top_member = grp.members.iter().copied().find(|&(s, _)| slots[s].top_inout.is_some());
        if let Some((ts, tlo)) = top_member {
            let ti = slots[ts].top_inout.as_ref().unwrap();
            let in_slice = slice_val_u(builder, ti.in_val, tlo, gw, slots[ts].loc);
            let Some((oe_m, out_m, res_v)) = resolve_top_inout_segment(&syn_slot, in_slice, builder) else {
                return;
            };
            for &(ms, mlo) in &grp.members {
                slot_parts.entry(ms).or_default().push((mlo, gw, res_v));
                if slots[ms].top_inout.is_some() {
                    top_io_parts.entry(ms).or_default().push((mlo, gw, oe_m, out_m));
                }
            }
        } else {
            let res_v = if syn_slot.drivers.iter().any(|d| d.en != TriEnable::Always) {
                let Some(v) = resolve_internal_tristate_slot(&syn_slot, gw, builder) else { return };
                v
            } else {
                let tmp_ph = builder.alloc_placeholder(gw, false, merged_four, None, syn_slot.loc);
                syn_slot.placeholder = tmp_ph;
                let prev_errs = builder.diags.len();
                resolve_one_signal(&syn_slot, builder, read);
                if builder.diags.len() > prev_errs {
                    return;
                }
                match builder.nodes[tmp_ph.0 as usize].kind {
                    BuildKind::Alias(target) => target,
                    _ => tmp_ph,
                }
            };
            for &(ms, mlo) in &grp.members {
                slot_parts.entry(ms).or_default().push((mlo, gw, res_v));
            }
        }
    }

    // 5. Stitch resolved segment values back onto each slot's placeholder and emit top `inout` outputs.
    for (sidx, slot) in slots.iter().enumerate() {
        if !in_equiv[sidx] {
            continue;
        }
        if let Some(mut parts) = slot_parts.remove(&sidx) {
            parts.sort_by_key(|p| p.0);
            let full = if parts.len() == 1 && parts[0].0 == 0 && parts[0].1 == slot.width {
                builder.resize_val(parts[0].2, slot.width, false, slot.signed, slot.loc)
            } else {
                let mut msb_first: Vec<ValueId> =
                    parts.iter().rev().map(|&(_, pw, pv)| builder.resize_val(pv, pw, false, false, slot.loc)).collect();
                let any_f4 = slot.four_state || msb_first.iter().any(|&v| builder.node_shape(v).2);
                if msb_first.len() == 1 {
                    builder.resize_val(msb_first.pop().unwrap(), slot.width, false, slot.signed, slot.loc)
                } else {
                    builder.alloc_op(NetOp::Concat, msb_first, slot.width, slot.signed, any_f4, slot.loc)
                }
            };
            builder.bind_placeholder(slot.placeholder, full);
        }
        if let Some(ti) = &slot.top_inout {
            if let Some(mut tparts) = top_io_parts.remove(&sidx) {
                tparts.sort_by_key(|p| p.0);
                let (oe_full, out_full) = if tparts.len() == 1 && tparts[0].0 == 0 && tparts[0].1 == slot.width {
                    (tparts[0].2, tparts[0].3)
                } else {
                    let oe_msb: Vec<ValueId> = tparts
                        .iter()
                        .rev()
                        .map(|&(_, pw, oe, _)| builder.resize_val(oe, pw, false, false, slot.loc))
                        .collect();
                    let out_msb: Vec<ValueId> = tparts
                        .iter()
                        .rev()
                        .map(|&(_, pw, _, ov)| builder.resize_val(ov, pw, false, false, slot.loc))
                        .collect();
                    let oe_f4 = oe_msb.iter().any(|&v| builder.node_shape(v).2);
                    let out_f4 = slot.four_state || out_msb.iter().any(|&v| builder.node_shape(v).2);
                    (
                        builder.alloc_op(NetOp::Concat, oe_msb, slot.width, false, oe_f4, slot.loc),
                        builder.alloc_op(NetOp::Concat, out_msb, slot.width, slot.signed, out_f4, slot.loc),
                    )
                };
                emit_top_inout_outputs(builder, slot, &ti.port_rel_name, oe_full, out_full);
            }
        }
    }
}

fn make_gap_slice(slot: &SignalSlot, lo: u32, hi: u32, builder: &mut NetBuilder) -> ValueId {
    let w = (hi - lo) + 1;
    if let Some(init_cv) = &slot.init {
        return builder.alloc_const(init_cv.slice(lo, w, false), slot.four_state, slot.loc);
    }
    if let Some(pull_high) = slot.pull {
        let fill = if pull_high { Int::from(-1) } else { Int::from(0) };
        return builder.alloc_const(CVal::new(w, false, fill), slot.four_state, slot.loc);
    }
    let uid = builder.undriven.len();
    let name = format!("{}[{hi}:{lo}]", slot.name);
    let uval = builder.alloc_raw(w, false, slot.four_state, BuildKind::Undriven(uid), Some(name), slot.loc);
    builder.undriven.push(UndrivenSlice {
        signal: slot.name.clone(),
        hi,
        lo,
        value: uval,
        loc: slot.loc,
        free: slot.free,
    });
    uval
}
