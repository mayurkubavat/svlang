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
    Check,
    Compilation,
    Diagnostic,
    Liveness,
    Location,
    LowerError,
    Memory,
    MemoryWrite,
    NET_OPS,
    Netlist,
    NetlistDiagnostic,
    Register,
    Scope,
    ScopeSignal,
    Signal,
    SvlangError,
    SYNTAX_KINDS,
    SyntaxError,
    SyntaxNode,
    SyntaxTree,
    Token,
    TOKEN_KINDS,
    Value,
    VALUE_KINDS,
    __version__,
)

__all__ = [
    "Check",
    "Compilation",
    "Diagnostic",
    "Liveness",
    "Location",
    "LowerError",
    "Memory",
    "MemoryWrite",
    "NET_OPS",
    "Netlist",
    "NetlistDiagnostic",
    "Register",
    "Scope",
    "ScopeSignal",
    "Signal",
    "SvlangError",
    "SYNTAX_KINDS",
    "SyntaxError",
    "SyntaxNode",
    "SyntaxTree",
    "Token",
    "TOKEN_KINDS",
    "Value",
    "VALUE_KINDS",
    "__version__",
]
