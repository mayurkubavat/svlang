#pragma once

#include <memory>
#include <vector>

#include "svlang_lint/rules/Rule.h"

namespace svlang_lint::rules {

using RuleFactory = std::unique_ptr<Rule> (*)();

/// Factories registered by SVLANG_LINT_REGISTER_RULE in each rule file. Filled during
/// static initialization; registerBuiltinRules() instantiates them.
std::vector<RuleFactory>& ruleFactories();

inline bool addRuleFactory(RuleFactory factory) {
    ruleFactories().push_back(factory);
    return true;
}

class RuleRegistry {
public:
    static RuleRegistry& instance() {
        static RuleRegistry reg;
        return reg;
    }

    void registerRule(std::unique_ptr<Rule> rule) {
        rules.push_back(std::move(rule));
    }

    const std::vector<std::unique_ptr<Rule>>& getRules() const {
        return rules;
    }

    /// Orders rules by ID so listings, catalogs and scheduling do not depend on link order.
    void sortById();

private:
    RuleRegistry() = default;
    std::vector<std::unique_ptr<Rule>> rules;
};

#define SVLANG_LINT_RULE_CONCAT_INNER(a, b) a##b
#define SVLANG_LINT_RULE_CONCAT(a, b) SVLANG_LINT_RULE_CONCAT_INNER(a, b)

/// Registers a rule factory (a function returning std::unique_ptr<Rule>). Put it at file
/// scope, after the factory's definition. Adding a rule then touches only its own file
/// and its tests; the library is linked whole-archive so no registration is dropped.
#define SVLANG_LINT_REGISTER_RULE(factory)                                             \
    [[maybe_unused]] static const bool SVLANG_LINT_RULE_CONCAT(svlang_lint_rule_reg_, __COUNTER__) = \
        ::svlang_lint::rules::addRuleFactory(&factory);

} // namespace svlang_lint::rules
