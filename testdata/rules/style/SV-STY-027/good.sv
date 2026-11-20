`timescale 1ns/1ps
`define DATA_WIDTH_VAL 8'd16
`undef DATA_WIDTH_VAL
`define DATA_WIDTH_VAL 8'd32
module dup_macro_tb (output logic [7:0] w);
    assign w = `DATA_WIDTH_VAL;
endmodule : dup_macro_tb
