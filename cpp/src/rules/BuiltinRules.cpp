#include "svlang_lint/rules/BuiltinRules.h"

#include <algorithm>

#include "svlang_lint/rules/RuleRegistry.h"

namespace svlang_lint::rules {

std::vector<RuleFactory>& ruleFactories() {
    static std::vector<RuleFactory> factories;
    return factories;
}

void RuleRegistry::sortById() {
    std::stable_sort(rules.begin(), rules.end(),
                     [](const auto& a, const auto& b) { return a->getId() < b->getId(); });
}

void registerBuiltinRules() {
    auto& reg = RuleRegistry::instance();
    if (!reg.getRules().empty()) return;
    for (auto factory : ruleFactories()) reg.registerRule(factory());
    reg.sortById();
}

} // namespace svlang_lint::rules
