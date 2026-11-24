//! SVA-FPV-001 (AssumeOnOutputOrState): `assume` or `assume property` constraining
//! a module `output` port or internal sequential state rather than primary inputs.

use crate::support::formal_ready;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn assume_on_output_or_state(c: &Cst, out: &mut Sink) {
    for hit in &formal_ready::facts(c).assume_output_or_state {
        out.add(
            "SVA-FPV-001",
            "AssumeOnOutputOrState",
            Severity::Warning,
            format!(
                "Formal assumption constrains {} instead of primary inputs. Over-constraining DUT outputs or internal state silently masks reachable bugs; use 'assert' for DUT invariants and 'assume' only on environment inputs.",
                hit.target_desc
            ),
            hit.loc,
        )
        .fixes
        .extend(hit.fix.iter().cloned());
    }
}
