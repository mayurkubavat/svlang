//! Bit-vector operators as AIG circuits. Words are `Vec<Lit>`, least
//! significant bit first. Division follows Btor2 (SMT-LIB): x / 0 is all
//! ones and x % 0 is x.

use crate::{neg, Aig, Lit, FALSE, TRUE};

pub type Word = Vec<Lit>;

impl Aig {
    pub fn konst(&self, bits: impl IntoIterator<Item = bool>) -> Word {
        bits.into_iter().map(|b| if b { TRUE } else { FALSE }).collect()
    }

    pub fn not_w(&self, a: &Word) -> Word {
        a.iter().map(|&l| neg(l)).collect()
    }

    pub fn zip(&mut self, a: &Word, b: &Word, f: fn(&mut Aig, Lit, Lit) -> Lit) -> Word {
        a.iter().zip(b).map(|(&x, &y)| f(self, x, y)).collect()
    }

    pub fn ite_w(&mut self, c: Lit, a: &Word, b: &Word) -> Word {
        a.iter().zip(b).map(|(&x, &y)| self.ite(c, x, y)).collect()
    }

    pub fn eq_w(&mut self, a: &Word, b: &Word) -> Lit {
        let bits: Vec<Lit> = a.iter().zip(b).map(|(&x, &y)| neg(self.xor(x, y))).collect();
        self.and_all(&bits)
    }

    /// Sum and carry out.
    pub fn add_c(&mut self, a: &Word, b: &Word, mut carry: Lit) -> (Word, Lit) {
        let mut out = Vec::with_capacity(a.len());
        for (&x, &y) in a.iter().zip(b) {
            let p = self.xor(x, y);
            out.push(self.xor(p, carry));
            let g = self.and(x, y);
            let pc = self.and(p, carry);
            carry = self.or(g, pc);
        }
        (out, carry)
    }

    pub fn add(&mut self, a: &Word, b: &Word) -> Word {
        self.add_c(a, b, FALSE).0
    }

    /// a - b, and whether a >= b (unsigned; the carry out of a + !b + 1).
    pub fn sub_c(&mut self, a: &Word, b: &Word) -> (Word, Lit) {
        let nb = self.not_w(b);
        self.add_c(a, &nb, TRUE)
    }

    pub fn neg(&mut self, a: &Word) -> Word {
        let z = self.konst(a.iter().map(|_| false));
        self.sub_c(&z, a).0
    }

    pub fn ult(&mut self, a: &Word, b: &Word) -> Lit {
        neg(self.sub_c(a, b).1)
    }

    pub fn slt(&mut self, a: &Word, b: &Word) -> Lit {
        // Flip the sign bits and compare unsigned.
        let (mut a, mut b) = (a.clone(), b.clone());
        let n = a.len() - 1;
        a[n] = neg(a[n]);
        b[n] = neg(b[n]);
        self.ult(&a, &b)
    }

    pub fn mul(&mut self, a: &Word, b: &Word) -> Word {
        let w = a.len();
        let mut acc = self.konst((0..w).map(|_| false));
        for (i, &bi) in b.iter().enumerate() {
            let mut row = self.konst((0..i).map(|_| false));
            for &aj in a.iter().take(w - i) {
                row.push(self.and(aj, bi));
            }
            acc = self.add(&acc, &row);
        }
        acc
    }

    /// Restoring division: quotient and remainder. Division by zero gives
    /// all-ones and the dividend, as Btor2 (SMT-LIB) defines.
    pub fn udivrem(&mut self, a: &Word, b: &Word) -> (Word, Word) {
        let w = a.len();
        let mut b1 = b.clone();
        b1.push(FALSE);
        let mut r: Word = vec![FALSE; w + 1];
        let mut q = vec![FALSE; w];
        for i in (0..w).rev() {
            r.insert(0, a[i]);
            r.truncate(w + 1);
            let (diff, ge) = self.sub_c(&r, &b1);
            q[i] = ge;
            r = self.ite_w(ge, &diff, &r);
        }
        r.truncate(w);
        (q, r)
    }

    fn abs(&mut self, a: &Word) -> Word {
        let s = *a.last().unwrap();
        let n = self.neg(a);
        self.ite_w(s, &n, a)
    }

    pub fn sdiv(&mut self, a: &Word, b: &Word) -> Word {
        let (sa, sb) = (*a.last().unwrap(), *b.last().unwrap());
        let (ua, ub) = (self.abs(a), self.abs(b));
        let q = self.udivrem(&ua, &ub).0;
        let s = self.xor(sa, sb);
        let nq = self.neg(&q);
        self.ite_w(s, &nq, &q)
    }

    pub fn srem(&mut self, a: &Word, b: &Word) -> Word {
        let sa = *a.last().unwrap();
        let (ua, ub) = (self.abs(a), self.abs(b));
        let r = self.udivrem(&ua, &ub).1;
        let nr = self.neg(&r);
        self.ite_w(sa, &nr, &r)
    }

    pub fn smod(&mut self, a: &Word, b: &Word) -> Word {
        let (sa, sb) = (*a.last().unwrap(), *b.last().unwrap());
        let (ua, ub) = (self.abs(a), self.abs(b));
        let u = self.udivrem(&ua, &ub).1;
        let nu = self.neg(&u);
        let zero = self.konst(u.iter().map(|_| false));
        let is_zero = self.eq_w(&u, &zero);
        let nu_plus_b = self.add(&nu, b);
        let u_plus_b = self.add(&u, b);
        // s>=0,t>=0: u; s<0,t>=0: -u+t; s>=0,t<0: u+t; s<0,t<0: -u
        let pos_t = self.ite_w(sa, &nu_plus_b, &u);
        let neg_t = self.ite_w(sa, &nu, &u_plus_b);
        let r = self.ite_w(sb, &neg_t, &pos_t);
        self.ite_w(is_zero, &zero, &r)
    }

    /// Shift by a word amount; `fill` enters from the vacated side.
    pub fn shift(&mut self, a: &Word, b: &Word, left: bool, fill: Lit) -> Word {
        let w = a.len();
        let mut cur = a.clone();
        let mut over = Vec::new();
        for (i, &bi) in b.iter().enumerate() {
            let step = 1usize.checked_shl(i as u32).unwrap_or(usize::MAX);
            if step >= w {
                over.push(bi);
                continue;
            }
            let shifted: Word = (0..w)
                .map(|j| {
                    if left {
                        if j >= step {
                            cur[j - step]
                        } else {
                            fill
                        }
                    } else if j + step < w {
                        cur[j + step]
                    } else {
                        fill
                    }
                })
                .collect();
            cur = self.ite_w(bi, &shifted, &cur);
        }
        let o = self.or_all(&over);
        let all = vec![fill; w];
        self.ite_w(o, &all, &cur)
    }

    pub fn rotate(&mut self, a: &Word, b: &Word, left: bool) -> Word {
        let w = a.len();
        let wk = self.konst((0..b.len()).map(|i| i < 64 && (w as u64 >> i) & 1 == 1));
        let amt = self.udivrem(b, &wk).1;
        let one = self.shift(a, &amt, left, FALSE);
        let back = self.sub_c(&wk, &amt).0;
        let two = self.shift(a, &back, !left, FALSE);
        // amt == 0 shifts the second half by w, which yields 0: fine.
        self.zip(&one, &two, Aig::or)
    }
}
