//! Netlists lowered to AIGs: the graph behaves like the design (checked by
//! simulating it), survives a round trip through both AIGER forms, and
//! refuses what it cannot express.

use std::collections::HashMap;

use sv_aiger::{aiger, lit_val, lower, Aig, Init, Options};
use sv_netlist::{lower_trees, LowerConfig, Netlist};

fn netlist_with(src: &str, cfg: &LowerConfig) -> Netlist {
    let tree = sv_syntax::parse_source(src, "t.sv", Default::default());
    lower_trees(&[tree], &[], &[], cfg).unwrap_netlist()
}

fn netlist(src: &str) -> Netlist {
    netlist_with(src, &LowerConfig::default())
}

fn aig(src: &str) -> Aig {
    lower(&netlist(src), &Options::default()).unwrap().aig
}

/// Runs `g` for `steps` clock edges from its initial state (free latches start at 0) with
/// every input driven by `input(name, step)`. Returns the step of the first bad state.
fn first_bad(g: &Aig, steps: usize, input: impl Fn(&str, usize) -> bool) -> Option<usize> {
    let mut state: HashMap<u32, bool> = g.latches.iter().map(|l| (l.lit, l.init == Init::One)).collect();
    for step in 0..steps {
        let vals = g.eval(|l| match state.get(&l) {
            Some(&v) => v,
            None => input(g.names.get(&(l >> 1)).map_or("", String::as_str), step),
        });
        assert!(g.constraints.iter().all(|&c| lit_val(&vals, c)), "test inputs must satisfy the constraints");
        if g.bads.iter().any(|&b| lit_val(&vals, b)) {
            return Some(step);
        }
        state = g.latches.iter().map(|l| (l.lit, lit_val(&vals, l.next))).collect();
    }
    None
}

const COUNTER: &str = "module cnt (input logic clk, rst, output logic [3:0] q);\n\
       initial q = 4'd0;\n\
       always_ff @(posedge clk) if (rst) q <= 4'd0; else if (q != 4'd9) q <= q + 4'd1;\n\
       always @(posedge clk) in_range: assert (q <= LIMIT);\n\
     endmodule\n";

#[test]
fn saturating_counter_behaves_like_the_design() {
    let good = aig(&COUNTER.replace("LIMIT", "4'd9"));
    assert_eq!((good.latches.len(), good.bads.len()), (4, 1));
    assert!(good.latches.iter().all(|l| l.init == Init::Zero));
    // Never resetting, and resetting every seventh edge: the count stays in range.
    assert_eq!(first_bad(&good, 40, |_, _| false), None);
    assert_eq!(first_bad(&good, 40, |n, s| n == "cnt.rst" && s % 7 == 0), None);

    // With the limit one too low the count reaches 9 after nine edges.
    let bad = aig(&COUNTER.replace("LIMIT", "4'd8"));
    assert_eq!(first_bad(&bad, 40, |_, _| false), Some(9));
    // Held in reset it never gets there.
    assert_eq!(first_bad(&bad, 40, |n, _| n == "cnt.rst"), None);
}

#[test]
fn inputs_and_latches_are_named_by_bit() {
    let g = aig(&COUNTER.replace("LIMIT", "4'd9"));
    let names: Vec<&str> = g.names.values().map(String::as_str).collect();
    for want in ["cnt.clk", "cnt.rst", "cnt.q[0]", "cnt.q[3]"] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
}

#[test]
fn assumption_becomes_a_constraint() {
    let g = aig("module m (input logic clk, a, b);\n\
           always @(posedge clk) begin assume (a); chk: assert (a || b); end\n\
         endmodule\n");
    assert_eq!((g.bads.len(), g.constraints.len()), (1, 1));
    assert_eq!(first_bad(&g, 4, |n, _| n == "m.a"), None);
}

#[test]
fn both_aiger_forms_read_back_the_same() {
    let g = aig(&COUNTER.replace("LIMIT", "4'd8"));
    for back in [aiger::read(&g.to_binary()).unwrap(), aiger::read(g.to_ascii().as_bytes()).unwrap()] {
        assert_eq!(
            (back.inputs.len(), back.latches.len(), back.bads.len(), back.constraints.len(), back.num_ands()),
            (g.inputs.len(), g.latches.len(), g.bads.len(), g.constraints.len(), g.num_ands())
        );
        assert_eq!(first_bad(&back, 40, |_, _| false), Some(9));
        // The symbol table brings the names back.
        assert_eq!(first_bad(&back, 40, |n, _| n == "cnt.rst"), None);
        assert_eq!(back.bad_names, ["in_range"]);
    }
}

#[test]
fn memories_must_be_expanded_first() {
    let src =
        "module mem (input logic clk, we, input logic [1:0] wa, ra, input logic [3:0] d, output logic [3:0] q);\n\
           logic [3:0] m [4];\n\
           always_ff @(posedge clk) begin if (we) m[wa] <= d; q <= m[ra]; end\n\
         endmodule\n";
    let e = lower(&netlist(src), &Options::default()).unwrap_err();
    assert!(e.to_string().contains("array"), "{e}");
    let cfg = LowerConfig { blast_memories_up_to: Some(16), ..Default::default() };
    let g = lower(&netlist_with(src, &cfg), &Options::default()).unwrap().aig;
    assert_eq!(g.latches.len(), 4 * 4 + 4);
}

#[test]
fn two_clocks_are_an_error() {
    let nl = netlist(
        "module two(input logic a, b, d, output logic p, q);\n\
           always_ff @(posedge a) p <= d;\n\
           always_ff @(posedge b) q <= d;\n\
         endmodule\n",
    );
    let e = lower(&nl, &Options::default()).unwrap_err();
    assert!(e.to_string().contains("more than one clock"), "{e}");
}

#[test]
fn liveness_is_a_justice_property_and_reads_back() {
    use sv_netlist::PropertyOptions;
    let mut nl = netlist(
        "module live (input logic clk, rst, req, output logic gnt);\n\
           always_ff @(posedge clk) if (rst) gnt <= 1'b0; else gnt <= req;\n\
           granted: assert property (@(posedge clk) req |-> s_eventually gnt);\n\
           fair_req: assume property (@(posedge clk) s_eventually !req);\n\
         endmodule\n",
    );
    nl.compile_properties(&PropertyOptions::default());
    assert_eq!(nl.liveness.len(), 2, "both properties are liveness obligations");
    let g = lower(&nl, &Options::default()).unwrap().aig;
    assert_eq!((g.justice.len(), g.fairness.len()), (1, 1));
    assert_eq!(g.justice[0].len(), 1);
    assert!(g.justice_names[0].ends_with("granted"), "{:?}", g.justice_names);
    let text = g.to_ascii();
    let header: Vec<&str> = text.lines().next().unwrap().split_whitespace().collect();
    assert_eq!(&header[8..], ["1", "1"], "{text}");
    for back in [aiger::read(&g.to_binary()).unwrap(), aiger::read(text.as_bytes()).unwrap()] {
        assert_eq!((back.justice.len(), back.justice[0].len(), back.fairness.len()), (1, 1, 1));
        assert_eq!(back.justice_names, g.justice_names);
        assert_eq!(back.num_ands(), g.num_ands());
    }
    // A design with no liveness keeps the short header.
    let plain = aig(&COUNTER.replace("LIMIT", "4'd9")).to_ascii();
    assert_eq!(plain.lines().next().unwrap().split_whitespace().count(), 8, "{plain}");
}

/// The value of justice property 0 at each of `steps` clock edges, from the initial state,
/// with every input driven by `input(name, step)`.
fn justice_trace(g: &Aig, steps: usize, input: impl Fn(&str, usize) -> bool) -> Vec<bool> {
    let mut state: HashMap<u32, bool> = g.latches.iter().map(|l| (l.lit, l.init == Init::One)).collect();
    let mut trace = Vec::new();
    for step in 0..steps {
        let vals = g.eval(|l| match state.get(&l) {
            Some(&v) => v,
            None => input(g.names.get(&(l >> 1)).map_or("", String::as_str), step),
        });
        trace.push(lit_val(&vals, g.justice[0][0]));
        state = g.latches.iter().map(|l| (l.lit, lit_val(&vals, l.next))).collect();
    }
    trace
}

#[test]
fn the_justice_literal_follows_the_design() {
    use sv_netlist::PropertyOptions;
    // A request is granted one edge later when `en` allows it.
    let mut nl = netlist(
        "module live (input logic clk, req, en, output logic gnt);\n\
           initial gnt = 1'b0;\n\
           always_ff @(posedge clk) gnt <= req & en;\n\
           granted: assert property (@(posedge clk) req |-> s_eventually gnt);\n\
         endmodule\n",
    );
    nl.compile_properties(&PropertyOptions::default());
    let g = lower(&nl, &Options::default()).unwrap().aig;
    // The model picks the request it watches with a free input; pick the one at step 2.
    let pick = g.names.values().find(|n| n.contains("pick")).expect("the pick input").clone();
    let run = |en: fn(usize) -> bool| {
        justice_trace(&g, 40, |name, step| match name {
            "live.req" => true,
            "live.en" => en(step),
            n if n == pick => step == 2,
            _ => false,
        })
    };
    // Never enabled: the watched request is never granted, and the literal stays true.
    let starved = run(|_| false);
    assert!(starved[10..].iter().all(|&b| b), "{starved:?}");
    // Enabled from step 20: the grant comes, and the literal is false from then on.
    let served = run(|step| step >= 20);
    assert!(served[25..].iter().all(|&b| !b), "{served:?}");
}
