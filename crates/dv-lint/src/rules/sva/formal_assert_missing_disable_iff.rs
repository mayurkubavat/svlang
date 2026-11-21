//! SV-FPV-003 (FormalAssertMissingDisableIff): Concurrent `assert`/`cover`
//! property missing `disable iff` in a module that declares a reset.

use crate::support::formal_ready;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn formal_assert_missing_disable_iff(c: &Cst, out: &mut Sink) {
    for hit in &formal_ready::facts(c).missing_disable_iff {
        out.add(
            "SV-FPV-003",
            "FormalAssertMissingDisableIff",
            Severity::Info,
            format!(
                "Concurrent property '{}' has no 'disable iff' (and no 'default disable iff') in a module with reset logic; warm resets will trigger false formal counterexamples.",
                hit.prop_name
            ),
            hit.loc,
        )
        .fixes
        .extend(hit.fix.iter().cloned());
    }
}
