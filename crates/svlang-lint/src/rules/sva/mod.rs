//! The sva rules: the Rust twins of `cpp/src/rules/sva/`, one file per rule,
//! tested by `testdata/rules/<area>/<ID>/`.

use crate::profile::*;
use crate::{Check, Rule, Severity};

mod assert_without_else;
mod assume_on_output_or_state;
mod concurrent_assert_in_function;
mod constant_or_contradictory_assume;
mod disable_iff_complex_expression;
mod disable_iff_reset;
mod formal_assert_missing_disable_iff;
mod immediate_assert_in_combinatorial_logic;
mod implication_in_cover_property;
mod mixed_clock_edges_in_property;
mod multi_clock_concurrent_assertion_domain;
mod non_consecutive_repetition_zero_bound;
mod resetless_flop_feeds_control;
mod side_effect_in_assertion_action;
mod sva_unbounded_delay;
mod sva_vacuous_antecedent;
mod time_in_assertion_action;
mod unboxed_wide_arith_or_memory;
mod unlabeled_assertion;

/// Every rule of this category.
pub(crate) fn rules() -> Vec<Rule> {
    vec![
        Rule { id: "SVA-PRP-001", name: "ImmediateAssertInCombinatorialLogic", category: "SVA", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Simple immediate assertions in always_comb or combinatorial blocks evaluate transient delta glitches. Convert to deferred immediate assertions (assert #0 or assert final) to wait for settled circuit values.", check: Check::Syntax(immediate_assert_in_combinatorial_logic::immediate_in_comb),
        },
        Rule { id: "SVA-PRP-002", name: "DisableIffComplexExpression", category: "SVA", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Flags 'disable iff (...)' clauses containing complex boolean expressions or multi-variable logic instead of simple reset conditions.", check: Check::Syntax(disable_iff_complex_expression::disable_iff_complex),
        },
        Rule { id: "SVA-PRP-003", name: "MixedClockEdgesInProperty", category: "SVA", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Flags SVA properties or assertions that mix both 'posedge' and 'negedge' clocking events.", check: Check::Syntax(mixed_clock_edges_in_property::mixed_edges),
        },
        Rule { id: "SVA-PRP-004", name: "NonConsecutiveRepetitionZeroBound", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Non-consecutive repetition operators ([= ...] and [-> ...]) must specify a minimum bound >= 1.", check: Check::Syntax(non_consecutive_repetition_zero_bound::zero_repetition),
        },
        Rule { id: "SVA-PRP-005", name: "MultiClockConcurrentAssertionDomain", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Disallows concurrent assertions mixing multiple asynchronous clock domains without formal CDC qualification.", check: Check::Syntax(multi_clock_concurrent_assertion_domain::multi_clock),
        },
        Rule { id: "SVA-PRP-008", name: "AssertWithoutElse", category: "SVA", severity: Severity::Warning, profiles: STYLE, guidelines: &["MG-8.1"], experimental: false, native: false, about: "Immediate and concurrent assertions must include an 'else' action block (e.g. 'else `uvm_error(...)'). Assertions without action blocks produce unhelpful one-line messages and cannot integrate with UVM reporting.", check: Check::Syntax(assert_without_else::assert_without_else),
        },
        Rule { id: "SVA-PRP-009", name: "TimeInAssertionAction", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Action blocks of immediate and concurrent assertions must not consume simulation time (# delays, @ event controls, or wait statements) per IEEE 1800 § 16.", check: Check::Syntax(time_in_assertion_action::time_in_action),
        },
        Rule { id: "SVA-PRP-010", name: "SideEffectInAssertionAction", category: "SVA", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Flags assignments, increments and other writes inside the pass or fail action of an immediate or concurrent assertion. Reporting calls are fine; state changes vanish when assertions are disabled.", check: Check::Syntax(side_effect_in_assertion_action::side_effect_in_action),
        },
        Rule { id: "SVA-PRP-011", name: "ConcurrentAssertInFunction", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Disallow concurrent assertions ('assert/assume/cover property') inside functions. Functions execute in zero simulation time and lack clocking contexts required by SVA semantics.", check: Check::Syntax(concurrent_assert_in_function::concurrent_in_function),
        },
        Rule { id: "SVA-PRP-012", name: "SvaUnboundedDelay", category: "SVA", severity: Severity::Warning, profiles: ADVISORY, guidelines: &[], experimental: false, native: false, about: "Flags unbounded cycle delays (##[1:$]) in SVA sequences where bounded delays improve simulation memory use and formal convergence.", check: Check::Syntax(sva_unbounded_delay::unbounded_delay),
        },
        Rule { id: "SVA-PRP-013", name: "UnlabeledAssertion", category: "SVA", severity: Severity::Warning, profiles: STYLE, guidelines: &[], experimental: false, native: false, about: "Immediate and concurrent assertions must declare an explicit label prefix (e.g. 'check_lbl: assert ...'). Lacking labels makes regression failure triage, ticket routing, and assertion control commands difficult.", check: Check::Syntax(unlabeled_assertion::unlabeled_assertion),
        },
        Rule { id: "SVA-PRP-014", name: "DisableIffReset", category: "SVA", severity: Severity::Warning, profiles: ADVISORY, guidelines: &[], experimental: false, native: false, about: "Concurrent assertions and property declarations should include a 'disable iff (...)' reset guard (unless covered by default disable iff or the paired property/assertion).", check: Check::Syntax(disable_iff_reset::disable_iff_reset) },
        Rule { id: "SVA-PRP-015", name: "SvaVacuousAntecedent", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: false, about: "Detects constant-false antecedents in assertion implications (0 |-> ...). False antecedents cause vacuous passes, giving a false 100% pass metric while never verifying DUT behavior.", check: Check::Syntax(sva_vacuous_antecedent::vacuous_antecedent),
        },
        Rule { id: "SVA-PRP-007", name: "ImplicationInCoverProperty", category: "SVA", severity: Severity::Warning, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: true, about: "Flags cover property statements that use implication ('|->' or '|=>'), which succeed vacuously whenever the antecedent is false instead of covering sequence completion.", check: Check::Syntax(implication_in_cover_property::implication_in_cover_property) },
        Rule { id: "SVA-FPV-001", name: "AssumeOnOutputOrState", category: "SVA", severity: Severity::Warning, profiles: ADVISORY, guidelines: &[], experimental: false, native: true, about: "Flags formal 'assume' statements that constrain module output ports or internal sequential state rather than primary inputs, which silently masks reachable DUT bugs.", check: Check::Syntax(assume_on_output_or_state::assume_on_output_or_state) },
        Rule { id: "SVA-FPV-002", name: "ResetlessFlopFeedsControl", category: "SVA", severity: Severity::Warning, profiles: ADVISORY, guidelines: &[], experimental: false, native: true, about: "Flags reset-less sequential flip-flops whose outputs feed 1-bit control/handshake ports, memory write-enables, or mux selectors in modules with reset logic.", check: Check::Syntax(resetless_flop_feeds_control::resetless_flop_feeds_control) },
        Rule { id: "SVA-FPV-003", name: "FormalAssertMissingDisableIff", category: "SVA", severity: Severity::Info, profiles: ADVISORY, guidelines: &[], experimental: false, native: true, about: "Flags concurrent 'assert' and 'cover' properties without 'disable iff' (and no 'default disable iff') inside elaborated modules that use reset. Advisory: selected by '--guideline formal-ready' or '--preset formal-ready'.", check: Check::Syntax(formal_assert_missing_disable_iff::formal_assert_missing_disable_iff) },
        Rule { id: "SVA-FPV-004", name: "UnboxedWideArithOrMemory", category: "SVA", severity: Severity::Info, profiles: ADVISORY, guidelines: &[], experimental: false, native: true, about: "Flags wide variable-by-variable multipliers/dividers (>= min_arith_width, default 16 bits) and large memories (>= min_mem_words, default 256 words) in formal-checked modules that should be black-boxed ('-bbox_mul' / '-bbox_a'). Advisory: selected by '--guideline formal-ready' or '--preset formal-ready'.", check: Check::Syntax(unboxed_wide_arith_or_memory::unboxed_wide_arith_or_memory) },
        Rule { id: "SVA-FPV-005", name: "ConstantOrContradictoryAssume", category: "SVA", severity: Severity::Error, profiles: SOUNDNESS, guidelines: &[], experimental: false, native: true, about: "Flags constant-false, tautological constant-true, or pairwise-contradictory formal 'assume' statements that render the formal environment vacuous or redundant.", check: Check::Syntax(constant_or_contradictory_assume::constant_or_contradictory_assume) },
    ]
}
