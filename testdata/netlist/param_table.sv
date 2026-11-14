// A parameter of an unpacked array type is a table of constants: selected by a
// parameter it folds to one value, selected by a signal it is a multiplexer.
module param_table #(
    parameter int unsigned W = 5
) (
    input  logic [1:0]  sel,
    output logic [7:0]  o,
    output logic [63:0] mask
);
    localparam logic [7:0] Table [0:3] = '{8'h11, 8'h22, 8'h33, 8'h44};
    localparam logic [63:0] Masks [4:6] = '{64'hC, 64'h14, 64'h30};
    assign o    = Table[sel];
    assign mask = Masks[W];
endmodule
