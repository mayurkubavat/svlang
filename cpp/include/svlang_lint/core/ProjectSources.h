#pragma once

#include "svlang_lint/core/ProjectConfig.h"

namespace slang::driver {
class Driver;
}

namespace svlang_lint::core {

/// Feeds the project's build settings to a slang driver, before
/// processOptions(). Include dirs and defines always apply; sources and file
/// lists only when `addSources` is set (the command line named no files).
/// Returns false if a file list could not be read.
bool applyBuildSettings(slang::driver::Driver& driver, const BuildSettings& build, bool addSources);

} // namespace svlang_lint::core
