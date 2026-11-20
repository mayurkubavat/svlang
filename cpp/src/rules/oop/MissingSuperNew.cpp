#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::oop {

class MissingSuperNew : public Rule {
public:
    std::string_view getId() const override { return "SV-OOP-003"; }
    std::string_view getName() const override { return "MissingSuperNew"; }
    std::string_view getDescription() const override {
        return "Constructors in derived classes should explicitly invoke super.new(...) to ensure "
               "base class state is initialized.";
    }
    std::string_view getCategory() const override { return "OOP"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        const model::Model& design = uvmModel.facade();
        for (const model::Class& cls : design.classes()) {
            if (cls.isParameterized) continue;
            if (cls.base == model::NoClass) continue;
            const model::Method* ctor = cls.findMethod("new");
            if (!ctor || ctor->isBuiltIn || ctor->callsSuperNew) continue;
            diags.addViolation(
                "SV-OOP-003",
                "MissingSuperNew",
                "Constructor 'new' in class '" + cls.name +
                "' derives from '" + design.cls(cls.base).name +
                "' but does not explicitly call super.new(...).",
                core::Severity::Warning,
                model::toSlang(ctor->loc)
            );
        }
    }
};

} // namespace svlang_lint::rules::oop

namespace svlang_lint::rules {
std::unique_ptr<Rule> createMissingSuperNew() {
    return std::make_unique<oop::MissingSuperNew>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createMissingSuperNew)
