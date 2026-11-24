//! SVL-RND-031 (RelationalConstraintCycle): Unsatisfiable strict relational cycle
//! across constraint variables (`a < b && b < a`, `a < b && b <= c && c <= a`).

use crate::support::constraint_ir;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn relational_constraint_cycle(c: &Cst, out: &mut Sink) {
    for hit in &constraint_ir::facts(c).relational_cycles {
        out.add(
            "SVL-RND-031",
            "RelationalConstraintCycle",
            Severity::Error,
            format!(
                "Constraint '{}' in class '{}' forms an unsatisfiable strict relational cycle involving '{}' and '{}'. \
Remove or relax one of the conflicting ordering constraints so randomize() can find a solution.",
                hit.constraint_name, hit.class_name, hit.lhs, hit.rhs
            ),
            hit.loc,
        );
    }
}
