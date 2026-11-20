# svlang-lint

Static analysis for SystemVerilog (IEEE 1800) and UVM (IEEE 1800.2)
verification code.

    bin/svlang-lint      the linter
    bin/dv-lsp        language server: outline, go to definition, references, hover, completion
    bin/dv-build      compiler-style errors from the same front end
    bin/dv-map        the component tree of a UVM testbench
    bin/dv-filelist   checks and normalizes VCS/Xcelium file lists
    python/           the Python SDK that project configs and Python rules import

Put `bin/` on your `PATH` and keep `python/` beside it: `svlang-lint` looks for
the SDK there when a project has a `.svlang-lint/config.py` or you pass
`--python-rules` (both need `python3`; set `SVLANG_LINT_PYTHON` to choose the
interpreter).

    svlang-lint tb/*.sv +incdir+tb --preset methodology
    svlang-lint -f tb.f --json findings.json --sarif findings.sarif
    svlang-lint --init            # write a starter .svlang-lint/config.py for this project
    svlang-lint --explain SV-RND-001

Guide, rule catalog and source: https://github.com/mayurkubavat/svlang
