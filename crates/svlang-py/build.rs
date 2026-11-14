//! Collects the variant names of `sv_syntax::SyntaxKind` and `sv_lexer::Kind`
//! so Python can list them and reject a misspelt kind instead of silently
//! matching nothing. Both enums are plain fieldless lists, one variant a line.

use std::path::Path;
use std::{env, fs};

fn variants(path: &Path, enum_name: &str) -> Vec<String> {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let head = format!("pub enum {enum_name} {{");
    let start = src.find(&head).unwrap_or_else(|| panic!("no `{head}` in {}", path.display())) + head.len();
    let body = &src[start..start + src[start..].find("\n}").expect("enum end")];
    body.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("#["))
        .map(|l| {
            let name = l.trim_end_matches(',');
            assert!(name.chars().all(|c| c.is_ascii_alphanumeric()), "unexpected variant line `{l}` in {enum_name}");
            name.to_owned()
        })
        .collect()
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let syntax = root.join("sv-syntax/src/kinds.rs");
    let lexer = root.join("sv-lexer/src/lib.rs");
    println!("cargo:rerun-if-changed={}", syntax.display());
    println!("cargo:rerun-if-changed={}", lexer.display());
    let list = |v: Vec<String>| v.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let out = format!(
        "pub const SYNTAX_KINDS: &[&str] = &[{}];\npub const TOKEN_KINDS: &[&str] = &[{}];\n",
        list(variants(&syntax, "SyntaxKind")),
        list(variants(&lexer, "Kind"))
    );
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("kinds.rs"), out).unwrap();
}
