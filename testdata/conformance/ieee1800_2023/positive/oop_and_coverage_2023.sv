// IEEE 1800-2023 positive conformance: OOP override specifiers, constructor
// `default` forwarding, `type(this)` in static context, `weak_reference`,
// derived `covergroup extends`, and `solve ... before` with reals & `.size()`.

virtual class Base2023;
    int m_tag;
    enum { RED, GREEN, BLUE } color;

    function new(int tag = 7);
        m_tag = tag;
    endfunction

    function void non_virt();
    endfunction

    virtual function void v_ext();
    endfunction

    pure virtual function int v_pure();

    constraint c_base { m_tag >= 0; }

    covergroup cg_base (bit [3:0] x) with function sample(bit en);
        option.per_instance = 1;
        cp_x: coverpoint x;
        cp_en: coverpoint en;
        cp_col: coverpoint color;
    endgroup
endclass

class Mid2023 extends Base2023(default);
    rand real r_val;
    rand int  dyn_q[];

    // `:initial` is legal when Base2023::non_virt is non-virtual.
    function :initial void non_virt();
    endfunction

    virtual function :extends :final void v_ext();
    endfunction

    virtual function :extends int v_pure();
        return m_tag;
    endfunction

    extern constraint :initial c_fresh;
    constraint :extends c_base { m_tag < 100; }

    constraint c_order {
        dyn_q.size() == 2;
        solve r_val before dyn_q.size(), dyn_q.size;
    }

    covergroup extends cg_base;
        option.weight = 2;
        cp_col: coverpoint color {
            ignore_bins ig = { BLUE };
        }
    endgroup : cg_base
endclass

constraint :initial Mid2023::c_fresh { dyn_q.size() > 0; }

class :final Leaf2023 extends Mid2023;
    function new(default);
        super.new(default);
    endfunction

    static function type(this) singleton();
        var static type(this) inst;
        if (inst == null) inst = new();
        return inst;
    endfunction
endclass

module oop_and_coverage_2023_top;
    initial begin
        Leaf2023 leaf = Leaf2023::singleton();
        weak_reference #(Leaf2023) wr = new(leaf);
        longint id = wr.get_id();
        if (wr.get() != null && id >= 0) wr.clear();
    end
endmodule
