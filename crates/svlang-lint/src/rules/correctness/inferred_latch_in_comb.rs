//! RTL-SYN-005 (InferredLatchInComb): Bit-accurate symbolic path analysis detects
//! incomplete combinational assignment in `always_comb` (`svlang::netlist`).

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn inferred_latch_in_comb(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).inferred_latches {
        out.add(
            "RTL-SYN-005",
            "InferredLatchInComb",
            Severity::Warning,
            format!(
                "Combinational completeness violation: {}. Assign a default value on all control paths or use always_latch if level-sensitive storage is intended.",
                hit.message
            ),
            hit.loc,
        );
    }
}
