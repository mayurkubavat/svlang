// Enum methods: a loop over the members with first/next/num, and next/prev of a signal.
module enum_methods (
    input  logic       clk,
    input  logic [1:0] d,
    output logic       undefined,
    output logic [1:0] nxt, prv
);
    typedef enum logic [1:0] { A = 2'd0, B = 2'd1, C = 2'd3 } e_t;
    e_t q;
    function automatic logic is_undefined(e_t sig);
        logic is_defined = 1'b0;
        for (int i = 0, e_t t = t.first(); i < t.num(); i += 1, t = t.next()) begin
            is_defined |= (sig == t);
        end
        return ~is_defined;
    endfunction
    always_ff @(posedge clk) q <= e_t'(d);
    assign undefined = is_undefined(q);
    assign nxt       = q.next();
    assign prv       = q.prev();
endmodule
