//! Error recovery must end: text that is not SystemVerilog gives errors, never a parser
//! that loops. Each case here once did (found by tools/quality/mutate_smoke.py).

use sv_syntax::{parse_source, Options};

fn errors(src: &str) -> usize {
    parse_source(src, "t.sv", Options::default()).errors.len()
}

#[test]
fn a_struct_member_that_is_no_declaration() {
    assert!(errors("module m;\n  typedef struct { * / t [7:0] a; } s;\nendmodule\n") > 0);
    assert!(errors("typedef union packed { ) } u;") > 0);
}

#[test]
fn an_argument_list_cut_off_by_a_semicolon() {
    assert!(errors("module m;\n  initial begin\n    f(a, );\n    g(*;\n    h(1, ;\n  end\nendmodule\n") > 0);
    assert!(errors("module m; sub u (.a(x), ; endmodule") > 0);
}
