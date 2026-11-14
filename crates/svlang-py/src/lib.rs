//! Python bindings for svlang, shipped on PyPI as `pysvlang`.
//!
//! The Python objects are thin handles. A [`Compilation`] owns the parsed
//! trees behind an `Arc`; every [`SyntaxTree`], [`SyntaxNode`] and [`Token`]
//! is that `Arc` plus an index, so walking a tree from Python copies nothing
//! and a node stays valid for as long as anything refers to it.

use std::fmt::Write as _;
use std::sync::Arc;

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList, PyString};

use svlang::syntax::Child;
use svlang::{DiagnosticStage, Driver, Locator, NodeId, Severity, Tree};

create_exception!(pysvlang, SvlangError, PyException, "Raised when svlang cannot read or compile its inputs.");
create_exception!(
    pysvlang,
    LowerError,
    SvlangError,
    "Raised when a design cannot be lowered to a netlist. `diagnostics` holds the reasons and \
     `unsupported` is True when the design is legal but uses a construct the netlist does not cover."
);

mod kinds {
    include!(concat!(env!("OUT_DIR"), "/kinds.rs"));
}

/// Rejects a syntax kind name that does not exist, so a typo raises instead of
/// matching nothing.
fn check_kind(kind: &str) -> PyResult<()> {
    if kinds::SYNTAX_KINDS.contains(&kind) {
        Ok(())
    } else {
        Err(PyValueError::new_err(format!("unknown syntax kind {kind:?} (see pysvlang.SYNTAX_KINDS)")))
    }
}

/// `text` with runs of whitespace folded to one space, cut to about `max` characters.
fn short(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        flat.chars().take(max - 3).collect::<String>() + "..."
    } else {
        flat
    }
}

fn sv_err(e: svlang::Error) -> PyErr {
    SvlangError::new_err(e.to_string())
}

// ---------------------------------------------------------------------------
// Diagnostics and locations
// ---------------------------------------------------------------------------

/// A compiler diagnostic with 1-based coordinates.
#[pyclass(frozen, get_all, module = "pysvlang")]
#[derive(Clone)]
struct Diagnostic {
    /// File path, or the name given to an in-memory source.
    file: String,
    /// 1-based line (0 when unknown).
    line: u32,
    /// 1-based byte column (0 when reported at a macro expansion).
    column: u32,
    /// "filelist", "preprocessor", "parser" or "semantic".
    stage: &'static str,
    /// "error" or "warning".
    severity: &'static str,
    /// The message text.
    message: String,
}

fn severity_name(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

impl From<svlang::Diagnostic> for Diagnostic {
    fn from(d: svlang::Diagnostic) -> Self {
        let stage = match d.stage {
            DiagnosticStage::FileList => "filelist",
            DiagnosticStage::Preprocessor => "preprocessor",
            DiagnosticStage::Parser => "parser",
            DiagnosticStage::Semantic => "semantic",
        };
        Self {
            file: d.file,
            line: d.line,
            column: d.column,
            stage,
            severity: severity_name(d.severity),
            message: d.message,
        }
    }
}

#[pymethods]
impl Diagnostic {
    fn __str__(&self) -> String {
        format!("{}:{}:{}: {}: {}", self.file, self.line, self.column, self.severity, self.message)
    }

    fn __repr__(&self) -> String {
        format!("Diagnostic({:?})", self.__str__())
    }
}

/// A source position. For code a macro produced, `file`/`line`/`column` are
/// where the text was written and `expansion_file`/`expansion_line` are the
/// outermost place the macro was used.
#[pyclass(frozen, get_all, module = "pysvlang")]
#[derive(Clone)]
struct Location {
    file: String,
    line: u32,
    column: u32,
    expansion_file: Option<String>,
    expansion_line: Option<u32>,
}

#[pymethods]
impl Location {
    fn __str__(&self) -> String {
        format!("{}:{}:{}", self.file, self.line, self.column)
    }

    fn __repr__(&self) -> String {
        format!("Location({:?})", self.__str__())
    }
}

fn locate(tree_ix: usize, tree: &Tree, tok: u32) -> Option<Location> {
    if tok as usize >= tree.tokens.len() {
        return None;
    }
    let mut loc = Locator::default();
    let l = loc.loc(tree_ix, tree, tok);
    let name = |f: u32| loc.files().get(f as usize).cloned().unwrap_or_default();
    Some(Location {
        file: name(l.file),
        line: l.line,
        column: l.column,
        expansion_file: l.expansion.map(|(f, _)| name(f)),
        expansion_line: l.expansion.map(|(_, line)| line),
    })
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Parsed SystemVerilog sources, ready for diagnostics, elaboration and
/// netlist lowering.
///
/// ``Compilation(files, *, include_dirs=(), defines=None, top=(), params=None,
/// sources=None, single_unit=False)`` parses files from disk. ``defines`` maps
/// macro names to values (``None`` for a bare ``+define+NAME``), ``params``
/// maps parameter names to ``-G`` overrides, and ``sources`` maps names to
/// in-memory source text compiled after the files.
#[pyclass(frozen, module = "pysvlang")]
struct Compilation {
    inner: Arc<svlang::Compilation>,
}

fn str_pairs(d: Option<&Bound<'_, PyDict>>) -> PyResult<Vec<(String, Option<String>)>> {
    let mut out = Vec::new();
    if let Some(d) = d {
        for (k, v) in d.iter() {
            let v = if v.is_none() { None } else { Some(v.str()?.to_string()) };
            out.push((k.extract::<String>()?, v));
        }
    }
    Ok(out)
}

#[pymethods]
impl Compilation {
    #[new]
    #[pyo3(signature = (files = Vec::new(), *, include_dirs = Vec::new(), defines = None, top = Vec::new(), params = None, sources = None, single_unit = false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        files: Vec<std::path::PathBuf>,
        include_dirs: Vec<std::path::PathBuf>,
        defines: Option<&Bound<'_, PyDict>>,
        top: Vec<String>,
        params: Option<&Bound<'_, PyDict>>,
        sources: Option<&Bound<'_, PyDict>>,
        single_unit: bool,
    ) -> PyResult<Self> {
        let mut d = Driver::new().single_unit(single_unit);
        for f in files {
            d = d.add_file(f);
        }
        for i in include_dirs {
            d = d.include_dir(i);
        }
        for (n, v) in str_pairs(defines)? {
            d = d.define(n, v);
        }
        for t in top {
            d = d.top(t);
        }
        for (n, v) in str_pairs(params)? {
            let v = v.ok_or_else(|| PyValueError::new_err(format!("parameter {n} needs a value")))?;
            d = d.param(n, v);
        }
        for (n, v) in str_pairs(sources)? {
            d = d.add_source(n, v.unwrap_or_default());
        }
        let comp = py.detach(move || d.compile()).map_err(sv_err)?;
        Ok(Self { inner: Arc::new(comp) })
    }

    /// Parses one in-memory source string.
    #[staticmethod]
    #[pyo3(signature = (text, name = "source.sv"))]
    fn from_source(py: Python<'_>, text: String, name: &str) -> Self {
        let name = name.to_owned();
        let comp = py.detach(move || svlang::Compilation::from_source(&name, &text));
        Self { inner: Arc::new(comp) }
    }

    /// Compiles simulator-style arguments: files, ``+incdir+``, ``+define+``,
    /// ``-f``/``-F`` filelists, ``-v``/``-y`` libraries, ``-top`` and ``-G``.
    #[staticmethod]
    fn from_args(py: Python<'_>, args: Vec<String>) -> PyResult<Self> {
        let comp = py.detach(move || svlang::Compilation::from_args(args)).map_err(sv_err)?;
        Ok(Self { inner: Arc::new(comp) })
    }

    /// The parsed syntax trees, one per compilation unit (library trees included).
    #[getter]
    fn trees(&self) -> Vec<SyntaxTree> {
        (0..self.inner.trees().len()).map(|ix| SyntaxTree { comp: self.inner.clone(), ix }).collect()
    }

    /// Explicit top module names, empty when the tops are inferred.
    #[getter]
    fn tops(&self) -> Vec<String> {
        self.inner.tops().to_vec()
    }

    /// True when any tree has preprocessor or syntax errors.
    #[getter]
    fn has_syntax_errors(&self) -> bool {
        self.inner.has_syntax_errors()
    }

    /// Every filelist, preprocessor, parser, semantic and elaboration diagnostic.
    fn diagnostics(&self, py: Python<'_>) -> Vec<Diagnostic> {
        let comp = self.inner.clone();
        py.detach(move || comp.diagnostics()).into_iter().map(Diagnostic::from).collect()
    }

    /// The elaborated instance hierarchy and parameter values, one line each.
    fn hierarchy(&self, py: Python<'_>) -> Vec<String> {
        let comp = self.inner.clone();
        py.detach(move || comp.elaborate().hierarchy)
    }

    /// Elaborates and lowers the design to a word-level RTL netlist.
    ///
    /// ``black_box_modules`` names modules and ``black_box_instances`` names
    /// instance paths whose bodies are not lowered: their ports stay and their
    /// outputs become free values.
    ///
    /// With ``compile_properties``, each concurrent property in the design
    /// (``assert property``, ``assume property``, ``cover property``) that
    /// is over bounded sequences becomes a check, which ``to_btor2`` then writes;
    /// ``skip_cycles`` leaves out attempts that start in the first clock
    /// ticks. ``Netlist.properties`` says what happened to each.
    ///
    /// Raises ``LowerError`` when the design has errors or uses constructs
    /// outside the synthesizable/formal subset.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, top = None, max_loop_iters = 65_536, blast_memories_up_to = None, allow_four_state_ops = false, cut_points = Vec::new(), black_box_modules = Vec::new(), black_box_instances = Vec::new(), compile_properties = false, skip_cycles = 0))]
    fn lower_netlist(
        &self,
        py: Python<'_>,
        top: Option<String>,
        max_loop_iters: usize,
        blast_memories_up_to: Option<usize>,
        allow_four_state_ops: bool,
        cut_points: Vec<String>,
        black_box_modules: Vec<String>,
        black_box_instances: Vec<String>,
        compile_properties: bool,
        skip_cycles: u32,
    ) -> PyResult<Netlist> {
        // `..Default::default()` keeps this building when svlang adds a LowerConfig option.
        #[allow(clippy::needless_update)]
        let cfg = svlang::LowerConfig {
            top,
            max_loop_iters,
            blast_memories_up_to,
            allow_four_state_ops,
            cut_points,
            black_box_modules,
            black_box_instances,
            ..Default::default()
        };
        let comp = self.inner.clone();
        let outcome = py.detach(move || {
            let mut outcome = comp.lower_netlist(&cfg);
            if let (svlang::LowerOutcome::Lowered(nl), true) = (&mut outcome, compile_properties) {
                let mut opts = svlang::netlist::PropertyOptions::default();
                opts.skip_cycles = skip_cycles;
                nl.compile_properties(&opts);
            }
            outcome
        });
        let (diags, unsupported) = match outcome {
            svlang::LowerOutcome::Lowered(nl) => return Ok(Netlist { inner: Arc::new(nl) }),
            svlang::LowerOutcome::Unsupported(d) => (d, true),
            svlang::LowerOutcome::Error(d) => (d, false),
        };
        let mut msg = String::from(if unsupported { "unsupported construct" } else { "lowering failed" });
        for d in &diags {
            let _ = write!(msg, "\n  {d}");
        }
        let list: Vec<NetlistDiagnostic> = diags.into_iter().map(NetlistDiagnostic::from).collect();
        let err = LowerError::new_err(msg);
        let value = err.value(py);
        value.setattr("diagnostics", PyList::new(py, list)?)?;
        value.setattr("unsupported", unsupported)?;
        Err(err)
    }

    fn __repr__(&self) -> String {
        format!("Compilation(trees={})", self.inner.trees().len())
    }
}

// ---------------------------------------------------------------------------
// Syntax trees
// ---------------------------------------------------------------------------

/// The lossless concrete syntax tree of one compilation unit.
#[pyclass(frozen, module = "pysvlang")]
struct SyntaxTree {
    comp: Arc<svlang::Compilation>,
    ix: usize,
}

impl SyntaxTree {
    fn tree(&self) -> &Tree {
        &self.comp.trees()[self.ix]
    }
}

/// A parse error inside one tree.
#[pyclass(frozen, get_all, module = "pysvlang")]
struct SyntaxError {
    message: String,
    location: Option<Location>,
}

#[pymethods]
impl SyntaxError {
    fn __repr__(&self) -> String {
        match &self.location {
            Some(l) => format!("SyntaxError({:?})", format!("{}: {}", l.__str__(), self.message)),
            None => format!("SyntaxError({:?})", self.message),
        }
    }
}

#[pymethods]
impl SyntaxTree {
    /// The ``SourceFile`` root node.
    #[getter]
    fn root(&self) -> SyntaxNode {
        SyntaxNode { comp: self.comp.clone(), ix: self.ix, id: self.tree().root }
    }

    /// Every file the preprocessor read for this unit, includes too.
    #[getter]
    fn files(&self) -> Vec<String> {
        self.tree().files.iter().map(|p| p.display().to_string()).collect()
    }

    /// Parse errors, in token order.
    #[getter]
    fn errors(&self) -> Vec<SyntaxError> {
        let t = self.tree();
        t.errors
            .iter()
            .map(|e| SyntaxError { message: e.message.clone(), location: locate(self.ix, t, e.token) })
            .collect()
    }

    /// True for a tree pulled in from a ``-v``/``-y`` library.
    #[getter]
    fn is_library(&self) -> bool {
        self.tree().library
    }

    /// Every node in preorder.
    fn preorder(&self) -> Vec<SyntaxNode> {
        self.tree().preorder().into_iter().map(|id| SyntaxNode { comp: self.comp.clone(), ix: self.ix, id }).collect()
    }

    /// Every token, in order (trivia is attached to the token after it).
    fn tokens(&self) -> Vec<Token> {
        (0..self.tree().tokens.len() as u32).map(|t| Token { comp: self.comp.clone(), ix: self.ix, tok: t }).collect()
    }

    /// The text of file ``index`` (see ``files``) rebuilt from the tree's leaves.
    #[pyo3(signature = (index = 0))]
    fn file_text(&self, index: u32) -> PyResult<String> {
        if index as usize >= self.tree().sources.len() {
            return Err(PyValueError::new_err(format!("no file {index} in this tree")));
        }
        Ok(self.tree().file_text(index))
    }

    /// An indented dump of the tree, for debugging.
    fn dump(&self) -> String {
        self.tree().debug_dump()
    }

    fn __len__(&self) -> usize {
        self.tree().nodes.len()
    }

    fn __repr__(&self) -> String {
        let t = self.tree();
        let file = t.files.first().map(|p| p.display().to_string()).unwrap_or_default();
        format!("SyntaxTree({file:?}, nodes={}, errors={})", t.nodes.len(), t.errors.len())
    }
}

/// A node of a syntax tree. ``kind`` names the IEEE 1800 construct, such as
/// ``"ModuleDecl"``; ``pysvlang.SYNTAX_KINDS`` lists them all.
#[pyclass(frozen, module = "pysvlang")]
struct SyntaxNode {
    comp: Arc<svlang::Compilation>,
    ix: usize,
    id: NodeId,
}

impl SyntaxNode {
    fn tree(&self) -> &Tree {
        &self.comp.trees()[self.ix]
    }

    fn node(&self, id: NodeId) -> SyntaxNode {
        SyntaxNode { comp: self.comp.clone(), ix: self.ix, id }
    }

    fn kind_name(&self, id: NodeId) -> String {
        format!("{:?}", self.tree().kind(id))
    }
}

#[pymethods]
impl SyntaxNode {
    /// The syntax kind, such as ``"ModuleDecl"``.
    #[getter]
    fn kind(&self) -> String {
        self.kind_name(self.id)
    }

    /// The node's index in its tree (stable for the tree's lifetime).
    #[getter]
    fn id(&self) -> u32 {
        self.id.0
    }

    /// The tree this node belongs to.
    #[getter(tree)]
    fn get_tree(&self) -> SyntaxTree {
        SyntaxTree { comp: self.comp.clone(), ix: self.ix }
    }

    /// Sub-nodes and tokens, in source order.
    #[getter]
    fn children(&self, py: Python<'_>) -> PyResult<Vec<Py<PyAny>>> {
        let mut out = Vec::new();
        for c in &self.tree().node(self.id).children {
            out.push(match *c {
                Child::Node(n) => Py::new(py, self.node(n))?.into_any(),
                Child::Token(t) => Py::new(py, Token { comp: self.comp.clone(), ix: self.ix, tok: t })?.into_any(),
            });
        }
        Ok(out)
    }

    /// Sub-nodes only, in source order.
    #[getter]
    fn child_nodes(&self) -> Vec<SyntaxNode> {
        self.tree().children(self.id).map(|n| self.node(n)).collect()
    }

    /// Every token under this node, in order.
    #[getter]
    fn tokens(&self) -> Vec<Token> {
        self.tree()
            .token_range(self.id)
            .into_iter()
            .map(|t| Token { comp: self.comp.clone(), ix: self.ix, tok: t })
            .collect()
    }

    /// The node's text as written, comments and whitespace included.
    #[getter]
    fn text(&self) -> String {
        self.tree().text(self.id)
    }

    /// The node's tokens joined without trivia.
    #[getter]
    fn bare_text(&self) -> String {
        self.tree().bare_text(self.id)
    }

    /// Where the node's first token is, or None for an empty node.
    #[getter]
    fn location(&self) -> Option<Location> {
        let (first, _) = self.tree().token_bounds(self.id)?;
        locate(self.ix, self.tree(), first)
    }

    /// The first direct sub-node of the given kind, or None.
    fn child(&self, kind: &str) -> PyResult<Option<SyntaxNode>> {
        check_kind(kind)?;
        Ok(self.tree().children(self.id).find(|&n| self.kind_name(n) == kind).map(|n| self.node(n)))
    }

    /// This node and everything below it, in preorder.
    fn walk(&self) -> Vec<SyntaxNode> {
        self.tree().subtree(self.id).into_iter().map(|n| self.node(n)).collect()
    }

    /// Every node of the given kind in this subtree (this node included), in preorder.
    fn find_all(&self, kind: &str) -> PyResult<Vec<SyntaxNode>> {
        check_kind(kind)?;
        Ok(self
            .tree()
            .subtree(self.id)
            .into_iter()
            .filter(|&n| self.kind_name(n) == kind)
            .map(|n| self.node(n))
            .collect())
    }

    fn __eq__(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.comp, &other.comp) && self.ix == other.ix && self.id == other.id
    }

    fn __hash__(&self) -> u64 {
        (Arc::as_ptr(&self.comp) as u64) ^ ((self.ix as u64) << 32) ^ self.id.0 as u64
    }

    fn __repr__(&self) -> String {
        format!("SyntaxNode({}, {:?})", self.kind(), short(&self.text(), 40))
    }
}

/// A token: the leaves of a syntax tree.
#[pyclass(frozen, module = "pysvlang")]
struct Token {
    comp: Arc<svlang::Compilation>,
    ix: usize,
    tok: u32,
}

impl Token {
    fn tree(&self) -> &Tree {
        &self.comp.trees()[self.ix]
    }
}

#[pymethods]
impl Token {
    /// The lexer kind, such as ``"Identifier"`` or ``"Keyword"``; ``pysvlang.TOKEN_KINDS`` lists them all.
    #[getter]
    fn kind(&self) -> String {
        format!("{:?}", self.tree().tokens[self.tok as usize].kind)
    }

    /// The token text.
    #[getter]
    fn text(&self) -> String {
        self.tree().tokens[self.tok as usize].text.to_string()
    }

    /// Whitespace, comments and directives written just before the token.
    #[getter]
    fn leading_trivia(&self) -> String {
        self.tree().trivia(self.tok).to_owned()
    }

    /// True when a macro expansion produced the token.
    #[getter]
    #[allow(clippy::wrong_self_convention)]
    fn from_macro(&self) -> bool {
        self.tree().tokens[self.tok as usize].from_macro
    }

    /// The token's index in its tree.
    #[getter]
    fn index(&self) -> u32 {
        self.tok
    }

    #[getter]
    fn location(&self) -> Option<Location> {
        locate(self.ix, self.tree(), self.tok)
    }

    fn __eq__(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.comp, &other.comp) && self.ix == other.ix && self.tok == other.tok
    }

    fn __hash__(&self) -> u64 {
        (Arc::as_ptr(&self.comp) as u64) ^ ((self.ix as u64) << 32) ^ ((self.tok as u64) << 1) ^ 1
    }

    fn __repr__(&self) -> String {
        format!("Token({}, {:?})", self.kind(), self.text())
    }
}

// ---------------------------------------------------------------------------
// Netlist
// ---------------------------------------------------------------------------

/// Why a design could not be lowered.
#[pyclass(frozen, get_all, module = "pysvlang")]
#[derive(Clone)]
struct NetlistDiagnostic {
    file: String,
    line: u32,
    column: u32,
    message: String,
    /// True when the construct is legal SystemVerilog the netlist does not cover.
    unsupported: bool,
}

impl From<svlang::NetlistDiag> for NetlistDiagnostic {
    fn from(d: svlang::NetlistDiag) -> Self {
        Self { file: d.file, line: d.loc.line, column: d.loc.column, message: d.message, unsupported: d.unsupported }
    }
}

#[pymethods]
impl NetlistDiagnostic {
    fn __str__(&self) -> String {
        format!("{}:{}:{}: {}", self.file, self.line, self.column, self.message)
    }

    fn __repr__(&self) -> String {
        format!("NetlistDiagnostic({:?})", self.__str__())
    }
}

/// A top-level port or a register of a netlist.
#[pyclass(frozen, get_all, module = "pysvlang")]
#[derive(Clone)]
struct Signal {
    name: String,
    width: u32,
    signed: bool,
}

#[pymethods]
impl Signal {
    fn __repr__(&self) -> String {
        format!("Signal({:?}, width={}{})", self.name, self.width, if self.signed { ", signed" } else { "" })
    }
}

/// A word-level RTL netlist: a topologically sorted value DAG with primary
/// inputs, outputs, registers and lowered SVA properties.
#[pyclass(frozen, module = "pysvlang")]
struct Netlist {
    inner: Arc<svlang::Netlist>,
}

#[pymethods]
impl Netlist {
    /// The top module name.
    #[getter]
    fn top(&self) -> String {
        self.inner.top.clone()
    }

    /// Primary inputs.
    #[getter]
    fn inputs(&self) -> Vec<Signal> {
        self.inner.inputs.iter().map(|p| Signal { name: p.name.clone(), width: p.width, signed: p.signed }).collect()
    }

    /// Primary outputs.
    #[getter]
    fn outputs(&self) -> Vec<Signal> {
        self.inner.outputs.iter().map(|p| Signal { name: p.name.clone(), width: p.width, signed: p.signed }).collect()
    }

    /// Flip-flops and latches.
    #[getter]
    fn states(&self) -> Vec<Signal> {
        self.inner.states.iter().map(|s| Signal { name: s.name.clone(), width: s.width, signed: false }).collect()
    }

    /// Number of nodes in the value DAG.
    #[getter]
    fn num_values(&self) -> usize {
        self.inner.values.len()
    }

    /// The concurrent properties of the design as ``(name, kind, compiled)``:
    /// the full hierarchical name, ``"assert"``, ``"assume"``, ``"cover"`` or
    /// ``"restrict"``, and whether ``compile_properties`` made a check for it.
    #[getter]
    fn properties(&self) -> Vec<(String, &'static str, bool)> {
        let nl = &self.inner;
        nl.properties
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let kind = match p.kind {
                    svlang::PropKind::Assert => "assert",
                    svlang::PropKind::Assume => "assume",
                    svlang::PropKind::Cover => "cover",
                    svlang::PropKind::Restrict => "restrict",
                };
                let name = nl.property_names.get(i).cloned().unwrap_or_default();
                (name, kind, nl.property_checks.get(i).copied().flatten().is_some())
            })
            .collect()
    }

    /// The netlist in svlang's readable text form.
    fn dump(&self) -> String {
        self.inner.dump()
    }

    /// The netlist as a Btor2 transition system, for model checkers such as
    /// rIC3, Pono, AVR and BtorMC. Returns ``(text, notes)``; the notes say
    /// what was left out, such as concurrent assertions.
    ///
    /// One step is one clock edge, so the design must have a single clock.
    /// Immediate assertions become ``bad`` lines named by their label and
    /// immediate assumptions become ``constraint`` lines. Raises
    /// ``SvlangError`` when the netlist cannot be modelled exactly.
    ///
    /// With ``trace_outputs``, the model is followed by ``trace_marker`` and
    /// one ``output`` line per named combinational net. ``name_prefix``
    /// prefixes the names of invented inputs and history registers. With
    /// ``covers``, each cover is a ``bad`` line that is reached when the
    /// cover is hit. With ``both_edges``, a design on both edges of its one
    /// clock is written with two model steps per clock cycle, not rejected.
    /// ``clocks`` declares the clocks of a design with several, each as
    /// ``(input, half_period, phase)``: a top-level input, the model steps it
    /// stays at one level, and the steps its wave is moved later.
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (*, trace_outputs = false, trace_marker = None, name_prefix = None, covers = false, both_edges = false, clocks = Vec::new()))]
    fn to_btor2(
        &self,
        py: Python<'_>,
        trace_outputs: bool,
        trace_marker: Option<String>,
        name_prefix: Option<String>,
        covers: bool,
        both_edges: bool,
        clocks: Vec<(String, u32, u32)>,
    ) -> PyResult<(String, Vec<String>)> {
        let mut opts = svlang::btor2::Options::default();
        opts.trace_outputs = trace_outputs;
        opts.covers = covers;
        opts.both_edges = both_edges;
        opts.clocks = clocks
            .into_iter()
            .map(|(input, half_period, phase)| svlang::btor2::Clock { input, half_period, phase })
            .collect();
        if let Some(m) = trace_marker {
            opts.trace_marker = m;
        }
        if let Some(p) = name_prefix {
            opts.name_prefix = p;
        }
        let nl = self.inner.clone();
        let model =
            py.detach(move || svlang::btor2::write(&nl, &opts)).map_err(|e| SvlangError::new_err(e.to_string()))?;
        Ok((model.btor2, model.notes))
    }

    /// The netlist bit-blasted to an and-inverter graph in AIGER 1.9, for
    /// bit-level model checkers and logic tools such as rIC3, ABC and nuXmv.
    /// Returns ``(data, notes)``: ``data`` is ``bytes`` in the binary form
    /// (``.aig``) or, with ``binary=False``, ``str`` in the ASCII form
    /// (``.aag``). Inputs, latches and bad states carry their names in the
    /// symbol table.
    ///
    /// The model is the one ``to_btor2`` writes, with the same ``covers``,
    /// ``both_edges``, ``clocks`` and ``name_prefix`` options: assertions are
    /// bad states and assumptions are constraints.
    /// Memories must be expanded first (``lower_netlist(blast_memories_up_to=N)``).
    /// Raises ``SvlangError`` when the netlist cannot be modelled exactly.
    #[pyo3(signature = (*, binary = true, name_prefix = None, covers = false, both_edges = false, clocks = Vec::new()))]
    fn to_aiger(
        &self,
        py: Python<'_>,
        binary: bool,
        name_prefix: Option<String>,
        covers: bool,
        both_edges: bool,
        clocks: Vec<(String, u32, u32)>,
    ) -> PyResult<(Py<PyAny>, Vec<String>)> {
        let mut opts = svlang::aiger::Options::default();
        opts.covers = covers;
        opts.both_edges = both_edges;
        opts.clocks = clocks
            .into_iter()
            .map(|(input, half_period, phase)| svlang::aiger::Clock { input, half_period, phase })
            .collect();
        if let Some(p) = name_prefix {
            opts.name_prefix = p;
        }
        let nl = self.inner.clone();
        let model =
            py.detach(move || svlang::aiger::lower(&nl, &opts)).map_err(|e| SvlangError::new_err(e.to_string()))?;
        let data = if binary {
            PyBytes::new(py, &model.aig.to_binary()).into_any().unbind()
        } else {
            PyString::new(py, &model.aig.to_ascii()).into_any().unbind()
        };
        Ok((data, model.notes))
    }

    /// The netlist as a Yosys JSON netlist (``yosys -p 'read_json top.json'``),
    /// built from Yosys's word-level cells, for synthesis, equivalence
    /// checking, drawing and SymbiYosys. Returns ``(text, notes)``.
    ///
    /// Registers, latches, asynchronous resets, ``$past``, free values and
    /// checks all have their Yosys cell. ``checks=False`` leaves the
    /// ``$assert``/``$assume``/``$cover`` cells out and ``src_attributes=False``
    /// the source locations. Memories must be expanded first
    /// (``lower_netlist(blast_memories_up_to=N)``). Raises ``SvlangError`` when
    /// something has no exact Yosys form.
    #[pyo3(signature = (*, checks = true, src_attributes = true))]
    fn to_yosys_json(&self, py: Python<'_>, checks: bool, src_attributes: bool) -> PyResult<(String, Vec<String>)> {
        let mut opts = svlang::yosys::Options::default();
        opts.checks = checks;
        opts.src_attributes = src_attributes;
        let nl = self.inner.clone();
        let model =
            py.detach(move || svlang::yosys::write(&nl, &opts)).map_err(|e| SvlangError::new_err(e.to_string()))?;
        Ok((model.json, model.notes))
    }

    fn __repr__(&self) -> String {
        let n = &self.inner;
        format!(
            "Netlist({:?}, inputs={}, outputs={}, states={}, values={})",
            n.top,
            n.inputs.len(),
            n.outputs.len(),
            n.states.len(),
            n.values.len()
        )
    }
}

// ---------------------------------------------------------------------------

#[pymodule]
fn _pysvlang(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("SYNTAX_KINDS", kinds::SYNTAX_KINDS.to_vec())?;
    m.add("TOKEN_KINDS", kinds::TOKEN_KINDS.to_vec())?;
    m.add("SvlangError", m.py().get_type::<SvlangError>())?;
    m.add("LowerError", m.py().get_type::<LowerError>())?;
    m.add_class::<Compilation>()?;
    m.add_class::<SyntaxTree>()?;
    m.add_class::<SyntaxNode>()?;
    m.add_class::<Token>()?;
    m.add_class::<SyntaxError>()?;
    m.add_class::<Diagnostic>()?;
    m.add_class::<Location>()?;
    m.add_class::<Netlist>()?;
    m.add_class::<NetlistDiagnostic>()?;
    m.add_class::<Signal>()?;
    Ok(())
}
