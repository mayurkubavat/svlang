//! Constant values: the four-state integer [`CVal`] of any width with its
//! IEEE 1800 operators, and the size limits elaboration and binding share.

use crate::bigint::Int;

/// Four-state logic value of a single bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BitState {
    /// Logic `0`.
    Zero,
    /// Logic `1`.
    One,
    /// Unknown `x`.
    X,
    /// High-impedance `z`.
    Z,
}

/// A constant integer: its width, signedness and four-state value, of any
/// width. `v` is kept normalized: below 2^w when unsigned, and in
/// -2^(w-1) .. 2^(w-1) when signed. `u` marks unknown bits (below 2^w);
/// at those bits `v` holds 0 for x and 1 for z, as in slang.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CVal {
    /// Bit width (`>= 1` for sized values).
    pub w: u32,
    /// Whether the constant is signed (`2'sb11`).
    pub s: bool,
    /// Value bits (`0`/`1` when known; `0` for `x` and `1` for `z` where `u` is `1`).
    pub v: Int,
    /// Unknown mask (`1` where the bit is `x` or `z`).
    pub u: Int,
}

/// Maximum packed vector bit width accepted during elaboration and binding.
#[doc(hidden)]
pub const MAX_BIT_WIDTH: u32 = 1 << 24;

/// Maximum element count accepted for unpacked dimensions, replications, and patterns.
#[doc(hidden)]
pub const MAX_ARRAY_ELEMS: u32 = 1 << 20;

/// Maximum cycle count accepted in SVA delay and repetition bounds.
#[doc(hidden)]
pub const MAX_SVA_CYCLES: u32 = 1 << 20;

/// Computes `|a - b| + 1` as a `u32` bounded by `max_len`, returning `None` on overflow.
#[doc(hidden)]
pub fn range_len_u32(a: i128, b: i128, max_len: u32) -> Option<u32> {
    let span = a.abs_diff(b).checked_add(1)?;
    let cnt = u32::try_from(span).ok()?;
    (cnt <= max_len).then_some(cnt)
}

/// slang abbreviates a value with more active bits than this.
const ABBREVIATE_BITS: u32 = 128;

impl CVal {
    /// Constructs a two-state constant of width `w` and signedness `s`, normalized to `w` bits.
    pub fn new(w: u32, s: bool, v: impl Into<Int>) -> CVal {
        CVal { w, s, v: v.into(), u: Int::from(0) }.norm()
    }

    /// Every bit x.
    pub fn all_x(w: u32, s: bool) -> CVal {
        CVal { w, s, v: Int::from(0), u: Int::from(-1) }.norm()
    }

    /// Every bit z.
    pub fn all_z(w: u32, s: bool) -> CVal {
        CVal { w, s, v: Int::from(-1), u: Int::from(-1) }.norm()
    }

    /// Four-state logic value of bit `i` (`0` = LSB).
    pub fn bit_state(&self, i: u32) -> BitState {
        if i >= self.w {
            return BitState::Zero;
        }
        let v = self.bits_v().bit(i);
        let u = self.u.bit(i);
        match (u, v) {
            (false, false) => BitState::Zero,
            (false, true) => BitState::One,
            (true, false) => BitState::X,
            (true, true) => BitState::Z,
        }
    }

    /// Parses a standalone SystemVerilog integer or string literal (`8'hff`, `-4`, `'x`, `"abc"`).
    pub fn parse_literal(raw: &str) -> Option<CVal> {
        let s = raw.trim();
        if s.starts_with('"') {
            return string_value(s);
        }
        if let Some(rest) = s.strip_prefix('-') {
            return unary("-", Self::parse_literal(rest)?);
        }
        if let Some(rest) = s.strip_prefix('+') {
            return Self::parse_literal(rest);
        }
        literal(&[s])
    }

    /// Evaluates a binary operator on two 4-state constants using IEEE 1800 rules.
    pub fn eval_binary(op: &str, a: CVal, b: CVal) -> Option<CVal> {
        if matches!(op, "==?" | "!=?") {
            let w = a.w.max(b.w);
            let s = a.s && b.s;
            let (ca, cb) = (a.cast(w, s), b.cast(w, s));
            // IEEE 11.4.6: unknown bits (x/z/?) in RHS act as don't-care wildcards;
            // unknown bits in LHS at non-wildcard positions make the comparison 1'bx unless
            // another non-wildcard bit mismatches!
            let mask = cb.u.not().trunc(w, false);
            let diff_known = ca.bits_v().xor(&cb.bits_v()).and(&mask).and(&ca.u.not());
            if !diff_known.is_zero() {
                return Some(CVal::bool(op == "!=?"));
            }
            let lhs_unk = ca.u.and(&mask);
            if !lhs_unk.is_zero() {
                return Some(CVal::all_x(1, false));
            }
            return Some(CVal::bool(op == "==?"));
        }
        binary(op, a, b)
    }

    /// Evaluates a unary or reduction operator on a 4-state constant.
    pub fn eval_unary(op: &str, a: CVal) -> Option<CVal> {
        if let Some(r) = unary(op, a.clone()) {
            return Some(r);
        }
        let w = a.w.max(1);
        let full = Int::from(-1).trunc(w, false);
        let a1 = a.ones();
        let a0 = a.bits_v().or(&a.u).not().trunc(w, false);
        match op {
            "&" | "~&" => {
                let res = if !a0.is_zero() {
                    CVal::bool(false)
                } else if a1 == full {
                    CVal::bool(true)
                } else {
                    CVal::all_x(1, false)
                };
                Some(if op == "~&" { unary("!", res)? } else { res })
            }
            "|" | "~|" => {
                let res = if !a1.is_zero() {
                    CVal::bool(true)
                } else if a0 == full {
                    CVal::bool(false)
                } else {
                    CVal::all_x(1, false)
                };
                Some(if op == "~|" { unary("!", res)? } else { res })
            }
            "^" | "~^" | "^~" => {
                if !a.known() {
                    return Some(CVal::all_x(1, false));
                }
                let mut parity = false;
                for i in 0..w {
                    parity ^= a1.bit(i);
                }
                if op != "^" {
                    parity = !parity;
                }
                Some(CVal::bool(parity))
            }
            _ => None,
        }
    }

    /// Value and unknown bits as unsigned integers of the width.
    pub fn four(w: u32, s: bool, v: Int, u: Int) -> CVal {
        let u = u.trunc(w.max(1), false);
        CVal { w, s, v, u }.norm()
    }

    /// Returns `true` when every bit is `0` or `1` with no `x` or `z` bits.
    pub fn known(&self) -> bool {
        self.u.is_zero()
    }

    /// The value bits as an unsigned integer of the width.
    pub fn bits_v(&self) -> Int {
        self.v.trunc(self.w.max(1), false)
    }

    /// Known one bits.
    fn ones(&self) -> Int {
        self.bits_v().and(&self.u.not())
    }

    /// True, false, or unknown (x) as a condition.
    pub fn truth(&self) -> Option<bool> {
        if !self.ones().is_zero() {
            Some(true)
        } else if self.known() {
            Some(false)
        } else {
            None
        }
    }

    /// Bits where `a` and `b` agree, x elsewhere (same width and sign).
    pub fn merge(a: &CVal, b: &CVal) -> CVal {
        let (av, bv) = (a.bits_v(), b.bits_v());
        let u = a.u.or(&b.u).or(&av.xor(&bv));
        CVal::four(a.w, a.s, av.and(&u.not()), u)
    }

    /// Bits `lo..lo+w` as a value of width `w`.
    pub fn slice(&self, lo: u32, w: u32, s: bool) -> CVal {
        CVal::four(w, s, self.bits_v().bits(lo, w), self.u.bits(lo, w))
    }

    /// `hi` above `lo` (`{hi, lo}`), unsigned.
    pub fn concat(hi: &CVal, lo: &CVal) -> CVal {
        let w = hi.w + lo.w;
        let v = hi.bits_v().shl(lo.w).or(&lo.bits_v());
        let u = hi.u.shl(lo.w).or(&lo.u);
        CVal::four(w.max(1), false, v, u)
    }

    /// The bits of `x` written at `lo..lo+x.w` of this value.
    pub fn with_bits(&self, lo: u32, x: &CVal) -> CVal {
        let ones = Int::from(1).shl(x.w).sub(&Int::from(1)).shl(lo);
        let keep = ones.not();
        let v = self.bits_v().and(&keep).or(&x.bits_v().trunc(x.w, false).shl(lo));
        let u = self.u.and(&keep).or(&x.u.trunc(x.w, false).shl(lo));
        CVal::four(self.w, self.s, v, u)
    }

    /// Into a two-state type: unknown bits become 0.
    pub fn two_state(&self) -> CVal {
        if self.known() {
            return self.clone();
        }
        CVal::new(self.w, self.s, self.ones())
    }

    /// Constructs a 32-bit signed two-state integer constant (`int`).
    pub fn int(v: i128) -> CVal {
        CVal::new(32, true, v)
    }

    /// Constructs a 1-bit unsigned two-state boolean constant (`1'b0` or `1'b1`).
    pub fn bool(b: bool) -> CVal {
        CVal::new(1, false, b as i128)
    }

    /// The value as an index or count (saturating beyond i128).
    pub fn i(&self) -> i128 {
        self.v.low_i128()
    }

    /// Returns `true` when at least one bit is known `1` under IEEE 1800 condition truth rules.
    pub fn is_true(&self) -> bool {
        self.truth() == Some(true)
    }

    /// Truncated to the width, sign-extended if signed.
    pub fn norm(self) -> CVal {
        let w = self.w.max(1);
        let v = self.v.trunc(w, self.s);
        let u = self.u.trunc(w, false);
        CVal { w: self.w, s: self.s, v, u }
    }

    /// Resizes and reinterprets this constant to width `w` and signedness `s`.
    ///
    /// Widening sign-extends both value and top-bit `x`/`z` states when `self.s` is `true`,
    /// and zero-extends when `self.s` is `false`.
    pub fn cast(&self, w: u32, s: bool) -> CVal {
        // A signed value's unknown top bit extends.
        let u = if self.s && w > self.w { self.u.trunc(self.w.max(1), true) } else { self.u.clone() };
        // `v` is sign-normalized, so x (0) extends as x and z (1) as z.
        CVal { w, s, v: self.v.clone(), u }.norm()
    }

    /// The value plus a small step, in the same type.
    pub fn bump(&self, d: i128) -> CVal {
        if !self.known() {
            return CVal::all_x(self.w, self.s);
        }
        CVal::new(self.w, self.s, self.v.add(&Int::from(d)))
    }

    /// slang's SVInt::writeTo: an optional `-`, the base, the digits;
    /// over 128 active bits, decimal cut to its leading digits with
    /// `...e<n>`.
    fn write(&self, radix: u32, mut with_base: bool) -> String {
        if !self.known() {
            return self.write_unknown(radix);
        }
        let neg = self.s && self.v.is_neg();
        let mag = if neg { self.v.neg() } else { self.v.clone() };
        let mut radix = radix;
        let active = mag.active_bits();
        if active > ABBREVIATE_BITS {
            with_base = true;
            radix = 10;
        }
        let mut out = String::new();
        if neg {
            out.push('-');
        }
        if with_base {
            out.push_str(&self.w.to_string());
            out.push('\'');
            if self.s {
                out.push('s');
            }
            out.push(match radix {
                2 => 'b',
                8 => 'o',
                10 => 'd',
                _ => 'h',
            });
        }
        if radix == 10 && active > ABBREVIATE_BITS {
            let exp = (((active - ABBREVIATE_BITS) as f64 / std::f64::consts::LOG2_10).ceil() as u32).max(5);
            out.push_str(&mag.div_pow10(exp).digits(10));
            out.push_str(&format!("...e{exp}"));
        } else {
            out.push_str(&mag.digits(radix));
        }
        out
    }

    /// slang's SVInt::writeTo for a value with unknown bits: decimal is
    /// one letter, other bases a digit (or x, z, X, Z) per group of bits.
    fn write_unknown(&self, radix: u32) -> String {
        let mut out = format!(
            "{}'{}{}",
            self.w,
            if self.s { "s" } else { "" },
            match radix {
                2 => 'b',
                8 => 'o',
                10 => 'd',
                _ => 'h',
            }
        );
        let w = self.w.max(1);
        let all_u = self.u == Int::from(-1).trunc(w, false);
        let v = self.bits_v();
        if radix == 10 {
            let any_x = !self.u.and(&v.not()).is_zero();
            out.push(if all_u && v == self.u {
                'z'
            } else if all_u && v.is_zero() {
                'x'
            } else if any_x {
                'X'
            } else {
                'Z'
            });
            return out;
        }
        let shift = match radix {
            2 => 1,
            8 => 3,
            _ => 4,
        };
        let mut digits = Vec::new();
        let (mut tv, mut tu) = (v, self.u.clone());
        let mut left = w as i64;
        while !tv.is_zero() || !tu.is_zero() {
            let n = if left < shift as i64 { left.max(0) as u32 } else { shift };
            let mask = (1u32 << n) - 1;
            let d = tv.bits(0, shift).low_i128() as u32 & mask;
            let du = tu.bits(0, shift).low_i128() as u32 & mask;
            digits.push(if du == 0 {
                char::from_digit(d, 16).unwrap_or('?')
            } else if du == mask && d == 0 {
                'x'
            } else if du == mask && d == mask {
                'z'
            } else if !d & du != 0 {
                'X'
            } else {
                'Z'
            });
            tv = tv.shr(shift);
            tu = tu.shr(shift);
            left -= shift as i64;
        }
        if left > 0 && matches!(digits.last(), Some('x' | 'z')) {
            digits.push('0');
        }
        if digits.is_empty() {
            digits.push('0');
        }
        out.extend(digits.iter().rev());
        out
    }

    /// slang's default form: binary below 8 unsigned bits, decimal up to
    /// 32 bits or signed, hex above; a plain number for a signed 32-bit
    /// decimal.
    pub fn show(&self) -> String {
        if !self.known() {
            let w = self.w.max(1);
            let all_u = self.u == Int::from(-1).trunc(w, false);
            let all_xz = all_u && (self.bits_v().is_zero() || self.bits_v() == self.u);
            let radix = if (self.w < 8 && !self.s) || (self.w <= 64 && !all_xz) {
                2
            } else if self.w <= 32 || self.s || all_xz {
                10
            } else {
                16
            };
            return self.write_unknown(radix);
        }
        let radix = if self.w < 8 && !self.s {
            2
        } else if self.w <= 32 || self.s {
            10
        } else {
            16
        };
        self.write(radix, !(radix == 10 && self.w == 32 && self.s))
    }

    /// Always decimal with its size, as enum values print.
    pub fn show_dec(&self) -> String {
        self.write(10, true)
    }
}

/// A string literal as an integral value: 8 bits a character, the first
/// one highest (IEEE 1800 5.9).
#[doc(hidden)]
pub fn string_value(lit: &str) -> Option<CVal> {
    let body = lit
        .strip_prefix("\"\"\"")
        .and_then(|s| s.strip_suffix("\"\"\""))
        .or_else(|| lit.strip_prefix('"').and_then(|s| s.strip_suffix('"')))?;
    let mut bytes = Vec::new();
    let mut it = body.bytes();
    while let Some(c) = it.next() {
        if c != b'\\' {
            bytes.push(c);
            continue;
        }
        bytes.push(match it.next()? {
            b'n' => b'\n',
            b't' => b'\t',
            b'\\' => b'\\',
            b'"' => b'"',
            b'v' => 0x0b,
            b'f' => 0x0c,
            b'a' => 0x07,
            _ => return None,
        });
    }
    let mut v = Int::from(0);
    for c in &bytes {
        v = v.shl(8).or(&Int::from(*c as i128));
    }
    Some(CVal::new((bytes.len().max(1) * 8) as u32, false, v))
}

/// `4'hA`, `12`, `'1`, `2'sb11`, `4'b10x1` as a value.
#[doc(hidden)]
pub fn literal(toks: &[&str]) -> Option<CVal> {
    let joined: String = toks.concat().replace('_', "");
    if let Some(i) = joined.find('\'') {
        let (size, rest) = joined.split_at(i);
        let rest = &rest[1..];
        let (signed, rest) = match rest.strip_prefix(['s', 'S']) {
            Some(r) => (true, r),
            None => (false, rest),
        };
        let mut chars = rest.chars();
        let b = chars.next()?.to_ascii_lowercase();
        let digits: String = chars.collect();
        let radix = match b {
            'b' => 2,
            'o' => 8,
            'd' => 10,
            'h' => 16,
            '0' | '1' if size.is_empty() => {
                // `'0` / `'1`: all bits, here taken as one.
                return Some(CVal::bool(b == '1'));
            }
            'x' if size.is_empty() && rest.len() == 1 => return Some(CVal::all_x(1, false)),
            'z' if size.is_empty() && rest.len() == 1 => {
                return Some(CVal::four(1, false, Int::from(1), Int::from(1)));
            }
            _ => return None,
        };
        let w = if size.is_empty() { 32 } else { size.parse().ok()? };
        let digits = digits.to_ascii_lowercase();
        if !digits.contains(['x', 'z', '?']) {
            let v = Int::parse_radix(&digits, radix)?;
            return Some(CVal::new(w, signed, v));
        }
        return four_state_literal(&digits, radix, w, signed);
    }
    if joined.is_empty() || !joined.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // An unsized decimal is a 32-bit signed integer (wider if it must be).
    let v = Int::parse_radix(&joined, 10)?;
    let w = (v.active_bits() + 1).max(32);
    Some(CVal::new(w, true, v))
}

/// Digits with x, z or ? (IEEE 5.7.1): each such digit stands for all its
/// bits, and a leading one fills the bits above.
fn four_state_literal(digits: &str, radix: u32, w: u32, signed: bool) -> Option<CVal> {
    if radix == 10 {
        // Only a single x or z digit is legal in decimal.
        return match digits {
            "x" => Some(CVal::all_x(w, signed)),
            "z" | "?" => Some(CVal::four(w, signed, Int::from(-1), Int::from(-1))),
            _ => None,
        };
    }
    let per = match radix {
        2 => 1,
        8 => 3,
        _ => 4,
    };
    let (mut v, mut u) = (Int::from(0), Int::from(0));
    let ones = Int::from((1i128 << per) - 1);
    for c in digits.chars() {
        v = v.shl(per);
        u = u.shl(per);
        match c {
            'x' => u = u.or(&ones),
            'z' | '?' => {
                u = u.or(&ones);
                v = v.or(&ones);
            }
            _ => v = v.or(&Int::from(c.to_digit(radix)? as i128)),
        }
    }
    let n = digits.len() as u32 * per;
    if n < w {
        let top = digits.chars().next()?;
        if matches!(top, 'x' | 'z' | '?') {
            let fill = Int::from(-1).shl(n).trunc(w, false);
            u = u.or(&fill);
            if top != 'x' {
                v = v.or(&fill);
            }
        }
    }
    Some(CVal::four(w, signed, v.trunc(w, false), u))
}

/// Extracts the unsigned shift count from `b` per IEEE 1800 §11.4.10, saturating to `lhs_w + 1`.
fn shift_amount(b: &CVal, lhs_w: u32) -> u32 {
    let ub = b.bits_v();
    let limit = u128::from(lhs_w) + 1;
    match ub.to_i128() {
        Some(v) if v >= 0 && (v as u128) <= limit => v as u32,
        _ => lhs_w.saturating_add(1),
    }
}

#[doc(hidden)]
pub fn binary(op: &str, a: CVal, b: CVal) -> Option<CVal> {
    if !a.known() || !b.known() {
        return binary_unknown(op, a, b);
    }
    let w = a.w.max(b.w);
    let s = a.s && b.s;
    let (x, y) = (a.cast(w, s).v, b.cast(w, s).v);
    if matches!(op, "/" | "%") && y.is_zero() {
        return Some(CVal::all_x(w, s));
    }
    let r = match op {
        "+" => x.add(&y),
        "-" => x.sub(&y),
        "*" => x.mul(&y),
        "/" => x.divrem(&y)?.0,
        "%" => x.divrem(&y)?.1,
        "**" => {
            // IEEE 1800 Table 11-4: result has the left operand's width and signedness.
            if b.s && b.v.is_neg() {
                let av = if a.s { a.v.clone() } else { a.bits_v() };
                if av.is_zero() {
                    return Some(CVal::all_x(a.w, a.s));
                }
                if av == Int::from(1) {
                    return Some(CVal::new(a.w, a.s, 1));
                }
                if a.s && av == Int::from(-1) {
                    let r = if b.v.bit(0) { -1 } else { 1 };
                    return Some(CVal::new(a.w, a.s, r));
                }
                return Some(CVal::new(a.w, a.s, 0));
            }
            let exp = b.bits_v();
            if exp.is_zero() {
                return Some(CVal::new(a.w, a.s, 1));
            }
            let w_mod = a.w.max(1);
            if w_mod > 4096 && exp.active_bits() > 16 {
                return None;
            }
            let mut acc = Int::from(1).trunc(w_mod, a.s);
            let mut base = a.v.trunc(w_mod, a.s);
            let bits = exp.active_bits();
            for i in 0..bits {
                if exp.bit(i) {
                    acc = acc.mul(&base).trunc(w_mod, a.s);
                }
                if i + 1 < bits {
                    base = base.mul(&base).trunc(w_mod, a.s);
                }
            }
            return Some(CVal::new(a.w, a.s, acc));
        }
        "&" => x.and(&y),
        "|" => x.or(&y),
        "^" => x.xor(&y),
        "~^" | "^~" => x.xor(&y).not(),
        "<<" | "<<<" => {
            let n = shift_amount(&b, a.w);
            return Some(CVal { v: a.v.shl(n), ..a }.norm());
        }
        ">>" => {
            let n = shift_amount(&b, a.w);
            return Some(CVal { v: a.bits_v().shr(n), ..a }.norm());
        }
        ">>>" => {
            let n = shift_amount(&b, a.w);
            let rv = if a.s { a.v.shr(n) } else { a.bits_v().shr(n) };
            return Some(CVal { v: rv, ..a }.norm());
        }
        "==" | "===" => return Some(CVal::bool(x == y)),
        "!=" | "!==" => return Some(CVal::bool(x != y)),
        "<" => return Some(CVal::bool(x < y)),
        "<=" => return Some(CVal::bool(x <= y)),
        ">" => return Some(CVal::bool(x > y)),
        ">=" => return Some(CVal::bool(x >= y)),
        "&&" => return Some(CVal::bool(a.is_true() && b.is_true())),
        "||" => return Some(CVal::bool(a.is_true() || b.is_true())),
        "->" => return Some(CVal::bool(!a.is_true() || b.is_true())),
        "<->" => return Some(CVal::bool(a.is_true() == b.is_true())),
        _ => return None,
    };
    Some(CVal::new(w, s, r))
}

/// `-a`, `+a`, `~a`, `!a` with four-state bits.
#[doc(hidden)]
pub fn unary(op: &str, a: CVal) -> Option<CVal> {
    Some(match op {
        "-" if !a.known() => CVal::all_x(a.w, a.s),
        "-" => CVal::new(a.w, a.s, a.v.neg()),
        "+" => a,
        "~" => {
            let v = a.bits_v().not().and(&a.u.not());
            CVal::four(a.w, a.s, v, a.u.clone())
        }
        "!" => a.truth().map_or_else(|| CVal::all_x(1, false), |t| CVal::bool(!t)),
        _ => return None,
    })
}

/// `binary` with an operand that has unknown bits (IEEE 11.4): bitwise
/// operators work bit by bit, the rest give x.
fn binary_unknown(op: &str, a: CVal, b: CVal) -> Option<CVal> {
    let w = a.w.max(b.w);
    let s = a.s && b.s;
    let (ca, cb) = (a.cast(w, s), b.cast(w, s));
    let (au, bu) = (&ca.u, &cb.u);
    let (a1, b1) = (ca.ones(), cb.ones());
    // Known zero bits.
    let a0 = ca.bits_v().or(au).not().trunc(w, false);
    let b0 = cb.bits_v().or(bu).not().trunc(w, false);
    let x1 = |op: &str| -> CVal { CVal::all_x(if op.is_empty() { 1 } else { w }, s && !op.is_empty()) };
    Some(match op {
        "&" => {
            let zero = a0.or(&b0);
            let u = au.or(bu).and(&zero.not());
            CVal::four(w, s, a1.and(&b1), u)
        }
        "|" => {
            let one = a1.or(&b1);
            let u = au.or(bu).and(&one.not());
            CVal::four(w, s, one, u)
        }
        "^" | "~^" | "^~" => {
            let u = au.or(bu);
            let mut v = ca.bits_v().xor(&cb.bits_v());
            if op != "^" {
                v = v.not();
            }
            CVal::four(w, s, v.and(&u.not()), u)
        }
        "===" | "!==" => {
            let same = ca.bits_v() == cb.bits_v() && ca.u == cb.u;
            CVal::bool(same == (op == "==="))
        }
        "&&" | "||" => {
            let (ta, tb) = (a.truth(), b.truth());
            let r = if op == "&&" {
                match (ta, tb) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                }
            } else {
                match (ta, tb) {
                    (Some(true), _) | (_, Some(true)) => Some(true),
                    (Some(false), Some(false)) => Some(false),
                    _ => None,
                }
            };
            r.map_or_else(|| x1(""), CVal::bool)
        }
        "->" => match (a.truth(), b.truth()) {
            (Some(false), _) | (_, Some(true)) => CVal::bool(true),
            (Some(true), Some(false)) => CVal::bool(false),
            _ => x1(""),
        },
        "<->" => match (a.truth(), b.truth()) {
            (Some(ta), Some(tb)) => CVal::bool(ta == tb),
            _ => x1(""),
        },
        "==" | "!=" | "<" | "<=" | ">" | ">=" => x1(""),
        "<<" | "<<<" | ">>" | ">>>" if b.known() => {
            let n = shift_amount(&b, a.w);
            let (v, u) = if op.starts_with("<<") {
                (a.v.shl(n), a.u.shl(n))
            } else if op == ">>>" && a.s {
                (a.v.shr(n), a.u.trunc(a.w.max(1), true).shr(n))
            } else {
                (a.bits_v().shr(n), a.u.shr(n))
            };
            CVal::four(a.w, a.s, v, u)
        }
        "**" | "<<" | "<<<" | ">>" | ">>>" => CVal::all_x(a.w, a.s),
        "+" | "-" | "*" | "/" | "%" => x1("w"),
        _ => return None,
    })
}
