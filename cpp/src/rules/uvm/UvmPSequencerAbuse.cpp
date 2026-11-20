#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "svlang_lint/model/SlangModel.h"

namespace svlang_lint::rules::uvm {

class UvmPSequencerAbuse : public Rule {
public:
    std::string_view getId() const override { return "UVM-SEQ-003"; }
    std::string_view getName() const override { return "UvmPSequencerAbuse"; }
    std::string_view getDescription() const override {
        return "Using `uvm_declare_p_sequencer or accessing p_sequencer violates the Law of Demeter by tightly coupling "
               "the sequence data tier to physical sequencer component internals. Pass configuration down through sequence fields or uvm_config_db.";
    }
    std::string_view getCategory() const override { return "Architecture"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Warning; }

    // Reads only svlang_lint::model, not slang's AST.
    void run(slang::ast::Compilation&,
             const svlang_lint::uvm::UvmModel& uvmModel,
             core::DiagnosticEngine& diags) override {
        for (const model::Class& cls : uvmModel.facade().classes()) {
            if (cls.isParameterized || cls.uvm != model::UvmKind::Sequence) continue;
            const model::Property* p = cls.findProperty("p_sequencer");
            if (!p) continue;
            diags.addViolation(
                "UVM-SEQ-003",
                "UvmPSequencerAbuse",
                "Sequence '" + cls.name + "' declares 'p_sequencer' via `uvm_declare_p_sequencer. "
                "This violates the Law of Demeter and architectural layering by tightly binding sequence scenario generation "
                "to physical testbench agent/sequencer topology. If sequencer topology or naming changes, sequences break. "
                "Decouple sequences by passing configuration parameters down through sequence properties or uvm_config_db.",
                core::Severity::Warning,
                model::toSlang(p->loc)
            );
        }
    }
};

} // namespace svlang_lint::rules::uvm

namespace svlang_lint::rules {
std::unique_ptr<Rule> createUvmPSequencerAbuse() {
    return std::make_unique<uvm::UvmPSequencerAbuse>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createUvmPSequencerAbuse)
