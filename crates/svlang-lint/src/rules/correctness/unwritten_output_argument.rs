//! SVL-COR-017 (UnwrittenOutputArgument): Concrete subroutine declares an
//! `output` argument that is never written in the subroutine body.

use crate::support::cfg_dataflow;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unwritten_output_argument(c: &Cst, out: &mut Sink) {
    for hit in &cfg_dataflow::facts(c).unwritten_outputs {
        out.add(
            "SVL-COR-017",
            "UnwrittenOutputArgument",
            Severity::Warning,
            format!(
                "Output argument '{}' of '{}' is never assigned in the body; callers receive the default initial value.",
                hit.arg_name, hit.routine_name
            ),
            hit.loc,
        );
    }
}
