#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::oop {

class VirtualCallInConstructor : public Rule {
public:
    std::string_view getId() const override { return "SV-OOP-005"; }
    std::string_view getName() const override { return "VirtualCallInConstructor"; }
    std::string_view getDescription() const override {
        return "Never call virtual methods inside constructors. In SystemVerilog, base constructors execute "
               "before child members are initialized, resulting in child methods operating on uninitialized data.";
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
            for (const model::Method& sub : cls.methods) {
                if (sub.isExtern || sub.name != "new" || sub.isBuiltIn) continue;
                for (const model::Call& call : sub.calls) {
                    if (call.isSystem || !call.calleeDeclaredVirtual) continue;
                    diags.addViolation(
                        "SV-OOP-005",
                        "VirtualCallInConstructor",
                        "Virtual method '" + call.name + "' is invoked inside constructor 'new' of class '" +
                        cls.name + "'. In SystemVerilog, base class constructors execute before derived class members are initialized. "
                        "Calling virtual methods in constructors executes overridden child methods against uninitialized state.",
                        core::Severity::Warning,
                        model::toSlang(call.loc)
                    );
                }
            }
        }
    }
};

} // namespace svlang_lint::rules::oop

namespace svlang_lint::rules {
std::unique_ptr<Rule> createVirtualCallInConstructor() {
    return std::make_unique<oop::VirtualCallInConstructor>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createVirtualCallInConstructor)
