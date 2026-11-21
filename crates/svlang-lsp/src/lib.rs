//! `svlang-lsp`: SystemVerilog/UVM language server for code browsing on the
//! native front end (`sv-syntax`) and live lint diagnostics/quick fixes
//! (`svlang-lint`) over stdio.
//!
//! Invoked via the unified CLI as:
//!
//! ```text
//! svlang lsp                 # LSP over stdio
//! svlang lsp --index DIR...  # index and print a summary (for testing)
//! ```

mod index;
mod json;
mod lint;
mod server;
mod text;
mod workspace;

/// Runs the language server CLI (`svlang lsp [args...]`) and returns the exit code.
pub fn run(args: &[String]) -> i32 {
    if matches!(args.first().map(String::as_str), Some("--version" | "-V")) {
        println!("svlang lsp {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        println!(
            "svlang lsp: SystemVerilog/UVM language server over stdio\n\n\
             usage:\n  \
               svlang lsp                 serve LSP over stdin/stdout\n  \
               svlang lsp --index DIR...  index workspace directories and print a summary\n  \
               svlang lsp --version       print the version"
        );
        return 0;
    }
    if args.first().map(String::as_str) == Some("--index") {
        let roots: Vec<std::path::PathBuf> = args[1..].iter().map(std::path::PathBuf::from).collect();
        let mut ws = workspace::Workspace::default();
        let t0 = std::time::Instant::now();
        let n = ws.scan(&roots, &[]);
        let syms: usize = ws.files.iter().flatten().map(|f| f.symbols.len()).sum();
        println!("{} files, {} symbols, {} names, {} ms", n, syms, ws.globals.len(), t0.elapsed().as_millis());
        return 0;
    }
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut s = server::Server::new(Box::new(std::io::stdout()));
    s.run(&mut input)
}
