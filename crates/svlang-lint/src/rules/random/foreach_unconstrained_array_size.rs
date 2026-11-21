//! SV-RND-042 (ForeachUnconstrainedArraySize): `foreach (arr[i])` in a constraint
//! block over a `rand` dynamic array or queue whose `.size()` is never constrained.

use crate::support::constraint_ir;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn foreach_unconstrained_array_size(c: &Cst, out: &mut Sink) {
    for hit in &constraint_ir::facts(c).unconstrained_foreach {
        out.add(
            "SV-RND-042",
            "ForeachUnconstrainedArraySize",
            Severity::Warning,
            format!(
                "Constraint foreach over rand dynamic container '{}' in class '{}' has no corresponding '{}.size()' constraint or pre-allocation. \
Without a size constraint, randomize() leaves '{}' empty (size 0) and the foreach body never constrains any elements.",
                hit.array_name, hit.class_name, hit.array_name, hit.array_name
            ),
            hit.loc,
        );
    }
}
