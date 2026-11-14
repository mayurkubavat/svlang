//! The Btor2 text: well formed, and the modelling choices in the crate docs
//! show up as the lines they promise.

use std::collections::HashSet;

use sv_btor2::{write, Fail, Options};
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
fn two_clocks_and_latches_are_rejected() {
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
    let e = write(
        &netlist("module lat(input logic en, d, output logic q);\n always_latch if (en) q <= d;\nendmodule\n"),
        &Options::default(),
    )
    .unwrap_err();
    assert!(matches!(e, Fail::Unsupported(_) | Fail::Rejected(_)) && e.to_string().contains("latch"), "{e}");
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
