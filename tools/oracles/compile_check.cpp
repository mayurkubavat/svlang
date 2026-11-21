// Minimal Slang compile-check oracle for tools/gates/{ieee_gate,slang_unit_gate}.py.
// Parses and elaborates with upstream Slang (`--std 1800-2023`, `+incdir+`,
// `+define+`, `-f`, etc.) and exits 0 when zero compile errors occur, 1 on
// compile errors, 2 on bad CLI options.
// Built as build/svlang_compile_check (`cmake --preset oracles && cmake --build build`).
#include <slang/ast/Compilation.h>
#include <slang/driver/Driver.h>

int main(int argc, char** argv) {
    slang::driver::Driver driver;
    driver.addStandardArgs();
    if (!driver.parseCommandLine(argc, argv) || !driver.processOptions()) {
        return 2;
    }
    bool parsed = driver.parseAllSources();
    auto comp = driver.createCompilation();
    bool diag_ok = driver.reportCompilation(*comp, /* quiet */ false);
    return (parsed && diag_ok && driver.diagEngine.getNumErrors() == 0) ? 0 : 1;
}
