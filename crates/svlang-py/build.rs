//! Collects the variant names of `sv_syntax::SyntaxKind`, `sv_lexer::Kind` and
//! `sv_netlist::NetOp` so Python can list them and reject a misspelt kind instead of
//! silently matching nothing. The first two are plain fieldless lists, one variant a line.

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

/// The variant names of `sv_netlist::NetOp`, which has variants with fields (`Slice { hi, lo }`):
/// the names at the enum's own level only.
fn op_names(path: &Path) -> Vec<String> {
    let src = fs::read_to_string(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let head = "pub enum NetOp {";
    let start = src.find(head).unwrap_or_else(|| panic!("no `{head}` in {}", path.display())) + head.len();
    let body = &src[start..start + src[start..].find("\n}").expect("enum end")];
    let (mut depth, mut names) = (0usize, Vec::new());
    for l in body.lines().map(str::trim) {
        if l.is_empty() || l.starts_with("//") || l.starts_with("#[") {
            continue;
        }
        if depth == 0 {
            let name: String = l.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            assert!(!name.is_empty(), "unexpected variant line `{l}` in NetOp");
            names.push(name);
        }
        depth += l.matches('{').count();
        depth -= l.matches('}').count();
    }
    names
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let syntax = root.join("sv-syntax/src/kinds.rs");
    let lexer = root.join("sv-lexer/src/lib.rs");
    println!("cargo:rerun-if-changed={}", syntax.display());
    let netlist = root.join("sv-netlist/src/ir.rs");
    println!("cargo:rerun-if-changed={}", lexer.display());
    println!("cargo:rerun-if-changed={}", netlist.display());
    let list = |v: Vec<String>| v.iter().map(|n| format!("{n:?}")).collect::<Vec<_>>().join(", ");
    let out = format!(
        "pub const SYNTAX_KINDS: &[&str] = &[{}];\npub const TOKEN_KINDS: &[&str] = &[{}];\n\
         pub const NET_OPS: &[&str] = &[{}];\n",
        list(variants(&syntax, "SyntaxKind")),
        list(variants(&lexer, "Kind")),
        list(op_names(&netlist))
    );
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("kinds.rs"), out).unwrap();
}
