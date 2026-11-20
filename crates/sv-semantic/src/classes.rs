//! Class member rules and parameter initializers (IEEE 1800 chapters 6.20
//! and 8), found from the syntax of class declarations:
//!
//! - a parameter declared in a body, or a `localparam` in a parameter
//!   port list, has a value (6.20.1, 6.20.4);
//! - `super` only in a class that extends another (8.15);
//! - a static method or a static property's initializer does not use the
//!   class's non-static properties (8.10, 8.9);
//! - a method defined outside its class returns the type its prototype
//!   declares, and is defined once (8.24);
//! - an override of a virtual method has as many arguments as the
//!   method it overrides, with the same directions, and the same return
//!   type (8.20).
//!
//! Types are compared only when both sides are built-in keyword types,
//! and base classes only when their names are unique in the design, so a
//! legal design never gets an error.

use crate::fx::{FxMap as HashMap, FxSet as HashSet};

use sv_syntax::{Child, NodeId, SyntaxKind, Tree};
use SyntaxKind::*;

use crate::names::Diag;

const BUILTIN: &[&str] = &[
    "void",
    "int",
    "integer",
    "shortint",
    "longint",
    "byte",
    "bit",
    "logic",
    "reg",
    "real",
    "shortreal",
    "realtime",
    "time",
    "string",
    "chandle",
    "event",
];

fn own_toks(t: &Tree, n: NodeId) -> Vec<&str> {
    t.node(n)
        .children
        .iter()
        .filter_map(|c| match c {
            Child::Token(i) => Some(&*t.tokens[*i as usize].text),
            Child::Node(_) => None,
        })
        .collect()
}

fn all_toks(t: &Tree, n: NodeId) -> Vec<&str> {
    t.token_range(n).into_iter().map(|i| &*t.tokens[i as usize].text).collect()
}

fn name_of(t: &Tree, n: NodeId) -> String {
    t.child(n, Name).map(|c| t.bare_text(c)).unwrap_or_default()
}

/// A built-in return type written as one keyword (`int`, `void`), or
/// none.
fn simple_type(t: &Tree, method: NodeId) -> Option<String> {
    let dt = t.child(method, ReturnType).and_then(|r| t.child(r, DataType))?;
    let tk = all_toks(t, dt);
    (tk.len() == 1 && BUILTIN.contains(&tk[0])).then(|| tk[0].to_string())
}

#[derive(Clone)]
struct Arg {
    name: String,
    dir: String,
    has_default: bool,
    is_default_kw: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Vis {
    Public,
    Protected,
    Local,
}

impl Vis {
    fn as_str(self) -> &'static str {
        match self {
            Vis::Public => "public",
            Vis::Protected => "protected",
            Vis::Local => "local",
        }
    }
}

fn vis_of(own: &[&str]) -> Vis {
    if own.contains(&"local") {
        Vis::Local
    } else if own.contains(&"protected") {
        Vis::Protected
    } else {
        Vis::Public
    }
}

/// IEEE 1800-2023 override specifiers (`:initial`, `:extends`, `:final`).
fn override_specs(own: &[&str]) -> (bool, bool, bool) {
    let (mut init, mut ext, mut fin) = (false, false, false);
    for w in own.windows(2) {
        if w[0] == ":" {
            match w[1] {
                "initial" => init = true,
                "extends" => ext = true,
                "final" => fin = true,
                _ => {}
            }
        }
    }
    (init, ext, fin)
}

#[derive(Clone)]
struct Method {
    tree: usize,
    node: NodeId,
    virt: bool,
    vis: Vis,
    specs: (bool, bool, bool),
    /// None: arguments declared in the body (not compared).
    args: Option<Vec<Arg>>,
    ret: Option<String>,
}

#[derive(Clone)]
struct Cstr {
    is_extern: bool,
    specs: (bool, bool, bool),
}

struct Class {
    tree: usize,
    node: NodeId,
    is_final: bool,
    generic: bool,
    base: Option<String>,
    base_parameterized: bool,
    extends_has_parens: bool,
    extends_has_default: bool,
    extends_arg_count: u32,
    methods: HashMap<String, Method>,
    constraints: HashMap<String, Cstr>,
    covergroups: HashSet<String>,
    local_members: HashSet<String>,
}

fn method_args(t: &Tree, m: NodeId) -> Option<Vec<Arg>> {
    let Some(pl) = t.child(m, PortList) else {
        // `task t; input a; ...`: not compared.
        let body = t.child(m, Body);
        let non_ansi = body.is_some_and(|b| {
            t.children(b).any(|c| {
                t.token_bounds(c).is_some_and(|(a, _)| {
                    matches!(&*t.tokens[a as usize].text, "input" | "output" | "inout" | "ref" | "const")
                })
            })
        });
        return (!non_ansi).then(Vec::new);
    };
    let mut dir = "input".to_string();
    let mut out = Vec::new();
    for p in t.children(pl).filter(|&c| t.kind(c) == Port) {
        let own = own_toks(t, p);
        if own.as_slice() == ["default"] {
            out.push(Arg { name: "default".into(), dir: "default".into(), has_default: true, is_default_kw: true });
            continue;
        }
        if let Some(d) = own.iter().find(|x| matches!(**x, "input" | "output" | "inout" | "ref")) {
            dir = if own.contains(&"const") { "const ref".into() } else { d.to_string() };
        }
        let decl = t.child(p, Declarator);
        let name = decl.map(|d| name_of(t, d)).unwrap_or_default();
        let has_default = decl.is_some_and(|d| own_toks(t, d).contains(&"=")) || own.contains(&"=");
        out.push(Arg { name, dir: dir.clone(), has_default, is_default_kw: false });
    }
    Some(out)
}

fn is_virtual(t: &Tree, m: NodeId) -> bool {
    let own = own_toks(t, m);
    own.contains(&"virtual") || own.contains(&"pure")
}

/// Runs this module's checks over every tree of a compile and returns the
/// errors found (class member and parameter initializer errors), in tree and source order.
pub fn check(trees: &[Tree]) -> Vec<Diag> {
    let mut out = Vec::new();
    let mut classes: HashMap<String, Vec<Class>> = HashMap::default();
    let mut class_order: Vec<String> = Vec::new();
    for (ix, t) in trees.iter().enumerate() {
        let mut in_header = HashSet::default();
        for n in t.preorder() {
            if t.kind(n) == ParamList {
                in_header.extend(t.children(n));
            }
        }
        for n in t.preorder() {
            match t.kind(n) {
                ParamDecl => params(ix, t, n, in_header.contains(&n), &mut out),
                ClassDecl => {
                    let cls_own = own_toks(t, n);
                    let (_, _, is_final) = override_specs(&cls_own);
                    let hdr = t.child(n, Header);
                    let generic = hdr.and_then(|h| t.child(h, ParamList)).is_some();
                    let ext_node = hdr.and_then(|h| t.child(h, Extends));
                    let base = ext_node.and_then(|e| {
                        let dt = t.child(e, DataType)?;
                        let tk = all_toks(t, dt);
                        let head = tk.iter().take_while(|&&x| x != "#").last().copied()?;
                        Some(head.to_string())
                    });
                    let base_parameterized =
                        ext_node.and_then(|e| t.child(e, DataType)).is_some_and(|dt| all_toks(t, dt).contains(&"#"));
                    let ext_own = ext_node.map(|e| own_toks(t, e)).unwrap_or_default();
                    let extends_has_default = ext_own.contains(&"default");
                    let ext_al = ext_node.and_then(|e| t.child(e, ArgList));
                    let extends_has_parens = ext_own.contains(&"(") || ext_al.is_some();
                    let extends_arg_count = if extends_has_default {
                        1
                    } else if let Some(al) = ext_al {
                        arg_list_count(t, al)
                    } else {
                        0
                    };
                    let mut methods = HashMap::default();
                    let mut local_members = HashSet::default();
                    for d in t.children(n).filter(|&c| t.kind(c) == DataDecl) {
                        if own_toks(t, d).contains(&"local") {
                            for decl in t.children(d).filter(|&c| t.kind(c) == Declarator) {
                                let nm = name_of(t, decl);
                                if !nm.is_empty() {
                                    local_members.insert(nm);
                                }
                            }
                        }
                    }
                    for m in t.children(n).filter(|&c| matches!(t.kind(c), FunctionDecl | TaskDecl | MethodPrototype)) {
                        let name = name_of(t, m);
                        if name.is_empty() || name.contains("::") {
                            continue;
                        }
                        let m_own = own_toks(t, m);
                        let vis = vis_of(&m_own);
                        if vis == Vis::Local && name != "new" {
                            local_members.insert(name.clone());
                        }
                        methods.insert(
                            name,
                            Method {
                                tree: ix,
                                node: m,
                                virt: is_virtual(t, m),
                                vis,
                                specs: override_specs(&m_own),
                                args: method_args(t, m),
                                ret: simple_type(t, m),
                            },
                        );
                    }
                    let mut constraints = HashMap::default();
                    for c in t.children(n).filter(|&c| matches!(t.kind(c), ConstraintDecl | ConstraintPrototype)) {
                        let c_own = own_toks(t, c);
                        let cname = name_of(t, c);
                        if cname == "new" {
                            out.push(Diag {
                                tree: ix,
                                node: t.child(c, Name).unwrap_or(c),
                                message: "constraint block cannot be named 'new'".into(),
                            });
                            continue;
                        }
                        if cname.is_empty() || cname.contains("::") || cname.contains('.') {
                            continue;
                        }
                        constraints.insert(
                            cname,
                            Cstr {
                                is_extern: t.kind(c) == ConstraintPrototype || c_own.contains(&"extern"),
                                specs: override_specs(&c_own),
                            },
                        );
                    }
                    let mut covergroups = HashSet::default();
                    for cg in t.children(n).filter(|&c| t.kind(c) == CovergroupDecl) {
                        let cg_name = name_of(t, cg);
                        if !cg_name.is_empty() {
                            covergroups.insert(cg_name);
                        }
                    }
                    let cname = name_of(t, n);
                    if !classes.contains_key(&cname) {
                        class_order.push(cname.clone());
                    }
                    classes.entry(cname).or_default().push(Class {
                        tree: ix,
                        node: n,
                        is_final,
                        generic,
                        base,
                        base_parameterized,
                        extends_has_parens,
                        extends_has_default,
                        extends_arg_count,
                        methods,
                        constraints,
                        covergroups,
                        local_members,
                    });
                    statics(ix, t, n, &mut out);
                }
                _ => {}
            }
        }
    }
    let unique = |name: &str| classes.get(name).filter(|v| v.len() == 1).map(|v| &v[0]);
    for (ix, t) in trees.iter().enumerate() {
        supers_and_this(ix, t, trees, &unique, &mut out);
    }
    out_of_block(trees, &unique, &mut out);
    check_hierarchy_and_overrides(trees, &class_order, &classes, &unique, &mut out);
    out
}

fn arg_list_count(t: &Tree, n: NodeId) -> u32 {
    let mut commas = 0;
    let mut any = false;
    for c in &t.node(n).children {
        match c {
            Child::Token(i) if &*t.tokens[*i as usize].text == "," => commas += 1,
            Child::Node(_) => any = true,
            _ => {}
        }
    }
    if commas == 0 && !any {
        0
    } else {
        commas + 1
    }
}

#[allow(clippy::too_many_lines)]
fn check_hierarchy_and_overrides<'a>(
    trees: &[Tree],
    class_order: &[String],
    classes: &'a HashMap<String, Vec<Class>>,
    unique: &dyn Fn(&str) -> Option<&'a Class>,
    out: &mut Vec<Diag>,
) {
    let mut reported_cycle_nodes: HashSet<String> = HashSet::default();
    for cname in class_order {
        let Some(list) = classes.get(cname) else { continue };
        for c in list {
            let ct = &trees[c.tree];
            let cls_at = ct.child(c.node, Name).unwrap_or(c.node);

            // 1. `:final` base class check & circular inheritance check.
            if let Some(ref bname) = c.base {
                if let Some(bc) = unique(bname) {
                    if bc.is_final {
                        out.push(Diag {
                            tree: c.tree,
                            node: cls_at,
                            message: format!("cannot extend ':final' class '{bname}'"),
                        });
                    }
                }
                if !reported_cycle_nodes.contains(cname) {
                    let mut seen = Vec::new();
                    let mut cur = Some(bname.clone());
                    while let Some(b) = cur {
                        if &b == cname {
                            out.push(Diag {
                                tree: c.tree,
                                node: cls_at,
                                message: format!("circular class inheritance involving '{cname}'"),
                            });
                            reported_cycle_nodes.insert(cname.clone());
                            for s in seen {
                                reported_cycle_nodes.insert(s);
                            }
                            break;
                        }
                        if seen.contains(&b) {
                            break;
                        }
                        seen.push(b.clone());
                        cur = unique(&b).and_then(|bc| bc.base.clone());
                    }
                }
            }

            // 2. Constructor `default` forwarding, `super.new`, and `local` constructor visibility.
            check_constructors(trees, cname, c, unique, out);

            // 3. Constraint `:initial` / `:extends` / `:final` override checks.
            for cm in ct.children(c.node).filter(|&x| matches!(ct.kind(x), ConstraintDecl | ConstraintPrototype)) {
                let name = name_of(ct, cm);
                if name.is_empty() || name == "new" || name.contains("::") || name.contains('.') {
                    continue;
                }
                let (is_init, is_ext, _) = override_specs(&own_toks(ct, cm));
                let mut seen = HashSet::default();
                let mut base = c.base.clone();
                let mut any_base = false;
                let mut base_final = false;
                let mut known = true;
                while let Some(b) = base {
                    if !seen.insert(b.clone()) {
                        break;
                    }
                    let Some(bc) = unique(&b) else {
                        known = false;
                        break;
                    };
                    if let Some(bc_cstr) = bc.constraints.get(&name) {
                        any_base = true;
                        base_final |= bc_cstr.specs.2;
                    }
                    base = bc.base.clone();
                }
                let at = ct.child(cm, Name).unwrap_or(cm);
                if base_final {
                    out.push(Diag {
                        tree: c.tree,
                        node: at,
                        message: format!("cannot override ':final' constraint '{name}'"),
                    });
                } else if is_init && any_base {
                    out.push(Diag {
                        tree: c.tree,
                        node: at,
                        message: format!("':initial' constraint '{name}' overrides a superclass constraint"),
                    });
                } else if is_ext && known && !any_base {
                    out.push(Diag {
                        tree: c.tree,
                        node: at,
                        message: format!("':extends' constraint '{name}' does not override a superclass constraint"),
                    });
                }
            }

            // 4. `covergroup extends <name>` check (IEEE 1800-2023 §19.3).
            for cg in ct.children(c.node).filter(|&x| ct.kind(x) == CovergroupDecl) {
                if !own_toks(ct, cg).contains(&"extends") {
                    continue;
                }
                let cg_name = name_of(ct, cg);
                if cg_name.is_empty() {
                    continue;
                }
                let mut seen = HashSet::default();
                let mut base = c.base.clone();
                let mut found_cg = false;
                let mut known = !c.base_parameterized;
                while let Some(b) = base {
                    if !seen.insert(b.clone()) {
                        break;
                    }
                    let Some(bc) = unique(&b) else {
                        known = false;
                        break;
                    };
                    if bc.generic || bc.base_parameterized {
                        known = false;
                        break;
                    }
                    if bc.covergroups.contains(&cg_name) {
                        found_cg = true;
                        break;
                    }
                    base = bc.base.clone();
                }
                if known && !found_cg {
                    out.push(Diag {
                        tree: c.tree,
                        node: ct.child(cg, Name).unwrap_or(cg),
                        message: format!("unknown base covergroup '{cg_name}'"),
                    });
                }
            }

            // 5. Method `:initial` / `:extends` / `:final` specifiers & virtual method overrides.
            for (name, m) in &c.methods {
                if name == "new" {
                    continue;
                }
                let mut seen = HashSet::default();
                let mut base = c.base.clone();
                let mut found: Option<&Method> = None;
                let mut virt = false;
                let mut base_final = false;
                let mut known = true;
                while let Some(b) = base {
                    if !seen.insert(b.clone()) {
                        break;
                    }
                    let Some(bc) = unique(&b) else {
                        known = false;
                        break;
                    };
                    if let Some(bm) = bc.methods.get(name) {
                        found.get_or_insert(bm);
                        virt |= bm.virt;
                        base_final |= bm.specs.2;
                    }
                    base = bc.base.clone();
                }
                let t = &trees[m.tree];
                let at = t.child(m.node, Name).unwrap_or(m.node);
                let (is_init, is_ext, _) = m.specs;
                if base_final {
                    out.push(Diag {
                        tree: m.tree,
                        node: at,
                        message: format!("cannot override ':final' method '{name}'"),
                    });
                    continue;
                }
                if is_init && virt {
                    out.push(Diag {
                        tree: m.tree,
                        node: at,
                        message: format!("':initial' method '{name}' cannot override a virtual superclass method"),
                    });
                }
                if is_ext && known && !virt {
                    out.push(Diag {
                        tree: m.tree,
                        node: at,
                        message: format!("':extends' method '{name}' does not override a virtual superclass method"),
                    });
                }
                let (Some(bm), true, true) = (found, virt, known) else { continue };
                if m.vis != bm.vis {
                    out.push(Diag {
                        tree: m.tree,
                        node: at,
                        message: format!(
                            "visibility '{}' of virtual method '{name}' does not match '{}' in its superclass",
                            m.vis.as_str(),
                            bm.vis.as_str()
                        ),
                    });
                }
                if let (Some(a), Some(b)) = (&m.args, &bm.args) {
                    if a.len() != b.len() {
                        out.push(Diag {
                            tree: m.tree,
                            node: at,
                            message: format!(
                                "virtual method '{name}' has different number of arguments from its superclass method"
                            ),
                        });
                        continue;
                    }
                    for (x, y) in a.iter().zip(b) {
                        if x.dir != y.dir {
                            out.push(Diag {
                                tree: m.tree,
                                node: at,
                                message: format!(
                                    "direction of virtual method argument '{}' differs from the direction given in its superclass method",
                                    x.name
                                ),
                            });
                        }
                    }
                }
                if let (Some(r), Some(br)) = (&m.ret, &bm.ret) {
                    if r != br && t.kind(m.node) == trees[bm.tree].kind(bm.node) {
                        out.push(Diag {
                            tree: m.tree,
                            node: at,
                            message: format!(
                                "return type '{r}' of virtual method '{name}' does not match '{br}' declared in its superclass"
                            ),
                        });
                    }
                }
            }
        }
    }
}

fn check_constructors<'a>(
    trees: &[Tree],
    cname: &str,
    c: &Class,
    unique: &dyn Fn(&str) -> Option<&'a Class>,
    out: &mut Vec<Diag>,
) {
    let ct = &trees[c.tree];
    let cls_at = ct.child(c.node, Name).unwrap_or(c.node);
    let own_new = c.methods.get("new");
    let mut new_has_default = false;
    if let Some(nm) = own_new {
        let mt = &trees[nm.tree];
        let at = mt.child(nm.node, Name).unwrap_or(nm.node);
        if let Some(args) = &nm.args {
            let def_cnt = args.iter().filter(|a| a.is_default_kw).count();
            new_has_default = def_cnt > 0;
            if def_cnt > 1 {
                out.push(Diag {
                    tree: nm.tree,
                    node: at,
                    message: "constructor 'new' can specify at most one 'default' argument".into(),
                });
            }
            if new_has_default && c.base.is_none() {
                out.push(Diag {
                    tree: nm.tree,
                    node: at,
                    message: format!("class '{cname}' does not have a super class for 'new(default)'"),
                });
            }
        }
        if c.extends_has_default && !new_has_default {
            out.push(Diag {
                tree: nm.tree,
                node: at,
                message: "class with 'extends ...(default)' cannot declare 'new' without 'default'".into(),
            });
        }
    }

    let Some(ref bname) = c.base else { return };
    if bname == cname || matches!(bname.as_str(), "semaphore" | "mailbox" | "process" | "weak_reference") {
        return;
    }
    let Some(bc) = unique(bname) else { return };

    // Check `local` constructor visibility on base class (unless derived is nested inside base).
    let nested_in_base = c.tree == bc.tree && {
        let bt = &trees[bc.tree];
        match (bt.token_bounds(bc.node), bt.token_bounds(c.node)) {
            (Some((ba, bb)), Some((ca, cb))) => ba <= ca && cb <= bb,
            _ => false,
        }
    };
    if let Some(bnew) = bc.methods.get("new") {
        if bnew.vis == Vis::Local && !nested_in_base {
            out.push(Diag {
                tree: c.tree,
                node: cls_at,
                message: format!("constructor of base class '{bname}' is 'local'"),
            });
            return;
        }
        // Check `new(default)` referencing `local` members of `bc`.
        if (new_has_default || c.extends_has_default) && !bc.local_members.is_empty() {
            let bt = &trees[bnew.tree];
            if let Some(pl) = bt.child(bnew.node, PortList) {
                for p in bt.children(pl).filter(|&x| bt.kind(x) == Port) {
                    if let Some(decl) = bt.child(p, Declarator) {
                        for init in bt.children(decl).filter(|&x| !matches!(bt.kind(x), Name | Dim)) {
                            let mut stk = vec![init];
                            while let Some(x) = stk.pop() {
                                if matches!(bt.kind(x), NameExpr | CallExpr) {
                                    let ident = if bt.kind(x) == CallExpr {
                                        bt.children(x)
                                            .next()
                                            .filter(|&k| bt.kind(k) == NameExpr)
                                            .map(|k| bt.bare_text(k))
                                    } else {
                                        Some(bt.bare_text(x))
                                    };
                                    if let Some(id) = ident {
                                        if bc.local_members.contains(&id) {
                                            out.push(Diag {
                                                tree: c.tree,
                                                node: own_new.map_or(cls_at, |m| m.node),
                                                message: format!(
                                                    "default constructor argument references 'local' member '{id}' of '{bname}'"
                                                ),
                                            });
                                        }
                                    }
                                }
                                stk.extend(bt.children(x));
                            }
                        }
                    }
                }
            }
        }
    }

    if c.generic || c.base_parameterized || bc.generic || bc.base_parameterized {
        return;
    }

    // Check `super.new` duplicate / missing / arity on non-parameterized classes with inline constructor bodies.
    let calls_super_new = own_new.and_then(|nm| {
        let mt = &trees[nm.tree];
        if mt.kind(nm.node) == MethodPrototype {
            return None;
        }
        let mut found = false;
        let mut stack = vec![nm.node];
        while let Some(x) = stack.pop() {
            if mt.kind(x) == CallExpr {
                if let Some(callee) = mt.children(x).next() {
                    if mt.kind(callee) == MemberExpr
                        && own_toks(mt, callee) == [".", "new"]
                        && mt
                            .children(callee)
                            .next()
                            .is_some_and(|b| mt.kind(b) == NameExpr && mt.bare_text(b) == "super")
                    {
                        found = true;
                        if let Some(al) = mt.child(x, ArgList) {
                            let has_def_arg =
                                mt.children(al).any(|a| mt.kind(a) == Arg && own_toks(mt, a).as_slice() == ["default"]);
                            if has_def_arg && !new_has_default {
                                out.push(Diag {
                                    tree: nm.tree,
                                    node: x,
                                    message: "'super.new(default)' requires 'default' in constructor port list".into(),
                                });
                            }
                        }
                    }
                }
            }
            if x != nm.node && matches!(mt.kind(x), ClassDecl | FunctionDecl | TaskDecl) {
                continue;
            }
            stack.extend(mt.children(x));
        }
        Some(found)
    });

    if c.extends_has_parens {
        if calls_super_new == Some(true) {
            if let Some(nm) = own_new {
                out.push(Diag {
                    tree: nm.tree,
                    node: nm.node,
                    message: "duplicate base constructor call: base arguments already given in 'extends'".into(),
                });
            }
        }
        if !c.extends_has_default {
            let max_base_args =
                match bc.methods.get("new") {
                    Some(bnew) => bnew.args.as_ref().map(|a| {
                        if a.iter().any(|x| x.is_default_kw) {
                            u32::MAX
                        } else {
                            a.len() as u32
                        }
                    }),
                    None => (!bc.extends_has_default).then_some(0),
                };
            if let Some(max_a) = max_base_args {
                if c.extends_arg_count > max_a {
                    out.push(Diag {
                        tree: c.tree,
                        node: cls_at,
                        message: format!("too many arguments to base class '{bname}' constructor"),
                    });
                }
            }
        }
    } else if let Some(bnew) = bc.methods.get("new") {
        if let Some(bargs) = &bnew.args {
            let required = bargs.iter().filter(|a| !a.has_default && !a.is_default_kw).count();
            if required > 0 && !new_has_default {
                match (own_new, calls_super_new) {
                    (None, _) => {
                        out.push(Diag {
                            tree: c.tree,
                            node: cls_at,
                            message: format!("base class '{bname}' constructor requires {required} argument(s)"),
                        });
                    }
                    (Some(nm), Some(false)) => {
                        out.push(Diag {
                            tree: nm.tree,
                            node: nm.node,
                            message: format!("constructor must call 'super.new' for base class '{bname}'"),
                        });
                    }
                    _ => {}
                }
            }
        }
    }
}

/// A body parameter, or a header `localparam`, without a value.
fn params(ix: usize, t: &Tree, n: NodeId, header: bool, out: &mut Vec<Diag>) {
    let own = own_toks(t, n);
    if own.first() == Some(&"defparam") {
        return;
    }
    if header && own.first() != Some(&"localparam") {
        return;
    }
    let decls: Vec<NodeId> = t.children(n).filter(|&c| t.kind(c) == Declarator).collect();
    for d in decls {
        if !own_toks(t, d).contains(&"=") {
            let message = if header {
                "local parameter is missing an initializer"
            } else {
                "parameter declaration is missing an initializer"
            };
            out.push(Diag { tree: ix, node: d, message: message.into() });
        }
    }
    // `localparam type T` in a header: the name is a direct child.
    if header && own.contains(&"type") && !own.contains(&"=") {
        if let Some(nm) = t.child(n, Name) {
            out.push(Diag { tree: ix, node: nm, message: "local parameter is missing an initializer".into() });
        }
    }
}

/// `super` and `this` handle validity checks (IEEE 1800 §8.11, §8.15).
fn supers_and_this<'a>(
    ix: usize,
    t: &Tree,
    trees: &[Tree],
    unique: &dyn Fn(&str) -> Option<&'a Class>,
    out: &mut Vec<Diag>,
) {
    let mut parent = vec![u32::MAX; t.nodes.len()];
    for n in t.preorder() {
        for c in t.children(n) {
            parent[c.0 as usize] = n.0;
        }
    }
    let up = |n: NodeId| (parent[n.0 as usize] != u32::MAX).then(|| NodeId(parent[n.0 as usize]));
    let in_dtype = |mut cur: Option<NodeId>| {
        while let Some(a) = cur {
            if t.kind(a) == DataType {
                return true;
            }
            cur = up(a);
        }
        false
    };
    for n in t.preorder() {
        match t.kind(n) {
            MemberExpr | ScopedExpr => {
                let own = own_toks(t, n);
                let followed_by_member =
                    up(n).is_some_and(|p| t.kind(p) == MemberExpr && t.children(p).next() == Some(n));
                if own.len() == 2 && own[1] == "super" && !followed_by_member {
                    out.push(Diag { tree: ix, node: n, message: "'super' must be followed by '.<member>'".into() });
                }
            }
            NameExpr if t.bare_text(n) == "super" => {
                let is_member_base = up(n).is_some_and(|p| t.kind(p) == MemberExpr && t.children(p).next() == Some(n));
                if !is_member_base {
                    out.push(Diag {
                        tree: ix,
                        node: n,
                        message: "'super' handle cannot be used bare without '.<member>'".into(),
                    });
                    continue;
                }
                let mut cur = up(n);
                let mut class = None;
                while let Some(a) = cur {
                    match t.kind(a) {
                        // `obj.randomize() with { super.x ... }` binds in obj's class.
                        WithClause | ConstraintBlock => break,
                        ClassDecl => {
                            let own_base = t.child(a, Header).and_then(|h| t.child(h, Extends)).is_some();
                            class = Some((name_of(t, a), Some(own_base)));
                            break;
                        }
                        FunctionDecl | TaskDecl => {
                            let nm = name_of(t, a);
                            if let Some((c, _)) = nm.split_once("::") {
                                let c = c.trim().to_string();
                                class = Some((c.clone(), unique(&c).map(|cls| cls.base.is_some())));
                                break;
                            }
                        }
                        _ => {}
                    }
                    cur = up(a);
                }
                if let Some((c, Some(false))) = class {
                    out.push(Diag { tree: ix, node: n, message: format!("class '{c}' does not have a super class") });
                }
            }
            NameExpr if t.bare_text(n) == "this" => {
                if in_dtype(up(n)) {
                    continue;
                }
                if let Some(p) = up(n) {
                    if t.kind(p) == BinaryExpr
                        && t.children(p).next() == Some(n)
                        && own_toks(t, p).first().is_some_and(|op| crate::checks::is_assign_op(op))
                    {
                        out.push(Diag { tree: ix, node: n, message: "cannot assign to 'this'".into() });
                    }
                }
                let mut cur = up(n);
                let mut in_prop_init = false;
                while let Some(a) = cur {
                    match t.kind(a) {
                        WithClause | ConstraintDecl | CovergroupDecl => break,
                        DataDecl if up(a).is_some_and(|p| t.kind(p) == ClassDecl) => {
                            in_prop_init = true;
                            break;
                        }
                        FunctionDecl | TaskDecl => {
                            let own = own_toks(t, a);
                            let kw = own.iter().position(|x| matches!(*x, "function" | "task"));
                            let is_static_fn =
                                own.iter().position(|x| *x == "static").is_some_and(|s| kw.is_some_and(|k| s < k));
                            let nm = name_of(t, a);
                            if let Some((c_raw, m_raw)) = nm.split_once("::") {
                                let proto_static = unique(c_raw.trim())
                                    .and_then(|cls| cls.methods.get(m_raw.trim()))
                                    .is_some_and(|pm| {
                                        let pt = &trees[pm.tree];
                                        let p_own = own_toks(pt, pm.node);
                                        let p_kw = p_own.iter().position(|x| matches!(*x, "function" | "task"));
                                        p_own
                                            .iter()
                                            .position(|x| *x == "static")
                                            .is_some_and(|s| p_kw.is_some_and(|k| s < k))
                                    });
                                if is_static_fn || proto_static {
                                    out.push(Diag {
                                        tree: ix,
                                        node: n,
                                        message: "'this' cannot be used in a static method".into(),
                                    });
                                }
                                break;
                            }
                            let mut anc = up(a);
                            let mut inside_cls = false;
                            while let Some(p) = anc {
                                if t.kind(p) == ClassDecl {
                                    inside_cls = true;
                                    break;
                                }
                                anc = up(p);
                            }
                            if !inside_cls {
                                out.push(Diag {
                                    tree: ix,
                                    node: n,
                                    message: "'this' can only be used inside a class".into(),
                                });
                            } else if is_static_fn {
                                out.push(Diag {
                                    tree: ix,
                                    node: n,
                                    message: "'this' cannot be used in a static method".into(),
                                });
                            }
                            break;
                        }
                        ClassDecl => break,
                        ModuleDecl | InterfaceDecl | ProgramDecl | PackageDecl => {
                            out.push(Diag {
                                tree: ix,
                                node: n,
                                message: "'this' can only be used inside a class".into(),
                            });
                            break;
                        }
                        _ => {}
                    }
                    cur = up(a);
                }
                if in_prop_init {
                    out.push(Diag {
                        tree: ix,
                        node: n,
                        message: "'this' cannot be used in a class property initializer".into(),
                    });
                }
            }
            _ => {}
        }
    }
}

/// Non-static properties, `super`, or bare `randomize` used in a static method
/// or a static property's initializer.
fn statics(ix: usize, t: &Tree, class: NodeId, out: &mut Vec<Diag>) {
    let mut props = HashSet::default();
    let mut has_static_randomize = false;
    for d in t.children(class) {
        match t.kind(d) {
            DataDecl => {
                let own = own_toks(t, d);
                if own.contains(&"static") || own.contains(&"typedef") {
                    continue;
                }
                for x in t.children(d).filter(|&c| t.kind(c) == Declarator) {
                    props.insert(name_of(t, x));
                }
            }
            FunctionDecl | TaskDecl | MethodPrototype if name_of(t, d) == "randomize" => {
                let own = own_toks(t, d);
                let kw = own.iter().position(|x| matches!(*x, "function" | "task"));
                if own.iter().position(|x| *x == "static").is_some_and(|s| kw.is_some_and(|k| s < k)) {
                    has_static_randomize = true;
                }
            }
            _ => {}
        }
    }
    props.remove("");
    let mut bodies: Vec<NodeId> = Vec::new();
    for m in t.children(class) {
        let own = own_toks(t, m);
        match t.kind(m) {
            FunctionDecl | TaskDecl => {
                let kw = own.iter().position(|x| matches!(*x, "function" | "task"));
                if own.iter().position(|x| *x == "static").is_some_and(|s| kw.is_some_and(|k| s < k)) {
                    bodies.push(m);
                }
            }
            DataDecl if own.contains(&"static") => {
                for x in t.children(m).filter(|&c| t.kind(c) == Declarator) {
                    bodies.extend(t.children(x).filter(|&c| !matches!(t.kind(c), Name | Dim)));
                }
            }
            _ => {}
        }
    }
    for b in bodies {
        // Names the method declares itself hide the properties.
        let mut local = HashSet::default();
        let mut stack = vec![b];
        let mut uses = Vec::new();
        while let Some(n) = stack.pop() {
            match t.kind(n) {
                Name => {
                    local.insert(t.bare_text(n));
                }
                LoopStmt => {
                    local.extend(own_toks(t, n).into_iter().map(String::from));
                }
                NameExpr => uses.push(n),
                WithClause | ClassDecl | DataType => continue,
                // `$bits(a)` and the like do not evaluate their argument.
                CallExpr
                    if t.children(n).next().is_some_and(|c| {
                        matches!(
                            t.bare_text(c).as_str(),
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
                        )
                    }) =>
                {
                    continue
                }
                _ => {}
            }
            stack.extend(t.children(n));
        }
        for u in uses {
            let name = t.bare_text(u);
            if name == "super" {
                out.push(Diag { tree: ix, node: u, message: "cannot use 'super' in a static class context".into() });
            } else if name == "randomize" && !has_static_randomize && !local.contains("randomize") {
                out.push(Diag {
                    tree: ix,
                    node: u,
                    message: "cannot call non-static class method 'randomize' in a static context".into(),
                });
            } else if props.contains(&name) && !local.contains(&name) {
                out.push(Diag {
                    tree: ix,
                    node: u,
                    message: format!("invalid use of non-static class property '{name}'"),
                });
            }
        }
    }
}

/// Out-of-block method and constraint definitions: prototype matching, lifetime,
/// and duplicate definition checks.
fn out_of_block<'a>(trees: &[Tree], unique: &dyn Fn(&str) -> Option<&'a Class>, out: &mut Vec<Diag>) {
    let mut seen: HashSet<(usize, u32, String)> = HashSet::default();
    for (ix, t) in trees.iter().enumerate() {
        let mut parent = HashMap::default();
        for n in t.preorder() {
            for c in t.children(n) {
                parent.insert(c, n);
            }
        }
        for n in t.preorder() {
            if t.kind(n) == ConstraintDecl {
                let full = name_of(t, n);
                let Some((c, cname)) = full.split_once("::") else { continue };
                let (c, cname) = (c.trim(), cname.trim());
                let Some(class) = unique(c) else { continue };
                if let Some(proto) = class.constraints.get(cname) {
                    if proto.is_extern && override_specs(&own_toks(t, n)) != proto.specs {
                        let at = t.child(n, Name).unwrap_or(n);
                        out.push(Diag {
                            tree: ix,
                            node: at,
                            message: format!(
                                "override specifiers of out-of-block constraint '{c}::{cname}' do not match prototype"
                            ),
                        });
                    }
                }
                continue;
            }
            if !matches!(t.kind(n), FunctionDecl | TaskDecl) {
                continue;
            }
            let full = name_of(t, n);
            let Some((c, m)) = full.split_once("::") else { continue };
            let (c, m) = (c.trim(), m.trim());
            let key = format!("{c}::{m}");
            let at = t.child(n, Name).unwrap_or(n);
            let own = own_toks(t, n);
            if let Some(kw) = own.iter().position(|x| matches!(*x, "function" | "task")) {
                if own[kw + 1..].contains(&"static") {
                    out.push(Diag {
                        tree: ix,
                        node: at,
                        message: format!("out-of-block class method '{key}' cannot have 'static' lifetime"),
                    });
                }
            }
            let scope = parent.get(&n).map_or(u32::MAX, |p| p.0);
            if !seen.insert((ix, scope, key.clone())) {
                out.push(Diag { tree: ix, node: at, message: format!("redefinition of '{key}'") });
                continue;
            }
            let Some(class) = unique(c) else { continue };
            let Some(proto) = class.methods.get(m) else { continue };
            let pt = &trees[proto.tree];
            if pt.kind(proto.node) != MethodPrototype {
                continue;
            }
            if let (Some(r), Some(pr)) = (simple_type(t, n), &proto.ret) {
                if &r != pr {
                    out.push(Diag {
                        tree: ix,
                        node: at,
                        message: format!(
                            "return type '{r}' of method '{m}' does not match '{pr}' declared in its prototype"
                        ),
                    });
                }
            }
        }
    }
}
