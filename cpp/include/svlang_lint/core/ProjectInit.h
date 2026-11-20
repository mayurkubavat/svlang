#pragma once

#include <filesystem>
#include <string>
#include <vector>

#include "svlang_lint/core/ProjectConfig.h"

namespace slang::ast {
class ClassType;
}
namespace svlang_lint::uvm {
class UvmModel;
}
namespace svlang_lint::style {
class NamingEngine;
}

namespace svlang_lint::core::init {

/// What `svlang-lint --init` found in the project folder. Paths in the
/// `*Rel` lists are relative to the root (or start with $UVM_HOME) and are
/// what goes into the config; `build` holds the same with absolute paths.
struct Discovery {
    std::filesystem::path root;
    std::vector<std::string> sourcesRel, filelistsRel, includeDirsRel, libraryRel;
    std::vector<std::string> unusedFilelists; // found but not usable as-is (commented out)
    std::string uvmNote;                      // where UVM came from, for the config comment
    size_t svFiles = 0;
    BuildSettings build;
};

/// Scans `root` for SystemVerilog files, file lists and the UVM library. When
/// `cliFiles` is non-empty those are the sources instead.
Discovery discover(const std::filesystem::path& root, const std::vector<std::filesystem::path>& cliFiles);

/// A naming target whose default convention most of the code does not follow.
struct NamingChoice {
    std::string target;   // "UvmDriver", ...
    std::string pattern;  // inferred regex; empty when disabled
    bool disable = false; // no dominant convention
    std::string evidence; // e.g. "17 of 18 classes end in _drv"
};

/// Infers house naming conventions for UVM class kinds from the user classes
/// in `model`, and applies them to `engine` so this run already uses them.
std::vector<NamingChoice> inferNaming(const uvm::UvmModel& model, style::NamingEngine& engine,
                                      const std::filesystem::path& libraryDir);

/// The .svlang-lint/config.py text.
std::string renderConfig(const Discovery& d, const std::vector<NamingChoice>& naming);

} // namespace svlang_lint::core::init
