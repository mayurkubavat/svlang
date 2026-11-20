#pragma once

// The slang backend of svlang_lint::model: the only place that turns slang's
// elaborated AST into a Model, and Model locations back into slang ones.

#include <functional>
#include <string_view>

#include "slang/text/SourceLocation.h"
#include "svlang_lint/model/Model.h"

namespace slang::ast {
class Compilation;
}

namespace svlang_lint::uvm {
class UvmModel;
}

namespace svlang_lint::model {

struct BuildOptions {
    /// Marks classes whose file is library code; may be empty.
    std::function<bool(std::string_view file)> isLibraryFile;
};

/// Snapshots every class the UVM model discovered (packages, $unit and nested
/// classes, in discovery order) with its properties and methods.
Model buildFromSlang(const slang::ast::Compilation& compilation, const uvm::UvmModel& uvm,
                     const BuildOptions& options = {});

/// The slang location a Model location was made from, for findings.
slang::SourceLocation toSlang(const Loc& loc);

} // namespace svlang_lint::model
