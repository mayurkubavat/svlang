//! Type checks (builder phase N2): what IEEE 1800 rejects about how a
//! declared object is used, found by resolving names in the scopes.
//!
//! - a bit-select of a `real`, or a `real` index (6.12);
//! - an edge (`posedge`, `negedge`, `edge`) of a `real` (6.12);
//! - a default value on a packed struct member (7.2.2);
//! - a procedural assignment to a net (10.3, 14.3);
//! - an assignment used as an expression without parentheses (11.3.6);
//! - `return` with a value in a void function (13.4.1);
//! - `fork ... join` or `join_any` in a function (13.4.4);
//! - `return` inside a fork (9.3.3);
//! - an enum with x/z values on a two-state base, a name after an x/z
//!   one with no value, or a sized value of the wrong width (6.19);
//! - an integer assigned to an enum variable, or an enum variable in a
//!   compound assignment (6.19.3, 6.19.4);
//! - a parameter whose value uses a specparam (6.20.5);
//! - a `randc` variable in `solve ... before`, a soft constraint or a
//!   distribution (18.5.4, 18.5.10, 18.5.14);
//! - an assignment, `++` / `--` or a system subroutine's output argument
//!   that writes to a literal, an operator result, a call or a parameter
//!   (11.4.1, 21.3);
//! - a variable, net or port in a constant expression: parameter values,
//!   dimensions, generate conditions and loop headers, elaboration task
//!   arguments, ANSI port and unpacked struct member defaults (11.2.1).
//!
//! A name the checks cannot resolve for sure (inherited, imported,
//! hierarchical) never makes an error.

use crate::fx::FxMap as HashMap;

use sv_syntax::{Child, NodeId, SyntaxKind, Tree};
use SyntaxKind::*;

use crate::names::Diag;
use crate::scopes::{Scopes, SymKind, ROOT, UNIT};

fn toks(t: &Tree, n: NodeId) -> Vec<&str> {
    t.node(n)
        .children
        .iter()
        .filter_map(|c| match c {
            Child::Token(i) => Some(&*t.tokens[*i as usize].text),
            Child::Node(_) => None,
        })
        .collect()
}

fn first_tok(t: &Tree, n: NodeId) -> Option<&str> {
    t.token_bounds(n).map(|(f, _)| &*t.tokens[f as usize].text)
}

fn name_of(t: &Tree, n: NodeId) -> String {
    t.child(n, Name).map(|c| t.bare_text(c)).unwrap_or_default()
}

const ASSIGN_OPS: &[&str] = &["=", "<=", "+=", "-=", "*=", "/=", "%=", "&=", "|=", "^=", "<<=", ">>=", "<<<=", ">>>="];
/// System subroutines that write an argument: the first one written and
/// whether every later one is written too.
fn output_args(name: &str) -> Option<(usize, bool)> {
    Some(match name {
        "$swrite" | "$swriteb" | "$swriteh" | "$swriteo" | "$sformat" | "$random" | "$fgets" | "$fread" | "$cast" => {
            (0, false)
        }
        n if n.starts_with("$dist_") => (0, false),
        "$ferror" | "$value$plusargs" | "$readmemb" | "$readmemh" => (1, false),
        "$fscanf" | "$sscanf" => (2, true),
        _ => return None,
    })
}

#[doc(hidden)] // shared with the typed checks in sv-semantic; not part of the API
pub fn is_assign_op(s: &str) -> bool {
    ASSIGN_OPS.contains(&s)
}

const REAL_TYPES: &[&str] = &["real", "shortreal", "realtime"];
const PROCEDURAL: &[&str] = &["initial", "always", "always_ff", "always_comb", "always_latch", "final"];

/// What a name resolved to: its kind and declared data type.
#[derive(Clone, Copy)]
struct Decl {
    kind: SymKind,
    tree: usize,
    dtype: Option<NodeId>,
    /// Declared with unpacked dimensions (an array of the type).
    array: bool,
}

struct Ctx<'a> {
    trees: &'a [Tree],
    scopes: &'a Scopes,
    /// Values by (scope, name).
    values: HashMap<(usize, &'a str), Vec<usize>>,
    /// Scope opened by each declaration node.
    opener: HashMap<(usize, NodeId), usize>,
    /// Typedefs by name, anywhere: whether every one is an enum / a real.
    enums: HashMap<String, bool>,
    reals: HashMap<String, bool>,
    /// Class declarations by name.
    classes: HashMap<String, Vec<(usize, NodeId)>>,
    /// Typeless `interconnect` net/port names by `(tree, module_or_iface_node, name)`.
    interconnects: crate::fx::FxSet<(usize, NodeId, String)>,
    /// Checker declaration names.
    checkers: crate::fx::FxSet<String>,
    /// Checker instance names by `(tree, container_node, inst_name)`.
    checker_insts: crate::fx::FxSet<(usize, NodeId, String)>,
    /// Program declaration names.
    programs: crate::fx::FxSet<String>,
    /// Program instance names by `(tree, container_node, inst_name)`.
    program_insts: crate::fx::FxSet<(usize, NodeId, String)>,
    /// Module / interface declaration names.
    design_defs: crate::fx::FxSet<String>,
    /// UDP (`primitive`) declaration names.
    udps: crate::fx::FxSet<String>,
    /// User-defined `nettype` names.
    nettypes: crate::fx::FxSet<String>,
    /// Declared net/var user-defined nettype by `(tree, container_node, name)`: `Some(udnt)` or `None` (built-in net).
    net_udnt: HashMap<(usize, NodeId, String), Option<String>>,
    /// Functions that have at least one `output`, `inout`, or non-`const` `ref` port.
    funcs_with_out_args: crate::fx::FxSet<String>,
    /// Whether any `global clocking` exists in the compilation.
    has_global_clocking: bool,
    /// Clocking `input`-only signals by `(tree, container_node, cb_name, sig_name)`.
    clocking_inputs: crate::fx::FxSet<(usize, NodeId, String, String)>,
}

/// Runs the type checks over every tree of a compile, resolving names in
/// `scopes`, and returns the errors found in tree and source order.
pub fn check(trees: &[Tree], scopes: &Scopes) -> Vec<Diag> {
    let mut values: HashMap<(usize, &str), Vec<usize>> = HashMap::default();
    for (i, s) in scopes.symbols.iter().enumerate() {
        if matches!(
            s.kind,
            SymKind::Net
                | SymKind::Variable
                | SymKind::Parameter
                | SymKind::Argument
                | SymKind::EnumValue
                | SymKind::Specparam
                | SymKind::Genvar
        ) {
            values.entry((s.scope, s.name.as_str())).or_default().push(i);
        }
    }
    let mut opener = HashMap::default();
    for (i, sc) in scopes.scopes.iter().enumerate() {
        if let Some(y) = sc.symbol {
            let y = &scopes.symbols[y];
            opener.insert((y.tree, y.node), i);
        }
    }
    let mut classes: HashMap<String, Vec<(usize, NodeId)>> = HashMap::default();
    let mut enums: HashMap<String, bool> = HashMap::default();
    let mut reals: HashMap<String, bool> = HashMap::default();
    let mut interconnects: crate::fx::FxSet<(usize, NodeId, String)> = Default::default();
    let mut checkers: crate::fx::FxSet<String> = Default::default();
    let mut programs: crate::fx::FxSet<String> = Default::default();
    let mut design_defs: crate::fx::FxSet<String> = Default::default();
    let mut udps: crate::fx::FxSet<String> = Default::default();
    let mut nettypes: crate::fx::FxSet<String> = Default::default();
    let mut funcs_with_out_args: crate::fx::FxSet<String> = Default::default();
    let mut has_global_clocking = false;
    for t in trees {
        for n in t.preorder() {
            match t.kind(n) {
                CheckerDecl => {
                    let nm = name_of(t, n);
                    if !nm.is_empty() {
                        checkers.insert(nm);
                    }
                }
                ProgramDecl => {
                    let nm = name_of(t, n);
                    if !nm.is_empty() {
                        programs.insert(nm);
                    }
                }
                ModuleDecl | InterfaceDecl => {
                    let nm = name_of(t, n);
                    if !nm.is_empty() {
                        design_defs.insert(nm);
                    }
                }
                UdpDecl => {
                    let nm = name_of(t, n);
                    if !nm.is_empty() {
                        udps.insert(nm);
                    }
                }
                NetTypeDecl => {
                    let nm = name_of(t, n);
                    if !nm.is_empty() {
                        nettypes.insert(nm);
                    }
                }
                FunctionDecl => {
                    let fnm = name_of(t, n);
                    if !fnm.is_empty() {
                        if let Some(pl) =
                            t.child(n, PortList).or_else(|| t.child(n, Header).and_then(|h| t.child(h, PortList)))
                        {
                            let mut cur_out = false;
                            for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
                                let own = toks(t, p);
                                if let Some(&d) =
                                    own.iter().find(|&&x| matches!(x, "input" | "output" | "inout" | "ref"))
                                {
                                    cur_out = d == "output" || d == "inout" || (d == "ref" && !own.contains(&"const"));
                                }
                                if cur_out {
                                    funcs_with_out_args.insert(fnm);
                                    break;
                                }
                            }
                        }
                    }
                }
                ClockingDecl if toks(t, n).contains(&"global") => {
                    has_global_clocking = true;
                }
                _ => {}
            }
        }
    }
    let mut checker_insts: crate::fx::FxSet<(usize, NodeId, String)> = Default::default();
    let mut program_insts: crate::fx::FxSet<(usize, NodeId, String)> = Default::default();
    let mut net_udnt: HashMap<(usize, NodeId, String), Option<String>> = HashMap::default();
    let mut clocking_inputs: crate::fx::FxSet<(usize, NodeId, String, String)> = Default::default();
    for (ix, t) in trees.iter().enumerate() {
        let parent = parents(t);
        let container_of = |start: NodeId| -> NodeId {
            let mut cur = up(&parent, start);
            while let Some(anc) = cur {
                if matches!(t.kind(anc), ModuleDecl | InterfaceDecl | ProgramDecl | CheckerDecl | PackageDecl) {
                    return anc;
                }
                cur = up(&parent, anc);
            }
            t.root
        };
        for n in t.preorder() {
            if t.kind(n) == ClassDecl {
                classes.entry(name_of(t, n)).or_default().push((ix, n));
            }
            if t.kind(n) == Instantiation {
                if let Some(dt) = t.child(n, DataType) {
                    let target: String = t
                        .token_range(dt)
                        .into_iter()
                        .map(|i| &*t.tokens[i as usize].text)
                        .take_while(|&s| s != "#")
                        .collect();
                    let target = target.trim();
                    let anc = container_of(n);
                    if checkers.contains(target) {
                        for inst in t.children(n).filter(|&c| t.kind(c) == Instance) {
                            let inm = name_of(t, inst);
                            if !inm.is_empty() {
                                checker_insts.insert((ix, anc, inm));
                            }
                        }
                    } else if programs.contains(target) {
                        for inst in t.children(n).filter(|&c| t.kind(c) == Instance) {
                            let inm = name_of(t, inst);
                            if !inm.is_empty() {
                                program_insts.insert((ix, anc, inm));
                            }
                        }
                    }
                }
            }
            if t.kind(n) == DataDecl {
                let own = toks(t, n);
                let anc = container_of(n);
                let dt_name = t.child(n, DataType).map(|dt| t.bare_text(dt));
                let is_builtin_net = own.iter().any(|&k| {
                    matches!(
                        k,
                        "wire"
                            | "tri"
                            | "wand"
                            | "wor"
                            | "triand"
                            | "trior"
                            | "tri0"
                            | "tri1"
                            | "trireg"
                            | "uwire"
                            | "supply0"
                            | "supply1"
                    )
                });
                if let Some(ref dtn) = dt_name {
                    if nettypes.contains(dtn.trim()) {
                        for decl in t.children(n).filter(|&c| t.kind(c) == Declarator) {
                            let vnm = name_of(t, decl);
                            if !vnm.is_empty() {
                                net_udnt.insert((ix, anc, vnm), Some(dtn.trim().to_string()));
                            }
                        }
                    }
                }
                if is_builtin_net {
                    for decl in t.children(n).filter(|&c| t.kind(c) == Declarator) {
                        let vnm = name_of(t, decl);
                        if !vnm.is_empty() {
                            net_udnt.entry((ix, anc, vnm)).or_insert(None);
                        }
                    }
                }
            }
            if t.kind(n) == ClockingDecl {
                let cb_nm = name_of(t, n);
                if !cb_nm.is_empty() {
                    let anc = container_of(n);
                    for ci in t.children(n).filter(|&c| t.kind(c) == ClockingItem) {
                        let c_own = toks(t, ci);
                        if c_own.contains(&"input") && !c_own.contains(&"output") && !c_own.contains(&"inout") {
                            for decl in t.children(ci).filter(|&c| t.kind(c) == Declarator) {
                                let snm = name_of(t, decl);
                                if !snm.is_empty() {
                                    clocking_inputs.insert((ix, anc, cb_nm.clone(), snm));
                                }
                            }
                        }
                    }
                }
            }
            if matches!(t.kind(n), DataDecl | Port) && toks(t, n).contains(&"interconnect") {
                let mut cur = up(&parent, n);
                while let Some(anc) = cur {
                    if matches!(t.kind(anc), ModuleDecl | InterfaceDecl) {
                        for d in t.children(n).filter(|&c| t.kind(c) == Declarator) {
                            let nm = name_of(t, d);
                            if !nm.is_empty() {
                                interconnects.insert((ix, anc, nm));
                            }
                        }
                        break;
                    }
                    cur = up(&parent, anc);
                }
            }
            if t.kind(n) != TypedefDecl {
                continue;
            }
            let name = name_of(t, n);
            let dt = t.child(n, DataType);
            let first = dt.and_then(|d| first_tok(t, d)).unwrap_or("");
            let dims = t.child(n, Dim).is_some();
            *enums.entry(name.clone()).or_insert(true) &= first == "enum" && !dims;
            *reals.entry(name).or_insert(true) &= REAL_TYPES.contains(&first) && !dims;
        }
    }
    let cx = Ctx {
        trees,
        scopes,
        values,
        opener,
        enums,
        reals,
        classes,
        interconnects,
        checkers,
        checker_insts,
        programs,
        program_insts,
        design_defs,
        udps,
        nettypes,
        net_udnt,
        funcs_with_out_args,
        has_global_clocking,
        clocking_inputs,
    };
    let mut out = Vec::new();
    cx.check_dpi(&mut out);
    for (ix, t) in trees.iter().enumerate() {
        let parent = parents(t);
        for n in t.preorder() {
            cx.node(ix, t, n, &parent, &mut out);
        }
    }
    out
}

/// Width of an integral type written with literal bounds only.
fn literal_width(t: &Tree, dt: NodeId) -> Option<u64> {
    let tk = toks(t, dt);
    let mut w: u64 = match tk.first().copied()? {
        "int" | "integer" => 32,
        "shortint" => 16,
        "longint" => 64,
        "byte" => 8,
        "bit" | "logic" | "reg" => 1,
        _ => return None,
    };
    for d in t.children(dt) {
        if t.kind(d) != Dim {
            return None;
        }
        let (a, b) = literal_range(t, d)?;
        w = w.checked_mul(a.abs_diff(b) + 1)?;
    }
    Some(w)
}

/// `[a:b]` or `[n]` with literal numbers.
fn literal_range(t: &Tree, d: NodeId) -> Option<(i64, i64)> {
    let tk = toks(t, d);
    let nums: Vec<i64> = t
        .children(d)
        .map(|c| (t.kind(c) == Literal).then(|| t.bare_text(c).parse::<i64>().ok()).flatten())
        .collect::<Option<_>>()?;
    match (nums.as_slice(), tk.contains(&":")) {
        ([a, b], true) => Some((*a, *b)),
        ([n], false) if *n > 0 => Some((0, n - 1)),
        _ => None,
    }
}

/// `'{a, b, c}` for an unpacked array with another number of elements
/// (10.10, 5.10: no C-like flattening).
fn array_pattern(t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
    let Some(d) = up(parent, n) else { return };
    if t.kind(d) != Declarator {
        return;
    }
    let Some(dim) = t.children(d).find(|&c| t.kind(c) == Dim) else { return };
    let Some((a, b)) = literal_range(t, dim) else { return };
    let own = toks(t, n);
    if own.contains(&":") || own.len() != t.children(n).count() + 1 {
        // Keyed, `default:`, or a replication `'{n{...}}`.
        return;
    }
    let count = t.children(n).count() as u64;
    let want = a.abs_diff(b) + 1;
    if count != want {
        err(n, format!("array '{}' has {want} elements, not {count}", name_of(t, d)));
    }
}

fn parents(t: &Tree) -> Vec<u32> {
    let mut parent = vec![u32::MAX; t.nodes.len()];
    for n in t.preorder() {
        for c in t.children(n) {
            parent[c.0 as usize] = n.0;
        }
    }
    parent
}

fn up(parent: &[u32], n: NodeId) -> Option<NodeId> {
    let p = parent[n.0 as usize];
    (p != u32::MAX).then_some(NodeId(p))
}

impl Ctx<'_> {
    /// A streaming concatenation assigned to a narrower integral target (11.4.14).
    fn stream_into(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let Some(p) = up(parent, n) else { return };
        let target = match t.kind(p) {
            Declarator if t.children(p).last() == Some(n) => {
                if t.child(p, Dim).is_some() {
                    return;
                }
                let Some(dd) = up(parent, p) else { return };
                t.child(dd, DataType).and_then(|dt| literal_width(t, dt))
            }
            BinaryExpr if toks(t, p).first() == Some(&"=") && t.children(p).nth(1) == Some(n) => {
                let Some(lhs) = t.children(p).next() else { return };
                self.name_decl(ix, t, lhs, parent)
                    .filter(|d| !d.array)
                    .and_then(|d| d.dtype.and_then(|dt| literal_width(&self.trees[d.tree], dt)))
            }
            _ => None,
        };
        let Some(target) = target else { return };
        // The operands: in the inner `{...}`, after any slice size.
        let mut width: u64 = 0;
        let kids: Vec<NodeId> = t.children(n).collect();
        let ops: Vec<NodeId> = {
            // Children after the second `{`: the node list holds the slice
            // size (if any) first.
            let mut braces = 0;
            let mut out = Vec::new();
            for c in &t.node(n).children {
                match c {
                    Child::Token(i) if &*t.tokens[*i as usize].text == "{" => braces += 1,
                    Child::Node(x) if braces >= 2 => out.push(*x),
                    _ => {}
                }
            }
            out
        };
        let _ = kids;
        for o in ops {
            let w = match t.kind(o) {
                NameExpr => self
                    .name_decl(ix, t, o, parent)
                    .filter(|d| !d.array)
                    .and_then(|d| d.dtype.and_then(|dt| literal_width(&self.trees[d.tree], dt))),
                Literal => {
                    let s = t.bare_text(o);
                    s.split_once('\'').and_then(|(sz, _)| sz.trim().parse::<u64>().ok()).filter(|&w| w > 0)
                }
                _ => None,
            };
            let Some(w) = w else { return };
            width += w;
        }
        if width > target {
            err(n, format!("stream of {width} bits is wider than its {target}-bit target"));
        }
    }

    /// The declaration a simple name at `at` refers to.
    fn resolve(&self, ix: usize, t: &Tree, at: NodeId, name: &str, parent: &[u32]) -> Option<Decl> {
        let mut cur = Some(at);
        while let Some(a) = cur {
            if let Some(&s) = self.opener.get(&(ix, a)) {
                return self.in_scopes(ix, s, name);
            }
            if a == t.root {
                return self.in_scopes(ix, UNIT, name);
            }
            // A block's own declarations, or a loop's.
            if !matches!(t.kind(a), ModuleDecl | InterfaceDecl | ProgramDecl | PackageDecl | ClassDecl) {
                for d in t.children(a).filter(|&c| t.kind(c) == DataDecl) {
                    if let Some(x) = t.children(d).find(|&x| t.kind(x) == Declarator && name_of(t, x) == name) {
                        let array = t.child(x, Dim).is_some();
                        return Some(Decl { kind: SymKind::Variable, tree: ix, dtype: t.child(d, DataType), array });
                    }
                }
            }
            cur = up(parent, a);
        }
        None
    }

    fn own(&self, ix: usize, s: usize, name: &str) -> Option<Decl> {
        let ids = self.values.get(&(s, name))?;
        let i = *ids.iter().find(|&&i| s != UNIT || self.scopes.symbols[i].tree == ix)?;
        let y = &self.scopes.symbols[i];
        let t = &self.trees[y.tree];
        let array = y.item.is_some_and(|it| t.kind(it) == Declarator && t.child(it, Dim).is_some());
        Some(Decl { kind: y.kind, tree: y.tree, dtype: y.dtype, array })
    }

    fn in_scopes(&self, ix: usize, s: usize, name: &str) -> Option<Decl> {
        let mut cur = Some(s);
        while let Some(c) = cur {
            if c == ROOT {
                break;
            }
            if let Some(d) = self.own(ix, c, name) {
                return Some(d);
            }
            // A class with a base: the name may be inherited.
            if let Some(y) = self.scopes.scopes[c].symbol {
                let y = &self.scopes.symbols[y];
                if y.kind == SymKind::Class {
                    let t = &self.trees[y.tree];
                    if crate::names::extends(t, y.node).is_some() {
                        return None;
                    }
                }
            }
            cur = self.scopes.scopes[c].parent;
            // Design units and packages see `$unit` of their own file.
            if cur == Some(ROOT) {
                cur = Some(UNIT);
            } else if c == UNIT {
                break;
            }
        }
        None
    }

    fn is_real(&self, d: &Decl) -> bool {
        let t = &self.trees[d.tree];
        let Some(dt) = d.dtype else { return false };
        if d.array {
            return false;
        }
        let text = t.bare_text(dt);
        REAL_TYPES.contains(&text.as_str()) || self.reals.get(&text).copied().unwrap_or(false)
    }

    fn is_enum(&self, d: &Decl) -> bool {
        let t = &self.trees[d.tree];
        let Some(dt) = d.dtype else { return false };
        if first_tok(t, dt) == Some("enum") {
            return true;
        }
        let one = t.token_bounds(dt).is_some_and(|(a, b)| a == b);
        one && self.enums.get(&t.bare_text(dt)).copied().unwrap_or(false)
    }

    fn name_decl(&self, ix: usize, t: &Tree, e: NodeId, parent: &[u32]) -> Option<Decl> {
        if t.kind(e) != NameExpr || t.token_bounds(e).is_none_or_multi() {
            return None;
        }
        self.resolve(ix, t, e, &t.bare_text(e), parent)
    }

    fn node(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], out: &mut Vec<Diag>) {
        let mut err = |node: NodeId, message: String| out.push(Diag { tree: ix, node, message });
        match t.kind(n) {
            SelectExpr => {
                let kids: Vec<NodeId> = t.children(n).collect();
                if let Some(&base) = kids.first() {
                    if self.name_decl(ix, t, base, parent).is_some_and(|d| self.is_real(&d)) {
                        err(n, format!("cannot select bits of real '{}'", t.bare_text(base)));
                    }
                }
                for &i in kids.iter().skip(1) {
                    if self.name_decl(ix, t, i, parent).is_some_and(|d| self.is_real(&d)) {
                        err(i, format!("index '{}' is real", t.bare_text(i)));
                    }
                }
            }
            EventExpr => {
                let tk = toks(t, n);
                if tk.iter().any(|x| matches!(*x, "posedge" | "negedge" | "edge")) {
                    for e in t.children(n) {
                        if self.name_decl(ix, t, e, parent).is_some_and(|d| self.is_real(&d)) {
                            err(e, format!("edge of real '{}'", t.bare_text(e)));
                        }
                    }
                }
            }
            DataType
                if matches!(toks(t, n).first(), Some(&"struct" | &"union"))
                    && (toks(t, n).contains(&"packed")
                        || (toks(t, n).first() == Some(&"union") && toks(t, n).contains(&"soft"))) =>
            {
                let tk = toks(t, n);
                let is_union = tk.first() == Some(&"union");
                let check_equal_widths = is_union && !tk.contains(&"tagged") && !tk.contains(&"soft");
                let mut first_w: Option<u64> = None;
                let mut reported_mismatch = false;
                let kind_word = if is_union { "union" } else { "struct" };
                for m in t.children(n).filter(|&c| t.kind(c) == DataDecl) {
                    let mw = if check_equal_widths {
                        t.child(m, DataType).and_then(|dt| literal_width(t, dt))
                    } else {
                        None
                    };
                    for d in t.children(m).filter(|&c| t.kind(c) == Declarator) {
                        if toks(t, d).contains(&"=") {
                            err(d, format!("packed {kind_word} member '{}' cannot have a default", name_of(t, d)));
                        }
                        if check_equal_widths && !reported_mismatch && t.child(d, Dim).is_none() {
                            if let Some(w) = mw {
                                match first_w {
                                    None => first_w = Some(w),
                                    Some(fw) if fw != w => {
                                        err(
                                            d,
                                            format!(
                                                "member '{}' of width {w} does not match packed union width {fw}",
                                                name_of(t, d)
                                            ),
                                        );
                                        reported_mismatch = true;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
            DataType if first_tok(t, n) == Some("enum") => self.enum_decl(t, n, &mut err),
            ExprStmt => self.assignment(ix, t, n, parent, &mut err),
            BinaryExpr | PrefixExpr | PostfixExpr | CallExpr => self.lvalues(ix, t, n, parent, &mut err),
            Dim | ElabTask | IfGenerate | CaseGenerate | LoopGenerate => {
                self.constant_context(ix, t, n, parent, &mut err)
            }
            ConcatExpr if matches!(toks(t, n).get(1).copied(), Some("<<" | ">>")) => {
                self.stream_into(ix, t, n, parent, &mut err)
            }
            AssignPattern => array_pattern(t, n, parent, &mut err),
            Declarator => {
                self.constant_context(ix, t, n, parent, &mut err);
                // `e v = 1;`
                let Some(p) = up(parent, n) else { return };
                if t.kind(p) != DataDecl {
                    return;
                }
                let d = Decl { kind: SymKind::Variable, tree: ix, dtype: t.child(p, DataType), array: false };
                if self.is_enum(&d) {
                    if let Some(v) = t.children(n).find(|&c| t.kind(c) == Literal) {
                        err(v, format!("cannot assign an integer to enum '{}' without a cast", name_of(t, n)));
                    }
                }
            }
            ReturnStmt => {
                // In a fork, or with a value in a void function.
                let mut cur = up(parent, n);
                while let Some(a) = cur {
                    match t.kind(a) {
                        ParBlock => {
                            err(n, "return inside a fork".into());
                            return;
                        }
                        FunctionDecl => {
                            let void = t.child(a, ReturnType).and_then(|r| first_tok(t, r)) == Some("void");
                            if void && t.children(n).next().is_some() {
                                err(n, "a void function cannot return a value".into());
                            }
                            return;
                        }
                        TaskDecl | ProceduralBlock | ClassDecl | ModuleDecl => return,
                        _ => {}
                    }
                    cur = up(parent, a);
                }
            }
            ParBlock => {
                let last = t.token_bounds(n).map(|(_, l)| &*t.tokens[l as usize].text);
                if last == Some("join_none") {
                    return;
                }
                let mut cur = up(parent, n);
                while let Some(a) = cur {
                    match t.kind(a) {
                        FunctionDecl => {
                            err(n, "only fork ... join_none is allowed in a function".into());
                            return;
                        }
                        TaskDecl | ProceduralBlock | ClassDecl | ModuleDecl => return,
                        _ => {}
                    }
                    cur = up(parent, a);
                }
            }
            ParamDecl => {
                if toks(t, n).first() == Some(&"defparam") {
                    let kids: Vec<NodeId> = t.children(n).filter(|&k| t.kind(k) != Attribute).collect();
                    for pair in kids.chunks_exact(2) {
                        let target = pair[0];
                        if let Some(dc) = self.name_decl(ix, t, target, parent) {
                            if dc.kind != SymKind::Parameter {
                                err(target, format!("defparam target '{}' is not a parameter", t.bare_text(target)));
                            }
                        }
                    }
                    return;
                }
                self.constant_context(ix, t, n, parent, &mut err);
                for d in t.children(n).filter(|&c| t.kind(c) == Declarator) {
                    let mut stack: Vec<NodeId> = t.children(d).filter(|&c| t.kind(c) != Name).collect();
                    while let Some(e) = stack.pop() {
                        if let Some(dc) = self.name_decl(ix, t, e, parent) {
                            if dc.kind == SymKind::Specparam {
                                err(
                                    e,
                                    format!("parameter '{}' cannot use specparam '{}'", name_of(t, d), t.bare_text(e)),
                                );
                            }
                        }
                        stack.extend(t.children(e));
                    }
                }
            }
            FunctionDecl | TaskDecl | MethodPrototype => {
                const NET_KWS: &[&str] = &[
                    "wire", "tri", "wand", "wor", "triand", "trior", "tri0", "tri1", "trireg", "uwire", "supply0",
                    "supply1",
                ];
                if let Some(pl) = t.child(n, PortList) {
                    for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
                        if toks(t, p).iter().any(|k| NET_KWS.contains(k)) {
                            err(p, "subroutine port cannot have a net type".into());
                        }
                    }
                }
                if let Some(body) = t.child(n, Body) {
                    for d in t.children(body).filter(|&c| t.kind(c) == DataDecl) {
                        let own = toks(t, d);
                        if own.first().is_some_and(|k| matches!(*k, "input" | "output" | "inout" | "ref"))
                            && own.iter().any(|k| NET_KWS.contains(k))
                        {
                            err(d, "subroutine port cannot have a net type".into());
                        }
                    }
                }
            }
            CovergroupDecl => {
                if let Some(wc) = t.child(n, WithClause) {
                    let own = toks(t, wc);
                    if let Some(fpos) = own.iter().position(|&k| k == "function") {
                        let fn_name = own.get(fpos + 1).copied().unwrap_or("");
                        if fn_name != "sample" {
                            err(wc, "covergroup 'with function' must name 'sample'".into());
                        }
                        match t.child(wc, PortList) {
                            None => err(wc, "covergroup 'with function sample' requires a port list".into()),
                            Some(pl) => {
                                for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
                                    if toks(t, p).iter().any(|&k| matches!(k, "output" | "inout")) {
                                        err(p, "covergroup sample argument cannot be 'output' or 'inout'".into());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            ClassDecl => {
                self.randc(t, n, &mut err);
                self.pure_constraints(t, n, &mut err);
            }
            NetTypeDecl => self.check_nettype_decl(ix, t, n, parent, &mut err),
            DataDecl | Port if toks(t, n).contains(&"interconnect") => {
                self.check_interconnect_decl(t, n, &mut err);
            }
            NameExpr => {
                self.check_interconnect_use(ix, t, n, parent, &mut err);
                if t.bare_text(n) == "$global_clock"
                    && !self.has_global_clocking
                    && (!self.within(t, n, parent, &[CheckerDecl]) || !self.checker_insts.is_empty())
                {
                    err(n, "'$global_clock' requires a 'global clocking' declaration".into());
                }
            }
            MemberExpr => self.check_hier_member(ix, t, n, parent, &mut err),
            CheckerDecl => {
                self.check_clocking_scope(t, n, &mut err);
                self.check_checker_decl(t, n, parent, &mut err);
            }
            ProgramDecl => {
                self.check_clocking_scope(t, n, &mut err);
                self.check_program_decl(t, n, &mut err);
            }
            ModuleDecl | InterfaceDecl => self.check_clocking_scope(t, n, &mut err),
            GateInst => self.check_gate_udnt(ix, t, n, parent, &mut err),
            Instantiation => {
                self.check_udp_udnt(ix, t, n, parent, &mut err);
                if let Some(pl) = t.child(n, DataType).and_then(|dt| t.child(dt, ParamList)) {
                    let al = t.child(pl, ArgList).unwrap_or(pl);
                    if toks(t, al).contains(&",") {
                        let args: Vec<NodeId> = t.children(al).filter(|&a| t.kind(a) == Arg).collect();
                        if args.is_empty()
                            || args.iter().any(|&a| {
                                toks(t, a).first() != Some(&".")
                                    && t.children(a).all(|c| t.kind(c) == Attribute)
                                    && toks(t, a).is_empty()
                            })
                        {
                            err(pl, "empty positional parameter assignment is not allowed".into());
                        }
                    }
                }
            }
            SpecparamDecl => self.constant_context(ix, t, n, parent, &mut err),
            PathDecl => self.check_specify_path(ix, t, n, parent, &mut err),
            TimingCheck => self.check_timing_check(ix, t, n, parent, &mut err),
            _ => {}
        }
    }

    /// What `n` writes to is assignable: an assignment's left side, the
    /// operand of `++` / `--`, a system subroutine's output argument
    /// (11.4.1, 11.4.2, 21.3).
    fn lvalues(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let op = toks(t, n).first().copied().unwrap_or("");
        let kids: Vec<NodeId> = t.children(n).collect();
        let targets: Vec<NodeId> = match t.kind(n) {
            // `a <= b` is a comparison unless it is the statement itself.
            BinaryExpr if ASSIGN_OPS.contains(&op) => {
                if op == "<=" && !up(parent, n).is_some_and(|p| t.kind(p) == ExprStmt) {
                    return;
                }
                if kids.len() != 2 || self.within(t, n, parent, &[Attribute, AttrSpec]) {
                    return;
                }
                vec![kids[0]]
            }
            PrefixExpr | PostfixExpr if matches!(op, "++" | "--") => kids,
            CallExpr => {
                let Some(&callee) = kids.first() else { return };
                let Some(list) = t.child(n, ArgList) else { return };
                let out = output_args(&t.bare_text(callee));
                if out.is_none() || t.kind(callee) != NameExpr {
                    return;
                }
                let (first, rest) = out.unwrap_or((0, false));
                // Argument slots, by commas; an empty one has no node.
                let mut slots: Vec<Option<NodeId>> = vec![None];
                for c in &t.node(list).children {
                    match c {
                        Child::Token(i) if &*t.tokens[*i as usize].text == "," => slots.push(None),
                        Child::Node(a) => {
                            let a = *a;
                            if let Some(s) = slots.last_mut() {
                                *s = t.children(a).next();
                            }
                        }
                        _ => {}
                    }
                }
                slots
                    .iter()
                    .enumerate()
                    .filter(|&(i, _)| i == first || (rest && i > first))
                    .filter_map(|(_, s)| *s)
                    .collect()
            }
            _ => return,
        };
        for e in targets {
            if self.not_assignable(ix, t, e, parent) {
                err(e, "expression is not assignable".into());
            }
        }
    }

    /// Expressions that must be constant (11.2.1): parameter values,
    /// dimensions, generate conditions and loop bounds, elaboration task
    /// arguments, ANSI port defaults and unpacked struct member defaults.
    /// They cannot refer to variables, nets or ports.
    fn constant_context(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let p = up(parent, n);
        let pk = p.map(|p| t.kind(p));
        let exprs: Vec<NodeId> = match t.kind(n) {
            ParamDecl => {
                if toks(t, n).first() == Some(&"defparam")
                    || self.within(t, n, parent, &[CheckerDecl, SequenceDecl, PropertyDecl, CovergroupDecl])
                {
                    return;
                }
                let mut v = Vec::new();
                for d in t.children(n).filter(|&c| t.kind(c) == Declarator) {
                    v.extend(t.children(d).filter(|&c| !matches!(t.kind(c), Name | Dim)));
                }
                v
            }
            Dim if matches!(pk, Some(Declarator | DataType)) => {
                if self.within(t, n, parent, &[CheckerDecl, SequenceDecl, PropertyDecl, CovergroupDecl, LetDecl])
                    || self.in_expression(t, n, parent)
                {
                    return;
                }
                t.children(n).collect()
            }
            ElabTask => t.children(n).collect(),
            IfGenerate | CaseGenerate => t.children(n).filter(|&c| t.kind(c) != GenerateBlock).take(1).collect(),
            // The header only; the body may be a single item.
            LoopGenerate => t
                .children(n)
                .filter(|&c| t.kind(c) == Literal || format!("{:?}", t.kind(c)).ends_with("Expr"))
                .collect(),
            // ANSI port defaults; unpacked struct member defaults.
            Declarator if toks(t, n).contains(&"=") => {
                let port = pk == Some(Port)
                    && p.and_then(|p| up(parent, p)).and_then(|l| up(parent, l)).is_some_and(|h| {
                        t.kind(h) == Header
                            && up(parent, h)
                                .is_some_and(|d| matches!(t.kind(d), ModuleDecl | InterfaceDecl | ProgramDecl))
                    });
                let member = pk == Some(DataDecl)
                    && p.and_then(|p| up(parent, p)).is_some_and(|d| {
                        t.kind(d) == DataType && matches!(toks(t, d).first(), Some(&"struct" | &"union"))
                    });
                if !port && !member {
                    return;
                }
                t.children(n).filter(|&c| !matches!(t.kind(c), Name | Dim)).collect()
            }
            _ => return,
        };
        // In a subroutine, names resolve by position (a header dimension
        // may use a parameter that a later argument hides); not checked.
        if self.within(t, n, parent, &[FunctionDecl, TaskDecl]) {
            return;
        }
        // Loop variables of enclosing generate loops are constants.
        let mut genvars = Vec::new();
        let mut cur = Some(n);
        while let Some(a) = cur {
            if t.kind(a) == LoopGenerate {
                genvars.extend(t.child(a, Name).map(|nm| t.bare_text(nm)));
            }
            cur = up(parent, a);
        }
        let mut stack = exprs;
        while let Some(e) = stack.pop() {
            match t.kind(e) {
                NameExpr => {
                    let name = t.bare_text(e);
                    if genvars.contains(&name) {
                        continue;
                    }
                    if self
                        .name_decl(ix, t, e, parent)
                        .is_some_and(|d| matches!(d.kind, SymKind::Variable | SymKind::Net | SymKind::Port))
                    {
                        err(
                            e,
                            format!(
                                "reference to non-constant variable '{name}' is not allowed in a constant expression"
                            ),
                        );
                    }
                }
                // `x.m`: a member or a method; not checked.
                MemberExpr | WithClause | AssignPattern | DataType => {}
                CallExpr => {
                    let kids: Vec<NodeId> = t.children(e).collect();
                    let callee = kids.first().map(|&c| t.bare_text(c)).unwrap_or_default();
                    let unevaluated = matches!(
                        callee.as_str(),
                        "$bits"
                            | "$size"
                            | "$left"
                            | "$right"
                            | "$low"
                            | "$high"
                            | "$increment"
                            | "$dimensions"
                            | "$unpacked_dimensions"
                            | "$typename"
                            | "$isunbounded"
                            | "$typeof"
                    );
                    for &k in kids.iter().skip(1) {
                        if t.kind(k) == ArgList {
                            stack.extend(t.children(k).skip(usize::from(unevaluated)));
                        } else {
                            stack.push(k);
                        }
                    }
                }
                _ => stack.extend(t.children(e)),
            }
        }
    }

    /// `n` is inside an expression (a cast's type, `new[n]`), not a
    /// declaration.
    fn in_expression(&self, t: &Tree, n: NodeId, parent: &[u32]) -> bool {
        let mut cur = up(parent, n);
        while let Some(a) = cur {
            match t.kind(a) {
                DataDecl | ParamDecl | TypedefDecl | Port | ModuleDecl | InterfaceDecl | ProgramDecl | PackageDecl
                | ClassDecl => return false,
                k if format!("{k:?}").ends_with("Expr") => return true,
                _ => {}
            }
            cur = up(parent, a);
        }
        false
    }

    fn within(&self, t: &Tree, n: NodeId, parent: &[u32], kinds: &[SyntaxKind]) -> bool {
        let mut cur = up(parent, n);
        while let Some(a) = cur {
            if kinds.contains(&t.kind(a)) {
                return true;
            }
            cur = up(parent, a);
        }
        false
    }

    /// An expression that can never name storage: a literal, an operator
    /// other than an assignment, `?:`, a call, or a parameter, enum value
    /// or specparam.
    fn not_assignable(&self, ix: usize, t: &Tree, e: NodeId, parent: &[u32]) -> bool {
        let op = toks(t, e).first().copied().unwrap_or("");
        match t.kind(e) {
            ParenExpr => t.children(e).next().is_some_and(|c| self.not_assignable(ix, t, c, parent)),
            Literal | CondExpr | CallExpr => true,
            BinaryExpr => !ASSIGN_OPS.contains(&op),
            PrefixExpr => !matches!(op, "++" | "--"),
            NameExpr => self
                .name_decl(ix, t, e, parent)
                .is_some_and(|d| matches!(d.kind, SymKind::Parameter | SymKind::EnumValue | SymKind::Specparam)),
            _ => false,
        }
    }

    fn assignment(&self, ix: usize, t: &Tree, stmt: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let Some(e) = t.children(stmt).next() else { return };
        if t.kind(e) != BinaryExpr {
            return;
        }
        let op = toks(t, e).first().copied().unwrap_or("");
        if !ASSIGN_OPS.contains(&op) {
            return;
        }
        let kids: Vec<NodeId> = t.children(e).collect();
        if kids.len() != 2 {
            return;
        }
        let (lhs, rhs) = (kids[0], kids[1]);
        // `a = b = c`: the inner assignment needs parentheses.
        for side in [lhs, rhs] {
            if t.kind(side) == BinaryExpr && toks(t, side).first().is_some_and(|o| ASSIGN_OPS.contains(o) && *o != "<=")
            {
                err(side, "an assignment inside an expression must be in parentheses".into());
                return;
            }
        }
        // Procedural code only (not `assign`, `force`).
        let mut procedural = false;
        let mut cur = up(parent, stmt);
        while let Some(a) = cur {
            match t.kind(a) {
                ProceduralBlock => {
                    procedural = toks(t, a).first().is_some_and(|k| PROCEDURAL.contains(k));
                    break;
                }
                FunctionDecl | TaskDecl => {
                    procedural = true;
                    break;
                }
                ProcAssign | ContinuousAssign | ClassDecl | ModuleDecl | InterfaceDecl | ProgramDecl => break,
                _ => {}
            }
            cur = up(parent, a);
        }
        let mut target = lhs;
        while t.kind(target) == SelectExpr {
            match t.children(target).next() {
                Some(b) => target = b,
                None => return,
            }
        }
        if procedural && t.kind(target) == MemberExpr {
            let m_own = toks(t, target);
            if m_own.len() == 2 && m_own[0] == "." {
                if let Some(base) = t.children(target).next() {
                    if t.kind(base) == NameExpr {
                        let cb_nm = t.bare_text(base);
                        let mut anc_opt = up(parent, stmt);
                        while let Some(anc) = anc_opt {
                            if matches!(t.kind(anc), ModuleDecl | InterfaceDecl | ProgramDecl | CheckerDecl) {
                                if self.clocking_inputs.contains(&(ix, anc, cb_nm, m_own[1].to_string())) {
                                    err(
                                        lhs,
                                        format!(
                                            "cannot drive clocking input signal '{}.{}'",
                                            t.bare_text(base),
                                            m_own[1]
                                        ),
                                    );
                                    return;
                                }
                                break;
                            }
                            anc_opt = up(parent, anc);
                        }
                    }
                }
            }
        }
        let Some(d) = self.name_decl(ix, t, target, parent) else { return };
        if procedural && d.kind == SymKind::Net {
            err(lhs, format!("cannot assign net '{}' in procedural code", t.bare_text(target)));
            return;
        }
        if target == lhs && d.kind == SymKind::Variable && !d.array && self.is_enum(&d) {
            if op != "=" && op != "<=" {
                err(e, format!("enum '{}' cannot be used in a compound assignment", t.bare_text(lhs)));
            } else if t.kind(rhs) == Literal {
                err(rhs, format!("cannot assign an integer to enum '{}' without a cast", t.bare_text(lhs)));
            }
        }
    }

    fn enum_decl(&self, t: &Tree, n: NodeId, err: &mut impl FnMut(NodeId, String)) {
        let base = t.child(n, DataType);
        let base_kw = base.and_then(|b| first_tok(t, b)).unwrap_or("int");
        let two_state = matches!(base_kw, "int" | "bit" | "byte" | "shortint" | "longint");
        // The base width when it is plain: keyword times literal ranges.
        let mut width: Option<u64> = match base_kw {
            "logic" | "bit" | "reg" => Some(1),
            "byte" => Some(8),
            "shortint" => Some(16),
            "int" | "integer" => Some(32),
            "longint" => Some(64),
            _ => None,
        };
        if let Some(b) = base {
            for d in t.children(b).filter(|&c| t.kind(c) == Dim) {
                let nums: Vec<i64> =
                    t.token_range(d).iter().filter_map(|&i| t.tokens[i as usize].text.parse().ok()).collect();
                let range_toks = t.token_range(d).len();
                width = match (width, nums.as_slice()) {
                    (Some(w), [a, b]) if range_toks == 5 => Some(w * ((a - b).unsigned_abs() + 1)),
                    _ => None,
                };
            }
        }
        let mut prev_xz = false;
        for d in t.children(n).filter(|&c| t.kind(c) == Declarator) {
            let Some(value) = t.children(d).find(|&c| !matches!(t.kind(c), Name | Dim)) else {
                if prev_xz {
                    err(d, format!("enum name '{}' after an x/z value needs a value", name_of(t, d)));
                }
                continue;
            };
            // x/z anywhere in the value (`{32{1'bx}}`).
            let any_xz = t.preorder_of(value).into_iter().filter(|&l| t.kind(l) == Literal).any(|l| {
                let text = t.bare_text(l);
                text.split_once('\'').is_some_and(|(_, d)| {
                    d.trim_start_matches(['s', 'S'])
                        .get(1..)
                        .unwrap_or("")
                        .chars()
                        .any(|c| matches!(c, 'x' | 'X' | 'z' | 'Z' | '?'))
                })
            });
            let lit = (t.kind(value) == Literal).then(|| t.bare_text(value));
            let Some(lit) = lit else {
                if any_xz && two_state {
                    err(d, format!("enum '{}' has an x/z value but a two-state base type", name_of(t, d)));
                }
                prev_xz = any_xz;
                continue;
            };
            let Some((size, digits)) = lit.split_once('\'') else {
                prev_xz = false;
                continue;
            };
            let digits = digits.trim_start_matches(['s', 'S']);
            let body = digits.get(1..).unwrap_or("");
            let xz = body.chars().any(|c| matches!(c, 'x' | 'X' | 'z' | 'Z' | '?'));
            if xz && two_state {
                err(d, format!("enum '{}' has an x/z value but a two-state base type", name_of(t, d)));
            }
            prev_xz = xz;
            if let (Ok(size), Some(w)) = (size.trim().parse::<u64>(), width) {
                if size != w {
                    err(d, format!("enum value '{}' is {size} bits wide, the base type {w}", name_of(t, d)));
                }
            }
        }
    }

    /// A class that is not virtual must implement every pure constraint
    /// it inherits (18.5.2). Only for a chain of plain class names.
    fn pure_constraints(&self, t: &Tree, class: NodeId, err: &mut impl FnMut(NodeId, String)) {
        let tk = toks(t, class);
        if tk.contains(&"virtual") || tk.contains(&"interface") {
            return;
        }
        let constraints = |t: &Tree, c: NodeId| -> Vec<(String, bool)> {
            t.children(c)
                .filter(|&x| matches!(t.kind(x), ConstraintDecl | ConstraintPrototype))
                .map(|x| (name_of(t, x), toks(t, x).contains(&"pure")))
                .collect()
        };
        let mut defined: Vec<String> = constraints(t, class).into_iter().map(|c| c.0).collect();
        let mut pure: Vec<String> = Vec::new();
        let (mut ct, mut cn) = (t, class);
        for _ in 0..32 {
            let Some(base) = self.base_class(ct, cn) else { break };
            let (bt, bn) = base;
            for (name, is_pure) in constraints(bt, bn) {
                if is_pure {
                    pure.push(name);
                } else {
                    defined.push(name);
                }
            }
            ct = bt;
            cn = bn;
        }
        for p in pure {
            if !defined.contains(&p) {
                err(class, format!("class '{}' does not implement pure constraint '{p}'", name_of(t, class)));
            }
        }
    }

    /// The single class a class extends, when named plainly.
    fn base_class(&self, t: &Tree, class: NodeId) -> Option<(&Tree, NodeId)> {
        let ext = crate::names::extends(t, class)?;
        let tk: Vec<&str> = t.token_range(ext).iter().map(|&i| &*t.tokens[i as usize].text).collect();
        let [_, name] = tk.as_slice() else { return None };
        match self.classes.get(*name).map(|v| v.as_slice()) {
            Some([(i, n)]) => Some((&self.trees[*i], *n)),
            _ => None,
        }
    }

    fn randc(&self, t: &Tree, class: NodeId, err: &mut impl FnMut(NodeId, String)) {
        let mut randc = Vec::new();
        for d in t.children(class).filter(|&c| t.kind(c) == DataDecl) {
            if toks(t, d).contains(&"randc") {
                randc.extend(t.children(d).filter(|&x| t.kind(x) == Declarator).map(|x| name_of(t, x)));
            }
        }
        if randc.is_empty() {
            return;
        }
        let is_randc = |s: &str| randc.iter().any(|r| r == s);
        for c in t.children(class).filter(|&c| t.kind(c) == ConstraintDecl) {
            for item in t.preorder_of(c) {
                match t.kind(item) {
                    ConstraintItem => {
                        let tk = toks(t, item);
                        if tk.first() == Some(&"solve") {
                            for x in tk.iter().filter(|x| is_randc(x)) {
                                err(item, format!("randc variable '{x}' cannot be in solve ... before"));
                            }
                        }
                        if tk.first() == Some(&"soft") {
                            for e in t.preorder_of(item) {
                                if t.kind(e) == NameExpr && is_randc(&t.bare_text(e)) {
                                    err(
                                        e,
                                        format!("randc variable '{}' cannot be in a soft constraint", t.bare_text(e)),
                                    );
                                }
                            }
                        }
                    }
                    ExprOrDist if toks(t, item).contains(&"dist") => {
                        if let Some(e) = t.children(item).next() {
                            if t.kind(e) == NameExpr && is_randc(&t.bare_text(e)) {
                                err(e, format!("randc variable '{}' cannot have a distribution", t.bare_text(e)));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// IEEE 1800-2023 §6.6.7: `nettype data_type id [with resolve_func];`.
    fn check_nettype_decl(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let nt_name = name_of(t, n);
        if let Some(dt) = t.child(n, DataType) {
            if let Some(ft) = first_tok(t, dt) {
                if matches!(ft, "string" | "chandle" | "event" | "virtual") {
                    err(dt, format!("invalid data type '{ft}' for nettype '{nt_name}'"));
                    return;
                }
            }
            // Unpacked dimensions on nettype must not be dynamic/queue/associative.
            for d in t.children(dt).filter(|&c| t.kind(c) == Dim) {
                let inner: Vec<&str> = toks(t, d).into_iter().filter(|&x| !matches!(x, "[" | "]")).collect();
                if inner.is_empty() || inner.contains(&"$") || inner == ["*"] || t.child(d, DataType).is_some() {
                    err(d, format!("nettype '{nt_name}' cannot use dynamically-sized array dimensions"));
                    return;
                }
            }
        }
        if toks(t, n).contains(&"with") {
            let Some(fn_node) = t.children(n).find(|&c| !matches!(t.kind(c), Attribute | DataType | Name)) else {
                return;
            };
            if t.kind(fn_node) != NameExpr {
                return;
            }
            let fn_name = t.bare_text(fn_node);
            let fn_clean = fn_name.trim().strip_prefix('\\').unwrap_or(fn_name.trim());
            // Check enclosing scopes for resolution function.
            let mut cur_anc = up(parent, n);
            let mut enclosing_scope = UNIT;
            let mut has_wildcard_import = false;
            while let Some(anc) = cur_anc {
                for imp in t.children(anc).filter(|&c| t.kind(c) == ImportDecl) {
                    if t.token_range(imp).iter().any(|&ti| &*t.tokens[ti as usize].text == "*") {
                        has_wildcard_import = true;
                    }
                }
                if let Some(&sc) = self.opener.get(&(ix, anc)) {
                    enclosing_scope = sc;
                    break;
                }
                cur_anc = up(parent, anc);
            }
            for imp in t.children(t.root).filter(|&c| t.kind(c) == ImportDecl) {
                if t.token_range(imp).iter().any(|&ti| &*t.tokens[ti as usize].text == "*") {
                    has_wildcard_import = true;
                }
            }
            let mut found_sym = None;
            let mut sc_opt = Some(enclosing_scope);
            while let Some(sc) = sc_opt {
                if let Some(sym) = self.scopes.symbols.iter().find(|s| s.scope == sc && s.name == fn_clean) {
                    found_sym = Some(sym);
                    break;
                }
                sc_opt = self.scopes.scopes[sc].parent;
            }
            if found_sym.is_none() {
                found_sym = self.scopes.symbols.iter().find(|s| s.scope == UNIT && s.name == fn_clean);
            }
            match found_sym {
                Some(sym) if sym.kind != SymKind::Function => {
                    err(fn_node, format!("resolution function '{fn_clean}' of nettype '{nt_name}' must be a function"));
                }
                Some(sym) => {
                    let ft = &self.trees[sym.tree];
                    if ft.kind(sym.node) == FunctionDecl {
                        if ft.child(sym.node, ReturnType).and_then(|rt| first_tok(ft, rt)) == Some("void") {
                            err(
                                fn_node,
                                format!("resolution function '{fn_clean}' of nettype '{nt_name}' cannot be void"),
                            );
                        }
                        if let Some(pl) = ft.child(sym.node, Header).and_then(|h| ft.child(h, PortList)) {
                            let ports: Vec<NodeId> = ft.children(pl).filter(|&p| ft.kind(p) == Port).collect();
                            if ports.len() != 1 {
                                err(
                                    fn_node,
                                    format!(
                                        "resolution function '{fn_clean}' of nettype '{nt_name}' must take exactly 1 dynamic-array argument"
                                    ),
                                );
                            } else if let Some(decl_n) = ft.child(ports[0], Declarator) {
                                let dims: Vec<NodeId> = ft.children(decl_n).filter(|&d| ft.kind(d) == Dim).collect();
                                let has_dyn = dims.iter().any(|&d| {
                                    toks(ft, d).into_iter().filter(|&x| !matches!(x, "[" | "]")).count() == 0
                                });
                                if !has_dyn {
                                    err(
                                        fn_node,
                                        format!(
                                            "argument of resolution function '{fn_clean}' for nettype '{nt_name}' must be a 1-D dynamic array 'T []'"
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                None if !has_wildcard_import => {
                    let any_global_fn =
                        self.scopes.symbols.iter().any(|s| s.kind == SymKind::Function && s.name == fn_clean);
                    if !any_global_fn {
                        err(fn_node, format!("undeclared resolution function '{fn_clean}' in nettype '{nt_name}'"));
                    }
                }
                None => {}
            }
        }
    }

    /// IEEE 1800-2023 §6.6.8: `interconnect` nets/ports cannot declare an explicit data type or `var`.
    fn check_interconnect_decl(&self, t: &Tree, n: NodeId, err: &mut impl FnMut(NodeId, String)) {
        let tk = toks(t, n);
        if tk.contains(&"var") {
            err(n, "interconnect net or port cannot be declared 'var'".into());
            return;
        }
        if let Some(dt) = t.child(n, DataType) {
            let dt_own = toks(t, dt);
            let only_signing = dt_own.iter().all(|&x| matches!(x, "signed" | "unsigned"));
            let has_named_or_kw_type =
                !only_signing || t.children(dt).any(|c| t.kind(c) != Dim && t.kind(c) != Attribute);
            if has_named_or_kw_type {
                err(dt, "interconnect net or port cannot have an explicit data type".into());
            }
        }
    }

    /// IEEE 1800-2023 §6.6.8: `interconnect` nets may only appear in port connections and `alias` statements.
    fn check_interconnect_use(
        &self,
        ix: usize,
        t: &Tree,
        n: NodeId,
        parent: &[u32],
        err: &mut impl FnMut(NodeId, String),
    ) {
        if self.interconnects.is_empty() {
            return;
        }
        if let Some(p) = up(parent, n) {
            if matches!(t.kind(p), MemberExpr | ScopedExpr) && t.children(p).next() != Some(n) {
                return;
            }
        }
        let raw = t.bare_text(n);
        let ident = raw.trim().strip_prefix('\\').unwrap_or(raw.trim());
        let mut cur = up(parent, n);
        let mut in_expr_stmt_or_assign = false;
        let mut mod_anc = None;
        while let Some(anc) = cur {
            match t.kind(anc) {
                Instantiation | GateInst | NetAlias | BindDirective | SpecifyBlock | Attribute => return,
                ContinuousAssign | ProceduralBlock => in_expr_stmt_or_assign = true,
                ModuleDecl | InterfaceDecl => {
                    mod_anc = Some(anc);
                    break;
                }
                FunctionDecl | TaskDecl => return,
                _ => {}
            }
            cur = up(parent, anc);
        }
        if in_expr_stmt_or_assign {
            if let Some(ma) = mod_anc {
                if self.interconnects.contains(&(ix, ma, ident.to_string())) {
                    err(
                        n,
                        format!("interconnect net '{ident}' cannot be used in continuous or procedural expressions"),
                    );
                }
            }
        }
    }

    /// IEEE 1800-2023 §30.4: parallel module path `(src => dst)` requires equal bit widths.
    fn check_specify_path(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let Some(ps) = t.child(n, PathSpec) else { return };
        let ps_toks = toks(t, ps);
        let is_parallel = ps_toks.contains(&"=>")
            || (ps_toks.contains(&"+=") && ps_toks.contains(&">"))
            || (ps_toks.contains(&"-=") && ps_toks.contains(&">"));
        if !is_parallel {
            return;
        }
        // Extract terminals before `=>` and after `=>` (when simple 1-to-1 parallel path).
        let mut before = Vec::new();
        let mut after = Vec::new();
        let mut seen_arrow = false;
        for ch in &t.node(ps).children {
            match *ch {
                Child::Token(i) => {
                    if matches!(&*t.tokens[i as usize].text, "=>" | "*>" | ">") {
                        seen_arrow = true;
                    }
                }
                Child::Node(c) if t.kind(c) != Attribute => {
                    if seen_arrow {
                        after.push(c);
                    } else {
                        before.push(c);
                    }
                }
                _ => {}
            }
        }
        if let ([src], [dst]) = (before.as_slice(), after.as_slice()) {
            let term_width = |nd: NodeId| -> Option<u64> {
                if t.kind(nd) == NameExpr {
                    let d = self.name_decl(ix, t, nd, parent).filter(|d| !d.array)?;
                    let Some(dt) = d.dtype else { return Some(1) };
                    let dt_tree = &self.trees[d.tree];
                    if let Some(w) = literal_width(dt_tree, dt) {
                        return Some(w);
                    }
                    // Implicit type with packed dimensions (`input wire [3:0] a`).
                    let own = toks(dt_tree, dt);
                    if own.iter().all(|&x| matches!(x, "signed" | "unsigned")) {
                        let mut w = 1u64;
                        for dm in dt_tree.children(dt) {
                            if dt_tree.kind(dm) != Dim {
                                return None;
                            }
                            let (a, b) = literal_range(dt_tree, dm)?;
                            w = w.checked_mul(a.abs_diff(b) + 1)?;
                        }
                        return Some(w);
                    }
                    None
                } else {
                    None
                }
            };
            if let (Some(sw), Some(dw)) = (term_width(*src), term_width(*dst)) {
                if sw != dw {
                    err(ps, format!("parallel specify path '=>' requires matching terminal widths ({sw} vs {dw})"));
                }
            }
        }
    }

    /// IEEE 1800-2023 Clause 31: timing check notifier must be a variable (not a net or parameter).
    fn check_timing_check(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let Some(chk_name) = first_tok(t, n) else { return };
        let notifier_idx = match chk_name {
            "$period" => Some(2usize),
            "$setup" | "$hold" | "$recovery" | "$removal" | "$width" | "$skew" => Some(3usize),
            "$timeskew" => Some(3usize),
            "$setuphold" | "$recrem" | "$fullskew" => Some(5usize),
            _ => None,
        };
        let Some(nidx) = notifier_idx else { return };
        let args: Vec<NodeId> = t.children(n).filter(|&c| t.kind(c) == TimingCheckArg).collect();
        let Some(&narg) = args.get(nidx) else { return };
        let Some(ne) = t.children(narg).find(|&c| t.kind(c) != Attribute) else { return };
        if t.kind(ne) == NameExpr {
            if let Some(d) = self.name_decl(ix, t, ne, parent) {
                if matches!(d.kind, SymKind::Net | SymKind::Parameter | SymKind::Specparam) {
                    err(ne, format!("timing check notifier '{}' in {chk_name} must be a variable", t.bare_text(ne)));
                }
            }
        }
    }

    /// IEEE 1800-2023 §14.14: at most one `global clocking` per module/interface/program/checker.
    fn check_clocking_scope(&self, t: &Tree, scope_node: NodeId, err: &mut impl FnMut(NodeId, String)) {
        let mut seen = false;
        for cd in t.children(scope_node).filter(|&c| t.kind(c) == ClockingDecl) {
            if toks(t, cd).contains(&"global") {
                if seen {
                    err(cd, "duplicate 'global clocking' declaration in the same scope".into());
                }
                seen = true;
            }
        }
    }

    /// IEEE 1800-2023 §17.2 & §24.3: disallow hierarchical references into checkers, and disallow
    /// references from design elements (`module` / `interface` / `package`) into `program` items.
    fn check_hier_member(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let m_own = toks(t, n);
        if m_own.len() != 2 || m_own[0] != "." {
            return;
        }
        let Some(base) = t.children(n).next() else { return };
        if t.kind(base) != NameExpr || t.token_bounds(base).is_none_or_multi() {
            return;
        }
        let bname = t.bare_text(base);
        let mut anc_opt = up(parent, n);
        let mut container = t.root;
        let mut in_design_elem = false;
        let mut in_checker = None;
        while let Some(anc) = anc_opt {
            match t.kind(anc) {
                CheckerDecl if in_checker.is_none() => in_checker = Some(anc),
                ModuleDecl | InterfaceDecl | PackageDecl => {
                    container = anc;
                    in_design_elem = true;
                    break;
                }
                ProgramDecl => {
                    container = anc;
                    break;
                }
                _ => {}
            }
            anc_opt = up(parent, anc);
        }
        let same_checker = in_checker.is_some_and(|chk| name_of(t, chk) == bname);
        if !same_checker
            && (self.checkers.contains(&bname)
                || self.checker_insts.contains(&(ix, container, bname.clone()))
                || in_checker.is_some_and(|chk| self.checker_insts.contains(&(ix, chk, bname.clone()))))
            && self.name_decl(ix, t, base, parent).is_none()
        {
            err(n, format!("hierarchical lookup into checker '{bname}' is not allowed"));
            return;
        }
        if in_design_elem
            && in_checker.is_none()
            && (self.programs.contains(&bname) || self.program_insts.contains(&(ix, container, bname.clone())))
            && self.name_decl(ix, t, base, parent).is_none()
        {
            err(n, format!("design element cannot reference program '{bname}'"));
        }
    }

    /// IEEE 1800-2023 Clause 17 checker semantic restrictions.
    fn check_checker_decl(&self, t: &Tree, chk: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        let mut local_vars: crate::fx::FxSet<String> = Default::default();
        let mut local_cgs: crate::fx::FxSet<String> = Default::default();
        for c in t.children(chk) {
            match t.kind(c) {
                NetTypeDecl => {
                    err(c, "user-defined nettype declaration is not allowed in a checker".into());
                }
                DataDecl => {
                    if let Some(dt) = t.child(c, DataType) {
                        let dtn = t.bare_text(dt);
                        if self.nettypes.contains(dtn.trim()) {
                            err(c, format!("user-defined nettype '{}' is not allowed in a checker", dtn.trim()));
                        }
                    }
                    let is_cg = t.child(c, DataType).is_some_and(|dt| local_cgs.contains(t.bare_text(dt).trim()));
                    for d in t.children(c).filter(|&x| t.kind(x) == Declarator) {
                        let vnm = name_of(t, d);
                        if !vnm.is_empty() {
                            if is_cg {
                                local_cgs.insert(vnm);
                            } else {
                                local_vars.insert(vnm);
                            }
                        }
                    }
                }
                CovergroupDecl => {
                    let cnm = name_of(t, c);
                    if !cnm.is_empty() {
                        local_cgs.insert(cnm);
                    }
                }
                _ => {}
            }
        }
        for sub in t.preorder_of(chk) {
            if sub != chk && t.kind(sub) == CheckerDecl {
                continue;
            }
            if t.kind(sub) == ProceduralBlock {
                for x in t.preorder_of(sub) {
                    if t.kind(x) == NameExpr && local_cgs.contains(&t.bare_text(x)) {
                        err(
                            x,
                            format!(
                                "covergroup '{}' cannot be referenced in a checker procedural block",
                                t.bare_text(x)
                            ),
                        );
                    }
                }
            }
            if t.kind(sub) == ExprStmt {
                if let Some(be) = t.children(sub).next().filter(|&e| t.kind(e) == BinaryExpr) {
                    let op = toks(t, be).first().copied().unwrap_or("");
                    let kids: Vec<NodeId> = t.children(be).collect();
                    if ASSIGN_OPS.contains(&op)
                        && kids.len() == 2
                        && t.kind(kids[0]) == NameExpr
                        && local_vars.contains(&t.bare_text(kids[0]))
                    {
                        for rx in t.preorder_of(kids[1]) {
                            if t.kind(rx) == CallExpr {
                                if let Some(callee) = t.children(rx).next().filter(|&k| t.kind(k) == NameExpr) {
                                    let fnm = t.bare_text(callee);
                                    if self.funcs_with_out_args.contains(&fnm) {
                                        err(
                                            rx,
                                            format!(
                                                "function '{fnm}' with output/inout/ref argument cannot be called in checker variable assignment"
                                            ),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        let _ = parent;
    }

    /// IEEE 1800-2023 Clause 24 program semantic restrictions.
    fn check_program_decl(&self, t: &Tree, prog: NodeId, err: &mut impl FnMut(NodeId, String)) {
        for c in t.preorder_of(prog) {
            if c == prog {
                continue;
            }
            match t.kind(c) {
                ProceduralBlock => {
                    if let Some(ft) = first_tok(t, c) {
                        if ft.starts_with("always") {
                            err(c, format!("'{ft}' procedure is not allowed inside a program"));
                        }
                    }
                }
                ModuleDecl | InterfaceDecl | ProgramDecl => {
                    err(c, "nested module, interface, or program declaration is not allowed inside a program".into());
                }
                Instantiation => {
                    if let Some(dt) = t.child(c, DataType) {
                        let target: String = t
                            .token_range(dt)
                            .into_iter()
                            .map(|i| &*t.tokens[i as usize].text)
                            .take_while(|&s| s != "#")
                            .collect();
                        if self.design_defs.contains(target.trim()) {
                            err(c, format!("cannot instantiate design element '{}' inside a program", target.trim()));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// IEEE 1800-2023 Clause 28 (`GateUDNTConn` & `BiDiSwitchNetTypes`).
    fn check_gate_udnt(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        if self.nettypes.is_empty() {
            return;
        }
        let Some(gw) = first_tok(t, n) else { return };
        let mut anc_opt = up(parent, n);
        let mut container = t.root;
        while let Some(anc) = anc_opt {
            if matches!(t.kind(anc), ModuleDecl | InterfaceDecl | ProgramDecl) {
                container = anc;
                break;
            }
            anc_opt = up(parent, anc);
        }
        let is_bidi = matches!(gw, "tran" | "tranif0" | "tranif1");
        for inst in t.children(n).filter(|&c| t.kind(c) == Instance) {
            let Some(al) = t.child(inst, ArgList) else { continue };
            let args: Vec<NodeId> = t
                .children(al)
                .filter(|&a| t.kind(a) == Arg)
                .filter_map(|a| t.children(a).find(|&x| t.kind(x) != Attribute))
                .collect();
            if is_bidi {
                if args.len() >= 2 && t.kind(args[0]) == NameExpr && t.kind(args[1]) == NameExpr {
                    let n0 = t.bare_text(args[0]);
                    let n1 = t.bare_text(args[1]);
                    if let (Some(u0), Some(u1)) =
                        (self.net_udnt.get(&(ix, container, n0)), self.net_udnt.get(&(ix, container, n1)))
                    {
                        if u0 != u1 {
                            err(inst, format!("bidirectional switch '{gw}' terminals must have matching nettypes"));
                        }
                    }
                }
            } else {
                for a in args {
                    if t.kind(a) == NameExpr {
                        if let Some(Some(udnt)) = self.net_udnt.get(&(ix, container, t.bare_text(a))) {
                            err(a, format!("gate '{gw}' terminal cannot connect to user-defined nettype '{udnt}'"));
                        }
                    }
                }
            }
        }
    }

    fn check_udp_udnt(&self, ix: usize, t: &Tree, n: NodeId, parent: &[u32], err: &mut impl FnMut(NodeId, String)) {
        if self.nettypes.is_empty() || self.udps.is_empty() {
            return;
        }
        let Some(dt) = t.child(n, DataType) else { return };
        let target = t.bare_text(dt);
        if !self.udps.contains(target.trim()) {
            return;
        }
        let mut anc_opt = up(parent, n);
        let mut container = t.root;
        while let Some(anc) = anc_opt {
            if matches!(t.kind(anc), ModuleDecl | InterfaceDecl | ProgramDecl) {
                container = anc;
                break;
            }
            anc_opt = up(parent, anc);
        }
        for inst in t.children(n).filter(|&c| t.kind(c) == Instance) {
            let Some(al) = t.child(inst, ArgList) else { continue };
            for arg in t.children(al).filter(|&a| t.kind(a) == Arg) {
                if let Some(a) = t.children(arg).find(|&x| t.kind(x) != Attribute) {
                    if t.kind(a) == NameExpr {
                        if let Some(Some(udnt)) = self.net_udnt.get(&(ix, container, t.bare_text(a))) {
                            err(
                                a,
                                format!(
                                    "UDP '{}' terminal cannot connect to user-defined nettype '{udnt}'",
                                    target.trim()
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    /// IEEE 1800-2023 Clause 35 DPI-C import/export and C-identifier signature validation.
    #[allow(clippy::too_many_lines, clippy::type_complexity)]
    fn check_dpi(&self, out: &mut Vec<Diag>) {
        let valid_c_id = |raw: &str| -> bool {
            let s = raw.trim().strip_prefix('\\').unwrap_or(raw.trim()).trim();
            !s.is_empty()
                && s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        };
        let sub_dpi_sig = |t: &Tree, n: NodeId, is_task: bool| -> Option<(bool, String, Vec<(String, String)>)> {
            let ret = if is_task {
                "void".to_string()
            } else if let Some(rt) = t.child(n, ReturnType) {
                let rtoks: Vec<&str> = t.token_range(rt).into_iter().map(|i| &*t.tokens[i as usize].text).collect();
                rtoks.join(" ")
            } else {
                "logic".to_string()
            };
            let mut args = Vec::new();
            if let Some(pl) = t.child(n, PortList).or_else(|| t.child(n, Header).and_then(|h| t.child(h, PortList))) {
                let mut dir = "input".to_string();
                let mut dty = "logic".to_string();
                for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
                    let own = toks(t, p);
                    let exp_dir = own.iter().find(|&&x| matches!(x, "input" | "output" | "inout" | "ref")).copied();
                    let dt = t.child(p, DataType);
                    match (exp_dir, dt) {
                        (_, Some(d)) => {
                            let dtoks: Vec<&str> =
                                t.token_range(d).into_iter().map(|i| &*t.tokens[i as usize].text).collect();
                            dty = dtoks.join(" ");
                        }
                        (Some(_), None) => dty = "logic".to_string(),
                        (None, None) => {}
                    }
                    if let Some(ed) = exp_dir {
                        dir = ed.to_string();
                    }
                    // If `dty` contains a user typedef identifier rather than built-in keywords, skip strict sig comparison.
                    let first_w = dty.split([' ', '[']).next().unwrap_or("");
                    if !matches!(
                        first_w,
                        "void"
                            | "bit"
                            | "logic"
                            | "reg"
                            | "byte"
                            | "shortint"
                            | "int"
                            | "longint"
                            | "integer"
                            | "real"
                            | "shortreal"
                            | "chandle"
                            | "string"
                    ) {
                        return None;
                    }
                    args.push((dir.clone(), dty.clone()));
                }
            }
            let first_r = ret.split([' ', '[']).next().unwrap_or("");
            if !matches!(
                first_r,
                "void"
                    | "bit"
                    | "logic"
                    | "reg"
                    | "byte"
                    | "shortint"
                    | "int"
                    | "longint"
                    | "real"
                    | "shortreal"
                    | "chandle"
                    | "string"
            ) {
                return None;
            }
            Some((is_task, ret, args))
        };
        let check_sub_types = |ix: usize,
                               t: &Tree,
                               err_node: NodeId,
                               sub_node: NodeId,
                               is_task: bool,
                               is_pure: bool,
                               out: &mut Vec<Diag>| {
            if !is_task {
                if let Some(rt) = t.child(sub_node, ReturnType) {
                    let rtoks: Vec<&str> = t.token_range(rt).into_iter().map(|i| &*t.tokens[i as usize].text).collect();
                    if is_pure && rtoks.as_slice() == ["void"] {
                        out.push(Diag {
                            tree: ix,
                            node: err_node,
                            message: "'pure' DPI function cannot have 'void' return type".into(),
                        });
                    }
                    if let Some(&first_rt) = rtoks.first() {
                        if matches!(first_rt, "integer" | "time" | "realtime" | "event")
                            || (is_pure && first_rt == "chandle")
                        {
                            out.push(Diag {
                                tree: ix,
                                node: err_node,
                                message: format!("invalid DPI function return type '{first_rt}'"),
                            });
                        }
                    }
                }
            }
            if let Some(pl) =
                t.child(sub_node, PortList).or_else(|| t.child(sub_node, Header).and_then(|h| t.child(h, PortList)))
            {
                let mut dir = "input";
                let mut dt_first = "logic";
                for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
                    let own = toks(t, p);
                    let exp_dir = own.iter().find(|&&x| matches!(x, "input" | "output" | "inout" | "ref")).copied();
                    let dt = t.child(p, DataType);
                    match (exp_dir, dt) {
                        (_, Some(d)) => dt_first = first_tok(t, d).unwrap_or("logic"),
                        (Some(_), None) => dt_first = "logic",
                        (None, None) => {}
                    }
                    if let Some(ed) = exp_dir {
                        dir = ed;
                    }
                    if dir == "ref" {
                        out.push(Diag {
                            tree: ix,
                            node: err_node,
                            message: "'ref' argument is not allowed on a DPI subroutine".into(),
                        });
                    } else if is_pure && matches!(dir, "output" | "inout") {
                        out.push(Diag {
                            tree: ix,
                            node: err_node,
                            message: format!("'pure' DPI function cannot have '{dir}' argument"),
                        });
                    }
                    if dt_first == "event" || (is_pure && dt_first == "chandle") {
                        out.push(Diag {
                            tree: ix,
                            node: err_node,
                            message: format!("invalid DPI argument type '{dt_first}'"),
                        });
                    }
                }
            }
        };

        let mut c_sigs: HashMap<String, (bool, String, Vec<(String, String)>)> = HashMap::default();
        let mut exported_c_ids: crate::fx::FxSet<String> = Default::default();
        let mut exported_in_scope: crate::fx::FxSet<(usize, NodeId, String)> = Default::default();

        for (ix, t) in self.trees.iter().enumerate() {
            let parent = parents(t);
            let scope_node_of = |n: NodeId| -> NodeId {
                let mut cur = up(&parent, n);
                while let Some(a) = cur {
                    if matches!(t.kind(a), ModuleDecl | InterfaceDecl | ProgramDecl | PackageDecl | ClassDecl) {
                        return a;
                    }
                    cur = up(&parent, a);
                }
                t.root
            };
            for n in t.preorder() {
                match t.kind(n) {
                    DpiImport => {
                        let own = toks(t, n);
                        let spec = own.iter().find(|s| s.starts_with('"')).copied().unwrap_or("");
                        if spec == "\"DPI\"" {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: "deprecated '\"DPI\"' spec string is not allowed; use '\"DPI-C\"'".into(),
                            });
                        } else if spec != "\"DPI-C\"" {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: format!("invalid DPI spec string {spec}; expected '\"DPI-C\"'"),
                            });
                        }
                        let is_pure = own.contains(&"pure");
                        let is_task = own.contains(&"task");
                        if is_pure && is_task {
                            out.push(Diag { tree: ix, node: n, message: "DPI task cannot be declared 'pure'".into() });
                        }
                        let sv_name = name_of(t, n);
                        let c_id = if let Some(eq_pos) = own.iter().position(|&x| x == "=") {
                            own.get(eq_pos.wrapping_sub(1)).copied().unwrap_or("")
                        } else {
                            sv_name.as_str()
                        };
                        if !c_id.is_empty() && !valid_c_id(c_id) {
                            out.push(Diag { tree: ix, node: n, message: format!("invalid DPI C identifier '{c_id}'") });
                        }
                        check_sub_types(ix, t, n, n, is_task, is_pure, out);
                        let clean_cid = c_id.trim().strip_prefix('\\').unwrap_or(c_id.trim()).trim().to_string();
                        if !clean_cid.is_empty() && valid_c_id(c_id) {
                            if let Some(sig) = sub_dpi_sig(t, n, is_task) {
                                if let Some(prev) = c_sigs.get(&clean_cid) {
                                    if prev != &sig {
                                        out.push(Diag {
                                            tree: ix,
                                            node: n,
                                            message: format!("DPI signature mismatch for C identifier '{clean_cid}'"),
                                        });
                                    }
                                } else {
                                    c_sigs.insert(clean_cid, sig);
                                }
                            }
                        }
                    }
                    DpiExport => {
                        let own = toks(t, n);
                        let spec = own.iter().find(|s| s.starts_with('"')).copied().unwrap_or("");
                        if spec == "\"DPI\"" {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: "deprecated '\"DPI\"' spec string is not allowed; use '\"DPI-C\"'".into(),
                            });
                        } else if spec != "\"DPI-C\"" {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: format!("invalid DPI spec string {spec}; expected '\"DPI-C\"'"),
                            });
                        }
                        let Some(kw_pos) = own.iter().position(|&x| matches!(x, "function" | "task")) else {
                            continue;
                        };
                        let want_task = own[kw_pos] == "task";
                        let sv_raw = own.get(kw_pos + 1).copied().unwrap_or("");
                        if sv_raw.is_empty() || sv_raw == ";" {
                            continue;
                        }
                        let sv_name = sv_raw.trim().strip_prefix('\\').unwrap_or(sv_raw.trim()).trim();
                        let c_raw = if let Some(eq_pos) = own.iter().position(|&x| x == "=") {
                            own.get(eq_pos.wrapping_sub(1)).copied().unwrap_or(sv_raw)
                        } else {
                            sv_raw
                        };
                        if !valid_c_id(c_raw) {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: format!("invalid DPI C identifier '{c_raw}'"),
                            });
                        }
                        let container = scope_node_of(n);
                        let mut found_node: Option<NodeId> = None;
                        for ch in t.children(container) {
                            match t.kind(ch) {
                                FunctionDecl | TaskDecl | DpiImport if name_of(t, ch) == sv_name => {
                                    found_node = Some(ch);
                                    break;
                                }
                                DataDecl | ParamDecl
                                    if t.children(ch).any(|d| t.kind(d) == Declarator && name_of(t, d) == sv_name) =>
                                {
                                    found_node = Some(ch);
                                    break;
                                }
                                _ => {}
                            }
                        }
                        let Some(target) = found_node else {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: format!("DPI export '{sv_name}' must be declared in the same scope"),
                            });
                            continue;
                        };
                        match t.kind(target) {
                            DpiImport => {
                                out.push(Diag {
                                    tree: ix,
                                    node: n,
                                    message: format!("cannot DPI-export imported subroutine '{sv_name}'"),
                                });
                                continue;
                            }
                            FunctionDecl | TaskDecl => {
                                let actual_task = t.kind(target) == TaskDecl;
                                if actual_task != want_task {
                                    out.push(Diag {
                                        tree: ix,
                                        node: n,
                                        message: format!("DPI export kind mismatch for '{sv_name}'"),
                                    });
                                    continue;
                                }
                                if !exported_in_scope.insert((ix, container, sv_name.to_string())) {
                                    out.push(Diag {
                                        tree: ix,
                                        node: n,
                                        message: format!("duplicate DPI export of '{sv_name}'"),
                                    });
                                    continue;
                                }
                                check_sub_types(ix, t, n, target, actual_task, false, out);
                                let clean_cid =
                                    c_raw.trim().strip_prefix('\\').unwrap_or(c_raw.trim()).trim().to_string();
                                if valid_c_id(c_raw) {
                                    if container != t.root {
                                        // Check signature compatibility across compilation, and duplicate C-id within same module/scope.
                                        if let Some(sig) = sub_dpi_sig(t, target, actual_task) {
                                            if let Some(prev) = c_sigs.get(&clean_cid) {
                                                if prev != &sig {
                                                    out.push(Diag {
                                                        tree: ix,
                                                        node: n,
                                                        message: format!(
                                                            "DPI signature mismatch for C identifier '{clean_cid}'"
                                                        ),
                                                    });
                                                }
                                            } else {
                                                c_sigs.insert(clean_cid.clone(), sig);
                                            }
                                        }
                                        if !exported_c_ids.insert(format!("{ix}:{}::{clean_cid}", container.0)) {
                                            out.push(Diag {
                                                tree: ix,
                                                node: n,
                                                message: format!("duplicate DPI export C identifier '{clean_cid}'"),
                                            });
                                        }
                                    } else if let Some(sig) = sub_dpi_sig(t, target, actual_task) {
                                        if let Some(prev) = c_sigs.get(&clean_cid) {
                                            if prev != &sig {
                                                out.push(Diag {
                                                    tree: ix,
                                                    node: n,
                                                    message: format!(
                                                        "DPI signature mismatch for C identifier '{clean_cid}'"
                                                    ),
                                                });
                                            }
                                        } else {
                                            c_sigs.insert(clean_cid, sig);
                                        }
                                    }
                                }
                            }
                            _ => {
                                out.push(Diag {
                                    tree: ix,
                                    node: n,
                                    message: format!("'{sv_name}' is not a subroutine"),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

trait Single {
    fn is_none_or_multi(&self) -> bool;
}

impl Single for Option<(u32, u32)> {
    /// No tokens, or more than one (a dotted or scoped name).
    fn is_none_or_multi(&self) -> bool {
        !self.is_some_and(|(a, b)| a == b)
    }
}

trait PreorderOf {
    fn preorder_of(&self, n: NodeId) -> Vec<NodeId>;
}

impl PreorderOf for Tree {
    fn preorder_of(&self, n: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![n];
        while let Some(x) = stack.pop() {
            out.push(x);
            let kids: Vec<NodeId> = self.children(x).collect();
            stack.extend(kids.into_iter().rev());
        }
        out
    }
}
