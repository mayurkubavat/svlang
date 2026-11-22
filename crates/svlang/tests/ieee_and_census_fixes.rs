//! Integration tests for IEEE 1800-2023 LRM conformance (`testdata/conformance/lrm/*.sv`)
//! and Phase 1 compiler / lowering bug fixes (`B1`–`B12`).

use std::path::Path;

use svlang::{Compilation, DiagnosticStage, Driver, LowerConfig, LowerOutcome};

#[test]
fn lrm_conformance_files_elaborate_through_native_svlang() {
    let lrm_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/lrm");
    let mut entries: Vec<_> = std::fs::read_dir(&lrm_dir)
        .expect("testdata/conformance/lrm must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sv"))
        .collect();
    entries.sort();
    assert_eq!(entries.len(), 23, "expected 23 LRM conformance files");

    for path in entries {
        let name = path.file_name().unwrap().to_str().unwrap();
        let comp = Driver::new().add_file(&path).compile().expect("LRM file must compile");
        let (diags, _elab) = comp.check();
        assert!(diags.is_empty(), "expected 0 diagnostics on LRM file {name}, got {diags:?}");
    }
}

#[test]
fn b1_time_literal_in_parameter_constant_evaluation_and_lowering() {
    let src = r#"
`timescale 1ns/1ps
module top #(
    parameter int unsigned Delay = 300ps
) (
    input  logic        clk,
    output logic [31:0] out_delay
);
    assign out_delay = Delay;
endmodule
"#;
    let comp = Compilation::from_source("b1_time_param.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            let dump = nl.dump();
            assert!(dump.contains("netlist top"), "dump:\n{dump}");
        }
        other => panic!("expected Lowered, got {other:?}"),
    }
}

#[test]
fn b2_dollar_bits_on_unpacked_typedef_struct_and_indexed_array() {
    let src = r#"
module top (
    input  logic [7:0] in_byte,
    output logic [31:0] out_w
);
    typedef logic [7:0] unpacked_bytes_t [4];
    typedef struct {
        logic [7:0] a;
        logic [15:0] b;
    } unpacked_pair_t;

    unpacked_bytes_t arr_sig;
    localparam int TOTAL_ARR = $bits(unpacked_bytes_t);
    localparam int TOTAL_STRUCT = $bits(unpacked_pair_t);
    localparam int ELEM_BITS = $bits(arr_sig[0]);

    logic [ELEM_BITS-1:0] elem_copy;
    assign arr_sig[0] = in_byte;
    assign elem_copy = arr_sig[0];
    assign out_w = TOTAL_ARR + TOTAL_STRUCT + ELEM_BITS + {24'd0, elem_copy};
endmodule
"#;
    let comp = Compilation::from_source("b2_dollar_bits.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            let dump = nl.dump();
            assert!(dump.contains("elem_copy"), "dump:\n{dump}");
        }
        other => panic!("expected Lowered, got {other:?}"),
    }
}

#[test]
fn b3_unbased_unsized_literal_in_concatenation_binds_as_one_bit() {
    let src = r#"
module top (
    input  logic [5:0] payload,
    output logic [7:0] out_vec
);
    assign out_vec = {'0, '1, payload};
endmodule
"#;
    let comp = Compilation::from_source("b3_concat_fill.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            assert_eq!(nl.top, "top");
        }
        other => panic!("expected Lowered, got {other:?}"),
    }
}

#[test]
fn b4_assignment_patterns_in_ternary_replication_type_keys_and_index_keys() {
    let src = r#"
module top (
    input  logic       sel,
    input  logic [3:0] a,
    input  logic [3:0] b,
    output logic [7:0] y_ternary,
    output logic [7:0] y_sibling,
    output logic [7:0] y_repl,
    output logic [7:0] y_idx,
    output logic [8:0] y_typekey
);
    typedef struct packed {
        logic [3:0] hi;
        logic [3:0] lo;
    } pair_t;

    typedef struct packed {
        int w;
        bit flag;
    } typed_s;

    pair_t p_tern, p_sib, p_base;
    logic [3:0] unpacked_rep [2];
    logic [3:0] unpacked_idx [2];
    typed_s ts;

    assign p_base = '{hi: a, lo: b};
    assign p_tern = sel ? ('{hi: a, lo: b}) : '{default: '0};
    assign p_sib  = (sel ? p_base : '{hi: b, lo: a});
    assign unpacked_rep = '{2{a}};
    assign unpacked_idx = '{0: a, 1: b};
    assign ts = '{int: 32'd7, bit: 1'b1};

    assign y_ternary = p_tern;
    assign y_sibling = p_sib;
    assign y_repl    = {unpacked_rep[0], unpacked_rep[1]};
    assign y_idx     = {unpacked_idx[0], unpacked_idx[1]};
    assign y_typekey = {ts.w[7:0], ts.flag};
endmodule
"#;
    let comp = Compilation::from_source("b4_patterns.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            assert_eq!(nl.top, "top");
        }
        other => panic!("expected Lowered, got {other:?}"),
    }
}

#[test]
fn b5_duplicate_package_typedef_does_not_cause_false_missing_member_error() {
    let comp = Driver::new()
        .add_source(
            "pkg_a.sv",
            "package pkg_a;\n  typedef struct packed { logic [3:0] x; } shared_t;\nendpackage\n",
        )
        .add_source(
            "pkg_b.sv",
            "package pkg_b;\n  typedef struct packed { logic [7:0] y; } shared_t;\nendpackage\n",
        )
        .add_source(
            "top.sv",
            "module top(output logic [7:0] out_y);\n  import pkg_b::*;\n  shared_t s;\n  assign s.y = 8'hA5;\n  assign out_y = s.y;\nendmodule\n",
        )
        .top("top")
        .compile()
        .unwrap();
    let diags = comp.diagnostics();
    assert!(diags.is_empty(), "expected 0 diagnostics when resolving imported pkg_b::shared_t, got {diags:?}");
}

#[test]
fn b6_elaboration_system_tasks_fire_only_in_active_elaborated_scopes() {
    let good_src = r#"
module top #(parameter int W = 8) (input logic clk);
    if (W < 4) begin : bad_cfg
        $error("W (%0d) must be >= 4", W);
    end
endmodule
"#;
    let good = Compilation::from_source("b6_good.sv", good_src);
    assert!(good.diagnostics().is_empty(), "inactive generate $error must not fire: {:?}", good.diagnostics());

    let bad = Driver::new().add_source("b6_bad.sv", good_src).param("W", "2").top("top").compile().unwrap();
    let diags = bad.diagnostics();
    assert_eq!(diags.len(), 1, "expected active $error diagnostic, got {diags:?}");
    assert!(diags[0].message.contains("$error encountered"), "got: {:?}", diags[0]);
}

#[test]
fn b7_duplicate_package_and_same_file_module_redefinitions_are_diagnosed() {
    let dup_pkg = Compilation::from_source("b7_pkg.sv", "package p; endpackage\npackage p; endpackage\n");
    assert!(
        dup_pkg.diagnostics().iter().any(|d| d.message.contains("redefinition of 'p'")),
        "expected package redefinition error, got {:?}",
        dup_pkg.diagnostics()
    );

    let dup_mod = Compilation::from_source("b7_mod.sv", "module m; endmodule\nmodule m; endmodule\n");
    assert!(
        dup_mod.diagnostics().iter().any(|d| d.message.contains("redefinition of 'm'")),
        "expected module redefinition error, got {:?}",
        dup_mod.diagnostics()
    );

    let extern_mod =
        Compilation::from_source("b7_ext.sv", "extern module m(input logic a);\nmodule m(input logic a); endmodule\n");
    assert!(
        extern_mod.diagnostics().is_empty(),
        "extern prototype + definition must pass: {:?}",
        extern_mod.diagnostics()
    );
}

#[test]
fn b8_separate_units_mode_isolates_preprocessor_macros_across_files() {
    let f1 = "`define LEAKED_VAL 8'h5A\nmodule m1(output logic [7:0] y); assign y = `LEAKED_VAL; endmodule\n";
    let f2 = "module m2(output logic [7:0] y);\n`ifdef LEAKED_VAL\n  assign y = `LEAKED_VAL;\n`else\n  assign y = 8'h00;\n`endif\nendmodule\n";

    // Default simulator mode carries macros across files.
    let shared = Driver::new().add_source("f1.sv", f1).add_source("f2.sv", f2).top("m2").compile().unwrap();
    assert!(shared.diagnostics().is_empty());
    let nl_shared = match shared.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => nl.dump(),
        other => panic!("expected Lowered, got {other:?}"),
    };

    // Strict IEEE 1800 §3.12.1 separate-units mode isolates macros per file.
    let isolated =
        Driver::new().separate_units(true).add_source("f1.sv", f1).add_source("f2.sv", f2).top("m2").compile().unwrap();
    assert!(isolated.diagnostics().is_empty());
    let nl_isolated = match isolated.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => nl.dump(),
        other => panic!("expected Lowered, got {other:?}"),
    };

    assert_ne!(nl_shared, nl_isolated, "separate_units(true) must not leak `define LEAKED_VAL into f2.sv");
    assert!(nl_isolated.contains("const(8'b00000000)"), "isolated dump:\n{nl_isolated}");
}

#[test]
fn b9_ieee2023_soft_packed_unions_pass_and_illegal_soft_forms_fail() {
    let soft_ok = r#"
module top (
    input  logic [7:0] in_b,
    output logic [15:0] out_w
);
    typedef union soft packed {
        logic [15:0] wide;
        logic [7:0]  narrow;
    } soft_u;

    soft_u u;
    always_comb begin
        u = '0;
        u.narrow = in_b;
    end
    assign out_w = u.wide;
endmodule
"#;
    let comp = Compilation::from_source("b9_soft_ok.sv", soft_ok);
    assert!(comp.diagnostics().is_empty(), "union soft packed must pass: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => assert_eq!(nl.top, "top"),
        other => panic!("expected Lowered, got {other:?}"),
    }

    // Hard packed union with mismatched widths must fail.
    let hard_bad = "module m; typedef union packed { logic [15:0] a; logic [7:0] b; } u_t; endmodule\n";
    let comp_hard = Compilation::from_source("b9_hard_bad.sv", hard_bad);
    assert!(!comp_hard.diagnostics().is_empty(), "non-soft union packed with unequal widths must fail");

    // `struct soft packed` and `union tagged soft packed` must fail in parser.
    let struct_soft =
        Compilation::from_source("b9_struct_soft.sv", "module m; struct soft packed { logic a; } s; endmodule\n");
    assert!(!struct_soft.diagnostics().is_empty(), "struct soft packed must be rejected");

    let tagged_soft =
        Compilation::from_source("b9_tagged_soft.sv", "module m; union tagged soft packed { void a; } u; endmodule\n");
    assert!(!tagged_soft.diagnostics().is_empty(), "union tagged soft packed must be rejected");
}

#[test]
fn b10_ieee2023_type_parameter_restrictions_in_headers_and_bodies() {
    let ok_src = r#"
module sub #(parameter type struct S = struct packed { logic a; }) ();
    typedef enum logic [1:0] { A = 0, B = 1 } e_t;
    localparam type enum E = e_t;
endmodule
module top;
    typedef struct packed { logic [3:0] x; } my_s;
    sub #(.S(my_s)) u_sub ();
endmodule
"#;
    let ok = Compilation::from_source("b10_ok.sv", ok_src);
    assert!(ok.diagnostics().is_empty(), "valid restricted type params must pass: {:?}", ok.diagnostics());

    let bad_src = r#"
module sub #(parameter type struct S = int) ();
endmodule
"#;
    let bad = Compilation::from_source("b10_bad.sv", bad_src);
    assert!(
        bad.diagnostics().iter().any(|d| d.message.contains("requires a struct type")),
        "expected type restriction diagnostic, got {:?}",
        bad.diagnostics()
    );
}

#[test]
fn b11_nested_design_element_default_nettype_and_ifdef_expr_diagnostics() {
    let nested_bad = r#"
module outer;
    module inner;
    endmodule
    `default_nettype none
endmodule
"#;
    let comp_nested = Compilation::from_source("b11_nested.sv", nested_bad);
    assert!(
        comp_nested.diagnostics().iter().any(|d| d.stage == DiagnosticStage::Preprocessor),
        "expected preprocessor error for `default_nettype inside outer after inner endmodule, got {:?}",
        comp_nested.diagnostics()
    );

    let bad_ifdef_1 = Compilation::from_source("b11_ifdef1.sv", "`ifdef (A && )\n`endif\nmodule m; endmodule\n");
    assert!(
        bad_ifdef_1.diagnostics().iter().any(|d| d.stage == DiagnosticStage::Preprocessor),
        "expected diagnostic on `ifdef (A && ), got {:?}",
        bad_ifdef_1.diagnostics()
    );

    let bad_ifdef_2 = Compilation::from_source("b11_ifdef2.sv", "`ifdef (A B)\n`endif\nmodule m; endmodule\n");
    assert!(
        bad_ifdef_2.diagnostics().iter().any(|d| d.stage == DiagnosticStage::Preprocessor),
        "expected diagnostic on `ifdef (A B), got {:?}",
        bad_ifdef_2.diagnostics()
    );
}

#[test]
fn b12_netlist_lowering_async_ternary_ff_unpacked_const_param_and_initial_const_case() {
    let src = r#"
module top #(
    parameter int SimInit = 1
) (
    input  logic       clk,
    input  logic       rst_n,
    input  logic [1:0] idx,
    input  logic [7:0] d,
    output logic [7:0] q_tern,
    output logic [7:0] lut_val,
    output logic [7:0] init_cfg
);
    localparam logic [7:0] ConstLut [4] = '{8'h11, 8'h22, 8'h33, 8'h44};

    // Async-reset ternary flop idiom
    always_ff @(posedge clk or negedge rst_n) begin
        q_tern <= !rst_n ? 8'h00 : d;
    end

    // Dynamic index into unpacked array parameter
    assign lut_val = ConstLut[idx];

    // Constant case inside initial block
    initial begin
        case (SimInit)
            0: init_cfg = 8'hAA;
            1: init_cfg = 8'h55;
            default: init_cfg = 8'h00;
        endcase
    end
endmodule
"#;
    let comp = Compilation::from_source("b12_lower.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            let dump = nl.dump();
            assert!(dump.contains("q_tern"), "dump:\n{dump}");
            assert!(dump.contains("lut_val"), "dump:\n{dump}");
        }
        other => panic!("expected Lowered for B12 patterns, got {other:?}"),
    }

    // Non-constant case inside initial block must remain fail-closed (Unsupported)
    let nonconst_init = r#"
module bad_init (input logic sel, output logic y);
    initial begin
        case (sel)
            1'b0: y = 1'b0;
            1'b1: y = 1'b1;
        endcase
    end
endmodule
"#;
    let comp_bad = Compilation::from_source("b12_bad_init.sv", nonconst_init);
    assert!(
        matches!(comp_bad.lower_netlist(&LowerConfig::default()), LowerOutcome::Unsupported(_)),
        "non-constant case in initial block must report Unsupported"
    );
}

#[test]
fn phase3_streaming_concatenations_pack_unpack_and_const_eval() {
    let src = r#"
module top (
    output logic [7:0]  rev_bits,
    output logic [15:0] swap_bytes,
    output logic [15:0] wide_left_aligned,
    output logic [7:0]  roundtrip_3bit,
    output logic [3:0]  unp_hi,
    output logic [3:0]  unp_lo,
    output logic [7:0]  proc_hi,
    output logic [7:0]  proc_lo
);
    typedef logic [15:0] u16_t;
    localparam [7:0]  in_a = 8'b11_101_001;
    localparam [7:0]  in_b = 8'hAB;
    localparam [7:0]  P_BITS = {<< {8'b0011_0101}};
    localparam [7:0]  P_NIB  = {<< 4 {8'hAB}};
    localparam [15:0] P_WIDE = {>> {8'hAB}};
    localparam [15:0] P_CAST = u16_t'({<< byte {16'hABCD}});
    localparam [7:0]  P_3BIT = {<< 3 {8'b11_101_001}};

    static_check: assert final (
        P_BITS == 8'hAC && P_NIB == 8'hBA && P_WIDE == 16'hAB00 && P_CAST == 16'hCDAB && P_3BIT == 8'd55
    );

    assign rev_bits = {<< {in_a}};
    assign swap_bytes = {<< byte {in_a, in_b}};
    assign wide_left_aligned = {>> {in_a}};

    // Non-divisor slice size `3`: `{<< 3 {y}} = {<< 3 {x}}` must invert back to `x`!
    wire [7:0] packed_3 = {<< 3 {in_a}};
    assign {<< 3 {roundtrip_3bit}} = packed_3;

    // Continuous LHS nibble-reversed unpack
    assign {<< 4 {unp_hi, unp_lo}} = in_b;

    // Procedural unpacked-array & streaming LHS/RHS
    logic [7:0] arr [0:1];
    always_comb begin
        arr = {>> {in_a, in_b}};
        {<< 8 {proc_hi, proc_lo}} = {>> {arr with [0:1]}};
    end
endmodule
"#;
    let comp = Compilation::from_source("phase3_stream.sv", src);
    assert!(comp.diagnostics().is_empty(), "diags: {:?}", comp.diagnostics());
    let nl = match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => nl,
        other => panic!("expected Lowered for streaming concatenations, got {other:?}"),
    };
    fn eval_node_cval(nl: &svlang::Netlist, vid: svlang::netlist::ValueId) -> svlang::CVal {
        let vn = nl.value(vid);
        match &vn.kind {
            svlang::netlist::ValueKind::Const(cv) => cv.clone(),
            svlang::netlist::ValueKind::Op { op: svlang::netlist::NetOp::Slice { hi, lo }, args } => {
                let inner = eval_node_cval(nl, args[0]);
                inner.slice(*lo, (*hi - *lo) + 1, vn.signed)
            }
            svlang::netlist::ValueKind::Op { op: svlang::netlist::NetOp::Concat, args } => {
                let mut it = args.iter().map(|&a| eval_node_cval(nl, a));
                let first = it.next().unwrap();
                it.fold(first, |acc, v| svlang::CVal::concat(&acc, &v))
            }
            other => panic!("unexpected non-const node {other:?}"),
        }
    }
    let get_const_out = |name: &str| -> i128 {
        let out = nl.outputs.iter().find(|o| o.name == name).unwrap();
        eval_node_cval(&nl, out.value).i()
    };
    assert_eq!(get_const_out("rev_bits"), 0b1001_0111);
    assert_eq!(get_const_out("swap_bytes"), 0xABE9);
    assert_eq!(get_const_out("wide_left_aligned"), 0xE900);
    assert_eq!(get_const_out("roundtrip_3bit"), 0b11_101_001);
    assert_eq!(get_const_out("unp_hi"), 0xB);
    assert_eq!(get_const_out("unp_lo"), 0xA);
    assert_eq!(get_const_out("proc_hi"), 0xAB);
    assert_eq!(get_const_out("proc_lo"), 0xE9);

    // Negative: wider stream assigned to narrower target without cast, and mismatched bitstream cast width
    let bad = r#"
module bad_stream (input logic [15:0] x, output logic [7:0] y);
    typedef logic [15:0] u16_t;
    assign y = {>> {x}};
    wire [15:0] z = u16_t'({>> {8'h12}});
endmodule
"#;
    let comp_bad = Compilation::from_source("bad_stream.sv", bad);
    assert!(!matches!(comp_bad.lower_netlist(&LowerConfig::default()), LowerOutcome::Lowered(_)));
}

#[test]
fn phase3_checkers_ch17_and_free_rand_variables_lower() {
    let lrm_chk = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/lrm/ch17_checkers.sv");
    let comp_lrm = Driver::new().add_file(&lrm_chk).compile().expect("ch17_checkers.sv must compile");
    let (diags, _) = comp_lrm.check();
    assert!(diags.is_empty(), "ch17_checkers.sv diags: {diags:?}");
    match comp_lrm.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => {
            assert!(nl.states.iter().any(|s| s.name == "ch17_checkers.u_chk.seen"), "states: {:?}", nl.states);
            assert!(nl.property_names.iter().any(|n| n.contains("u_chk.a_ack")), "props: {:?}", nl.property_names);
        }
        other => panic!("expected Lowered on ch17_checkers.sv, got {other:?}"),
    }

    // Package checker, rand / const rand free checker vars, $anyseq/$anyconst, untyped & sequence ports, procedural & bound checkers
    let src = r#"
package chk_pkg;
    checker shadow_chk (
        logic [3:0] din,
        untyped valid_expr,
        event clk = $inferred_clock,
        logic rst = $inferred_disable
    );
        default clocking @clk; endclocking
        default disable iff (rst);
        rand bit [3:0] free_seq;
        const rand bit [3:0] free_const;
        bit [3:0] sampled_d;
        always_ff @clk sampled_d <= din;
        a_match: assert property (valid_expr |-> ##1 (sampled_d == $past(din) && free_const == $past(free_const)));
        c_free: cover property (free_seq == 4'hA);
    endchecker
endpackage

module dut (
    input  logic       clk,
    input  logic       rst,
    input  logic       vld,
    input  logic [3:0] data,
    output logic [3:0] any_out
);
    import chk_pkg::*;
    default clocking @(posedge clk); endclocking
    default disable iff (rst);

    assign any_out = $anyseq ^ $anyconst;

    shadow_chk u_struct (data, vld && (data != 4'h0));
    always_ff @(posedge clk) begin
        shadow_chk u_proc (data, vld);
    end
endmodule

bind dut chk_pkg::shadow_chk u_bound (data, vld);
"#;
    let comp = Compilation::from_source("phase3_checker.sv", src);
    assert!(comp.diagnostics().is_empty(), "checker diags: {:?}", comp.diagnostics());
    let nl = match comp.lower_netlist(&LowerConfig::default()) {
        LowerOutcome::Lowered(nl) => nl,
        other => panic!("expected Lowered for checker with rand/const rand/bind/procedural instances, got {other:?}"),
    };
    let has_free =
        |sig: &str, fk: svlang::netlist::FreeKind| nl.undriven.iter().any(|u| u.signal == sig && u.free == fk);
    assert!(has_free("dut.u_struct.free_seq", svlang::netlist::FreeKind::AnySeq));
    assert!(has_free("dut.u_struct.free_const", svlang::netlist::FreeKind::AnyConst));
    assert!(has_free("dut.u_bound.free_const", svlang::netlist::FreeKind::AnyConst));
    assert!(nl.property_names.iter().any(|p| p.contains("u_struct.a_match")));
    assert!(nl.property_names.iter().any(|p| p.contains("u_proc.a_match")));
    assert!(nl.property_names.iter().any(|p| p.contains("u_bound.a_match")));
}

#[test]
fn phase3_udps_gates_and_specify_ch28_31_lower_and_validate() {
    let lrm_udp =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/lrm/ch28_31_gates_udp_specify.sv");
    let comp_lrm = Driver::new().add_file(&lrm_udp).compile().expect("ch28_31_gates_udp_specify.sv must compile");
    let (diags, _) = comp_lrm.check();
    assert!(diags.is_empty(), "ch28_31_gates_udp_specify.sv diags: {diags:?}");
    let LowerOutcome::Lowered(nl) = comp_lrm.lower_netlist(&LowerConfig::default()) else {
        panic!("expected Lowered on ch28_31_gates_udp_specify.sv");
    };
    assert!(nl.states.iter().any(|s| s.name.contains("f1.q")), "expected sequential UDP state f1.q");
    assert_eq!(nl.outputs.len(), 2, "expected outputs y and q");

    // Negative specify block checks: parallel `=>` width mismatch and wire notifier
    let bad_spec = r#"
module bad_spec (input wire [3:0] a, input wire clk, output wire [7:0] y);
    wire bad_notif;
    assign y = {4'b0, a};
    specify
        (a => y) = 1;
        $setup(a, posedge clk, 1, bad_notif);
    endspecify
endmodule
"#;
    let comp_bad = Compilation::from_source("bad_spec.sv", bad_spec);
    let msgs: Vec<String> = comp_bad.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("parallel specify path '=>'")), "msgs: {msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("must be a variable")), "msgs: {msgs:?}");
}

#[test]
fn phase3_config_nettype_and_interconnect_elaboration() {
    let cfg_src = r#"
module impl_fast (input logic a, output logic y);
    assign y = a;
endmodule

module impl_inv (input logic a, output logic y);
    assign y = ~a;
endmodule

module top_mod (output logic y1, output logic y2);
    localparam logic a = 1'b1;
    impl_fast u1 (.a(a), .y(y1));
    impl_fast u2 (.a(a), .y(y2));
endmodule

config inv_u2_cfg;
    design work.top_mod;
    default liblist work;
    instance top_mod.u2 use work.impl_inv;
endconfig
"#;
    let comp =
        Driver::new().add_source("cfg.sv", cfg_src).top("inv_u2_cfg").compile().expect("config source must compile");
    let (diags, _) = comp.check();
    assert!(diags.is_empty(), "config diags: {diags:?}");
    let LowerOutcome::Lowered(nl) = comp.lower_netlist(&LowerConfig::default()) else {
        panic!("expected Lowered with config instance rebinding");
    };
    let out_const = |nl: &svlang::Netlist, name: &str| -> i128 {
        let out = nl.outputs.iter().find(|o| o.name == name).unwrap();
        match &nl.value(out.value).kind {
            svlang::netlist::ValueKind::Const(cv) => cv.i(),
            svlang::netlist::ValueKind::Op { op: svlang::netlist::NetOp::Not, args } => match &nl.value(args[0]).kind {
                svlang::netlist::ValueKind::Const(cv) => (!cv.i()) & ((1i128 << cv.w) - 1),
                other => panic!("expected Const under Not for {name}, got {other:?}"),
            },
            other => panic!("expected Const for {name}, got {other:?}"),
        }
    };
    assert_eq!(out_const(&nl, "y1"), 1);
    assert_eq!(out_const(&nl, "y2"), 0);

    // User-defined `nettype` with valid resolution function + typeless `interconnect` bus
    let nt_src = r#"
module sender (output logic [3:0] p);
    assign p = 4'hC;
endmodule

module receiver (input logic [3:0] p, output logic [3:0] out_p);
    assign out_p = p;
endmodule

module nt_top (output logic [7:0] q, output logic [3:0] ic_out);
    localparam logic [7:0] d = 8'h5A;
    typedef logic [7:0] byte_v_t;
    function automatic byte_v_t resolve_or (input byte_v_t drivers []);
        byte_v_t acc = '0;
        foreach (drivers[i]) acc |= drivers[i];
        return acc;
    endfunction
    nettype byte_v_t wor_byte_nt with resolve_or;

    wor_byte_nt n_bus;
    assign n_bus = d;
    assign q = n_bus;

    interconnect [3:0] ic_bus;
    sender u_s (.p(ic_bus));
    receiver u_r (.p(ic_bus), .out_p(ic_out));
endmodule
"#;
    let comp_nt = Compilation::from_source("nt_ic.sv", nt_src);
    assert!(comp_nt.diagnostics().is_empty(), "nettype/interconnect diags: {:?}", comp_nt.diagnostics());
    let LowerOutcome::Lowered(nl_nt) = comp_nt.lower_netlist(&LowerConfig::default()) else {
        panic!("expected Lowered for nettype + interconnect");
    };
    assert_eq!(out_const(&nl_nt, "q"), 0x5A);
    assert_eq!(out_const(&nl_nt, "ic_out"), 0xC);

    // Negative: `interconnect logic bad;` and `interconnect` read in `assign`, plus invalid `nettype` resolve fn
    let bad_nt = r#"
module bad_nt_ic (output logic y);
    function void bad_res(input int x); endfunction
    nettype int my_nt with bad_res;
    interconnect logic bad_decl;
    interconnect ic_sig;
    assign y = ic_sig;
endmodule
"#;
    let comp_bad = Compilation::from_source("bad_nt_ic.sv", bad_nt);
    let msgs: Vec<String> = comp_bad.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert!(msgs.iter().any(|m| m.contains("cannot be void")), "msgs: {msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("explicit data type")), "msgs: {msgs:?}");
    assert!(msgs.iter().any(|m| m.contains("interconnect net 'ic_sig'")), "msgs: {msgs:?}");
}

#[test]
fn ieee1800_2023_conformance_suite_positive_and_negative() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/conformance/ieee1800_2023");

    // 1. Positive suite: zero diagnostics, and RTL+SVA file lowers cleanly to `sv-netlist`.
    let mut pos_files: Vec<_> = std::fs::read_dir(base.join("positive"))
        .expect("positive dir must exist")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sv"))
        .collect();
    pos_files.sort();
    assert_eq!(pos_files.len(), 2);
    for path in &pos_files {
        let name = path.file_name().unwrap().to_str().unwrap();
        let comp = Driver::new().add_file(path).compile().expect("positive 2023 file must compile");
        let (diags, _) = comp.check();
        assert!(diags.is_empty(), "expected 0 diagnostics on {name}, got {diags:?}");
        if name == "types_pp_and_sva_2023.sv" {
            match comp.lower_netlist(&LowerConfig::default()) {
                LowerOutcome::Lowered(nl) => assert_eq!(nl.properties.len(), 5),
                other => panic!("expected Lowered on {name}, got {other:?}"),
            }
        }
    }

    // 2. Negative suite: verify all expected diagnostics fire cleanly.
    let oop_neg = Driver::new()
        .add_file(base.join("negative/oop_override_and_ctor_errors.sv"))
        .compile()
        .expect("oop negative file must parse");
    let (oop_diags, _) = oop_neg.check();
    let oop_msgs: Vec<String> = oop_diags.iter().map(|d| d.message.clone()).collect();
    for needle in [
        "cannot extend ':final' class 'SealedCls'",
        "circular class inheritance",
        "':extends' method 'non_virt'",
        "':initial' method 'virt_m'",
        "cannot override ':final' method 'locked_m'",
        "cannot override ':final' constraint 'c_locked'",
        "':initial' constraint 'c_existing'",
        "':extends' constraint 'c_missing'",
        "out-of-block constraint 'OvDerived::c_ext_mismatch'",
        "unknown base covergroup 'no_such_cg'",
        "does not have a super class for 'new(default)'",
    ] {
        assert!(oop_msgs.iter().any(|m| m.contains(needle)), "missing {needle:?} in {oop_msgs:?}");
    }

    let cls_neg = Driver::new()
        .add_file(base.join("negative/clauses_14_17_24_28_35_errors.sv"))
        .compile()
        .expect("clauses negative file must parse");
    let (cls_diags, _) = cls_neg.check();
    let cls_msgs: Vec<String> = cls_diags.iter().map(|d| d.message.clone()).collect();
    for needle in [
        "duplicate 'global clocking'",
        "cannot drive clocking input signal 'gcb1.q_in'",
        "user-defined nettype 'real_nt' is not allowed in a checker",
        "hierarchical lookup into checker 'u_chk'",
        "design element cannot reference program 'prog_bad'",
        "'always_comb' procedure is not allowed inside a program",
        "gate 'and' terminal cannot connect to user-defined nettype",
        "UDP 'my_buf' terminal cannot connect to user-defined nettype",
        "bidirectional switch 'tran' terminals must have matching nettypes",
        "deprecated '\"DPI\"' spec string",
        "'pure' DPI function cannot have 'void' return type",
        "invalid DPI function return type 'integer'",
        "'not_a_func' is not a subroutine",
    ] {
        assert!(cls_msgs.iter().any(|m| m.contains(needle)), "missing {needle:?} in {cls_msgs:?}");
    }
}

#[test]
fn mutual_param_function_dim_cycle_terminates_and_extra_sema_checks() {
    // Regression test for mutual parameter/function return-dimension cycle stack overflow.
    let cyc_src = r#"
module M;
    localparam logic [bar()-1:0] foo = 1;
    function logic [$bits(foo)-1:0] bar;
        return 1;
    endfunction
endmodule
"#;
    let comp = Compilation::from_source("mutual_cycle.sv", cyc_src);
    // Must terminate cleanly without stack overflow and emit a diagnostic on unresolved dimension/cycle.
    let _ = comp.diagnostics();

    // defparam on non-parameter, subroutine net port, covergroup sample rules, empty #(,).
    let neg_src = r#"
module sub; endmodule
module top;
    int not_a_param;
    defparam not_a_param = 1;
    function void bad_fn;
        input wire w;
    endfunction
    covergroup cg1 (int x) with function sample(bit a, output int y);
        coverpoint x;
    endgroup
    covergroup cg2 with function not_sample;
    endgroup
    sub #(,) u_sub();
endmodule
"#;
    let neg = Compilation::from_source("extra_sema_neg.sv", neg_src);
    let msgs: Vec<String> = neg.diagnostics().iter().map(|d| d.message.clone()).collect();
    for needle in [
        "defparam target 'not_a_param' is not a parameter",
        "subroutine port cannot have a net type",
        "covergroup sample argument cannot be 'output' or 'inout'",
        "covergroup 'with function' must name 'sample'",
        "empty positional parameter assignment is not allowed",
    ] {
        assert!(msgs.iter().any(|m| m.contains(needle)), "missing {needle:?} in {msgs:?}");
    }
}

/// Legal forms the front-end differential run against a simulator showed
/// svlang rejecting.
#[test]
fn differential_run_legal_forms_are_accepted() {
    for (name, src) in [
        // 29.8: `udp_instance ::= [ name_of_instance ] ( ... )`.
        (
            "unnamed_udp_instance.sv",
            r#"
primitive my_mux (output z, input a, b, s);
  table 0 ? 0 : 0; 1 ? 0 : 1; ? 0 1 : 0; ? 1 1 : 1; endtable
endprimitive
module top;
  wire z1, z2, z3, z4, z5, z6, z7, z8;
  reg a, b, s;
  my_mux u (z1, a, b, s);
  my_mux (z2, a, b, s);
  my_mux #0 (z3, a, b, s);
  my_mux #0 (z4, a, b, s), (z5, a, b, s);
  my_mux (strong0, weak1) (z6, a, b, s);
  my_mux (strong0, weak1) #1 (z7, a, b, s);
  my_mux #2 u2 (z8, a, b, s);
endmodule
"#,
        ),
        // 19.5.2: a transition bin with a repetition and no `=>`.
        (
            "transition_bin_repetition.sv",
            r#"
module top;
  bit clk;
  bit [3:0] a;
  covergroup cg @(posedge clk);
    cp : coverpoint a {
      bins t3 = (5 [*2]);
      bins t5 = (8 [-> 2] => 9);
      bins t6 = (10 [= 2]);
      bins t7 = (3 [*2:3] => 7);
      bins d = default sequence;
    }
  endgroup
endmodule
"#,
        ),
        // 18.5: a constraint expression that starts with a concatenation.
        (
            "constraint_concat_first.sv",
            r#"
class c;
  rand bit [7:0] q0, q1, q2, q3;
  constraint cp { {q1, q0} == {q3, q2}; if (q0 > 1) { q1 < 4; } }
endclass
"#,
        ),
        // 5.7.1: white space, a newline included, between base and digits.
        ("based_literal_newline.sv", "module top;\n  logic [3:0] w;\n  initial w = {2'b11, 2'd\n    1};\nendmodule\n"),
        // 27.6: unnamed generate blocks are `genblk<n>`, nested ones too.
        (
            "implicit_genblk_names.sv",
            r#"
module top;
  logic ok;
  if (1) begin : named_first end
  for (genvar k = 0; k < 2; k++) begin
    for (genvar j = 0; j < 2; j++) begin logic [3:0] datum = k*2+j; end
  end
  for (genvar k = 0; k < 2; k++) begin logic [3:0] datum = k+8; end
  assign ok = genblk2[0].genblk1[0].datum == 0 && genblk3[1].datum == 9;
endmodule
"#,
        ),
        // 22.14: `logic` is a plain name under the 1364-2005 keyword set.
        (
            "begin_keywords_logic_name.sv",
            "`begin_keywords \"1364-2005\"\nmodule top; reg logic; initial logic = 1; endmodule\n`end_keywords\n",
        ),
        // 8.19: a class constant is set in its constructor.
        ("class_const_in_new.sv", "class c; const int k; function new(); k = 5; endfunction endclass\n"),
        // 14.12: `default clocking cb;` designates an existing block.
        (
            "default_clocking_designation.sv",
            r#"
module top;
  bit clk;
  clocking cb @(posedge clk);
  endclocking
  default clocking cb;
endmodule
"#,
        ),
    ] {
        let comp = Compilation::from_source(name, src);
        assert!(comp.diagnostics().is_empty(), "{name}: {:?}", comp.diagnostics());
    }
}

/// The differential fixes keep rejecting what they must.
#[test]
fn differential_run_fixes_keep_their_errors() {
    // 12.7.3: a foreach body is a statement, not a null one.
    let foreach_null = Compilation::from_source(
        "foreach_null.sv",
        "module top; int q[4]; initial begin foreach (q[i]); end endmodule\n",
    );
    assert!(!foreach_null.diagnostics().is_empty());
    // 26.3: an explicit import names something the package declares.
    let bad_import = Compilation::from_source(
        "bad_import.sv",
        "package p; typedef int good_t; endpackage\nmodule top; import p::missing_t; import p::good_t; endmodule\n",
    );
    let msgs: Vec<_> = bad_import.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert_eq!(msgs, ["no member named 'missing_t' in package 'p'"]);
    // 6.20.6: a const variable is set by its declaration only.
    let const_write = Compilation::from_source(
        "const_write.sv",
        "module top; const int x = 1; int y; initial begin y = 3; x = 2; x += 1; end endmodule\n",
    );
    let msgs: Vec<_> = const_write.diagnostics().iter().map(|d| d.message.clone()).collect();
    assert_eq!(msgs, ["cannot assign to const variable 'x'", "cannot assign to const variable 'x'"]);
    // 23.3.2: a module instance needs a name.
    let unnamed = Compilation::from_source(
        "unnamed_module_instance.sv",
        "module leaf (output o, input i); assign o = i; endmodule\nmodule top; reg i; wire o; leaf #0 (o, i); endmodule\n",
    );
    assert!(
        unnamed.diagnostics().iter().any(|d| d.message.contains("has no instance name")),
        "{:?}",
        unnamed.diagnostics()
    );
    // Two clocking blocks of one name are still a redefinition.
    let twice = Compilation::from_source(
        "clocking_twice.sv",
        "module top; bit clk; clocking cb @(posedge clk); endclocking clocking cb @(posedge clk); endclocking endmodule\n",
    );
    assert!(
        twice.diagnostics().iter().any(|d| d.message.contains("redefinition of 'cb'")),
        "{:?}",
        twice.diagnostics()
    );
}
