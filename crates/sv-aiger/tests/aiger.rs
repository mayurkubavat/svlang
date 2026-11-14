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
