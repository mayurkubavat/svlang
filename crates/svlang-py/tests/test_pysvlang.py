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
        " c_ab: cover property (@(posedge clk) a ##1 b);\n"
        "endmodule\n"
    )
    comp = pysvlang.Compilation.from_source(src, "cp.sv")
    assert comp.lower_netlist().properties == [
        ("cp.a_next", "assert", False),
        ("cp.a_rng", "assert", False),
        ("cp.c_ab", "cover", False),
    ]
    nl = comp.lower_netlist(compile_properties=True)
    assert [done for _, _, done in nl.properties] == [True, False, True]
    text, notes = nl.to_btor2(covers=True)
    bad = [line.split()[-1] for line in text.splitlines() if " bad " in line]
    assert bad == ["cp.a_next", "cp.c_ab"]
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
