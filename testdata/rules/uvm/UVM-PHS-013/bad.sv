`include "uvm_mock.svh"
interface bus_if; logic rst_n; endinterface
class early_vif_driver extends uvm_driver #(uvm_sequence_item);
    `uvm_component_utils(early_vif_driver)
    virtual bus_if m_vif;
    function new(string name, uvm_component parent);
        super.new(name, parent);
    endfunction : new
    function void build_phase(uvm_phase phase);
        super.build_phase(phase);
        if (!uvm_config_db#(virtual bus_if)::get(this, "", "vif", m_vif))
            `uvm_fatal("NOVIF", "missing vif")
        m_vif.rst_n = 1'b0;
    endfunction : build_phase
endclass : early_vif_driver
