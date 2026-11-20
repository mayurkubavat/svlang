#include "svlang_lint/rules/Rule.h"
#include "svlang_lint/rules/RuleRegistry.h"
#include "slang/ast/ASTVisitor.h"
#include "slang/ast/expressions/AssignmentExpressions.h"
#include "slang/ast/expressions/MiscExpressions.h"
#include "slang/ast/expressions/ConversionExpression.h"
#include "slang/ast/symbols/VariableSymbols.h"
#include "slang/syntax/AllSyntax.h"
#include "slang/syntax/SyntaxPrinter.h"
#include <algorithm>
#include <cctype>
#include <unordered_map>

namespace svlang_lint::rules::uvm {

class UvmComponentDirectNewVisitor : public engine::AstVisitor<UvmComponentDirectNewVisitor> {
public:
    UvmComponentDirectNewVisitor(const svlang_lint::uvm::UvmModel& model, core::DiagnosticEngine& diags)
        : uvmModel(model), diags(diags) {}

    // The class is named as the target's declaration writes it
    // (`uvm_tlm_fifo#(REQ)`), without a package: one name per line of code.
    // slang's toString() names a specialization's arguments through the alias
    // that created it first (`#(pkg::cls::REQ)` here, `#(int)` there), which
    // depends on elaboration order.
    static std::string writtenType(const slang::ast::Symbol& target) {
        const auto* vs = target.as_if<slang::ast::ValueSymbol>();
        if (!vs) return {};
        const auto* ts = vs->getDeclaredType()->getTypeSyntax();
        if (!ts) return {};
        std::string text =
            slang::syntax::SyntaxPrinter().setIncludeTrivia(false).setIncludeSkipped(false).print(*ts).str();
        std::erase_if(text, [](unsigned char c) { return std::isspace(c); });
        auto head = text.substr(0, text.find('#'));
        if (auto p = head.rfind("::"); p != std::string::npos) text = text.substr(p + 2);
        return text;
    }

    static const slang::ast::NewClassExpression* newOf(const slang::ast::Expression* e) {
        while (e && e->kind == slang::ast::ExpressionKind::Conversion)
            e = &e->as<slang::ast::ConversionExpression>().operand();
        return e ? e->as_if<slang::ast::NewClassExpression>() : nullptr;
    }

    void handle(const slang::ast::AssignmentExpression& a) {
        if (const auto* nc = newOf(&a.right())) {
            if (const auto* sym = a.left().getSymbolReference()) {
                if (auto t = writtenType(*sym); !t.empty()) written[nc] = std::move(t);
            }
        }
        visitDefault(a);
    }

    void handle(const slang::ast::VariableSymbol& v) {
        if (const auto* nc = newOf(v.getInitializer())) {
            if (auto t = writtenType(v); !t.empty()) written[nc] = std::move(t);
        }
        visitDefault(v);
    }

    static bool isNonFactoryInfra(const svlang_lint::uvm::UvmModel& m, const slang::ast::Type& ty) {
        return m.isDerivedFrom(ty, "uvm_tlm_fifo") ||
               m.isDerivedFrom(ty, "uvm_tlm_fifo_base") ||
               m.isDerivedFrom(ty, "uvm_tlm_analysis_fifo") ||
               m.isDerivedFrom(ty, "uvm_port_component") ||
               m.isDerivedFrom(ty, "uvm_port_component_base") ||
               m.isDerivedFrom(ty, "uvm_root");
    }

    void handle(const slang::ast::ClassType& cls) {
        std::string_view name = cls.genericClass ? cls.genericClass->name : cls.name;
        if (name == "uvm_component_registry") return;
        visitDefault(cls);
    }

    void handle(const slang::ast::NewClassExpression& expr) {
        if (!expr.isSuperClass && uvmModel.isUvmComponent(*expr.type) &&
            !isNonFactoryInfra(uvmModel, *expr.type)) {
            auto it = written.find(&expr);
            diags.addViolation(
                "UVM-FAC-001",
                "UvmComponentDirectNew",
                "Direct instantiation of uvm_component '" +
                (it != written.end() ? it->second : std::string(expr.type->toString())) +
                "' using new(). Use type_id::create() to enable factory overrides.",
                core::Severity::Error,
                expr.sourceRange.start()
            );
        }
        visitDefault(expr);
    }

private:
    std::unordered_map<const slang::ast::NewClassExpression*, std::string> written;
    const svlang_lint::uvm::UvmModel& uvmModel;
    core::DiagnosticEngine& diags;
};

class UvmComponentDirectNew : public Rule {
public:
    std::string_view getId() const override { return "UVM-FAC-001"; }
    std::string_view getName() const override { return "UvmComponentDirectNew"; }
    std::string_view getDescription() const override {
        return "UVM components must be created using the UVM factory (type_name::type_id::create) "
               "instead of direct new() calls to support testbench overrides.";
    }
    std::string_view getCategory() const override { return "UVM"; }
    core::Severity getDefaultSeverity() const override { return core::Severity::Error; }

    engine::RuleTask check(engine::RuleContext& ctx) override {
        const auto& uvmModel = ctx.uvmModel();
        auto& diags = ctx.diags();
        UvmComponentDirectNewVisitor visitor(uvmModel, diags);
        co_await ctx.visitAst(visitor);
    }
};

} // namespace svlang_lint::rules::uvm

namespace svlang_lint::rules {
std::unique_ptr<Rule> createUvmComponentDirectNew() {
    return std::make_unique<uvm::UvmComponentDirectNew>();
}
}

SVLANG_LINT_REGISTER_RULE(svlang_lint::rules::createUvmComponentDirectNew)
