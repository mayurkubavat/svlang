#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::uvm {

class UvmSequenceDirectVifAccess : public Rule {
public:
    std::string_view getId() const override { return "UVM-SEQ-001"; }
    std::string_view getName() const override { return "UvmSequenceDirectVifAccess"; }
    std::string_view getDescription() const override {
        return "Sequences and sequence items must not directly declare or drive virtual interface signals. "
               "Signal-level driving is the sole responsibility of the driver.";
    }
    std::string_view getCategory() const override { return "UVM"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation& /*compilation*/,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        for (const model::Class& cls : uvmModel.facade().classes()) {
            if (cls.isParameterized) continue;

            bool isSeq = cls.uvm == model::UvmKind::Sequence;
            bool isSeqItem = cls.uvm == model::UvmKind::SequenceItem;

            if (!isSeq && !isSeqItem) continue;

            // 1. Check class property members
            for (const model::Property& prop : cls.properties) {
                if (!prop.isVirtualInterface) continue;
                std::string kindStr = isSeq ? "Sequence" : "Sequence item";
                diags.addViolation(
                    "UVM-SEQ-001",
                    "UvmSequenceDirectVifAccess",
                    kindStr + " class '" + cls.name +
                    "' directly declares virtual interface property '" + prop.name +
                    "'. Sequences must remain protocol-abstract and transaction-level; "
                    "pin-level driving is the responsibility of the driver component.",
                    core::Severity::Warning,
                    model::toSlang(prop.loc)
                );
            }
            // 2. Check local variables inside methods
            for (const model::Method& sub : cls.methods) {
                if (sub.isExtern) continue; // as before the port: in-class bodies only
                for (const model::Local& var : sub.locals) {
                    if (!var.isVirtualInterface) continue;
                    diags.addViolation(
                        "UVM-SEQ-001",
                        "UvmSequenceDirectVifAccess",
                        "Method '" + sub.name + "' in sequence class '" +
                        cls.name + "' declares virtual interface variable '" +
                        var.name + "'. Pin-level driving must be confined to drivers.",
                        core::Severity::Warning,
                        model::toSlang(var.loc)
                    );
                }
            }
        }
    }
};

} // namespace svlang_lint::rules::uvm

namespace svlang_lint::rules {
std::unique_ptr<Rule> createUvmSequenceDirectVifAccess() {
    return std::make_unique<uvm::UvmSequenceDirectVifAccess>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createUvmSequenceDirectVifAccess)
