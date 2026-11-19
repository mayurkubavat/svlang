//! SV-RTL-004 (PortWidthMismatch): Elaborated module/interface instance port
//! connection width differs from the formal port width.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn port_width_mismatch(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).port_width_mismatches {
        out.add(
            "SV-RTL-004",
            "PortWidthMismatch",
            Severity::Warning,
            format!(
                "Port '{}' on instance '{}' ({}) expects {} bit(s), but connected expression is {} bit(s). \
Implicit port width coercion silently truncates upper bits or zero-extends.",
                hit.port_name, hit.inst_name, hit.def_name, hit.formal_width, hit.actual_width
            ),
            hit.loc,
        );
    }
}
