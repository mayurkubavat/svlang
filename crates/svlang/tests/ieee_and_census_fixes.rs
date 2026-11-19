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
        match name {
            // Checker and UDP instantiation in module hierarchy are scheduled for Phase 3;
            // verify they parse cleanly and only report the known unelaborated-target diagnostic.
            "ch17_checkers.sv" => {
                assert!(
                    diags.iter().all(|d| d.stage == DiagnosticStage::Semantic && d.message.contains("req_ack_chk")),
                    "unexpected diags in {name}: {diags:?}"
                );
            }
            "ch28_31_gates_udp_specify.sv" => {
                assert!(
                    diags.iter().all(|d| d.stage == DiagnosticStage::Semantic
                        && (d.message.contains("ch29_mux") || d.message.contains("ch29_dff"))),
                    "unexpected diags in {name}: {diags:?}"
                );
            }
            _ => {
                assert!(diags.is_empty(), "expected 0 diagnostics on LRM file {name}, got {diags:?}");
            }
        }
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
