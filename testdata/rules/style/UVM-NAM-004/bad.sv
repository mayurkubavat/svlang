// Test Case: UVM-NAM-004 (UvmCreateStringMismatch)
// Extracted from sample_tb.sv (Test Case 39)

`include "uvm_mock.svh"

// --- TEST CASE 39: UVM-NAM-004 (UVM Factory Create String Mismatch) ---
class StringMismatchComponent extends uvm_component;
    `uvm_component_utils(StringMismatchComponent)
    MyDriver m_tx_driver;

    function new(string name, uvm_component parent);
        super.new(name, parent);
    endfunction

    function void build_phase(uvm_phase phase);
        super.build_phase(phase);
        // VIOLATION UVM-NAM-004: String literal "wrong_driver_name" does not match variable m_tx_driver!
        m_tx_driver = MyDriver::type_id::create("wrong_driver_name", this);
    endfunction
endclass
