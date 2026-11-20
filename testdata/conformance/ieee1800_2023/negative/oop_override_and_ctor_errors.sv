// IEEE 1800-2023 negative conformance: OOP `:final`/`:extends`/`:initial`,
// constructor `default`, inheritance cycle, and derived covergroup errors.

class :final SealedCls;
endclass

class BadExtendFinal extends SealedCls;
endclass

class CycA extends CycB;
endclass

class CycB extends CycA;
endclass

virtual class OvBase;
    function void non_virt(); endfunction
    virtual function void virt_m(); endfunction
    function :final void locked_m(); endfunction
    constraint :final c_locked {}
    constraint c_existing {}
endclass

class OvDerived extends OvBase;
    function :extends void non_virt(); endfunction
    function :initial void virt_m(); endfunction
    function void locked_m(); endfunction
    constraint :extends c_locked {}
    constraint :initial c_existing {}
    constraint :extends c_missing {}
    extern constraint :initial c_ext_mismatch;
    covergroup extends no_such_cg;
    endgroup
endclass

constraint OvDerived::c_ext_mismatch {}

class NoBaseDefaultCtor;
    function new(default);
    endfunction
endclass
