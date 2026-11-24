# svlang

Pure-Rust IEEE 1800-2023 SystemVerilog & UVM compiler, static linter,
language server, and formal/RTL lowering toolchain in one binary.

    bin/svlang        the unified command line (`check`, `lint`, `lsp`, `filelist`,
                      `map`, `preprocess`, `hier`, `cst`, `netlist`, `btor2`,
                      `aiger`, `yosys`, `stages`)
    python/           the Python SDK that `.svlang-lint/config.py` and Python
                      rules import

Put `bin/` on your `PATH` and keep `python/` beside it: `svlang lint` looks for
the SDK there when a project has a `.svlang-lint/config.py` or you pass
`--python-rules` (both need `python3`; set `SVLANG_LINT_PYTHON` to choose the
interpreter).

    svlang check -f sim/top.f -top top
    svlang lint tb/*.sv +incdir+tb --preset methodology
    svlang lint -f tb.f --json findings.json --sarif findings.sarif
    svlang lint --init            # write a starter .svlang-lint/config.py
    svlang lint --explain SVL-RND-001
    svlang lsp                    # serve LSP over stdio to any editor
    svlang btor2 -f rtl.f -top top -o top.btor2

Tip: symlinking `svlang-lint -> svlang` automatically invokes `svlang lint`.

Guide, rule catalog and source: https://github.com/mayurkubavat/svlang
