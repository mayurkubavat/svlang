//! Bit-blasting Btor2 into an AIG: states become latches (one per bit),
//! inputs become inputs, `bad` and `constraint` lines carry over. Arrays
//! are not supported at bit level; `justice` and `fair` lines carry over.

use crate::err::{anyhow, bail, Result};
use crate::model::{Line, Model, Op, Sort};
use crate::words::Word;
use crate::{neg, Aig, Init, Lit, FALSE, TRUE};
use std::collections::HashMap;

/// Bit-blast a Btor2 model.
pub(crate) fn from_btor2(m: &Model) -> Result<Aig> {
    let bb = Blaster { m };
    let mut g = Aig::new();
    let mut f: HashMap<u32, Word> = HashMap::new();
    // Latches first: init and next expressions may refer to any line.
    for l in m.lines() {
        if let Some(s) = l.sort {
            if matches!(m.sort(s), Some(Sort::Array { .. })) {
                bail!("arrays are not supported at bit level yet");
            }
        }
        if l.op == Op::State {
            let w = bb.width(l)?;
            let bits: Word = (0..w).map(|_| g.latch(Init::Free)).collect();
            for (i, &b) in bits.iter().enumerate() {
                let name = l.symbol.clone().unwrap_or_else(|| format!("s{}", l.id));
                g.names.insert(b >> 1, if w == 1 { name } else { format!("{name}[{i}]") });
            }
            f.insert(l.id, bits);
        }
    }
    for l in m.lines() {
        let w = match l.op {
            Op::State => continue,
            Op::Input => {
                let w = bb.width(l)?;
                let bits: Word = (0..w).map(|_| g.input()).collect();
                let name = l.symbol.clone().unwrap_or_else(|| format!("i{}", l.id));
                for (i, &b) in bits.iter().enumerate() {
                    g.names.insert(b >> 1, if w == 1 { name.clone() } else { format!("{name}[{i}]") });
                }
                bits
            }
            Op::Init => {
                let s = f[&(l.args[0] as u32)].clone();
                let v = bb.get(&f, &g, l.args[1])?;
                for (b, x) in s.iter().zip(v) {
                    let init = match x {
                        FALSE => Init::Zero,
                        TRUE => Init::One,
                        e => Init::Expr(e),
                    };
                    g.latches.iter_mut().find(|q| q.lit == *b).unwrap().init = init;
                }
                continue;
            }
            Op::Next => {
                let s = f[&(l.args[0] as u32)].clone();
                let v = bb.get(&f, &g, l.args[1])?;
                for (b, x) in s.iter().zip(v) {
                    g.set_next(*b, x);
                }
                continue;
            }
            Op::Bad => {
                let b = bb.get(&f, &g, l.args[0])?[0];
                g.bads.push(b);
                g.bad_names.push(l.symbol.clone().unwrap_or_default());
                continue;
            }
            Op::Constraint => {
                let c = bb.get(&f, &g, l.args[0])?[0];
                g.constraints.push(c);
                continue;
            }
            Op::Output => {
                if let Some(name) = &l.symbol {
                    let bits = bb.get(&f, &g, l.args[0])?;
                    g.outputs.push((name.clone(), bits));
                }
                continue;
            }
            Op::Justice => {
                let mut set = Vec::with_capacity(l.args.len());
                for &a in &l.args {
                    set.push(bb.get(&f, &g, a)?[0]);
                }
                g.justice.push(set);
                g.justice_names.push(l.symbol.clone().unwrap_or_default());
                continue;
            }
            Op::Fair => {
                let c = bb.get(&f, &g, l.args[0])?[0];
                g.fairness.push(c);
                continue;
            }
            _ => bb.encode(&mut g, &f, l)?,
        };
        f.insert(l.id, w);
    }
    // A state with no `next` keeps a free value every cycle: model it as a
    // latch fed by a fresh input.
    let missing: Vec<Lit> = {
        let has: std::collections::HashSet<u32> = m.with_op(Op::Next).map(|l| l.args[0] as u32).collect();
        m.with_op(Op::State).filter(|l| !has.contains(&l.id)).flat_map(|l| f[&l.id].clone()).collect()
    };
    for b in missing {
        let i = g.input();
        g.set_next(b, i);
    }
    Ok(g)
}

struct Blaster<'m> {
    m: &'m Model,
}

impl Blaster<'_> {
    fn width(&self, l: &Line) -> Result<u32> {
        let s = l.sort.ok_or_else(|| anyhow!("line {} has no sort", l.id))?;
        self.m.width(s).ok_or_else(|| anyhow!("line {} is not a bit-vector", l.id))
    }

    fn get(&self, f: &HashMap<u32, Word>, g: &Aig, a: i64) -> Result<Word> {
        let w =
            f.get(&(a.unsigned_abs() as u32)).ok_or_else(|| anyhow!("node {} used before it is defined", a.abs()))?;
        Ok(if a < 0 { g.not_w(w) } else { w.clone() })
    }

    fn encode(&self, g: &mut Aig, f: &HashMap<u32, Word>, l: &Line) -> Result<Word> {
        let w = self.width(l)? as usize;
        let arg = |i: usize| -> Result<Word> { self.get(f, g, l.args[i]) };
        let num = |i: usize| -> Result<usize> {
            l.nums.get(i).and_then(|s| s.parse().ok()).ok_or_else(|| anyhow!("line {}: bad number", l.id))
        };
        let a = if !l.args.is_empty() { Some(arg(0)?) } else { None };
        let b = if l.args.len() > 1 { Some(arg(1)?) } else { None };
        let c = if l.args.len() > 2 { Some(arg(2)?) } else { None };
        let (a_, b_) = (a.as_ref(), b.as_ref());
        let a1 = || a_.unwrap();
        let b1 = || b_.unwrap();
        let bit = |x: Lit1| vec![x];
        type Lit1 = Lit;
        Ok(match l.op {
            Op::Zero => g.konst((0..w).map(|_| false)),
            Op::One => g.konst((0..w).map(|i| i == 0)),
            Op::Ones => g.konst((0..w).map(|_| true)),
            Op::Const => {
                let s = &l.nums[0];
                g.konst((0..w).map(|i| s.as_bytes().get(s.len().wrapping_sub(1 + i)) == Some(&b'1')))
            }
            Op::Constd => g.konst(decimal_bits(&l.nums[0], w)?),
            Op::Consth => g.konst(hex_bits(&l.nums[0], w)?),
            Op::Sext | Op::Uext => {
                let mut v = a1().clone();
                let fill = if l.op == Op::Sext { *v.last().unwrap() } else { FALSE };
                v.resize(w, fill);
                v
            }
            Op::Slice => {
                let (hi, lo) = (num(0)?, num(1)?);
                let x = a1();
                if lo > hi || hi >= x.len() {
                    bail!("slice [{hi}:{lo}] out of range for a {}-bit operand", x.len());
                }
                x[lo..=hi].to_vec()
            }
            Op::Not => g.not_w(a1()),
            Op::Inc => {
                let one = g.konst((0..w).map(|i| i == 0));
                g.add(a1(), &one)
            }
            Op::Dec => {
                let one = g.konst((0..w).map(|i| i == 0));
                g.sub_c(a1(), &one).0
            }
            Op::Neg => g.neg(a1()),
            Op::Redand => bit(g.and_all(a1())),
            Op::Redor => bit(g.or_all(a1())),
            Op::Redxor => bit(a1().iter().fold(FALSE, |acc, &x| g.xor(acc, x))),
            Op::Iff | Op::Xnor => g.zip(a1(), b1(), |g, x, y| neg(g.xor(x, y))),
            Op::Implies => g.zip(a1(), b1(), |g, x, y| g.or(neg(x), y)),
            Op::Eq => bit(g.eq_w(a1(), b1())),
            Op::Neq => bit(neg(g.eq_w(a1(), b1()))),
            Op::Ult => bit(g.ult(a1(), b1())),
            Op::Ulte => bit(neg(g.ult(b1(), a1()))),
            Op::Ugt => bit(g.ult(b1(), a1())),
            Op::Ugte => bit(neg(g.ult(a1(), b1()))),
            Op::Slt => bit(g.slt(a1(), b1())),
            Op::Slte => bit(neg(g.slt(b1(), a1()))),
            Op::Sgt => bit(g.slt(b1(), a1())),
            Op::Sgte => bit(neg(g.slt(a1(), b1()))),
            Op::And => g.zip(a1(), b1(), Aig::and),
            Op::Nand => g.zip(a1(), b1(), |g, x, y| neg(g.and(x, y))),
            Op::Or => g.zip(a1(), b1(), Aig::or),
            Op::Nor => g.zip(a1(), b1(), |g, x, y| neg(g.or(x, y))),
            Op::Xor => g.zip(a1(), b1(), Aig::xor),
            Op::Add => g.add(a1(), b1()),
            Op::Sub => g.sub_c(a1(), b1()).0,
            Op::Mul => g.mul(a1(), b1()),
            Op::Udiv => g.udivrem(a1(), b1()).0,
            Op::Urem => g.udivrem(a1(), b1()).1,
            Op::Sdiv => g.sdiv(a1(), b1()),
            Op::Srem => g.srem(a1(), b1()),
            Op::Smod => g.smod(a1(), b1()),
            Op::Sll => g.shift(a1(), b1(), true, FALSE),
            Op::Srl => g.shift(a1(), b1(), false, FALSE),
            Op::Sra => {
                let s = *a1().last().unwrap();
                g.shift(a1(), b1(), false, s)
            }
            Op::Rol => g.rotate(a1(), b1(), true),
            Op::Ror => g.rotate(a1(), b1(), false),
            Op::Concat => {
                // `concat a b`: a is the high part.
                let mut v = b1().clone();
                v.extend_from_slice(a1());
                v
            }
            Op::Ite => {
                let c = c.as_ref().unwrap();
                g.ite_w(a1()[0], b1(), c)
            }
            Op::Uaddo => bit(g.add_c(a1(), b1(), FALSE).1),
            Op::Saddo => {
                let s = g.add(a1(), b1());
                let (sa, sb, ss) = (*a1().last().unwrap(), *b1().last().unwrap(), *s.last().unwrap());
                let same = neg(g.xor(sa, sb));
                let flip = g.xor(ss, sa);
                bit(g.and(same, flip))
            }
            Op::Usubo => bit(g.ult(a1(), b1())),
            Op::Ssubo => {
                let s = g.sub_c(a1(), b1()).0;
                let (sa, sb, ss) = (*a1().last().unwrap(), *b1().last().unwrap(), *s.last().unwrap());
                let differ = g.xor(sa, sb);
                let flip = g.xor(ss, sa);
                bit(g.and(differ, flip))
            }
            Op::Umulo => {
                let n = a1().len();
                let mut x = a1().clone();
                let mut y = b1().clone();
                x.resize(2 * n, FALSE);
                y.resize(2 * n, FALSE);
                let p = g.mul(&x, &y);
                bit(g.or_all(&p[n..]))
            }
            Op::Smulo => {
                let n = a1().len();
                let (sa, sb) = (*a1().last().unwrap(), *b1().last().unwrap());
                let mut x = a1().clone();
                let mut y = b1().clone();
                x.resize(2 * n, sa);
                y.resize(2 * n, sb);
                let p = g.mul(&x, &y);
                // Overflow unless bits n-1 .. 2n-1 all equal.
                let top = p[n - 1];
                let diffs: Vec<_> = p[n..].iter().map(|&q| g.xor(q, top)).collect();
                bit(g.or_all(&diffs))
            }
            Op::Udivo => bit(FALSE),
            Op::Sdivo => {
                let n = a1().len();
                let min = g.konst((0..n).map(|i| i == n - 1));
                let ones = g.konst((0..n).map(|_| true));
                let x = g.eq_w(a1(), &min);
                let y = g.eq_w(b1(), &ones);
                bit(g.and(x, y))
            }
            Op::Read | Op::Write => bail!("arrays are not supported by this engine yet"),
            op => bail!("unexpected operator {} in expression", op.name()),
        })
    }
}

/// Bits (LSB first) of a decimal literal, two's complement for negatives.
fn decimal_bits(s: &str, w: usize) -> Result<Vec<bool>> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(d) => (true, d),
        None => (false, s),
    };
    let mut mag = vec![false; w];
    for ch in digits.chars() {
        // mag = mag * 10 + d (mod 2^w), bit by bit with a carry
        let mut carry = ch.to_digit(10).ok_or_else(|| anyhow!("bad decimal constant {s}"))? as u64;
        for b in mag.iter_mut() {
            let v = *b as u64 * 10 + carry;
            *b = v & 1 == 1;
            carry = v >> 1;
        }
    }
    if neg {
        let mut carry = true;
        for b in mag.iter_mut() {
            let v = !*b;
            *b = v ^ carry;
            carry = v && carry;
        }
    }
    Ok(mag)
}

fn hex_bits(s: &str, w: usize) -> Result<Vec<bool>> {
    let mut bits = Vec::with_capacity(s.len() * 4);
    for ch in s.chars().rev() {
        let d = ch.to_digit(16).ok_or_else(|| anyhow!("bad hex constant {s}"))?;
        for i in 0..4 {
            bits.push((d >> i) & 1 == 1);
        }
    }
    bits.resize(w, false);
    Ok(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn val(bits: &[bool]) -> u64 {
        bits.iter().enumerate().map(|(i, &b)| (b as u64) << i).sum()
    }

    #[test]
    fn constants() {
        assert_eq!(val(&decimal_bits("1234", 16).unwrap()), 1234);
        assert_eq!(val(&decimal_bits("-1", 8).unwrap()), 255);
        assert_eq!(val(&decimal_bits("300", 8).unwrap()), 300 % 256);
        assert_eq!(val(&hex_bits("ff", 12).unwrap()), 255);
    }

    #[test]
    fn rejects_bad_slices_and_huge_references() {
        // slice [2:5] (lo > hi) and [9:0] (past the width) are errors,
        // not panics.
        for line in ["4 slice 3 2 2 5", "4 slice 3 2 9 0"] {
            let text = format!("1 sort bitvec 8\n2 input 1\n3 sort bitvec 4\n{line}\n");
            let m = crate::parse::parse(&text);
            if let Ok(m) = m {
                assert!(from_btor2(&m).is_err(), "{line}");
            }
        }
        // A reference past u32 must not wrap onto node 1.
        let big = format!("1 sort bitvec 1\n2 input 1\n3 not 1 {}\n", (1u64 << 32) + 2);
        assert!(crate::parse::parse(&big).is_err());
    }
}
