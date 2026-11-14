# Front-end census

What happens when every module of a set of open RTL designs is made a top and lowered to
the word-level netlist (`tools/quality/census.py`, designs in `tools/quality/census_corpus.json`).
svlang `291f349f1c81`, 2026-11-14, reference front end slang version 12.1.0+d02b7cd --compat=vcs --timescale=1ns/1ps. Regenerate after a fix; the designs are kept outside the repository.

2168 modules. 557 cannot stand alone as a top (see *Invalid as a top*), which leaves 1611 measured: **455 lowered (28.2%)**, 706 unsupported, 342 wrongly rejected, 0 crashes, 5 out of memory, 18 timeouts, 85 lowered although the reference rejects the input.

## Designs

| design | commit | license | modules | lowered | unsupported | rejected | memout | timeout | accepted | invalid | lowered of measured |
| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [picorv32](https://github.com/YosysHQ/picorv32) | `ef203c2b0a` | ISC | 21 | 11 | 0 | 4 | 0 | 0 | 1 | 5 | 69% |
| [serv](https://github.com/olofk/serv) | `f200eb2ed7` | ISC | 69 | 23 | 25 | 0 | 0 | 0 | 17 | 4 | 35% |
| [biriscv](https://github.com/ultraembedded/biriscv) | `6af9c4be5a` | Apache-2.0 | 46 | 28 | 10 | 4 | 0 | 0 | 1 | 3 | 65% |
| [verilog-axi](https://github.com/alexforencich/verilog-axi) | `516bd5dadc` | MIT | 55 | 39 | 15 | 1 | 0 | 0 | 0 | 0 | 71% |
| [scr1](https://github.com/syntacore/scr1) | `ebb5e3551a` | SHL-2.0 | 38 | 27 | 2 | 8 | 0 | 0 | 0 | 1 | 73% |
| [common_cells](https://github.com/pulp-platform/common_cells) | `e73baaec2c` | SHL-0.51 | 200 | 24 | 130 | 13 | 0 | 0 | 22 | 11 | 13% |
| [axi](https://github.com/pulp-platform/axi) | `ee000c0eba` | SHL-0.51 | 118 | 4 | 45 | 1 | 0 | 0 | 0 | 68 | 8% |
| [cvfpu](https://github.com/openhwgroup/cvfpu) | `532f0f88b4` | SHL-0.51 | 12 | 0 | 12 | 0 | 0 | 0 | 0 | 0 | 0% |
| [ibex](https://github.com/lowRISC/ibex) | `3d524f19ee` | Apache-2.0 | 30 | 7 | 11 | 9 | 0 | 0 | 0 | 3 | 26% |
| [cv32e40p](https://github.com/openhwgroup/cv32e40p) | `b353438ff4` | SHL-0.51 | 123 | 43 | 42 | 22 | 3 | 0 | 7 | 6 | 37% |
| [cv32e40x](https://github.com/openhwgroup/cv32e40x) | `d952cd63bc` | SHL-0.51 | 42 | 19 | 5 | 7 | 0 | 0 | 1 | 10 | 59% |
| [cva6](https://github.com/openhwgroup/cva6) | `742dc82a0c` | SHL-0.51 | 84 | 6 | 15 | 3 | 0 | 0 | 3 | 57 | 22% |
| [caliptra-rtl](https://github.com/chipsalliance/caliptra-rtl) | `f2d4641cfa` | Apache-2.0 | 401 | 96 | 121 | 69 | 0 | 18 | 25 | 72 | 29% |
| [opentitan](https://github.com/lowRISC/opentitan) | `0d2b191a4a` | Apache-2.0 | 615 | 111 | 268 | 196 | 0 | 0 | 6 | 34 | 19% |
| [basejump_stl](https://github.com/bespoke-silicon-group/basejump_stl) | `8c01732c0d` | BSD-3-Clause | 314 | 17 | 5 | 5 | 2 | 0 | 2 | 283 | 55% |
| **all** | | | 2168 | 455 | 706 | 342 | 5 | 18 | 85 | 557 | 28% |

## Unsupported constructs

Legal SystemVerilog that lowering does not handle yet, by how many tops each blocks.
*Tops blocked* counts the modules that fail with the diagnostic when made a top, wherever below them it
arises. *Only blocker* counts those with no other diagnostic, so fixing the row alone should free them.
*Modules with it* counts the modules whose own text has the construct.

| tops blocked | only blocker | modules with it | designs | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- |
| 372 | 218 | 220 | 9 | unsupported statement 'AssertStmt' inside initial block | `axi/src/axi_id_prepend.sv:116` `assert(NoBus > 0)` |
| 199 | 141 | 177 | 10 | unsupported procedural statement 'EndLabel' in RTL | `scr1/src/core/pipeline/scr1_pipe_hdu.sv:633` `always_comb begin : csr_if_regsel` |
| 154 | 83 | 104 | 7 | unsupported expression statement or task call inside initial block | `verilog-axi/rtl/axi_crossbar_addr.v:176` `$display("Addressing configuration for axi_crossbar_addr instance %m");` |
| 108 | 53 | 7 | 2 | initial block with timing control ('…', '…', '…') is not synthesizable in RTL | `opentitan/hw/ip/prim/rtl/prim_onehot_check.sv:32` `// This should only be disabled in special circumstances, for example` |
| 64 | 6 | 36 | 6 | unsupported statement 'EndLabel' inside initial block | `common_cells/src/cc_rstgen_bypass.sv:62` `initial begin : p_assertions` |
| 27 | 27 | 18 | 3 | $isunknown requires allow_four_state_ops | `ibex/rtl/ibex_csr.sv:91` |
| 23 | 5 | 20 | 4 | unsupported statement 'LoopStmt' inside initial block | `verilog-axi/rtl/axi_crossbar_addr.v:169` `for (i = 0; i < M_COUNT*M_REGIONS; i = i + 1) begin` |
| 19 | 0 | 2 | 2 | unsupported statement 'DataDecl' inside initial block | `opentitan/hw/ip/prim/rtl/prim_util_memload.svh:59` `logic show_mem_paths;` |
| 8 | 7 | 4 | 2 | memory write inside asynchronous reset branch is not supported | `biriscv/src/core/biriscv_lsu.v:475` `ram_q[i] <= {(WIDTH) {1'b0}};` |
| 8 | 7 | 6 | 4 | scalarized unpacked array '…' exceeds maximum N elements | `biriscv/src/tcm/tcm_mem_ram.v:52` `reg [63:0] ram [8191:0] /*verilator public*/;` |
| 6 | 4 | 6 | 2 | inout port '…' is not supported in RTL netlist | `picorv32/picosoc/spiflash.v:42` `inout io0, // MOSI` |
| 3 | 1 | 2 | 2 | streaming concatenation '{<< ...}' / '{>> ...}' is not yet supported in RTL netlist lowering | `cv32e40x/rtl/cv32e40x_alu.sv:80` `assign operand_a_rev = {<<{operand_a_i}};` |
| 3 | 0 | 1 | 1 | unsupported system function '…' in RTL expression | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:468` `rand_delay_en = $test$plusargs("RV_SYNC_RAND_DELAY");` |

## Wrongly rejected

svlang reports an error and the reference front end accepts the same files: the input is good and
svlang is wrong, or stricter than the standard asks.

| tops blocked | only blocker | modules with it | designs | diagnostic | example |
| ---: | ---: | ---: | ---: | --- | --- |
| 159 | 44 | 57 | 6 | parameter '…' has no constant value | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/lfsr.sv:40` `localparam logic [63:0] Masks [4:64] = '{64'hC,` |
| 119 | 4 | 35 | 3 | signal '…' has non-synthesizable type 'packed dimension with non-constant bounds' | `basejump_stl/bsg_misc/bsg_dff.sv:9` `,input [width_p-1:0] data_i` |
| 68 | 24 | 57 | 6 | two-event clocked block must have top-level 'if (reset) ... else ...' structure | `caliptra-rtl/src/aes/rtl/aes_reg_status.sv:38` `always_ff @(posedge clk_i or negedge rst_ni) begin : reg_ops` |
| 37 | 8 | 14 | 5 | undeclared identifier '…' | `common_cells/src/cc_lossy_valid_to_stream.sv:129` `` `FFARNC(mem_q, mem_d, clr_i, {2{data_t'('0)}}, clk_i, rst_ni) `` |
| 25 | 25 | 24 | 5 | combinational loop detected | `common_cells/src/cc_onehot.sv:30` `logic [Lvls-1:0][2**(Lvls-1)-1:0] sum, carry;` |
| 11 | 11 | 4 | 2 | identifier '…' used before its declaration | `biriscv/src/dcache/dcache_core.v:186` `if ((mem_rd_m_q \|\| (mem_wr_m_q != 4'b0)) && !tag_hit_any_m_w)` |
| 9 | 3 | 5 | 2 | unsized unbased literal is illegal inside concatenation | `scr1/src/core/pipeline/scr1_pipe_ialu.sv:467` `? {'0, div_quo_bit}` |
| 8 | 4 | 3 | 1 | unknown or unsupported type '…' | `opentitan/hw/ip/prim/rtl/prim_mubi_pkg.sv:43` `function automatic logic mubi4_test_true_strict(mubi4_t val);` |
| 5 | 5 | 5 | 1 | top-level module '…' has unconnected interface port '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/dec/el2_dec_gpr_ctl.sv:43` `el2_regfile_if.veer_gpr_rf regfile,` |
| 5 | 0 | 1 | 1 | insufficient elements in unpacked array assignment pattern | `scr1/src/core/pipeline/scr1_pipe_ifu.sv:424` `q_err <= '{SCR1_IFU_Q_SIZE_HALF{1'b0}};` |
| 4 | 4 | 2 | 2 | cannot index non-array value | `caliptra-rtl/src/aes/rtl/aes_pkg.sv:698` `vec_c[i] = vec_c[i] ^ (mat_a[j][i] & vec_b[7-j]);` |
| 4 | 2 | 4 | 3 | inferred latch for '…' in always_comb block (not assigned on all paths) | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/serial_deglitch.sv:45` `q_o = 1'b1;` |
| 4 | 1 | 3 | 1 | undeclared function '…' | `opentitan/hw/ip/prim/rtl/prim_cipher_pkg.sv:347` `state_out[k*8 +: 8] = sbox4_8bit(state_in[k*8 +: 8], sbox4);` |
| 3 | 0 | 1 | 1 | non-synthesizable type '…' | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv:457` `function automatic int unsigned path_hash(string s);` |
| 3 | 0 | 1 | 1 | unexpected `"…"`; expected an item | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 3 | 0 | 1 | 1 | unexpected `endmodule` | `picorv32/picosoc/picosoc.v:220` `endmodule` |
| 3 | 0 | 1 | 1 | unknown macro or directive | `picorv32/picosoc/picosoc.v:22` `` `error "picosoc.v must be read before picorv32.v!" `` |
| 3 | 0 | 1 | 1 | use of undeclared identifier '…' | `picorv32/picosoc/picosoc.v:93` `irq[5] = irq_5;` |
| 2 | 2 | 2 | 2 | continuous assignment LHS must have constant bit/element bounds | `ibex/rtl/ibex_pmp.sv:178` `assign region_addr_mask[r][b] = (csr_pmp_cfg_i[r].mode != PMP_MODE_NAPOT) \|` |
| 2 | 2 | 1 | 1 | no member named '…' in 'struct{logic pp_en;logic od_en;logic pu_en;}i3c_io_pkg::s$N' | `opentitan/hw/ip/i3c/rtl/i3c_controller_trx.sv:1081` `sda_drv_mod.en = sda_drv_d.en & ~(sda_drv_d.od_en ? sda_d : 'b0);` |
| 2 | 0 | 2 | 2 | untyped assignment pattern requires an explicit type prefix or assignment context | `ibex/rtl/ibex_cheriot_ex.sv:258` `'{CTAG: lc_ctag, SD_LM: lc_csdlm, GL_LG: lc_cglg};` |
| 1 | 1 | 1 | 1 | overlapping drivers on '…': driven by clocked procedural block and always_comb block | `verilog-axi/rtl/axi_vfifo_dec.v:641` `input_valid_int_next <= 1'b0;` |
| 1 | 1 | 1 | 1 | overlapping drivers on '…': driven by clocked procedural block and clocked procedural block | `biriscv/src/dcache/dcache_core_data_ram.v:77` `ram[addr1_i][7:0] <= data1_i[7:0];` |
| 1 | 1 | 1 | 1 | signal '…' has non-synthesizable type '…' | `ibex/rtl/ibex_tracer.sv:95` `string file_name;` |
| 1 | 0 | 1 | 1 | cannot resolve hierarchical or member reference '…' | `opentitan/hw/ip/rram_ctrl/rtl/rram_ctrl_mp.sv:208` `if (info_page_addr == HwLcMgrInfoPageCfg[i].page &&` |
| 1 | 0 | 1 | 1 | packed dimension requires constant bounds | `opentitan/hw/ip/tlul/rtl/tlul_pkg.sv:196` `function automatic logic [DataIntgWidth-1:0] get_data_intg(logic [top_pkg::TL_DW-1:0] data);` |
| 1 | 0 | 1 | 1 | part-select width exceeds maximum bit width | `cva6/core/multiplier.sv:126` `MULH, MULHU, MULHSU: result_o = mult_result_q[CVA6Cfg.XLEN*2-1:CVA6Cfg.XLEN];` |
| 1 | 0 | 1 | 1 | signal '…' has non-synthesizable type 'unpacked array exceeds maximum element limit' | `picorv32/picosoc/spiflash.v:102` `reg [7:0] memory [0:16*1024*1024-1];` |
| 1 | 0 | 1 | 1 | signal '…' has non-synthesizable type 'unpacked array with non-constant bounds' | `opentitan/hw/ip/prim_generic/rtl/prim_ram_1r1w.sv:65` `logic [Width-1:0] mem [Depth];` |
| 1 | 0 | 1 | 1 | undeclared identifier 'DataIntgWidth' | `opentitan/hw/ip/tlul/rtl/tlul_pkg.sv:196` `function automatic logic [DataIntgWidth-1:0] get_data_intg(logic [top_pkg::TL_DW-1:0] data);` |
| 1 | 0 | 1 | 1 | undeclared identifier 'HwLcMgrDataCfg' | `opentitan/hw/ip/rram_ctrl/rtl/rram_ctrl_mp.sv:155` `.region_cfg_i(HwLcMgrDataCfg[HwLcMgrDataRules-1:0]),` |
| 1 | 0 | 1 | 1 | undeclared identifier 'HwOtpDataCfg' | `opentitan/hw/ip/rram_ctrl/rtl/rram_ctrl_mp.sv:145` `.region_cfg_i(HwOtpDataCfg[HwOtpDataRules-1:0]),` |

## Crashes and limits

A top that panics, or needs more than 120 s or 4.0 GB.

| tops | what | example |
| ---: | --- | --- |
| 18 | timeout | `caliptra-rtl/src/riscv_core/veer_el2/rtl/lib/beh_lib.sv` `rvecc_encode_64` (1 files) |
| 5 | memout | `cv32e40p/rtl/vendor/pulp_platform_common_cells/src/deprecated/find_first_one.sv` `find_first_one` (1 files) |

## Lowered although the reference rejects the input

svlang produced a netlist from input the reference front end calls an error. Either svlang misses a
check it should make, or the reference is stricter than the simulators these designs target.

| tops | reference error | example |
| ---: | --- | --- |
| 74 | unknown module '…' | `picorv32/picosoc/ice40up5k_spram.v:39` (top `ice40up5k_spram`) |
| 3 | unknown macro or compiler directive | `opentitan/hw/ip/spi_host/rtl/spi_host_fsm.sv:620` (top `spi_host_core`) |
| 2 | use of undeclared identifier '…' | `basejump_stl/bsg_misc/bsg_mul/bsg_mul_booth_4_block.sv:36` (top `bsg_mul_booth_4_block`) |
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

- 4 of the 557 are instantiated inside a top that lowered
- 8 hold a construct that blocks a top above them
- 545 are not reached by any measured top

| tops | reference error | example |
| ---: | --- | --- |
| 277 | parameter '…' has no constant value | `basejump_stl` `bsg_imod_range` |
| 88 | unknown module '…' | `common_cells` `cc_cdc_2phase` |
| 48 | top-level module '…' has unconnected interface port '…' | `axi` `axi_dumper_intf` |
| 43 | invalid member access for type '…' (aka '…') | `axi` `axi_chan_compare` |
| 20 | unknown macro or compiler directive | `biriscv` `biriscv_fetch` |
| 15 | value must not have any unknown bits | `serv` `serv_rf_ram` |
| 11 | value must be positive | `cva6` `ariane_regfile_fpga` |
| 10 | no top-level module found to lower | `biriscv` `RAM16X1D` |
| 6 | '…': No such file or directory | `common_cells` `cc_addr_decode_dync` |
| 5 | unknown class or package '…' | `axi` `axi_atop_filter_intf` |
| 4 | duplicate definition of '…' [-Wduplicate-definition] | `cva6` `cva6_ptw` |
| 4 | replication constant can only be zero inside of a concatenation | `cva6` `wt_dcache_mem` |
| 3 | invalid target type '…' (aka '…') for assignment pattern | `axi` `axi_lite_to_axi` |
| 3 | unknown package '…' | `cva6` `cva6_hpdcache_subsystem_l15_adapter` |
| 2 | expected identifier | `picorv32` `testbench` |

## Method and limits

- Each module is compiled as a top with its default parameters, from the file that defines it and the
  files that define the names it uses, found by a text scan. No design's own build flow is run, so a
  configuration a flow would set (a define, a parameter, a generated file) is absent.
- Lowering stops at its first failing stage, so a top's diagnostics are a lower bound: fixing a row can
  uncover the next one.
- A lowered netlist is not checked for correctness here; this census measures coverage, not equivalence.
- Testbench, verification and tool directories are left out by name.
