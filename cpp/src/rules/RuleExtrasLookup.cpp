#include "svlang_lint/rules/RuleExtras.h"

namespace svlang_lint::rules {

const RuleExtra* findRuleExtra(std::string_view ruleId) {
    for (const auto& e : ruleExtras()) {
        if (e.ruleId == ruleId) return &e;
    }
    return nullptr;
}

} // namespace svlang_lint::rules
