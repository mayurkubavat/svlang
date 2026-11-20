#pragma once

#include <ostream>
#include <string>

#include "slang/ast/Compilation.h"
#include "svlang_lint/core/Diagnostic.h"
#include "svlang_lint/uvm/UvmModel.h"

namespace svlang_lint::plugins {

/// Runs Python rules (subclasses of svlang-lint.Rule) against an exported design model.
///
/// The model (version 2, see python/svlang_lint/context.py) holds the user classes with their
/// role, base chain, properties and methods; each method's arguments and the calls it makes
/// (name, receiver, declaring class, argument text); config_db set/get calls; TLM ports and
/// connect() calls; and the per-rule options from the project config. It is written to a
/// private temporary file, the runner is started without a shell, and its JSON findings are
/// read from stdout (its stderr passes through).
class PythonPluginManager {
public:
    static bool runPythonRules(const std::string& rulesPath,
                               slang::ast::Compilation& compilation,
                               const svlang_lint::uvm::UvmModel& uvmModel,
                               core::DiagnosticEngine& diags);

    /// Writes the model the Python rules see (exposed for testing and --dump-model use).
    static void writeModel(std::ostream& out, slang::ast::Compilation& compilation,
                           const svlang_lint::uvm::UvmModel& uvmModel);
};

} // namespace svlang_lint::plugins
