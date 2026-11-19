//! SV-RND-043 (DanglingDisableSoft): `disable soft x;` when neither the class
//! nor its base classes declare a `soft` constraint on `x`.

use crate::support::constraint_ir;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn dangling_disable_soft(c: &Cst, out: &mut Sink) {
    for hit in &constraint_ir::facts(c).dangling_disable_soft {
        out.add(
            "SV-RND-043",
            "DanglingDisableSoft",
            Severity::Warning,
            format!(
                "'disable soft {}' in class '{}' targets a variable with no 'soft' constraint in '{}' or its base classes. \
Verify the variable name or remove the redundant 'disable soft' directive.",
                hit.var_name, hit.class_name, hit.class_name
            ),
            hit.loc,
        );
    }
}
