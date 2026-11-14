//! AIGER 1.9 reader (binary `aig` and ASCII `aag`) and ASCII writer.
//! Bad-state, constraint and latch-reset sections are supported; outputs
//! count as bad states when a file has no bad section (AIGER 1.0 style).
//! Justice and fairness sections are rejected.

use crate::err::{anyhow, bail, Context, Result};
use crate::{Aig, Init, Lit, Node};

pub fn read(bytes: &[u8]) -> Result<Aig> {
    let mut r = Reader { b: bytes, pos: 0 };
    let header = r.line()?;
    let h: Vec<&str> = header.split_whitespace().collect();
    let binary = match h.first() {
        Some(&"aig") => true,
        Some(&"aag") => false,
        _ => bail!("not an AIGER file"),
    };
    let num = |i: usize| -> Result<u32> { h.get(i).map_or(Ok(0), |s| s.parse().context("bad AIGER header")) };
    let (m, ni, nl, no, na, nb, nc, nj, nf) =
        (num(1)?, num(2)?, num(3)?, num(4)?, num(5)?, num(6)?, num(7)?, num(8)?, num(9)?);
    if nj > 0 || nf > 0 {
        bail!("AIGER justice / fairness (liveness) is not supported yet");
    }
    if m != ni + nl + na {
        bail!("AIGER header: M must equal I + L + A");
    }
    let mut g = Aig::new();
    // Inputs and latches keep their AIGER variable numbers.
    if binary {
        for _ in 0..ni {
            g.input();
        }
    } else {
        for k in 0..ni {
            let l: u32 = r.line()?.trim().parse()?;
            if l != 2 * (k + 1) {
                bail!("ASCII AIGER inputs must be numbered 2, 4, ... in order");
            }
            g.input();
        }
    }
    let mut latch_lines = Vec::new();
    for _ in 0..nl {
        latch_lines.push(r.line()?);
    }
    let mut outs = Vec::new();
    for _ in 0..no {
        outs.push(r.lit_line()?);
    }
    let mut bads = Vec::new();
    for _ in 0..nb {
        bads.push(r.lit_line()?);
    }
    let mut cons = Vec::new();
    for _ in 0..nc {
        cons.push(r.lit_line()?);
    }
    let first_latch = ni + 1;
    for (k, line) in latch_lines.iter().enumerate() {
        let w: Vec<Lit> = line
            .split_whitespace()
            .map(|s| s.parse::<Lit>())
            .collect::<std::result::Result<_, std::num::ParseIntError>>()?;
        let own = 2 * (first_latch + k as u32);
        let (next, reset) = if binary {
            (w[0], w.get(1).copied())
        } else {
            if w[0] != own {
                bail!("ASCII AIGER latches must be numbered in order");
            }
            (w[1], w.get(2).copied())
        };
        let init = match reset {
            None | Some(0) => Init::Zero,
            Some(1) => Init::One,
            Some(x) if x == own => Init::Free,
            Some(x) => bail!("latch reset {x} must be 0, 1 or the latch itself"),
        };
        let l = g.latch(init);
        debug_assert_eq!(l, own);
        g.latches.last_mut().unwrap().next = next;
    }
    if binary {
        for k in 0..na {
            let lhs = 2 * (ni + nl + 1 + k);
            let d0 = r.varint()?;
            let d1 = r.varint()?;
            let a = lhs.checked_sub(d0).ok_or_else(|| anyhow!("bad AND delta"))?;
            let b = a.checked_sub(d1).ok_or_else(|| anyhow!("bad AND delta"))?;
            g.raw_and(a, b);
        }
    } else {
        for k in 0..na {
            let w: Vec<Lit> = r
                .line()?
                .split_whitespace()
                .map(|s| s.parse::<Lit>())
                .collect::<std::result::Result<_, std::num::ParseIntError>>()?;
            let lhs = 2 * (ni + nl + 1 + k);
            if w.len() != 3 || w[0] != lhs || w[1] >= lhs || w[2] >= lhs {
                bail!("ASCII AIGER ANDs must be numbered in order with smaller inputs");
            }
            g.raw_and(w[1], w[2]);
        }
    }
    // Symbols: i<k> name, l<k> name, b<k> name.
    let mut bad_names: Vec<(usize, String)> = Vec::new();
    while let Ok(line) = r.line() {
        if line.starts_with('c') {
            break;
        }
        if let Some((tag, name)) = line.split_once(' ') {
            let (kind, idx) = tag.split_at(1);
            if let Ok(i) = idx.parse::<u32>() {
                let v = match kind {
                    "i" => Some(1 + i),
                    "l" => Some(first_latch + i),
                    "b" => {
                        bad_names.push((i as usize, name.to_string()));
                        None
                    }
                    _ => None,
                };
                if let Some(v) = v {
                    g.names.insert(v, name.to_string());
                }
            }
        }
    }
    g.bads = if bads.is_empty() { outs } else { bads };
    g.bad_names = vec![String::new(); g.bads.len()];
    for (k, n) in bad_names {
        if let Some(slot) = g.bad_names.get_mut(k) {
            *slot = n;
        }
    }
    g.constraints = cons;
    let maxv = g.num_vars();
    for l in g.bads.iter().chain(&g.constraints).chain(g.latches.iter().map(|l| &l.next)) {
        if l >> 1 >= maxv {
            bail!("literal {l} out of range");
        }
    }
    Ok(g)
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn line(&mut self) -> Result<String> {
        if self.pos >= self.b.len() {
            bail!("unexpected end of AIGER file");
        }
        let end = self.b[self.pos..].iter().position(|&c| c == b'\n').map_or(self.b.len(), |e| self.pos + e);
        let s = String::from_utf8_lossy(&self.b[self.pos..end]).into_owned();
        self.pos = end + 1;
        Ok(s)
    }
    fn lit_line(&mut self) -> Result<Lit> {
        Ok(self.line()?.trim().parse()?)
    }
    fn varint(&mut self) -> Result<u32> {
        let mut x: u32 = 0;
        let mut shift = 0;
        loop {
            let c = *self.b.get(self.pos).ok_or_else(|| anyhow!("truncated AND section"))?;
            self.pos += 1;
            x |= ((c & 0x7f) as u32) << shift;
            if c & 0x80 == 0 {
                return Ok(x);
            }
            shift += 7;
        }
    }
}

/// The symbol table: `i<k>`, `l<k>` and `b<k>` lines for the inputs, latches and bad
/// states that have a name.
fn symbols(g: &Aig) -> String {
    let mut s = String::new();
    for (k, &i) in g.inputs.iter().enumerate() {
        if let Some(n) = g.names.get(&(i >> 1)) {
            s += &format!("i{k} {n}\n");
        }
    }
    for (k, l) in g.latches.iter().enumerate() {
        if let Some(n) = g.names.get(&(l.lit >> 1)) {
            s += &format!("l{k} {n}\n");
        }
    }
    for (k, n) in g.bad_names.iter().enumerate().filter(|(_, n)| !n.is_empty()) {
        s += &format!("b{k} {n}\n");
    }
    s
}

/// ASCII AIGER 1.9 text. Latches with an [`Init::Expr`] reset are written
/// as uninitialized (AIGER cannot express them).
pub fn write_ascii(g: &Aig) -> String {
    // Renumber: inputs, latches, ands.
    let mut map = vec![0u32; g.nodes.len()];
    let mut next = 1;
    for &i in &g.inputs {
        map[(i >> 1) as usize] = next;
        next += 1;
    }
    for l in &g.latches {
        map[(l.lit >> 1) as usize] = next;
        next += 1;
    }
    for (v, n) in g.nodes.iter().enumerate() {
        if matches!(n, Node::And(..)) {
            map[v] = next;
            next += 1;
        }
    }
    let m = |l: Lit| 2 * map[(l >> 1) as usize] + (l & 1);
    let na = g.num_ands();
    let mut s = format!(
        "aag {} {} {} 0 {} {} {}\n",
        next - 1,
        g.inputs.len(),
        g.latches.len(),
        na,
        g.bads.len(),
        g.constraints.len()
    );
    for &i in &g.inputs {
        s += &format!("{}\n", m(i));
    }
    for l in &g.latches {
        let r = match l.init {
            Init::Zero => "0".to_string(),
            Init::One => "1".to_string(),
            Init::Free | Init::Expr(_) => m(l.lit).to_string(),
        };
        s += &format!("{} {} {}\n", m(l.lit), m(l.next), r);
    }
    for &b in &g.bads {
        s += &format!("{}\n", m(b));
    }
    for &c in &g.constraints {
        s += &format!("{}\n", m(c));
    }
    for (v, n) in g.nodes.iter().enumerate() {
        if let Node::And(a, b) = *n {
            s += &format!("{} {} {}\n", 2 * map[v], m(a), m(b));
        }
    }
    s += &symbols(g);
    s
}

/// Binary AIGER 1.9 (the format ABC and rIC3 read fastest). Latches with
/// an [`Init::Expr`] reset are written as uninitialized.
pub fn write_binary(g: &Aig) -> Vec<u8> {
    let mut map = vec![0u32; g.nodes.len()];
    let mut next = 1;
    for &i in &g.inputs {
        map[(i >> 1) as usize] = next;
        next += 1;
    }
    for l in &g.latches {
        map[(l.lit >> 1) as usize] = next;
        next += 1;
    }
    for (v, n) in g.nodes.iter().enumerate() {
        if matches!(n, Node::And(..)) {
            map[v] = next;
            next += 1;
        }
    }
    let m = |l: Lit| 2 * map[(l >> 1) as usize] + (l & 1);
    let mut out = format!(
        "aig {} {} {} 0 {} {} {}\n",
        next - 1,
        g.inputs.len(),
        g.latches.len(),
        g.num_ands(),
        g.bads.len(),
        g.constraints.len()
    )
    .into_bytes();
    for l in &g.latches {
        let r = match l.init {
            Init::Zero => String::new(),
            Init::One => " 1".into(),
            Init::Free | Init::Expr(_) => format!(" {}", m(l.lit)),
        };
        out.extend(format!("{}{}\n", m(l.next), r).bytes());
    }
    for &b in g.bads.iter().chain(&g.constraints) {
        out.extend(format!("{}\n", m(b)).bytes());
    }
    let varint = |out: &mut Vec<u8>, mut x: u32| {
        while x >= 0x80 {
            out.push((x as u8 & 0x7f) | 0x80);
            x >>= 7;
        }
        out.push(x as u8);
    };
    for (v, n) in g.nodes.iter().enumerate() {
        if let Node::And(a, b) = *n {
            let lhs = 2 * map[v];
            let (x, y) = (m(a), m(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            varint(&mut out, lhs - hi);
            varint(&mut out, hi - lo);
        }
    }
    out.extend(symbols(g).bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lit_val, neg};

    #[test]
    fn binary_round_trip_keeps_behaviour() {
        let mut g = Aig::new();
        let (a, b) = (g.input(), g.input());
        let l = g.latch(Init::One);
        let x = g.xor(a, l);
        let n = g.and(x, neg(b));
        g.set_next(l, n);
        let bad = g.and(l, b);
        g.bads.push(bad);
        g.constraints.push(neg(a));
        let r = read(&write_binary(&g)).unwrap();
        assert_eq!((r.inputs.len(), r.latches.len(), r.bads.len(), r.constraints.len()), (2, 1, 1, 1));
        assert_eq!(r.latches[0].init, Init::One);
        for m in 0..8u32 {
            let val = |g: &Aig, l: Lit| {
                let vals = g.eval(|x| {
                    let k = g.inputs.iter().position(|&i| i == x).map(|p| m >> p & 1 == 1);
                    k.unwrap_or(m & 4 == 4)
                });
                lit_val(&vals, l)
            };
            assert_eq!(val(&g, g.latches[0].next), val(&r, r.latches[0].next));
            assert_eq!(val(&g, g.bads[0]), val(&r, r.bads[0]));
        }
    }
}
