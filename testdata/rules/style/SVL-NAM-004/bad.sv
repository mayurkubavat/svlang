`timescale 1ns/1ps
`define badMacroName 8'h5A
module macro_naming_tb (output logic [7:0] v);
    assign v = `badMacroName;
endmodule : macro_naming_tb
