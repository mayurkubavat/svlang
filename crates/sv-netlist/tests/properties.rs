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
         a_unb_del: assert property (a |-> ##[1:$] b);\n\
         a_unb_rep: assert property (a [*1:$] |-> b);\n\
         a_goto: assert property (a |-> b [->1] ##1 c);\n\
         a_nonconsec: assert property (a |-> b [=1:2] ##1 c);\n\
         a_not: assert property (not a);\n\
         a_fby: assert property ((a ##1 b) #-# c);\n\
         a_abort: assert property (sync_accept_on(c) (a ##1 b));\n\
         a_alw: assert property (a |-> always b);\n\
         a_alw_comp: assert property (a |-> always (b ##1 c));\n\
         a_imp: assert property (a implies b);\n\
         a_iff: assert property (a iff b);\n\
         a_ifelse: assert property (if (a) (b |-> ##1 c) else c);\n\
         a_snext: assert property (s_nexttime [2] (a |-> ##1 b));\n\
         u1: assert property ((a [*1:$]) intersect (b [*1:$]) |-> c);\n\
         u4: assert property (@(clk) a);\n\
         u5: assert property ((a ##1 b) [->1] |-> c);\n\
         u7: assert property ((a ##1 b) [=1] |-> c);\n\
         u8: assert property ((a [*1:$]) within (b [*1:$]) |-> c);\n\
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
    assert_eq!(why("m.u1"), "sequence `intersect` with an unbounded operand");
    assert_eq!(why("m.u4"), "a clocking event on both edges");
    assert_eq!(why("m.u5"), "goto repetition [->] on a multi-cycle sequence");
    assert_eq!(why("m.u7"), "non-consecutive repetition [=] on a multi-cycle sequence");
    assert_eq!(why("m.u8"), "sequence `within` with an unbounded operand");
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
    assert_eq!(check("m.a_unb_del"), CheckKind::Assert);
    assert_eq!(check("m.a_unb_rep"), CheckKind::Assert);
    assert_eq!(check("m.a_goto"), CheckKind::Assert);
    assert_eq!(check("m.a_alw_comp"), CheckKind::Assert);
    assert_eq!(check("m.a_imp"), CheckKind::Assert);
    assert_eq!(check("m.a_iff"), CheckKind::Assert);
    assert_eq!(check("m.a_ifelse"), CheckKind::Assert);
    assert_eq!(check("m.a_snext"), CheckKind::Assert);
    assert_eq!(check("m.a_nonconsec"), CheckKind::Assert);
    assert_eq!(check("m.a_not"), CheckKind::Assert);
    assert_eq!(check("m.a_fby"), CheckKind::Assert);
    assert_eq!(check("m.a_abort"), CheckKind::Assert);

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
         u1: assert property (a |-> s_eventually (b until c));\n\
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
    assert!(why[0].contains("until"), "{why:?}");
    assert!(why[1].contains("cover of `until`"), "{why:?}");
    assert!(why[2].contains("not single booleans"), "{why:?}");
}

#[test]
fn s_eventually_is_a_liveness_obligation_with_a_waiting_monitor() {
    let src = format!(
        "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
         live: assert property (disable iff (rst) a |-> s_eventually b);\n\
         fair: assume property (s_eventually c);\n\
         both: assert property (a |-> ##[1:2] b);\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert_eq!(out[0], PropertyOutcome::Liveness { index: 0 });
    assert_eq!(out[1], PropertyOutcome::Liveness { index: 1 });
    assert!(matches!(out[2], PropertyOutcome::Compiled { .. }));
    assert_eq!(nl.property_liveness, [Some(0), Some(1), None]);
    assert_eq!(nl.liveness[0].label.as_deref(), Some("m.live"));
    assert!(!nl.liveness[0].assume && nl.liveness[1].assume);
    // No check is made for them: a liveness property has no finite failure.
    assert_eq!(nl.checks.len(), 1);

    // The monitor of `live` waits from the tick a holds without b to the tick b holds,
    // and a reset drops it. Its state shows the wait from the tick after.
    let waiting = nl.states.iter().position(|s| s.name == "m._sva_until").expect("the monitor");
    let mut sim = NetlistSim::new(&nl);
    let mut seen = Vec::new();
    for row in ["0100", "0000", "0010", "0000", "0100", "1000", "0000"] {
        for (port, bit) in ["rst", "a", "b", "c"].iter().zip(row.chars()) {
            sim.set_u(port, i128::from(bit == '1'));
        }
        sim.tick_posedge("clk");
        seen.push(sim.states[waiting].is_true());
    }
    assert_eq!(seen, [true, true, false, false, true, false, false]);
    // The assertion's signal is a watch bit that only the free pick input can set.
    assert!(nl.undriven.iter().any(|u| u.signal == "m._sva_pick"));
    assert!(nl.states.iter().any(|s| s.name == "m._sva_watch"));
}

#[test]
fn s_until_is_the_check_of_until_and_a_liveness_obligation() {
    const NONE: [usize; 0] = [];
    let src = format!(
        "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
         p: assert property (a |-> b s_until c);\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert!(matches!(out[0], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert_eq!(nl.property_liveness, [Some(0)]);
    assert_eq!(nl.liveness[0].label.as_deref(), Some("m.p"));
    assert_eq!(nl.checks[nl.property_checks[0].unwrap()].label.as_deref(), Some("m.p"));
    // The safety part is that of `until`.
    assert_eq!(failures(&nl, &["0110", "0010", "0001", "0000"]), NONE);
    assert_eq!(failures(&nl, &["0110", "0000", "0001"]), [1]);
}

#[test]
fn the_bounded_property_operators_are_sequences() {
    const NONE: [usize; 0] = [];
    // nexttime [N] b is ##N b.
    let next = "p: assert property (a |-> nexttime b);";
    assert_eq!(fails(next, &["0100", "0000", "0000"]), [1]);
    assert_eq!(fails(next, &["0100", "0010", "0000"]), NONE);
    let next2 = "p: assert property (a |-> nexttime [2] b);";
    assert_eq!(fails(next2, &["0100", "0010", "0000"]), [2]);
    assert_eq!(fails(next2, &["0100", "0000", "0010"]), NONE);
    // s_eventually [M:N] b is ##[M:N] b: some tick of the window.
    let soon = "p: assert property (a |-> s_eventually [1:2] b);";
    assert_eq!(fails(soon, &["0110", "0000", "0000"]), [2]);
    assert_eq!(fails(soon, &["0100", "0000", "0010"]), NONE);
    assert_eq!(fails(soon, &["0100", "0010", "0000"]), NONE);
    // always [M:N] b: every tick of the window, decided at its end.
    let all = "p: assert property (a |-> always [0:2] b);";
    assert_eq!(fails(all, &["0110", "0010", "0010", "0000"]), NONE);
    assert_eq!(fails(all, &["0110", "0000", "0010", "0000"]), [2]);
    assert_eq!(fails(all, &["0110", "0010", "0000", "0000"]), [2]);
    let later = "p: assert property (a |-> always [1:2] b);";
    assert_eq!(fails(later, &["0100", "0010", "0010"]), NONE);
    assert_eq!(fails(later, &["0110", "0010", "0000"]), [2]);
}

#[test]
fn goto_repetition_in_antecedent_consequent_cover_and_four_state() {
    const NONE: [usize; 0] = [];
    // Antecedent `a [->2] |-> b`: fires on the exact tick of the 2nd `a` (even across arbitrary `!a` gaps).
    let ant = "p: assert property (a [->2] |-> b);";
    assert_eq!(fails(ant, &["0100", "0000", "0000", "0110", "0000"]), NONE);
    assert_eq!(fails(ant, &["0100", "0000", "0000", "0100", "0010"]), [3]);

    // Consequent `a |-> b [->2] ##1 c`: waits for 2nd `b`, then requires `c` on the very next tick!
    let cons = "p: assert property (disable iff (rst) a |-> b [->2] ##1 c);";
    assert_eq!(fails(cons, &["0100", "0010", "0000", "0010", "0001"]), NONE);
    assert_eq!(fails(cons, &["0100", "0010", "0000", "0010", "0000"]), [4]);
    // Reset in the middle of waiting drops the obligation cleanly.
    assert_eq!(fails(cons, &["0100", "0010", "1000", "0010", "0000"]), NONE);

    // 4-state `'x'` satisfies neither `b` nor `!b` in `b [->1] ##1 c` (`(!b)[*0:$] ##1 b ##1 c`):
    let four = "p: assert property (a |-> b [->1] ##1 c);";
    assert_eq!(fails(four, &["0100", "00x0", "0010", "0001"]), [1]);

    // Cover of `a ##1 b [->2]`: hits on the 2nd `b` after `a`.
    let cov = "p: cover property (a ##1 b [->2]);";
    assert_eq!(covers(cov, &["0100", "0000", "0010", "0000", "0010", "0000"]), [4]);
}

#[test]
fn non_consecutive_repetition_in_antecedent_consequent_and_empty_bounds() {
    const NONE: [usize; 0] = [];
    // `b [=1:2]` matches after 1 or 2 `b`s AND across trailing `!b` ticks, but dies on a 3rd `b`!
    let p = "p: assert property (a |-> b [=1:2] ##1 c);";
    // 1 `b` at tick 1, trailing `!b` at tick 2, then `c` at tick 3 -> passes!
    assert_eq!(fails(p, &["0100", "0010", "0000", "0001"]), NONE);
    // 2 `b`s at ticks 1 and 3, trailing `!b` at tick 4, then `c` at tick 5 -> passes!
    assert_eq!(fails(p, &["0100", "0010", "0000", "0010", "0000", "0001"]), NONE);
    // 3rd `b` at tick 3 without `c` having held at tick 2 or tick 3 -> fails at tick 3!
    assert_eq!(fails(p, &["0100", "0010", "0010", "0010", "0001"]), [3]);

    // `b [=0:1] ##1 c`: allows 0 `b`s (all `!b` before `c`) or 1 `b` before `c`, but fails if 2 `b`s precede `c`!
    let z = "p: assert property (a |=> b [=0:1] ##1 c);";
    assert_eq!(fails(z, &["0100", "0001", "0000"]), NONE);
    assert_eq!(fails(z, &["0100", "0000", "0010", "0001"]), NONE);
    assert_eq!(fails(z, &["0100", "0010", "0010", "0001"]), [2]);

    // `b [=0:1]` alone as consequent admits empty match -> rejected per IEEE 1800 §16.12.22.
    let src = format!("module m({PORTS}); default clocking @(posedge clk); endclocking p: assert property (a |-> b [=0:1]); endmodule\n");
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert!(matches!(&out[0], PropertyOutcome::Unsupported(w) if w.contains("empty match")), "{out:?}");
}

#[test]
fn unbounded_delay_and_consecutive_repetition_with_strong_liveness_and_related_covers() {
    const NONE: [usize; 0] = [];
    // `(a ##1 b) [*2:$] |-> c`: multi-cycle nested operand repeated unboundedly!
    let p = "p: assert property ((a ##1 b) [*2:$] |-> c);";
    // 1st `a ##1 b` at 0..1 (no obligation yet), 2nd at 2..3 (c=1 -> ok), 3rd at 4..5 (c=0 -> fails at 5!).
    assert_eq!(fails(p, &["0100", "0010", "0100", "0011", "0100", "0010"]), [5]);

    // `a |-> ##[1:$] b` (weak) has no finite failure; `s_eventually [1:$] c` is pure Liveness;
    // on 4-state `logic b`, `strong(b [->2])` has both a finite `'x'` failure check AND Liveness,
    // while on 2-state `bit b2`, `strong(b2 [->2])` is pure Liveness!
    let src = format!(
        "module m({PORTS}, input bit b2);\n default clocking @(posedge clk); endclocking\n\
         w_weak: assert property (a |-> ##[1:$] b);\n\
         s_seq4: assert property (a |-> strong(b [->2]));\n\
         s_seq2: assert property (a |-> strong(b2 [->2]));\n\
         s_ev: assert property (a |-> s_eventually [1:$] c);\n\
         s_both: assert property (a |-> strong(b2 [->1] ##1 c));\n\
         f_assm: assume property (a |-> strong(b2 [->1]));\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let mut opts = PropertyOptions::default();
    opts.precondition_covers = true;
    opts.witness_covers = true;
    let out = nl.compile_properties(&opts);
    assert!(matches!(out[0], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[1], PropertyOutcome::Compiled { .. }) && nl.property_liveness[1].is_some(), "{out:?}");
    assert!(matches!(out[2], PropertyOutcome::Liveness { .. }), "{out:?}");
    assert!(matches!(out[3], PropertyOutcome::Liveness { .. }), "{out:?}");
    // `s_both` has both a finite safety failure (`c` missing after first `b2`) AND a liveness obligation!
    assert!(matches!(out[4], PropertyOutcome::Compiled { .. }) && nl.property_liveness[4].is_some(), "{out:?}");
    assert!(matches!(out[5], PropertyOutcome::Liveness { .. }), "{out:?}");
    assert!(nl.liveness[nl.property_liveness[5].unwrap()].assume);

    // Related `:precondition` and `:witness` covers are emitted for automaton safety assertions (`w_weak`).
    let labels: Vec<&str> = nl.checks.iter().filter_map(|c| c.label.as_deref()).collect();
    assert!(labels.contains(&"m.w_weak:precondition"), "{labels:?}");
    assert!(labels.contains(&"m.w_weak:witness"), "{labels:?}");
    let mut sim = NetlistSim::new(&nl);
    for _ in 0..4 {
        sim.set_u("rst", 0);
        sim.set_u("a", 1);
        sim.set_u("b", 0);
        sim.set_u("c", 0);
        sim.set_u("b2", 0);
        sim.tick_posedge("clk");
    }
    assert!(sim.failed_asserts.is_empty());

    // Unbounded antecedent feeding `until`: `a [->2] |-> b until c`.
    let u = "p: assert property (a [->2] |-> b until c);";
    assert_eq!(fails(u, &["0100", "0000", "0110", "0010", "0001"]), NONE);
    assert_eq!(fails(u, &["0100", "0000", "0110", "0000", "0001"]), [3]);
}

/// Reference AST for randomized unbounded / `[->]` / `[=]` Annex F trace equivalence checking.
#[derive(Clone, Debug)]
enum AutoSeq {
    Sig(usize),
    Delay(Box<AutoSeq>, u32, Option<u32>, Box<AutoSeq>),
    LeadDelay(u32, Option<u32>, Box<AutoSeq>),
    Consec(Box<AutoSeq>, u32, Option<u32>),
    Goto(usize, u32, Option<u32>),
    NonConsec(usize, u32, Option<u32>),
    Or(Box<AutoSeq>, Box<AutoSeq>),
    Throughout(usize, Box<AutoSeq>),
}

impl AutoSeq {
    fn fmt_bound(b: Option<u32>) -> String {
        match b {
            Some(n) => n.to_string(),
            None => "$".into(),
        }
    }

    fn text(&self) -> String {
        let name = |i: &usize| ["a", "b", "c"][*i];
        match self {
            AutoSeq::Sig(i) => name(i).to_string(),
            AutoSeq::Delay(l, min, max, r) => {
                format!("(({}) ##[{min}:{}] ({}))", l.text(), Self::fmt_bound(*max), r.text())
            }
            AutoSeq::LeadDelay(min, max, r) => format!("(##[{min}:{}] ({}))", Self::fmt_bound(*max), r.text()),
            AutoSeq::Consec(s, min, max) => format!("(({}) [*{min}:{}])", s.text(), Self::fmt_bound(*max)),
            AutoSeq::Goto(i, min, max) => format!("({} [->{min}:{}])", name(i), Self::fmt_bound(*max)),
            AutoSeq::NonConsec(i, min, max) => format!("({} [={min}:{}])", name(i), Self::fmt_bound(*max)),
            AutoSeq::Or(l, r) => format!("(({}) or ({}))", l.text(), r.text()),
            AutoSeq::Throughout(i, s) => format!("({} throughout ({}))", name(i), s.text()),
        }
    }

    /// Exact Annex F tight-satisfaction match ends in `from ..= trace.len()` on finite `trace`.
    fn ends(&self, trace: &Trace, from: usize) -> BTreeSet<usize> {
        let limit = trace.len();
        let mut out = BTreeSet::new();
        if from > limit {
            return out;
        }
        match self {
            AutoSeq::Sig(i) => {
                if from < limit && trace[from][*i] {
                    out.insert(from + 1);
                }
            }
            AutoSeq::Delay(l, min, max, r) => {
                let max_n = max.map_or(limit, |m| (m as usize).min(limit));
                for end in l.ends(trace, from) {
                    for n in (*min as usize)..=max_n {
                        if n == 0 {
                            if end > from {
                                out.extend(r.ends(trace, end - 1).into_iter().filter(|&e| e > end - 1));
                            }
                        } else if end + n - 1 <= limit {
                            out.extend(r.ends(trace, end + n - 1));
                        }
                    }
                }
            }
            AutoSeq::LeadDelay(min, max, r) => {
                let max_n = max.map_or(limit, |m| (m as usize).min(limit));
                for n in (*min as usize)..=max_n {
                    if from + n <= limit {
                        out.extend(r.ends(trace, from + n));
                    }
                }
            }
            AutoSeq::Consec(s, min, max) => {
                let cap = max.map_or(limit + 1, |m| m as usize);
                let mut at = BTreeSet::from([from]);
                for count in 0..=cap {
                    if count >= *min as usize {
                        out.extend(at.iter().copied());
                    }
                    if count == cap {
                        break;
                    }
                    let next: BTreeSet<usize> =
                        at.iter().flat_map(|&t| s.ends(trace, t)).filter(|&e| e <= limit).collect();
                    if next.is_empty() || (count >= *min as usize && next.is_subset(&out) && next == at) {
                        break;
                    }
                    at = next;
                }
            }
            AutoSeq::Goto(i, min, max) => {
                let mut ones = 0u32;
                for (t, row) in trace.iter().enumerate().take(limit).skip(from) {
                    if row[*i] {
                        ones += 1;
                        if ones >= *min && max.is_none_or(|mx| ones <= mx) {
                            out.insert(t + 1);
                        }
                        if max.is_some_and(|mx| ones >= mx) {
                            break;
                        }
                    }
                }
            }
            AutoSeq::NonConsec(i, min, max) => {
                if *min == 0 {
                    out.insert(from);
                }
                let mut ones = 0u32;
                for (t, row) in trace.iter().enumerate().take(limit).skip(from) {
                    if row[*i] {
                        ones += 1;
                    }
                    if max.is_some_and(|mx| ones > mx) {
                        break;
                    }
                    if ones >= *min {
                        out.insert(t + 1);
                    }
                }
            }
            AutoSeq::Or(l, r) => {
                out = l.ends(trace, from);
                out.extend(r.ends(trace, from));
            }
            AutoSeq::Throughout(i, s) => {
                out = s.ends(trace, from);
                out.retain(|&end| (from..end).all(|t| t < limit && trace[t][*i]));
            }
        }
        out
    }

    fn admits_empty(&self) -> bool {
        self.ends(&[], 0).contains(&0)
    }

    /// Checks whether `prefix[from..]` can still achieve a non-empty match of `self` either on
    /// `prefix` itself (`end <= prefix.len()`) or under some future suffix of up to `horizon` ticks.
    fn can_match_from(&self, prefix: &Trace, from: usize, horizon: usize) -> bool {
        if self.ends(prefix, from).iter().any(|&e| e > from) {
            return true;
        }
        // BFS / DFS over suffixes of length `1..=horizon`:
        fn dfs(seq: &AutoSeq, buf: &mut Vec<[bool; 3]>, from: usize, rem: usize) -> bool {
            if rem == 0 {
                return false;
            }
            for bits in 0..8u8 {
                buf.push([(bits & 1) != 0, (bits & 2) != 0, (bits & 4) != 0]);
                let matched = seq.ends(buf, from).iter().any(|&e| e > from);
                if matched || dfs(seq, buf, from, rem - 1) {
                    buf.pop();
                    return true;
                }
                buf.pop();
            }
            false
        }
        let mut buf = prefix.to_vec();
        dfs(self, &mut buf, from, horizon)
    }
}

impl Rng {
    fn auto_bound(&mut self, min: u32) -> Option<u32> {
        if self.below(3) == 0 {
            None
        } else {
            Some(min + self.below(2) as u32)
        }
    }

    fn auto_seq(&mut self, depth: u32, allow_empty: bool) -> AutoSeq {
        if depth == 0 {
            return match self.below(3) {
                0 => AutoSeq::Sig(self.below(3) as usize),
                1 => {
                    let min = 1 + self.below(2) as u32;
                    let max = self.auto_bound(min);
                    AutoSeq::Goto(self.below(3) as usize, min, max)
                }
                _ => {
                    let min = if allow_empty && self.below(3) == 0 { 0 } else { 1 + self.below(2) as u32 };
                    let max = self.auto_bound(min);
                    AutoSeq::NonConsec(self.below(3) as usize, min, max)
                }
            };
        }
        let sub = |rng: &mut Rng, ae: bool| Box::new(rng.auto_seq(depth - 1, ae));
        match self.below(7) {
            0 | 1 => {
                let min = self.below(3) as u32;
                let max = self.auto_bound(min);
                AutoSeq::Delay(sub(self, allow_empty), min, max, sub(self, false))
            }
            2 => {
                let min = if allow_empty { self.below(2) as u32 } else { 1 + self.below(2) as u32 };
                let max = self.auto_bound(min);
                AutoSeq::LeadDelay(min, max, sub(self, false))
            }
            3 => {
                let min = if allow_empty && self.below(3) == 0 { 0 } else { 1 + self.below(2) as u32 };
                let max = self.auto_bound(min);
                AutoSeq::Consec(sub(self, false), min, max)
            }
            4 => {
                let min = 1 + self.below(2) as u32;
                let max = self.auto_bound(min);
                AutoSeq::Goto(self.below(3) as usize, min, max)
            }
            5 => AutoSeq::Or(sub(self, allow_empty), sub(self, allow_empty)),
            _ => AutoSeq::Throughout(self.below(3) as usize, sub(self, allow_empty)),
        }
    }
}

#[test]
fn random_automaton_properties_agree_with_annex_f() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let (mut checked, mut failed, mut hit) = (0usize, 0usize, 0usize);

    for _ in 0..200 {
        // Force at least one `[->]`, `[=]`, or `$` operator so we exercise `sva_nfa` directly.
        let antecedent = rng.auto_seq(1, true);
        let overlapped = rng.below(2) == 0;
        let consequent = rng.auto_seq(1, false);
        let text = if overlapped {
            format!("{} |-> {}", antecedent.text(), consequent.text())
        } else {
            format!("{} |=> {}", antecedent.text(), consequent.text())
        };
        if !text.contains('$') && !text.contains("[->") && !text.contains("[=") {
            continue;
        }
        if consequent.admits_empty() {
            continue;
        }
        // Ensure antecedent and consequent are satisfiable:
        if !consequent.can_match_from(&[], 0, 4) {
            continue;
        }
        let ant_can_start = if overlapped {
            antecedent.can_match_from(&[], 0, 4)
        } else {
            antecedent.admits_empty() || antecedent.can_match_from(&[], 0, 4)
        };
        if !ant_can_start {
            continue;
        }

        let src = format!(
            "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
             p: assert property ({text});\n q: cover property ({text});\nendmodule\n"
        );
        let mut nl = lower_src(&src).unwrap_netlist();
        let outcomes = nl.compile_properties(&PropertyOptions::default());
        assert!(
            matches!(outcomes[0], PropertyOutcome::Compiled { .. } | PropertyOutcome::Liveness { .. })
                && matches!(outcomes[1], PropertyOutcome::Compiled { .. }),
            "{text}: {outcomes:?}"
        );

        let trace: Vec<[bool; 3]> = (0..24).map(|_| [rng.below(8) < 4, rng.below(8) < 4, rng.below(8) < 4]).collect();
        let rows: Vec<String> = trace.iter().map(|r| format!("0{}{}{}", r[0] as u8, r[1] as u8, r[2] as u8)).collect();
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();

        // Compute exact Annex F expected `want_fail` and `want_hit` ticks on `0..trace.len()`:
        // First collect every tick `t_start` at which an antecedent match (`from <= t_start`) triggers `consequent`:
        let mut cons_starts = BTreeSet::new();
        for from in 0..trace.len() {
            for end in antecedent.ends(&trace, from) {
                let start_tick = if overlapped {
                    if end == from {
                        continue;
                    }
                    end - 1
                } else {
                    end
                };
                if start_tick < trace.len() {
                    cons_starts.insert(start_tick);
                }
            }
        }

        let mut want_hit = BTreeSet::new();
        for &st in &cons_starts {
            for end in consequent.ends(&trace, st) {
                if end > st && end <= trace.len() {
                    want_hit.insert(end - 1);
                }
            }
        }
        let want_hit_vec: Vec<usize> = want_hit.into_iter().collect();

        // For each obligation started at `st`, find the exact first tick `t >= st` where `trace[..=t]`
        // cannot be extended (within horizon 6, sufficient for depth-1 `[0..4]` bounds) to a match of `consequent`:
        let mut want_fail_set = BTreeSet::new();
        for &st in &cons_starts {
            for t in st..trace.len() {
                let prefix = &trace[..=t];
                // If already matched on some prefix `st ..= s` (`s <= t`), obligation is discharged!
                if consequent.ends(prefix, st).iter().any(|&e| e > st) {
                    break;
                }
                if !consequent.can_match_from(prefix, st, 4) {
                    want_fail_set.insert(t);
                    break;
                }
            }
        }
        let want_fail_vec: Vec<usize> = want_fail_set.into_iter().collect();

        assert_eq!(ticks(&nl, &rows_ref, Watch::HitCovers), want_hit_vec, "cover {text}\n{rows_ref:?}");
        assert_eq!(ticks(&nl, &rows_ref, Watch::FailedAsserts), want_fail_vec, "assert {text}\n{rows_ref:?}");
        checked += 1;
        failed += want_fail_vec.len();
        hit += want_hit_vec.len();
    }
    assert!(checked >= 80 && failed >= 40 && hit >= 80, "checked={checked} failed={failed} hit={hit}");
}

#[test]
fn property_not_bounded_unbounded_and_four_state() {
    // `not a` vs `!a` on 4-state `'x'`:
    // SVA boolean `a` is false on `'x'`, so `not a` HOLDS on `'x'` whereas `!a` is `'x'` (FAILS)!
    assert_eq!(fails("p: assert property (not a);", &["0000", "0100", "0x00"]), [1]);
    assert_eq!(fails("p: assert property (!a);", &["0000", "0100", "0x00"]), [1, 2]);

    // Bounded sequence negation `not (a ##1 b)` and implication negation `not (a |-> b)`:
    assert_eq!(fails("p: assert property (not (a ##1 b));", &["0100", "0010", "0100", "0000"]), [1]);
    // `not (a |-> b)` == `a && !b`: fails when `a == 0` OR when `a == 1 && b == 1`!
    assert_eq!(fails("p: assert property (not (a |-> b));", &["0100", "0110", "0000", "0100"]), [1, 2]);
    // Consequent negation `a |-> not (b ##1 c)`:
    assert_eq!(fails("p: assert property (a |-> not (b ##1 c));", &["0110", "0001", "0110", "0000"]), [1]);

    // Unbounded negation:
    // `not strong(a ##[1:$] b)` is pure safety (`a ##[1:$] b` never completes), whereas
    // `not weak(a ##[1:$] b)` (and default `not (a ##[1:$] b)` in `assert`) has both safety + liveness!
    let src = format!(
        "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
         ns: assert property (not strong(a ##[1:$] b));\n\
         nw: assert property (not weak(a ##[1:$] b));\n\
         nev: assert property (not (s_eventually [1:$] (a ##1 b)));\n\
         nalw: assert property (not (always a));\n\
         nunt: assert property (not (a |-> b until c));\n\
         ngoto: assert property (not (a [->1] |-> (b ##1 c)));\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert!(matches!(out[0], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[1], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[2], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[3], PropertyOutcome::Liveness { .. }), "{out:?}");
    assert!(matches!(out[4], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert!(matches!(out[5], PropertyOutcome::Compiled { .. }), "{out:?}");
    // `ns` has no liveness obligation, `nw` and `nalw` and `nunt` and `ngoto` have liveness obligations:
    assert_eq!(nl.property_liveness[0], None);
    assert!(nl.property_liveness[1].is_some());
    assert_eq!(nl.property_liveness[2], None);
    assert!(nl.property_liveness[3].is_some());
    assert!(nl.property_liveness[4].is_some());
    assert!(nl.property_liveness[5].is_some());

    // Simulate `not strong(a ##[1:$] b)`: fails at tick 2 when `b` completes `a ##[1:$] b`:
    assert_eq!(fails("p: assert property (not strong(a ##[1:$] b));", &["0100", "0000", "0010", "0000"]), [2]);
    // Simulate `not (a |-> b until c)`: `a=0` fails at tick 0; `a=1, c=1` fails at tick 0;
    // `a=1, b=0, c=0` holds!
    assert_eq!(fails("p: assert property (not (a |-> b until c));", &["0100", "0000", "0101"]), [1, 2]);
}

#[test]
fn followed_by_overlapped_and_non_overlapped() {
    // `(a ##1 b) #-# c` == `a ##1 (b && c)`:
    assert_eq!(fails("p: assert property ((a ##1 b) #-# c);", &["0100", "0111", "0111", "0010"]), [3]);
    assert_eq!(covers("p: cover property ((a ##1 b) #-# c);", &["0100", "0011", "0100", "0010"]), [1]);

    // Empty-admitting prefix `a [*0:1]`:
    // `#-#` drops empty left match (`a && b`); `#=#` starts `b` at tick 0 on empty match (`b@0 || (a@0 && b@1)`).
    assert_eq!(fails("p: assert property (a [*0:1] #-# b);", &["0110", "0010"]), [1]);
    assert_eq!(fails("p: assert property (a [*0:1] #=# b);", &["0010", "0110", "0000", "0000"]), [3]);

    // Unbounded prefix / consequent with `#-#`:
    assert_eq!(covers("p: cover property ((a ##1 b) #-# s_eventually c);", &["0100", "0010", "0000", "0001"]), [3]);
    assert_eq!(fails("p: assert property (a [->1] #-# b);", &["0000", "0100", "0110"]), [1]);

    // Empty-admitting consequent in `#-#` is rejected per IEEE 1800 §16.12.22:
    let src = format!(
        "module m({PORTS}); default clocking @(posedge clk); endclocking\n\
         p: assert property (a #-# b [*0:1]);\nendmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert!(matches!(&out[0], PropertyOutcome::Unsupported(w) if w.contains("empty match")), "{out:?}");
}

#[test]
fn abort_operators_accept_and_reject_prefix_exact() {
    // Prefix-exact `sync_accept_on(c) (a ##1 b)`:
    // - Tick 0: `a=0, c=0` -> fails immediately at tick 0; `c=1` at tick 1 does NOT retroactively forgive tick 0!
    // - Tick 2: `a=0, c=1` -> accepted immediately at tick 2!
    // - Tick 3: `a=1, c=0`, Tick 4: `b=0, c=1` -> accepted at tick 4!
    let p = "p: assert property (sync_accept_on(c) (a ##1 b));";
    assert_eq!(fails(p, &["0000", "0111", "0001", "0100", "0101"]), [0]);

    // `reject_on(c) (a ##1 b)`:
    // - Even if `b=1` at tick 1, `c=1` at tick 1 rejects!
    let r = "p: assert property (reject_on(c) (a ##1 b));";
    assert_eq!(fails(r, &["0100", "0111", "0110"]), [1]);

    // 4-state `'x'` on abort condition neither accepts nor rejects:
    assert_eq!(fails(p, &["000x"]), [0]);
    assert_eq!(fails(r, &["010x", "011x"]), NONE);

    // Outer abort overrides inner abort when both trigger on the same tick:
    let acc_over_rej = "p: assert property (accept_on(c) reject_on(c) a);";
    assert_eq!(fails(acc_over_rej, &["0001", "0100"]), NONE);
    let rej_over_acc = "p: assert property (reject_on(c) accept_on(c) a);";
    assert_eq!(fails(rej_over_acc, &["0101", "0100"]), [0]);

    // Abort wrapping an implication:
    // 1. `accept_on(c) (a ##1 b |-> rst)`: `c=1` during antecedent aborts that attempt cleanly.
    let imp_acc = "p: assert property (accept_on(c) (a ##1 b |-> rst));";
    assert_eq!(fails(imp_acc, &["0101", "0010", "0100", "0010"]), [3]);
    // 2. `reject_on(c) (a ##1 b |-> rst)`:
    //    - If `a=0, c=0` at tick 0 and we gate attempts by `if (!c || a)`? Test bare:
    //      At tick 0 `a=0, c=1` -> rejects immediately at tick 0!
    //      Under procedural `if (rst)`: attempt starts at tick 0 (`rst=1, a=0, c=0`) -> antecedent dies at tick 0,
    //      so `c=1` at tick 1 (`rst=0`) does NOT fail attempt 0!
    let imp_rej = "always @(posedge clk) if (rst) p: assert property (sync_reject_on(c) (a ##1 b |-> a));";
    assert_eq!(fails(imp_rej, &["1000", "0001"]), NONE);
    assert_eq!(fails(imp_rej, &["1100", "0001"]), [1]);
    assert_eq!(fails(imp_rej, &["1001", "0000"]), [0]);

    // Abort wrapping `until`:
    let unt_acc = "p: assert property (sync_accept_on(c) (a |-> b until rst));";
    assert_eq!(fails(unt_acc, &["0100", "0101", "0000"]), [0]);
    assert_eq!(fails(unt_acc, &["0110", "0001", "0000"]), NONE);
    let unt_rej = "p: assert property (sync_reject_on(c) (a |-> b until rst));";
    assert_eq!(fails(unt_rej, &["0110", "1001"]), [1]);

    // Cover with `accept_on`:
    assert_eq!(covers("p: cover property (accept_on(c) (a ##1 b));", &["0001", "0100", "0000"]), [0]);
}

#[test]
fn random_not_followed_by_and_abort_properties_agree_with_annex_f() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut checked, mut failed, mut hit) = (0usize, 0usize, 0usize);

    for _ in 0..160 {
        let mode = rng.below(4);
        let s1 = rng.auto_seq(1, mode == 1);
        let s2 = rng.auto_seq(0, false);
        if s2.admits_empty() || !s2.can_match_from(&[], 0, 4) {
            continue;
        }
        let overlapped = rng.below(2) == 0;
        let abort_sig = rng.below(3) as usize;
        let abort_name = ["a", "b", "c"][abort_sig];

        // Mode 0: `accept_on(abort) s2` (using depth-1 sequence `sq`)
        // Mode 1: `reject_on(abort) sq`
        // Mode 2: `s1 #-# s2` / `s1 #=# s2` (with at least one automaton op)
        // Mode 3: `not (s1 |-> s2)` (== `s1 #-# not s2`, with single-bool `s2` for crisp oracle)
        let sq = rng.auto_seq(1, false);
        if (mode == 0 || mode == 1) && (sq.admits_empty() || !sq.can_match_from(&[], 0, 4)) {
            continue;
        }
        if mode == 2 {
            let can_start = if overlapped {
                s1.can_match_from(&[], 0, 4)
            } else {
                s1.admits_empty() || s1.can_match_from(&[], 0, 4)
            };
            if !can_start {
                continue;
            }
        }
        let b_idx = rng.below(3) as usize;
        let b_name = ["a", "b", "c"][b_idx];
        if mode == 3 && !s1.can_match_from(&[], 0, 4) {
            continue;
        }

        let text = match mode {
            0 => {
                let kw = if rng.below(2) == 0 { "accept_on" } else { "sync_accept_on" };
                format!("{kw}({abort_name}) ({})", sq.text())
            }
            1 => {
                let kw = if rng.below(2) == 0 { "reject_on" } else { "sync_reject_on" };
                format!("{kw}({abort_name}) ({})", sq.text())
            }
            2 => {
                let op = if overlapped { "#-#" } else { "#=#" };
                format!("({}) {op} ({})", s1.text(), s2.text())
            }
            _ => format!("not (({}) |-> {b_name})", s1.text()),
        };
        // Route Modes 2 & 3 through `sva_nfa` when they contain automaton ops; skip purely bounded
        // cases here because bounded `Matcher` decides at `t + span` whereas `sva_nfa` decides on tight prefixes.
        if (mode == 2 || mode == 3) && !text.contains('$') && !text.contains("[->") && !text.contains("[=") {
            continue;
        }

        let src = format!(
            "module m({PORTS});\n default clocking @(posedge clk); endclocking\n\
             p: assert property ({text});\n q: cover property ({text});\nendmodule\n"
        );
        let mut nl = lower_src(&src).unwrap_netlist();
        let outcomes = nl.compile_properties(&PropertyOptions::default());
        assert!(
            matches!(outcomes[0], PropertyOutcome::Compiled { .. } | PropertyOutcome::Liveness { .. })
                && matches!(outcomes[1], PropertyOutcome::Compiled { .. }),
            "{text}: {outcomes:?}"
        );

        let trace: Vec<[bool; 3]> = (0..20).map(|_| [rng.below(8) < 4, rng.below(8) < 4, rng.below(8) < 4]).collect();
        let rows: Vec<String> = trace.iter().map(|r| format!("0{}{}{}", r[0] as u8, r[1] as u8, r[2] as u8)).collect();
        let rows_ref: Vec<&str> = rows.iter().map(String::as_str).collect();

        let mut want_fail = BTreeSet::new();
        let mut want_hit = BTreeSet::new();

        for st in 0..trace.len() {
            match mode {
                0 => {
                    // `accept_on(abort) sq` from `st`:
                    for t in st..trace.len() {
                        if trace[t][abort_sig] {
                            want_hit.insert(t);
                            break;
                        }
                        let prefix = &trace[..=t];
                        if sq.ends(prefix, st).iter().any(|&e| e > st) {
                            want_hit.insert(t);
                            break;
                        }
                        if !sq.can_match_from(prefix, st, 4) {
                            want_fail.insert(t);
                            break;
                        }
                    }
                }
                1 => {
                    // `reject_on(abort) sq` from `st`:
                    // `CoverNfa` (`(!abort) throughout sq`) hits on every abort-free match end:
                    for end in sq.ends(&trace, st) {
                        if end > st && end <= trace.len() && (st..end).all(|u| !trace[u][abort_sig]) {
                            want_hit.insert(end - 1);
                        }
                    }
                    for t in st..trace.len() {
                        if trace[t][abort_sig] {
                            want_fail.insert(t);
                            break;
                        }
                        let prefix = &trace[..=t];
                        if sq.ends(prefix, st).iter().any(|&e| e > st) {
                            break;
                        }
                        if !sq.can_match_from(prefix, st, 4) {
                            want_fail.insert(t);
                            break;
                        }
                    }
                }
                2 => {
                    let comb_seq = if overlapped {
                        AutoSeq::Delay(Box::new(s1.clone()), 0, Some(0), Box::new(s2.clone()))
                    } else {
                        AutoSeq::Delay(Box::new(s1.clone()), 1, Some(1), Box::new(s2.clone()))
                    };
                    for end in comb_seq.ends(&trace, st) {
                        if end > st && end <= trace.len() {
                            want_hit.insert(end - 1);
                        }
                    }
                    for t in st..trace.len() {
                        let prefix = &trace[..=t];
                        if comb_seq.ends(prefix, st).iter().any(|&e| e > st) {
                            break;
                        }
                        if !comb_seq.can_match_from(prefix, st, 5) {
                            want_fail.insert(t);
                            break;
                        }
                    }
                }
                _ => {
                    // `not (s1 |-> b)` == `s1 ##0 !b` from `st`:
                    let has_match = |tr: &Trace| -> BTreeSet<usize> {
                        s1.ends(tr, st).into_iter().filter(|&e| e > st && e <= tr.len() && !tr[e - 1][b_idx]).collect()
                    };
                    for end in has_match(&trace) {
                        want_hit.insert(end - 1);
                    }
                    fn can_ext(s1: &AutoSeq, b_idx: usize, buf: &mut Vec<[bool; 3]>, st: usize, rem: usize) -> bool {
                        if s1.ends(buf, st).into_iter().any(|e| e > st && e <= buf.len() && !buf[e - 1][b_idx]) {
                            return true;
                        }
                        if rem == 0 {
                            return false;
                        }
                        for bits in 0..8u8 {
                            buf.push([(bits & 1) != 0, (bits & 2) != 0, (bits & 4) != 0]);
                            if can_ext(s1, b_idx, buf, st, rem - 1) {
                                buf.pop();
                                return true;
                            }
                            buf.pop();
                        }
                        false
                    }
                    for t in st..trace.len() {
                        let prefix = &trace[..=t];
                        if !has_match(prefix).is_empty() {
                            break;
                        }
                        let mut buf = prefix.to_vec();
                        if !can_ext(&s1, b_idx, &mut buf, st, 5) {
                            want_fail.insert(t);
                            break;
                        }
                    }
                }
            }
        }

        let want_hit_vec: Vec<usize> = want_hit.into_iter().collect();
        let want_fail_vec: Vec<usize> = want_fail.into_iter().collect();
        assert_eq!(ticks(&nl, &rows_ref, Watch::HitCovers), want_hit_vec, "cover {text}\n{rows_ref:?}");
        assert_eq!(ticks(&nl, &rows_ref, Watch::FailedAsserts), want_fail_vec, "assert {text}\n{rows_ref:?}");
        checked += 1;
        failed += want_fail_vec.len();
        hit += want_hit_vec.len();
    }
    assert!(checked >= 50 && failed >= 30 && hit >= 50, "checked={checked} failed={failed} hit={hit}");
}

#[test]
fn unbounded_always_safety_and_liveness() {
    // `always b`: fails at any tick where `b == 0`.
    let p_top = "p: assert property (@(posedge clk) always b);";
    assert_eq!(fails(p_top, &["0010", "0010", "0000", "0010"]), [2]);

    // `a |-> always b`: once `a` holds, `b` must hold at that tick and every tick thereafter.
    let p_imp = "p: assert property (@(posedge clk) a |-> always b);";
    assert_eq!(fails(p_imp, &["0000", "0000", "0110", "0010", "0000"]), [4]);
    assert_eq!(fails(p_imp, &["0000", "0000", "0000"]), [] as [usize; 0]);

    // `disable iff (rst) (a |-> always [2:$] b)`: shifted by 2 ticks, cleared by `rst`.
    let p_shift = "p: assert property (@(posedge clk) disable iff (rst) (a |-> always [2:$] b));";
    // `a` at tick 0 -> `b` required from tick 2 onwards; fails at tick 3 when `b == 0`.
    assert_eq!(fails(p_shift, &["0100", "0000", "0010", "0000"]), [3]);
    // If `rst` fires at tick 2, the obligation is aborted!
    assert_eq!(fails(p_shift, &["0100", "0000", "1000", "0000"]), [] as [usize; 0]);

    // NFA antecedent feeding unbounded `always`: `(a [->2]) |-> always b`.
    let p_nfa = "p: assert property (@(posedge clk) (a [->2]) |-> always b);";
    assert_eq!(fails(p_nfa, &["0100", "0000", "0110", "0010", "0000"]), [4]);

    // `always s_eventually b` and `a |-> always (b s_until c)` compile into liveness monitors.
    let src = format!(
        "module m({PORTS});\n\
           p_ev: assert property (@(posedge clk) (always s_eventually b));\n\
           p_su: assert property (@(posedge clk) a |-> always (b s_until c));\n\
           c_bad: cover property (@(posedge clk) always b);\n\
         endmodule\n"
    );
    let mut nl = lower_src(&src).unwrap_netlist();
    let out = nl.compile_properties(&PropertyOptions::default());
    assert!(matches!(out[0], PropertyOutcome::Liveness { .. }), "{out:?}");
    assert!(matches!(out[1], PropertyOutcome::Compiled { .. }), "{out:?}");
    assert_eq!(nl.liveness.len(), 2);
    assert!(
        matches!(&out[2], PropertyOutcome::Unsupported(w) if w == "a cover of `always` with no upper bound"),
        "{out:?}"
    );
}

#[test]
fn tier4_implies_iff_ifelse_snexttime_and_compound_always_semantics() {
    // Rows are `"rst,a,b,c"`.
    // 1. `(a ##1 b) implies (##1 c)`: at tick t, if `a(t) && b(t+1)`, then `c(t+1)` must hold.
    let p_imp = "p: assert property (@(posedge clk) (a ##1 b) implies (##1 c));";
    assert_eq!(fails(p_imp, &["0100", "0011", "0100", "0010"]), [3]);

    // 2. `a iff b`: fails whenever `a != b`.
    let p_iff = "p: assert property (@(posedge clk) a iff b);";
    assert_eq!(fails(p_iff, &["0000", "0110", "0100", "0010"]), [2, 3]);

    // 3. `if (a) (b |-> ##1 c) else c`: when `a=1`, `b` implies next-tick `c`; when `a=0`, `c` must hold immediately.
    let p_if = "p: assert property (@(posedge clk) if (a) (b |-> ##1 c) else c);";
    // Tick 0: a=0, c=1 (ok); Tick 1: a=0, c=0 -> fails at tick 2 (span=1 lookback); Tick 2: a=1, b=1; Tick 3: a=0, c=0 -> fails!
    assert_eq!(fails(p_if, &["0001", "0000", "0111", "0100"]), [2, 3]);

    // 4. `s_nexttime [2] (a |-> ##1 b)`: starts checking `a |-> ##1 b` 2 ticks after attempt start.
    let p_snext = "p: assert property (@(posedge clk) s_nexttime [2] (a |-> ##1 b));";
    // `a=1, b=0` at ticks 0..1 is before tick 2 so ignored; `a=1` at tick 2 with `b=0` at tick 3 fails at tick 3!
    assert_eq!(fails(p_snext, &["0100", "0000", "0100", "0000"]), [3]);

    // 5. Compound unbounded `always`: `c |-> always (a |-> ##1 b)`
    let p_comp_alw = "p: assert property (@(posedge clk) disable iff (rst) (c |-> always (a |-> ##1 b)));";
    // Before `c=1`: `a=1, b=0` at ticks 0..1 does NOT fail. Once `c=1` at tick 2, `a=1` at tick 3 without `b=1` at tick 4 fails at tick 4!
    assert_eq!(fails(p_comp_alw, &["0100", "0000", "0001", "0100", "0000"]), [4]);
    // Reset at tick 4 aborts both the sticky trigger and the pending `a |-> ##1 b` obligation:
    assert_eq!(fails(p_comp_alw, &["0001", "0100", "1000", "0100", "0000"]), [] as [usize; 0]);

    // 6. Automaton `implies` / `if..else` with unbounded `[->1]`:
    let p_nfa_if = "p: assert property (@(posedge clk) if (a) (b |-> c [->1]) else c);";
    // Tick 0: a=1, b=1, c=0 -> waits for c; Tick 1: a=1, b=0, c=1 -> satisfies tick 0; Tick 2: a=0, c=0 -> fails immediately!
    assert_eq!(fails(p_nfa_if, &["0110", "0101", "0000"]), [2]);
}

#[test]
fn tier8_sva_local_vars_bounded_prophecy_case_xmr_and_macro_attr() {
    // 1. SVA `case (...) ... endcase` (IEEE 1800-2023 §16.12.17):
    let p_case = "\
        p: assert property (@(posedge clk)\n\
          case ({a, b})\n\
            2'b00: 1'b1;\n\
            2'b01, 2'b10: c;\n\
            default: ##1 c;\n\
          endcase);";
    // Tick 0: 00 -> ok; Tick 1: 01, c=1 -> ok; Tick 2: 10, c=0 -> reported at tick 3 (span=1); Tick 3: 11, Tick 4: c=0 -> fails at tick 4!
    assert_eq!(fails(p_case, &["0000", "0011", "0100", "0110", "0000"]), [3, 4]);

    // 2. Bounded SVA local variables with declaration initializer, `+=`, `++`, and pipelined `##[1:2]` tag match:
    let src_bounded = "\
        module scoreboard (\n\
          input logic clk,\n\
          input logic rst,\n\
          input logic start,\n\
          input logic step,\n\
          input logic req,\n\
          input logic rsp,\n\
          input logic [1:0] req_tag,\n\
          input logic [1:0] rsp_tag,\n\
          input logic [7:0] din,\n\
          input logic [7:0] dout\n\
        );\n\
          property p_accum;\n\
            logic [7:0] acc = 8'd0;\n\
            @(posedge clk) disable iff (rst)\n\
              (start, acc = din) ##1 (step, acc += din, acc++) |=> (dout == acc);\n\
          endproperty\n\
          property p_pipe;\n\
            logic [1:0] tag_s;\n\
            logic [7:0] dat_s;\n\
            @(posedge clk) disable iff (rst)\n\
              (req, tag_s = req_tag, dat_s = din) ##[1:2] (rsp && rsp_tag == tag_s) |-> (dout == dat_s);\n\
          endproperty\n\
          a_accum: assert property (p_accum);\n\
          a_pipe:  assert property (p_pipe);\n\
        endmodule\n";
    let mut nl_b = lower_src(src_bounded).unwrap_netlist();
    let out_b = nl_b.compile_properties(&PropertyOptions::default());
    assert!(out_b.iter().all(|o| matches!(o, PropertyOutcome::Compiled { .. })), "{out_b:?}");
    assert_eq!(nl_b.checks.len(), 2);

    // Cycle-accurate check of `a_accum`:
    // Tick 0: start=1, din=10 -> acc=10
    // Tick 1: step=1, din=5   -> acc=10+5+1=16
    // Tick 2: dout=16 -> PASS; if instead dout=15 -> FAIL!
    for (dout_t2, expect_fail) in [(16i128, false), (15i128, true)] {
        let mut sim = NetlistSim::new(&nl_b);
        let mut tick = |start: i128, step: i128, din: i128, dout: i128| -> bool {
            sim.set_u("rst", 0);
            sim.set_u("start", start);
            sim.set_u("step", step);
            sim.set_u("req", 0);
            sim.set_u("rsp", 0);
            sim.set_u("req_tag", 0);
            sim.set_u("rsp_tag", 0);
            sim.set_u("din", din);
            sim.set_u("dout", dout);
            let before = sim.failed_asserts.len();
            sim.tick_posedge("clk");
            sim.failed_asserts[before..].iter().any(|l| l.ends_with("a_accum"))
        };
        assert!(!tick(1, 0, 10, 0));
        assert!(!tick(0, 1, 5, 0));
        assert_eq!(tick(0, 0, 0, dout_t2), expect_fail);
    }

    // 3. Unbounded SVA local variable scoreboard via symbolic prophecy constant (`##[1:$]` and `s_eventually`):
    let src_unbounded = "\
        module fifo_sb (\n\
          input logic clk,\n\
          input logic rst,\n\
          input logic push,\n\
          input logic pop_match,\n\
          input logic [7:0] wdata,\n\
          input logic [7:0] rdata\n\
        );\n\
          property p_data_safety;\n\
            logic [7:0] d;\n\
            @(posedge clk) disable iff (rst)\n\
              (push, d = wdata) |-> ##[1:$] (!pop_match || (rdata == d));\n\
          endproperty\n\
          property p_data_live;\n\
            logic [7:0] d;\n\
            @(posedge clk) disable iff (rst)\n\
              (push, d = wdata) |=> s_eventually (pop_match && rdata == d);\n\
          endproperty\n\
          cover_match: cover property (@(posedge clk) disable iff (rst) s_cap ##[1:$] pop_match);\n\
          sequence s_cap;\n\
            logic [7:0] d;\n\
            (push, d = wdata) ##1 (rdata == d);\n\
          endsequence\n\
          a_safety: assert property (p_data_safety);\n\
          a_live:   assert property (p_data_live);\n\
        endmodule\n";
    let mut nl_u = lower_src(src_unbounded).unwrap_netlist();
    let out_u = nl_u.compile_properties(&PropertyOptions::default());
    assert!(matches!(out_u[0], PropertyOutcome::Compiled { .. }), "{out_u:?}");
    assert!(matches!(out_u[1], PropertyOutcome::Compiled { .. }), "{out_u:?}");
    assert!(matches!(out_u[2], PropertyOutcome::Liveness { .. }), "{out_u:?}");

    // 4. Macro-defined `-D FORMAL_KEEP="(* keep *)"` + pasted `(``* keep *``)` + recursive generate XMR + `bind` XMR:
    let src_xmr = "\
        `define PASTE_KEEP (``* keep *``)\n\
        module fib_rec #(parameter int N = 0) (output logic [7:0] val);\n\
          if (N <= 0) assign val = 8'd0;\n\
          else if (N == 1) assign val = 8'd1;\n\
          else begin : rec\n\
            fib_rec #(N - 1) f1();\n\
            fib_rec #(N - 2) f2();\n\
            assign val = f1.val + f2.val;\n\
          end\n\
        endmodule\n\
        module sub_unit(input logic clk, input logic [3:0] d, output logic [3:0] q);\n\
          `FORMAL_KEEP `PASTE_KEEP logic [3:0] secret_q;\n\
          always_ff @(posedge clk) begin secret_q <= d; q <= secret_q; end\n\
        endmodule\n\
        module top_xmr(input logic clk, input logic [3:0] d, output logic [3:0] q, output logic [7:0] fib5);\n\
          sub_unit u_sub(.clk(clk), .d(d), .q(q));\n\
          fib_rec #(5) u_fib(.val(fib5));\n\
        endmodule\n\
        module top_props(input logic clk, input logic [3:0] q, input logic [7:0] fib5);\n\
          a_xmr: assert property (@(posedge clk) (fib5 == 8'd5) && (top_xmr.u_sub.secret_q == $past(top_xmr.d) || 1'b1));\n\
        endmodule\n\
        bind top_xmr top_props u_props(.clk(clk), .q(q), .fib5(fib5));\n";
    let mut opts = sv_syntax::Options::default();
    opts.defines.push("FORMAL_KEEP=(* keep *)".into());
    let tree = sv_syntax::parse_source(src_xmr, "xmr.sv", opts);
    assert!(tree.errors.is_empty(), "{:?}", tree.errors);
    let mut nl_x = match sv_netlist::lower_trees(
        &[tree],
        &["top_xmr".into()],
        &[],
        &sv_netlist::LowerConfig { top: Some("top_xmr".into()), ..Default::default() },
    ) {
        sv_netlist::LowerOutcome::Lowered(nl) => nl,
        other => panic!("expected lowered netlist, got {other:?}"),
    };
    let out_x = nl_x.compile_properties(&PropertyOptions::default());
    assert!(matches!(out_x[0], PropertyOutcome::Compiled { .. }), "{out_x:?}");
}
