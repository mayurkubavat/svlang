"""Python bindings for svlang, a pure-Rust IEEE 1800-2023 SystemVerilog front end.

    import pysvlang

    comp = pysvlang.Compilation(["rtl/top.sv"], include_dirs=["rtl"], defines={"SYNTH": None})
    for d in comp.diagnostics():
        print(d)
    for node in comp.trees[0].root.find_all("ModuleDecl"):
        print(node.child("Name").bare_text, node.location)
    nl = comp.lower_netlist()
    print(nl.top, len(nl.states))
"""

from ._pysvlang import (
    Compilation,
    Diagnostic,
    Location,
    LowerError,
    Netlist,
    NetlistDiagnostic,
    Signal,
    SYNTAX_KINDS,
    TOKEN_KINDS,
    SvlangError,
    SyntaxError,
    SyntaxNode,
    SyntaxTree,
    Token,
    __version__,
)

__all__ = [
    "Compilation",
    "Diagnostic",
    "Location",
    "LowerError",
    "Netlist",
    "NetlistDiagnostic",
    "Signal",
    "SYNTAX_KINDS",
    "TOKEN_KINDS",
    "SvlangError",
    "SyntaxError",
    "SyntaxNode",
    "SyntaxTree",
    "Token",
    "__version__",
]
