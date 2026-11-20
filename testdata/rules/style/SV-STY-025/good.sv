`timescale 1ns/1ps
module aligned_fifo (
    input  logic       clk,
    input  logic       rst_n,
    input  logic [7:0] din,
    output logic [7:0] dout,
    output logic       valid
);
    assign dout = din;
    assign valid = clk & rst_n;
endmodule : aligned_fifo
