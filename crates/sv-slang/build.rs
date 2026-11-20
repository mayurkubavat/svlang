// Finds libsvlang_lint_model from the CMake build: $SVLANG_LINT_BUILD_DIR, else
// <repo>/build-model (cmake --preset model). Exposes its directory to dependents as DEP_SVLANG_LINT_MODEL_LIBDIR
// so they can set an rpath (see dv-model/build.rs).
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=SVLANG_LINT_BUILD_DIR");
    let dir = match std::env::var_os("SVLANG_LINT_BUILD_DIR") {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../build-model"),
    };
    let dir = dir.canonicalize().unwrap_or(dir);
    let found = ["libsvlang_lint_model.so", "libsvlang_lint_model.dylib", "svlang_lint_model.dll"]
        .iter()
        .any(|f| dir.join(f).exists());
    if !found {
        panic!(
            "libsvlang_lint_model not found in {}; build it first (cmake --preset model && cmake --build build-model --target svlang_lint_model) \
             or set SVLANG_LINT_BUILD_DIR",
            dir.display()
        );
    }
    println!("cargo:rerun-if-changed={}", dir.join("libsvlang_lint_model.so").display());
    println!("cargo:rustc-link-search=native={}", dir.display());
    println!("cargo:rustc-link-lib=dylib=svlang_lint_model");
    println!("cargo:libdir={}", dir.display());
    println!("cargo:rustc-link-arg=-Wl,-rpath,{}", dir.display());
}
