//! SVL-COR-016 (UninitializedLocalRead): Automatic local scalar/handle variable
//! read before any definite write on all reaching paths.

use crate::support::cfg_dataflow;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn uninitialized_local_read(c: &Cst, out: &mut Sink) {
    for hit in &cfg_dataflow::facts(c).uninit_reads {
        out.add(
            "SVL-COR-016",
            "UninitializedLocalRead",
            Severity::Warning,
            format!(
                "Automatic local variable '{}' in '{}' is read before being initialized or assigned.",
                hit.var_name, hit.routine_name
            ),
            hit.loc,
        );
    }
}
