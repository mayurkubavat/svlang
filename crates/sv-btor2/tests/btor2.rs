//! The Btor2 text: well formed, and the modelling choices in the crate docs
//! show up as the lines they promise.

use std::collections::HashSet;

use sv_btor2::{write, Clock, Fail, Options};
use sv_netlist::{lower_trees, LowerConfig, Netlist};

fn netlist(src: &str) -> Netlist {
    let tree = sv_syntax::parse_source(src, "t.sv", Default::default());
    lower_trees(&[tree], &[], &[], &LowerConfig::default()).unwrap_netlist()
}

/// Every line is `<id> <op> ...` with ids that go up, and every sort or node
/// a line refers to was defined above it.
fn well_formed(btor: &str) {
    let mut seen: HashSet<u64> = HashSet::new();
    let mut last = 0;
    for line in btor.lines().filter(|l| !l.is_empty() && !l.starts_with(';')) {
        let w: Vec<&str> = line.split_whitespace().collect();
        let id: u64 = w[0].parse().unwrap_or_else(|_| panic!("no id: {line}"));
        assert!(id > last, "ids must go up: {line}");
        last = id;
        // Which of the words after the op are references.
        let refs: &[usize] = match w[1] {
            "sort" if w[2] == "array" => &[3, 4],
            "sort" => &[],
            "const" | "constd" | "consth" | "input" | "state" | "zero" | "one" | "ones" => &[2],
            "bad" | "constraint" | "output" | "fair" | "justice" => &[2],
            "slice" | "uext" | "sext" => &[2, 3],
            _ => &[2, 3, 4, 5],
        };
        for &i in refs {
            let Some(word) = w.get(i) else { continue };
            let Ok(n) = word.trim_start_matches('-').parse::<u64>() else { continue };
            assert!(seen.contains(&n), "{n} used before it is defined: {line}");
        }
        seen.insert(id);
    }
    assert!(last > 0, "empty model");
}

fn ok(src: &str) -> String {
    let m = write(&netlist(src), &Options::default()).unwrap();
    well_formed(&m.btor2);
    m.btor2
}

fn lines<'a>(btor: &'a str, op: &str) -> Vec<&'a str> {
    btor.lines().filter(|l| l.split_whitespace().nth(1) == Some(op)).collect()
}

#[test]
fn counter_with_async_reset_and_labelled_assert() {
    let b = ok("module cnt(input logic clk, rst_n, output logic [3:0] q);\n\
           always_ff @(posedge clk or negedge rst_n) if (!rst_n) q <= 4'd0; else q <= q + 4'd1;\n\
           always @(posedge clk) p_le: assert (q <= 4'd15);\n\
         endmodule\n");
    assert_eq!(lines(&b, "state").len(), 1);
    assert!(lines(&b, "bad").iter().any(|l| l.ends_with(" p_le")));
    // The reset acts synchronously: the next state picks the reset value.
    assert!(!lines(&b, "ite").is_empty());
}

#[test]
fn anyconst_is_held_and_x_is_free() {
    let b = ok("module fr(input logic clk, input logic [1:0] s);\n\
           (* anyconst *) wire [3:0] k;\n\
           logic [3:0] y;\n\
           always_comb case (s) 2'd0: y = k; default: y = 'x; endcase\n\
           always @(posedge clk) assert (y == k);\n\
         endmodule\n");
    // `k` is a state whose next value is itself.
    let st = lines(&b, "state").into_iter().find(|l| l.ends_with(" fr.k")).expect("k state");
    let id = st.split_whitespace().next().unwrap();
    assert!(lines(&b, "next").iter().any(|l| {
        let w: Vec<_> = l.split_whitespace().collect();
        w[3] == id && w[4] == id
    }));
    // The X default is a free input, not 0.
    assert!(lines(&b, "input").iter().any(|l| l.contains("svlang_x_")));
}

#[test]
fn name_prefix_is_used_for_invented_names() {
    let nl = netlist(
        "module fr(input logic clk, input logic s, output logic [3:0] y);\n\
           always_comb if (s) y = 4'd1; else y = 'x;\n\
           always @(posedge clk) assert (y != 4'd2);\n\
         endmodule\n",
    );
    let mut o = Options::default();
    o.name_prefix = "mytool".into();
    let b = write(&nl, &o).unwrap().btor2;
    assert!(lines(&b, "input").iter().any(|l| l.contains("mytool_x_")), "{b}");
}

#[test]
fn two_clocks_are_rejected() {
    let e = write(
        &netlist(
            "module two(input logic a, b, d, output logic p, q);\n\
               always_ff @(posedge a) p <= d;\n\
               always_ff @(posedge b) q <= d;\n\
             endmodule\n",
        ),
        &Options::default(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("more than one clock"), "{e}");
}

#[test]
fn concurrent_assertions_are_noted_not_written() {
    let nl = netlist(
        "module ca(input logic clk, a, b);\n\
           assert property (@(posedge clk) a |-> b);\n\
         endmodule\n",
    );
    let m = write(&nl, &Options::default()).unwrap();
    assert!(lines(&m.btor2, "bad").is_empty());
    assert!(m.notes.iter().any(|n| n.contains("concurrent")), "{:?}", m.notes);
}

#[test]
fn trace_section_names_combinational_nets() {
    let nl = netlist(
        "module tr(input logic clk, input logic [3:0] a, output logic [3:0] q);\n\
           logic [3:0] n;\n\
           assign n = a + 4'd1;\n\
           always_ff @(posedge clk) q <= n;\n\
         endmodule\n",
    );
    let mut o = Options::default();
    assert!(!write(&nl, &o).unwrap().btor2.contains("trace-outputs"));
    o.trace_outputs = true;
    let b = write(&nl, &o).unwrap().btor2;
    well_formed(&b);
    let (model, section) = b.split_once("; trace-outputs\n").expect("marker");
    assert!(lines(model, "output").is_empty());
    assert!(section.lines().any(|x| x.ends_with(" tr.n")), "{section}");
}

#[test]
fn concurrent_assumption_is_an_error() {
    let nl = netlist(
        "module cs(input logic clk, a, b);\n\
           assume property (@(posedge clk) a |-> b);\n\
         endmodule\n",
    );
    let e = write(&nl, &Options::default()).unwrap_err();
    assert!(matches!(e, Fail::Unsupported(_)) && e.to_string().contains("assumptions"), "{e}");
}

fn compiled(src: &str, skip_cycles: u32) -> Netlist {
    let mut nl = netlist(src);
    let mut opts = sv_netlist::PropertyOptions::default();
    opts.skip_cycles = skip_cycles;
    for (i, o) in nl.compile_properties(&opts).iter().enumerate() {
        assert!(matches!(o, sv_netlist::PropertyOutcome::Compiled { .. }), "property {i}: {o:?}");
    }
    nl
}

#[test]
fn compiled_properties_are_bad_lines_and_constraints() {
    let nl = compiled(
        "module cp(input logic clk, rst, a, b, c);\n\
           default clocking @(posedge clk); endclocking\n\
           a_next: assert property (disable iff (rst) a |=> b);\n\
           assert property (a ##2 b |-> c);\n\
           m_in: assume property (c |-> a);\n\
           c_seq: cover property (a ##1 b);\n\
         endmodule\n",
        1,
    );
    let m = write(&nl, &Options::default()).unwrap();
    well_formed(&m.btor2);
    let bads = lines(&m.btor2, "bad");
    assert_eq!(bads.len(), 2, "{bads:?}");
    assert!(bads[0].ends_with(" cp.a_next"), "{bads:?}");
    assert!(bads[1].ends_with(" cp._assert_1"), "{bads:?}");
    assert_eq!(lines(&m.btor2, "constraint").len(), 1);
    assert!(m.notes.iter().any(|n| n.contains("1 cover(s)")), "{:?}", m.notes);
    assert!(!m.notes.iter().any(|n| n.contains("concurrent")), "{:?}", m.notes);

    let mut with_covers = Options::default();
    with_covers.covers = true;
    let m = write(&nl, &with_covers).unwrap();
    well_formed(&m.btor2);
    let bads = lines(&m.btor2, "bad");
    assert_eq!(bads.len(), 3, "{bads:?}");
    assert!(bads[2].ends_with(" cp.c_seq"), "{bads:?}");
}

#[test]
fn a_property_outside_the_subset_is_left_out_and_named() {
    let mut nl = netlist(
        "module un(input logic clk, a, b);\n\
           a_rng: assert property (@(posedge clk) a |-> ##[1:$] b);\n\
           a_ok: assert property (@(posedge clk) a |-> b);\n\
           a_win: assert property (@(posedge clk) a |-> ##[1:3] b);\n\
         endmodule\n",
    );
    let out = nl.compile_properties(&Default::default());
    assert!(matches!(&out[0], sv_netlist::PropertyOutcome::Unsupported(w) if w.contains("[1:$]")), "{out:?}");
    assert!(matches!(out[1], sv_netlist::PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[2], sv_netlist::PropertyOutcome::Compiled { .. }), "{out:?}");
    let m = write(&nl, &Options::default()).unwrap();
    well_formed(&m.btor2);
    let bads = lines(&m.btor2, "bad");
    assert_eq!(bads.len(), 2, "{bads:?}");
    assert!(bads[0].ends_with(" un.a_ok") && bads[1].ends_with(" un.a_win"), "{bads:?}");
    assert!(m.notes.iter().any(|n| n.starts_with("1 concurrent")), "{:?}", m.notes);
}

const BOTH: &str = "module be(input logic clk, rst, d, output logic p, n);\n\
       always_ff @(posedge clk) if (rst) p <= 1'b0; else p <= d;\n\
       always_ff @(negedge clk) n <= p;\n\
       always @(posedge clk) a_pos: assert (rst || p == $past(d));\n\
     endmodule\n";

#[test]
fn both_edges_are_rejected_unless_asked_for() {
    let nl = netlist(BOTH);
    let e = write(&nl, &Options::default()).unwrap_err();
    assert!(e.to_string().contains("different edges"), "{e}");

    let mut opts = Options::default();
    opts.both_edges = true;
    let m = write(&nl, &opts).unwrap();
    well_formed(&m.btor2);
    assert_eq!(m.steps_per_cycle, 2);
    assert!(m.notes.iter().any(|n| n.contains("half a clock cycle")), "{:?}", m.notes);
    // The clock level is a state that starts at 0, and the clock input is held to it.
    let phase = lines(&m.btor2, "state").into_iter().find(|l| l.ends_with(" svlang_clock_phase")).expect("phase state");
    let id = phase.split_whitespace().next().unwrap();
    assert!(lines(&m.btor2, "init").iter().any(|l| l.split_whitespace().nth(3) == Some(id)), "{}", m.btor2);
    assert_eq!(lines(&m.btor2, "constraint").len(), 1, "{}", m.btor2);
}

#[test]
fn one_edge_is_written_the_same_with_both_edges_on() {
    let nl = netlist("module oe(input logic clk, d, output logic q);\n always_ff @(negedge clk) q <= d;\nendmodule\n");
    let mut opts = Options::default();
    let plain = write(&nl, &opts).unwrap();
    opts.both_edges = true;
    let m = write(&nl, &opts).unwrap();
    assert_eq!(m.btor2, plain.btor2);
    assert_eq!(m.steps_per_cycle, 1);
}

#[test]
fn both_edges_need_the_clock_to_be_an_input() {
    let nl = netlist(
        "module gc(input logic clk, en, d, output logic p, n);\n\
           logic g;\n\
           assign g = clk & en;\n\
           always_ff @(posedge g) p <= d;\n\
           always_ff @(negedge g) n <= p;\n\
         endmodule\n",
    );
    let mut opts = Options::default();
    opts.both_edges = true;
    let e = write(&nl, &opts).unwrap_err();
    assert!(e.to_string().contains("not a top-level input"), "{e}");
}

#[test]
fn a_latch_is_its_data_over_a_held_state() {
    let m = write(
        &netlist("module lat(input logic en, d, output logic q);\n always_latch if (en) q <= d;\nendmodule\n"),
        &Options::default(),
    )
    .unwrap();
    well_formed(&m.btor2);
    let held: Vec<&str> = lines(&m.btor2, "state");
    assert_eq!(held.len(), 1, "{}", m.btor2);
    assert!(held[0].ends_with(" svlang_latch_0"), "{}", m.btor2);
    // The value is a mux of the data and the held state, and the held state follows it.
    let id = held[0].split_whitespace().next().unwrap();
    let ite = lines(&m.btor2, "ite");
    assert!(ite.iter().any(|l| l.split_whitespace().last() == Some(id)), "{}", m.btor2);
    assert_eq!(lines(&m.btor2, "next").len(), 1);
}

#[test]
fn latch_loops_are_rejected() {
    let own =
        netlist("module l1(input logic en, output logic [3:0] q);\n always_latch if (en) q <= q + 4'd1;\nendmodule\n");
    let e = write(&own, &Options::default()).unwrap_err();
    assert!(e.to_string().contains("depends on itself"), "{e}");
    let pair = netlist(
        "module l2(input logic a, b, d, output logic p, q);\n\
           always_latch if (a) p <= d ^ q;\n\
           always_latch if (b) q <= p;\n\
         endmodule\n",
    );
    let e = write(&pair, &Options::default()).unwrap_err();
    assert!(e.to_string().contains("depends on itself"), "{e}");
    // One latch feeding another is fine.
    let chain = netlist(
        "module l3(input logic a, b, d, output logic p, q);\n\
           always_latch if (a) p <= d;\n\
           always_latch if (b) q <= p;\n\
         endmodule\n",
    );
    well_formed(&write(&chain, &Options::default()).unwrap().btor2);
}

const TWO: &str = "module two(input logic a, b, d, output logic p, q);\n\
       always_ff @(posedge a) p <= d;\n\
       always_ff @(posedge b) q <= p;\n\
     endmodule\n";

#[test]
fn several_clocks_need_a_wave_each() {
    let nl = netlist(TWO);
    let mut opts = Options::default();
    opts.clocks = vec![Clock { input: "a".into(), half_period: 1, phase: 0 }];
    let e = write(&nl, &opts).unwrap_err();
    assert!(e.to_string().contains("b (the clock of two.q) is not a declared clock"), "{e}");

    opts.clocks.push(Clock { input: "b".into(), half_period: 2, phase: 1 });
    let m = write(&nl, &opts).unwrap();
    well_formed(&m.btor2);
    assert_eq!(m.steps_per_cycle, 2);
    assert!(m.notes.iter().any(|n| n.contains("2 clocks")), "{:?}", m.notes);
    // One tick counter, and each clock input held to its wave.
    assert_eq!(lines(&m.btor2, "state").iter().filter(|l| l.ends_with(" svlang_clock_phase")).count(), 1);
    assert_eq!(lines(&m.btor2, "constraint").len(), 2, "{}", m.btor2);

    opts.clocks[1].half_period = 0;
    assert!(write(&nl, &opts).unwrap_err().to_string().contains("half period of 0"));
}

#[test]
fn a_derived_second_clock_is_rejected() {
    let nl = netlist(
        "module dv(input logic clk, d, output logic q);\n\
           logic half;\n\
           always_ff @(posedge clk) half <= ~half;\n\
           always_ff @(posedge half) q <= d;\n\
         endmodule\n",
    );
    let mut opts = Options::default();
    opts.clocks = vec![Clock { input: "clk".into(), half_period: 1, phase: 0 }];
    let e = write(&nl, &opts).unwrap_err();
    assert!(e.to_string().contains("not a top-level input"), "{e}");
}

#[test]
fn declared_clocks_do_not_change_a_one_edge_design() {
    let nl = netlist("module oc(input logic clk, d, output logic q);\n always_ff @(posedge clk) q <= d;\nendmodule\n");
    let mut opts = Options::default();
    let plain = write(&nl, &opts).unwrap();
    opts.clocks = vec![Clock { input: "clk".into(), half_period: 3, phase: 2 }];
    let m = write(&nl, &opts).unwrap();
    assert_eq!(m.btor2, plain.btor2);
    assert_eq!(m.steps_per_cycle, 1);
}
