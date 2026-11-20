#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::oop {

class NonVirtualMethodOverride : public Rule {
public:
    std::string_view getId() const override { return "SV-OOP-001"; }
    std::string_view getName() const override { return "NonVirtualMethodOverride"; }
    std::string_view getDescription() const override {
        return "Overriding a non-virtual method in a derived class breaks polymorphism when accessed "
               "via a base class handle.";
    }
    std::string_view getCategory() const override { return "OOP"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        const model::Model& design = uvmModel.facade();
        for (const model::Class& cls : design.classes()) {
            if (cls.isParameterized || cls.base == model::NoClass) continue;
            const auto chain = design.ancestors(design.idOf(cls));

            for (const model::Method& sub : cls.methods) {
                if (sub.isExtern) continue; // as before the port: in-class bodies only
                if (sub.name == "new" || sub.isBuiltIn) continue;

                // The nearest ancestor that declares a method of this name
                for (model::ClassId a : chain) {
                    const model::Class& ancClass = design.cls(a);
                    const model::Method* baseMethod = ancClass.findMethod(sub.name);
                    if (!baseMethod) continue;
                    if (baseMethod->isBuiltIn) break;

                    if (!baseMethod->isVirtual) {
                        auto* v = diags.addViolation(
                            "SV-OOP-001",
                            "NonVirtualMethodOverride",
                            "Method '" + sub.name + "' in class '" +
                            cls.name + "' overrides non-virtual method in base class '" +
                            ancClass.name + "'.",
                            core::Severity::Warning,
                            model::toSlang(sub.loc)
                        );
                        diags.addRelated(v, model::toSlang(baseMethod->loc), "non-virtual method in '" +
                                         ancClass.name + "'");
                    }
                    break; // Stop after finding immediate ancestor definition
                }
            }
        }
    }
};

} // namespace svlang_lint::rules::oop

namespace svlang_lint::rules {
std::unique_ptr<Rule> createNonVirtualMethodOverride() {
    return std::make_unique<oop::NonVirtualMethodOverride>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createNonVirtualMethodOverride)
