//! The Yosys JSON text: well-formed JSON with the cells the crate docs promise, and, when
//! Yosys is installed, equivalent to what Yosys itself makes of the same SystemVerilog.

use std::path::{Path, PathBuf};
use std::process::Command;

use sv_netlist::{lower_trees, LowerConfig, Netlist};
use sv_yosys::{write, Fail, Options};

fn netlist_with(src: &str, top: &str, cfg: LowerConfig) -> Netlist {
    let tree = sv_syntax::parse_source(src, "t.sv", Default::default());
    let tops: Vec<String> = if top.is_empty() { Vec::new() } else { vec![top.to_owned()] };
    lower_trees(&[tree], &tops, &[], &cfg).unwrap_netlist()
}

fn netlist(src: &str) -> Netlist {
    netlist_with(src, "", LowerConfig::default())
}

fn json(src: &str) -> String {
    let m = write(&netlist(src), &Options::default()).unwrap();
    check_json(&m.json);
    m.json
}

/// How many cells of type `ty` the text has.
fn cells(json: &str, ty: &str) -> usize {
    json.matches(&format!("\"type\": \"{ty}\"")).count()
}

/// Panics unless `text` is one well-formed JSON value.
fn check_json(text: &str) {
    fn ws(s: &[u8], mut i: usize) -> usize {
        while i < s.len() && s[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }
    fn value(s: &[u8], i: usize) -> usize {
        let i = ws(s, i);
        match s[i] {
            b'{' | b'[' => {
                let (close, object) = if s[i] == b'{' { (b'}', true) } else { (b']', false) };
                let mut i = ws(s, i + 1);
                if s[i] == close {
                    return i + 1;
                }
                loop {
                    if object {
                        i = ws(s, string(s, ws(s, i)));
                        assert_eq!(s[i], b':', "expected ':' at byte {i}");
                        i += 1;
                    }
                    i = ws(s, value(s, i));
                    if s[i] == close {
                        return i + 1;
                    }
                    assert_eq!(s[i], b',', "expected ',' at byte {i}");
                    i += 1;
                }
            }
            b'"' => string(s, i),
            b'0'..=b'9' => {
                let mut i = i;
                while s[i].is_ascii_digit() {
                    i += 1;
                }
                i
            }
            c => panic!("unexpected {:?} at byte {i}", c as char),
        }
    }
    fn string(s: &[u8], i: usize) -> usize {
        assert_eq!(s[i], b'"', "expected a string at byte {i}");
        let mut i = i + 1;
        while s[i] != b'"' {
            i += if s[i] == b'\\' { 2 } else { 1 };
        }
        i + 1
    }
    let s = text.as_bytes();
    assert_eq!(ws(s, value(s, 0)), s.len(), "text after the JSON value");
}

#[test]
fn counter_is_a_dff_an_adder_and_a_mux() {
    let j = json(
        "module cnt (input logic clk, rst, output logic [3:0] q);\n\
           initial q = 4'd5;\n\
           always_ff @(posedge clk) if (rst) q <= 4'd0; else q <= q + 4'd1;\n\
           always @(posedge clk) in_range: assert (q != 4'd15);\n\
         endmodule\n",
    );
    assert_eq!((cells(&j, "$dff"), cells(&j, "$add"), cells(&j, "$mux"), cells(&j, "$assert")), (1, 1, 1, 1));
    assert!(j.contains("\"clk\": {\"direction\": \"input\", \"bits\": [2]}"), "{j}");
    assert!(j.contains("\"q\": {\"direction\": \"output\", \"bits\": [4, 5, 6, 7]}"), "{j}");
    // The register's wire has its hierarchical name and initial value; the assertion its label.
    assert!(
        j.contains("\"cnt.q\": {\"hide_name\": 0, \"bits\": [4, 5, 6, 7], \"attributes\": {\"init\": \"0101\"}}"),
        "{j}"
    );
    assert!(j.contains("\"cnt.in_range\": {\n          \"hide_name\": 0,\n          \"type\": \"$assert\""), "{j}");
    assert!(j.contains("\"src\": \"t.sv:3."), "{j}");
}

#[test]
fn slices_concatenations_and_extensions_are_wiring() {
    let j = json(
        "module w (input logic [7:0] a, input logic signed [3:0] s, output logic [7:0] swap, output logic signed [7:0] ext);\n\
           assign swap = {a[3:0], a[7:4]};\n\
           assign ext = s;\n\
         endmodule\n",
    );
    assert_eq!(j.matches("\"type\"").count(), 0, "{j}");
    assert!(j.contains("\"swap\": {\"direction\": \"output\", \"bits\": [6, 7, 8, 9, 2, 3, 4, 5]}"), "{j}");
    assert!(j.contains("\"ext\": {\"direction\": \"output\", \"bits\": [10, 11, 12, 13, 13, 13, 13, 13]}"), "{j}");
}

#[test]
fn resets_latches_and_free_values_pick_their_cells() {
    let j = json(
        "module r (input logic clk, arst_n, ld, en, input logic [3:0] d, v, output logic [3:0] a, b, c, k);\n\
           (* anyconst *) logic [3:0] any;\n\
           always_ff @(posedge clk or negedge arst_n) if (!arst_n) a <= 4'hC; else a <= d;\n\
           always_ff @(posedge clk or posedge ld) if (ld) b <= v; else b <= d;\n\
           always_latch if (en) c = d;\n\
           assign k = any;\n\
         endmodule\n",
    );
    assert_eq!((cells(&j, "$adff"), cells(&j, "$aldff"), cells(&j, "$dlatch"), cells(&j, "$anyconst")), (1, 1, 1, 1));
    assert!(j.contains("\"ARST_POLARITY\": \"00000000000000000000000000000000\", \"ARST_VALUE\": \"1100\""), "{j}");
}

#[test]
fn past_is_a_register_on_its_clock() {
    let j = json(
        "module p (input logic clk, input logic [3:0] d, output logic [3:0] q);\n\
           always_ff @(posedge clk) q <= d;\n\
           always @(posedge clk) follows: assert (q == $past(d));\n\
         endmodule\n",
    );
    assert_eq!(cells(&j, "$dff"), 2, "{j}");
}

#[test]
fn checks_can_be_left_out() {
    let nl = netlist(
        "module c (input logic clk, a);\n always @(posedge clk) ok: assert (a);\n p: assert property (@(posedge clk) a);\nendmodule\n",
    );
    let m = write(&nl, &Options::default()).unwrap();
    assert_eq!(cells(&m.json, "$assert"), 1);
    assert!(m.notes.iter().any(|n| n.contains("1 concurrent property is not written")), "{:?}", m.notes);
    let mut o = Options::default();
    o.checks = false;
    o.src_attributes = false;
    let m = write(&nl, &o).unwrap();
    assert_eq!(cells(&m.json, "$assert"), 0);
    assert!(!m.json.contains("\"src\""));
}

#[test]
fn a_memory_must_be_expanded_first() {
    let src = std::fs::read_to_string(designs().join("mem.sv")).unwrap();
    let e = write(&netlist(&src), &Options::default()).unwrap_err();
    assert!(matches!(&e, Fail::Unsupported(m) if m.contains("blast_memories_up_to")), "{e}");
    let cfg = LowerConfig { blast_memories_up_to: Some(16), ..Default::default() };
    let m = write(&netlist_with(&src, "", cfg), &Options::default()).unwrap();
    assert_eq!(cells(&m.json, "$dff"), 5);
}

fn designs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/equiv")
}

/// The Yosys to check against: `$YOSYS`, or `yosys` on the path.
fn yosys() -> Option<String> {
    let bin = std::env::var("YOSYS").unwrap_or_else(|_| "yosys".into());
    Command::new(&bin).arg("-V").output().ok().filter(|o| o.status.success()).map(|_| bin)
}

/// Asks Yosys to prove that `json` and its own reading of `sv` agree on every output, for
/// every input, over `steps` steps from the all-zero state (0 for a design with no state).
/// Clocks, resets and enables are ordinary inputs here (`clk2fflogic`), so every edge and
/// asynchronous event is covered.
fn equivalent(yosys: &str, top: &str, sv: &Path, json: &Path, steps: u32) -> bool {
    let seq = if steps == 0 { String::new() } else { format!(" -seq {steps} -set-init-zero") };
    let script = format!(
        "read_verilog -sv -DGOLD {}; hierarchy -top {top}; proc; flatten; memory; opt_clean; rename {top} gold; \
         read_json {}; rename {top} gate; chformal -remove gate; clk2fflogic; \
         miter -equiv -flatten -make_assert gold gate miter; hierarchy -top miter; \
         sat -verify -prove-asserts{seq} miter",
        sv.display(),
        json.display()
    );
    Command::new(yosys).args(["-q", "-p", &script]).output().unwrap().status.success()
}

#[test]
fn yosys_proves_the_json_equivalent_to_its_own_reading() {
    let Some(yosys) = yosys() else {
        eprintln!("skipped: no Yosys (set YOSYS or put yosys on the path)");
        return;
    };
    let dir = std::env::temp_dir().join(format!("sv-yosys-equiv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Design, and the steps to unroll (0: no state).
    let designs_and_steps =
        [("comb", 0), ("divs", 0), ("misc", 0), ("seq", 10), ("lat", 6), ("hier", 8), ("mem", 6), ("aload", 6)];
    for (top, steps) in designs_and_steps {
        let sv = designs().join(format!("{top}.sv"));
        let src = std::fs::read_to_string(&sv).unwrap();
        let cfg = LowerConfig { blast_memories_up_to: Some(64), ..Default::default() };
        let model = write(&netlist_with(&src, top, cfg), &Options::default()).unwrap();
        let json = dir.join(format!("{top}.json"));
        std::fs::write(&json, &model.json).unwrap();
        assert!(equivalent(&yosys, top, &sv, &json, steps), "{top}: Yosys did not prove equivalence");

        // The proof must be able to fail: swap the first cell of one kind for another.
        let swap = match top {
            "comb" => Some(("$lt", "$le")),
            "divs" => Some(("$div", "$mod")),
            "seq" => Some(("$adff", "$dff")),
            "hier" => Some(("$add", "$sub")),
            _ => None,
        };
        if let Some((from, to)) = swap {
            let broken = model.json.replacen(&format!("\"type\": \"{from}\""), &format!("\"type\": \"{to}\""), 1);
            assert_ne!(broken, model.json);
            let json = dir.join(format!("{top}-broken.json"));
            std::fs::write(&json, broken).unwrap();
            assert!(!equivalent(&yosys, top, &sv, &json, steps), "{top}: the {from} to {to} swap went unnoticed");
        }
    }
}
