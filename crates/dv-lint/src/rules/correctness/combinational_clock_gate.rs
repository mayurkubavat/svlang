//! SV-RTL-003 (CombinationalClockOrResetGate): `always_ff` clocked by a locally
//! driven combinational net instead of a primary clock or clock-gating cell.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn combinational_clock_gate(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).comb_clock_gate {
        out.add(
            "SV-RTL-003",
            "CombinationalClockOrResetGate",
            Severity::Warning,
            format!(
                "Clock signal '{}' of always_ff in module '{}' is driven by local combinational logic. \
Combinational clock gating creates glitches and hold-time violations; instantiate an integrated clock-gating (ICG) cell instead.",
                hit.clk_name, hit.module_name
            ),
            hit.loc,
        );
    }
}
