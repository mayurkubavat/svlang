//! SV-FPV-002 (ResetlessFlopFeedsControl): Reset-less flip-flop driving a
//! 1-bit control/handshake output, memory write-enable, or mux selector.

use crate::support::formal_ready;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn resetless_flop_feeds_control(c: &Cst, out: &mut Sink) {
    for hit in &formal_ready::facts(c).resetless_control {
        out.add(
            "SV-FPV-002",
            "ResetlessFlopFeedsControl",
            Severity::Warning,
            format!(
                "Reset-less sequential register '{}' feeds {} without reset qualification, creating an X-propagation hazard in post-reset formal and gate-level sign-off.",
                hit.state_name, hit.sink_desc
            ),
            hit.loc,
        );
    }
}
