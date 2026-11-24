`timescale 1ns/1ps
module struct_pat_order_tb (output logic [15:0] out_v);
    typedef struct packed {
        logic [7:0] addr;
        logic [7:0] data;
    } req_t;
    req_t r = '{data: 8'hAA, addr: 8'h10};
    assign out_v = {r.addr, r.data};
endmodule : struct_pat_order_tb
