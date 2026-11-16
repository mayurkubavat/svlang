# Lowering speed

How long svlang takes on the designs of the [front-end census](../reference/census.md), stage by
stage (`tools/quality/lowering_bench.py`). Every module the census lowers is compiled again as a top,
alone, 5 times in a row; the median is kept. svlang `bb34551689a3`, 2026-11-15, release build, one process at a time (CPU: AMD Ryzen 7 8745HS w/ Radeon 780M Graphics, 16 threads); reference front end slang version 12.1.0+d02b7cd.
Regenerate on a quiet machine; the numbers are times.

1081 tops, 1,571,434 source lines compiled (a file used by several tops is counted for each). All four stages take **32.2 s in total**, 48,834 lines a second; the median top takes 4.9 ms and the slowest 2.27 s.

Where the time goes: parse 9%, elaborate 22%, lower 68%, btor2 1%.

## By design

Seconds are the sum over the design's tops; memory is the highest any one top needed.

| design | tops | lines | parse | elaborate | lower | Btor2 | all | lines/s | peak memory | netlist values |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| picorv32 | 11 | 25,687 | 0.04 | 0.03 | 0.07 | 0.00 | 0.15 | 176,916 | 19 MB | 8,913 |
| serv | 27 | 16,173 | 0.02 | 0.03 | 0.06 | 0.00 | 0.12 | 136,516 | 13 MB | 14,317 |
| biriscv | 34 | 15,439 | 0.03 | 0.03 | 0.10 | 0.01 | 0.17 | 92,765 | 29 MB | 32,277 |
| verilog-axi | 47 | 35,030 | 0.05 | 0.10 | 1.29 | 0.01 | 1.44 | 24,247 | 206 MB | 63,026 |
| scr1 | 29 | 11,341 | 0.03 | 0.04 | 0.07 | 0.00 | 0.14 | 82,723 | 13 MB | 7,895 |
| tech_cells_generic | 9 | 1,200 | 0.00 | 0.00 | 0.00 | 0.00 | 0.01 | 171,331 | 6 MB | 44 |
| common_cells | 182 | 86,719 | 0.19 | 0.23 | 1.19 | 0.02 | 1.63 | 53,140 | 324 MB | 62,584 |
| axi | 6 | 17,274 | 0.05 | 0.02 | 0.03 | 0.00 | 0.10 | 179,876 | 14 MB | 1,013 |
| cvfpu | 7 | 19,392 | 0.03 | 0.04 | 0.10 | 0.00 | 0.18 | 107,159 | 34 MB | 12,538 |
| ibex | 10 | 14,819 | 0.02 | 0.11 | 0.14 | 0.00 | 0.27 | 54,545 | 28 MB | 5,454 |
| cv32e40p | 96 | 61,716 | 0.08 | 0.27 | 1.02 | 0.01 | 1.39 | 44,414 | 350 MB | 60,260 |
| cv32e40x | 31 | 56,656 | 0.06 | 0.38 | 0.47 | 0.00 | 0.91 | 61,981 | 44 MB | 15,783 |
| cva6 | 8 | 25,048 | 0.04 | 0.25 | 0.28 | 0.00 | 0.57 | 43,905 | 60 MB | 2,359 |
| caliptra-rtl | 297 | 464,819 | 1.05 | 2.99 | 12.53 | 0.24 | 16.81 | 27,645 | 316 MB | 1,044,367 |
| opentitan | 263 | 704,791 | 1.28 | 2.50 | 4.34 | 0.08 | 8.20 | 85,980 | 191 MB | 297,279 |
| basejump_stl | 24 | 15,330 | 0.04 | 0.02 | 0.03 | 0.00 | 0.09 | 161,827 | 10 MB | 2,875 |
| **all** | 1081 | 1,571,434 | 3.01 | 7.05 | 21.73 | 0.39 | 32.18 | 48,834 | 350 MB | 1,630,984 |

## Against the reference front end

The front end alone, as a command and by the wall clock: `svlang check` against the reference on the
same files and top. Both parse, elaborate and run their checks; neither lowers. A command's start-up
is part of each time, which matters for the many tops that take a few milliseconds.

| design | tops | svlang check | reference | reference / svlang |
| --- | ---: | ---: | ---: | ---: |
| picorv32 | 11 | 0.10 s | 0.08 s | 0.8x |
| serv | 27 | 0.10 s | 0.15 s | 1.6x |
| biriscv | 34 | 0.11 s | 0.17 s | 1.5x |
| verilog-axi | 47 | 0.23 s | 0.27 s | 1.2x |
| scr1 | 29 | 0.11 s | 0.15 s | 1.4x |
| tech_cells_generic | 9 | 0.02 s | 0.04 s | 2.3x |
| common_cells | 182 | 0.70 s | 1.06 s | 1.5x |
| axi | 6 | 0.08 s | 0.07 s | 0.8x |
| cvfpu | 7 | 0.09 s | 0.09 s | 1.1x |
| ibex | 10 | 0.16 s | 0.07 s | 0.5x |
| cv32e40p | 96 | 0.52 s | 0.58 s | 1.1x |
| cv32e40x | 31 | 0.52 s | 0.22 s | 0.4x |
| cva6 | 8 | 0.32 s | 0.07 s | 0.2x |
| caliptra-rtl | 297 | 4.71 s | 3.05 s | 0.6x |
| opentitan | 263 | 4.46 s | 2.66 s | 0.6x |
| basejump_stl | 24 | 0.10 s | 0.13 s | 1.4x |
| **all** | 1081 | 12.32 s | 8.86 s | 0.7x |

## The slowest tops

| top | files | lines | parse | elaborate | lower | Btor2 | all | netlist values | peak memory |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `caliptra-rtl` `soc_ifc_reg` | 6 | 11,217 | 22.6 | 429.2 | 1,814.9 | 6.9 | 2,273.5 ms | 30,015 | 78 MB |
| `caliptra-rtl` `kv` | 15 | 2,967 | 5.9 | 9.4 | 1,283.9 | 35.9 | 1,335.1 ms | 168,073 | 316 MB |
| `caliptra-rtl` `el2_ifu` | 9 | 5,939 | 12.6 | 301.0 | 810.4 | 0.6 | 1,124.6 ms | 28,024 | 215 MB |
| `caliptra-rtl` `el2_ifu_bp_ctl` | 4 | 2,361 | 5.0 | 221.2 | 665.0 | 0.5 | 891.7 ms | 19,516 | 186 MB |
| `caliptra-rtl` `aes_reduced_round` | 25 | 4,547 | 7.1 | 55.0 | 562.9 | 12.0 | 637.1 ms | 62,006 | 210 MB |
| `opentitan` `aes_reduced_round` | 22 | 4,400 | 6.1 | 41.6 | 528.8 | 11.7 | 588.3 ms | 62,024 | 191 MB |
| `caliptra-rtl` `aes_sub_bytes` | 21 | 4,334 | 6.6 | 52.7 | 517.6 | 11.4 | 588.3 ms | 59,926 | 202 MB |
| `caliptra-rtl` `ecc_top` | 51 | 15,787 | 28.5 | 56.4 | 452.8 | 15.6 | 553.4 ms | 39,064 | 133 MB |
| `opentitan` `aes_sub_bytes` | 18 | 4,187 | 6.0 | 39.5 | 491.3 | 11.2 | 548.0 ms | 59,944 | 183 MB |
| `verilog-axi` `axi_ram` | 1 | 373 | 0.6 | 1.0 | 482.0 | 0.1 | 483.6 ms | 639 | 206 MB |
| `verilog-axi` `axil_ram` | 1 | 183 | 0.3 | 0.5 | 481.8 | 0.0 | 482.7 ms | 455 | 206 MB |
| `caliptra-rtl` `pv` | 14 | 1,877 | 4.2 | 8.8 | 405.6 | 11.2 | 429.8 ms | 48,886 | 137 MB |
| `cv32e40p` `cb_filter` | 5 | 562 | 0.8 | 5.6 | 396.5 | 0.6 | 403.6 ms | 2,544 | 350 MB |
| `common_cells` `cc_cb_filter` | 6 | 710 | 1.4 | 5.2 | 382.1 | 5.1 | 393.8 ms | 16,095 | 323 MB |
| `common_cells` `cb_filter` | 7 | 767 | 1.4 | 5.3 | 381.5 | 5.1 | 393.4 ms | 16,095 | 324 MB |

Stage times in milliseconds.

## The most memory

| top | lines | netlist values | Btor2 lines | peak memory |
| --- | ---: | ---: | ---: | ---: |
| `cv32e40p` `cb_filter` | 562 | 2,544 | 3,111 | 350 MB |
| `common_cells` `cb_filter` | 767 | 16,095 | 15,533 | 324 MB |
| `common_cells` `cc_cb_filter` | 710 | 16,095 | 15,533 | 323 MB |
| `caliptra-rtl` `kv` | 2,967 | 168,073 | 138,111 | 316 MB |
| `caliptra-rtl` `el2_ifu` | 5,939 | 28,024 | 0 | 215 MB |
| `caliptra-rtl` `aes_reduced_round` | 4,547 | 62,006 | 53,122 | 210 MB |
| `verilog-axi` `axi_ram` | 373 | 639 | 0 | 206 MB |
| `verilog-axi` `axil_ram` | 183 | 455 | 0 | 206 MB |
| `caliptra-rtl` `aes_sub_bytes` | 4,334 | 59,926 | 51,350 | 202 MB |
| `opentitan` `aes_reduced_round` | 4,400 | 62,024 | 53,134 | 191 MB |

## Method and limits

- Only tops that lower are timed; a module the census reports as unsupported or rejected stops early
  and would look fast.
- `lower` includes the checks lowering repeats on its own input, and `Btor2` is the writer with its
  default options. Stage times come from inside the process (`svlang stages`), so they leave out its
  start-up; the comparison with the reference uses the wall clock for both.
- One process at a time, files read from a warm cache. The census's own run is in parallel and under
  load, so its times are not these.
