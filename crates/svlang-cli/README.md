# `svlang` command line

The [svlang](https://crates.io/crates/svlang) SystemVerilog front end as a
command: check a design, look at what the compiler sees, and write the design
out for formal tools. One self-contained binary, no C++ and no other crates.

```sh
cargo install svlang-cli        # installs the `svlang` binary
```

```sh
svlang check   -f sim/top.f -top top           # errors and warnings; exit 1 on errors
svlang check   --json rtl/*.sv                 # the same as a JSON array, for CI and editors
svlang preprocess rtl/top.sv +define+SYNTH    # the source after macros, includes and `ifdef
svlang hier    -f sim/top.f -top top -G W=16   # elaborated instances and parameter values
svlang cst     rtl/fifo.sv                     # the lossless syntax tree
svlang netlist -f sim/top.f -top top           # the word-level RTL netlist
svlang btor2   -f sim/top.f -top top -o top.btor2
svlang aiger   -f sim/top.f -top top --blast-memories 1024 -o top.aig
svlang yosys   -f sim/top.f -top top --blast-memories 1024 -o top.json   # yosys: read_json
```

The compile arguments are the ones a simulator takes: source files, `-f` and
`-F` file lists, `+incdir+DIR`, `+define+NAME=VALUE`, `-y`/`-v` libraries,
`-top MODULE`, `-G NAME=VALUE` and `--single-unit`.

`btor2` and `aiger` write the model that
[`sv-btor2`](https://crates.io/crates/sv-btor2) and
[`sv-aiger`](https://crates.io/crates/sv-aiger) describe, ready for a model
checker (`ric3 check top.btor2 ic3`). `--cut PATH`, `--black-box MODULE` and
`--black-box-instance PATH` free a signal or leave a module out, and
`--blast-memories N` expands memories into registers.

`--properties` compiles the design's concurrent properties
(`assert property`, `assume property`, `cover property`) over bounded
sequences into checks, so they are written too; `--skip-cycles N` leaves out
attempts that start in the first N clock ticks, and `--covers` writes covers
as reachable bad states. Properties that are not compiled are listed as
notes.

`svlang help` lists every option. Exit status: 0 success, 1 the design has
errors or cannot be written in the requested form, 2 bad usage.

## License

Apache-2.0.
