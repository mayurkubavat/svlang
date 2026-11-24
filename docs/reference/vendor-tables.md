# Measured vendor tables

Simulators accept SystemVerilog that IEEE 1800 forbids, and real code relies
on it. slang (and so svlang-lint) is strict by default and offers one option per
relaxation; `--compat vcs` turns on a fixed bundle of them. These tables say
what each tool actually accepts, measured, not copied from documentation.

Each probe in `testdata/vendor/` is legal SystemVerilog except for one
construct, and names the slang option that accepts it. `tools/quality/vendor_table.py`
compiles every probe with one tool and records the result in
`data/vendor/<tool>.json` with the tool's version and the date:

```
tools/quality/vendor_table.py measure verilator      # or iverilog, vcs, xcelium, questa, slang, svlang
tools/quality/vendor_table.py table                  # this table, from data/vendor/
svlang lint $(tools/quality/vendor_table.py slang-flags data/vendor/vcs.json) -f files.f
```

`slang-flags` prints the compatibility flags for the relaxations a measured tool
accepts, so `svlang` accepts the same code that tool does.

## Results

"no" can also mean the tool fails on something else in the probe (Verilator
5.020 has no covergroups); the JSON keeps each rejection's message.

| Probe | slang option | iverilog | svlang-lint | svlang | verilator |
|---|---|---|---|---|---|
| `array-concat-pattern` | `--allow-array-concat-assign-pattern` | no | no | yes | yes |
| `bare-value-param` | `--allow-bare-value-param-assigment` | no | no | yes | yes |
| `cross-auto-bin-max` | `--allow-cross-auto-bin-max` | no | no | no | no |
| `declarations-after-statements` | `none` | no | no | yes | no |
| `genblk-reference` | `--allow-genblk-reference` | yes | no | yes | yes |
| `hierarchical-const` | `--allow-hierarchical-const` | no | no | yes | no |
| `merging-ansi-ports` | `--allow-merging-ansi-ports` | no | no | no | no |
| `relax-enum` | `--relax-enum-conversions` | no | no | yes | no |
| `relax-string` | `--relax-string-conversions` | no | no | yes | no |
| `self-determined-stream` | `--allow-self-determined-stream-concat` | no | no | yes | yes |
| `toplevel-iface-ports` | `--allow-toplevel-iface-ports` | no | no | no | yes |
| `use-before-declare` | `--allow-use-before-declare` | yes | no | no | yes |

- iverilog: Icarus Verilog version 12.0 (stable) () (measured 2026-11-08)
- svlang-lint: Slang strict reference at 6c930fa (measured 2026-11-08)
- svlang: svlang at v0.5.2 (measured 2026-11-23)
- verilator: Verilator 5.020 2024-01-01 rev (Debian 5.020-1) (measured 2026-11-08)

## Not measured yet

VCS, Xcelium and Questa need their licences, so they are measured on a
machine that has them: `measure vcs` runs `vcs -full64 -sverilog`,
`measure xcelium` runs `xrun -sv -elaborate`, `measure questa` runs
`qrun -sv -optimize`, each with `probe` as the top. Commit the JSON it
writes; the table above picks it up.

## Relaxations accepted by `svlang check`

`svlang check` currently accepts eight simulator-compatibility probes by default
(hierarchical names in constants, implicit enum and string conversions,
declarations after statements, bare `#W` parameter values, implicit `genblk<n>`
references, self-determined streaming, and assignment patterns in array
concatenations).
