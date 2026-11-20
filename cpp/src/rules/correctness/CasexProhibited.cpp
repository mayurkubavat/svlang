#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "slang/syntax/AllSyntax.h"
#include "slang/syntax/SyntaxTree.h"
#include "slang/syntax/SyntaxVisitor.h"

namespace svlang_lint::rules::correctness {

class CasexVisitor : public engine::SyntaxVisitor<CasexVisitor> {
public:
    explicit CasexVisitor(core::DiagnosticEngine& diags) : diags(diags) {}

    void handle(const slang::syntax::CaseStatementSyntax& node) {
        if (node.caseKeyword.kind == slang::parsing::TokenKind::CaseXKeyword) {
            std::vector<core::FixItHint> fixIts;
            fixIts.push_back({
                node.caseKeyword.range(),
                "casez",
                "Replace 'casex' with 'casez'",
                core::FixSafety::Unsafe
            });

            diags.addViolation(
                "SV-COR-009",
                "CasexProhibited",
                "Use of 'casex' statement is prohibited in verification environments. "
                "In SystemVerilog (IEEE 1800 § 12.5.3), 'casex' treats both literal 'x'/'z' and unknown simulation 'x' bits as don't-care wildcards. "
                "If an uninitialized signal, timing race, or bug produces 'x', 'casex' silently matches the first matching branch, "
                "masking critical testbench and DUT failures. Replace 'casex' with 'casez' or 'case inside'.",
                core::Severity::Error,
                node.caseKeyword.location(),
                std::move(fixIts)
            );
        }
        visitDefault(node);
    }

private:
    core::DiagnosticEngine& diags;
};

class CasexProhibited : public Rule {
public:
    std::string_view getId() const override { return "SV-COR-009"; }
    std::string_view getName() const override { return "CasexProhibited"; }
    std::string_view getDescription() const override {
        return "Disallows use of 'casex' statements. 'casex' treats unknown simulation 'x' values as wildcards, "
               "masking uninitialized logic bugs and timing errors.";
    }
    std::string_view getCategory() const override { return "Correctness"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Error; }

    engine::RuleTask check(engine::RuleContext& ctx) override {
        auto& diags = ctx.diags();
        CasexVisitor visitor(diags);
        co_await ctx.visitSyntax(visitor);
    }
};

} // namespace svlang_lint::rules::correctness

namespace svlang_lint::rules {
std::unique_ptr<Rule> createCasexProhibited() {
    return std::make_unique<correctness::CasexProhibited>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createCasexProhibited)
