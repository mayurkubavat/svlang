//! UVM Register Abstraction Layer (`uvm_reg_block` / `uvm_reg` / `uvm_reg_field`)
//! specification extractor over `svlang::syntax::Tree`.
//!
//! Extracts register maps (`create_map` / `add_reg`), register widths (`super.new`),
//! and field bit-slices, access policies (`RW`, `RO`, `W1C`, `W1S`, `W0C`, `WO`, `RC`),
//! and reset values (`uvm_reg_field::configure`) for formal CSR verification and
//! RAL static analysis.

use std::collections::HashMap;
use svlang::syntax::{NodeId, SyntaxKind, Tree};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RalFieldSpec {
    pub name: String,
    pub lsb: u32,
    pub width: u32,
    pub access: String,
    pub volatile: bool,
    pub reset: u64,
    pub has_reset: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RalRegSpec {
    pub name: String,
    pub class_name: String,
    pub offset: u64,
    pub n_bits: u32,
    pub rights: String,
    pub fields: Vec<RalFieldSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RalBlockSpec {
    pub name: String,
    pub base_addr: u64,
    pub addr_unit_bytes: u32,
    pub registers: Vec<RalRegSpec>,
}

/// Parses a SystemVerilog integer literal (`32'h10`, `'hFF`, `8'b1010`, `16`, `'0`, `'1`).
pub fn parse_sv_u64(raw: &str) -> Option<u64> {
    let s = raw.trim().replace('_', "");
    if s.is_empty() {
        return None;
    }
    if s == "'0" {
        return Some(0);
    }
    if s == "'1" {
        return Some(u64::MAX);
    }
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return u64::from_str_radix(hex, 16).ok();
    }
    if let Some((_, rhs)) = s.split_once('\'') {
        let rhs = rhs.trim_start_matches(['s', 'S']);
        if rhs.is_empty() {
            return None;
        }
        let (radix, digits) = match rhs.as_bytes()[0] {
            b'h' | b'H' => (16, &rhs[1..]),
            b'd' | b'D' => (10, &rhs[1..]),
            b'o' | b'O' => (8, &rhs[1..]),
            b'b' | b'B' => (2, &rhs[1..]),
            b'0' => return Some(0),
            b'1' => return Some(u64::MAX),
            _ => (10, rhs),
        };
        return u64::from_str_radix(digits.trim(), radix).ok();
    }
    s.parse::<u64>().ok()
}

fn unquote(raw: &str) -> String {
    let t = raw.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        t[1..t.len() - 1].to_string()
    } else {
        t.to_string()
    }
}

fn subtree_nodes(tree: &Tree, root: NodeId) -> Vec<NodeId> {
    let mut out = vec![root];
    let mut i = 0;
    while i < out.len() {
        let cur = out[i];
        i += 1;
        out.extend(tree.children(cur));
    }
    out
}

fn call_arg_texts(tree: &Tree, call: NodeId) -> Vec<String> {
    let Some(arg_list) = tree.child(call, SyntaxKind::ArgList) else {
        return Vec::new();
    };
    let mut args = Vec::new();
    for ch in tree.children(arg_list) {
        if tree.kind(ch) == SyntaxKind::Arg {
            // Strip `.param(val)` named argument wrapper if used.
            let txt = tree.bare_text(ch);
            if let Some(rest) = txt.strip_prefix('.') {
                if let Some((_, inner)) = rest.split_once('(') {
                    if let Some(val) = inner.strip_suffix(')') {
                        args.push(val.trim().to_string());
                        continue;
                    }
                }
            }
            args.push(txt.trim().to_string());
        } else {
            let txt = tree.bare_text(ch);
            if !txt.trim().is_empty() {
                args.push(txt.trim().to_string());
            }
        }
    }
    args
}

fn class_name_and_base(tree: &Tree, cls: NodeId) -> Option<(String, Option<String>)> {
    let (lo, hi) = tree.token_bounds(cls)?;
    let mut cname: Option<String> = None;
    let mut base: Option<String> = None;
    let mut ti = lo;
    while ti <= hi {
        let txt = &*tree.tokens[ti as usize].text;
        if txt == "class" {
            let mut k = ti + 1;
            while k <= hi && matches!(&*tree.tokens[k as usize].text, "automatic" | "static") {
                k += 1;
            }
            if k <= hi {
                cname = Some(tree.tokens[k as usize].text.to_string());
            }
        } else if txt == "extends" && cname.is_some() {
            if ti < hi {
                base = Some(tree.tokens[(ti + 1) as usize].text.to_string());
            }
            break;
        } else if txt == ";" && cname.is_some() {
            break;
        }
        ti += 1;
    }
    cname.map(|n| (n, base))
}

#[derive(Clone, Debug)]
struct ParsedRegClass {
    class_name: String,
    n_bits: u32,
    fields: Vec<RalFieldSpec>,
}

/// Extracts all `uvm_reg_block` register models (or standalone `uvm_reg` classes wrapped in
/// a synthetic `"default_block"` when no `uvm_reg_block` is present) from `trees`.
pub fn extract_ral_blocks(trees: &[Tree]) -> Vec<RalBlockSpec> {
    let mut reg_classes: HashMap<String, ParsedRegClass> = HashMap::new();
    let mut raw_blocks: Vec<(&Tree, NodeId, String)> = Vec::new();

    // Pass 1: discover `uvm_reg` classes and `uvm_reg_block` classes.
    for tree in trees {
        for cls in tree.preorder() {
            if tree.kind(cls) != SyntaxKind::ClassDecl {
                continue;
            }
            let Some((cname, base)) = class_name_and_base(tree, cls) else {
                continue;
            };
            let base_str = base.as_deref().unwrap_or("");
            if base_str.contains("reg_block") {
                raw_blocks.push((tree, cls, cname));
                continue;
            }

            let mut n_bits: u32 = 0;
            let mut fields: Vec<RalFieldSpec> = Vec::new();

            for fn_node in tree.children(cls) {
                if tree.kind(fn_node) != SyntaxKind::FunctionDecl {
                    continue;
                }
                for call in subtree_nodes(tree, fn_node) {
                    if tree.kind(call) != SyntaxKind::CallExpr {
                        continue;
                    }
                    let Some(callee) = tree.children(call).next() else {
                        continue;
                    };
                    let callee_txt = tree.bare_text(callee);
                    if callee_txt == "super.new" {
                        let args = call_arg_texts(tree, call);
                        if let Some(nb) = args.get(1).and_then(|s| parse_sv_u64(s)) {
                            if nb > 0 && nb <= 1024 {
                                n_bits = nb as u32;
                            }
                        }
                    } else if let Some(recv) = callee_txt.strip_suffix(".configure") {
                        let fname = recv.trim().trim_start_matches("this.").to_string();
                        let args = call_arg_texts(tree, call);
                        // uvm_reg_field::configure(parent, size, lsb_pos, access, volatile, reset, has_reset, ...)
                        if args.len() >= 4 && args[0] == "this" && !fname.is_empty() {
                            let Some(width) = parse_sv_u64(&args[1]).map(|v| v as u32) else {
                                continue;
                            };
                            let Some(lsb) = parse_sv_u64(&args[2]).map(|v| v as u32) else {
                                continue;
                            };
                            if width == 0 {
                                continue;
                            }
                            let access = unquote(&args[3]).to_ascii_uppercase();
                            let volatile = args.get(4).and_then(|s| parse_sv_u64(s)).is_some_and(|v| v != 0);
                            let reset = args.get(5).and_then(|s| parse_sv_u64(s)).unwrap_or(0);
                            let has_reset = args.get(6).map(|s| parse_sv_u64(s).unwrap_or(1) != 0).unwrap_or(true);
                            fields.push(RalFieldSpec { name: fname, lsb, width, access, volatile, reset, has_reset });
                        }
                    }
                }
            }

            if !fields.is_empty() || base_str == "uvm_reg" {
                fields.sort_by_key(|f| f.lsb);
                if n_bits == 0 {
                    let max_bit = fields.iter().map(|f| f.lsb + f.width).max().unwrap_or(32);
                    n_bits = max_bit.next_power_of_two().max(8);
                }
                reg_classes.insert(cname.clone(), ParsedRegClass { class_name: cname, n_bits, fields });
            }
        }
    }

    // Pass 2: assemble `RalBlockSpec`s from `uvm_reg_block` classes.
    let mut blocks = Vec::new();
    for (tree, cls, bname) in raw_blocks {
        // Map member variable name -> register class type name (`rand ctrl_reg ctrl;`).
        let mut member_types: HashMap<String, String> = HashMap::new();
        for ch in tree.children(cls) {
            if tree.kind(ch) == SyntaxKind::DataDecl {
                let mut type_name: Option<String> = None;
                for sub in tree.children(ch) {
                    if tree.kind(sub) == SyntaxKind::DataType {
                        type_name = Some(tree.bare_text(sub).trim().to_string());
                    } else if tree.kind(sub) == SyntaxKind::Declarator {
                        if let Some(ref tn) = type_name {
                            if reg_classes.contains_key(tn) {
                                if let Some(nm) = tree.child(sub, SyntaxKind::Name) {
                                    member_types.insert(tree.bare_text(nm).trim().to_string(), tn.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut base_addr: u64 = 0;
        let mut addr_unit_bytes: u32 = 4;
        let mut registers: Vec<RalRegSpec> = Vec::new();

        for fn_node in tree.children(cls) {
            if tree.kind(fn_node) != SyntaxKind::FunctionDecl {
                continue;
            }
            for call in subtree_nodes(tree, fn_node) {
                if tree.kind(call) != SyntaxKind::CallExpr {
                    continue;
                }
                let Some(callee) = tree.children(call).next() else {
                    continue;
                };
                let callee_txt = tree.bare_text(callee);
                if callee_txt.ends_with("create_map") {
                    // create_map(name, base_addr, n_bytes, endian, ...)
                    let args = call_arg_texts(tree, call);
                    if let Some(ba) = args.get(1).and_then(|s| parse_sv_u64(s)) {
                        base_addr = ba;
                    }
                    if let Some(nb) = args.get(2).and_then(|s| parse_sv_u64(s)) {
                        if nb > 0 && nb <= 64 {
                            addr_unit_bytes = nb as u32;
                        }
                    }
                } else if callee_txt.ends_with(".add_reg") {
                    // map.add_reg(rg, offset, rights = "RW", ...)
                    let args = call_arg_texts(tree, call);
                    if args.len() >= 2 {
                        let inst_name = args[0].trim().trim_start_matches("this.").to_string();
                        let offset = parse_sv_u64(&args[1]).unwrap_or(0);
                        let rights =
                            args.get(2).map(|s| unquote(s).to_ascii_uppercase()).unwrap_or_else(|| "RW".into());
                        if let Some(rcls_name) = member_types
                            .get(&inst_name)
                            .or_else(|| reg_classes.contains_key(&inst_name).then_some(&inst_name))
                        {
                            if let Some(rc) = reg_classes.get(rcls_name) {
                                registers.push(RalRegSpec {
                                    name: inst_name,
                                    class_name: rc.class_name.clone(),
                                    offset,
                                    n_bits: rc.n_bits,
                                    rights,
                                    fields: rc.fields.clone(),
                                });
                            }
                        }
                    }
                }
            }
        }
        registers.sort_by_key(|r| r.offset);
        blocks.push(RalBlockSpec { name: bname, base_addr, addr_unit_bytes, registers });
    }

    // Fallback when a file defines standalone `uvm_reg` classes without a `uvm_reg_block`.
    if blocks.is_empty() && !reg_classes.is_empty() {
        let mut regs: Vec<RalRegSpec> = reg_classes
            .into_values()
            .enumerate()
            .map(|(idx, rc)| RalRegSpec {
                name: rc.class_name.clone(),
                class_name: rc.class_name,
                offset: (idx as u64) * 4,
                n_bits: rc.n_bits,
                rights: "RW".into(),
                fields: rc.fields,
            })
            .collect();
        regs.sort_by(|a, b| a.name.cmp(&b.name));
        for (idx, r) in regs.iter_mut().enumerate() {
            r.offset = (idx as u64) * 4;
        }
        blocks.push(RalBlockSpec {
            name: "default_reg_block".into(),
            base_addr: 0,
            addr_unit_bytes: 4,
            registers: regs,
        });
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_uvm_reg_block_registers_fields_offsets_and_access_policies() {
        let src = r#"
class ctrl_reg extends uvm_reg;
  rand uvm_reg_field en;
  rand uvm_reg_field mode;
  rand uvm_reg_field irq_clr;
  function new(string name = "ctrl_reg");
    super.new(name, 32, UVM_NO_COVERAGE);
  endfunction
  virtual function void build();
    en      = uvm_reg_field::type_id::create("en");
    mode    = uvm_reg_field::type_id::create("mode");
    irq_clr = uvm_reg_field::type_id::create("irq_clr");
    en.configure(this, 1, 0, "RW", 0, 1'b1, 1, 1, 0);
    mode.configure(this, 3, 1, "RO", 0, 3'h5, 1, 0, 0);
    irq_clr.configure(this, 4, 4, "W1C", 1, 4'h0, 1, 1, 0);
  endfunction
endclass

class uart_reg_block extends uvm_reg_block;
  rand ctrl_reg ctrl;
  virtual function void build();
    ctrl = ctrl_reg::type_id::create("ctrl");
    ctrl.configure(this);
    ctrl.build();
    default_map = create_map("default_map", 'h1000, 4, UVM_LITTLE_ENDIAN);
    default_map.add_reg(ctrl, 'h04, "RW");
  endfunction
endclass
"#;
        let tree = svlang::syntax::parse_source(src, "uart_ral.sv", Default::default());
        let blocks = extract_ral_blocks(&[tree]);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.name, "uart_reg_block");
        assert_eq!(b.base_addr, 0x1000);
        assert_eq!(b.addr_unit_bytes, 4);
        assert_eq!(b.registers.len(), 1);
        let r = &b.registers[0];
        assert_eq!(r.name, "ctrl");
        assert_eq!(r.offset, 0x04);
        assert_eq!(r.n_bits, 32);
        assert_eq!(r.fields.len(), 3);
        assert_eq!(
            (
                r.fields[0].name.as_str(),
                r.fields[0].lsb,
                r.fields[0].width,
                r.fields[0].access.as_str(),
                r.fields[0].reset
            ),
            ("en", 0, 1, "RW", 1)
        );
        assert_eq!(
            (
                r.fields[1].name.as_str(),
                r.fields[1].lsb,
                r.fields[1].width,
                r.fields[1].access.as_str(),
                r.fields[1].reset
            ),
            ("mode", 1, 3, "RO", 5)
        );
        assert_eq!(
            (
                r.fields[2].name.as_str(),
                r.fields[2].lsb,
                r.fields[2].width,
                r.fields[2].access.as_str(),
                r.fields[2].volatile
            ),
            ("irq_clr", 4, 4, "W1C", true)
        );
    }
}
