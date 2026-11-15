# Front-end census

What happens when every module of a set of open RTL designs is made a top and lowered to
the word-level netlist (`tools/quality/census.py`, designs in `tools/quality/census_corpus.json`).
svlang `db957328fc88`, 2026-11-14, reference front end slang version 12.1.0+d02b7cd --timescale=1ns/1ps. Regenerate after a fix; the designs are kept outside the repository.

2184 modules. 611 cannot stand alone as a top (see *Invalid as a top*), which leaves 1573 measured: **1064 lowered (67.6%)**, 360 unsupported, 119 wrongly rejected, 0 crashes, 5 out of memory, 0 timeouts, 25 lowered although the reference rejects the input.

Where the unsupported and wrongly rejected ones stop: 3 at parse, 9 at elaborate, 467 at lower. Of the 1089 netlists, the Btor2 writer takes 959 and refuses 130 (see *Lowered, not written as Btor2*).

With `--skip-initial-checks` `--isunknown-is-false`: 1267 of 1564 lowered (81.0%), 49 unsupported, 208 wrongly rejected.

## Designs

| design | commit | license | modules | lowered | unsupported | rejected | memout | accepted | invalid | lowered of measured |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [picorv32](https://github.com/YosysHQ/picorv32) | `ef203c2b0a` | ISC | 21 | 11 | 0 | 4 | 0 | 0 | 6 | 73% |
| [serv](https://github.com/olofk/serv) | `f200eb2ed7` | ISC | 73 | 27 | 5 | 0 | 0 | 0 | 41 | 84% |
| [biriscv](https://github.com/ultraembedded/biriscv) | `6af9c4be5a` | Apache-2.0 | 46 | 34 | 3 | 3 | 0 | 2 | 4 | 81% |
| [verilog-axi](https://github.com/alexforencich/verilog-axi) | `516bd5dadc` | MIT | 55 | 47 | 2 | 6 | 0 | 0 | 0 | 85% |
| [scr1](https://github.com/syntacore/scr1) | `ebb5e3551a` | SHL-2.0 | 38 | 29 | 0 | 8 | 0 | 0 | 1 | 78% |
| [tech_cells_generic](https://github.com/pulp-platform/tech_cells_generic) | `55cb54513e` | SHL-0.51 | 12 | 9 | 2 | 1 | 0 | 0 | 0 | 75% |
| [common_cells](https://github.com/pulp-platform/common_cells) | `e73baaec2c` | SHL-0.51 | 200 | 182 | 7 | 1 | 0 | 4 | 6 | 94% |
| [axi](https://github.com/pulp-platform/axi) | `ee000c0eba` | SHL-0.51 | 118 | 6 | 0 | 1 | 0 | 0 | 111 | 86% |
| [cvfpu](https://github.com/openhwgroup/cvfpu) | `532f0f88b4` | SHL-0.51 | 12 | 7 | 1 | 1 | 0 | 0 | 3 | 78% |
| [ibex](https://github.com/lowRISC/ibex) | `3d524f19ee` | Apache-2.0 | 30 | 10 | 10 | 7 | 0 | 0 | 3 | 37% |
| [cv32e40p](https://github.com/openhwgroup/cv32e40p) | `b353438ff4` | SHL-0.51 | 123 | 96 | 1 | 10 | 3 | 2 | 11 | 86% |
| [cv32e40x](https://github.com/openhwgroup/cv32e40x) | `d952cd63bc` | SHL-0.51 | 42 | 31 | 0 | 1 | 0 | 0 | 10 | 97% |
| [cva6](https://github.com/openhwgroup/cva6) | `742dc82a0c` | SHL-0.51 | 84 | 8 | 1 | 3 | 0 | 4 | 68 | 50% |
| [caliptra-rtl](https://github.com/chipsalliance/caliptra-rtl) | `f2d4641cfa` | Apache-2.0 | 401 | 281 | 53 | 37 | 0 | 3 | 27 | 75% |
| [opentitan](https://github.com/lowRISC/opentitan) | `0d2b191a4a` | Apache-2.0 | 615 | 262 | 275 | 36 | 0 | 7 | 35 | 45% |
| [basejump_stl](https://github.com/bespoke-silicon-group/basejump_stl) | `8c01732c0d` | BSD-3-Clause | 314 | 24 | 0 | 0 | 2 | 3 | 285 | 83% |
| **all** | | | 2184 | 1064 | 360 | 119 | 5 | 25 | 611 | 68% |

## Unsupported constructs

Legal SystemVerilog that lowering does not handle yet, by how many tops each blocks.
*Tops blocked* counts the modules that fail with the diagnostic when made a top, wherever below them it
arises. *Only blocker* counts those with no other diagnostic, so fixing the row alone should free them.
*Modules with it* counts the modules whose own text has the construct.

| tops blocked | only blocker | modules with it | designs | stage | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- | --- |
| 215 | 203 | 10 | 2 | lower | immediate assertion on a non-constant value inside initial block is not supported | `opentitan/hw/ip/prim/rtl/prim_sparse_fsm_flop.sv:64` `` `ASSERT_INIT_NET(AssertConnected_A, unused_assert_connected === 1'b1 \|\| !EnableAlertTriggerSVA) `` |
| 108 | 108 | 55 | 4 | lower | $isunknown requires allow_four_state_ops | `common_cells/src/cc_addr_decode_dync.sv:151` `if (!$isunknown(addr_map_i) && ~config_ongoing_i) begin` |
| 49 | 19 | 15 | 5 | lower | unsupported expression statement or task call inside initial block | `tech_cells_generic/src/rtl/tc_sram.sv:110` `for (int unsigned i = 0; i < NumWords; i++) begin` |
| 22 | 0 | 2 | 2 | lower | non-constant '…' condition inside initial block is not supported | `opentitan/hw/ip/prim/rtl/prim_util_memload.svh:63` `if (show_mem_paths) $display("%m");` |
| 9 | 9 | 7 | 5 | lower | scalarized unpacked array '…' exceeds maximum N elements | `biriscv/src/tcm/tcm_mem_ram.v:52` `reg [63:0] ram [8191:0] /*verilator public*/;` |
| 6 | 5 | 6 | 2 | lower | inout port '…' is not supported in RTL netlist | `picorv32/picosoc/spiflash.v:42` `inout io0, // MOSI` |
| 3 | 2 | 3 | 2 | lower | side-effecting function '…' called inside an expression is not supported | `cv32e40p/rtl/vendor/pulp_platform_fpnew/src/fpnew_cast_multi.sv:446` `if ((input_exp_q >= signed'(fpnew_pkg::int_width(int_fmt_q2) - 1 + op_mod_q2)) // Exponent larger than max ...` |
| 3 | 0 | 1 | 1 | lower | unsupported system function '…' in RTL expression | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:468` `rand_delay_en = $test$plusargs("RV_SYNC_RAND_DELAY");` |
| 2 | 2 | 1 | 1 | lower | sequence local-variable match items '(seq, v = expr)' are not supported | `opentitan/hw/ip/tlul/rtl/tlul_assert.sv:410` `(h2d.a_valid && d2h.a_ready, pre_addr = h2d.a_address)` |
| 2 | 0 | 1 | 1 | lower | unsupported statement 'CaseStmt' inside initial block | `tech_cells_generic/src/rtl/tc_sram.sv:111` `unique case (SimInit)` |
| 1 | 1 | 1 | 1 | lower | unpacked array slice is not yet supported in RTL netlist lowering | `opentitan/hw/ip/rram_ctrl/rtl/rram_ctrl_mp.sv:145` `.region_cfg_i(HwOtpDataCfg[HwOtpDataRules-1:0]),` |
| 1 | 0 | 1 | 1 | lower | loop exceeded max_loop_iters (N) | `axi/src/axi_lite_lfsr.sv:191` `for (int unsigned i = 0; i < Width - 1; i++) begin : gen_comp_conection` |

## Wrongly rejected

svlang reports an error and the reference front end accepts the same files: the input is good and
svlang is wrong, or stricter than the standard asks.

| tops blocked | only blocker | modules with it | designs | stage | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- | --- |
| 21 | 11 | 26 | 4 | lower | parameter '…' has no constant value | `tech_cells_generic/src/rtl/tc_clk.sv:108` `parameter int unsigned Delay = 300ps` |
| 17 | 0 | 2 | 2 | lower | cannot resolve hierarchical or member reference '…' | `caliptra-rtl/src/caliptra_prim/rtl/caliptra_prim_sparse_fsm_flop.sv:48` `for (int i = 0, StateEnumT t = t.first(); i < t.num(); i += 1, t = t.next()) begin` |
| 17 | 0 | 2 | 2 | lower | unsupported call target '…' | `caliptra-rtl/src/caliptra_prim/rtl/caliptra_prim_sparse_fsm_flop.sv:48` `for (int i = 0, StateEnumT t = t.first(); i < t.num(); i += 1, t = t.next()) begin` |
| 10 | 0 | 13 | 1 | lower | signal '…' has non-synthesizable type 'packed dimension with non-constant bounds' | `opentitan/hw/ip/lc_ctrl/rtl/lc_ctrl_state_decode.sv:33` `logic [$bits(ext_dec_lc_state_t)-1:0] dec_lc_state_buf;` |
| 9 | 3 | 5 | 2 | lower | unsized unbased literal is illegal inside concatenation | `scr1/src/core/pipeline/scr1_pipe_ialu.sv:467` `? {'0, div_quo_bit}` |
| 8 | 8 | 3 | 2 | lower | combinational loop detected | `verilog-axi/rtl/axi_crossbar_addr.v:277` `wire [S_INT_THREADS-1:0] thread_trans_start;` |
| 6 | 5 | 5 | 4 | lower | inferred latch for '…' in always_comb block (not assigned on all paths) | `common_cells/src/deprecated/serial_deglitch.sv:43` `q_o = 1'b1;` |
| 6 | 4 | 5 | 4 | lower | untyped assignment pattern requires an explicit type prefix or assignment context | `ibex/rtl/ibex_cheriot_ex.sv:258` `'{CTAG: lc_ctag, SD_LM: lc_csdlm, GL_LG: lc_cglg};` |
| 5 | 5 | 5 | 1 | elaborate | top-level module '…' has unconnected interface port '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/dec/el2_dec_gpr_ctl.sv:43` `el2_regfile_if.veer_gpr_rf regfile,` |
| 5 | 0 | 1 | 1 | lower | insufficient elements in unpacked array assignment pattern | `scr1/src/core/pipeline/scr1_pipe_ifu.sv:424` `q_err <= '{SCR1_IFU_Q_SIZE_HALF{1'b0}};` |
| 4 | 0 | 4 | 2 | lower | immediate assertion in initial block is false | `opentitan/hw/ip/keymgr_dpe/rtl/keymgr_dpe_kmac_if.sv:49` `` `ASSERT_INIT(MaxAdvDataWidth_A, MaxAdvDataWidth != 1) `` |
| 3 | 3 | 1 | 1 | lower | overlapping drivers on '…': driven by clocked procedural block and clocked procedural block | `biriscv/src/dcache/dcache_core_data_ram.v:77` `ram[addr1_i][7:0] <= data1_i[7:0];` |
| 3 | 3 | 2 | 2 | lower | undeclared identifier '…' | `opentitan/hw/ip/prim/rtl/prim_diff_encode.sv:38` `.out_o({diff_n_buf, diff_p_buf})` |
| 3 | 2 | 3 | 3 | lower | continuous assignment LHS must have constant bit/element bounds | `ibex/rtl/ibex_pmp.sv:178` `assign region_addr_mask[r][b] = (csr_pmp_cfg_i[r].mode != PMP_MODE_NAPOT) \|` |
| 3 | 0 | 1 | 1 | lower | non-synthesizable type '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:457` `function automatic int unsigned path_hash(string s);` |
| 3 | 0 | 1 | 1 | parse | unexpected `"…"`; expected an item | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 3 | 0 | 1 | 1 | parse | unexpected `endmodule` | `picorv32/picosoc/picosoc.v:220` `endmodule` |
| 3 | 0 | 1 | 1 | parse | unknown macro or directive | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 2 | 2 | 1 | 1 | elaborate | no member named '…' in 'struct{logic pp_en;logic od_en;logic pu_en;}i3c_io_pkg::s$N' | `opentitan/hw/ip/i3c/rtl/i3c_controller_trx.sv:1081` `sda_drv_mod.en = sda_drv_d.en & ~(sda_drv_d.od_en ? sda_d : 'b0);` |
| 2 | 2 | 1 | 1 | lower | overlapping drivers on '…': driven by clocked procedural block and always_comb block | `verilog-axi/rtl/axi_vfifo_dec.v:641` `input_valid_int_next <= 1'b0;` |
| 2 | 0 | 1 | 1 | lower | signal '…' has non-synthesizable type 'unpacked array with non-constant bounds' | `opentitan/hw/ip/prim_generic/rtl/prim_ram_1r1w.sv:65` `logic [Width-1:0] mem [Depth];` |
| 1 | 1 | 1 | 1 | lower | cannot index non-array value | `opentitan/hw/ip/prim/rtl/prim_sdc_example.sv:228` `.in_i (ConstIn0[idx]),` |
| 1 | 1 | 1 | 1 | lower | overlapping drivers on '…': driven by continuous assignment and continuous assignment | `caliptra-rtl/src/riscv_core/veer_el2/rtl/dec/el2_dec_decode_ctl.sv:463` `assign dec_i0_predict_p_d.prett[31:1] = dec_i0_brp.prett[31:1];` |
| 1 | 1 | 1 | 1 | lower | signal '…' has non-synthesizable type '…' | `ibex/rtl/ibex_tracer.sv:95` `string file_name;` |
| 1 | 1 | 1 | 1 | lower | two-event clocked block must have top-level 'if (reset) ... else ...' structure | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/delta_counter.sv:31` `always_ff @(posedge clk_i or negedge rst_ni) overflow_q <= ~rst_ni ? 1'b0 : overflow_d;` |
| 1 | 1 | 1 | 1 | elaborate | unknown module '…' | `cv32e40p/rtl/vendor/pulp_platform_fpnew/src/fpnew_divsqrt_multi.sv:291` `div_sqrt_top_mvp i_divsqrt_lei (` |
| 1 | 0 | 1 | 1 | elaborate | no member named '…' in 'struct packed{kmac_reg_pkg::kmac_hw2reg_intr_state_reg_t intr_state;kmac_reg_pkg::kmac_hw2reg_cfg_regwen_reg_t cfg_regwen;kmac_reg_pkg::kmac_hw2reg_status_reg_t status;kmac_reg_pkg::kmac_hw2reg_err_code_reg_t err_code;}kmac_reg_pkg::s$N' | `caliptra-rtl/src/sha3/rtl/kmac.sv:563` `assign hw2reg.entropy_refresh_hash_cnt.de = 1'b 1;` |
| 1 | 0 | 1 | 1 | elaborate | no member named '…' in 'struct packed{kmac_reg_pkg::kmac_reg2hw_intr_state_reg_t intr_state;kmac_reg_pkg::kmac_reg2hw_intr_enable_reg_t intr_enable;kmac_reg_pkg::kmac_reg2hw_intr_test_reg_t intr_test;kmac_reg_pkg::kmac_reg2hw_alert_test_reg_t alert_test;kmac_reg_pkg::kmac_reg2hw_cfg_shadowed_reg_t cfg_shadowed;kmac_reg_pkg::kmac_reg2hw_cmd_reg_t cmd;kmac_reg_pkg::kmac_reg2hw_prefix_mreg_t[N:N] prefix;}kmac_reg_pkg::s$N' | `caliptra-rtl/src/sha3/rtl/kmac.sv:525` `assign unused_key_share1 = ^reg2hw.key_share1;` |
| 1 | 0 | 1 | 1 | lower | signal '…' has non-synthesizable type 'unpacked array exceeds maximum element limit' | `picorv32/picosoc/spiflash.v:102` `reg [7:0] memory [0:16*1024*1024-1];` |

## Lowered, not written as Btor2

The netlist came out and the Btor2 writer (`sv-btor2`) refused it: the netlist holds something
its transition system has no form for yet.

| tops | designs | what the writer says | example |
| ---: | ---: | --- | --- |
| 81 | 6 | not modelled: more than one clock; every register steps on each model step | `verilog-axi/rtl/axi_vfifo_raw_rd.v` `axi_vfifo_raw_rd` |
| 30 | 3 | not modelled: more than one clock, and the clock of … is not a top-level input | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv` `rvdffpcie` |
| 10 | 3 | not supported yet: concurrent assumptions in the RTL that are not compiled to checks: leaving them out would make the model weaker than the design says | `caliptra-rtl/src/caliptra_prim/rtl/caliptra_prim_arbiter_ppc.sv` `caliptra_prim_arbiter_ppc` |
| 5 | 1 | not supported yet: initial values of memory … kept as an array | `verilog-axi/rtl/axi_ram.v` `axi_ram` |
| 3 | 3 | not modelled: blocks clocked on different edges; every register steps on each model step | `picorv32/picosoc/spimemio.v` `spimemio` |
| 1 | 1 | not modelled: latch …: its value depends on itself within a step | `verilog-axi/rtl/axi_vfifo_enc.v` `axi_vfifo_enc` |

## Crashes and limits

A top that panics, or needs more than 120 s or 4.0 GB.

| tops | what | example |
| ---: | --- | --- |
| 5 | memout | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/deprecated/find_first_one.sv` `find_first_one` (1 files) |

## Lowered although the reference rejects the input

svlang produced a netlist from input the reference front end calls an error. Either svlang misses a
check it should make, or the reference is stricter than the simulators these designs target.

| tops | reference error | example |
| ---: | --- | --- |
| 9 | unknown macro or compiler directive | `biriscv/src/core/biriscv_fetch.v:102` (top `biriscv_frontend`) |
| 3 | use of undeclared identifier '…' | `basejump_stl/bsg_misc/bsg_mul/bsg_mul_booth_4_block.sv:28` (top `bsg_mul_32_32`) |
| 2 | $error encountered: The clearable N-phase CDC with async resetsynchronization requires at least N synchronizer stages for the FIFO. | `common_cells/src/cc_cdc_2phase_clearable.sv:209` (top `cc_cdc_2phase_clearable_dst`) |
| 2 | $error encountered: The clearable CDC FIFO with async reset synchronization requires at least3 synchronizer stages for the FIFO. | `common_cells/src/cc_cdc_fifo_gray_clearable.sv:265` (top `cc_cdc_fifo_gray_clearable_src`) |
| 2 | cannot refer to element N of '…' [-Windex-oob] | `cv32e40p/rtl/cv32e40p_register_file_ff.sv:89` (top `cv32e40p_register_file`) |
| 1 | invalid member access for type '…' (aka '…') | `cva6/core/aes.sv:194` (top `aes`) |
| 1 | $error encountered: cva6_accel_first_pass_decoder: instantiated non-functional module stub. Please replace this with your accelerator's first pass decoder (or unset ENABLE_ACCELERATOR). | `cva6/core/cva6_accel_first_pass_decoder_stub.sv:30` (top `cva6_accel_first_pass_decoder`) |
| 1 | scalar type cannot be indexed [-Wcannot-index-scalar] | `cva6/core/cvxif_example/copro_alu.sv:60` (top `copro_alu`) |
| 1 | duplicate definition of '…' [-Wduplicate-definition] | `cva6/core/pmp/include/riscv.sv:15` (top `pmp`) |
| 1 | $error encountered: rvdffe.gen_ff.genblk1: rvdffe must be WIDTH >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:203` (top `rvdffe`) |
| 1 | $error encountered: rvdffie.genblk1: rvdffie must be WIDTH >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:389` (top `rvdffie`) |
| 1 | $error encountered: rvdffiee.genblk1: rvdffie width must be >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:428` (top `rvdffiee`) |

## Invalid as a top

Both front ends reject these: most are modules whose parameters have no usable default, or that need
a file, a define or a generated header the census does not supply. They say nothing about svlang as
tops. Where another top instantiates one, it is still measured there:

- 19 of the 611 are instantiated inside a top that lowered
- 2 hold a construct that blocks a top above them
- 590 are not reached by any measured top

| tops | reference error | example |
| ---: | --- | --- |
| 277 | parameter '…' has no constant value | `basejump_stl` `bsg_imod_range` |
| 93 | invalid member access for type '…' (aka '…') | `axi` `axi_chan_compare` |
| 58 | unknown module '…' | `picorv32` `ice40up5k_spram` |
| 55 | top-level module '…' has unconnected interface port '…' | `axi` `axi_dumper_intf` |
| 19 | unknown macro or compiler directive | `biriscv` `biriscv_fetch` |
| 15 | value must not have any unknown bits | `serv` `serv_rf_ram` |
| 12 | immediate assertion in initial block is false | `cv32e40p` `stream_fork` |
| 12 | value must be positive | `cva6` `ariane_regfile_fpga` |
| 10 | no top-level module found to lower | `biriscv` `RAM16X1D` |
| 7 | duplicate definition of '…' [-Wduplicate-definition] | `cva6` `cva6_ptw` |
| 5 | '…': No such file or directory | `serv` `ice40_pll` |
| 5 | replication constant can only be zero inside of a concatenation | `cv32e40p` `id_queue` |
| 4 | invalid target type '…' (aka '…') for assignment pattern | `axi` `axi_lite_to_axi` |
| 4 | unknown class or package '…' | `cva6` `cva6_hpdcache_if_adapter` |
| 3 | cannot select range of N elements from '…' (aka '…') [-Wrange-width-oob] | `axi` `axi_sim_mem_intf` |

## Method and limits

- Each module is compiled as a top with its default parameters, from the file that defines it and the
  files that define the names it uses, found by a text scan. No design's own build flow is run, so a
  configuration a flow would set (a define, a parameter, a generated file) is absent.
- Lowering stops at its first failing stage, so a top's diagnostics are a lower bound: fixing a row can
  uncover the next one.
- A lowered netlist is not checked for correctness here; this census measures coverage, not equivalence.
- Testbench, verification and tool directories are left out by name.
