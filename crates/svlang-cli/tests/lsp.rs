//! Drives `svlang lsp` over stdio on a small UVM workspace.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

const PKG: &str = r#"package my_pkg;
  import uvm_pkg::*;
  // A bus transaction.
  class my_item extends uvm_sequence_item;
    rand bit [7:0] addr; // target address
    function void show(); endfunction
    extern task send(int n);
  endclass
  task my_item::send(int n); endtask
  class my_drv extends uvm_driver #(my_item);
    my_item item;
    task run_phase(uvm_phase phase);
      my_item local_it;
      item.show();
      local_it.addr = 1;
      this.item.send(2);
      `my_macro(x)
      item.
    endtask
  endclass
endpackage
"#;

const BASE: &str = r#"package uvm_pkg;
class uvm_object;
  // The instance name.
  function string get_name(); endfunction
endclass
class uvm_sequence_item extends uvm_object;
endclass
class uvm_driver #(type REQ = int) extends uvm_object;
  REQ req;
endclass
endpackage
"#;

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next: u32,
}

impl Client {
    fn start(root: &Path) -> Client {
        Self::start_with_opts(root, "{}")
    }

    fn start_with_opts(root: &Path, init_opts: &str) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_svlang"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut c = Client { child, stdin, stdout, next: 1 };
        let root_uri = format!("file://{}", root.display());
        c.request(
            "initialize",
            &format!(r#"{{"rootUri":"{}","initializationOptions":{},"capabilities":{{}}}}"#, root_uri, init_opts),
        );
        c.notify("initialized", "{}");
        c
    }

    fn send(&mut self, body: &str) {
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.stdin.flush().unwrap();
    }

    fn notify(&mut self, method: &str, params: &str) {
        self.send(&format!(r#"{{"jsonrpc":"2.0","method":"{}","params":{}}}"#, method, params));
    }

    fn read(&mut self) -> String {
        let mut len = 0;
        loop {
            let mut line = String::new();
            self.stdout.read_line(&mut line).unwrap();
            let l = line.trim();
            if l.is_empty() {
                break;
            }
            if let Some(v) = l.strip_prefix("Content-Length:") {
                len = v.trim().parse().unwrap();
            }
        }
        let mut buf = vec![0; len];
        self.stdout.read_exact(&mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// Sends a request and returns its response, skipping notifications.
    fn request(&mut self, method: &str, params: &str) -> String {
        let id = self.next;
        self.next += 1;
        self.send(&format!(r#"{{"jsonrpc":"2.0","id":{},"method":"{}","params":{}}}"#, id, method, params));
        loop {
            let m = self.read();
            if m.contains(&format!(r#""id":{}"#, id)) {
                return m;
            }
        }
    }

    /// The next `publishDiagnostics` for `file`.
    fn diagnostics(&mut self, file: &Path) -> String {
        loop {
            let m = self.read();
            if m.contains("textDocument/publishDiagnostics") && m.contains(&format!("file://{}", file.display())) {
                return m;
            }
        }
    }

    fn at(&mut self, method: &str, file: &Path, line: u32, col: u32) -> String {
        self.request(
            method,
            &format!(
                r#"{{"textDocument":{{"uri":"file://{}"}},"position":{{"line":{},"character":{}}}}}"#,
                file.display(),
                line,
                col
            ),
        )
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn workspace() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-test-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(dir.join("pkg.sv"), PKG).unwrap();
    std::fs::write(dir.join("lib/uvm_base.sv"), BASE).unwrap();
    std::fs::write(dir.join("macros.svh"), "// Prints its argument.\n`define my_macro(a) $display(a)\n").unwrap();
    dir
}

/// Line and column of the `nth` occurrence of `needle` (plus `delta` columns).
fn pos(text: &str, needle: &str, nth: usize, delta: u32) -> (u32, u32) {
    let off = text.match_indices(needle).nth(nth).unwrap_or_else(|| panic!("{needle}")).0;
    let line = text[..off].matches('\n').count() as u32;
    let col = (off - text[..off].rfind('\n').map(|i| i + 1).unwrap_or(0)) as u32;
    (line, col + delta)
}

fn line_of(resp: &str) -> Vec<u32> {
    // "range":{"start":{"line":N
    resp.match_indices(r#""range":{"start":{"line":"#)
        .map(|(i, m)| resp[i + m.len()..].split(|c: char| !c.is_ascii_digit()).next().unwrap().parse().unwrap())
        .collect()
}

#[test]
fn navigation_and_completion() {
    let dir = workspace();
    let pkg = dir.join("pkg.sv");
    let mut c = Client::start(&dir);

    // item.show() -> show's declaration
    let (l, k) = pos(PKG, "item.show", 0, 6);
    let r = c.at("textDocument/definition", &pkg, l, k);
    assert_eq!(line_of(&r), vec![pos(PKG, "void show", 0, 0).0], "{r}");

    // local_it.addr -> addr in my_item
    let (l, k) = pos(PKG, "local_it.addr", 0, 10);
    let r = c.at("textDocument/definition", &pkg, l, k);
    assert_eq!(line_of(&r), vec![4], "{r}");

    // this.item.send -> prototype and out-of-block body
    let (l, k) = pos(PKG, "item.send", 0, 6);
    let r = c.at("textDocument/definition", &pkg, l, k);
    assert_eq!(line_of(&r), vec![6, 8], "{r}");

    // A macro in another file
    let (l, k) = pos(PKG, "`my_macro", 0, 2);
    let r = c.at("textDocument/definition", &pkg, l, k);
    assert!(r.contains("macros.svh"), "{r}");

    // A base class in a library file
    let (l, k) = pos(PKG, "uvm_sequence_item", 0, 3);
    let r = c.at("textDocument/definition", &pkg, l, k);
    assert!(r.contains("uvm_base.sv"), "{r}");

    // Hover shows the declaration and its trailing comment
    let (l, k) = pos(PKG, "local_it.addr", 0, 10);
    let r = c.at("textDocument/hover", &pkg, l, k);
    assert!(r.contains("target address") && r.contains("rand bit [7:0] addr;"), "{r}");

    // Completion after `item.`: own and inherited members
    let (l, k) = pos(PKG, "item.\n", 0, 5);
    let r = c.at("textDocument/completion", &pkg, l, k);
    for m in ["\"show\"", "\"addr\"", "\"send\"", "\"get_name\""] {
        assert!(r.contains(m), "{m} missing: {r}");
    }
    assert!(!r.contains("\"run_phase\""), "{r}");

    // Completion of plain names sees locals and members
    let (l, k) = pos(PKG, "local_it.addr", 0, 0);
    let r = c.at("textDocument/completion", &pkg, l, k);
    assert!(r.contains("\"local_it\"") && r.contains("\"item\"") && r.contains("\"get_name\""), "{r}");
    let (l, k) = pos(PKG, "local_it.addr", 0, 3);
    let r = c.at("textDocument/completion", &pkg, l, k);
    assert!(r.contains("\"local_it\"") && !r.contains("\"item\""), "{r}");

    // Backtick completion lists workspace macros
    let (l, k) = pos(PKG, "`my_macro", 0, 1);
    let r = c.at("textDocument/completion", &pkg, l, k);
    assert!(r.contains("\"my_macro\"") && r.contains("\"uvm_info\""), "{r}");

    // References to my_item: class, uses, scope prefix
    let (l, k) = pos(PKG, "class my_item", 0, 7);
    let r = c.at("textDocument/references", &pkg, l, k);
    assert_eq!(line_of(&r).len(), 5, "{r}");

    // Outline: package > classes > members
    let r = c
        .request("textDocument/documentSymbol", &format!(r#"{{"textDocument":{{"uri":"file://{}"}}}}"#, pkg.display()));
    assert!(
        r.starts_with(r#"{"jsonrpc":"2.0","id""#)
            && r.contains(r#""name":"my_pkg""#)
            && r.contains(r#""name":"my_item::send""#),
        "{r}"
    );
    assert!(!r.contains("local_it"), "{r}");

    // Workspace symbols
    let r = c.request("workspace/symbol", r#"{"query":"drv"}"#);
    assert!(r.contains(r#""name":"my_drv""#), "{r}");

    // Edits are seen without saving
    let edited = PKG.replace("function void show();", "function void show2();");
    let body = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":2}},"contentChanges":[{{"text":{}}}]}}"#,
        pkg.display(),
        json_str(&edited)
    );
    c.notify("textDocument/didChange", &body);
    let r = c.request("workspace/symbol", r#"{"query":"show2"}"#);
    assert!(r.contains(r#""name":"show2""#), "{r}");

    let r = c.request("shutdown", "null");
    assert!(r.contains(r#""result":null"#));
    c.notify("exit", "null");
    let status = c.child.wait().unwrap();
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&dir);
}

const DRIVE: &str = "interface bus_if;
  logic clk;
  logic [7:0] data;
  clocking cb @(posedge clk);
    output data;
  endclocking
endinterface
class drv;
  virtual bus_if vif;
  task drive();
    vif.cb.data = 8'hFF;
  endtask
endclass
";

#[test]
fn diagnostics_and_quick_fixes() {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-lint-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let file = dir.join("drv.sv");
    // The file on disk is clean: what is reported is the editor's text.
    std::fs::write(&file, DRIVE.replace("data = 8", "data <= 8")).unwrap();
    let mut c = Client::start(&dir);
    let open = |text: &str| {
        format!(
            r#"{{"textDocument":{{"uri":"file://{}","languageId":"systemverilog","version":1,"text":{}}}}}"#,
            file.display(),
            json_str(text)
        )
    };
    c.notify("textDocument/didOpen", &open(DRIVE));
    let d = c.diagnostics(&file);
    // The blocking drive of a clocking block signal, on the `=` of line 11.
    assert!(d.contains(r#""code":"SV-INT-001""#), "{d}");
    assert!(d.contains(r#""source":"svlang-lint""#) && d.contains(r#""severity":1"#), "{d}");
    let (line, col) = pos(DRIVE, "= 8'hFF", 0, 0);
    assert!(d.contains(&format!(r#""start":{{"line":{line},"character":{col}}}"#)), "{d}");

    // Its quick fix rewrites the operator.
    let r = c.request(
        "textDocument/codeAction",
        &format!(
            r#"{{"textDocument":{{"uri":"file://{}"}},"range":{{"start":{{"line":{line},"character":0}},"end":{{"line":{line},"character":40}}}},"context":{{"diagnostics":[]}}}}"#,
            file.display()
        ),
    );
    assert!(r.contains(r#""kind":"quickfix""#) && r.contains(r#""newText":"<=""#), "{r}");
    assert!(r.contains(&format!(r#""end":{{"line":{line},"character":{}}}"#, col + 1)), "{r}");
    // And the finding can be waived with a comment on the line above.
    assert!(r.contains(r#""title":"Waive SV-INT-001 on this line""#), "{r}");
    assert!(r.contains(r#""newText":"    // svlang-lint-disable-next-line SV-INT-001\n""#), "{r}");

    // Typing the fix clears it, without a save.
    let change = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":2}},"contentChanges":[{{"text":{}}}]}}"#,
        file.display(),
        json_str(&DRIVE.replace("data = 8", "data <= 8"))
    );
    c.notify("textDocument/didChange", &change);
    let d = c.diagnostics(&file);
    assert!(!d.contains(r#""code":"SV-INT-001""#), "{d}");

    // So does the waiver comment.
    let waived = DRIVE.replace("    vif.cb", "    // svlang-lint-disable-next-line SV-INT-001\n    vif.cb");
    let change = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":3}},"contentChanges":[{{"text":{}}}]}}"#,
        file.display(),
        json_str(&waived)
    );
    c.notify("textDocument/didChange", &change);
    let d = c.diagnostics(&file);
    assert!(!d.contains(r#""code":"SV-INT-001""#), "{d}");

    // Closing the file takes its diagnostics away.
    c.notify("textDocument/didClose", &format!(r#"{{"textDocument":{{"uri":"file://{}"}}}}"#, file.display()));
    let d = c.diagnostics(&file);
    assert!(d.contains(r#""diagnostics":[]"#), "{d}");
    let r = c.request("shutdown", "null");
    assert!(r.contains(r#""result":null"#));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn formal_ready_diagnostics_and_quick_fixes() {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-fpv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dir = dir.canonicalize().unwrap();
    let file = dir.join("fifo_fpv.sv");
    let src = "module fifo_fpv (\n\
  input  logic clk,\n\
  input  logic rst_ni,\n\
  input  logic req,\n\
  output logic gnt\n\
);\n\
  always_ff @(posedge clk or negedge rst_ni) begin\n\
    if (!rst_ni) gnt <= 1'b0;\n\
    else         gnt <= req;\n\
  end\n\
  m_bad_out: assume property (@(posedge clk) disable iff (!rst_ni) !gnt);\n\
  a_no_rst:  assert property (@(posedge clk) req |=> gnt);\n\
endmodule\n";
    std::fs::write(&file, src).unwrap();

    let mut c = Client::start_with_opts(&dir, r#"{"preset":"formal-ready"}"#);
    let open = format!(
        r#"{{"textDocument":{{"uri":"file://{}","languageId":"systemverilog","version":1,"text":{}}}}}"#,
        file.display(),
        json_str(src)
    );
    c.notify("textDocument/didOpen", &open);
    let d = c.diagnostics(&file);
    assert!(d.contains(r#""code":"SV-FPV-001""#), "{d}");
    assert!(d.contains(r#""code":"SV-FPV-003""#), "{d}");

    let actions = c.request(
        "textDocument/codeAction",
        &format!(
            r#"{{"textDocument":{{"uri":"file://{}"}},"range":{{"start":{{"line":10,"character":0}},"end":{{"line":11,"character":60}}}},"context":{{"diagnostics":[]}}}}"#,
            file.display()
        ),
    );
    assert!(actions.contains("Replace 'assume' on DUT output/state with 'assert'"), "{actions}");
    assert!(actions.contains(r#""newText":"assert""#), "{actions}");
    assert!(actions.contains("Insert 'disable iff (!rst_ni)'"), "{actions}");
    assert!(actions.contains(r#""newText":") disable iff (!rst_ni)""#), "{actions}");

    // Applying both quick fixes clears SV-FPV-001 and SV-FPV-003.
    let fixed = src.replace("m_bad_out: assume property", "m_bad_out: assert property").replace(
        "a_no_rst:  assert property (@(posedge clk) req",
        "a_no_rst:  assert property (@(posedge clk) disable iff (!rst_ni) req",
    );
    let change = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":2}},"contentChanges":[{{"text":{}}}]}}"#,
        file.display(),
        json_str(&fixed)
    );
    c.notify("textDocument/didChange", &change);
    let d2 = c.diagnostics(&file);
    assert!(!d2.contains(r#""code":"SV-FPV-001""#), "{d2}");
    assert!(!d2.contains(r#""code":"SV-FPV-003""#), "{d2}");

    let _ = c.request("shutdown", "null");
    let _ = std::fs::remove_dir_all(&dir);
}

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[test]
fn semantic_tokens_inactive_ifdef_hierarchies_and_rtl_signal_trace() {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-wave2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let rtl = r#"module child_alu (
  input  logic [7:0] a_i,
  output logic [7:0] y_o
);
  assign y_o = a_i + 8'd1;
endmodule

module top_soc (
  input  logic       clk,
  input  logic [7:0] req_data,
  output logic [7:0] out_data
);
  logic [7:0] pipe_sig;
`ifdef USE_BYPASS
  assign pipe_sig = req_data;
`else
  child_alu u_alu (
    .a_i (req_data),
    .y_o (pipe_sig)
  );
`endif
  always_ff @(posedge clk) begin
    out_data <= pipe_sig;
  end
endmodule
"#;
    let rtl_file = dir.join("soc.sv");
    std::fs::write(&rtl_file, rtl).unwrap();

    let tb = r#"package tb_pkg;
  class base_txn;
    function void helper(); endfunction
  endclass
  class axi_txn extends base_txn;
    function void run_txn();
      helper();
    endfunction
  endclass
endpackage
"#;
    let tb_file = dir.join("tb_pkg.sv");
    std::fs::write(&tb_file, tb).unwrap();

    let mut c = Client::start_with_opts(&dir, r#"{"lintTrigger":"off"}"#);

    // 1. Semantic tokens
    let sem = c.request(
        "textDocument/semanticTokens/full",
        &format!(r#"{{"textDocument":{{"uri":"file://{}"}}}}"#, rtl_file.display()),
    );
    assert!(sem.contains(r#""data":["#) && !sem.contains(r#""data":[]"#), "{sem}");

    // 2. Inactive `ifdef ranges: without USE_BYPASS, the `ifdef branch (line 14) is inactive;
    // once USE_BYPASS is defined via svlang/setLintOptions, the `else branch becomes inactive!
    let inact1 =
        c.request("svlang/inactiveRanges", &format!(r#"{{"textDocument":{{"uri":"file://{}"}}}}"#, rtl_file.display()));
    assert!(inact1.contains(r#""line":14"#), "{inact1}");

    let _ = c.request("svlang/setLintOptions", r#"{"defines":["USE_BYPASS"]}"#);
    let inact2 =
        c.request("svlang/inactiveRanges", &format!(r#"{{"textDocument":{{"uri":"file://{}"}}}}"#, rtl_file.display()));
    assert!(inact2.contains(r#""line":16"#), "{inact2}");

    // 3. RTL module hierarchy (`top_soc` -> `u_alu : child_alu`)
    let hier = c.request("svlang/rtlHierarchy", "{}");
    assert!(hier.contains(r#""moduleName":"top_soc""#), "{hier}");
    assert!(hier.contains(r#""instanceName":"u_alu""#), "{hier}");
    assert!(hier.contains(r#""moduleName":"child_alu""#), "{hier}");

    // 4. Signal driver/load trace on `pipe_sig`:
    //    - `.y_o(pipe_sig)` is a DRIVER (output port of `child_alu`) and `assign pipe_sig = ...` is a DRIVER
    //    - `out_data <= pipe_sig;` is a LOAD!
    let (pl, pc) = pos(rtl, "pipe_sig", 0, 2);
    let trace = c.at("svlang/traceSignal", &rtl_file, pl, pc);
    assert!(trace.contains(r#""name":"pipe_sig""#), "{trace}");
    assert!(trace.contains(r#""drivers":[{"#), "{trace}");
    assert!(trace.contains(r#".y_o (pipe_sig)"#), "{trace}");
    assert!(trace.contains(r#"out_data <= pipe_sig;"#), "{trace}");

    // 5. Type hierarchy on `base_txn` -> subtype `axi_txn`, and `axi_txn` -> supertype `base_txn`
    let (bl, bc) = pos(tb, "base_txn", 0, 2);
    let prep_ty = c.at("textDocument/prepareTypeHierarchy", &tb_file, bl, bc);
    assert!(prep_ty.contains(r#""name":"base_txn""#), "{prep_ty}");
    let item_start = prep_ty.find(r#""result":["#).unwrap() + 10;
    let item_end = prep_ty.rfind(']').unwrap();
    let base_item = &prep_ty[item_start..item_end];
    let subs = c.request("typeHierarchy/subtypes", &format!(r#"{{"item":{base_item}}}"#));
    assert!(subs.contains(r#""name":"axi_txn""#), "{subs}");

    // 6. Call hierarchy on `helper` -> incoming call from `run_txn`
    let (hl, hc) = pos(tb, "helper", 0, 2);
    let prep_call = c.at("textDocument/prepareCallHierarchy", &tb_file, hl, hc);
    assert!(prep_call.contains(r#""name":"helper""#), "{prep_call}");
    let c_start = prep_call.find(r#""result":["#).unwrap() + 10;
    let c_end = prep_call.rfind(']').unwrap();
    let helper_item = &prep_call[c_start..c_end];
    let incoming = c.request("callHierarchy/incomingCalls", &format!(r#"{{"item":{helper_item}}}"#));
    assert!(incoming.contains(r#""name":"run_txn""#), "{incoming}");

    let _ = c.request("shutdown", "null");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_trigger_require_config_ignore_and_fix_all() {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-cfg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Style violation with safe autofix: missing matching end label (SV-STY-104) + trailing space (SV-STY-102).
    let src = "module demo;\n  logic a;   \nendmodule\n";
    let file = dir.join("demo.sv");
    std::fs::write(&file, src).unwrap();

    // Start with `requireConfig: true` and `preset: "style"`: automatic open stays in `standalone` mode (0 diags),
    // then `svlang/lintNow` runs on demand and reports SV-STY-104 & SV-STY-102 + `source.fixAll.svlang`!
    let mut c = Client::start_with_opts(&dir, r#"{"requireConfig":true,"preset":"style"}"#);
    let open = format!(
        r#"{{"textDocument":{{"uri":"file://{}","languageId":"systemverilog","version":1,"text":{}}}}}"#,
        file.display(),
        json_str(src)
    );
    c.notify("textDocument/didOpen", &open);
    let d_initial = c.diagnostics(&file);
    assert!(d_initial.contains(r#""diagnostics":[]"#), "requireConfig should suppress auto-lint: {d_initial}");
    let st = c.request("svlang/status", "{}");
    assert!(st.contains(r#""state":"standalone""#), "{st}");

    // Manual `svlang/lintNow` runs even without a config file!
    let _ = c.request("svlang/lintNow", "{}");
    let d_manual = c.diagnostics(&file);
    assert!(d_manual.contains(r#""code":"SV-STY-102""#), "{d_manual}");

    // `source.fixAll.svlang` returns combined safe fixes!
    let fix_all = c.request(
        "textDocument/codeAction",
        &format!(
            r#"{{"textDocument":{{"uri":"file://{}"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":2,"character":9}}}},"context":{{"diagnostics":[],"only":["source.fixAll.svlang"]}}}}"#,
            file.display()
        ),
    );
    assert!(fix_all.contains(r#""kind":"source.fixAll.svlang""#), "{fix_all}");

    // Adding `SV-STY-102` to `ignore` via `svlang/setLintOptions` (with `requireConfig: false`) silences `SV-STY-102`!
    let _ = c.request("svlang/setLintOptions", r#"{"requireConfig":false,"preset":"style","ignore":["SV-STY-102"]}"#);
    let d_ignored = c.diagnostics(&file);
    assert!(!d_ignored.contains(r#""code":"SV-STY-102""#), "{d_ignored}");

    let _ = c.request("shutdown", "null");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mode1_syntax_diagnostics_keep_compiler_checks_with_lint_off() {
    let dir = std::env::temp_dir().join(format!("svlang-lsp-mode1-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Clean IEEE 1800-2023 SV file that intentionally violates static lint rules
    // (legacy `reg` SV-BAN-001 + trailing space SV-STY-102 + blocking in clocked always SV-COR-001):
    // in Mode 1 (`lintTrigger: "off"`, `syntaxDiagnostics: true`), 0 diagnostics should appear!
    let clean_sv = "module mode1_demo(input logic clk, output logic q);\n  reg r;   \n  always_ff @(posedge clk) q = 1'b0;\nendmodule\n";
    let file = dir.join("mode1_demo.sv");
    std::fs::write(&file, clean_sv).unwrap();

    let mut c = Client::start_with_opts(&dir, r#"{"lintTrigger":"off","syntaxDiagnostics":true,"preset":"all"}"#);
    let open = format!(
        r#"{{"textDocument":{{"uri":"file://{}","languageId":"systemverilog","version":1,"text":{}}}}}"#,
        file.display(),
        json_str(clean_sv)
    );
    c.notify("textDocument/didOpen", &open);
    let d_clean = c.diagnostics(&file);
    assert!(d_clean.contains(r#""diagnostics":[]"#), "Mode 1 must not emit lint rules on open: {d_clean}");
    let st1 = c.request("svlang/status", "{}");
    assert!(st1.contains(r#""state":"syntax""#), "{st1}");

    // Introduce an IEEE 1800 semantic error (undeclared identifier `missing_sig` in a self-contained module):
    // Mode 1 immediately reports `SV-CMP-001` with `source: "svlang"` while still suppressing lint rules!
    let broken_sv =
        "module mode1_demo(input logic clk, output logic q);\n  reg r;   \n  assign q = missing_sig;\nendmodule\n";
    let change = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":2}},"contentChanges":[{{"text":{}}}]}}"#,
        file.display(),
        json_str(broken_sv)
    );
    c.notify("textDocument/didChange", &change);
    let d_broken = c.diagnostics(&file);
    assert!(d_broken.contains(r#""code":"SV-CMP-001""#), "Mode 1 must report compiler diagnostics: {d_broken}");
    assert!(d_broken.contains(r#""source":"svlang""#), "{d_broken}");
    assert!(!d_broken.contains(r#""code":"SV-BAN-001""#), "{d_broken}");
    assert!(!d_broken.contains(r#""code":"SV-STY-102""#), "{d_broken}");

    // Fix the compiler error -> diagnostics clear back to [] while `svlang/lintNow` still runs all lint rules on demand!
    let fix_change = format!(
        r#"{{"textDocument":{{"uri":"file://{}","version":3}},"contentChanges":[{{"text":{}}}]}}"#,
        file.display(),
        json_str(clean_sv)
    );
    c.notify("textDocument/didChange", &fix_change);
    let d_fixed = c.diagnostics(&file);
    assert!(d_fixed.contains(r#""diagnostics":[]"#), "{d_fixed}");

    let _ = c.request("svlang/lintNow", "{}");
    let d_manual = c.diagnostics(&file);
    assert!(d_manual.contains(r#""code":"SV-BAN-001""#), "Manual Lint Now must still run lint rules: {d_manual}");
    assert!(d_manual.contains(r#""source":"svlang-lint""#), "{d_manual}");

    let _ = c.request("shutdown", "null");
    let _ = std::fs::remove_dir_all(&dir);
}
