//! SV-FPV-005 (ConstantOrContradictoryAssume): Constant-false, tautological, or
//! pairwise-contradictory `assume` statements in formal verification.

use crate::support::formal_ready;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn constant_or_contradictory_assume(c: &Cst, out: &mut Sink) {
    for hit in &formal_ready::facts(c).bad_assumes {
        out.add(
            "SV-FPV-005",
            "ConstantOrContradictoryAssume",
            Severity::Error,
            format!("Formal assumption {}.", hit.reason),
            hit.loc,
        );
    }
}
