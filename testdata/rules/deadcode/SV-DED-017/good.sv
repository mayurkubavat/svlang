`timescale 1ns/1ps
`define USED_LOCAL_MASK 8'h0F
module unref_macro_tb (input logic [7:0] a, output logic [7:0] y);
    assign y = a & `USED_LOCAL_MASK;
endmodule : unref_macro_tb
