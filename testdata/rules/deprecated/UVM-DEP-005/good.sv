`include "uvm_mock.svh"
class smoke_seq extends uvm_sequence;
  `uvm_object_utils(smoke_seq)
  function new(string name = "smoke_seq"); super.new(name); endfunction
  virtual task body();
    uvm_phase ph = get_starting_phase();
    if (ph != null) begin
      ph.raise_objection(this, "run");
      ph.drop_objection(this, "run");
    end
  endtask
endclass
