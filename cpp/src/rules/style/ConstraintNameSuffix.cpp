#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::style {

class ConstraintNameSuffix : public Rule {
public:
    std::string_view getId() const override { return "UVM-STY-002"; }
    std::string_view getName() const override { return "ConstraintNameSuffix"; }
    std::string_view getDescription() const override {
        return "Enforces a naming standard requiring all constraint blocks to end with '_c' suffix.";
    }
    std::string_view getCategory() const override { return "Style"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        for (const model::Class& cls : uvmModel.facade().classes()) {
            if (cls.isParameterized) continue;
            for (const model::Constraint& c : cls.constraints) {
                if (c.name.ends_with("_c")) continue;
                diags.addViolation(
                    "UVM-STY-002",
                    "ConstraintNameSuffix",
                    "Constraint block '" + c.name + "' in class '" + cls.name +
                    "' does not follow the constraint naming convention. "
                    "Constraint block names must end with '_c' suffix (e.g. '" + c.name +
                    "_c') to clearly distinguish constraint sets from random variables in solver logs.",
                    core::Severity::Warning,
                    model::toSlang(c.loc)
                );
            }
        }
    }
};

} // namespace svlang_lint::rules::style

namespace svlang_lint::rules {
std::unique_ptr<Rule> createConstraintNameSuffix() {
    return std::make_unique<style::ConstraintNameSuffix>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createConstraintNameSuffix)
