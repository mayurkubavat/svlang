//! `svlang`: the command line of the svlang SystemVerilog front end.
//!
//!     svlang <command> [options] <compile arguments>
//!
//! The compile arguments are what a simulator takes: source files, `-f`/`-F`
//! file lists, `+incdir+DIR`, `+define+NAME=VALUE`, `-y`/`-v` libraries,
//! `-top MODULE` and `-G NAME=VALUE`. See [`USAGE`] for the commands.
//!
//! Exit status: 0 success, 1 the design has errors or cannot be written in the
//! requested form, 2 bad usage or unreadable input.

use std::fmt::Write as _;
use std::io::Write as _;
use std::process::ExitCode;

use svlang::{Compilation, Diagnostic, DiagnosticStage, Driver, LowerConfig, LowerOutcome, Netlist, Severity};

const USAGE: &str = "\
svlang: a SystemVerilog (IEEE 1800-2023) front end

usage: svlang <command> [options] <compile arguments>

commands:
  check      report every error and warning; exit 1 when there are errors
  hier       print the elaborated instance hierarchy and parameter values
  cst        print the syntax tree of every source file
  netlist    lower the design to a word-level RTL netlist and print it
  btor2      write the design as a Btor2 transition system
  aiger      write the design as an and-inverter graph in AIGER
  help       print this text
  version    print the version

compile arguments (as a simulator takes them):
  FILE...  -f LIST  -F LIST  +incdir+DIR  +define+NAME[=VALUE]
  -y DIR  -v FILE  -top MODULE  -G NAME=VALUE  --single-unit

options:
  --json                  check: print the diagnostics as a JSON array
  -o, --output FILE       netlist, btor2, aiger: write to FILE, not stdout
  --ascii                 aiger: write the ASCII form (.aag), not binary
  --trace-outputs         btor2: add the section that names combinational nets
  --blast-memories N      netlist, btor2, aiger: expand memories of depth <= N
  --four-state            netlist, btor2, aiger: keep ===, !== and $isunknown
  --cut PATH              netlist, btor2, aiger: cut the signal at PATH (repeatable)
  --black-box MODULE      netlist, btor2, aiger: do not lower MODULE (repeatable)
  --black-box-instance PATH   the same for one instance (repeatable)

exit status: 0 success, 1 errors in the design, 2 bad usage
";

/// The command's own options; everything else is a compile argument.
#[derive(Default)]
struct Opts {
    json: bool,
    output: Option<String>,
    ascii: bool,
    trace_outputs: bool,
    lower: LowerConfig,
    compile_args: Vec<String>,
}

/// A usage error: printed with a pointer to `svlang help`, exit status 2.
struct Usage(String);

fn parse(args: &[String]) -> Result<Opts, Usage> {
    let mut o = Opts::default();
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<'_, String>| {
        it.next().cloned().ok_or_else(|| Usage(format!("{name} needs a value")))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => o.json = true,
            "--ascii" => o.ascii = true,
            "--trace-outputs" => o.trace_outputs = true,
            "--four-state" => o.lower.allow_four_state_ops = true,
            "-o" | "--output" => o.output = Some(value(a, &mut it)?),
            "--blast-memories" => {
                let v = value(a, &mut it)?;
                o.lower.blast_memories_up_to =
                    Some(v.parse().map_err(|_| Usage(format!("--blast-memories needs a number, not '{v}'")))?);
            }
            "--cut" => o.lower.cut_points.push(value(a, &mut it)?),
            "--black-box" => o.lower.black_box_modules.push(value(a, &mut it)?),
            "--black-box-instance" => o.lower.black_box_instances.push(value(a, &mut it)?),
            _ => o.compile_args.push(a.clone()),
        }
    }
    Ok(o)
}

fn compile(o: &Opts) -> Result<Compilation, Usage> {
    if o.compile_args.is_empty() {
        return Err(Usage("no source files given".into()));
    }
    let mut d = Driver::new();
    d.add_args(&o.compile_args).map_err(|e| Usage(e.to_string()))?;
    d.compile().map_err(|e| Usage(e.to_string()))
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn diagnostics_json(diags: &[Diagnostic]) -> String {
    let mut out = String::from("[");
    for (i, d) in diags.iter().enumerate() {
        let stage = match d.stage {
            DiagnosticStage::FileList => "filelist",
            DiagnosticStage::Preprocessor => "preprocessor",
            DiagnosticStage::Parser => "parser",
            DiagnosticStage::Semantic => "semantic",
        };
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let _ = write!(
            out,
            "{}\n  {{\"file\": {}, \"line\": {}, \"column\": {}, \"stage\": \"{stage}\", \"severity\": \"{severity}\", \"message\": {}}}",
            if i == 0 { "" } else { "," },
            json_string(&d.file),
            d.line,
            d.column,
            json_string(&d.message)
        );
    }
    out.push_str(if diags.is_empty() { "]\n" } else { "\n]\n" });
    out
}

/// Writes `data` to the `-o` file, or to stdout.
fn emit(o: &Opts, data: &[u8]) -> Result<(), Usage> {
    match &o.output {
        Some(path) => std::fs::write(path, data).map_err(|e| Usage(format!("cannot write {path}: {e}"))),
        None => {
            let _ = std::io::stdout().write_all(data);
            Ok(())
        }
    }
}

/// The design's netlist, or its diagnostics on stderr and `None`.
fn lower(comp: &Compilation, o: &Opts) -> Option<Netlist> {
    let (diags, what) = match comp.lower_netlist(&o.lower) {
        LowerOutcome::Lowered(nl) => return Some(nl),
        LowerOutcome::Unsupported(d) => (d, "unsupported"),
        LowerOutcome::Error(d) => (d, "error"),
    };
    for d in &diags {
        eprintln!("{}:{}:{}: {what}: {}", d.file, d.loc.line, d.loc.column, d.message);
    }
    eprintln!("svlang: the design was not lowered ({} {what}(s))", diags.len());
    None
}

fn notes(notes: &[String]) {
    for n in notes {
        eprintln!("note: {n}");
    }
}

fn run(command: &str, o: &Opts) -> Result<ExitCode, Usage> {
    let failed = ExitCode::from(1);
    match command {
        "check" => {
            let diags = compile(o)?.diagnostics();
            let errors = diags.iter().filter(|d| d.severity == Severity::Error).count();
            if o.json {
                print!("{}", diagnostics_json(&diags));
            } else {
                for d in &diags {
                    println!("{d}");
                }
                println!("{errors} error(s), {} warning(s)", diags.len() - errors);
            }
            Ok(if errors == 0 { ExitCode::SUCCESS } else { failed })
        }
        "hier" => {
            let comp = compile(o)?;
            let (diags, elab) = comp.check();
            let errors: Vec<&Diagnostic> = diags.iter().filter(|d| d.severity == Severity::Error).collect();
            for d in &errors {
                eprintln!("{d}");
            }
            for line in &elab.hierarchy {
                println!("{line}");
            }
            Ok(if errors.is_empty() { ExitCode::SUCCESS } else { failed })
        }
        "cst" => {
            let comp = compile(o)?;
            for tree in comp.trees() {
                print!("{}", tree.debug_dump());
            }
            Ok(if comp.has_syntax_errors() { failed } else { ExitCode::SUCCESS })
        }
        "netlist" => {
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            emit(o, nl.dump().as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        "btor2" => {
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            let mut opts = svlang::btor2::Options::default();
            opts.trace_outputs = o.trace_outputs;
            match svlang::btor2::write(&nl, &opts) {
                Ok(m) => {
                    notes(&m.notes);
                    emit(o, m.btor2.as_bytes())?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("svlang: {e}");
                    Ok(failed)
                }
            }
        }
        "aiger" => {
            if o.output.is_none() && !o.ascii {
                return Err(Usage("binary AIGER needs -o FILE (or --ascii to print the text form)".into()));
            }
            let Some(nl) = lower(&compile(o)?, o) else { return Ok(failed) };
            match svlang::aiger::lower(&nl, &svlang::aiger::Options::default()) {
                Ok(m) => {
                    notes(&m.notes);
                    eprintln!("note: {}", m.aig.summary());
                    let data = if o.ascii { m.aig.to_ascii().into_bytes() } else { m.aig.to_binary() };
                    emit(o, &data)?;
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("svlang: {e}");
                    Ok(failed)
                }
            }
        }
        other => Err(Usage(format!("unknown command '{other}'"))),
    }
}

fn main() -> ExitCode {
    svlang::source::exit_on_closed_pipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        eprint!("{USAGE}");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        "version" | "--version" | "-V" => {
            println!("svlang {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        _ => {}
    }
    match parse(&args[1..]).and_then(|o| run(command, &o)) {
        Ok(code) => code,
        Err(Usage(msg)) => {
            eprintln!("svlang: {msg}");
            eprintln!("run 'svlang help' for usage");
            ExitCode::from(2)
        }
    }
}
