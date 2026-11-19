//! SV-RTL-002 (PartialResetInAlwaysFf): Asynchronous-reset `always_ff` block
//! resets only a subset of the registers it drives.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn partial_reset_in_always_ff(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).partial_ff_reset {
        out.add(
            "SV-RTL-002",
            "PartialResetInAlwaysFf",
            Severity::Warning,
            format!(
                "Asynchronous-reset always_ff block in module '{}' omits reset assignments for: {}. \
Move intentionally unreset datapath flops into a separate clock-only always_ff block so synthesis does not infer async-reset clock-enable logic.",
                hit.module_name,
                hit.unreset_regs.join(", ")
            ),
            hit.loc,
        );
    }
}
