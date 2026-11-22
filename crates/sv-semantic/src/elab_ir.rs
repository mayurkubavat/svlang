//! Arena-backed elaborated RTL design object model (IEEE 1800-2023 Clause 23).
//!
//! Produced alongside the flat hierarchy text lines during [`crate::elaborate_full`] /
//! [`crate::elaborate_with_params`]. Every elaborated module or interface instance gets
//! its own [`Instance`] and its own [`Symbol`]s (no instance sharing across separate
//! instantiations), and taken generate blocks (`if`/`case`/`for`) appear as lexical
//! scope prefixes (`g[0].x`) on [`Symbol::rel_path`] and [`Instance::rel_path`].
//!
//! The types themselves live in `sv-ir` (`sv_ir::elab_ir`) and are re-exported
//! here; this module adds how the builder resolves declarations into them.

use sv_syntax::SyntaxKind;

use crate::{Builder, Env, Found, NodeRef};

pub use sv_ir::elab_ir::*;

// ---- Builder -> RtlType resolution ------------------------------------------

impl Builder<'_> {
    /// Resolves a CST declaration `(dtype, declarator)` in `(scope, env)` (with `self.inst`
    /// already populated with the active `Vals`) into an [`RtlType`].
    pub(crate) fn rtl_type(
        &self,
        dt: Option<NodeRef>,
        item: Option<NodeRef>,
        scope: usize,
        env: &Env,
        depth: usize,
    ) -> RtlType {
        if depth > 24 {
            return RtlType::NonSynthesizable("recursive type".into());
        }
        // 1. Unpacked dimensions on the declarator (`logic [7:0] mem [0:15]`).
        if let Some(decl_item) = item {
            let udims: Vec<NodeRef> =
                self.children(decl_item).into_iter().filter(|c| self.kind(*c) == SyntaxKind::Dim).collect();
            if !udims.is_empty() {
                let elem = self.rtl_type(dt, None, scope, env, depth + 1);
                return self.wrap_unpacked_dims(elem, &udims, scope, env);
            }
        }
        let Some(dt) = dt else {
            // Implicit type (`wire w`, `input a`): 1-bit unsigned 4-state logic.
            return RtlType::Integral { width: 1, signed: false, four_state: true };
        };
        self.rtl_data_type(dt, scope, env, None, depth)
    }

    fn wrap_unpacked_dims(&self, elem: RtlType, udims: &[NodeRef], scope: usize, env: &Env) -> RtlType {
        if let RtlType::NonSynthesizable(_) = &elem {
            return elem;
        }
        let mut total_elems: u32 = match &elem {
            RtlType::UnpackedArray { dims, .. } => {
                let mut acc = 1u32;
                for &(l, r) in dims {
                    let Some(c) = crate::types::range_len_u32(l, r, crate::types::MAX_ARRAY_ELEMS) else {
                        return RtlType::NonSynthesizable("unpacked array exceeds maximum element limit".into());
                    };
                    let Some(next) = acc.checked_mul(c).filter(|&v| v <= crate::types::MAX_ARRAY_ELEMS) else {
                        return RtlType::NonSynthesizable("unpacked array exceeds maximum element limit".into());
                    };
                    acc = next;
                }
                acc
            }
            _ => 1,
        };
        let mut bounds = Vec::with_capacity(udims.len());
        for &d in udims {
            let toks = self.toks_of(d);
            let inner: Vec<&str> = toks.iter().copied().filter(|t| !matches!(*t, "[" | "]")).collect();
            if inner.is_empty() {
                return RtlType::NonSynthesizable("dynamic array".into());
            }
            if inner.contains(&"$") {
                return RtlType::NonSynthesizable("queue".into());
            }
            if inner == ["*"] || self.child(d, SyntaxKind::DataType).is_some() {
                return RtlType::NonSynthesizable("associative array".into());
            }
            if self.dim_is_error(d, scope, env, true) {
                return RtlType::NonSynthesizable("invalid unpacked array dimension".into());
            }
            let Some((l, r)) = self.dim_range(d, scope, env) else {
                return RtlType::NonSynthesizable("unpacked array with non-constant bounds".into());
            };
            let Some(cnt) = crate::types::range_len_u32(l, r, crate::types::MAX_ARRAY_ELEMS) else {
                return RtlType::NonSynthesizable("unpacked array exceeds maximum element limit".into());
            };
            let Some(next) = total_elems.checked_mul(cnt).filter(|&v| v <= crate::types::MAX_ARRAY_ELEMS) else {
                return RtlType::NonSynthesizable("unpacked array exceeds maximum element limit".into());
            };
            total_elems = next;
            bounds.push((l, r));
        }
        match elem {
            RtlType::UnpackedArray { elem: inner_elem, dims: mut inner_dims } => {
                bounds.append(&mut inner_dims);
                RtlType::UnpackedArray { elem: inner_elem, dims: bounds }
            }
            other => RtlType::UnpackedArray { elem: Box::new(other), dims: bounds },
        }
    }

    pub(crate) fn rtl_data_type(
        &self,
        dt: NodeRef,
        scope: usize,
        env: &Env,
        alias: Option<String>,
        depth: usize,
    ) -> RtlType {
        if depth > 24 {
            return RtlType::NonSynthesizable("recursive type".into());
        }
        let own = self.own_tokens(dt);
        let pdims: Vec<NodeRef> = self.children(dt).into_iter().filter(|c| self.kind(*c) == SyntaxKind::Dim).collect();

        // Non-synthesizable scalar keywords.
        if let Some(&kw) = own.first() {
            match kw {
                "real" | "shortreal" | "realtime" | "string" | "chandle" => {
                    return RtlType::NonSynthesizable(kw.into());
                }
                "virtual" => return RtlType::NonSynthesizable("virtual interface".into()),
                "event" => return RtlType::Event,
                "void" => return RtlType::Void,
                _ => {}
            }
        }

        // `enum [base_type] { ... } [pdims]`
        if own.first() == Some(&"enum") {
            let base = self.child(dt, SyntaxKind::DataType);
            let (width, signed, four_state) = match base {
                Some(b) => {
                    let bty = self.rtl_data_type(b, scope, env, None, depth + 1);
                    if let RtlType::NonSynthesizable(r) = bty {
                        return RtlType::NonSynthesizable(r);
                    }
                    let Some(shape) = bty.integral_shape() else {
                        return RtlType::NonSynthesizable("non-integral enum base type".into());
                    };
                    shape
                }
                None => (32, true, false),
            };
            let variants = self.enum_values(dt, scope, env, depth + 1);
            let ety = RtlType::Enum { name: alias, width, signed, four_state, variants };
            return self.wrap_packed_dims(ety, &pdims, scope, env);
        }

        // `struct` / `union` (packed or unpacked)
        if matches!(own.first(), Some(&"struct" | &"union")) {
            let is_union = own.first() == Some(&"union");
            let is_tagged = own.contains(&"tagged");
            let is_soft = own.contains(&"soft");
            let is_packed = own.contains(&"packed") || (is_union && is_soft);
            let signed = own.contains(&"signed");
            if is_packed {
                let mut raw_members: Vec<RawMember> = Vec::new();
                for m in self.children(dt) {
                    if self.kind(m) != SyntaxKind::DataDecl {
                        continue;
                    }
                    let mdt = self.child(m, SyntaxKind::DataType);
                    let mbounds = self.packed_1d_bounds(mdt, scope, env, depth + 1);
                    for d in self.children(m) {
                        if self.kind(d) == SyntaxKind::Declarator {
                            let Some((fname, _)) = self.name_of(d) else { continue };
                            let fty = self.rtl_type(mdt, Some(d), scope, env, depth + 1);
                            if let RtlType::NonSynthesizable(r) = fty {
                                return RtlType::NonSynthesizable(r);
                            }
                            let Some(w) = fty.bit_width() else {
                                return RtlType::NonSynthesizable(format!(
                                    "unpacked member '{fname}' inside packed struct or union"
                                ));
                            };
                            if is_union && !is_tagged && !is_soft {
                                if let Some(&(_, _, first_w, _)) = raw_members.first() {
                                    if w != first_w {
                                        return RtlType::NonSynthesizable(format!(
                                            "member '{fname}' of width {w} does not match packed union width {first_w}"
                                        ));
                                    }
                                }
                            }
                            raw_members.push((fname, fty, w, mbounds));
                        }
                    }
                }
                let total = if is_union {
                    raw_members.iter().map(|m| m.2).max().unwrap_or(1)
                } else {
                    let mut sum = 0u32;
                    for &(_, _, w, _) in &raw_members {
                        let Some(next) = sum.checked_add(w).filter(|&tw| tw <= crate::types::MAX_BIT_WIDTH) else {
                            return RtlType::NonSynthesizable("packed struct exceeds maximum bit width".into());
                        };
                        sum = next;
                    }
                    sum.max(1)
                };
                let four_state = raw_members.iter().any(|(_, t, _, _)| t.is_four_state());
                let mut cursor = total;
                let fields: Vec<PackedField> = raw_members
                    .into_iter()
                    .map(|(fname, fty, w, bounds)| {
                        let lo = if is_union {
                            0
                        } else {
                            cursor = cursor.saturating_sub(w);
                            cursor
                        };
                        PackedField { name: fname, lo, width: w, ty: fty, bounds }
                    })
                    .collect();
                let sty = if is_union {
                    RtlType::PackedUnion { name: alias, width: total, signed, four_state, fields }
                } else {
                    RtlType::PackedStruct { name: alias, width: total, signed, four_state, fields }
                };
                return self.wrap_packed_dims(sty, &pdims, scope, env);
            }
            // Unpacked struct / union.
            let mut fields = Vec::new();
            for m in self.children(dt) {
                if self.kind(m) != SyntaxKind::DataDecl {
                    continue;
                }
                let mdt = self.child(m, SyntaxKind::DataType);
                for d in self.children(m) {
                    if self.kind(d) == SyntaxKind::Declarator {
                        if let Some((fname, _)) = self.name_of(d) {
                            let fty = self.rtl_type(mdt, Some(d), scope, env, depth + 1);
                            fields.push((fname, fty));
                        }
                    }
                }
            }
            return RtlType::UnpackedStruct { name: alias, is_union, fields };
        }

        // Built-in integer / vector keywords (`logic`, `bit`, `reg`, `int`, `byte`, ...)
        // or implicit vector (`[7:0]`, `signed [7:0]`).
        let kw = own.iter().find(|t| crate::types::INT_TYPES.contains(t)).copied();
        let implicit = kw.is_none()
            && own.iter().all(|t| matches!(*t, "signed" | "unsigned" | "parameter" | "localparam"))
            && (!own.is_empty() || !pdims.is_empty());
        if kw.is_some() || implicit {
            let four_state = !matches!(kw, Some("bit" | "int" | "byte" | "shortint" | "longint"));
            let default_signed = matches!(kw, Some("int" | "integer" | "byte" | "shortint" | "longint"));
            let signed = if own.contains(&"unsigned") {
                false
            } else if own.contains(&"signed") {
                true
            } else {
                default_signed
            };
            let base_w: u32 = match kw {
                Some("int" | "integer") => 32,
                Some("byte") => 8,
                Some("shortint") => 16,
                Some("longint" | "time") => 64,
                _ => 1,
            };
            // Single-dimension vector (`logic [7:0]`) is a plain `Integral`; multi-dimensional
            // (`logic [3:0][7:0]`) wraps outer dimensions as `PackedArray` so `x[i]` yields `logic [7:0]`.
            if pdims.is_empty() {
                return RtlType::Integral { width: base_w, signed, four_state };
            }
            if base_w == 1 && pdims.len() == 1 {
                if self.dim_is_error(pdims[0], scope, env, false) {
                    return RtlType::NonSynthesizable("invalid packed dimension".into());
                }
                let Some((l, r)) = self.dim_range(pdims[0], scope, env) else {
                    return RtlType::NonSynthesizable("packed dimension with non-constant bounds".into());
                };
                let Some(width) = crate::types::range_len_u32(l, r, crate::types::MAX_BIT_WIDTH) else {
                    return RtlType::NonSynthesizable("packed vector exceeds maximum bit width".into());
                };
                return RtlType::Integral { width, signed, four_state };
            }
            let inner_start = if base_w == 1 {
                let last = pdims[pdims.len() - 1];
                if self.dim_is_error(last, scope, env, false) {
                    return RtlType::NonSynthesizable("invalid packed dimension".into());
                }
                let Some((l, r)) = self.dim_range(last, scope, env) else {
                    return RtlType::NonSynthesizable("packed dimension with non-constant bounds".into());
                };
                let Some(w) = crate::types::range_len_u32(l, r, crate::types::MAX_BIT_WIDTH) else {
                    return RtlType::NonSynthesizable("packed vector exceeds maximum bit width".into());
                };
                let inner = RtlType::Integral { width: w, signed: false, four_state };
                let mut wrapped = self.wrap_packed_dims(inner, &pdims[..pdims.len() - 1], scope, env);
                if signed {
                    if let RtlType::PackedArray { signed: s, .. } = &mut wrapped {
                        *s = true;
                    }
                }
                return wrapped;
            } else {
                let inner = RtlType::Integral { width: base_w, signed: false, four_state };
                self.wrap_packed_dims(inner, &pdims, scope, env)
            };
            return inner_start;
        }

        // Named type: type parameter, typedef, interface, or class.
        let toks = self.toks_of(dt);
        if toks.is_empty() {
            return RtlType::Integral { width: 1, signed: false, four_state: true };
        }
        let head: Vec<&str> = toks.iter().take_while(|t| **t != "[").copied().collect();
        if let [tname] = head.as_slice() {
            if let Some(bd) = env.get(*tname) {
                // Evaluate the argument with the parameter values of the instance it came from.
                let saved = bd.vals.clone().map(|v| self.inst.replace(v));
                let base = self.rtl_data_type(bd.node, bd.scope, &bd.env, Some((*tname).to_string()), depth + 1);
                if let Some(sv) = saved {
                    *self.inst.borrow_mut() = sv;
                }
                return self.wrap_packed_dims(base, &pdims, scope, env);
            }
            let mut cur = Some(scope);
            while let Some(c) = cur {
                if let Some(Some(def_dt)) = self.scopes[c].type_params.get(*tname) {
                    let base = self.rtl_data_type(*def_dt, c, env, Some((*tname).to_string()), depth + 1);
                    return self.wrap_packed_dims(base, &pdims, scope, env);
                }
                cur = self.scopes[c].parent;
            }
            if self.interfaces.contains_key(*tname) {
                return RtlType::Interface { def: (*tname).to_string(), modport: None };
            }
            if let Some(td) = self.block_typedef(tname, dt) {
                let udims: Vec<NodeRef> =
                    self.children(td).into_iter().filter(|c| self.kind(*c) == SyntaxKind::Dim).collect();
                let base = match self.child(td, SyntaxKind::DataType) {
                    Some(inner_dt) => self.rtl_data_type(inner_dt, scope, env, Some((*tname).to_string()), depth + 1),
                    None => RtlType::NonSynthesizable(format!("unresolved forward typedef '{tname}'")),
                };
                let with_udims =
                    if udims.is_empty() { base } else { self.wrap_unpacked_dims(base, &udims, scope, env) };
                return self.wrap_packed_dims(with_udims, &pdims, scope, env);
            }
        }
        if let [iface, ".", mp] = head.as_slice() {
            if self.interfaces.contains_key(*iface) {
                return RtlType::Interface { def: (*iface).to_string(), modport: Some((*mp).to_string()) };
            }
        }
        if head.len() == 1 || head.contains(&"::") {
            match self.lookup_type_pub(dt, scope) {
                Some(Found::Typedef(td, td_scope)) => {
                    let tname = self.name_of(td).map(|x| x.0);
                    let udims: Vec<NodeRef> =
                        self.children(td).into_iter().filter(|c| self.kind(*c) == SyntaxKind::Dim).collect();
                    let base = match self.child(td, SyntaxKind::DataType) {
                        Some(inner_dt) => self.rtl_data_type(inner_dt, td_scope, env, tname, depth + 1),
                        None => RtlType::NonSynthesizable(format!(
                            "unresolved forward typedef '{}'",
                            tname.unwrap_or_default()
                        )),
                    };
                    let with_udims =
                        if udims.is_empty() { base } else { self.wrap_unpacked_dims(base, &udims, td_scope, env) };
                    let mut wrapped = self.wrap_packed_dims(with_udims, &pdims, scope, env);
                    if own.contains(&"signed") || own.contains(&"unsigned") {
                        let sg = own.contains(&"signed");
                        match &mut wrapped {
                            RtlType::Integral { signed, .. }
                            | RtlType::PackedStruct { signed, .. }
                            | RtlType::PackedUnion { signed, .. }
                            | RtlType::PackedArray { signed, .. }
                            | RtlType::Enum { signed, .. } => *signed = sg,
                            _ => {}
                        }
                    }
                    return wrapped;
                }
                Some(Found::Class(cid)) => {
                    return RtlType::NonSynthesizable(format!("class {}", self.classes[cid].name));
                }
                Some(Found::Param(Some(pdt), psc)) => {
                    let base = self.rtl_data_type(pdt, psc, env, None, depth + 1);
                    return self.wrap_packed_dims(base, &pdims, scope, env);
                }
                _ => {}
            }
        }
        if matches!(head.as_slice(), ["mailbox"] | ["semaphore"] | ["process"]) {
            return RtlType::NonSynthesizable(format!("class {}", head[0]));
        }
        let raw_name = self.tree(dt).bare_text(dt.node).trim().to_string();
        RtlType::NonSynthesizable(format!("unknown or unsupported type '{raw_name}'"))
    }

    fn wrap_packed_dims(&self, mut elem: RtlType, pdims: &[NodeRef], scope: usize, env: &Env) -> RtlType {
        if let RtlType::NonSynthesizable(_) = &elem {
            return elem;
        }
        for &d in pdims.iter().rev() {
            if self.dim_is_error(d, scope, env, false) {
                return RtlType::NonSynthesizable("invalid packed dimension".into());
            }
            let Some((left, right)) = self.dim_range(d, scope, env) else {
                return RtlType::NonSynthesizable("packed dimension with non-constant bounds".into());
            };
            let Some(count) = crate::types::range_len_u32(left, right, crate::types::MAX_BIT_WIDTH) else {
                return RtlType::NonSynthesizable("packed array exceeds maximum bit width".into());
            };
            let Some(ew) = elem.bit_width() else {
                return RtlType::NonSynthesizable("packed dimension applied to unpacked type".into());
            };
            let Some(width) = ew.checked_mul(count).filter(|&tw| tw <= crate::types::MAX_BIT_WIDTH) else {
                return RtlType::NonSynthesizable("packed array exceeds maximum bit width".into());
            };
            let four_state = elem.is_four_state();
            elem = RtlType::PackedArray { elem: Box::new(elem), left, right, width, signed: false, four_state };
        }
        elem
    }

    /// Returns `(left, right)` bounds when `dt` resolves to a 1-D packed vector (`logic [7:0]` or `logic [0:7]`).
    pub(crate) fn packed_1d_bounds(
        &self,
        dt: Option<NodeRef>,
        scope: usize,
        env: &Env,
        depth: usize,
    ) -> Option<(i128, i128)> {
        if depth > 16 {
            return None;
        }
        let dt = dt?;
        let pdims: Vec<NodeRef> = self.children(dt).into_iter().filter(|c| self.kind(*c) == SyntaxKind::Dim).collect();
        if pdims.len() == 1 {
            return self.dim_range(pdims[0], scope, env);
        }
        if !pdims.is_empty() {
            return None;
        }
        let toks = self.toks_of(dt);
        let head: Vec<&str> = toks.iter().take_while(|t| **t != "[").copied().collect();
        if let [tname] = head.as_slice() {
            if let Some(bd) = env.get(*tname) {
                let saved = bd.vals.clone().map(|v| self.inst.replace(v));
                let r = self.packed_1d_bounds(Some(bd.node), bd.scope, &bd.env, depth + 1);
                if let Some(sv) = saved {
                    *self.inst.borrow_mut() = sv;
                }
                return r;
            }
            if let Some(td) = self.block_typedef(tname, dt) {
                return self.packed_1d_bounds(self.child(td, SyntaxKind::DataType), scope, env, depth + 1);
            }
        }
        if head.len() == 1 || head.contains(&"::") {
            if let Some(Found::Typedef(td, td_scope)) = self.lookup_type_pub(dt, scope) {
                return self.packed_1d_bounds(self.child(td, SyntaxKind::DataType), td_scope, env, depth + 1);
            }
        }
        None
    }
}

/// A packed struct or union member while its layout is computed: name, type, bit width and
/// the declared range of a one-dimensional packed vector member.
type RawMember = (String, RtlType, u32, Option<(i128, i128)>);
