//! The `svlang` binary on small designs: output, files written and exit status.

use std::path::PathBuf;
use std::process::{Command, Output};

const COUNTER: &str = "module cnt #(parameter int W = 4) (input logic clk, rst, output logic [W-1:0] q);\n\
       initial q = '0;\n\
       always_ff @(posedge clk) if (rst) q <= '0; else if (q != 9) q <= q + 1'b1;\n\
       always @(posedge clk) in_range: assert (q <= 9);\n\
     endmodule\n";

/// A fresh directory holding `name` with `text`.
fn source(test: &str, name: &str, text: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("svlang-cli-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    std::fs::write(&file, text).unwrap();
    (dir, file)
}

fn svlang(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_svlang")).args(args).output().unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn check_clean_design() {
    let (_, f) = source("clean", "cnt.sv", COUNTER);
    let o = svlang(&["check", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "0 error(s), 0 warning(s)\n");
}

#[test]
fn check_reports_errors_as_text_and_json() {
    let (_, f) = source("bad", "bad.sv", "module bad;\n  assign x = ;\nendmodule\n");
    let o = svlang(&["check", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    let out = stdout(&o);
    assert!(out.contains("bad.sv:2:14: error: expected an expression"), "{out}");
    assert!(out.ends_with("1 error(s), 0 warning(s)\n"), "{out}");

    let o = svlang(&["check", "--json", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    let out = stdout(&o);
    assert!(out.starts_with("[\n  {\"file\": \"") && out.ends_with("}\n]\n"), "{out}");
    assert!(out.contains("\"line\": 2, \"column\": 14, \"stage\": \"parser\", \"severity\": \"error\""), "{out}");
}

#[test]
fn json_of_a_clean_design_is_an_empty_array() {
    let (_, f) = source("empty", "cnt.sv", COUNTER);
    let o = svlang(&["check", "--json", f.to_str().unwrap()]);
    assert_eq!((o.status.code(), stdout(&o).as_str()), (Some(0), "[]\n"));
}

#[test]
fn hier_applies_parameter_overrides() {
    let (_, f) = source("hier", "cnt.sv", COUNTER);
    let o = svlang(&["hier", f.to_str().unwrap(), "-G", "W=5"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(stdout(&o), "$root instance cnt : cnt\ncnt parameter W = 5\n");
}

#[test]
fn cst_and_netlist_print_the_design() {
    let (_, f) = source("dump", "cnt.sv", COUNTER);
    let o = svlang(&["cst", f.to_str().unwrap()]);
    assert!(stdout(&o).starts_with("SourceFile\n  ModuleDecl\n"), "{}", stdout(&o));
    let o = svlang(&["netlist", f.to_str().unwrap()]);
    assert!(stdout(&o).starts_with("netlist cnt\n"), "{}", stdout(&o));
    let o = svlang(&["netlist", f.to_str().unwrap(), "--cut", "cnt.q"]);
    assert!(stdout(&o).contains("(cut)"), "{}", stdout(&o));
}

#[test]
fn btor2_names_the_assertion() {
    let (_, f) = source("btor2", "cnt.sv", COUNTER);
    let o = svlang(&["btor2", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).lines().any(|l| l.contains(" bad ") && l.ends_with(" in_range")), "{}", stdout(&o));
}

#[test]
fn aiger_writes_a_file_or_ascii() {
    let (dir, f) = source("aiger", "cnt.sv", COUNTER);
    let out = dir.join("cnt.aig");
    let o = svlang(&["aiger", f.to_str().unwrap(), "-o", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).is_empty());
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"aig "));

    let o = svlang(&["aiger", "--ascii", f.to_str().unwrap()]);
    assert!(stdout(&o).starts_with("aag "), "{}", stdout(&o));
    // Binary AIGER is never printed to the terminal.
    let o = svlang(&["aiger", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn a_design_that_cannot_be_lowered_exits_1() {
    let (_, f) = source(
        "two",
        "two.sv",
        "module two(input logic a, b, d, output logic p, q);\n always_ff @(posedge a) p <= d;\n always_ff @(posedge b) q <= d;\nendmodule\n",
    );
    let o = svlang(&["btor2", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("more than one clock"), "{}", stderr(&o));
    assert!(stdout(&o).is_empty());
}

#[test]
fn usage_errors_exit_2() {
    for args in [&[][..], &["frob", "x.sv"], &["check"], &["check", "/no/such/file.sv"], &["check", "--wat", "x.sv"]] {
        let o = svlang(args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(stdout(&o).is_empty(), "{args:?}");
    }
    let o = svlang(&["help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(stdout(&o).contains("usage: svlang <command>"));
    let o = svlang(&["version"]);
    assert_eq!(stdout(&o), format!("svlang {}\n", env!("CARGO_PKG_VERSION")));
}

#[test]
fn properties_option_writes_concurrent_assertions() {
    let (_, f) = source(
        "props",
        "hs.sv",
        "module hs(input logic clk, req, output logic ack);\n\
           always_ff @(posedge clk) ack <= req;\n\
           a_ack: assert property (@(posedge clk) req |=> ack);\n\
         endmodule\n",
    );
    // Without --properties the assertion is left out and noted.
    let o = svlang(&["btor2", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(!stdout(&o).lines().any(|l| l.contains(" bad ")), "{}", stdout(&o));
    assert!(stderr(&o).contains("note:"), "{}", stderr(&o));
    // With it, it is a bad line under its hierarchical name.
    let o = svlang(&["btor2", "--properties", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).lines().any(|l| l.contains(" bad ") && l.ends_with("hs.a_ack")), "{}", stdout(&o));
    assert!(stderr(&o).contains("1 of 1 concurrent properties compiled"), "{}", stderr(&o));
}

#[test]
fn yosys_writes_a_json_netlist() {
    let (_, f) = source("yosys", "cnt.sv", COUNTER);
    let o = svlang(&["yosys", f.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with("{\n  \"creator\": \"svlang sv-yosys "), "{out}");
    assert!(out.contains("\"type\": \"$dff\"") && out.contains("\"in_range\""), "{out}");
}
