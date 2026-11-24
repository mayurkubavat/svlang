//! RTL-FSM-003 (FsmDeadlockState): Non-reset, non-terminal FSM state with no
//! outgoing transition to any other state.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn fsm_deadlock_state(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).fsm_deadlock {
        out.add(
            "RTL-FSM-003",
            "FsmDeadlockState",
            Severity::Warning,
            format!(
                "FSM state '{}' of state register '{}' in module '{}' has no outgoing transition to any other state. \
Once entered, the state machine is permanently trapped until reset.",
                hit.state_name, hit.state_var, hit.module_name
            ),
            hit.loc,
        );
    }
}
