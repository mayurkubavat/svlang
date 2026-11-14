# Front-end census

What happens when every module of a set of open RTL designs is made a top and lowered to
the word-level netlist (`tools/quality/census.py`, designs in `tools/quality/census_corpus.json`).
svlang `4ba1e5f9dd24`, 2026-11-14, reference front end slang version 12.1.0+d02b7cd --compat=vcs --timescale=1ns/1ps. Regenerate after a fix; the designs are kept outside the repository.

2184 modules. 610 cannot stand alone as a top (see *Invalid as a top*), which leaves 1574 measured: **987 lowered (62.7%)**, 374 unsupported, 184 wrongly rejected, 0 crashes, 9 out of memory, 0 timeouts, 20 lowered although the reference rejects the input.

## Designs

| design | commit | license | modules | lowered | unsupported | rejected | memout | accepted | invalid | lowered of measured |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [picorv32](https://github.com/YosysHQ/picorv32) | `ef203c2b0a` | ISC | 21 | 11 | 0 | 4 | 0 | 0 | 6 | 73% |
| [serv](https://github.com/olofk/serv) | `f200eb2ed7` | ISC | 73 | 27 | 5 | 0 | 0 | 0 | 41 | 84% |
| [biriscv](https://github.com/ultraembedded/biriscv) | `6af9c4be5a` | Apache-2.0 | 46 | 28 | 10 | 4 | 0 | 0 | 4 | 67% |
| [verilog-axi](https://github.com/alexforencich/verilog-axi) | `516bd5dadc` | MIT | 55 | 41 | 12 | 2 | 0 | 0 | 0 | 75% |
| [scr1](https://github.com/syntacore/scr1) | `ebb5e3551a` | SHL-2.0 | 38 | 29 | 0 | 8 | 0 | 0 | 1 | 78% |
| [tech_cells_generic](https://github.com/pulp-platform/tech_cells_generic) | `55cb54513e` | SHL-0.51 | 12 | 9 | 2 | 1 | 0 | 0 | 0 | 75% |
| [common_cells](https://github.com/pulp-platform/common_cells) | `e73baaec2c` | SHL-0.51 | 200 | 182 | 7 | 1 | 0 | 4 | 6 | 94% |
| [axi](https://github.com/pulp-platform/axi) | `ee000c0eba` | SHL-0.51 | 118 | 5 | 0 | 1 | 0 | 0 | 112 | 83% |
| [cvfpu](https://github.com/openhwgroup/cvfpu) | `532f0f88b4` | SHL-0.51 | 12 | 6 | 2 | 1 | 0 | 0 | 3 | 67% |
| [ibex](https://github.com/lowRISC/ibex) | `3d524f19ee` | Apache-2.0 | 30 | 9 | 9 | 9 | 0 | 0 | 3 | 33% |
| [cv32e40p](https://github.com/openhwgroup/cv32e40p) | `b353438ff4` | SHL-0.51 | 123 | 95 | 4 | 10 | 3 | 0 | 11 | 85% |
| [cv32e40x](https://github.com/openhwgroup/cv32e40x) | `d952cd63bc` | SHL-0.51 | 42 | 27 | 1 | 4 | 0 | 0 | 10 | 84% |
| [cva6](https://github.com/openhwgroup/cva6) | `742dc82a0c` | SHL-0.51 | 84 | 9 | 1 | 4 | 0 | 3 | 67 | 53% |
| [caliptra-rtl](https://github.com/chipsalliance/caliptra-rtl) | `f2d4641cfa` | Apache-2.0 | 401 | 240 | 50 | 77 | 4 | 3 | 27 | 64% |
| [opentitan](https://github.com/lowRISC/opentitan) | `0d2b191a4a` | Apache-2.0 | 615 | 247 | 270 | 57 | 0 | 7 | 34 | 43% |
| [basejump_stl](https://github.com/bespoke-silicon-group/basejump_stl) | `8c01732c0d` | BSD-3-Clause | 314 | 22 | 1 | 1 | 2 | 3 | 285 | 76% |
| **all** | | | 2184 | 987 | 374 | 184 | 9 | 20 | 610 | 63% |

## Unsupported constructs

Legal SystemVerilog that lowering does not handle yet, by how many tops each blocks.
*Tops blocked* counts the modules that fail with the diagnostic when made a top, wherever below them it
arises. *Only blocker* counts those with no other diagnostic, so fixing the row alone should free them.
*Modules with it* counts the modules whose own text has the construct.

| tops blocked | only blocker | modules with it | designs | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- |
| 212 | 193 | 10 | 2 | immediate assertion on a non-constant value inside initial block is not supported | `opentitan/hw/ip/prim/rtl/prim_sparse_fsm_flop.sv:64` `` `ASSERT_INIT_NET(AssertConnected_A, unused_assert_connected === 1'b1 \|\| !EnableAlertTriggerSVA) `` |
| 105 | 105 | 54 | 4 | $isunknown requires allow_four_state_ops | `common_cells/src/cc_addr_decode_dync.sv:151` `if (!$isunknown(addr_map_i) && ~config_ongoing_i) begin` |
| 50 | 20 | 16 | 5 | unsupported expression statement or task call inside initial block | `tech_cells_generic/src/rtl/tc_sram.sv:110` `for (int unsigned i = 0; i < NumWords; i++) begin` |
| 22 | 0 | 2 | 2 | non-constant '…' condition inside initial block is not supported | `opentitan/hw/ip/prim/rtl/prim_util_memload.svh:63` `if (show_mem_paths) $display("%m");` |
| 15 | 13 | 7 | 4 | memory write inside asynchronous reset branch is not supported | `biriscv/src/core/biriscv_lsu.v:475` `ram_q[i] <= {(WIDTH) {1'b0}};` |
| 15 | 12 | 10 | 3 | exponentiation '…' with non-constant exponent is not synthesizable | `verilog-axi/rtl/axi_crossbar_addr.v:190` `if ((M_BASE_ADDR_INT[i*ADDR_WIDTH +: ADDR_WIDTH] & (2**M_ADDR_WIDTH[i*32 +: 32]-1)) != 0) begin` |
| 9 | 9 | 7 | 5 | scalarized unpacked array '…' exceeds maximum N elements | `biriscv/src/tcm/tcm_mem_ram.v:52` `reg [63:0] ram [8191:0] /*verilator public*/;` |
| 6 | 5 | 6 | 2 | inout port '…' is not supported in RTL netlist | `picorv32/picosoc/spiflash.v:42` `inout io0, // MOSI` |
| 6 | 5 | 5 | 4 | streaming concatenation '{<< ...}' / '{>> ...}' is not yet supported in RTL netlist lowering | `cv32e40x/rtl/cv32e40x_alu.sv:80` `assign operand_a_rev = {<<{operand_a_i}};` |
| 3 | 0 | 3 | 2 | side-effecting function '…' called inside an expression is not supported | `cv32e40p/rtl/vendor/pulp_platform_fpnew/src/fpnew_cast_multi.sv:446` `if ((input_exp_q >= signed'(fpnew_pkg::int_width(int_fmt_q2) - 1 + op_mod_q2)) // Exponent larger than max ...` |
| 3 | 0 | 1 | 1 | unsupported system function '…' in RTL expression | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:468` `rand_delay_en = $test$plusargs("RV_SYNC_RAND_DELAY");` |
| 2 | 2 | 1 | 1 | sequence local-variable match items '(seq, v = expr)' are not supported | `opentitan/hw/ip/tlul/rtl/tlul_assert.sv:410` `(h2d.a_valid && d2h.a_ready, pre_addr = h2d.a_address)` |
| 2 | 0 | 1 | 1 | unsupported statement 'CaseStmt' inside initial block | `tech_cells_generic/src/rtl/tc_sram.sv:111` `unique case (SimInit)` |
| 1 | 0 | 1 | 1 | loop exceeded max_loop_iters (N) | `axi/src/axi_lite_lfsr.sv:191` `for (int unsigned i = 0; i < Width - 1; i++) begin : gen_comp_conection` |

## Wrongly rejected

svlang reports an error and the reference front end accepts the same files: the input is good and
svlang is wrong, or stricter than the standard asks.

| tops blocked | only blocker | modules with it | designs | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- |
| 28 | 28 | 24 | 6 | combinational loop detected | `cv32e40x/rtl/cv32e40x_controller_bypass.sv:334` `ctrl_byp_o.jalr_fw_mux_sel = SELJ_FW_WB;` |
| 21 | 14 | 5 | 2 | two-event clocked block must have top-level 'if (reset) ... else ...' structure | `caliptra-rtl/src/doe/rtl/doe_key_mem.sv:132` `always @ (posedge clk or negedge reset_n)` |
| 19 | 7 | 26 | 4 | parameter '…' has no constant value | `tech_cells_generic/src/rtl/tc_clk.sv:108` `parameter int unsigned Delay = 300ps` |
| 15 | 0 | 3 | 2 | cannot resolve hierarchical or member reference '…' | `caliptra-rtl/src/caliptra_prim/rtl/caliptra_prim_sparse_fsm_flop.sv:48` `for (int i = 0, StateEnumT t = t.first(); i < t.num(); i += 1, t = t.next()) begin` |
| 14 | 0 | 2 | 2 | unsupported call target '…' | `caliptra-rtl/src/caliptra_prim/rtl/caliptra_prim_sparse_fsm_flop.sv:48` `for (int i = 0, StateEnumT t = t.first(); i < t.num(); i += 1, t = t.next()) begin` |
| 12 | 12 | 3 | 2 | cannot index non-array value | `caliptra-rtl/src/aes/rtl/aes_pkg.sv:698` `vec_c[i] = vec_c[i] ^ (mat_a[j][i] & vec_b[7-j]);` |
| 11 | 11 | 4 | 2 | identifier '…' used before its declaration | `biriscv/src/dcache/dcache_core.v:186` `if ((mem_rd_m_q \|\| (mem_wr_m_q != 4'b0)) && !tag_hit_any_m_w)` |
| 9 | 3 | 5 | 2 | unsized unbased literal is illegal inside concatenation | `scr1/src/core/pipeline/scr1_pipe_ialu.sv:467` `? {'0, div_quo_bit}` |
| 8 | 0 | 3 | 2 | undeclared identifier 'KeccakRate' | `opentitan/hw/ip/kmac/rtl/sha3pad.sv:811` `` `ASSERT_INIT(PrefixLessThanBlock_A, PrefixSize/8 < KeccakRate[4]) `` |
| 7 | 5 | 6 | 4 | inferred latch for '…' in always_comb block (not assigned on all paths) | `common_cells/src/deprecated/serial_deglitch.sv:43` `q_o = 1'b1;` |
| 7 | 0 | 10 | 1 | signal '…' has non-synthesizable type 'packed dimension with non-constant bounds' | `opentitan/hw/ip/lc_ctrl/rtl/lc_ctrl_state_decode.sv:33` `logic [$bits(ext_dec_lc_state_t)-1:0] dec_lc_state_buf;` |
| 6 | 4 | 5 | 4 | untyped assignment pattern requires an explicit type prefix or assignment context | `ibex/rtl/ibex_cheriot_ex.sv:258` `'{CTAG: lc_ctag, SD_LM: lc_csdlm, GL_LG: lc_cglg};` |
| 5 | 5 | 5 | 1 | top-level module '…' has unconnected interface port '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/dec/el2_dec_gpr_ctl.sv:43` `el2_regfile_if.veer_gpr_rf regfile,` |
| 5 | 4 | 3 | 2 | undeclared identifier '…' | `opentitan/hw/ip/prim/rtl/prim_diff_encode.sv:38` `.out_o({diff_n_buf, diff_p_buf})` |
| 5 | 0 | 1 | 1 | insufficient elements in unpacked array assignment pattern | `scr1/src/core/pipeline/scr1_pipe_ifu.sv:424` `q_err <= '{SCR1_IFU_Q_SIZE_HALF{1'b0}};` |
| 4 | 0 | 4 | 2 | immediate assertion in initial block is false | `opentitan/hw/ip/keymgr_dpe/rtl/keymgr_dpe_kmac_if.sv:49` `` `ASSERT_INIT(MaxAdvDataWidth_A, MaxAdvDataWidth != 1) `` |
| 4 | 0 | 4 | 1 | undeclared scoped identifier '…' | `ibex/rtl/ibex_cs_registers.sv:26` `parameter ibex_pkg::pmp_cfg_t PMPRstCfg[PMP_MAX_REGIONS] = ibex_pkg::PmpCfgRst,` |
| 3 | 2 | 3 | 3 | continuous assignment LHS must have constant bit/element bounds | `ibex/rtl/ibex_pmp.sv:178` `assign region_addr_mask[r][b] = (csr_pmp_cfg_i[r].mode != PMP_MODE_NAPOT) \|` |
| 3 | 0 | 1 | 1 | expected `)` | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | expected `;` | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | non-synthesizable type '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:457` `function automatic int unsigned path_hash(string s);` |
| 3 | 0 | 1 | 1 | parameter declaration is missing an initializer | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | recursive use of macro `CLP_MAX_OF | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | redefinition of '…' | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | unexpected `"…"`; expected an item | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 3 | 0 | 1 | 1 | unexpected `)`; expected an item | `caliptra-rtl/src/doe/rtl/doe_fsm.sv:79` `` localparam BLOCK_OFFSET_W = `CLP_MAX_OF(HEK_BLOCK_OFFSET_W, `CLP_MAX_OF(FE_BLOCK_OFFSET_W, UDS_BLOCK_OFFSET... `` |
| 3 | 0 | 1 | 1 | unexpected `endmodule` | `picorv32/picosoc/picosoc.v:220` `endmodule` |
| 3 | 0 | 1 | 1 | unknown macro or directive | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 3 | 0 | 1 | 1 | use of undeclared identifier '…' | `picorv32/picosoc/picosoc.v:93` `irq[5] = irq_5;` |
| 2 | 2 | 1 | 1 | no member named '…' in 'struct{logic pp_en;logic od_en;logic pu_en;}i3c_io_pkg::s$N' | `opentitan/hw/ip/i3c/rtl/i3c_controller_trx.sv:1081` `sda_drv_mod.en = sda_drv_d.en & ~(sda_drv_d.od_en ? sda_d : 'b0);` |
| 2 | 2 | 1 | 1 | overlapping drivers on '…': driven by clocked procedural block and always_comb block | `verilog-axi/rtl/axi_vfifo_dec.v:641` `input_valid_int_next <= 1'b0;` |
| 1 | 1 | 1 | 1 | overlapping drivers on '…': driven by clocked procedural block and clocked procedural block | `biriscv/src/dcache/dcache_core_data_ram.v:77` `ram[addr1_i][7:0] <= data1_i[7:0];` |
| 1 | 1 | 1 | 1 | overlapping drivers on '…': driven by continuous assignment and continuous assignment | `caliptra-rtl/src/riscv_core/veer_el2/rtl/dec/el2_dec_decode_ctl.sv:463` `assign dec_i0_predict_p_d.prett[31:1] = dec_i0_brp.prett[31:1];` |
| 1 | 1 | 1 | 1 | signal '…' has non-synthesizable type '…' | `ibex/rtl/ibex_tracer.sv:95` `string file_name;` |
| 1 | 1 | 1 | 1 | unknown module '…' | `cv32e40p/rtl/vendor/pulp_platform_fpnew/src/fpnew_divsqrt_multi.sv:291` `div_sqrt_top_mvp i_divsqrt_lei (` |
| 1 | 0 | 1 | 1 | no member named '…' in 'struct packed{kmac_reg_pkg::kmac_hw2reg_intr_state_reg_t intr_state;kmac_reg_pkg::kmac_hw2reg_cfg_regwen_reg_t cfg_regwen;kmac_reg_pkg::kmac_hw2reg_status_reg_t status;kmac_reg_pkg::kmac_hw2reg_err_code_reg_t err_code;}kmac_reg_pkg::s$N' | `caliptra-rtl/src/sha3/rtl/kmac.sv:563` `assign hw2reg.entropy_refresh_hash_cnt.de = 1'b 1;` |
| 1 | 0 | 1 | 1 | no member named '…' in 'struct packed{kmac_reg_pkg::kmac_reg2hw_intr_state_reg_t intr_state;kmac_reg_pkg::kmac_reg2hw_intr_enable_reg_t intr_enable;kmac_reg_pkg::kmac_reg2hw_intr_test_reg_t intr_test;kmac_reg_pkg::kmac_reg2hw_alert_test_reg_t alert_test;kmac_reg_pkg::kmac_reg2hw_cfg_shadowed_reg_t cfg_shadowed;kmac_reg_pkg::kmac_reg2hw_cmd_reg_t cmd;kmac_reg_pkg::kmac_reg2hw_prefix_mreg_t[N:N] prefix;}kmac_reg_pkg::s$N' | `caliptra-rtl/src/sha3/rtl/kmac.sv:525` `assign unused_key_share1 = ^reg2hw.key_share1;` |
| 1 | 0 | 1 | 1 | part-select width exceeds maximum bit width | `cva6/core/multiplier.sv:126` `MULH, MULHU, MULHSU: result_o = mult_result_q[CVA6Cfg.XLEN*2-1:CVA6Cfg.XLEN];` |
| 1 | 0 | 1 | 1 | signal '…' has non-synthesizable type 'unpacked array exceeds maximum element limit' | `picorv32/picosoc/spiflash.v:102` `reg [7:0] memory [0:16*1024*1024-1];` |
| 1 | 0 | 1 | 1 | signal '…' has non-synthesizable type 'unpacked array with non-constant bounds' | `opentitan/hw/ip/prim_generic/rtl/prim_ram_1r1w.sv:65` `logic [Width-1:0] mem [Depth];` |

4 more diagnostics, in 2 tops, are in the results file.

## Crashes and limits

A top that panics, or needs more than 120 s or 4.0 GB.

| tops | what | example |
| ---: | --- | --- |
| 9 | memout | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/deprecated/find_first_one.sv` `find_first_one` (1 files) |

## Lowered although the reference rejects the input

svlang produced a netlist from input the reference front end calls an error. Either svlang misses a
check it should make, or the reference is stricter than the simulators these designs target.

| tops | reference error | example |
| ---: | --- | --- |
| 7 | unknown macro or compiler directive | `opentitan/hw/ip/usbdev/rtl/usb_fs_tx.sv:267` (top `usb_fs_nb_pe`) |
| 3 | use of undeclared identifier '…' | `basejump_stl/bsg_misc/bsg_mul/bsg_mul_booth_4_block.sv:28` (top `bsg_mul_32_32`) |
| 2 | $error encountered: The clearable N-phase CDC with async resetsynchronization requires at least N synchronizer stages for the FIFO. | `common_cells/src/cc_cdc_2phase_clearable.sv:209` (top `cc_cdc_2phase_clearable_dst`) |
| 2 | $error encountered: The clearable CDC FIFO with async reset synchronization requires at least3 synchronizer stages for the FIFO. | `common_cells/src/cc_cdc_fifo_gray_clearable.sv:265` (top `cc_cdc_fifo_gray_clearable_src`) |
| 1 | invalid member access for type '…' (aka '…') | `cva6/core/aes.sv:194` (top `aes`) |
| 1 | $error encountered: cva6_accel_first_pass_decoder: instantiated non-functional module stub. Please replace this with your accelerator's first pass decoder (or unset ENABLE_ACCELERATOR). | `cva6/core/cva6_accel_first_pass_decoder_stub.sv:30` (top `cva6_accel_first_pass_decoder`) |
| 1 | duplicate definition of '…' [-Wduplicate-definition] | `cva6/core/pmp/include/riscv.sv:15` (top `pmp`) |
| 1 | $error encountered: rvdffe.gen_ff.genblk1: rvdffe must be WIDTH >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:203` (top `rvdffe`) |
| 1 | $error encountered: rvdffie.genblk1: rvdffie must be WIDTH >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:389` (top `rvdffie`) |
| 1 | $error encountered: rvdffiee.genblk1: rvdffie width must be >= N | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:428` (top `rvdffiee`) |

## Invalid as a top

Both front ends reject these: most are modules whose parameters have no usable default, or that need
a file, a define or a generated header the census does not supply. They say nothing about svlang as
tops. Where another top instantiates one, it is still measured there:

- 10 of the 610 are instantiated inside a top that lowered
- 3 hold a construct that blocks a top above them
- 597 are not reached by any measured top

| tops | reference error | example |
| ---: | --- | --- |
| 277 | parameter '…' has no constant value | `basejump_stl` `bsg_imod_range` |
| 96 | invalid member access for type '…' (aka '…') | `axi` `axi_chan_compare` |
| 59 | top-level module '…' has unconnected interface port '…' | `axi` `axi_dumper_intf` |
| 58 | unknown module '…' | `picorv32` `ice40up5k_spram` |
| 19 | unknown macro or compiler directive | `biriscv` `biriscv_fetch` |
| 15 | value must not have any unknown bits | `serv` `serv_rf_ram` |
| 12 | immediate assertion in initial block is false | `cv32e40p` `stream_fork` |
| 12 | value must be positive | `cva6` `ariane_regfile_fpga` |
| 10 | no top-level module found to lower | `biriscv` `RAM16X1D` |
| 7 | duplicate definition of '…' [-Wduplicate-definition] | `cva6` `cva6_ptw` |
| 6 | replication constant can only be zero inside of a concatenation | `cv32e40p` `id_queue` |
| 5 | '…': No such file or directory | `serv` `ice40_pll` |
| 4 | invalid target type '…' (aka '…') for assignment pattern | `axi` `axi_lite_to_axi` |
| 4 | unknown class or package '…' | `cva6` `cva6_hpdcache_if_adapter` |
| 3 | unknown package '…' | `cva6` `cva6_hpdcache_subsystem_l15_adapter` |

## Method and limits

- Each module is compiled as a top with its default parameters, from the file that defines it and the
  files that define the names it uses, found by a text scan. No design's own build flow is run, so a
  configuration a flow would set (a define, a parameter, a generated file) is absent.
- Lowering stops at its first failing stage, so a top's diagnostics are a lower bound: fixing a row can
  uncover the next one.
- A lowered netlist is not checked for correctness here; this census measures coverage, not equivalence.
- Testbench, verification and tool directories are left out by name.
