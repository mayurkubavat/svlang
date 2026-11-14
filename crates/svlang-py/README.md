# pysvlang

Python bindings for [svlang](https://github.com/mayurkubavat/svlang), a
pure-Rust IEEE 1800-2023 SystemVerilog front end: filelists, preprocessor,
a lossless concrete syntax tree, elaboration, compiler checks and a
word-level RTL netlist.

The wheels are self-contained native extensions built with
[PyO3](https://pyo3.rs) and [maturin](https://www.maturin.rs). There is no
C++ build and nothing else to install; one abi3 wheel per platform works on
every Python from 3.9 on.

```sh
pip install pysvlang
```

> Status: alpha. The API below is the first slice (parse, diagnostics,
> tree walking, hierarchy, netlist summary) and may change before 1.0.

## Use

```python
import pysvlang

comp = pysvlang.Compilation(
    ["rtl/top.sv", "rtl/fifo.sv"],
    include_dirs=["rtl/include"],
    defines={"SYNTHESIS": None, "DEPTH": 16},
    top=["top"],
    params={"WIDTH": 32},
)
# Or simulator-style arguments, -f filelists included:
# comp = pysvlang.Compilation.from_args(["-f", "files.f", "-top", "top"])

for d in comp.diagnostics():
    print(d)                       # rtl/top.sv:12:5: error: ...

for tree in comp.trees:
    for mod in tree.root.find_all("ModuleDecl"):
        print(mod.child("Name").bare_text, mod.location)

print("\n".join(comp.hierarchy()))

nl = comp.lower_netlist(top="top")
print(nl.top, len(nl.inputs), len(nl.states))
print(nl.dump())
```

The syntax tree is lossless: `tree.file_text()` rebuilds the source
byte for byte, and every `Token` carries its `leading_trivia` (whitespace,
comments, directives). Node kinds are strings such as `"ModuleDecl"` or
`"ProceduralBlock"`; `pysvlang.SYNTAX_KINDS` and `pysvlang.TOKEN_KINDS` list
them all, and asking for a kind that does not exist raises `ValueError`.

`lower_netlist()` raises `pysvlang.LowerError` when the design has errors or
uses constructs outside the synthesizable/formal subset. The exception's
`diagnostics` list says why, and `unsupported` tells the two cases apart.

`nl.to_btor2()` returns the netlist as a Btor2 transition system for
word-level model checkers, with a list of notes on anything left out:

```python
text, notes = comp.lower_netlist().to_btor2()
open("top.btor2", "w").write(text)      # then: ric3 check top.btor2 ic3
```

`nl.to_aiger()` gives the same model bit-blasted to AIGER, as `bytes`
(binary `.aig`) or, with `binary=False`, as text (`.aag`):

```python
nl = comp.lower_netlist(blast_memories_up_to=1024)   # AIGER has no memories
data, notes = nl.to_aiger()
open("top.aig", "wb").write(data)       # then: ric3 check top.aig ic3
```

`nl.to_yosys_json()` writes the netlist for Yosys, as its word-level cells:

```python
text, notes = nl.to_yosys_json()
open("top.json", "w").write(text)       # then: yosys -p 'read_json top.json; synth; stat'
```

Immediate assertions become `bad` lines. For concurrent properties
(`assert property`, `assume property`, `cover property`), lower with
`compile_properties=True`: each one over bounded sequences becomes a check
that both exporters write, and the notes say how many were left out. A
concurrent assumption that could not be compiled raises `SvlangError`.

```python
nl = comp.lower_netlist(compile_properties=True, skip_cycles=1)
text, notes = nl.to_btor2(covers=True)
```

Parsing, checking and lowering release the GIL, so several compilations
can run in threads at once.

## Build from source

Only needed to work on the bindings. Needs Rust 1.85 or later and Python 3.9 or later.

```sh
cd crates/svlang-py
python -m venv .venv && . .venv/bin/activate
pip install maturin pytest
maturin develop --release
pytest tests
```

`maturin build --release` writes a wheel to `target/wheels/`.

## Releasing

The Python workflow builds and tests the wheels on every change. Pushing a
tag `pysvlang-vX.Y.Z`, where `X.Y.Z` is the workspace version in the root
`Cargo.toml`, also publishes the wheels and sdist to PyPI through trusted
publishing (repository `mayurkubavat/svlang`, workflow `python.yml`,
environment `pypi`).

## License

Apache-2.0, like svlang.
