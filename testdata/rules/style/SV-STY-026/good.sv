`timescale 1ns/1ps
`define GOOD_MACRO_NAME 8'h5A
module macro_naming_tb (output logic [7:0] v);
    assign v = `GOOD_MACRO_NAME;
endmodule : macro_naming_tb
