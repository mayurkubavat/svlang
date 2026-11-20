#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "slang/ast/symbols/CompilationUnitSymbols.h"
#include "slang/ast/symbols/VariableSymbols.h"
#include "slang/ast/symbols/SubroutineSymbols.h"
#include "slang/ast/types/AllTypes.h"

namespace svlang_lint::rules::portability {

class GlobalUnitPollution : public Rule {
public:
    std::string_view getId() const override { return "SV-MOD-001"; }
    std::string_view getName() const override { return "GlobalUnitPollution"; }
    std::string_view getDescription() const override {
        return "Disallow variables, nets, typedefs, tasks, and functions declared directly in the $unit compilation unit scope. "
               "Declarations outside a package or module cause cross-file namespace pollution and compile-order dependencies in multi-unit compilation.";
    }
    std::string_view getCategory() const override { return "Portability"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Error; }
    uint32_t getProfiles() const override {
        return static_cast<uint32_t>(RuleProfile::Soundness);
    }

    void run(slang::ast::Compilation& compilation,
             const svlang_lint::uvm::UvmModel& /*uvmModel*/,
             core::DiagnosticEngine& diags) override {
        const auto& root = compilation.getRoot();
        for (const auto& topMember : root.members()) {
            if (topMember.kind == slang::ast::SymbolKind::CompilationUnit) {
                const auto& cu = topMember.as<slang::ast::CompilationUnitSymbol>();
                for (const auto& member : cu.members()) {
                    if (member.kind == slang::ast::SymbolKind::Variable ||
                        member.kind == slang::ast::SymbolKind::Net ||
                        member.kind == slang::ast::SymbolKind::TypeAlias ||
                        member.kind == slang::ast::SymbolKind::Subroutine) {
                        
                        std::string symKindStr;
                        if (member.kind == slang::ast::SymbolKind::Variable) symKindStr = "variable";
                        else if (member.kind == slang::ast::SymbolKind::Net) symKindStr = "net";
                        else if (member.kind == slang::ast::SymbolKind::TypeAlias) symKindStr = "typedef";
                        else if (member.kind == slang::ast::SymbolKind::Subroutine) symKindStr = "subroutine";

                        diags.addViolation(
                            "SV-MOD-001",
                            "GlobalUnitPollution",
                            "Illegal declaration of " + symKindStr + " '" + std::string(member.name) +
                            "' in $unit compilation unit scope. All shared data types, variables, and routines "
                            "must be encapsulated within a package (e.g. 'package my_pkg; ... endpackage') or module.",
                            core::Severity::Error,
                            member.location
                        );
                    }
                }
            }
        }
    }
};

} // namespace svlang_lint::rules::portability

namespace svlang_lint::rules {
std::unique_ptr<Rule> createGlobalUnitPollution() {
    return std::make_unique<portability::GlobalUnitPollution>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createGlobalUnitPollution)
