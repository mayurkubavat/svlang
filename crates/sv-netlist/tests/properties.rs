//! Concurrent properties compiled to checks (`Netlist::compile_properties`): simulated tick
//! by tick, a check fails exactly at the ticks IEEE 1800 says the property fails.

mod sim;

use sim::{lower_src, NetlistSim};
use sv_netlist::{CheckKind, Netlist, PropertyOptions, PropertyOutcome};

const PORTS: &str = "input logic clk, rst, a, b, c";

/// A module with `PORTS` and `body`, lowered, with every property compiled.
fn compiled(body: &str, skip_cycles: u32) -> Netlist {
    let src = format!("module m({PORTS});\n default clocking @(posedge clk); endclocking\n{body}\nendmodule\n");
    let mut nl = lower_src(&src).unwrap_netlist();
    let mut opts = PropertyOptions::default();
    opts.skip_cycles = skip_cycles;
    for o in nl.compile_properties(&opts) {
        assert!(matches!(o, PropertyOutcome::Compiled { .. }), "{o:?}");
    }
    nl
}

/// Drives one clock tick per row of `stimulus` (`rst a b c` as bits) and returns the
/// ticks, counted from 0, at which an assertion failed.
fn failures(nl: &Netlist, stimulus: &[&str]) -> Vec<usize> {
    let mut sim = NetlistSim::new(nl);
    let mut out = Vec::new();
    for (tick, row) in stimulus.iter().enumerate() {
        for (port, bit) in ["rst", "a", "b", "c"].iter().zip(row.chars()) {
            sim.set_u(port, i128::from(bit == '1'));
        }
        let before = sim.failed_asserts.len();
        sim.tick_posedge("clk");
        if sim.failed_asserts.len() > before {
            out.push(tick);
        }
    }
    out
}

fn fails(body: &str, stimulus: &[&str]) -> Vec<usize> {
    failures(&compiled(body, 0), stimulus)
}

#[test]
fn overlapped_implication_is_checked_from_the_first_tick() {
    let p = "p: assert property (a |-> b);";
    assert_eq!(fails(p, &["0100", "0110", "0000", "0100"]), [0, 3]);
}

#[test]
fn next_cycle_implication_fails_one_tick_after_its_antecedent() {
    let p = "p: assert property (a |=> b);";
    //                 a      b      a,!b   !b: fails    a         b
    assert_eq!(fails(p, &["0100", "0010", "0100", "0000", "0100", "0010"]), [3]);
    // The antecedent in the last tick has no next tick yet: no failure.
    assert_eq!(fails(p, &["0000", "0100"]), [] as [usize; 0]);
}

#[test]
fn a_plain_sequence_must_match_at_every_tick() {
    // `a ##1 b` started at tick t needs a at t and b at t+1; it is decided at t+1.
    let p = "p: assert property (a ##1 b);";
    assert_eq!(fails(p, &["0100", "0110", "0010", "0010"]), [3]);
}

#[test]
fn fixed_delays_and_repetition_place_each_boolean_on_its_tick() {
    let p = "p: assert property (a ##2 b |-> ##1 c);";
    // a at 0, b at 2, so c is due at 3.
    assert_eq!(fails(p, &["0100", "0000", "0010", "0001"]), [] as [usize; 0]);
    assert_eq!(fails(p, &["0100", "0000", "0010", "0000"]), [3]);
    // b one tick early is not a match.
    assert_eq!(fails(p, &["0100", "0010", "0000", "0000"]), [] as [usize; 0]);

    let r = "p: assert property (a [*3] |=> b);";
    assert_eq!(fails(r, &["0100", "0100", "0100", "0000"]), [3]);
    assert_eq!(fails(r, &["0100", "0000", "0100", "0100", "0000"]), [] as [usize; 0]);
    // Overlapping runs of three: ticks 0-2 and 1-3.
    assert_eq!(fails(r, &["0100", "0100", "0100", "0110", "0000"]), [4]);
}

#[test]
fn disable_iff_drops_an_attempt_at_any_of_its_ticks() {
    let p = "p: assert property (disable iff (rst) a ##1 a |=> b);";
    assert_eq!(fails(p, &["0100", "0100", "0000"]), [2]);
    for at in 0..3 {
        let mut rows = vec!["0100", "0100", "0000"];
        let with_rst = rows[at].replacen('0', "1", 1);
        rows[at] = &with_rst;
        assert_eq!(fails(p, &rows), [] as [usize; 0], "reset at tick {at}");
    }
}

#[test]
fn sampled_value_functions_wait_for_real_history() {
    // At tick 0 `$rose(a)` has no earlier sample: the check waits one tick.
    let p = "p: assert property ($rose(a) |-> b);";
    assert_eq!(fails(p, &["0100", "0000", "0100", "0100"]), [2]);
    // `$past(a, 2)` is real from tick 2.
    let q = "p: assert property (b |-> $past(a, 2));";
    assert_eq!(fails(q, &["0010", "0010", "0010", "0100", "0000", "0010"]), [2]);
}

#[test]
fn attempts_in_the_skipped_cycles_are_not_checked() {
    let p = "p: assert property (a |=> b);";
    let rows = ["0100", "0100", "0000"];
    assert_eq!(failures(&compiled(p, 0), &rows), [1, 2]);
    assert_eq!(failures(&compiled(p, 1), &rows), [2]);
    assert_eq!(failures(&compiled(p, 2), &rows), [] as [usize; 0]);
}

#[test]
fn the_tick_counter_stops_and_checks_stay_on() {
    let nl = compiled("p: assert property (a |=> b);", 5);
    let mut rows = vec!["0000"; 40];
    rows[30] = "0100";
    assert_eq!(failures(&nl, &rows), [31]);
    let tick = nl.states.iter().find(|s| s.name == "m._sva_tick").expect("one tick counter");
    assert_eq!(tick.width, 3, "counts to 6");
}

#[test]
fn kinds_names_and_unsupported_operators() {
    let src = format!(
        "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
         a1: assert property (a |-> b);\n\
         assume property (a |=> c);\n\
         c1: cover property (a ##1 b);\n\
         for (genvar i = 0; i < 2; i++) begin : g\n\
           a2: assert property (a |-> c);\n\
         end\n\
         u1: assert property (a |-> ##[1:$] b);\n\
         u2: assert property (a |-> s_eventually b);\n\
         u3: assert property (a until b);\n\
         u4: assert property (@(clk) a);\n\
         u5: assert property (a [*1:2] |-> b);\n\
         u6: assert property (not a);\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    let named: Vec<(&str, &PropertyOutcome)> = nl.property_names.iter().map(String::as_str).zip(&out).collect();
    let why = |name: &str| match named.iter().find(|(n, _)| *n == name) {
        Some((_, PropertyOutcome::Unsupported(w))) => w.clone(),
        other => panic!("{name}: {other:?}"),
    };
    assert_eq!(why("m.u1"), "the delay range [1:$]");
    assert_eq!(why("m.u2"), "`eventually`");
    assert_eq!(why("m.u3"), "`until`");
    assert_eq!(why("m.u4"), "a clocking event on both edges");
    assert_eq!(why("m.u5"), "the repetition range [1:2]");
    assert_eq!(why("m.u6"), "property `not`");

    let check = |name: &str| {
        let i = nl.property_names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name}"));
        let c = &nl.checks[nl.property_checks[i].unwrap_or_else(|| panic!("{name} not compiled"))];
        assert_eq!(c.label.as_deref(), Some(name));
        assert!(!c.immediate);
        c.kind
    };
    assert_eq!(check("m.a1"), CheckKind::Assert);
    assert_eq!(check("m._assume_1"), CheckKind::Assume);
    assert_eq!(check("m.c1"), CheckKind::Cover);
    assert_eq!(check("m.g[0].a2"), CheckKind::Assert);
    assert_eq!(check("m.g[1].a2"), CheckKind::Assert);

    // Compiling again keeps what is there.
    let checks = nl.checks.len();
    assert_eq!(nl.compile_properties(&PropertyOptions::default()), out);
    assert_eq!(nl.checks.len(), checks);
}
