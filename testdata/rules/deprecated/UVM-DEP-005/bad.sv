`include "uvm_mock.svh"
class smoke_seq extends uvm_sequence;
  `uvm_object_utils(smoke_seq)
  function new(string name = "smoke_seq"); super.new(name); endfunction
  virtual task body();
    if (starting_phase != null) begin
      starting_phase.raise_objection(this, "run");
      get_starting_phase().drop_objection(this, "run");
    end
  endtask
endclass
