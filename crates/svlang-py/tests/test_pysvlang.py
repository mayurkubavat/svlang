import pytest

import pysvlang

COUNTER = """// a counter
module counter #(parameter int W = 4) (input logic clk, rst, output logic [W-1:0] q);
  always_ff @(posedge clk) if (rst) q <= '0; else q <= q + 1'b1;
endmodule
"""


@pytest.fixture
def comp():
    return pysvlang.Compilation.from_source(COUNTER, "counter.sv")


def test_clean_source_has_no_diagnostics(comp):
    assert comp.diagnostics() == []
    assert not comp.has_syntax_errors


def test_hierarchy(comp):
    assert comp.hierarchy() == ["$root instance counter : counter", "counter parameter W = 4"]


def test_tree_is_lossless(comp):
    (tree,) = comp.trees
    assert tree.file_text() == COUNTER
    assert tree.root.kind == "SourceFile"
    # Trivia after the last token (the final newline) belongs to no node.
    assert tree.root.text == COUNTER.rstrip("\n")


def test_walk_and_find(comp):
    root = comp.trees[0].root
    (mod,) = root.find_all("ModuleDecl")
    assert mod.child("Name").bare_text == "counter"
    assert [n.kind for n in mod.walk()].count("ProceduralBlock") == 1
    assert mod in root.walk()
    assert mod == root.find_all("ModuleDecl")[0]
    assert len({mod, root.find_all("ModuleDecl")[0]}) == 1
    assert mod.tree.root == root


def test_children_mix_nodes_and_tokens(comp):
    mod = comp.trees[0].root.find_all("ModuleDecl")[0]
    first = mod.children[0]
    assert isinstance(first, pysvlang.Token)
    assert (first.kind, first.text) == ("Keyword", "module")
    assert first.leading_trivia == "// a counter\n"
    assert isinstance(mod.children[1], pysvlang.SyntaxNode)


def test_locations(comp):
    mod = comp.trees[0].root.find_all("ModuleDecl")[0]
    loc = mod.location
    assert (loc.file, loc.line, loc.column) == ("counter.sv", 2, 1)
    assert loc.expansion_file is None


def test_unknown_kind_raises(comp):
    with pytest.raises(ValueError, match="unknown syntax kind"):
        comp.trees[0].root.find_all("ModuleDeclaration")


def test_kind_lists():
    assert "ModuleDecl" in pysvlang.SYNTAX_KINDS
    assert "Identifier" in pysvlang.TOKEN_KINDS


def test_syntax_error_reported():
    c = pysvlang.Compilation.from_source("module m; assign x = ; endmodule", "bad.sv")
    assert c.has_syntax_errors
    (d,) = c.diagnostics()
    assert (d.file, d.line, d.column, d.stage, d.severity) == ("bad.sv", 1, 22, "parser", "error")
    (e,) = c.trees[0].errors
    assert e.location.column == 22
    assert str(d).startswith("bad.sv:1:22: error:")


def test_driver_options():
    c = pysvlang.Compilation(
        sources={"a.sv": "`ifdef WIDE\nmodule a #(parameter P = 1) (); endmodule\n`endif\n"},
        defines={"WIDE": None},
        params={"P": "7"},
        top=["a"],
    )
    assert c.tops == ["a"]
    assert c.hierarchy() == ["$root instance a : a", "a parameter P = 7"]


def test_files_and_include_dirs(tmp_path):
    (tmp_path / "inc").mkdir()
    (tmp_path / "inc" / "w.svh").write_text("`define W 3\n")
    top = tmp_path / "top.sv"
    top.write_text('`include "w.svh"\nmodule top (input logic [`W-1:0] a); endmodule\n')
    c = pysvlang.Compilation([str(top)], include_dirs=[str(tmp_path / "inc")])
    assert c.diagnostics() == []
    assert any(f.endswith("w.svh") for f in c.trees[0].files)


def test_from_args(tmp_path):
    top = tmp_path / "top.sv"
    top.write_text("module top #(parameter N = 1) (); endmodule\n")
    c = pysvlang.Compilation.from_args([str(top), "-GN=5"])
    assert "top parameter N = 5" in c.hierarchy()


def test_missing_file_raises():
    with pytest.raises(pysvlang.SvlangError, match="could not read"):
        pysvlang.Compilation(["/nonexistent/x.sv"])


def test_lower_netlist(comp):
    nl = comp.lower_netlist()
    assert nl.top == "counter"
    assert [(s.name, s.width) for s in nl.inputs] == [("clk", 1), ("rst", 1)]
    assert [(s.name, s.width) for s in nl.outputs] == [("q", 4)]
    assert [(s.name, s.width) for s in nl.states] == [("counter.q", 4)]
    assert nl.dump().startswith("netlist counter")


def test_lower_error_carries_diagnostics():
    c = pysvlang.Compilation.from_source("module m (output logic y); assign y = nope; endmodule", "m.sv")
    with pytest.raises(pysvlang.LowerError) as info:
        c.lower_netlist()
    assert info.value.unsupported is False
    (d,) = info.value.diagnostics
    assert "nope" in d.message and d.file == "m.sv"


def test_cut_points(comp):
    nl = comp.lower_netlist(cut_points=["counter.q"])
    assert nl.top == "counter"


BLACK_BOX = """module leaf (input logic a, output logic y);
  assign y = ~a;
endmodule
module top (input logic a, output logic y);
  leaf u (.a(a), .y(y));
endmodule
"""


@pytest.mark.parametrize("kw", [{"black_box_modules": ["leaf"]}, {"black_box_instances": ["top.u"]}])
def test_black_box(kw):
    c = pysvlang.Compilation.from_source(BLACK_BOX, "bb.sv")
    full = c.lower_netlist(top="top")
    boxed = c.lower_netlist(top="top", **kw)
    assert "not(" in full.dump()
    assert "not(" not in boxed.dump()


SAT_COUNTER = """module cnt (input logic clk, rst, output logic [3:0] q);
  always_ff @(posedge clk) if (rst) q <= 4'd0; else if (q != 4'd9) q <= q + 4'd1;
  always @(posedge clk) in_range: assert (rst || q <= 4'd9);
endmodule
"""


def test_to_btor2():
    nl = pysvlang.Compilation.from_source(SAT_COUNTER, "cnt.sv").lower_netlist()
    text, notes = nl.to_btor2()
    assert any(line.endswith(" in_range") and " bad " in line for line in text.splitlines())
    assert isinstance(notes, list)
    traced, _ = nl.to_btor2(trace_outputs=True, trace_marker="; my-marker")
    assert "; my-marker" in traced


def test_compile_properties():
    src = (
        "module cp(input logic clk, a, b);\n"
        " a_next: assert property (@(posedge clk) a |=> b);\n"
        " a_rng: assert property (@(posedge clk) a |-> ##[1:$] b);\n"
        " a_noclk: assert property (a |-> b);\n"
        " c_ab: cover property (@(posedge clk) a ##1 b);\n"
        "endmodule\n"
    )
    comp = pysvlang.Compilation.from_source(src, "cp.sv")
    assert comp.lower_netlist().properties == [
        ("cp.a_next", "assert", False),
        ("cp.a_rng", "assert", False),
        ("cp.a_noclk", "assert", False),
        ("cp.c_ab", "cover", False),
    ]
    nl = comp.lower_netlist(compile_properties=True)
    assert [done for _, _, done in nl.properties] == [True, True, False, True]
    text, notes = nl.to_btor2(covers=True)
    bad = [line.split()[-1] for line in text.splitlines() if " bad " in line]
    assert bad == ["cp.a_next", "cp.a_rng", "cp.c_ab"]
    assert any(n.startswith("1 concurrent") for n in notes)


def test_to_btor2_both_edges():
    src = "module be(input logic clk, d, output logic p, n);\n always_ff @(posedge clk) p <= d;\n always_ff @(negedge clk) n <= p;\nendmodule\n"
    nl = pysvlang.Compilation.from_source(src, "be.sv").lower_netlist()
    with pytest.raises(pysvlang.SvlangError, match="different edges"):
        nl.to_btor2()
    text, notes = nl.to_btor2(both_edges=True)
    assert "svlang_clock_phase" in text
    assert any("half a clock cycle" in n for n in notes)


def test_to_btor2_several_clocks():
    src = "module two(input logic a, b, d, output logic p, q);\n always_ff @(posedge a) p <= d;\n always_ff @(posedge b) q <= p;\nendmodule\n"
    nl = pysvlang.Compilation.from_source(src, "two.sv").lower_netlist()
    with pytest.raises(pysvlang.SvlangError, match="not a declared clock"):
        nl.to_btor2(clocks=[("a", 1, 0)])
    text, notes = nl.to_btor2(clocks=[("a", 1, 0), ("b", 2, 0)])
    assert text.count(" constraint ") == 2
    assert any("2 clocks" in n for n in notes)


def test_to_btor2_rejects_two_clocks():
    src = "module two(input logic a, b, d, output logic p, q);\n always_ff @(posedge a) p <= d;\n always_ff @(posedge b) q <= d;\nendmodule\n"
    nl = pysvlang.Compilation.from_source(src, "two.sv").lower_netlist()
    with pytest.raises(pysvlang.SvlangError, match="more than one clock"):
        nl.to_btor2()


def test_to_aiger():
    nl = pysvlang.Compilation.from_source(SAT_COUNTER, "cnt.sv").lower_netlist()
    data, notes = nl.to_aiger()
    assert isinstance(data, bytes) and data.startswith(b"aig ")
    assert b"b0 in_range" in data and b"l0 cnt.q[0]" in data
    text, _ = nl.to_aiger(binary=False)
    header = text.splitlines()[0].split()
    # aag M I L O A B C: clk and rst, four register bits, one bad state.
    assert header[0] == "aag" and header[2:4] == ["2", "4"] and header[6] == "1"
    assert isinstance(notes, list)


def test_to_aiger_with_compiled_property():
    src = """module hs(input logic clk, req, output logic ack);
  always_ff @(posedge clk) ack <= req;
  a_ack: assert property (@(posedge clk) req |=> ack);
endmodule
"""
    comp = pysvlang.Compilation.from_source(src, "hs.sv")
    plain, notes = comp.lower_netlist().to_aiger(binary=False)
    assert plain.splitlines()[0].split()[6] == "0" and notes
    text, _ = comp.lower_netlist(compile_properties=True).to_aiger(binary=False)
    assert text.splitlines()[0].split()[6] == "1"
    assert "b0 hs.a_ack" in text


def test_to_yosys_json():
    import json

    nl = pysvlang.Compilation.from_source(SAT_COUNTER, "cnt.sv").lower_netlist()
    text, notes = nl.to_yosys_json()
    module = json.loads(text)["modules"]["cnt"]
    assert module["ports"]["q"] == {"direction": "output", "bits": [4, 5, 6, 7]}
    types = sorted(c["type"] for c in module["cells"].values())
    assert "$dff" in types and "$assert" in types
    assert module["cells"]["in_range"]["type"] == "$assert"
    plain, _ = nl.to_yosys_json(checks=False, src_attributes=False)
    assert "$assert" not in plain and '"src"' not in plain


def test_values(comp):
    nl = comp.lower_netlist()
    assert len(nl.values()) == nl.num_values
    assert nl.value(0) == nl.values()[0] and hash(nl.value(0)) == hash(nl.values()[0])
    with pytest.raises(IndexError):
        nl.value(nl.num_values)
    clk = nl.input("clk")
    assert (clk.kind, clk.width, clk.name, clk.op, clk.operands()) == ("Input", 1, "counter.clk", None, [])
    with pytest.raises(KeyError):
        nl.input("nope")
    q = nl.output("q")
    assert q.kind == "StateOut" and q.register.name == "counter.q"
    assert nl.signal("counter.q") == q
    (add,) = nl.find_ops("Add")
    assert add.kind == "Op" and add.width == 4
    lhs, rhs = add.operands()
    assert lhs == q
    assert (rhs.kind, rhs.bits, rhs.int) == ("Const", "0001", 1)
    assert all(v.id < add.id for v in add.operands())
    assert add.location.file == "counter.sv" and add.location.line == 3
    with pytest.raises(ValueError):
        nl.find_ops("Plus")
    assert {v.kind for v in nl.values()} <= set(pysvlang.VALUE_KINDS)
    assert {v.op for v in nl.values() if v.kind == "Op"} <= set(pysvlang.NET_OPS)
    assert "Mux" in pysvlang.NET_OPS and "Slice" in pysvlang.NET_OPS


def test_registers(comp):
    nl = comp.lower_netlist()
    (reg,) = nl.registers()
    assert [s.name for s in nl.states] == [reg.name]
    assert (reg.name, reg.width, reg.is_latch, reg.edge) == ("counter.q", 4, False, "posedge")
    assert reg.clock == nl.input("clk")
    assert reg.async_reset is None and reg.latch_enable is None
    assert reg.out == nl.output("q")
    assert reg.next.op == "Mux"
    assert reg == nl.registers()[0] and len({reg, nl.registers()[0]}) == 1


def test_async_reset_and_latch():
    src = (
        "module r(input logic clk, rst_n, en, d, output logic q, l);\n"
        " always_ff @(posedge clk or negedge rst_n) if (!rst_n) q <= 1'b1; else q <= d;\n"
        " always_latch if (en) l = d;\n"
        "endmodule\n"
    )
    nl = pysvlang.Compilation.from_source(src, "r.sv").lower_netlist()
    by_name = {r.name: r for r in nl.registers()}
    signal, active_high, value = by_name["r.q"].async_reset
    assert signal == nl.input("rst_n") and not active_high and value.bits == "1"
    latch = by_name["r.l"]
    assert latch.is_latch and latch.clock is None and latch.edge is None
    enable = latch.latch_enable
    assert enable.width == 1 and nl.input("en") in enable.operands()


def test_slice_params():
    src = "module s(input logic [7:0] a, output logic [3:0] y);\n assign y = a[6:3];\nendmodule\n"
    nl = pysvlang.Compilation.from_source(src, "s.sv").lower_netlist()
    (sl,) = nl.find_ops("Slice")
    assert sl.op_params() == {"hi": 6, "lo": 3}
    assert sl.operands() == [nl.input("a")]
    assert nl.input("a").op_params() == {}


def test_memories():
    src = (
        "module m(input logic clk, we, input logic [1:0] wa, ra, input logic [7:0] wd,\n"
        "         output logic [7:0] rd);\n"
        " logic [7:0] mem [0:3];\n"
        " always_ff @(posedge clk) if (we) mem[wa] <= wd;\n"
        " assign rd = mem[ra];\n"
        "endmodule\n"
    )
    nl = pysvlang.Compilation.from_source(src, "m.sv").lower_netlist()
    (mem,) = nl.memories()
    assert (mem.name, mem.width, mem.depth, mem.low_index) == ("m.mem", 8, 4, 0)
    (w,) = mem.writes()
    assert w.clock == nl.input("clk") and w.edge == "posedge"
    assert w.data == nl.input("wd")
    reads = [v for v in nl.values() if v.kind == "MemRead"]
    assert reads and all(v.memory == mem for v in reads)


def test_checks_and_liveness():
    src = (
        "module c(input logic clk, req, gnt);\n"
        " always_comb chk: assert (!(req && gnt));\n"
        " a_next: assert property (@(posedge clk) req |=> gnt);\n"
        " a_live: assert property (@(posedge clk) req |-> s_eventually gnt);\n"
        "endmodule\n"
    )
    nl = pysvlang.Compilation.from_source(src, "c.sv").lower_netlist(compile_properties=True)
    checks = {c.label: c for c in nl.checks()}
    assert checks["chk"].immediate and checks["chk"].kind == "assert"
    prop = checks["c.a_next"]
    assert not prop.immediate and prop.clock == nl.input("clk") and prop.edge == "posedge"
    assert prop.enable.width == 1 and prop.expr.width == 1
    assert prop.location.line == 3
    (live,) = nl.liveness()
    assert (live.label, live.assume, live.edge) == ("c.a_live", False, "posedge")
    assert live.clock == nl.input("clk") and live.signal.width == 1


def test_scope_tree():
    src = (
        "module leaf(input logic a, output logic y);\n logic t;\n assign t = a;\n assign y = t;\nendmodule\n"
        "module top(input logic [3:0] a, output logic [3:0] y);\n"
        " for (genvar i = 0; i < 2; i++) begin : g\n  leaf u(.a(a[i]), .y(y[i]));\n end\n"
        " assign y[3:2] = a[3:2];\n"
        "endmodule\n"
    )
    (root,) = pysvlang.Compilation.from_source(src, "t.sv").scope_tree()
    assert (root.path, root.name, root.module) == ("top", "top", "top")
    ports = {s.name: s for s in root.signals}
    assert (ports["a"].kind, ports["a"].direction, ports["a"].width) == ("port", "input", 4)
    paths = [s.path for s in root.walk()]
    assert paths[0] == "top" and "top.g[0].u" in paths and "top.g[1].u" in paths
    leaf = next(s for s in root.walk() if s.path == "top.g[1].u")
    assert leaf.module == "leaf"
    t = next(s for s in leaf.signals if s.name == "t")
    assert (t.path, t.kind, t.direction) == ("top.g[1].u.t", "var", None)


def test_stubs_match_module():
    """Every public name and member of the native module is in the stub file, and the reverse."""
    import ast
    import pathlib

    stub = pathlib.Path(pysvlang.__file__).with_name("_pysvlang.pyi")
    tree = ast.parse(stub.read_text())
    native = pysvlang._pysvlang
    stub_names, stub_members = set(), {}
    for node in tree.body:
        if isinstance(node, ast.ClassDef):
            stub_names.add(node.name)
            members = set()
            for item in node.body:
                if isinstance(item, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    members.add(item.name)
                elif isinstance(item, ast.AnnAssign):
                    members.add(item.target.id)
            stub_members[node.name] = members
        elif isinstance(node, ast.AnnAssign):
            stub_names.add(node.target.id)
    real_names = {n for n in dir(native) if not n.startswith("_")} | {"__version__"}
    assert stub_names == real_names
    assert set(pysvlang.__all__) == real_names
    ignore = {"__init__", "__eq__", "__hash__", "__repr__", "__str__", "__len__"}
    for name, members in stub_members.items():
        cls = getattr(native, name)
        if issubclass(cls, BaseException):
            continue
        real = {m for m in vars(cls) if not m.startswith("_")}
        assert members - ignore == real, name


def test_separate_units_and_formal_lowering_options(tmp_path):
    f1 = tmp_path / "u1.sv"
    f2 = tmp_path / "u2.sv"
    f1.write_text("`define ONLY_U1 1\nmodule u1; endmodule\n")
    f2.write_text("`ifdef ONLY_U1\nmodule leaked; endmodule\n`else\nmodule clean; endmodule\n`endif\n")
    isolated = pysvlang.Compilation([str(f1), str(f2)], separate_units=True)
    mods = [n.child("Name").bare_text for t in isolated.trees for n in t.root.find_all("ModuleDecl")]
    assert mods == ["u1", "clean"]

    rtl = (
        "module top(input logic clk, a, b, output logic y);\n"
        "  initial assert (a);\n"
        "  assign y = $isunknown(a) ? 1'b0 : b;\n"
        "  ap: assert property (@(posedge clk) a |-> b);\n"
        "endmodule\n"
    )
    nl = pysvlang.Compilation.from_source(rtl, "top.sv").lower_netlist(
        skip_initial_checks=True,
        isunknown_is_false=True,
        compile_properties=True,
        precondition_covers=True,
        witness_covers=True,
    )
    labels = {c.label: c.kind for c in nl.checks()}
    assert labels["top.ap"] == "assert"
    assert labels["top.ap:precondition"] == "cover"
    assert labels["top.ap:witness"] == "cover"
