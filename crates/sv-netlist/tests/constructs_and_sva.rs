//! Integration tests for P1 (1 to 10) and P2 (11 to 14) RTL constructs, cycle-accurate
//! simulation equivalence (`blast_memories`), combinational loop & fail-closed checks,
//! and bound SVA property/sequence trees (`BoundProperty`, `PropExpr`, `SeqExpr`).

mod sim;

use sim::{lower_src, lower_src_cfg, lower_testdata, NetlistSim};
use sv_netlist::{Edge, LowerConfig, LowerOutcome};
use sv_semantic::{CycleBound, CycleRange, PropExpr, PropKind, RepeatKind, SampledKind, SeqExpr, SvaEdge};

fn expect_lowered(out: LowerOutcome) -> sv_netlist::Netlist {
    match out {
        LowerOutcome::Lowered(nl) => nl,
        LowerOutcome::Unsupported(d) => panic!("unexpected Unsupported: {d:#?}"),
        LowerOutcome::Error(d) => panic!("unexpected Error: {d:#?}"),
    }
}

// ============================================================================
// P1 #1: Memories (`fifo.sv`) & `blast_memories` cycle-accurate equivalence
// ============================================================================

#[test]
fn p1_01_fifo_memory_and_blast_equivalence() {
    let nl_mem = expect_lowered(lower_testdata("fifo.sv", &[], &LowerConfig::default()));
    assert_eq!(nl_mem.top, "sync_fifo");
    assert_eq!(nl_mem.memories.len(), 1);
    assert_eq!(nl_mem.memories[0].depth, 4);
    assert_eq!(nl_mem.memories[0].width, 8);
    assert_eq!(nl_mem.properties.len(), 1);
    assert_eq!(nl_mem.properties[0].label.as_deref(), Some("p_full_count"));

    let mut nl_blasted = nl_mem.clone();
    nl_blasted.blast_memories(16);
    assert!(nl_blasted.memories.is_empty(), "depth-4 FIFO memory should be blasted into State registers");
    assert!(nl_blasted.states.len() >= nl_mem.states.len() + 4);

    // Run identical cycle stimulus against both the Memory netlist and the blasted register netlist!
    for nl in [&nl_mem, &nl_blasted] {
        let mut s = NetlistSim::new(nl);
        // Synchronous reset
        s.set_u("rst", 1);
        s.set_u("push", 0);
        s.set_u("pop", 0);
        s.set_u("wdata", 0);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("empty"), 1);
        assert_eq!(s.out_i("full"), 0);
        assert_eq!(s.out_i("count"), 0);

        // Push 4 items: 0x11, 0x22, 0x33, 0x44
        s.set_u("rst", 0);
        for (i, val) in [0x11, 0x22, 0x33, 0x44].into_iter().enumerate() {
            s.set_u("push", 1);
            s.set_u("wdata", val);
            s.tick_posedge("clk");
            assert_eq!(s.out_i("count"), (i + 1) as i128);
        }
        assert_eq!(s.out_i("full"), 1);
        assert_eq!(s.out_i("empty"), 0);

        // Attempt overflow push (guarded by `let do_push = push && !full`)
        s.set_u("push", 1);
        s.set_u("wdata", 0x99);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("count"), 4);
        assert_eq!(s.out_i("rdata"), 0x11);

        // Pop 2 items (`0x11`, `0x22`)
        s.set_u("push", 0);
        s.set_u("pop", 1);
        assert_eq!(s.out_i("rdata"), 0x11);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("rdata"), 0x22);
        assert_eq!(s.out_i("count"), 3);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("rdata"), 0x33);
        assert_eq!(s.out_i("count"), 2);

        // Simultaneous push (`0x55`) + pop (`0x33`) with wraparound
        s.set_u("push", 1);
        s.set_u("pop", 1);
        s.set_u("wdata", 0x55);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("count"), 2);
        assert_eq!(s.out_i("rdata"), 0x44);

        // Drain remaining 2 items (`0x44`, `0x55`)
        s.set_u("push", 0);
        s.set_u("pop", 1);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("rdata"), 0x55);
        assert_eq!(s.out_i("count"), 1);
        s.tick_posedge("clk");
        assert_eq!(s.out_i("count"), 0);
        assert_eq!(s.out_i("empty"), 1);
        assert!(s.failed_asserts.is_empty(), "FIFO assertion failed: {:?}", s.failed_asserts);
    }
}

// ============================================================================
// P1 #2: Static loop unrolling (`for`, `while`, `do...while`, `repeat`, `foreach`, `break`/`continue`)
// ============================================================================

#[test]
fn p1_02_static_loops_for_while_do_repeat_foreach_break_continue() {
    let src = r#"
module loop_suite (
    input  logic [7:0] x,
    input  logic [2:0] stop_at,
    output logic [7:0] rev,
    output logic [7:0] sum_even,
    output logic [3:0] first_set,
    output logic [7:0] rep_acc,
    output logic [7:0] asc_copy
);
    logic [0:7] asc_tmp;

    // 1. Multi-init / comma-step `for`: bit reversal
    always_comb begin
        rev = '0;
        for (int i = 0, j = 7; i < 8; i++, j--) begin
            rev[j] = x[i];
        end
    end

    // 2. `while` + `continue`: sum indices 0,2,4,6 where x[i] is set
    always_comb begin
        int k;
        k = 0;
        sum_even = '0;
        while (k < 8) begin
            int cur;
            cur = k;
            k = k + 1;
            if (cur[0]) continue;
            if (x[cur]) sum_even = sum_even + cur[7:0] + 8'd1;
        end
    end

    // 3. Dynamic `break` inside `for`: find first set bit up to `stop_at`
    always_comb begin
        first_set = 4'd8;
        for (int i = 0; i < 8; i++) begin
            if (i[2:0] == stop_at) break;
            if (x[i]) begin
                first_set = i[3:0];
                break;
            end
        end
    end

    // 4. `do...while` + `repeat`
    always_comb begin
        int n;
        n = 0;
        rep_acc = x;
        do begin
            rep_acc = rep_acc + 8'd2;
            n++;
        end while (n < 2);
        repeat (3) begin
            rep_acc = rep_acc + 8'd1;
        end
    end

    // 5. `foreach` over ascending `[0:7]` packed vector
    always_comb begin
        foreach (asc_tmp[idx]) begin
            asc_tmp[idx] = x[idx];
        end
        asc_copy = asc_tmp;
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("x", 0b1011_0100);
    sim.set_u("stop_at", 7);
    assert_eq!(sim.out_i("rev"), 0b0010_1101);
    // Even indices set in 0b1011_0100: bit 2 (val 3), bit 4 (val 5) -> 8
    assert_eq!(sim.out_i("sum_even"), 8);
    // Lowest set bit is index 2 (< stop_at=7)
    assert_eq!(sim.out_i("first_set"), 2);
    // If stop_at == 2, loop breaks before seeing bit 2 -> stays 8!
    sim.set_u("stop_at", 2);
    assert_eq!(sim.out_i("first_set"), 8);
    // do..while (+4) + repeat(3) (+3) = x + 7
    assert_eq!(sim.out_i("rep_acc"), 0b1011_0100 + 7);
    // asc_tmp[0] (MSB of [0:7]) = x[0] (LSB of [7:0]) -> bit-reversed!
    assert_eq!(sim.out_i("asc_copy"), 0b0010_1101);

    // Verify bounded loop guard returns `LowerOutcome::Unsupported` when exceeding `max_loop_iters`.
    let unbounded = r#"
module too_many (input logic [7:0] a, output logic [7:0] b);
    always_comb begin
        b = a;
        for (int i = 0; i < 64; i++) b = b + 8'd1;
    end
endmodule
"#;
    let cfg = LowerConfig { max_loop_iters: 16, ..LowerConfig::default() };
    match lower_src_cfg(unbounded, &[], &cfg) {
        LowerOutcome::Unsupported(d) => assert!(d.iter().any(|e| e.message.contains("loop"))),
        other => panic!("expected Unsupported on exceeded max_loop_iters, got {other:?}"),
    }
}

// ============================================================================
// P1 #3: Timing-free `function` & `task` inlining (incl. package fn, early `return`, `output`/`inout`)
// ============================================================================

#[test]
fn p1_03_function_and_task_inlining_and_fail_closed() {
    let src = r#"
package math_pkg;
    function automatic logic [7:0] clamp_add(input logic [7:0] a, input logic [7:0] b);
        logic [8:0] sum;
        sum = {1'b0, a} + {1'b0, b};
        if (sum[8]) return 8'hFF;
        clamp_add = sum[7:0];
    endfunction
endpackage

module sub_top (
    input  logic [7:0] a,
    input  logic [7:0] b,
    output logic [7:0] sat,
    output logic [7:0] swapped_hi,
    output logic [7:0] swapped_lo
);
    function automatic logic [3:0] nibble_xor(input logic [7:0] v);
        nibble_xor = v[7:4] ^ v[3:0];
    endfunction

    task automatic accum_and_swap(
        inout  logic [7:0] x,
        inout  logic [7:0] y,
        output logic [7:0] s
    );
        logic [7:0] tmp;
        s = math_pkg::clamp_add(x, y);
        tmp = x;
        x = y ^ {4'h0, nibble_xor(tmp)};
        y = tmp;
    endtask

    always_comb begin
        logic [7:0] lx, ly;
        lx = a;
        ly = b;
        accum_and_swap(lx, ly, sat);
        swapped_hi = lx;
        swapped_lo = ly;
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("a", 0xF0);
    sim.set_u("b", 0x25);
    assert_eq!(sim.out_i("sat"), 0xFF); // 0xF0 + 0x25 = 0x115 -> saturated to 0xFF
    assert_eq!(sim.out_i("swapped_lo"), 0xF0);
    assert_eq!(sim.out_i("swapped_hi"), 0x25 ^ 0x0F); // nibble_xor(0xF0) = 0xF

    sim.set_u("a", 0x12);
    sim.set_u("b", 0x34);
    assert_eq!(sim.out_i("sat"), 0x46);
    assert_eq!(sim.out_i("swapped_lo"), 0x12);
    assert_eq!(sim.out_i("swapped_hi"), 0x34 ^ 0x03);

    // Recursive function must fail closed with `LowerOutcome::Error`.
    let rec = r#"
module rec_bad (input logic [3:0] x, output logic [3:0] y);
    function automatic logic [3:0] f(input logic [3:0] n);
        if (n == 0) return 0;
        return f(n - 1);
    endfunction
    assign y = f(x);
endmodule
"#;
    assert!(matches!(lower_src(rec), LowerOutcome::Error(_)));

    // Task with timing control `#1` must fail closed with `LowerOutcome::Unsupported`.
    let timed = r#"
module timed_bad (input logic a, output logic b);
    task automatic t(output logic o);
        #1 o = a;
    endtask
    always_comb t(b);
endmodule
"#;
    assert!(matches!(lower_src(timed), LowerOutcome::Unsupported(_)));
}

// ============================================================================
// P1 #4: Nested packed/unpacked `struct` & `union packed` RMW
// ============================================================================

#[test]
fn p1_04_packed_unpacked_structs_and_unions() {
    let src = r#"
module struct_union_top (
    input  logic        sel_raw,
    input  logic [15:0] raw_in,
    input  logic [7:0]  hi_override,
    output logic [7:0]  out_hi,
    output logic [7:0]  out_lo,
    output logic [7:0]  unp_sum
);
    typedef struct packed {
        logic [7:0] hi;
        logic [7:0] lo;
    } pair_t;

    typedef union packed {
        logic [15:0] raw;
        pair_t       pair;
    } view_u;

    typedef struct {
        logic [7:0] a;
        logic [7:0] b;
    } unp_s;

    view_u u;
    unp_s  rec;

    always_comb begin
        u.raw = raw_in;
        if (!sel_raw) begin
            u.pair.hi = hi_override;
        end
        out_hi = u.pair.hi;
        out_lo = u.pair.lo;

        rec.a = u.pair.hi;
        rec.b = u.pair.lo;
        unp_sum = rec.a + rec.b;
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("raw_in", 0xABCD);
    sim.set_u("hi_override", 0x42);
    sim.set_u("sel_raw", 1);
    assert_eq!(sim.out_i("out_hi"), 0xAB);
    assert_eq!(sim.out_i("out_lo"), 0xCD);
    assert_eq!(sim.out_i("unp_sum"), (0xAB + 0xCD) & 0xFF);

    sim.set_u("sel_raw", 0);
    assert_eq!(sim.out_i("out_hi"), 0x42);
    assert_eq!(sim.out_i("out_lo"), 0xCD);
    assert_eq!(sim.out_i("unp_sum"), (0x42 + 0xCD) & 0xFF);
}

// ============================================================================
// P1 #5: Compound `op=` & `++`/`--` (including signed `>>>=` and array element RMW)
// ============================================================================

#[test]
fn p1_05_compound_assign_and_inc_dec() {
    let src = r#"
module compound_ops (
    input  logic               clk,
    input  logic               rst,
    input  logic [1:0]         idx,
    input  logic signed [7:0]  sin,
    output logic signed [7:0]  ashr_out,
    output logic [7:0]         elem_out
);
    logic [7:0] arr [0:3];

    always_comb begin
        logic signed [7:0] tmp;
        tmp = sin;
        tmp >>>= 2;
        tmp += 8'sd1;
        ashr_out = tmp;
    end

    always_ff @(posedge clk) begin
        if (rst) begin
            for (int i = 0; i < 4; i++) arr[i] <= 8'd10;
        end else begin
            arr[idx] += 8'd5;
        end
    end

    assign elem_out = arr[idx];
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    // -16 (0xF0) >>> 2 = -4, + 1 = -3
    sim.set_u("sin", -16);
    assert_eq!(sim.out_i("ashr_out"), -3);

    sim.set_u("rst", 1);
    sim.set_u("idx", 2);
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("elem_out"), 10);

    sim.set_u("rst", 0);
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("elem_out"), 15);
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("elem_out"), 20);
    sim.set_u("idx", 1);
    assert_eq!(sim.out_i("elem_out"), 10);
}

// ============================================================================
// P1 #6: `case inside`, `casez`, `casex`, `always_comb` latch check, and `always_latch`
// ============================================================================

#[test]
fn p1_06_case_inside_casez_latch_error_and_always_latch() {
    let src = r#"
module case_and_latch (
    input  logic [3:0] x,
    input  logic       latch_en,
    input  logic [3:0] latch_d,
    output logic [1:0] cls,
    output logic [1:0] cz,
    output logic [3:0] latch_q
);
    always_comb begin
        case (x) inside
            [4'd1 : 4'd3]: cls = 2'd1;
            4'b1?0?:       cls = 2'd2;
            default:       cls = 2'd0;
        endcase
    end

    always_comb begin
        casez (x)
            4'b11??: cz = 2'd3;
            4'b01?1: cz = 2'd2;
            default: cz = 2'd1;
        endcase
    end

    always_latch begin
        if (latch_en) latch_q <= latch_d;
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    assert!(nl.states.iter().any(|s| s.clock.is_none() && s.latch_enable.is_some()));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("x", 2);
    assert_eq!(sim.out_i("cls"), 1);
    sim.set_u("x", 0b1101);
    assert_eq!(sim.out_i("cls"), 2);
    assert_eq!(sim.out_i("cz"), 3);
    sim.set_u("x", 0b0101);
    assert_eq!(sim.out_i("cz"), 2);

    // Transparent latch holds when `latch_en == 0` and passes through when `latch_en == 1`!
    sim.set_u("latch_en", 1);
    sim.set_u("latch_d", 0xA);
    assert_eq!(sim.out_i("latch_q"), 0xA);
    sim.set_u("latch_en", 0);
    sim.set_u("latch_d", 0x5);
    assert_eq!(sim.out_i("latch_q"), 0xA);

    // Accidental latch in `always_comb` MUST fail closed with `"inferred latch"`!
    let bad_comb = r#"
module bad_latch (input logic en, input logic [3:0] d, output logic [3:0] q);
    always_comb begin
        if (en) q = d;
    end
endmodule
"#;
    match lower_src(bad_comb) {
        LowerOutcome::Error(diags) => {
            assert!(diags.iter().any(|d| d.message.contains("inferred latch")), "got {diags:?}");
        }
        other => panic!("expected Error on inferred latch in always_comb, got {other:?}"),
    }
}

// ============================================================================
// P1 #7: Indexed part-selects (`+:`, `-:`) & `[0:N]` ascending bit-order vectors
// ============================================================================

#[test]
fn p1_07_indexed_part_selects_and_ascending_ranges() {
    let src = r#"
module part_selects (
    input  logic [15:0] data,
    input  logic [3:0]  base,
    input  logic [3:0]  patch,
    output logic [3:0]  plus_rd,
    output logic [3:0]  minus_rd,
    output logic [15:0] patched,
    output logic [3:0]  asc_hi_nibble
);
    logic [0:7] asc;

    always_comb begin
        plus_rd  = data[base +: 4];
        minus_rd = data[base -: 4];

        patched = data;
        patched[base +: 4] = patch;

        asc = data[7:0];
        // In `logic [0:7] asc`, index 0 is the MSB and `[0:3]` is the upper nibble (`data[7:4]`)!
        asc_hi_nibble = asc[0:3];
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("data", 0xABCD);
    sim.set_u("base", 4);
    sim.set_u("patch", 0xE);
    assert_eq!(sim.out_i("plus_rd"), 0xC); // bits [7:4] of 0xABCD
    assert_eq!(sim.out_i("patched"), 0xABED);
    assert_eq!(sim.out_i("asc_hi_nibble"), 0xC); // upper nibble of 0xCD

    sim.set_u("base", 7);
    assert_eq!(sim.out_i("minus_rd"), 0xC); // bits [7:4] via `[7 -: 4]`
}

// ============================================================================
// P1 #8: System functions (`$clog2`, `$bits`, `$countones`, `$onehot`, `$onehot0`, `$past`, `$rose`, `$fell`, `$stable`, `$changed`)
// ============================================================================

#[test]
fn p1_08_system_functions_and_sampled_value_nodes() {
    let src = r#"
module sys_and_sampled (
    input  logic       clk,
    input  logic       en,
    input  logic [7:0] x,
    output logic [31:0] clog_dyn,
    output logic [31:0] struct_bits,
    output logic [31:0] ones,
    output logic        oh,
    output logic        oh0,
    output logic [7:0]  past2,
    output logic        rose_lsb,
    output logic        fell_lsb,
    output logic        stable_x,
    output logic        changed_x
);
    typedef struct packed {
        logic [4:0] a;
        logic [2:0] b;
    } s8_t;
    s8_t dummy;

    assign clog_dyn    = $clog2(x);
    assign struct_bits = $bits(dummy);
    assign ones        = $countones(x);
    assign oh          = $onehot(x);
    assign oh0         = $onehot0(x);

    always_ff @(posedge clk) begin
        past2     <= $past(x, 2, en);
        rose_lsb  <= $rose(x[0]);
        fell_lsb  <= $fell(x[0]);
        stable_x  <= $stable(x);
        changed_x <= $changed(x);
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    assert_eq!(nl.sampled.len(), 5);
    assert!(nl
        .sampled
        .iter()
        .any(|s| s.kind == SampledKind::Past && s.depth == 2 && s.gate.is_some() && s.clock.is_some()));
    assert!(nl.sampled.iter().any(|s| s.kind == SampledKind::Rose));
    assert!(nl.sampled.iter().any(|s| s.kind == SampledKind::Fell));
    assert!(nl.sampled.iter().any(|s| s.kind == SampledKind::Stable));
    assert!(nl.sampled.iter().any(|s| s.kind == SampledKind::Changed));

    let mut sim = NetlistSim::new(&nl);
    assert_eq!(sim.out_i("struct_bits"), 8);
    for (v, exp_clog) in [(0, 0), (1, 0), (2, 1), (3, 2), (4, 2), (5, 3), (8, 3), (9, 4)] {
        sim.set_u("x", v);
        assert_eq!(sim.out_i("clog_dyn"), exp_clog, "$clog2({v})");
    }

    sim.set_u("x", 0);
    assert_eq!(sim.out_i("ones"), 0);
    assert_eq!(sim.out_i("oh"), 0);
    assert_eq!(sim.out_i("oh0"), 1);

    sim.set_u("x", 0b0010_0000);
    assert_eq!(sim.out_i("ones"), 1);
    assert_eq!(sim.out_i("oh"), 1);
    assert_eq!(sim.out_i("oh0"), 1);

    sim.set_u("x", 0b0010_0101);
    assert_eq!(sim.out_i("ones"), 3);
    assert_eq!(sim.out_i("oh"), 0);
    assert_eq!(sim.out_i("oh0"), 0);

    // Step sampled-value history across clock ticks!
    sim.set_u("en", 1);
    sim.set_u("x", 0x11); // LSB = 1
    sim.tick_posedge("clk");
    sim.set_u("x", 0x20); // LSB = 0 (fell!)
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("fell_lsb"), 1);
    assert_eq!(sim.out_i("changed_x"), 1);
    sim.set_u("x", 0x20); // unchanged -> stable!
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("stable_x"), 1);
    assert_eq!(sim.out_i("past2"), 0x11);
}

// ============================================================================
// P1 #9 & #10: Bound SVA Property & Sequence Trees (`BoundProperty`, `PropExpr`, `SeqExpr`)
// ============================================================================

#[test]
fn p1_09_10_bound_sva_sequences_properties_clocking_let_and_lrm_ch16() {
    let src = r#"
module sva_suite (
    input logic clk,
    input logic rst_n,
    input logic req,
    input logic gnt,
    input logic done,
    input logic abort
);
    default clocking cb @(posedge clk); endclocking
    default disable iff (!rst_n);

    let handshake_ok = gnt && !abort;

    sequence s_req_gnt(r, g);
        r ##[1:3] g;
    endsequence

    property p_proto(r, g, d);
        s_req_gnt(r, g) |=> (!abort throughout d[->1]);
    endproperty

    a_proto:   assert   property (p_proto(req, handshake_ok, done));
    m_no_abrt: assume   property (req |-> ##0 !abort[*2]);
    c_fusion:  cover    property ((req ##1 gnt) #-# s_eventually done);
    r_until:   restrict property (accept_on(abort) (gnt implies (req until done)));
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    assert_eq!(nl.properties.len(), 4);

    // 1. `a_proto`: inherits `default clocking cb @(posedge clk)` and `default disable iff (!rst_n)`
    let a_proto = nl.properties.iter().find(|p| p.label.as_deref() == Some("a_proto")).unwrap();
    assert_eq!(a_proto.kind, PropKind::Assert);
    assert_eq!(a_proto.clock.as_ref().map(|c| c.edge), Some(SvaEdge::Posedge));
    assert!(a_proto.disable_iff.is_some(), "should inherit default disable iff (!rst_n)");
    match &a_proto.expr {
        PropExpr::NonOverlappedImplication { lhs, rhs } => {
            match lhs {
                SeqExpr::Delay { range, lhs: Some(_), rhs: _ } => {
                    assert_eq!(*range, CycleRange { min: 1, max: CycleBound::Const(3) });
                }
                other => panic!("expected expanded s_req_gnt Delay[1:3], got {other:?}"),
            }
            match rhs.as_ref() {
                PropExpr::Seq(SeqExpr::Throughout { seq, .. }) => match seq.as_ref() {
                    SeqExpr::Repeat { kind: RepeatKind::Goto, range, .. } => {
                        assert_eq!(*range, CycleRange::exact(1));
                    }
                    other => panic!("expected Goto[->1], got {other:?}"),
                },
                other => panic!("expected Throughout sequence property, got {other:?}"),
            }
        }
        other => panic!("expected NonOverlappedImplication, got {other:?}"),
    }

    // 2. `m_no_abrt`: `req |-> ##0 !abort[*2]` (`Fusion` + `Consecutive` repeat)
    let m_no = nl.properties.iter().find(|p| p.label.as_deref() == Some("m_no_abrt")).unwrap();
    assert_eq!(m_no.kind, PropKind::Assume);
    match &m_no.expr {
        PropExpr::OverlappedImplication { rhs, .. } => match rhs.as_ref() {
            PropExpr::Seq(SeqExpr::Delay { range, lhs: None, rhs: rep }) if *range == CycleRange::exact(0) => {
                assert!(matches!(rep.as_ref(), SeqExpr::Repeat { kind: RepeatKind::Consecutive, .. }));
            }
            PropExpr::Seq(SeqExpr::Repeat { kind: RepeatKind::Consecutive, .. })
            | PropExpr::Seq(SeqExpr::Fusion(_, _)) => {}
            other => panic!("unexpected m_no_abrt rhs: {other:?}"),
        },
        other => panic!("expected OverlappedImplication, got {other:?}"),
    }

    // 3. `c_fusion`: `#-#` (`FollowedBy { overlapped: true }`) + `Eventually { strong: true }`
    let c_fus = nl.properties.iter().find(|p| p.label.as_deref() == Some("c_fusion")).unwrap();
    assert_eq!(c_fus.kind, PropKind::Cover);
    assert!(matches!(
        &c_fus.expr,
        PropExpr::FollowedBy { overlapped: true, rhs, .. }
            if matches!(rhs.as_ref(), PropExpr::Eventually { strong: true, .. })
    ));

    // 4. `r_until`: `AcceptOn` + `Implies` + `Until`
    let r_until = nl.properties.iter().find(|p| p.label.as_deref() == Some("r_until")).unwrap();
    assert_eq!(r_until.kind, PropKind::Restrict);
    assert!(matches!(&r_until.expr, PropExpr::AcceptOn { .. }));

    // Also parse & elaborate `testdata/conformance/lrm/ch16_assertions.sv` and verify SvaBinder never panics!
    let ch16_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/lrm/ch16_assertions.sv");
    let ch16_tree = sv_syntax::parse_file(&ch16_path, sv_syntax::Options::default());
    let (_, elab) = sv_semantic::semantic_diags_with_params(std::slice::from_ref(&ch16_tree), &[], &[]);
    assert!(!elab.design.tops.is_empty());
    let mut arena = sv_semantic::ExprArena::new();
    let mut binder = sv_semantic::SvaBinder::new(std::slice::from_ref(&ch16_tree), &elab.design, &mut arena);
    let mut bound_count = 0usize;
    for inst in &elab.design.instances {
        for item in &inst.items {
            if item.kind == sv_semantic::elab_ir::ElabItemKind::ConcurrentAssert
                && binder
                    .bind_concurrent_assert(item.tree, item.node, inst.id, &item.scope_prefix, &item.gen_consts, None)
                    .is_some()
            {
                bound_count += 1;
            }
        }
    }
    assert!(bound_count > 0, "expected ch16_assertions.sv concurrent properties to bind cleanly");
}

// ============================================================================
// P2 #11 & #12: `interface` + `modport` flattening & Aggregate top-level ports
// ============================================================================

#[test]
fn p2_11_interface_and_modport_flattening_and_direction_check() {
    let src = r#"
interface bus_if #(parameter int W = 8) (input logic clk);
    logic         valid;
    logic [W-1:0] data;
    logic [W-1:0] resp;

    modport master (output valid, output data, input resp, input clk);
    modport slave  (input  valid, input  data, output resp, input clk);
endinterface

module producer (bus_if.master bus, input logic en, input logic [7:0] din);
    assign bus.valid = en;
    assign bus.data  = din + 8'd1;
endmodule

module consumer (bus_if.slave bus);
    always_ff @(posedge bus.clk) begin
        if (bus.valid) bus.resp <= bus.data ^ 8'hFF;
    end
endmodule

module soc_top (
    input  logic       clk,
    input  logic       en,
    input  logic [7:0] din,
    output logic [7:0] dout
);
    bus_if #(.W(8)) u_bus (.clk(clk));
    producer u_prod (.bus(u_bus), .en(en), .din(din));
    consumer u_cons (.bus(u_bus));
    assign dout = u_bus.resp;
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    assert_eq!(nl.states.len(), 1);
    assert_eq!(nl.states[0].clock.map(|c| c.edge), Some(Edge::Posedge));

    let mut sim = NetlistSim::new(&nl);
    sim.set_u("en", 1);
    sim.set_u("din", 0x41); // bus.data = 0x42 -> bus.resp = 0x42 ^ 0xFF = 0xBD
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("dout"), 0xBD);

    // Driving a `modport` `input` signal must fail closed!
    let bad_mp = r#"
interface sif;
    logic a;
    modport rd (input a);
endinterface
module writer (sif.rd p);
    assign p.a = 1'b1;
endmodule
module top (output logic y);
    sif u();
    writer w(u);
    assign y = u.a;
endmodule
"#;
    assert!(matches!(lower_src(bad_mp), LowerOutcome::Error(_)));
}

#[test]
fn p2_12_aggregate_top_level_ports_scalarized() {
    let src = r#"
package types_pkg;
    typedef struct {
        logic [3:0] a;
        logic [3:0] b;
    } req_s;
endpackage

module agg_top (
    input  types_pkg::req_s req,
    input  logic [7:0]      vec_in [0:1],
    output types_pkg::req_s resp,
    output logic [7:0]      sum_out
);
    always_comb begin
        resp.a  = req.b;
        resp.b  = req.a;
        sum_out = vec_in[0] + vec_in[1] + {4'h0, req.a};
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let in_names: Vec<&str> = nl.inputs.iter().map(|p| p.name.as_str()).collect();
    let out_names: Vec<&str> = nl.outputs.iter().map(|p| p.name.as_str()).collect();
    assert!(in_names.contains(&"req.a") && in_names.contains(&"req.b"), "inputs: {in_names:?}");
    assert!(in_names.contains(&"vec_in[0]") && in_names.contains(&"vec_in[1]"), "inputs: {in_names:?}");
    assert!(out_names.contains(&"resp.a") && out_names.contains(&"resp.b"), "outputs: {out_names:?}");

    let mut sim = NetlistSim::new(&nl);
    sim.set_u("req.a", 0x3);
    sim.set_u("req.b", 0x9);
    sim.set_u("vec_in[0]", 10);
    sim.set_u("vec_in[1]", 20);
    assert_eq!(sim.out_i("resp.a"), 0x9);
    assert_eq!(sim.out_i("resp.b"), 0x3);
    assert_eq!(sim.out_i("sum_out"), 33);
}

// ============================================================================
// Combinational loop SCC cycle detection & Non-synthesizable fail-closed checks
// ============================================================================

#[test]
fn comb_loop_and_non_synthesizable_fail_closed() {
    match lower_testdata("comb_loop.sv", &[], &LowerConfig::default()) {
        LowerOutcome::Error(diags) => {
            assert!(
                diags.iter().any(|d| d.message.contains("combinational loop")
                    && d.message.contains("comb_loop.a")
                    && d.message.contains("comb_loop.b")),
                "expected cycle path naming comb_loop.a and comb_loop.b, got {diags:?}"
            );
        }
        other => panic!("expected combinational loop Error, got {other:?}"),
    }

    // `real` variable in RTL module -> fail-closed Error/Unsupported
    let real_mod = "module m (input logic clk, output logic y); real r; assign y = clk; endmodule";
    assert!(!matches!(lower_src(real_mod), LowerOutcome::Lowered(_)));

    // `string` variable in RTL module -> fail-closed Error/Unsupported
    let str_mod = "module m (input logic clk, output logic y); string s; assign y = clk; endmodule";
    assert!(!matches!(lower_src(str_mod), LowerOutcome::Lowered(_)));

    // `inout` tri-state port -> fail-closed Unsupported/Error
    let inout_mod = "module m (inout wire io, input logic en); assign io = en ? 1'b1 : 1'bz; endmodule";
    assert!(!matches!(lower_src(inout_mod), LowerOutcome::Lowered(_)));
}

#[test]
fn regression_past_feedback_and_sva_guards() {
    let past_fb = r#"
module past_fb (input bit clk, input bit [7:0] x, output bit [7:0] y);
    assign y = $past(y, 1, , @(posedge clk)) + x;
endmodule
"#;
    let nl = expect_lowered(lower_src(past_fb));
    let samp_id = nl
        .values
        .iter()
        .find(|v| matches!(v.kind, sv_netlist::ValueKind::Sampled(_)))
        .map(|v| v.id)
        .expect("Sampled ValueNode");
    let add_id = nl
        .values
        .iter()
        .find(|v| matches!(v.kind, sv_netlist::ValueKind::Op { op: sv_netlist::NetOp::Add, .. }))
        .map(|v| v.id)
        .expect("Add ValueNode");
    assert!(samp_id.0 < add_id.0, "Sampled ValueId ({samp_id:?}) must precede Add ValueId ({add_id:?})");
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("x", 5);
    assert_eq!(sim.out_i("y"), 5);
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("y"), 10);

    let cond_ca = "module m (input logic clk, en, a); always_ff @(posedge clk) if (en) assert property (a); endmodule";
    // Under a condition: lowered, with the path condition kept for the property.
    let cond_nl = lower_src(cond_ca).unwrap_netlist();
    assert!(cond_nl.property_enables[0].is_some());
    assert!(cond_nl.dump().contains(" when=v"), "{}", cond_nl.dump());
    let loop_ca =
        "module m (input logic clk, a); always_ff @(posedge clk) for (int i=0;i<2;i++) assert property (a); endmodule";
    let loop_nl = lower_src(loop_ca).unwrap_netlist();
    assert_eq!(loop_nl.properties.len(), 2);
    assert_eq!(loop_nl.property_names, ["m._assert_1", "m._assert_2"]);
    let init_ca = "module m (input logic clk, a); initial assert property (@(posedge clk) a); endmodule";
    assert!(matches!(lower_src(init_ca), LowerOutcome::Unsupported(_)));

    let sva_isunk = "module m (input logic clk, a); assert property (@(posedge clk) !$isunknown(a)); endmodule";
    assert!(matches!(lower_src(sva_isunk), LowerOutcome::Unsupported(_)));
    let nl_4 = expect_lowered(lower_src_cfg(
        sva_isunk,
        &[],
        &LowerConfig { allow_four_state_ops: true, ..LowerConfig::default() },
    ));
    assert_eq!(nl_4.properties.len(), 1);
    assert!(nl_4
        .values
        .iter()
        .any(|v| matches!(v.kind, sv_netlist::ValueKind::Op { op: sv_netlist::NetOp::IsUnknown, .. })));
}

#[test]
fn regression_signed_cast_casez_and_exhaustive_case() {
    let src = r#"
module signed_and_case (
    input  logic        [3:0] u4,
    input  logic        [3:0] cz_sel,
    input  logic signed [1:0] sel2,
    output logic signed [7:0] acc_out,
    output logic              case_neg1,
    output logic              in_neg,
    output logic        [1:0] cz_out,
    output logic        [1:0] s2_out,
    output logic              all_ones
);
    always_comb begin
        logic signed [7:0] acc;
        acc = 8'sd10;
        acc += $signed(u4);
        acc_out = acc;
        case ($signed(u4))
            -4'sd1:  case_neg1 = 1'b1;
            default: case_neg1 = 1'b0;
        endcase
        in_neg = ($signed(u4) inside {[-4:-1]});
        casez (cz_sel)
            4'b10xx: cz_out = 2'd1;
            4'b10zz: cz_out = 2'd2;
            default: cz_out = 2'd3;
        endcase
        case (sel2)
            -2: s2_out = 2'd0;
            -1: s2_out = 2'd1;
             0: s2_out = 2'd2;
             1: s2_out = 2'd3;
        endcase
        case (u4)
            '1:      all_ones = 1'b1;
            default: all_ones = 1'b0;
        endcase
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("u4", 0xF);
    sim.set_u("cz_sel", 0b1010);
    sim.set_u("sel2", -2);
    assert_eq!(sim.out_i("acc_out"), 9);
    assert_eq!(sim.out_i("case_neg1"), 1);
    assert_eq!(sim.out_i("in_neg"), 1);
    assert_eq!(sim.out_i("cz_out"), 2);
    assert_eq!(sim.out_i("s2_out"), 0);
    assert_eq!(sim.out_i("all_ones"), 1);
    sim.set_u("u4", 1);
    assert_eq!(sim.out_i("all_ones"), 0);
}

#[test]
fn regression_memories_descending_rom_partial_nba_and_scalarize() {
    let src = r#"
module sub_reader (input logic [7:0] arr [0:1], output logic [7:0] s);
    assign s = arr[0] + arr[1];
endmodule

module mem_cases (
    input  logic       clk,
    input  logic [1:0] raddr,
    input  logic [1:0] waddr,
    input  logic [3:0] lo_nib,
    input  logic [3:0] hi_nib,
    output logic [7:0] rom_val,
    output logic [7:0] ram_val,
    output logic [7:0] port_sum
);
    logic [7:0] rom [3:0] = '{8'hAA, 8'hBB, 8'hCC, 8'hDD};
    assign rom_val = rom[raddr];

    logic [7:0] ram [0:3];
    always_ff @(posedge clk) begin
        ram[waddr][3:0] <= lo_nib;
        ram[waddr][7:4] <= hi_nib;
    end
    assign ram_val = ram[raddr];

    logic [7:0] passed [0:1];
    assign passed[0] = 8'd12;
    assign passed[1] = 8'd30;
    sub_reader u_sub (.arr(passed), .s(port_sum));
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    assert_eq!(nl.memories.len(), 2, "rom and ram stay Memory; passed scalarizes to Elems");
    let states_before = nl.states.len();
    let mut rom_only = expect_lowered(lower_src(
        "module rom_m (input logic [1:0] a, output logic [7:0] y);\
           logic [7:0] rom [3:0] = '{8'hAA, 8'hBB, 8'hCC, 8'hDD};\
           assign y = rom[a];\
         endmodule",
    ));
    rom_only.blast_memories(16);
    assert_eq!(rom_only.states.len(), 0, "blasting ROM must create zero State registers");
    assert_eq!(states_before, 0);

    let mut sim = NetlistSim::new(&nl);
    for (addr, exp) in [(3, 0xAA), (2, 0xBB), (1, 0xCC), (0, 0xDD)] {
        sim.set_u("raddr", addr);
        assert_eq!(sim.out_i("rom_val"), exp);
    }
    assert_eq!(sim.out_i("port_sum"), 42);
    sim.set_u("waddr", 2);
    sim.set_u("lo_nib", 0x5);
    sim.set_u("hi_nib", 0xA);
    sim.tick_posedge("clk");
    sim.set_u("raddr", 2);
    assert_eq!(sim.out_i("ram_val"), 0xA5);
}

#[test]
fn regression_interface_task_async_reset_check_multibit_clk_oob_and_foreach() {
    let src = r#"
interface pkt_if;
    logic [7:0] data;
    task automatic write_pkt(input logic [7:0] v);
        data <= v;
    endtask
endinterface

module drv (pkt_if bus, input logic clk, input logic [7:0] din);
    always_ff @(posedge clk) bus.write_pkt(din);
endmodule

module misc_top (
    input  logic        clk,
    input  logic        rst,
    input  logic        rst_ok,
    input  logic [1:0]  clk_vec,
    input  logic [7:0]  din,
    input  logic [3:0]  nib,
    output logic [7:0]  if_out,
    output logic [7:0]  ar_q,
    output logic [7:0]  mb_q,
    output bit   [7:0]  oob_wr,
    output bit   [3:0]  oob_rd,
    output bit   [39:0] wide_dec,
    output bit   [7:0]  mat_sum
);
    pkt_if u_if();
    drv u_drv (.bus(u_if), .clk(clk), .din(din));
    assign if_out = u_if.data;

    always_ff @(posedge clk or posedge rst) begin
        if (rst) begin
            assert (rst_ok);
            ar_q <= 8'd0;
        end else begin
            ar_q <= din;
        end
    end

    always_ff @(posedge clk_vec) mb_q <= din;

    always_comb begin
        bit [7:0] v;
        bit [39:0] w40;
        bit [7:0] mat [0:1][0:1];
        bit [7:0] s;
        v = 8'b0011_0101;
        v[9:6] = nib;
        oob_wr = v;
        oob_rd = v[6 +: 4];
        w40 = 40'd0;
        w40 += -1;
        wide_dec = w40;
        mat[0][0] = 8'd1; mat[0][1] = 8'd2;
        mat[1][0] = 8'd3; mat[1][1] = 8'd4;
        s = 8'd0;
        foreach (mat[i, j]) s += mat[i][j];
        mat_sum = s;
    end
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("din", 0x77);
    sim.set_u("nib", 0b1110); // writes v[7:6] = 2'b10 -> v = 8'b1011_0101 = 0xB5; v[6 +: 4] = 4'b0010
    sim.set_u("rst", 0);
    sim.set_u("rst_ok", 0);
    sim.tick_posedge("clk");
    assert!(sim.failed_asserts.is_empty(), "rst-branch assert must not fire when rst=0");
    assert_eq!(sim.out_i("if_out"), 0x77);
    assert_eq!(sim.out_i("ar_q"), 0x77);
    assert_eq!(sim.out_i("oob_wr"), 0xB5);
    assert_eq!(sim.out_i("oob_rd"), 0b0010);
    assert_eq!(sim.out_i("wide_dec"), 0xFF_FFFF_FFFF);
    assert_eq!(sim.out_i("mat_sum"), 10);

    // Toggling MSB of `clk_vec` (`00 -> 10`) must NOT trigger `@(posedge clk_vec)`; bit 0 (`10 -> 11`) MUST!
    sim.set_u("din", 0x3C);
    sim.set_u("clk_vec", 0b10);
    sim.eval_comb();
    sim.set_u("clk_vec", 0b11);
    // Drive posedge via NetlistSim state update helper:
    sim.set_u("clk_vec", 0b00);
    sim.tick_posedge("clk_vec"); // sets 0 -> 1 (LSB 0 -> 1)
    assert_eq!(sim.out_i("mb_q"), 0x3C);

    // Asserting `rst = 1` while `rst_ok = 0` triggers the gated reset check on clock evaluation.
    sim.set_u("rst", 1);
    sim.set_u("rst_ok", 0);
    sim.tick_posedge("clk");
    assert!(!sim.failed_asserts.is_empty(), "rst-branch assert must fire when rst=1 and rst_ok=0");
}

// ============================================================================
// Immediate assertions with a null pass statement inside if / else
// ============================================================================

/// `if (c) assume (a); else assume (b);`: the `;` is the assertion's null
/// action block (IEEE 1800-2023 12.4), so the `else` belongs to the `if` and
/// both assumptions are checks, each under its own path condition.
#[test]
fn immediate_assert_null_action_keeps_if_else() {
    let nl = expect_lowered(lower_src(
        "module t(input logic r, input logic [3:0] c);\n\
           always @* if (c == 4'd0) assume (!r); else assume (r);\n\
           always_comb if (c == 4'd1) assert (r); else assert (!r);\n\
         endmodule\n",
    ));
    let kinds: Vec<&str> = nl.checks.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, ["assume", "assume", "assert", "assert"]);
    // Each else-branch check is enabled when the if's condition is false.
    for pair in nl.checks.chunks(2) {
        assert_ne!(pair[0].enable, pair[1].enable);
    }
}

// ============================================================================
// Formal attributes (* anyconst *) / (* anyseq *)
// ============================================================================

/// `(* anyconst *)` and `(* anyseq *)` on an undriven declaration mark its
/// undriven slice, so a formal back end can hold an anyconst value for the
/// whole trace (Yosys reads these attributes the same way).
#[test]
fn formal_free_attributes_mark_undriven_slices() {
    let nl = expect_lowered(lower_src(
        "module t(input wire clk);\n\
           (* anyconst *) wire [4:0] chk;\n\
           (* anyseq *) wire [3:0] d, e;\n\
           wire [1:0] plain;\n\
           reg [4:0] q;\n\
           always @(posedge clk) q <= chk ^ {1'b0, d} ^ {1'b0, e} ^ {3'b0, plain};\n\
         endmodule\n",
    ));
    let kind = |name: &str| nl.undriven.iter().find(|u| u.signal == name).map(|u| u.free);
    assert_eq!(kind("t.chk"), Some(sv_netlist::FreeKind::AnyConst));
    assert_eq!(kind("t.d"), Some(sv_netlist::FreeKind::AnySeq));
    assert_eq!(kind("t.e"), Some(sv_netlist::FreeKind::AnySeq));
    assert_eq!(kind("t.plain"), Some(sv_netlist::FreeKind::Undriven));
}

/// An attribute that says "free" on a signal something drives is a contradiction.
#[test]
fn formal_free_attribute_on_driven_signal_is_an_error() {
    let out = lower_src(
        "module t(input wire a);\n\
           (* anyconst *) wire c;\n\
           assign c = a;\n\
         endmodule\n",
    );
    assert!(matches!(out, LowerOutcome::Error(_)), "{out:?}");
}

// ============================================================================
// Cut points (`LowerConfig::cut_points`)
// ============================================================================

/// A cut point disconnects a signal from its drivers: its readers get a free
/// value (`FreeKind::Cut`), and the signals that drive it or share its driver
/// are untouched.
#[test]
fn cut_points_free_a_signal_for_its_readers_only() {
    let src = "module child(input wire [3:0] i, output wire [3:0] o);\n\
                 assign o = i + 4'd1;\n\
               endmodule\n\
               module t(input wire clk, input wire [3:0] d,\n\
                        output wire [3:0] y_q, y_w, y_alias, y_o);\n\
                 reg [3:0] q;\n\
                 always @(posedge clk) q <= d;\n\
                 wire [3:0] w = d ^ 4'h5;\n\
                 wire [3:0] alias_of_w = w;\n\
                 wire [3:0] o;\n\
                 child u(.i(d), .o(o));\n\
                 assign y_q = q;\n\
                 assign y_w = w;\n\
                 assign y_alias = alias_of_w;\n\
                 assign y_o = o;\n\
               endmodule\n";
    let cfg =
        LowerConfig { cut_points: vec!["t.q".into(), "t.alias_of_w".into(), "t.u.o".into()], ..LowerConfig::default() };
    let nl = expect_lowered(lower_src_cfg(src, &[], &cfg));
    let cut = |name: &str| {
        nl.undriven.iter().find(|u| u.signal == name && u.free == sv_netlist::FreeKind::Cut).map(|u| u.value)
    };
    let out = |name: &str| nl.outputs.iter().find(|o| o.name == name).map(|o| o.value).unwrap();
    // The register and the alias are cut: their readers see the free values.
    assert_eq!(Some(out("y_q")), cut("t.q"));
    assert_eq!(Some(out("y_alias")), cut("t.alias_of_w"));
    // `w`, which the alias merely copied, still has its own driver.
    assert_eq!(cut("t.w"), None);
    assert_ne!(Some(out("y_w")), cut("t.alias_of_w"));
    // A child's output port: the parent net it drives no longer follows the child.
    assert!(cut("t.u.o").is_some());
    let plain = expect_lowered(lower_src(src));
    assert!(plain.undriven.is_empty());
}

/// A cut point that names nothing is an error, not a silent no-op.
#[test]
fn cut_point_that_names_no_signal_is_an_error() {
    let cfg = LowerConfig { cut_points: vec!["t.typo".into()], ..LowerConfig::default() };
    let out = lower_src_cfg("module t(input wire a, output wire b);\n assign b = a;\nendmodule\n", &[], &cfg);
    match out {
        LowerOutcome::Error(ds) => assert!(ds[0].message.contains("cut point 't.typo'"), "{ds:?}"),
        other => panic!("expected an error, got {other:?}"),
    }
}

// ============================================================================
// Black boxes (`LowerConfig::black_box_modules`, `black_box_instances`)
// ============================================================================

const BLACK_BOX_SRC: &str = "module leaf(input wire clk, input wire [3:0] i, output reg [3:0] o);\n\
                               reg [3:0] mem [0:7];\n\
                               always @(posedge clk) begin mem[i[2:0]] <= i; o <= mem[i[2:0]] + 4'd1; end\n\
                             endmodule\n\
                             module mid(input wire clk, input wire [3:0] i, output wire [3:0] o);\n\
                               wire [3:0] t;\n\
                               leaf l(.clk(clk), .i(i), .o(t));\n\
                               assign o = ~t;\n\
                             endmodule\n\
                             module t(input wire clk, input wire [3:0] d, output wire [3:0] y1, y2);\n\
                               mid a(.clk(clk), .i(d), .o(y1));\n\
                               mid b(.clk(clk), .i(d), .o(y2));\n\
                             endmodule\n";

/// A black-boxed module keeps its ports and loses its body: no registers or
/// memories from inside it, and its outputs are free values.
#[test]
fn black_box_by_module_drops_the_body_and_frees_the_outputs() {
    let plain = expect_lowered(lower_src(BLACK_BOX_SRC));
    assert_eq!(plain.states.len() + plain.memories.len(), 4, "two leaves: a register and a memory each");

    let cfg = LowerConfig { black_box_modules: vec!["leaf".into()], ..LowerConfig::default() };
    let nl = expect_lowered(lower_src_cfg(BLACK_BOX_SRC, &[], &cfg));
    assert!(nl.states.is_empty() && nl.memories.is_empty());
    let cut: Vec<&str> =
        nl.undriven.iter().filter(|u| u.free == sv_netlist::FreeKind::Cut).map(|u| u.signal.as_str()).collect();
    assert_eq!(cut, ["t.a.l.o", "t.b.l.o"]);
    // Nothing inside the box is a signal of the netlist; its ports are.
    assert!(!nl.traces.iter().any(|s| s.name.starts_with("t.a.l.mem")));
    assert!(nl.traces.iter().any(|s| s.name == "t.a.l.i"));
}

/// By instance path: only that instance, and everything below it.
#[test]
fn black_box_by_instance_path_takes_the_subtree() {
    let cfg = LowerConfig { black_box_instances: vec!["t.a".into()], ..LowerConfig::default() };
    let nl = expect_lowered(lower_src_cfg(BLACK_BOX_SRC, &[], &cfg));
    // `t.b` and its leaf are intact; `t.a` is a box, its leaf is gone with it.
    assert_eq!(nl.states.len() + nl.memories.len(), 2);
    let cut: Vec<&str> =
        nl.undriven.iter().filter(|u| u.free == sv_netlist::FreeKind::Cut).map(|u| u.signal.as_str()).collect();
    assert_eq!(cut, ["t.a.o"]);
    assert!(!nl.traces.iter().any(|s| s.name.starts_with("t.a.l.") || s.name == "t.a.t"));
}

/// A black box that names nothing, or the top module, is an error.
#[test]
fn black_box_names_are_checked() {
    for (cfg, want) in [
        (
            LowerConfig { black_box_modules: vec!["typo".into()], ..LowerConfig::default() },
            "no instance of that module",
        ),
        (LowerConfig { black_box_instances: vec!["t.c".into()], ..LowerConfig::default() }, "is not an instance"),
        (LowerConfig { black_box_modules: vec!["t".into()], ..LowerConfig::default() }, "cannot be a black box"),
    ] {
        match lower_src_cfg(BLACK_BOX_SRC, &[], &cfg) {
            LowerOutcome::Error(ds) => assert!(ds[0].message.contains(want), "{ds:?}"),
            other => panic!("expected an error, got {other:?}"),
        }
    }
}

/// A module-level `integer k` used as the loop index of two clocked `always`
/// blocks: nothing reads `k` outside the loops, so which block wrote it last is
/// unobservable and the design lowers. Reading it elsewhere keeps the error.
#[test]
fn shared_loop_index_written_by_two_blocks() {
    let src = |extra: &str| {
        format!(
            "module t(input logic clk, input logic [7:0] d, output logic [7:0] a, b{extra});\n\
               integer k;\n\
               always @(posedge clk) for (k = 0; k < 8; k = k + 1) a[k] <= d[k];\n\
               always @(posedge clk) for (k = 0; k < 8; k = k + 1) b[k] <= d[7 - k];\n\
             {}endmodule\n",
            if extra.is_empty() { "" } else { "  assign o = k[0];\n" }
        )
    };
    expect_lowered(lower_src(&src("")));
    assert!(matches!(lower_src(&src(", output logic o")), LowerOutcome::Error(_)));
}

#[test]
fn phase3_dynamic_sim_streaming_udp_gates_and_checkers() {
    let src = r#"
primitive mux_udp (out, sel, a, b);
    output out;
    input sel, a, b;
    table
        0 0 ? : 0;
        0 1 ? : 1;
        1 ? 0 : 0;
        1 ? 1 : 1;
    endtable
endprimitive

primitive dff_udp (q, clk, d);
    output reg q;
    input clk, d;
    initial q = 0;
    table
        (01) 0 : ? : 0;
        (01) 1 : ? : 1;
        (0?) ? : ? : -;
        ?   (??) : ? : -;
    endtable
endprimitive

checker delay_chk (logic req, logic ack, event clk = $inferred_clock);
    default clocking @clk; endclocking
    bit q_req;
    always_ff @clk q_req <= req;
    a_ok: assert property (q_req |-> ack);
endchecker

module top (
    input  logic       clk,
    input  logic       sel,
    input  logic       din,
    input  logic [7:0] s_in,
    output logic       mux_y,
    output logic       dff_q,
    output logic       tri_y,
    output logic       pull_hi,
    output logic [3:0] arr_and,
    output logic [7:0] rt_3bit,
    output logic [3:0] unp_hi,
    output logic [3:0] unp_lo
);
    default clocking @(posedge clk); endclocking
    wire float_hi;
    pullup (float_hi);
    assign pull_hi = float_hi;

    dff_udp u_dff (dff_q, clk, din);
    mux_udp u_mux (mux_y, sel, din, dff_q);
    bufif1  u_tri (tri_y, din, sel);
    and     u_arr [3:0] (arr_and, s_in[3:0], s_in[7:4]);

    wire [7:0] p3 = {<< 3 {s_in}};
    assign {<< 3 {rt_3bit}} = p3;
    always_comb begin
        {<< 4 {unp_hi, unp_lo}} = s_in;
    end

    delay_chk u_chk (din, dff_q);
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("sel", 0);
    sim.set_u("din", 1);
    sim.set_u("s_in", 0xA5);
    assert_eq!(sim.out_i("pull_hi"), 1);
    assert_eq!(sim.out_i("dff_q"), 0, "initial q = 0 on sequential UDP");
    assert_eq!(sim.out_i("mux_y"), 1, "sel=0 selects din=1");
    assert_eq!(sim.out_cval("tri_y").bit_state(0), sv_semantic::BitState::Z, "bufif1 disabled drives 1'bz");
    assert_eq!(sim.out_i("arr_and"), 0xA & 0x5);
    assert_eq!(sim.out_i("rt_3bit"), 0xA5, "3-bit streaming pack + unpack is exact identity");
    assert_eq!(sim.out_i("unp_hi"), 0x5);
    assert_eq!(sim.out_i("unp_lo"), 0xA);

    // Clock posedge loads din=1 into dff_udp!
    sim.tick_posedge("clk");
    assert_eq!(sim.out_i("dff_q"), 1);
    sim.set_u("sel", 1);
    assert_eq!(sim.out_i("tri_y"), 1, "bufif1 enabled drives din=1");
    assert_eq!(sim.out_i("mux_y"), 1, "sel=1 selects dff_q=1");
}

#[test]
fn sourceless_black_box_infers_port_directions_and_checks_errors() {
    let src = "\
module top(
    input logic clk,
    input logic [7:0] a,
    output logic [7:0] y,
    output logic [7:0] bus
);
    logic [7:0] mid;
    assign bus[3:0] = a[3:0];
    missing_ip #(.W(8)) u_ip (
        .clk,
        .in_expr(a + 8'd1),
        .mid_out(mid),
        .bus_hi(bus[7:4])
    );
    missing_consumer u_cons (
        .clk(clk),
        .in_mid(mid),
        .out_y(y)
    );
endmodule
";
    // Without `-bbox_m`, instantiating `missing_ip` is an error.
    assert!(matches!(lower_src(src), LowerOutcome::Error(_)));

    let cfg = LowerConfig {
        black_box_modules: vec!["missing_ip".into(), "missing_consumer".into()],
        ..LowerConfig::default()
    };
    let nl = expect_lowered(lower_src_cfg(src, &[], &cfg));
    let cuts: Vec<&str> =
        nl.undriven.iter().filter(|u| u.free == sv_netlist::FreeKind::Cut).map(|u| u.signal.as_str()).collect();
    assert_eq!(cuts, ["top.mid", "top.bus[7:4]", "top.y"]);

    // Undeclared identifier inside a source-less black-box port expression fails closed!
    let bad_src = "module top(input logic clk); missing_ip u(.clk, .bad(no_such_sig)); endmodule\n";
    let bad_cfg = LowerConfig { black_box_modules: vec!["missing_ip".into()], ..LowerConfig::default() };
    match lower_src_cfg(bad_src, &[], &bad_cfg) {
        LowerOutcome::Error(ds) => assert!(ds[0].message.contains("no_such_sig"), "{ds:?}"),
        other => panic!("expected undeclared identifier error, got {other:?}"),
    }

    // Wildcard `.*` on a source-less black-box fails closed as Unsupported.
    let wc_src = "module top(input logic clk); missing_ip u(.*); endmodule\n";
    assert!(matches!(lower_src_cfg(wc_src, &[], &bad_cfg), LowerOutcome::Unsupported(_)));
}

#[test]
fn complexity_auto_boxing_mul_arrays_and_formal_coverage_targets() {
    let src = "\
module top(
    input logic clk,
    input logic sel,
    input logic [15:0] a, b,
    input logic [3:0] c, d,
    input logic [5:0] addr,
    output logic [15:0] prod_wide,
    output logic [15:0] prod_const,
    output logic [3:0] prod_narrow,
    output logic [7:0] mem_big_q,
    output logic [7:0] mem_small_q,
    output logic reg_q
);
    logic [7:0] big_mem [0:63];
    logic [7:0] small_mem [0:3];
    always_ff @(posedge clk) begin
        big_mem[addr] <= a[7:0];
        small_mem[addr[1:0]] <= a[7:0];
        if (sel)
            reg_q <= 1'b1;
        else
            reg_q <= 1'b0;
    end
    assign prod_wide = a * b;
    assign prod_const = a * 16'd3;
    assign prod_narrow = c * d;
    assign mem_big_q = big_mem[addr];
    assign mem_small_q = small_mem[addr[1:0]];
endmodule
";
    let cfg = LowerConfig {
        bbox_mul_width: Some(16),
        bbox_array_words: Some(32),
        blast_memories_up_to: Some(16),
        ..LowerConfig::default()
    };
    let mut nl = expect_lowered(lower_src_cfg(src, &[], &cfg));
    // `big_mem` (64 words >= 32) was boxed; `small_mem` (4 words <= 16) was blasted to 4 states + `reg_q` = 5 states.
    assert!(nl.memories.is_empty());
    assert_eq!(nl.states.len(), 5);
    // `a * b` (16-bit variable*variable) was boxed, while `a * 16'd3` and 4-bit `c * d` remained intact!
    let mul_ops = nl
        .values
        .iter()
        .filter(|v| matches!(v.kind, sv_netlist::ValueKind::Op { op: sv_netlist::NetOp::Mul, .. }))
        .count();
    assert_eq!(mul_ops, 2, "only constant-stride 16-bit mul and narrow 4-bit mul remain");
    assert_eq!(
        nl.undriven.iter().filter(|u| u.free == sv_netlist::FreeKind::Cut).count(),
        2,
        "one cut for wide mul and one cut for 64-word memory read"
    );

    let branch_covs = nl.add_branch_covers(1);
    assert!(branch_covs >= 2, "at least one true/false branch cover pair synthesized");
    let toggle_covs = nl.add_toggle_covers(1);
    assert!(toggle_covs >= 2, "rise/fall toggle covers synthesized on registers");
}

#[test]
fn unrolled_loop_task_concurrent_sva_and_external_prop_user_functions() {
    let src = r#"
package pkt_pkg;
  function automatic logic is_valid_pair(input logic [3:0] hdr, input logic [3:0] mask);
    logic [3:0] masked;
    masked = hdr & mask;
    return ^masked;
  endfunction
endpackage

module lane_chk (
  input  logic       clk,
  input  logic       rst_n,
  input  logic [2:0] en,
  input  logic [2:0] req,
  input  logic [2:0] gnt,
  input  logic [3:0] hdr,
  input  logic [3:0] mask,
  output logic       parity_q
);
  import pkt_pkg::*;

  function automatic logic local_check(input logic [3:0] h, input logic [3:0] m);
    return is_valid_pair(h, m);
  endfunction

  task automatic check_one_lane(input int idx, input logic gate);
    if (gate)
      t_chk: assert property (@(posedge clk) disable iff (!rst_n) req[idx] |-> ##1 gnt[idx]);
  endtask

  always_ff @(posedge clk or negedge rst_n) begin
    if (!rst_n) begin
      parity_q <= 1'b0;
    end else begin
      parity_q <= local_check(hdr, mask);
      for (int i = 0; i < 3; i++) begin
        if (en[i])
          a_loop: assert property (disable iff (!rst_n) req[i] |-> ##1 gnt[i]);
      end
      check_one_lane(0, en[0]);
      check_one_lane(2, en[2]);
    end
  end
endmodule
"#;
    let tree = sv_syntax::parse_source(src, "loop_task_sva.sv", sv_syntax::Options::default());
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let trees = vec![tree];
    let elab = sv_semantic::elaborate_with_options(&trees, &["lane_chk".to_string()], &[], &[]);
    assert!(elab.diags.is_empty(), "{:?}", elab.diags);
    let mut nl = expect_lowered(sv_netlist::lower_design(&trees, &elab.design, &LowerConfig::default()));

    // 3 unrolled `for` iterations + 2 task calls = 5 concurrent properties, each gated by `rst_n && en[i]`.
    assert_eq!(nl.properties.len(), 5);
    assert_eq!(
        nl.property_names,
        ["lane_chk.a_loop", "lane_chk.a_loop_2", "lane_chk.a_loop_3", "lane_chk.t_chk", "lane_chk.t_chk_2"]
    );
    assert!(nl.property_enables.iter().all(|e| e.is_some()), "all 5 unrolled properties keep reachability enables");
    let outcomes = nl.compile_properties(&sv_netlist::PropertyOptions::default());
    assert!(outcomes.iter().all(|o| matches!(o, sv_netlist::PropertyOutcome::Compiled { .. })), "{outcomes:?}");

    // External CLI property calling both module function `local_check` and package function `pkt_pkg::is_valid_pair`:
    let ext_src =
        "module __ext; assert property (@(posedge clk) disable iff (!rst_n) (local_check(hdr, mask) |=> parity_q == $past(pkt_pkg::is_valid_pair(hdr, mask)))); endmodule";
    let ext_tree = sv_syntax::parse_source(ext_src, "<ext>", sv_syntax::Options::default());
    let ca_node =
        ext_tree.preorder().into_iter().find(|&n| ext_tree.kind(n) == sv_syntax::SyntaxKind::ConcurrentAssert).unwrap();
    let mut ext_arena = sv_semantic::ExprArena::new();
    let mut binder = sv_semantic::SvaBinder::new(&trees, &elab.design, &mut ext_arena).with_extra_tree(&ext_tree);
    let top_id = elab.design.tops[0];
    let ext_prop = binder.bind_concurrent_assert(trees.len(), ca_node, top_id, "", &[], None).unwrap();
    assert!(binder.diags.is_empty(), "{:?}", binder.diags);

    // Without `trees`, `lower_external_property` still rejects non-constant user functions cleanly;
    // with `trees`, `compile_external_properties_with_trees` inlines both functions!
    assert!(nl.lower_external_property(&elab.design, &ext_arena, &ext_prop, false).is_err());
    let ext_res = nl.compile_external_properties_with_trees(
        &trees,
        &elab.design,
        &[sv_netlist::ExternalProperty {
            arena: &ext_arena,
            property: &ext_prop,
            name: "lane_chk.ext_fn_prop".to_string(),
        }],
        &sv_netlist::PropertyOptions::default(),
    );
    assert!(matches!(ext_res.as_slice(), [Ok(sv_netlist::Compiled::Check(_))]), "{ext_res:?}");
}

#[test]
fn bitwise_op_of_shifted_vectors_is_not_a_loop() {
    // Vectorized code (CIRCT's output for a leading-zero counter): `g0` feeds only bit 0 of
    // `v` and reads only bit 1 of it, which holds `g`. No bit depends on itself.
    let src = r#"
module lzc (input logic [2:0] in_i, output logic [1:0] cnt_o, output logic empty_o);
  wire g; wire g0;
  wire [1:0] v = {g, 1'h0} | {1'h0, g0};
  assign g0 = v[1] | in_i[2];
  assign g = in_i[0] | in_i[1];
  assign cnt_o = {~v[1], v[1] & ~in_i[0]};
  assign empty_o = ~v[0];
endmodule
"#;
    let nl = expect_lowered(lower_src(src));
    let mut sim = NetlistSim::new(&nl);
    for x in 0i128..8 {
        sim.set_u("in_i", x);
        let g = (x & 1) | ((x >> 1) & 1);
        let g0 = g | ((x >> 2) & 1);
        assert_eq!(sim.out_i("empty_o"), 1 - g0, "in_i={x}");
        assert_eq!(sim.out_i("cnt_o"), ((1 - g) << 1) | (g & (1 - (x & 1))), "in_i={x}");
    }
}

#[test]
fn bitwise_op_real_loop_is_still_a_loop() {
    // A real loop through a bitwise operation is still one.
    let real = "module m (input logic a, output logic [1:0] y); assign y = {y[0], a} | {1'b0, y[1]}; endmodule";
    assert!(matches!(lower_src(real), LowerOutcome::Error(_)), "y[0] <- y[1] <- y[0] is a loop");
}

#[test]
fn shift_chain_through_alias_is_not_a_loop() {
    // Quartus lpm_divide: a vector built from its own lower bits through an alias.
    let a = "module m (input logic [7:0] d, output logic [15:0] y);\n  wire [15:0] D, Dt;\n  assign D = Dt, Dt = {D[7:0], d};\n  assign y = D;\nendmodule\n";
    let nl = expect_lowered(lower_src(a));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("d", 0x5a);
    assert_eq!(sim.out_i("y"), 0x5a5a);
}

#[test]
fn or_chain_inside_one_vector_is_not_a_loop() {
    // Each bit ORs the bit below it: a prefix OR kept in one vector.
    let a = "module m (input logic [3:0] d, output logic [3:0] y);\n  wire [3:0] s;\n  assign s = {(s[2] | d[3]), (s[1] | d[2]), (s[0] | d[1]), d[0]};\n  assign y = s;\nendmodule\n";
    let nl = expect_lowered(lower_src(a));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("d", 2);
    assert_eq!(sim.out_i("y"), 0xe);
}

#[test]
fn long_shift_chain_through_alias_is_not_a_loop() {
    // Nine stages of eight bits, as in a Quartus lpm_divide: bit k+8 is bit k, down to `n`.
    let a = "module m (input logic [7:0] n, output logic [71:0] y);\n  wire [71:0] A, T;\n  assign A = T, T = {A[63:0], n};\n  assign y = A;\nendmodule\n";
    let nl = expect_lowered(lower_src(a));
    let mut sim = NetlistSim::new(&nl);
    sim.set_u("n", 0xa5);
    assert_eq!(sim.out_i("y"), 0x00a5_a5a5_a5a5_a5a5_a5a5);
    // A bit that names itself is still a loop.
    let real = "module m (input logic [7:0] n, output logic [15:0] y);\n  wire [15:0] A, T;\n  assign A = T, T = {A[15:8], n};\n  assign y = A;\nendmodule\n";
    assert!(matches!(lower_src(real), LowerOutcome::Error(_)), "bits 15:8 of T are themselves");
}
