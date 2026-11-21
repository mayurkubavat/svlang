# `svlang` command line

The unified [svlang](https://crates.io/crates/svlang) SystemVerilog (IEEE 1800-2023)
and UVM command line: compile and elaborate, lint and auto-fix, serve LSP to
editors, normalize simulator filelists, map UVM testbenches, and lower RTL + SVA
to netlist / Btor2 / AIGER / Yosys JSON. One self-contained pure-Rust binary.

```sh
cargo install svlang-cli        # installs the `svlang` binary
```

```sh
svlang check    -f sim/top.f -top top              # compiler errors and warnings; exit 1 on errors
svlang check    --json rtl/*.sv                    # diagnostics as JSON for CI and editors
svlang lint     -f sim/top.f --preset formal-ready # static SV/UVM/FPV lint (+ --fix, --sarif, --html)
svlang lsp                                         # Language Server Protocol (LSP) over stdio
svlang filelist -f sim/top.f --args                # validate and normalize VCS/Xcelium filelists
svlang map      -f sim/tb.f --json                 # UVM testbench component hierarchy
svlang preprocess rtl/top.sv +define+SYNTH         # preprocessed source
svlang hier     -f sim/top.f -top top -G W=16      # elaborated instances and parameter values
svlang cst      rtl/fifo.sv                        # lossless concrete syntax tree
svlang netlist  -f sim/top.f -top top              # word-level RTL netlist
svlang btor2    -f sim/top.f -top top -o top.btor2
svlang aiger    -f sim/top.f -top top --blast-memories 1024 -o top.aig
svlang yosys    -f sim/top.f -top top --blast-memories 1024 -o top.json
```

The compile arguments are the ones a simulator takes: source files, `-f` and
`-F` file lists, `+incdir+DIR`, `+define+NAME=VALUE`, `-y`/`-v` libraries,
`-top MODULE`, `-G NAME=VALUE`, `--single-unit`, and `--separate-units`.

If invoked via a symlink named `svlang-lint`, `svlang` automatically dispatches
to `svlang lint`.

`svlang help` (and `svlang <command> --help`) lists every option. Exit status:
0 success, 1 errors or lint findings, 2 bad usage.

## License

Apache-2.0.
