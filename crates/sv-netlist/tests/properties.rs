//! Concurrent properties compiled to checks (`Netlist::compile_properties`): simulated tick
//! by tick, a check fails exactly at the ticks IEEE 1800 says the property fails.

mod sim;

use std::collections::BTreeSet;

use sim::{lower_src, NetlistSim};
use sv_netlist::{CheckKind, Netlist, PropertyOptions, PropertyOutcome};
use sv_semantic::CVal;

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

/// Which checks to watch in a run.
#[derive(Clone, Copy)]
enum Watch {
    FailedAsserts,
    ViolatedAssumes,
    HitCovers,
}

/// Drives one clock tick per row of `stimulus` (`rst a b c` as bits, `x` for unknown) and returns, for each
/// tick counted from 0, the labels of the watched checks that fired at it.
fn run(nl: &Netlist, stimulus: &[&str], watch: Watch) -> Vec<Vec<String>> {
    let mut sim = NetlistSim::new(nl);
    let mut out = Vec::new();
    for row in stimulus {
        for (port, bit) in ["rst", "a", "b", "c"].iter().zip(row.chars()) {
            match bit {
                'x' => sim.set_cval(port, CVal::all_x(1, false)),
                _ => sim.set_u(port, i128::from(bit == '1')),
            }
        }
        let fired = |sim: &NetlistSim| match watch {
            Watch::FailedAsserts => sim.failed_asserts.clone(),
            Watch::ViolatedAssumes => sim.violated_assumes.clone(),
            Watch::HitCovers => sim.hit_covers.clone(),
        };
        let before = fired(&sim).len();
        sim.tick_posedge("clk");
        out.push(fired(&sim).split_off(before));
    }
    out
}

/// The ticks at which any watched check fired.
fn ticks(nl: &Netlist, stimulus: &[&str], watch: Watch) -> Vec<usize> {
    run(nl, stimulus, watch).iter().enumerate().filter(|(_, labels)| !labels.is_empty()).map(|(t, _)| t).collect()
}

/// The ticks, counted from 0, at which an assertion failed.
fn failures(nl: &Netlist, stimulus: &[&str]) -> Vec<usize> {
    ticks(nl, stimulus, Watch::FailedAsserts)
}

fn fails(body: &str, stimulus: &[&str]) -> Vec<usize> {
    failures(&compiled(body, 0), stimulus)
}

/// The ticks at which a cover of `body` is hit.
fn covers(body: &str, stimulus: &[&str]) -> Vec<usize> {
    ticks(&compiled(body, 0), stimulus, Watch::HitCovers)
}

const NONE: [usize; 0] = [];

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
fn a_property_under_a_condition_starts_only_when_reached() {
    // As `a |-> (b |=> c)`: reached at a tick where a holds.
    let p = "always @(posedge clk) begin if (a) p: assert property (b |=> c); end";
    assert_eq!(fails(p, &["0110", "0000"]), [1]);
    assert_eq!(fails(p, &["0010", "0000"]), [] as [usize; 0]);
    assert_eq!(fails(p, &["0110", "0001"]), [] as [usize; 0]);
    // The else branch and a case arm carry their own conditions.
    let e = "always @(posedge clk) if (a) begin end else p: assert property (b);";
    assert_eq!(fails(e, &["0100", "0000", "0010"]), [1]);
    let c = "always @(posedge clk) case ({a, b}) 2'b10: p: assert property (c); default: ; endcase";
    assert_eq!(fails(c, &["0100", "0110", "0101", "0000"]), [0]);
    // Not under a condition: no path condition is kept.
    let nl = compiled("always @(posedge clk) begin p: assert property (a |-> b); end", 0);
    assert_eq!(nl.property_enables, [None]);
    assert_eq!(failures(&nl, &["0100"]), [0]);
}

#[test]
fn a_property_under_a_condition_in_a_cover_and_an_async_reset_block() {
    let src = format!(
        "module m({PORTS}, output logic q);\n\
           always_ff @(posedge clk or posedge rst)\n\
             if (rst) q <= 1'b0;\n\
             else begin\n\
               q <= a;\n\
               p: assert property (a |=> q);\n\
               if (b) w: assert property (c);\n\
             end\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    for o in nl.compile_properties(&PropertyOptions::default()) {
        assert!(matches!(o, PropertyOutcome::Compiled { .. }), "{o:?}");
    }
    // p holds by construction outside reset; w fails when b holds without c.
    assert_eq!(failures(&nl, &["1000", "0100", "0010", "0011", "1010"]), [2]);
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
         u3: assert property (a s_until b);\n\
         u4: assert property (@(clk) a);\n\
         u5: assert property (a [*1:$] |-> b);\n\
         u6: assert property (not a);\n\
         u7: assert property (a |-> b [->1] ##1 c);\n\
         u8: assert property (a |-> b [=1:2] ##1 c);\n\
         u9: assert property (a |-> ##[1:2000] b);\n\
         u10: assert property (a |-> b [*0:1]);\n\
         u11: assert property (a [*0] |-> b);\n\
         u12: assert property ((a ##[1:60] b) within (c ##[1:900] c));\n\
         u13: assert property (a |-> (b |-> ##[1:2] c));\n\
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
    assert!(why("m.u3").contains("s_until"), "{}", why("m.u3"));
    assert_eq!(why("m.u4"), "a clocking event on both edges");
    assert_eq!(why("m.u5"), "the repetition range [1:$]");
    assert_eq!(why("m.u6"), "property `not`");
    assert_eq!(why("m.u7"), "the goto repetition [->]");
    assert_eq!(why("m.u8"), "the non-consecutive repetition [=]");
    assert_eq!(why("m.u9"), "a property longer than 1024 clock ticks");
    assert_eq!(why("m.u10"), "a sequence that admits an empty match, used as a property");
    assert_eq!(why("m.u11"), "an antecedent that has no match");
    assert_eq!(why("m.u12"), "a property of more than 65536 terms");
    assert_eq!(why("m.u13"), "an implication inside another property operator");

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

// Bounded sequences. In the comments a tick is written as its number; rows are `rst a b c`.

#[test]
fn a_delay_range_in_the_consequent_needs_one_match_and_is_decided_at_its_end() {
    let p = "p: assert property (a |-> ##[1:3] b);";
    // b low at 1, 2 and 3: the attempt of tick 0 fails, at tick 3.
    assert_eq!(fails(p, &["0100", "0000", "0000", "0000", "0000"]), [3]);
    // b at any one of the three ticks is a match.
    assert_eq!(fails(p, &["0100", "0010", "0000", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0000", "0010", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0000", "0000", "0010"]), NONE);
    // b with a, and b one tick too late, are outside the window.
    assert_eq!(fails(p, &["0110", "0000", "0000", "0000", "0010"]), [3]);
    // Two attempts, each with its own window: the one of tick 1 has b at 4, the one of tick 0 not.
    assert_eq!(fails(p, &["0100", "0100", "0000", "0000", "0010"]), [3]);
}

#[test]
fn a_plain_ranged_sequence_needs_a_match_from_every_tick() {
    let p = "p: assert property (a ##[1:2] b);";
    assert_eq!(fails(p, &["0110", "0110", "0110", "0110"]), NONE);
    // From 0: a, then b neither at 1 nor at 2. From 1: a, then b at 3.
    assert_eq!(fails(p, &["0110", "0100", "0100", "0110"]), [2]);
    // From 1 there is no a: no match, whatever b does.
    assert_eq!(fails(p, &["0100", "0010", "0010", "0010"]), [3]);
}

#[test]
fn every_match_of_the_antecedent_is_an_obligation() {
    let p = "p: assert property (a ##[1:2] b |-> c);";
    // a at 0 and b at 1 and at 2: two matches, c is due at 1 and at 2.
    assert_eq!(fails(p, &["0100", "0011", "0011", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0011", "0010", "0000"]), [2], "c missing at 2");
    assert_eq!(fails(p, &["0100", "0010", "0011", "0000"]), [2], "c missing at 1, reported when the attempt ends");
    // Only the match that exists obliges.
    assert_eq!(fails(p, &["0100", "0000", "0011", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0010", "0001", "0000"]), [2]);
    assert_eq!(fails(p, &["0100", "0001", "0001", "0000"]), NONE, "no b: vacuous");
}

#[test]
fn a_delay_range_from_zero_joins_on_the_same_tick() {
    let p = "p: assert property (a ##[0:1] b |-> c);";
    // a and b at 0 is a match ending at 0; a at 0 with b at 1 one ending at 1.
    assert_eq!(fails(p, &["0111", "0011", "0000"]), NONE);
    assert_eq!(fails(p, &["0110", "0000", "0000"]), [1], "c missing at 0");
    assert_eq!(fails(p, &["0111", "0010", "0000"]), [1], "c missing at 1");
    assert_eq!(fails(p, &["0010", "0100", "0000"]), NONE, "b before a is no match");
}

#[test]
fn a_repetition_range_in_the_antecedent_and_in_the_consequent() {
    let p = "p: assert property (a [*2:3] |=> b);";
    // From 0: a a ends at 1 (b due at 2, missing), a a a ends at 2 (b due at 3).
    assert_eq!(fails(p, &["0100", "0100", "0100", "0010", "0000"]), [3]);
    // b at 2 and 3 serves the attempts of 0 and of 1.
    assert_eq!(fails(p, &["0100", "0100", "0110", "0010", "0000"]), NONE);
    // b missing at 3: a a a from 0 (decided at 3) and a a from 1 (decided at 4).
    assert_eq!(fails(p, &["0100", "0100", "0110", "0000", "0000"]), [3, 4]);

    let q = "p: assert property (a |=> b [*1:2] ##1 c);";
    assert_eq!(fails(q, &["0100", "0010", "0001", "0000"]), NONE, "b c");
    assert_eq!(fails(q, &["0100", "0010", "0010", "0001"]), NONE, "b b c");
    assert_eq!(fails(q, &["0100", "0010", "0000", "0001"]), [3], "b, gap, c");
    assert_eq!(fails(q, &["0100", "0000", "0001", "0001"]), [3], "no b");
    assert_eq!(fails(q, &["0100", "0010", "0010", "0010", "0001"]), [3], "three b: c too late");
}

#[test]
fn a_repetition_from_zero_can_match_no_tick() {
    // `b [*0:1] ##1 c` from tick t: c at t, or b at t and c at t + 1.
    let p = "p: assert property (a |=> b [*0:1] ##1 c);";
    assert_eq!(fails(p, &["0100", "0001", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0010", "0001"]), NONE);
    assert_eq!(fails(p, &["0100", "0000", "0001"]), [2]);
    // `a ##1 b [*0:1]`: a alone, or a then b.
    let q = "p: assert property (a ##1 b [*0:1] |-> c);";
    assert_eq!(fails(q, &["0101", "0011", "0000"]), NONE);
    assert_eq!(fails(q, &["0100", "0011", "0000"]), [1], "c missing with a");
    assert_eq!(fails(q, &["0101", "0010", "0000"]), [1], "c missing with b");
    assert_eq!(fails(q, &["0101", "0000", "0000"]), NONE);
    // The empty match of the antecedent of `|=>` ends before the attempt's tick, so c is due
    // at every tick; and one tick after each b.
    let r = "p: assert property (b [*0:1] |=> c);";
    assert_eq!(fails(r, &["0001", "0011", "0000", "0001", "0001"]), [2, 3]);
    assert_eq!(fails(r, &["0001", "0011", "0001", "0001"]), NONE);
    // With `|->` the empty match has no last tick to start the consequent at: only b obliges.
    let s = "p: assert property (b [*0:1] |-> c);";
    assert_eq!(fails(s, &["0000", "0010", "0011", "0000"]), [1]);
    // `a ##0 empty` is no match, so `a ##0 b [*0:1]` is a and b together.
    let t = "p: assert property (a ##0 b [*0:1] |=> c);";
    assert_eq!(fails(t, &["0100", "0000", "0000"]), NONE);
    assert_eq!(fails(t, &["0110", "0000", "0000"]), [1]);
    assert_eq!(fails(t, &["0110", "0001", "0000"]), NONE);
}

#[test]
fn sequence_or_in_the_consequent_and_in_the_antecedent() {
    let p = "p: assert property (a |-> (b or (##2 c)));";
    assert_eq!(fails(p, &["0110", "0000", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "0000", "0001"]), NONE);
    assert_eq!(fails(p, &["0100", "0010", "0000"]), [2]);

    let q = "p: assert property ((a or (b ##1 b)) |=> c);";
    // a at 0: c due at 1 (there). b at 0 and 1: c due at 2 (missing).
    assert_eq!(fails(q, &["0110", "0011", "0000", "0000"]), [2]);
    assert_eq!(fails(q, &["0100", "0000", "0000", "0000"]), [2], "decided at the longest match's end");
    assert_eq!(fails(q, &["0100", "0001", "0000", "0000"]), NONE);
    assert_eq!(fails(q, &["0010", "0010", "0001", "0000"]), NONE);
}

#[test]
fn sequence_and_ends_with_the_later_operand() {
    let p = "p: assert property (a |-> ((b ##1 b) and (c ##2 c)));";
    assert_eq!(fails(p, &["0111", "0010", "0001"]), NONE);
    assert_eq!(fails(p, &["0111", "0000", "0001"]), [2]);
    assert_eq!(fails(p, &["0111", "0010", "0000"]), [2]);
    // As an antecedent it ends at 1, where b ##1 b does: c is due at 2.
    let q = "p: assert property ((a and (b ##1 b)) |=> c);";
    assert_eq!(fails(q, &["0110", "0010", "0000"]), [2]);
    assert_eq!(fails(q, &["0110", "0010", "0001"]), NONE);
    assert_eq!(fails(q, &["0010", "0110", "0000"]), NONE, "a has to start with it");
}

#[test]
fn intersect_needs_both_operands_to_end_together() {
    // Of the three lengths of the left operand only the three-tick one is left.
    let p = "p: assert property (a |-> ((b ##[1:3] c) intersect (1 [*3])));";
    assert_eq!(fails(p, &["0110", "0000", "0001"]), NONE);
    assert_eq!(fails(p, &["0110", "0001", "0000", "0001"]), [2], "c at 1 and 3 only");
    // The same antecedents with `and` in place of `intersect` match at 1 as well.
    let q = "p: assert property ((a ##[1:2] b) intersect (c ##2 c) |-> rst);";
    let r = "p: assert property ((a ##[1:2] b) and (c ##2 c) |-> rst);";
    assert_eq!(fails(q, &["0101", "0000", "0011"]), [2]);
    assert_eq!(fails(q, &["0101", "0010", "0001"]), NONE, "a ##1 b is one tick short");
    assert_eq!(fails(r, &["0101", "0010", "0001"]), [2]);
}

#[test]
fn throughout_holds_at_every_tick_of_the_match() {
    let p = "p: assert property (a |-> (b throughout (##2 c)));";
    assert_eq!(fails(p, &["0110", "0010", "0011"]), NONE);
    assert_eq!(fails(p, &["0110", "0000", "0011"]), [2], "broken in the middle");
    assert_eq!(fails(p, &["0110", "0010", "0001"]), [2], "broken at the last tick");
    assert_eq!(fails(p, &["0100", "0010", "0011"]), [2], "broken at the first tick");
    // With a range the condition covers only the ticks of the match taken.
    let q = "p: assert property (a |-> (b throughout (##[1:2] c)));";
    assert_eq!(fails(q, &["0110", "0011", "0000"]), NONE, "the match that ends at 1");
    assert_eq!(fails(q, &["0110", "0000", "0011"]), [2]);
    assert_eq!(fails(q, &["0110", "0010", "0011"]), NONE);
}

#[test]
fn within_needs_the_inner_match_inside_the_outer_one() {
    let p = "p: assert property (a |-> (b within (c ##2 c)));";
    assert_eq!(fails(p, &["0101", "0010", "0001"]), NONE);
    assert_eq!(fails(p, &["0111", "0000", "0001"]), NONE);
    assert_eq!(fails(p, &["0101", "0000", "0001", "0010"]), [2], "b after the outer match");
    assert_eq!(fails(p, &["0101", "0010", "0000"]), [2], "no outer match");
    // A two-tick inner sequence fits at 0-1 or at 1-2.
    let q = "p: assert property (a |-> ((b ##1 b) within (c [*3])));";
    assert_eq!(fails(q, &["0111", "0011", "0001"]), NONE);
    assert_eq!(fails(q, &["0101", "0011", "0011"]), NONE);
    assert_eq!(fails(q, &["0111", "0001", "0011"]), [2], "b at 0 and 2");
    assert_eq!(fails(q, &["0101", "0001", "0011", "0010"]), [2], "b at 2 and 3 runs over the end");
}

#[test]
fn first_match_keeps_only_the_earliest_end() {
    let all = "p: assert property (a ##[1:2] b |-> c);";
    let first = "p: assert property (first_match(a ##[1:2] b) |-> c);";
    // b at 1 and 2, c only at 1: the second match is not a first match.
    let rows = ["0100", "0011", "0010", "0000"];
    assert_eq!(fails(all, &rows), [2]);
    assert_eq!(fails(first, &rows), NONE);
    assert_eq!(fails(first, &["0100", "0000", "0010", "0000"]), [2], "the first match is the one at 2");
    assert_eq!(fails(first, &["0100", "0010", "0011", "0000"]), [2], "c is due at 1");
    // In a consequent: c has to follow the first b, not just some b.
    let any = "p: assert property (a |-> (##[0:2] b) ##1 c);";
    let one = "p: assert property (a |-> first_match(##[0:2] b) ##1 c);";
    let rows = ["0100", "0010", "0010", "0001"];
    assert_eq!(fails(any, &rows), NONE);
    assert_eq!(fails(one, &rows), [3]);
    assert_eq!(fails(one, &["0100", "0010", "0001", "0000"]), NONE);
}

#[test]
fn an_unknown_boolean_is_not_a_match() {
    // b unknown at 1 is no match there: the only match ends at 2, where c holds.
    let p = "p: assert property (a ##[1:2] b |-> c);";
    assert_eq!(fails(p, &["0100", "00x0", "0011", "0000"]), NONE);
    assert_eq!(fails(p, &["0100", "00x0", "0010", "0000"]), [2]);
    // Nor is it an earlier match for `first_match`: the first match is the one at 2.
    let q = "p: assert property (first_match(a ##[1:2] b) |-> c);";
    assert_eq!(fails(q, &["0100", "00x1", "0010", "0000"]), [2]);
    assert_eq!(fails(q, &["0100", "00x0", "0011", "0000"]), NONE);
    // An unknown consequent is not a match either.
    let r = "p: assert property (a |-> ##[1:2] b);";
    assert_eq!(fails(r, &["0100", "00x0", "00x0", "0000"]), [2]);
    assert_eq!(fails(r, &["0100", "00x0", "0010", "0000"]), NONE);
}

#[test]
fn cover_of_a_ranged_sequence_is_hit_when_a_match_exists() {
    let p = "p: cover property (a ##[1:2] b);";
    // Hit at the tick the attempt is decided: two after its start.
    assert_eq!(covers(p, &["0100", "0000", "0010", "0000"]), [2]);
    assert_eq!(covers(p, &["0100", "0010", "0000", "0000"]), [2]);
    assert_eq!(covers(p, &["0100", "0000", "0000", "0010"]), NONE);
    assert_eq!(covers(p, &["0010", "0010", "0010", "0010"]), NONE);
    // An implication is covered by a match of the antecedent whose consequent holds.
    let q = "p: cover property (a ##[1:2] b |-> c);";
    assert_eq!(covers(q, &["0100", "0010", "0001", "0000"]), NONE, "c not with b");
    assert_eq!(covers(q, &["0100", "0001", "0011", "0000"]), [2]);
    assert_eq!(covers(q, &["0001", "0001", "0001", "0001"]), NONE, "vacuous");
}

#[test]
fn assume_of_a_ranged_property_is_violated_like_the_assertion_fails() {
    let nl = compiled("p: assume property (a |-> ##[1:2] b);", 0);
    assert_eq!(nl.checks.iter().filter(|c| c.kind == CheckKind::Assume).count(), 1);
    assert_eq!(ticks(&nl, &["0100", "0000", "0000", "0000"], Watch::ViolatedAssumes), [2]);
    assert_eq!(ticks(&nl, &["0100", "0000", "0010", "0000"], Watch::ViolatedAssumes), NONE);
    assert_eq!(ticks(&nl, &["0100", "0000", "0000", "0000"], Watch::FailedAsserts), NONE);
}

#[test]
fn disable_iff_covers_the_whole_window_of_a_ranged_attempt() {
    let p = "p: assert property (disable iff (rst) a |-> ##[1:3] b);";
    let rows = ["0100", "0000", "0000", "0000", "0000"];
    assert_eq!(fails(p, &rows), [3]);
    for at in 0..5 {
        let with_rst: Vec<String> = rows
            .iter()
            .enumerate()
            .map(|(t, r)| if t == at { r.replacen('0', "1", 1) } else { r.to_string() })
            .collect();
        let with_rst: Vec<&str> = with_rst.iter().map(String::as_str).collect();
        // The attempt is decided at tick 3: a reset after that is too late.
        let expect: &[usize] = if at <= 3 { &[] } else { &[3] };
        assert_eq!(fails(p, &with_rst), expect, "reset at tick {at}");
    }
}

#[test]
fn skipped_cycles_and_a_condition_with_a_ranged_property() {
    let p = "p: assert property (a |-> ##[1:2] b);";
    let rows = ["0100", "0100", "0000", "0000"];
    assert_eq!(failures(&compiled(p, 0), &rows), [2, 3]);
    assert_eq!(failures(&compiled(p, 1), &rows), [3]);
    assert_eq!(failures(&compiled(p, 2), &rows), NONE);

    // Reached only where c holds; then a and, one or two ticks on, b.
    let q = "always @(posedge clk) if (c) p: assert property (a ##[1:2] b);";
    assert_eq!(fails(q, &["0101", "0000", "0010"]), NONE);
    assert_eq!(fails(q, &["0001", "0000", "0010"]), [2]);
    assert_eq!(fails(q, &["0100", "0000", "0000"]), NONE);
    // The condition leads every match of the antecedent.
    let r = "always @(posedge clk) if (a) p: assert property (b ##[1:2] b |-> c);";
    assert_eq!(fails(r, &["0110", "0010", "0011"]), [2], "c missing at 1");
    assert_eq!(fails(r, &["0110", "0011", "0010"]), [2], "c missing at 2");
    assert_eq!(fails(r, &["0010", "0010", "0010", "0010"]), NONE);
}

// A reference for bounded sequences, written from the definitions over a recorded trace
// rather than as conditions over history, to compare the compiled checks with.

/// A sequence over the signals `a`, `b`, `c` (0, 1, 2).
#[derive(Clone, Debug)]
enum Seq {
    Sig(usize),
    /// `lhs ##[min:max] rhs`.
    Then(Box<Seq>, u32, u32, Box<Seq>),
    /// `##[min:max] rhs`.
    After(u32, u32, Box<Seq>),
    /// `seq [*min:max]`.
    Times(Box<Seq>, u32, u32),
    Or(Box<Seq>, Box<Seq>),
    And(Box<Seq>, Box<Seq>),
    Intersect(Box<Seq>, Box<Seq>),
    Within(Box<Seq>, Box<Seq>),
    Throughout(usize, Box<Seq>),
    First(Box<Seq>),
}

type Trace = [[bool; 3]];

impl Seq {
    fn text(&self) -> String {
        let name = |i: &usize| ["a", "b", "c"][*i];
        match self {
            Seq::Sig(i) => name(i).to_string(),
            Seq::Then(l, min, max, r) => format!("(({}) ##[{min}:{max}] ({}))", l.text(), r.text()),
            Seq::After(min, max, r) => format!("(##[{min}:{max}] ({}))", r.text()),
            Seq::Times(s, min, max) => format!("(({}) [*{min}:{max}])", s.text()),
            Seq::Or(l, r) => format!("(({}) or ({}))", l.text(), r.text()),
            Seq::And(l, r) => format!("(({}) and ({}))", l.text(), r.text()),
            Seq::Intersect(l, r) => format!("(({}) intersect ({}))", l.text(), r.text()),
            Seq::Within(l, r) => format!("(({}) within ({}))", l.text(), r.text()),
            Seq::Throughout(i, s) => format!("({} throughout ({}))", name(i), s.text()),
            Seq::First(s) => format!("first_match({})", s.text()),
        }
    }

    /// Where the matches that start at `from` end: the tick after the last tick of each, so
    /// `from` itself for the empty match. A tick past the trace has every signal low.
    fn ends(&self, trace: &Trace, from: usize) -> BTreeSet<usize> {
        let mut out = BTreeSet::new();
        match self {
            Seq::Sig(i) => {
                if trace.get(from).is_some_and(|row| row[*i]) {
                    out.insert(from + 1);
                }
            }
            Seq::Then(l, min, max, r) => {
                for end in l.ends(trace, from) {
                    for n in *min as usize..=*max as usize {
                        // IEEE 1800-2023 16.9.2.1: `empty ##0 s` and `s ##0 empty` do not
                        // match; `empty ##n s` is `##(n-1) s`; `s ##n empty` is `s ##(n-1) 1`.
                        if n == 0 {
                            if end > from {
                                out.extend(r.ends(trace, end - 1).into_iter().filter(|&e| e > end - 1));
                            }
                        } else {
                            out.extend(r.ends(trace, end + n - 1));
                        }
                    }
                }
            }
            Seq::After(min, max, r) => {
                for n in *min as usize..=*max as usize {
                    out.extend(r.ends(trace, from + n));
                }
            }
            Seq::Times(s, min, max) => {
                let mut at = BTreeSet::from([from]);
                for count in 0..=*max {
                    if count >= *min {
                        out.extend(at.iter().copied());
                    }
                    at = at.iter().flat_map(|&t| s.ends(trace, t)).collect();
                }
            }
            Seq::Or(l, r) => {
                out = l.ends(trace, from);
                out.extend(r.ends(trace, from));
            }
            Seq::And(l, r) => {
                let r = r.ends(trace, from);
                for a in l.ends(trace, from) {
                    out.extend(r.iter().map(|&b| a.max(b)));
                }
            }
            Seq::Intersect(l, r) => {
                let r = r.ends(trace, from);
                out = l.ends(trace, from).into_iter().filter(|e| r.contains(e)).collect();
            }
            Seq::Within(inner, outer) => {
                for end in outer.ends(trace, from) {
                    if (from..=end).any(|t| inner.ends(trace, t).iter().any(|&e| e <= end)) {
                        out.insert(end);
                    }
                }
            }
            Seq::Throughout(i, s) => {
                out = s.ends(trace, from);
                out.retain(|&end| (from..end).all(|t| trace.get(t).is_some_and(|row| row[*i])));
            }
            Seq::First(s) => out.extend(s.ends(trace, from).first()),
        }
        out
    }

    /// The lengths a match can have, whatever the trace.
    fn lengths(&self) -> BTreeSet<usize> {
        let pairs = |l: &Seq, r: &Seq| -> Vec<(usize, usize)> {
            let r = r.lengths();
            l.lengths().into_iter().flat_map(|a| r.iter().map(move |&b| (a, b))).collect()
        };
        match self {
            Seq::Sig(_) => BTreeSet::from([1]),
            Seq::Then(l, min, max, r) => {
                let mut out = BTreeSet::new();
                for (a, b) in pairs(l, r) {
                    for n in *min as usize..=*max as usize {
                        if n > 0 {
                            out.insert(a + n - 1 + b);
                        } else if a > 0 && b > 0 {
                            out.insert(a + b - 1);
                        }
                    }
                }
                out
            }
            Seq::After(min, max, r) => {
                let r = r.lengths();
                (*min as usize..=*max as usize).flat_map(|n| r.iter().map(move |&b| n + b)).collect()
            }
            Seq::Times(s, min, max) => {
                let (one, mut total, mut out) = (s.lengths(), BTreeSet::from([0]), BTreeSet::new());
                for count in 0..=*max {
                    if count >= *min {
                        out.extend(total.iter().copied());
                    }
                    total = total.iter().flat_map(|&t| one.iter().map(move |&l| t + l)).collect();
                }
                out
            }
            Seq::Or(l, r) => l.lengths().union(&r.lengths()).copied().collect(),
            Seq::And(l, r) => pairs(l, r).into_iter().map(|(a, b)| a.max(b)).collect(),
            Seq::Intersect(l, r) => l.lengths().intersection(&r.lengths()).copied().collect(),
            Seq::Within(inner, outer) => match inner.lengths().first() {
                Some(&least) => outer.lengths().into_iter().filter(|&l| l >= least).collect(),
                None => BTreeSet::new(),
            },
            Seq::Throughout(_, s) | Seq::First(s) => s.lengths(),
        }
    }
}

/// `antecedent |-> consequent` (`|=>` when not `overlapped`), or the consequent alone.
struct Prop {
    antecedent: Option<(Seq, bool)>,
    consequent: Seq,
}

impl Prop {
    fn text(&self) -> String {
        match &self.antecedent {
            Some((a, true)) => format!("{} |-> {}", a.text(), self.consequent.text()),
            Some((a, false)) => format!("{} |=> {}", a.text(), self.consequent.text()),
            None => self.consequent.text(),
        }
    }

    /// The ticks, after the attempt's start, at which the consequent can have to start.
    fn starts(&self) -> BTreeSet<usize> {
        match &self.antecedent {
            None => BTreeSet::from([0]),
            // `|->` starts it at the last tick of a match that is not empty; `|=>` one later.
            Some((a, true)) => a.lengths().into_iter().filter(|&l| l > 0).map(|l| l - 1).collect(),
            Some((a, false)) => a.lengths(),
        }
    }

    /// Whether the property is one to compile: IEEE 1800-2023 16.12.22 wants a sequence
    /// used as a property to have a match and no empty one, and an antecedent to have a
    /// match that starts the consequent.
    fn legal(&self) -> bool {
        let c = self.consequent.lengths();
        !c.is_empty() && !c.contains(&0) && !self.starts().is_empty()
    }

    /// From the attempt's start to the last tick any match of it reads.
    fn span(&self) -> usize {
        let longest = self.consequent.lengths().last().copied().unwrap_or(1);
        self.starts().last().copied().unwrap_or(0) + longest - 1
    }

    /// For the attempt that starts at `from`: whether an assertion holds, and whether a
    /// cover is hit.
    fn attempt(&self, trace: &Trace, from: usize) -> (bool, bool) {
        let held = |at: usize| !self.consequent.ends(trace, at).iter().all(|&e| e == at);
        let Some((a, overlapped)) = &self.antecedent else {
            return (held(from), held(from));
        };
        let ends = a.ends(trace, from);
        let due: Vec<usize> = if *overlapped {
            ends.into_iter().filter(|&e| e > from).map(|e| e - 1).collect()
        } else {
            ends.into_iter().collect()
        };
        (due.iter().all(|&at| held(at)), due.iter().any(|&at| held(at)))
    }
}

/// A small deterministic generator (xorshift).
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) % n
    }

    fn range(&mut self, least: u32) -> (u32, u32) {
        let min = least + self.below(3 - u64::from(least)) as u32;
        (min, min + self.below(3) as u32)
    }

    /// A random sequence. With `empty`, repetitions can be `[*0:n]`; `first_match` is then
    /// left out, because the reference's [`Seq::lengths`] does not model which of its
    /// matches a match that always exists hides.
    fn seq(&mut self, depth: u32, empty: bool) -> Seq {
        if depth == 0 || self.below(4) == 0 {
            return Seq::Sig(self.below(3) as usize);
        }
        let sub = |rng: &mut Rng| Box::new(rng.seq(depth - 1, empty));
        match self.below(if empty { 10 } else { 11 }) {
            0..=2 => {
                let (min, max) = self.range(0);
                Seq::Then(sub(self), min, max, sub(self))
            }
            3 => {
                let (min, max) = self.range(0);
                Seq::After(min, max, sub(self))
            }
            4 => {
                let (min, max) = self.range(if empty { 0 } else { 1 });
                Seq::Times(sub(self), min, max)
            }
            5 => Seq::Or(sub(self), sub(self)),
            6 => Seq::And(sub(self), sub(self)),
            7 => Seq::Intersect(sub(self), sub(self)),
            8 => Seq::Within(sub(self), sub(self)),
            9 => Seq::Throughout(self.below(3) as usize, sub(self)),
            // Not over a lone boolean: the parser does not take `(first_match(a))` yet.
            _ => match sub(self) {
                s if matches!(*s, Seq::Sig(_) | Seq::First(_)) => *s,
                s => Seq::First(s),
            },
        }
    }
}

#[test]
fn random_bounded_properties_agree_with_the_definitions() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut checked, mut failed, mut passed, mut hit, mut illegal) = (0, 0, 0, 0, 0);
    for round in 0..400 {
        let empty = round % 4 == 3;
        let consequent = rng.seq(2, empty);
        let antecedent = match rng.below(3) {
            0 => None,
            n => Some((rng.seq(2, empty), n == 1)),
        };
        let prop = Prop { antecedent, consequent };
        let text = prop.text();
        let src = format!(
            "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
             p: assert property ({text});\n q: cover property ({text});\nendmodule\n"
        );
        let mut nl = lower_src(&src).unwrap_netlist();
        let outcomes = nl.compile_properties(&PropertyOptions::default());
        if !prop.legal() {
            assert!(outcomes.iter().all(|o| matches!(o, PropertyOutcome::Unsupported(_))), "{text}: {outcomes:?}");
            illegal += 1;
            continue;
        }
        assert!(outcomes.iter().all(|o| matches!(o, PropertyOutcome::Compiled { .. })), "{text}: {outcomes:?}");

        // Signals are high more often than low, so that long sequences do match.
        let trace: Vec<[bool; 3]> = (0..48).map(|_| [rng.below(8) < 5, rng.below(8) < 5, rng.below(8) < 5]).collect();
        let rows: Vec<String> = trace.iter().map(|r| format!("0{}{}{}", r[0] as u8, r[1] as u8, r[2] as u8)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let span = prop.span();
        let (mut want_fail, mut want_hit) = (Vec::new(), Vec::new());
        for tick in span..trace.len() {
            let (holds, covered) = prop.attempt(&trace, tick - span);
            if !holds {
                want_fail.push(tick);
            }
            if covered {
                want_hit.push(tick);
            }
        }
        assert_eq!(ticks(&nl, &rows, Watch::FailedAsserts), want_fail, "assert {text}\n{rows:?}");
        assert_eq!(ticks(&nl, &rows, Watch::HitCovers), want_hit, "cover {text}\n{rows:?}");
        checked += 1;
        failed += want_fail.len();
        passed += trace.len() - span - want_fail.len();
        hit += want_hit.len();
    }
    // The comparison is not a vacuous one.
    assert!(checked >= 250 && illegal >= 5, "{checked} checked, {illegal} illegal");
    assert!(failed >= 1000 && passed >= 1000 && hit >= 1000, "{failed} failed, {passed} passed, {hit} hit");
}

#[test]
fn a_skip_can_be_given_per_clock_and_edge() {
    let src = "module m(input logic clk, clk2, a, b);\n\
               p: assert property (@(posedge clk) a |-> b);\n\
               n: assert property (@(negedge clk) a |-> b);\n\
               q: assert property (@(posedge clk2) a |-> b);\n\
               endmodule\n";
    let mut nl = lower_src(src).unwrap_netlist();
    let mut opts = PropertyOptions::default();
    opts.skip_cycles = 1;
    opts.skip_by_clock = vec![
        sv_netlist::ClockSkip { input: "clk".into(), posedge: true, ticks: 3 },
        sv_netlist::ClockSkip { input: "clk2".into(), posedge: true, ticks: 0 },
    ];
    nl.compile_properties(&opts);
    // The tick counter of each clock and edge counts to the skip of that clock and edge:
    // 3 for posedge clk (2 bits), the default 1 for negedge clk, and none for clk2.
    let widths: Vec<u32> = nl.states.iter().filter(|s| s.name.starts_with("m._sva_tick")).map(|s| s.width).collect();
    assert_eq!(widths, [2, 1], "{:?}", nl.states);
}

#[test]
fn until_holds_its_left_side_up_to_the_tick_before_its_end() {
    const NONE: [usize; 0] = [];
    let p = "p: assert property (a |-> b until c);";
    // Started at 0, b held at 0 and 1, c ends it at 2; nothing is owed after that.
    assert_eq!(fails(p, &["0110", "0010", "0001", "0000"]), NONE);
    // b dropped at 1 before c came.
    assert_eq!(fails(p, &["0110", "0000", "0001"]), [1]);
    // It stays owed for as long as c stays away, and fails at each such tick.
    assert_eq!(fails(p, &["0110", "0010", "0000", "0000", "0001", "0000"]), [2, 3]);
    // At the starting tick itself: b or c.
    assert_eq!(fails(p, &["0100", "0000"]), [0, 1]);
    assert_eq!(fails(p, &["0101", "0000"]), NONE);
    // No start, nothing owed.
    assert_eq!(fails(p, &["0000", "0000"]), NONE);
}

#[test]
fn until_with_needs_its_left_side_at_the_end_tick_too() {
    const NONE: [usize; 0] = [];
    let with = "p: assert property (a |-> b until_with c);";
    let plain = "p: assert property (a |-> b until c);";
    let rows = ["0110", "0001", "0000"];
    assert_eq!(fails(plain, &rows), NONE);
    assert_eq!(fails(with, &rows), [1]);
    assert_eq!(fails(with, &["0110", "0011", "0000"]), NONE);
}

#[test]
fn until_after_an_antecedent_sequence_and_next_cycle_implication() {
    const NONE: [usize; 0] = [];
    // `|=>`: owed from the tick after a.
    let next = "p: assert property (a |=> b until c);";
    assert_eq!(fails(next, &["0100", "0000", "0001"]), [1]);
    assert_eq!(fails(next, &["0100", "0010", "0001", "0000"]), NONE);
    // The antecedent `a ##1 a` ends at tick 1: owed from there.
    let seq = "p: assert property (a ##1 a |-> b until c);";
    assert_eq!(fails(seq, &["0100", "0100", "0000", "0001"]), [1, 2]);
    assert_eq!(fails(seq, &["0100", "0000", "0000"]), NONE);
    // Without an antecedent an `until` starts at every tick.
    let bare = "p: assert property (b until c);";
    assert_eq!(fails(bare, &["0010", "0000", "0001", "0010"]), [1]);
}

#[test]
fn until_is_dropped_by_disable_iff_and_waits_out_the_skipped_cycles() {
    const NONE: [usize; 0] = [];
    let p = "p: assert property (disable iff (rst) a |-> b until c);";
    // Owed from 0; the reset at 1 drops it for good.
    assert_eq!(fails(p, &["0110", "1000", "0000", "0000"]), NONE);
    assert_eq!(fails(p, &["0110", "0000", "0000"]), [1, 2]);
    // An attempt in a skipped cycle is never owed.
    let q = "p: assert property (a |-> b until c);";
    assert_eq!(failures(&compiled(q, 1), &["0110", "0000", "0000"]), NONE);
    assert_eq!(failures(&compiled(q, 1), &["0000", "0110", "0000"]), [2]);
}

#[test]
fn until_forms_that_are_not_compiled() {
    let src = format!(
        "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
         u1: assert property (a |-> b s_until c);\n\
         u2: cover property (a |-> b until c);\n\
         u3: assert property (a |-> (b ##1 b) until c);\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    let why: Vec<&str> = out
        .iter()
        .map(|o| match o {
            PropertyOutcome::Unsupported(w) => w.as_str(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(why[0].contains("s_until"), "{why:?}");
    assert!(why[1].contains("cover of `until`"), "{why:?}");
    assert!(why[2].contains("not single booleans"), "{why:?}");
}
