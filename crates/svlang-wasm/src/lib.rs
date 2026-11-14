//! svlang for the browser: one function, [`run`], behind a plain C interface that any
//! WebAssembly host can call without generated bindings.
//!
//! Build with `cargo build --release --target wasm32-unknown-unknown -p svlang-wasm`
//! (see `www/README.md`). The host passes strings as (pointer, length) pairs into memory
//! it gets from [`svlang_alloc`], and reads the result as a 4-byte little-endian length
//! followed by that many bytes of UTF-8 JSON.

use std::fmt::Write as _;

use svlang::netlist::{PropertyOptions, PropertyOutcome};
use svlang::{Compilation, Diagnostic, DiagnosticStage, Driver, LowerConfig, LowerOutcome, Netlist, Severity};

/// The commands [`run`] takes.
pub const COMMANDS: &[&str] = &["check", "preprocess", "cst", "hier", "netlist", "btor2", "aiger", "yosys"];

/// Runs `command` (one of [`COMMANDS`]) on one SystemVerilog source text and returns JSON:
///
/// ```json
/// {"ok": true, "output": "...", "diagnostics": [{"line": 3, "column": 5, "severity": "error", "stage": "parser", "message": "..."}], "notes": ["..."]}
/// ```
///
/// `ok` is false when the design has errors or cannot be written in the form asked for;
/// `output` is the command's text (empty for `check`); `diagnostics` are the compiler's,
/// and `notes` say what a writer left out. `top` names the top module, or is empty to let
/// svlang find it.
pub fn run(command: &str, source: &str, top: &str) -> String {
    let mut r = Reply::default();
    if !COMMANDS.contains(&command) {
        r.notes.push(format!("unknown command '{command}'"));
        return r.json();
    }
    let mut driver = Driver::new().add_source("design.sv", source);
    if !top.is_empty() {
        driver = driver.top(top);
    }
    let comp = match driver.compile() {
        Ok(c) => c,
        Err(e) => {
            r.notes.push(e.to_string());
            return r.json();
        }
    };
    match command {
        "check" => {
            r.diagnostics = comp.diagnostics();
            r.ok = !r.has_errors();
        }
        "preprocess" => {
            r.diagnostics = comp.diagnostics();
            r.diagnostics.retain(|d| d.stage == DiagnosticStage::Preprocessor);
            r.output = preprocessed(&comp);
            r.ok = !r.has_errors();
        }
        "cst" => {
            r.output = comp.trees().iter().map(svlang::Tree::debug_dump).collect();
            r.ok = !comp.has_syntax_errors();
        }
        "hier" => {
            let (diags, elab) = comp.check();
            r.diagnostics = diags;
            r.output = elab.hierarchy.join("\n");
            r.ok = !r.has_errors();
        }
        _ => lowered(command, &comp, top, &mut r),
    }
    r.json()
}

/// The commands that need the netlist.
fn lowered(command: &str, comp: &Compilation, top: &str, r: &mut Reply) {
    // Memories as registers, so every writer can take the design.
    let cfg = LowerConfig {
        top: (!top.is_empty()).then(|| top.to_owned()),
        blast_memories_up_to: Some(1024),
        ..Default::default()
    };
    let mut nl: Netlist = match comp.lower_netlist(&cfg) {
        LowerOutcome::Lowered(nl) => nl,
        LowerOutcome::Unsupported(d) | LowerOutcome::Error(d) => {
            r.diagnostics = d
                .into_iter()
                .map(|d| Diagnostic {
                    file: d.file,
                    line: d.loc.line,
                    column: d.loc.column,
                    stage: DiagnosticStage::Semantic,
                    severity: Severity::Error,
                    message: d.message,
                })
                .collect();
            return;
        }
    };
    for (i, outcome) in nl.compile_properties(&PropertyOptions::default()).iter().enumerate() {
        if let PropertyOutcome::Unsupported(why) = outcome {
            r.notes.push(format!("property {i} not compiled: {why}"));
        }
    }
    let written = match command {
        "netlist" => Ok((nl.dump(), Vec::new())),
        "btor2" => svlang::btor2::write(&nl, &svlang::btor2::Options::default())
            .map(|m| (m.btor2, m.notes))
            .map_err(|e| e.to_string()),
        "aiger" => svlang::aiger::lower(&nl, &svlang::aiger::Options::default())
            .map(|m| (m.aig.to_ascii(), m.notes))
            .map_err(|e| e.to_string()),
        _ => svlang::yosys::write(&nl, &svlang::yosys::Options::default())
            .map(|m| (m.json, m.notes))
            .map_err(|e| e.to_string()),
    };
    match written {
        Ok((text, notes)) => {
            r.output = text;
            r.notes.extend(notes);
            r.ok = true;
        }
        Err(e) => r.notes.push(e),
    }
}

/// The token stream after macro expansion, with the line breaks and spaces of the source.
fn preprocessed(comp: &Compilation) -> String {
    let mut text = String::new();
    for tree in comp.trees() {
        for (i, token) in tree.tokens.iter().enumerate() {
            let gap = tree.trivia(i as u32);
            if i == 0 {
            } else if gap.contains('\n') {
                text.push('\n');
            } else if gap.contains(char::is_whitespace)
                || gap.contains("/*")
                || (token.from_macro && token.space_before)
            {
                text.push(' ');
            }
            text.push_str(&token.text);
        }
        text.push('\n');
    }
    text
}

#[derive(Default)]
struct Reply {
    ok: bool,
    output: String,
    diagnostics: Vec<Diagnostic>,
    notes: Vec<String>,
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl Reply {
    fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }

    fn json(&self) -> String {
        let diags: Vec<String> = self
            .diagnostics
            .iter()
            .map(|d| {
                let stage = match d.stage {
                    DiagnosticStage::FileList => "filelist",
                    DiagnosticStage::Preprocessor => "preprocessor",
                    DiagnosticStage::Parser => "parser",
                    DiagnosticStage::Semantic => "semantic",
                };
                let severity = if d.severity == Severity::Error { "error" } else { "warning" };
                format!(
                    "{{\"line\": {}, \"column\": {}, \"severity\": \"{severity}\", \"stage\": \"{stage}\", \"message\": {}}}",
                    d.line,
                    d.column,
                    json_string(&d.message)
                )
            })
            .collect();
        let notes: Vec<String> = self.notes.iter().map(|n| json_string(n)).collect();
        format!(
            "{{\"ok\": {}, \"output\": {}, \"diagnostics\": [{}], \"notes\": [{}]}}",
            self.ok,
            json_string(&self.output),
            diags.join(", "),
            notes.join(", ")
        )
    }
}

// ---- the C interface -------------------------------------------------------------------

/// Memory for the host to write a string into: `len` bytes, to be given back with
/// [`svlang_free`].
#[no_mangle]
pub extern "C" fn svlang_alloc(len: usize) -> *mut u8 {
    let mut buf = Vec::<u8>::with_capacity(len.max(1));
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf);
    ptr
}

/// Gives back memory from [`svlang_alloc`] or [`svlang_run`].
///
/// # Safety
///
/// `ptr` and `len` must be what `svlang_alloc` was called with and returned, or a pointer
/// `svlang_run` returned with its result's length plus 4, and must not be used again.
#[no_mangle]
pub unsafe extern "C" fn svlang_free(ptr: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(ptr, 0, len.max(1)));
}

/// [`run`] for the host: the command, the source and the top module name as UTF-8
/// (pointer, length) pairs. Returns a pointer to a 4-byte little-endian length followed by
/// that many bytes of JSON, to be given back with `svlang_free(pointer, length + 4)`.
///
/// # Safety
///
/// Each pointer must point at its length of readable bytes (a length of 0 reads nothing).
#[no_mangle]
pub unsafe extern "C" fn svlang_run(
    command: *const u8,
    command_len: usize,
    source: *const u8,
    source_len: usize,
    top: *const u8,
    top_len: usize,
) -> *mut u8 {
    let text = |ptr: *const u8, len: usize| {
        if len == 0 {
            String::new()
        } else {
            String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned()
        }
    };
    let json = run(&text(command, command_len), &text(source, source_len), &text(top, top_len));
    let mut out = Vec::with_capacity(json.len() + 4);
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    debug_assert_eq!(out.capacity(), json.len() + 4);
    let ptr = out.as_mut_ptr();
    std::mem::forget(out);
    ptr
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTER: &str = "module cnt (input logic clk, rst, output logic [3:0] q);\n\
           always_ff @(posedge clk) if (rst) q <= '0; else if (q != 9) q <= q + 1'b1;\n\
           always @(posedge clk) in_range: assert (q <= 9);\n\
         endmodule\n";

    #[test]
    fn check_reports_diagnostics_as_json() {
        assert_eq!(run("check", COUNTER, ""), "{\"ok\": true, \"output\": \"\", \"diagnostics\": [], \"notes\": []}");
        let r = run("check", "module m;\n  assign x = ;\nendmodule\n", "");
        assert!(r.starts_with("{\"ok\": false, \"output\": \"\", \"diagnostics\": [{\"line\": 2, \"column\": 14, \"severity\": \"error\", \"stage\": \"parser\", \"message\": \"expected an expression"), "{r}");
    }

    #[test]
    fn every_command_gives_its_text() {
        for (command, want) in [
            ("preprocess", "module cnt (input logic clk"),
            ("cst", "SourceFile\\n  ModuleDecl"),
            ("hier", "$root instance cnt : cnt"),
            ("netlist", "netlist cnt\\n"),
            ("btor2", " in_range\\n"),
            ("aiger", "aag "),
            ("yosys", "\\\"type\\\": \\\"$dff\\\""),
        ] {
            let r = run(command, COUNTER, "");
            assert!(r.starts_with("{\"ok\": true, ") && r.contains(want), "{command}: {r}");
        }
    }

    #[test]
    fn errors_are_reported_not_thrown() {
        let two_clocks = "module two(input logic a, b, d, output logic p, q);\n always_ff @(posedge a) p <= d;\n always_ff @(posedge b) q <= d;\nendmodule\n";
        let r = run("btor2", two_clocks, "");
        assert!(r.starts_with("{\"ok\": false") && r.contains("clock"), "{r}");
        let r = run("netlist", "module m; assign x = 1; endmodule", "");
        assert!(r.starts_with("{\"ok\": false") && r.contains("undeclared"), "{r}");
        assert!(run("frob", COUNTER, "").contains("unknown command 'frob'"));
    }

    #[test]
    fn the_c_interface_round_trips() {
        let (command, source) = ("hier", COUNTER);
        unsafe {
            let c = svlang_alloc(command.len());
            std::ptr::copy_nonoverlapping(command.as_ptr(), c, command.len());
            let s = svlang_alloc(source.len());
            std::ptr::copy_nonoverlapping(source.as_ptr(), s, source.len());
            let out = svlang_run(c, command.len(), s, source.len(), std::ptr::null(), 0);
            let len = u32::from_le_bytes(*(out as *const [u8; 4])) as usize;
            let json = std::str::from_utf8(std::slice::from_raw_parts(out.add(4), len)).unwrap().to_owned();
            svlang_free(out, len + 4);
            svlang_free(c, command.len());
            svlang_free(s, source.len());
            assert_eq!(json, run(command, source, ""));
        }
    }
}
