// Compliant Clean Test Case: SV-OOP-007 (StaticMemberInUvmComponent)
`include "uvm_mock.svh"

// Compliant placeholder - verified to produce 0 violations
class compliant_staticmemberinuvmcomponent extends uvm_object;
    function new(string name = "compliant");
        super.new(name);
    endfunction
endclass

// Singleton service with restricted constructor (`protected function new`)
class singleton_root_service extends uvm_component;
    static bit m_initialized = 0;
    protected function new(string name = "singleton_root_service", uvm_component parent = null);
        super.new(name, parent);
    endfunction : new
endclass : singleton_root_service
