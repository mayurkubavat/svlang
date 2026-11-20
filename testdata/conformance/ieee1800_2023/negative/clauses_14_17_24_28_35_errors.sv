// IEEE 1800-2023 negative conformance: Clauses 14 (clocking), 17 (checker),
// 24 (program), 28 (UDNT gate connections), and 35 (DPI-C).

nettype real real_nt;

primitive my_buf(output o, input i);
    table
        0 : 0;
        1 : 1;
    endtable
endprimitive

checker chk_bad(input logic clk);
    real_nt illegal_udnt;
    int x;
endchecker

program prog_bad(input logic clk);
    logic q;
    always_comb q = clk;
endprogram

module clauses_neg_top(input logic clk);
    global clocking gcb1 @(posedge clk); input q_in; endclocking
    global clocking gcb2 @(posedge clk); endclocking

    logic q_in;
    initial gcb1.q_in <= 1'b1;

    chk_bad u_chk(clk);
    int leak1 = u_chk.x;
    logic leak2 = prog_bad.q;

    real_nt rn;
    wire wn;
    and g_bad(rn, wn);
    my_buf u_udp(rn, wn);
    tran sw_bad(rn, wn);

    import "DPI" function void bad_dpi_spec();
    import "DPI-C" pure function void bad_pure_void(output int o);
    import "DPI-C" function integer bad_ret_integer();
    int not_a_func;
    export "DPI-C" function not_a_func;
endmodule
