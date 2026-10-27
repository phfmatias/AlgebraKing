//   AlgebraKing //
//
//   File:         [number.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Exact integers, rationals and Maple-style decimal software floats.']

use num_bigint::{BigInt, Sign};
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use std::cmp::Ordering;

/// A Maple numeric value. Floats are `m * 10^e` and keep their mantissa as typed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Num {
    Int(BigInt),
    Rat(BigInt, BigInt),
    Float(BigInt, i64),
}

pub fn pow10(n: u64) -> BigInt {
    num_traits::pow(BigInt::from(10), n as usize)
}

fn ndigits(m: &BigInt) -> u64 {
    if m.is_zero() {
        1
    } else {
        m.abs().to_string().len() as u64
    }
}

// rounded division, half away from zero
fn round_div(a: &BigInt, b: &BigInt) -> BigInt {
    let (q, r) = a.div_rem(b);
    if (r.abs() * 2) >= b.abs() {
        if (a.sign() == Sign::Minus) != (b.sign() == Sign::Minus) {
            q - 1
        } else {
            q + 1
        }
    } else {
        q
    }
}

impl Num {
    pub fn int(i: i64) -> Num {
        Num::Int(BigInt::from(i))
    }

    pub fn zero() -> Num {
        Num::int(0)
    }

    pub fn one() -> Num {
        Num::int(1)
    }

    /// Build p/q in lowest terms (q != 0).
    pub fn rat(p: BigInt, q: BigInt) -> Result<Num, String> {
        if q.is_zero() {
            return Err("numeric exception: division by zero".into());
        }
        let g = p.gcd(&q);
        let (mut p, mut q) = (p / &g, q / &g);
        if q.is_negative() {
            p = -p;
            q = -q;
        }
        if q.is_one() {
            Ok(Num::Int(p))
        } else {
            Ok(Num::Rat(p, q))
        }
    }

    /// Round a float to `digits` significant digits.
    pub fn float(m: BigInt, e: i64, digits: u32) -> Num {
        let nd = ndigits(&m);
        if nd > digits as u64 {
            let k = nd - digits as u64;
            let m2 = round_div(&m, &pow10(k));
            let e2 = e + k as i64;
            if ndigits(&m2) > digits as u64 {
                return Num::Float(m2 / 10, e2 + 1);
            }
            return Num::Float(m2, e2);
        }
        Num::Float(m, e)
    }

    /// Parse a Maple float literal such as `1.50`, `2.`, `.5`, `1e-3`, `2.5E+4`.
    pub fn parse_float(s: &str) -> Option<Num> {
        let s = s.trim();
        let (mant, exp) = match s.find(['e', 'E']) {
            Some(p) => (&s[..p], s[p + 1..].parse::<i64>().ok()?),
            None => (s, 0),
        };
        let (neg, mant) = match mant.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, mant.strip_prefix('+').unwrap_or(mant)),
        };
        let (ip, fp) = match mant.find('.') {
            Some(p) => (&mant[..p], &mant[p + 1..]),
            None => (mant, ""),
        };
        let digits = format!("{}{}", ip, fp);
        if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        let mut m: BigInt = digits.parse().ok()?;
        if neg {
            m = -m;
        }
        Some(Num::Float(m, exp - fp.len() as i64))
    }

    pub fn is_zero(&self) -> bool {
        match self {
            Num::Int(i) => i.is_zero(),
            Num::Rat(..) => false,
            Num::Float(m, _) => m.is_zero(),
        }
    }

    pub fn is_exact_zero(&self) -> bool {
        matches!(self, Num::Int(i) if i.is_zero())
    }

    pub fn is_one(&self) -> bool {
        matches!(self, Num::Int(i) if i.is_one())
    }

    pub fn is_minus_one(&self) -> bool {
        matches!(self, Num::Int(i) if *i == BigInt::from(-1))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Num::Float(..))
    }

    pub fn is_negative(&self) -> bool {
        match self {
            Num::Int(i) => i.is_negative(),
            Num::Rat(p, _) => p.is_negative(),
            Num::Float(m, _) => m.is_negative(),
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Num::Int(i) => i.to_i64(),
            _ => None,
        }
    }

    pub fn to_f64(&self) -> f64 {
        match self {
            Num::Int(i) => i.to_f64().unwrap_or(f64::NAN),
            Num::Rat(p, q) => p.to_f64().unwrap_or(f64::NAN) / q.to_f64().unwrap_or(f64::NAN),
            Num::Float(m, e) => format!("{}e{}", m, e).parse().unwrap_or(f64::NAN),
        }
    }

    /// Exact rational value (p, q) of any number, floats included.
    pub fn as_ratio(&self) -> (BigInt, BigInt) {
        match self {
            Num::Int(i) => (i.clone(), BigInt::one()),
            Num::Rat(p, q) => (p.clone(), q.clone()),
            Num::Float(m, e) => {
                if *e >= 0 {
                    (m * pow10(*e as u64), BigInt::one())
                } else {
                    (m.clone(), pow10((-e) as u64))
                }
            }
        }
    }

    pub fn cmp_value(&self, other: &Num) -> Ordering {
        let (a, b) = self.as_ratio();
        let (c, d) = other.as_ratio();
        (a * d).cmp(&(c * b))
    }

    /// Convert to a float with `digits` significant digits.
    pub fn to_float(&self, digits: u32) -> Num {
        match self {
            Num::Float(m, e) => Num::float(m.clone(), *e, digits),
            Num::Int(i) => Num::float(i.clone(), 0, digits),
            Num::Rat(p, q) => {
                let s = digits as i64 + ndigits(q) as i64 - ndigits(p) as i64 + 2;
                let m = if s >= 0 {
                    round_div(&(p * pow10(s as u64)), q)
                } else {
                    round_div(p, &(q * pow10((-s) as u64)))
                };
                Num::float(m, -s, digits)
            }
        }
    }

    pub fn neg(&self) -> Num {
        match self {
            Num::Int(i) => Num::Int(-i),
            Num::Rat(p, q) => Num::Rat(-p, q.clone()),
            Num::Float(m, e) => Num::Float(-m, *e),
        }
    }

    pub fn add(&self, o: &Num, digits: u32) -> Num {
        if self.is_float() || o.is_float() {
            let (Num::Float(m1, e1), Num::Float(m2, e2)) = (self.to_float(digits + 2), o.to_float(digits + 2))
            else {
                unreachable!()
            };
            // negligible term: skip the huge alignment
            let gap = digits as i64 + 4;
            if !m1.is_zero() && !m2.is_zero() {
                let l1 = e1 + ndigits(&m1) as i64;
                let l2 = e2 + ndigits(&m2) as i64;
                if l1 - l2 > gap {
                    return Num::float(m1, e1, digits);
                }
                if l2 - l1 > gap {
                    return Num::float(m2, e2, digits);
                }
            }
            let e = e1.min(e2);
            let m = m1 * pow10((e1 - e) as u64) + m2 * pow10((e2 - e) as u64);
            return Num::float(m, e, digits);
        }
        let (a, b) = self.as_ratio();
        let (c, d) = o.as_ratio();
        Num::rat(a * &d + c * &b, b * d).expect("nonzero denominator")
    }

    pub fn mul(&self, o: &Num, digits: u32) -> Num {
        if self.is_float() || o.is_float() {
            let (Num::Float(m1, e1), Num::Float(m2, e2)) = (self.to_float(digits + 2), o.to_float(digits + 2))
            else {
                unreachable!()
            };
            return Num::float(m1 * m2, e1 + e2, digits);
        }
        let (a, b) = self.as_ratio();
        let (c, d) = o.as_ratio();
        Num::rat(a * c, b * d).expect("nonzero denominator")
    }

    pub fn inv(&self, digits: u32) -> Result<Num, String> {
        if self.is_zero() {
            if self.is_float() {
                return Err("numeric exception: division by zero".into());
            }
            return Err("numeric exception: division by zero".into());
        }
        match self {
            Num::Float(m, e) => {
                let s = digits as i64 + ndigits(m) as i64 + 2;
                let q = round_div(&pow10(s as u64), m);
                Ok(Num::float(q, -s - e, digits))
            }
            _ => {
                let (a, b) = self.as_ratio();
                Num::rat(b, a)
            }
        }
    }

    /// Integer power; errors on 0^negative.
    pub fn pow_int(&self, n: &BigInt, digits: u32) -> Result<Num, String> {
        if n.is_negative() {
            return self.pow_int(&-n, digits)?.inv(digits);
        }
        let Some(mut k) = n.to_u64() else {
            return Err("integer too large in context".into());
        };
        if !self.is_float() {
            let (a, b) = self.as_ratio();
            if k > 1_000_000 && a.abs() > BigInt::one() {
                return Err("integer too large in context".into());
            }
            let e = k as usize;
            return Num::rat(num_traits::pow(a, e), num_traits::pow(b, e));
        }
        let work = digits + 5;
        let mut base = self.to_float(work);
        let mut acc = Num::Float(BigInt::one(), 0);
        while k > 0 {
            if k & 1 == 1 {
                acc = acc.mul(&base, work);
            }
            base = base.mul(&base, work);
            k >>= 1;
        }
        Ok(acc.to_float(digits))
    }

    /// Maple 1-D text of the number.
    pub fn to_maple(&self) -> String {
        match self {
            Num::Int(i) => i.to_string(),
            Num::Rat(p, q) => format!("{}/{}", p, q),
            Num::Float(m, e) => float_text(m, *e),
        }
    }
}

/// Exact integer n-th root if `a` is a perfect n-th power (a >= 0).
pub fn exact_root(a: &BigInt, n: u32) -> Option<BigInt> {
    if a.is_negative() {
        return None;
    }
    if a.is_zero() || a.is_one() || n == 1 {
        return Some(a.clone());
    }
    let r = a.nth_root(n);
    if num_traits::pow(r.clone(), n as usize) == *a {
        Some(r)
    } else {
        None
    }
}

fn float_text(m: &BigInt, e: i64) -> String {
    if m.is_zero() {
        return "0.".into();
    }
    let neg = m.is_negative();
    let ds = m.abs().to_string();
    let d = ds.len() as i64;
    let exp10 = d - 1 + e;
    let sign = if neg { "-" } else { "" };
    let positional_max = d.max(10);
    if exp10 < -5 || exp10 >= positional_max {
        let (first, rest) = ds.split_at(1);
        let ex = if exp10 < 0 { format!("({})", exp10) } else { exp10.to_string() };
        return format!("{}{}.{}*10^{}", sign, first, rest, ex);
    }
    if e >= 0 {
        format!("{}{}{}.", sign, ds, "0".repeat(e as usize))
    } else {
        let intd = d + e;
        if intd > 0 {
            let (a, b) = ds.split_at(intd as usize);
            format!("{}{}.{}", sign, a, b)
        } else {
            format!("{}0.{}{}", sign, "0".repeat((-intd) as usize), ds)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(s: &str) -> Num {
        Num::parse_float(s).unwrap()
    }

    #[test]
    fn float_parse_print() {
        assert_eq!(f("2.").to_maple(), "2.");
        assert_eq!(f("2.50").to_maple(), "2.50");
        assert_eq!(f(".5").to_maple(), "0.5");
        assert_eq!(f("1e-3").to_maple(), "0.001");
        assert_eq!(f("1e10").to_maple(), "1.*10^10");
        assert_eq!(f("1.5e-20").to_maple(), "1.5*10^(-20)");
        assert_eq!(f("-3.25").to_maple(), "-3.25");
    }

    #[test]
    fn float_arith() {
        let third = Num::Rat(1.into(), 3.into()).to_float(10);
        assert_eq!(third.to_maple(), "0.3333333333");
        assert_eq!(f("1.5").add(&Num::int(1), 10).to_maple(), "2.5");
        assert_eq!(f("2.").mul(&Num::int(3), 10).to_maple(), "6.");
        assert_eq!(Num::int(2).to_float(10).inv(10).unwrap().to_maple(), "0.5000000000");
        assert_eq!(Num::Rat(5.into(), 2.into()).to_float(10).to_maple(), "2.500000000");
        let two_thirds = Num::Rat(2.into(), 3.into()).to_float(10);
        assert_eq!(two_thirds.to_maple(), "0.6666666667");
    }

    #[test]
    fn exact_ops() {
        let a = Num::rat(1.into(), 2.into()).unwrap();
        let b = Num::rat(1.into(), 3.into()).unwrap();
        assert_eq!(a.add(&b, 10).to_maple(), "5/6");
        assert_eq!(a.mul(&b, 10).to_maple(), "1/6");
        assert_eq!(Num::int(2).pow_int(&BigInt::from(100), 10).unwrap().to_maple(), "1267650600228229401496703205376");
        assert!(Num::int(0).inv(10).is_err());
        assert_eq!(exact_root(&BigInt::from(27), 3), Some(BigInt::from(3)));
        assert_eq!(exact_root(&BigInt::from(12), 2), None);
    }
}
