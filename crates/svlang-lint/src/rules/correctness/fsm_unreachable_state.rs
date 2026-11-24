//! RTL-FSM-002 (FsmUnreachableState): FSM enum state neither entered on reset
//! nor reachable via any state transition.

use crate::support::rtl_fsm_cdc;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn fsm_unreachable_state(c: &Cst, out: &mut Sink) {
    for hit in &rtl_fsm_cdc::facts(c).fsm_unreachable {
        out.add(
            "RTL-FSM-002",
            "FsmUnreachableState",
            Severity::Warning,
            format!(
                "FSM state '{}' of state register '{}' in module '{}' is never entered on reset or targeted by any state transition. \
Remove dead states or connect the missing state transition.",
                hit.state_name, hit.state_var, hit.module_name
            ),
            hit.loc,
        );
    }
}
