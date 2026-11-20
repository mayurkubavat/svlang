#pragma once

#include <string_view>
#include <vector>
#include "svlang_lint/core/Diagnostic.h"
#include "svlang_lint/rules/Guidelines.h"
#include "svlang_lint/rules/Rule.h"

namespace svlang_lint::reporting {

/// How a run measured up against one methodology guideline.
struct GuidelineResult {
    const rules::Guideline* guideline = nullptr;
    std::vector<std::string_view> rules; // rules of this guideline that ran
    size_t findings = 0;                 // findings those rules reported (after waivers)
    bool passed() const { return findings == 0; }
};

/// One result per guideline that had at least one of its rules active, in guide order.
std::vector<GuidelineResult> computeConformance(const std::vector<rules::Rule*>& activeRules,
                                                const std::vector<core::Violation>& violations);

/// Console table of the results, one line per guideline plus a per-chapter tally.
void printConformance(std::ostream& os, const std::vector<GuidelineResult>& results);

} // namespace svlang_lint::reporting
