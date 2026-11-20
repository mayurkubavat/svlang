#pragma once

#include <string>
#include <vector>
#include "svlang_lint/core/Diagnostic.h"
#include "svlang_lint/rules/RuleRegistry.h"

namespace svlang_lint::reporting {

class SarifReporter {
public:
    static bool writeSarif(const std::string& outputPath,
                           const std::vector<core::Violation>& violations,
                           const std::vector<std::unique_ptr<rules::Rule>>& rules);
};

} // namespace svlang_lint::reporting
