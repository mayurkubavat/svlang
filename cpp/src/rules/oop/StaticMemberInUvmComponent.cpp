#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::oop {

class StaticMemberInUvmComponent : public Rule {
public:
    std::string_view getId() const override { return "SV-OOP-007"; }
    std::string_view getName() const override { return "StaticMemberInUvmComponent"; }
    std::string_view getDescription() const override {
        return "Non-const static variable declared in a uvm_component subclass is shared across all instances "
               "simulation-wide. In multi-channel or multi-agent environments, instances will overwrite each other's state.";
    }
    std::string_view getCategory() const override { return "Architecture"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Error; }

    // Reads only svlang_lint::model, not slang's AST: the first rule on the facade.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        const model::Model& design = uvmModel.facade();
        for (const model::Class& cls : design.classes()) {
            if (cls.isParameterized || !model::isComponent(cls.uvm) || cls.name == "uvm_component") continue;
            bool restrictedNew = false;
            for (const model::Method& m : cls.methods) {
                if (!m.isBuiltIn && m.name == "new" && m.visibility != model::Visibility::Public) {
                    restrictedNew = true;
                    break;
                }
            }
            if (restrictedNew) continue;
            for (const model::Property& prop : cls.properties) {
                if (!prop.isStatic || prop.isConst) continue;
                diags.addViolation(
                    "SV-OOP-007",
                    "StaticMemberInUvmComponent",
                    "Non-const static member property '" + prop.name + "' in uvm_component class '" +
                    cls.name + "'. In SystemVerilog, static class members are shared across all instances "
                    "simulation-wide. In multi-channel environments (e.g. multiple agents), instances will overwrite each other's state "
                    "causing race conditions. Use an instance-scoped member or a dedicated singleton service instead.",
                    core::Severity::Error,
                    model::toSlang(prop.loc)
                );
            }
        }
    }
};

} // namespace svlang_lint::rules::oop

namespace svlang_lint::rules {
std::unique_ptr<Rule> createStaticMemberInUvmComponent() {
    return std::make_unique<oop::StaticMemberInUvmComponent>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createStaticMemberInUvmComponent)
