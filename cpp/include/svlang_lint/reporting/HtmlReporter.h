#pragma once

#include <string>
#include <vector>
#include <memory>
#include "svlang_lint/core/Diagnostic.h"
#include "svlang_lint/rules/Rule.h"

namespace svlang_lint::reporting {

struct ComplexityRow {
    std::string name;
    std::string file;
    size_t line = 0;
    int cognitive = 0;
    int lines = 0;
};

class HtmlReporter {
public:
    /// Generates a standalone, dependency-free interactive HTML dashboard.
    /// `mostComplex` is the routines with the highest nesting-weighted complexity.
    static bool writeHtml(const std::string& outputPath,
                          const std::vector<core::Violation>& violations,
                          const std::vector<std::unique_ptr<rules::Rule>>& rules,
                          const std::vector<ComplexityRow>& mostComplex = {});

private:
    static std::string escapeHtml(std::string_view str);
};

} // namespace svlang_lint::reporting
