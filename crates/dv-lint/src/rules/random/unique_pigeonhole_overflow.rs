//! SV-RND-044 (UniquePigeonholeOverflow): `unique { arr }` on a fixed-size
//! unpacked array whose element count exceeds the element type's cardinality.

use crate::support::constraint_ir;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unique_pigeonhole_overflow(c: &Cst, out: &mut Sink) {
    for hit in &constraint_ir::facts(c).pigeonhole_unique {
        out.add(
            "SV-RND-044",
            "UniquePigeonholeOverflow",
            Severity::Error,
            format!(
                "'unique {{ {} }}' in class '{}' requires {} distinct values, but the element type only has {} possible values. \
Widen the element type or shrink the array dimension so randomize() can succeed.",
                hit.array_name, hit.class_name, hit.elem_count, hit.domain_size
            ),
            hit.loc,
        );
    }
}
