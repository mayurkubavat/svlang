// With the slang feature, lets the binary find libsvlang_lint_model at run time.
fn main() {
    if let Ok(dir) = std::env::var("DEP_SVLANG_LINT_MODEL_LIBDIR") {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    }
}
