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


def test_to_btor2_rejects_two_clocks():
    src = "module two(input logic a, b, d, output logic p, q);\n always_ff @(posedge a) p <= d;\n always_ff @(posedge b) q <= d;\nendmodule\n"
    nl = pysvlang.Compilation.from_source(src, "two.sv").lower_netlist()
    with pytest.raises(pysvlang.SvlangError, match="more than one clock"):
        nl.to_btor2()
