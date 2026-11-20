//! SV-FPV-004 (UnboxedWideArithOrMemory): Wide variable-by-variable arithmetic
//! (`*`, `/`, `%`) or large memory array inside a formal verification cone.

use crate::support::formal_ready;
use crate::syntax::Cst;
use crate::{Severity, Sink};

pub(crate) fn unboxed_wide_arith_or_memory(c: &Cst, out: &mut Sink) {
    let min_arith_w = crate::options::get("SV-FPV-004", "min_arith_width", 16).max(1) as u32;
    let min_mem_words = crate::options::get("SV-FPV-004", "min_mem_words", 256).max(1) as u32;
    for hit in &formal_ready::facts(c).wide_complexity {
        let triggers = if hit.is_memory { hit.metric >= min_mem_words } else { hit.metric >= min_arith_w };
        if !triggers {
            continue;
        }
        out.add(
            "SV-FPV-004",
            "UnboxedWideArithOrMemory",
            Severity::Info,
            format!(
                "Unabstracted {} inside a formal-checked module risks SAT/IC3 state-space blow-up; consider '-bbox_mul', '-bbox_a', or black-boxing the datapath macro.",
                hit.kind_desc
            ),
            hit.loc,
        );
    }
}
