#pragma once

#include <string>
#include <vector>

#include "svlang_lint/core/Diagnostic.h"

namespace slang::ast {
class Compilation;
}
namespace svlang_lint::uvm {
class UvmModel;
}

namespace svlang_lint::reporting {

/// Writes the testbench map: the component tree (which component class
/// creates which), TLM ports and connections, and config_db set/get flows,
/// with the findings that fall inside each component class attached to it.
/// A path ending in .json gets the data; anything else a standalone HTML page.
bool writeTestbenchMap(const std::string& path, slang::ast::Compilation& compilation,
                       const uvm::UvmModel& model, const std::vector<core::Violation>& violations);

} // namespace svlang_lint::reporting
