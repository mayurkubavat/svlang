# Census patches

svlang follows IEEE 1800 and does not take code that only simulators accept. Where a
census design has such code, the code is rewritten to the standard and the rewrite is kept
here as a patch against the design's pinned commit (`census_corpus.json`).
`census.py fetch` applies `<design>.patch` to a clean checkout. The designs themselves
stay outside this repository; each patch is a derived work under its design's own license.

To change one: edit the files in the fetched design, check them with the reference front
end in its strict mode and with `svlang check`, then write `git diff` over the patch.

| patch | license | what was not standard | the rewrite |
| --- | --- | --- | --- |
| `biriscv.patch` | Apache-2.0 | `src/dcache/dcache_core.v` and `src/icache/icache.v` read six signals above their declarations (6.18: a name is declared before it is used) | The declarations move above the first use. A `wire x = expr;` becomes a declaration there and an `assign x = expr;` where the line was. No logic moves. |
| `opentitan.patch` | Apache-2.0 | `pwrmgr.sv` uses `slow_peri_reqs_masked` in assertions above its declaration. `rstmgr_crash_info.sv` selects `slot_sel_i[SlotCntWidth-1:0]`, which is `[-1:0]` when one slot is enough (a crash dump of 32 bits or fewer). | The declaration moves above the assertions. The select is inside a generate `if`; with a single slot the output is `slots[0]`. For the widths the design uses the elaborated logic is the original line. |

Three other modules looked the same and were left alone: their code is right for every
parameter value the design uses, and only the default, a placeholder, is out of range.
They are measured with a legal value instead (`params` in `census_corpus.json`):
`axi_vfifo` (verilog-axi), `mem_stream_to_banks_detailed` (axi) and `sha512` (Caliptra).
cva6's `multiplier` takes its configuration as a struct parameter that defaults to all
zeros, which a command line cannot set; it is listed under `default_only` and counted as
not able to stand alone.
