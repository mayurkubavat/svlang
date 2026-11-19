//! SV-RTL-001 (MixedResetPolarity): Same reset signal used with both `posedge`
//! and `negedge` sensitivity in the same module.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn mixed_reset_polarity(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).mixed_reset_polarity {
        out.add(
            "SV-RTL-001",
            "MixedResetPolarity",
            Severity::Warning,
            format!(
                "Reset signal '{}' in module '{}' is sensitive to both posedge and negedge across sequential blocks. \
Mixed reset polarities leave part of the module in reset while the rest runs.",
                hit.reset_name, hit.module_name
            ),
            hit.loc,
        );
    }
}
