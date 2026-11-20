#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/uvm/UvmModel.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::style {

class UvmComponentNamingConvention : public Rule {
public:
    std::string_view getId() const override { return "SV-NAM-001"; }
    std::string_view getName() const override { return "UvmComponentNamingConvention"; }
    std::string_view getDescription() const override {
        return "Enforces standard naming conventions for UVM components: classes derived from uvm_driver must end "
               "with '_driver', uvm_monitor with '_monitor', uvm_agent with '_agent', uvm_env with '_env', and uvm_test with '_test'.";
    }
    std::string_view getCategory() const override { return "Naming"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        const model::Model& design = uvmModel.facade();
        for (const model::Class& cls : design.classes()) {
            if (cls.isParameterized) continue;
            const std::string& name = cls.name;
            if (name.starts_with("uvm_") || name.empty()) continue;

            struct SuffixCheck {
                std::string_view baseClass;
                std::string_view requiredSuffix;
                std::string_view role;
            };

            static const SuffixCheck checks[] = {
                { "uvm_driver", "_driver", "driver" },
                { "uvm_monitor", "_monitor", "monitor" },
                { "uvm_agent", "_agent", "agent" },
                { "uvm_env", "_env", "environment" },
                { "uvm_test", "_test", "test" }
            };

            for (const auto& check : checks) {
                if (design.derivesFrom(design.idOf(cls), check.baseClass)) {
                    if (!name.ends_with(check.requiredSuffix)) {
                        diags.addViolation(
                            "SV-NAM-001",
                            "UvmComponentNamingConvention",
                            "UVM " + std::string(check.role) + " class '" + name +
                            "' derived from '" + std::string(check.baseClass) +
                            "' does not end with required suffix '" + std::string(check.requiredSuffix) +
                            "'. Following uniform naming conventions is essential for VIP maintainability and automated testbench reflection.",
                            core::Severity::Warning,
                            model::toSlang(cls.loc)
                        );
                    }
                    break;
                }
            }
        }
    }
};

} // namespace svlang_lint::rules::style

namespace svlang_lint::rules {
std::unique_ptr<Rule> createUvmComponentNamingConvention() {
    return std::make_unique<style::UvmComponentNamingConvention>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createUvmComponentNamingConvention)
