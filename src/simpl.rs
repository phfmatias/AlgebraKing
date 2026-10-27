//   AlgebraKing //
//
//   File:         [simpl.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Automatic simplification of sums, products and powers, like the Maple kernel.']

use crate::ast::{Expr, RelOp};
use crate::number::{exact_root, Num};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive};

pub type SResult = Result<Expr, String>;

fn num(n: Num) -> Expr {
    Expr::Num(n)
}

fn is_eq(e: &Expr) -> bool {
    matches!(e, Expr::Rel(RelOp::Eq, ..))
}

fn is_rtable(e: &Expr) -> bool {
    matches!(e, Expr::Matrix(_) | Expr::Vector(..))
}

/// Split a term into numeric coefficient and the rest (None = pure number).
fn split_coef(e: Expr) -> (Num, Option<Expr>) {
    match e {
        Expr::Num(n) => (n, None),
        Expr::Mul(mut v) if matches!(v.first(), Some(Expr::Num(_))) => {
            let Expr::Num(c) = v.remove(0) else { unreachable!() };
            let rest = if v.len() == 1 { v.pop().unwrap() } else { Expr::Mul(v) };
            (c, Some(rest))
        }
        other => (Num::one(), Some(other)),
    }
}

fn with_coef(c: Num, rest: Expr) -> Expr {
    if c.is_one() {
        return rest;
    }
    match rest {
        Expr::Mul(mut v) => {
            v.insert(0, num(c));
            Expr::Mul(v)
        }
        r => Expr::Mul(vec![num(c), r]),
    }
}

/// Polynomial-like degree used to order the terms of a sum.
fn degree(e: &Expr) -> f64 {
    match e {
        Expr::Sym(_) | Expr::Idx(..) => 1.0,
        Expr::Num(_) | Expr::Const(_) => 0.0,
        Expr::Pow(b, x) => match x.as_ref() {
            Expr::Num(n) => degree(b) * n.to_f64(),
            _ => degree(b),
        },
        Expr::Mul(v) => v.iter().map(degree).sum(),
        Expr::Fn(..) | Expr::Apply(..) if has_symbol(e) => 1.0,
        _ => 0.0,
    }
}

pub fn has_symbol(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |x| {
        if matches!(x, Expr::Sym(_)) {
            found = true;
        }
    });
    found
}

fn elementwise2(a: &Expr, b: &Expr, op: &dyn Fn(Expr, Expr) -> SResult, what: &str) -> SResult {
    match (a, b) {
        (Expr::Matrix(r1), Expr::Matrix(r2)) => {
            if r1.len() != r2.len() || r1.first().map(|r| r.len()) != r2.first().map(|r| r.len()) {
                return Err(format!("(in rtable/{}) invalid arguments", what));
            }
            let mut rows = Vec::new();
            for (x, y) in r1.iter().zip(r2) {
                let mut row = Vec::new();
                for (p, q) in x.iter().zip(y) {
                    row.push(op(p.clone(), q.clone())?);
                }
                rows.push(row);
            }
            Ok(Expr::Matrix(rows))
        }
        (Expr::Vector(v1, o1), Expr::Vector(v2, o2)) => {
            if v1.len() != v2.len() || o1 != o2 {
                return Err(format!("(in rtable/{}) invalid arguments", what));
            }
            let mut out = Vec::new();
            for (p, q) in v1.iter().zip(v2) {
                out.push(op(p.clone(), q.clone())?);
            }
            Ok(Expr::Vector(out, *o1))
        }
        (Expr::List(v1), Expr::List(v2)) => {
            if v1.len() != v2.len() {
                return Err("adding lists of different length".into());
            }
            let mut out = Vec::new();
            for (p, q) in v1.iter().zip(v2) {
                out.push(op(p.clone(), q.clone())?);
            }
            Ok(Expr::List(out))
        }
        _ => Err(format!("(in rtable/{}) invalid arguments", what)),
    }
}

fn map_entries(e: &Expr, f: &dyn Fn(Expr) -> SResult) -> SResult {
    match e {
        Expr::Matrix(rows) => {
            let mut out = Vec::new();
            for r in rows {
                out.push(r.iter().map(|x| f(x.clone())).collect::<Result<Vec<_>, _>>()?);
            }
            Ok(Expr::Matrix(out))
        }
        Expr::Vector(v, o) => Ok(Expr::Vector(v.iter().map(|x| f(x.clone())).collect::<Result<Vec<_>, _>>()?, *o)),
        Expr::List(v) => Ok(Expr::List(v.iter().map(|x| f(x.clone())).collect::<Result<Vec<_>, _>>()?)),
        other => f(other.clone()),
    }
}

pub fn identity(n: usize) -> Expr {
    Expr::Matrix((0..n).map(|i| (0..n).map(|j| Expr::int((i == j) as i64)).collect()).collect())
}

/// Sum with Maple automatic simplification.
pub fn add(terms: Vec<Expr>, d: u32) -> SResult {
    let mut flat = Vec::new();
    for t in terms {
        match t {
            Expr::Add(v) => flat.extend(v),
            other => flat.push(other),
        }
    }
    if flat.iter().any(is_eq) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for t in flat {
            match t {
                Expr::Rel(RelOp::Eq, a, b) => {
                    l.push(*a);
                    r.push(*b);
                }
                other => {
                    l.push(other.clone());
                    r.push(other);
                }
            }
        }
        return Ok(Expr::eq(add(l, d)?, add(r, d)?));
    }
    if flat.iter().any(|t| is_rtable(t) || matches!(t, Expr::List(_))) {
        let mut acc: Option<Expr> = None;
        let mut scalars = Vec::new();
        for t in flat {
            if is_rtable(&t) || matches!(t, Expr::List(_)) {
                acc = Some(match acc {
                    None => t,
                    Some(a) => elementwise2(&a, &t, &|x, y| add(vec![x, y], d), "Sum")?,
                });
            } else {
                scalars.push(t);
            }
        }
        let acc = acc.unwrap();
        if scalars.is_empty() {
            return Ok(acc);
        }
        let s = add(scalars, d)?;
        return match acc {
            Expr::Matrix(rows) if rows.len() == rows.first().map(|r| r.len()).unwrap_or(0) => {
                let mut rows = rows;
                for (i, row) in rows.iter_mut().enumerate() {
                    row[i] = add(vec![row[i].clone(), s.clone()], d)?;
                }
                Ok(Expr::Matrix(rows))
            }
            _ => Err("(in rtable/Sum) invalid arguments".into()),
        };
    }
    // infinity absorbs finite numbers
    if flat.iter().any(|t| inf_sign(t).is_some()) && flat.iter().all(|t| inf_sign(t).is_some() || matches!(t, Expr::Num(_)))
    {
        let signs: Vec<i8> = flat.iter().filter_map(inf_sign).collect();
        let inf = Expr::Const("infinity".into());
        return Ok(if signs.iter().all(|s| *s > 0) {
            inf
        } else if signs.iter().all(|s| *s < 0) {
            Expr::Mul(vec![Expr::int(-1), inf])
        } else {
            Expr::Const("undefined".into())
        });
    }
    let mut constant = Num::zero();
    let mut has_const = false;
    let mut coll: Vec<(Expr, Num)> = Vec::new();
    for t in flat {
        let (c, rest) = split_coef(t);
        match rest {
            None => {
                constant = constant.add(&c, d);
                has_const = true;
            }
            Some(r) => {
                if let Some(slot) = coll.iter_mut().find(|(k, _)| *k == r) {
                    slot.1 = slot.1.add(&c, d);
                } else {
                    coll.push((r, c));
                }
            }
        }
    }
    let mut out: Vec<Expr> = Vec::new();
    for (r, c) in coll {
        if c.is_exact_zero() {
            continue;
        }
        if c.is_zero() {
            constant = constant.add(&c, d);
            has_const = true;
            continue;
        }
        out.push(with_coef(c, r));
    }
    let is_order = |e: &Expr| matches!(e, Expr::Fn(n, _) if n == "O");
    if out.iter().any(is_order) {
        // truncated series: ascending powers, constant first, O(...) last
        out.sort_by(|a, b| {
            (is_order(a), degree(a)).partial_cmp(&(is_order(b), degree(b))).unwrap_or(std::cmp::Ordering::Equal)
        });
        if has_const && !constant.is_exact_zero() {
            out.insert(0, num(constant));
        }
    } else {
        out.sort_by(|a, b| degree(b).partial_cmp(&degree(a)).unwrap_or(std::cmp::Ordering::Equal));
        if has_const && (!constant.is_exact_zero() || out.is_empty()) {
            out.push(num(constant));
        }
    }
    let r = match out.len() {
        0 => Expr::int(0),
        1 => out.pop().unwrap(),
        _ => Expr::Add(out),
    };
    Ok(canon_complex(r, d))
}

fn split_pow(e: Expr) -> (Expr, Expr) {
    match e {
        Expr::Pow(b, x) => (*b, *x),
        other => (other, Expr::int(1)),
    }
}

/// Product with Maple automatic simplification.
pub fn mul(factors: Vec<Expr>, d: u32) -> SResult {
    let mut flat = Vec::new();
    for f in factors {
        match f {
            Expr::Mul(v) => flat.extend(v),
            other => flat.push(other),
        }
    }
    if flat.iter().any(is_eq) {
        let (mut l, mut r) = (Vec::new(), Vec::new());
        for t in flat {
            match t {
                Expr::Rel(RelOp::Eq, a, b) => {
                    l.push(*a);
                    r.push(*b);
                }
                other => {
                    l.push(other.clone());
                    r.push(other);
                }
            }
        }
        return Ok(Expr::eq(mul(l, d)?, mul(r, d)?));
    }
    let nrt = flat.iter().filter(|t| is_rtable(t) || matches!(t, Expr::List(_))).count();
    if nrt > 1 {
        return Err("(in rtable/Product) invalid arguments; use `.` for matrix products".into());
    }
    if nrt == 1 {
        let pos = flat.iter().position(|t| is_rtable(t) || matches!(t, Expr::List(_))).unwrap();
        let rt = flat.remove(pos);
        let s = mul(flat, d)?;
        return map_entries(&rt, &|x| mul(vec![s.clone(), x], d));
    }
    let mut coef = Num::one();
    let mut coll: Vec<(Expr, Expr)> = Vec::new();
    for f in flat {
        if let Expr::Num(n) = f {
            coef = coef.mul(&n, d);
            continue;
        }
        let (b, x) = split_pow(f);
        if let Some(slot) = coll.iter_mut().find(|(k, ex)| {
            *k == b && ((matches!(ex, Expr::Num(_)) && matches!(x, Expr::Num(_))) || *ex == x)
        }) {
            slot.1 = add(vec![slot.1.clone(), x], d)?;
        } else {
            coll.push((b, x));
        }
    }
    let mut out: Vec<Expr> = Vec::new();
    for (b, x) in coll {
        let p = pow(b, x, d)?;
        match p {
            Expr::Num(n) => coef = coef.mul(&n, d),
            Expr::Mul(v) => {
                for f in v {
                    match f {
                        Expr::Num(n) => coef = coef.mul(&n, d),
                        other => out.push(other),
                    }
                }
            }
            other => out.push(other),
        }
    }
    if coef.is_zero() {
        return Ok(num(coef));
    }
    if out.is_empty() {
        return Ok(num(coef));
    }
    if !coef.is_one() && out.len() == 1 {
        if let Expr::Add(terms) = &out[0] {
            let c = num(coef.clone());
            let ts: Result<Vec<_>, _> = terms.iter().map(|t| mul(vec![c.clone(), t.clone()], d)).collect();
            return add(ts?, d);
        }
    }
    if out.len() == 1 && matches!(&out[0], Expr::Const(c) if c == "infinity") && !coef.is_zero() {
        coef = if coef.is_negative() { Num::int(-1) } else { Num::one() };
    }
    if coef.is_one() && out.len() == 1 {
        return Ok(canon_complex(out.pop().unwrap(), d));
    }
    if !coef.is_one() {
        out.insert(0, num(coef));
    }
    Ok(canon_complex(Expr::Mul(out), d))
}

fn float_from_f64(v: f64, d: u32) -> Option<Num> {
    if !v.is_finite() {
        return None;
    }
    let digits = d.clamp(1, 17) as usize;
    Num::parse_float(&format!("{:.*e}", digits - 1, v)).map(|n| match n {
        Num::Float(m, e) => Num::float(m, e, d),
        o => o,
    })
}

fn pow_num(b: &Num, x: &Num, d: u32) -> Result<Option<Num>, String> {
    if let Num::Int(n) = x {
        return b.pow_int(n, d).map(Some);
    }
    if b.is_float() || x.is_float() {
        if d > 15 {
            return Ok(None);
        }
        let (bv, xv) = (b.to_f64(), x.to_f64());
        if bv < 0.0 {
            return Ok(None);
        }
        return Ok(float_from_f64(bv.powf(xv), d));
    }
    // exact rational base and rational exponent p/q
    let Num::Rat(p, q) = x else { return Ok(None) };
    let Some(qn) = q.to_u32() else { return Ok(None) };
    let (n, den) = b.as_ratio();
    if n.is_negative() {
        return Ok(None);
    }
    match (exact_root(&n, qn), exact_root(&den, qn)) {
        (Some(rn), Some(rd)) => Ok(Some(Num::rat(rn, rd)?.pow_int(p, d)?)),
        _ => Ok(None),
    }
}

#[allow(clippy::needless_range_loop)]
fn matmul(a: &Expr, b: &Expr, d: u32) -> SResult {
    let (Expr::Matrix(r1), Expr::Matrix(r2)) = (a, b) else { unreachable!() };
    let n = r1.first().map(|r| r.len()).unwrap_or(0);
    if n != r2.len() {
        return Err(format!(
            "(in LinearAlgebra:-MatrixMatrixMultiply) first matrix column dimension ({}) <> second matrix row dimension ({})",
            n,
            r2.len()
        ));
    }
    let m = r2.first().map(|r| r.len()).unwrap_or(0);
    let mut rows = Vec::new();
    for row in r1 {
        let mut out = Vec::new();
        for j in 0..m {
            let mut terms = Vec::new();
            for k in 0..n {
                terms.push(mul(vec![row[k].clone(), r2[k][j].clone()], d)?);
            }
            out.push(add(terms, d)?);
        }
        rows.push(out);
    }
    Ok(Expr::Matrix(rows))
}

fn conj(e: &Expr) -> Expr {
    match e {
        Expr::Num(_) => e.clone(),
        other => Expr::func("conjugate", vec![other.clone()]),
    }
}

/// Non-commutative product `a . b` (matrix product, dot product, or ordinary product).
pub fn dot(a: Expr, b: Expr, d: u32) -> SResult {
    let col = |v: &Vec<Expr>| Expr::Matrix(v.iter().map(|x| vec![x.clone()]).collect());
    let row = |v: &Vec<Expr>| Expr::Matrix(vec![v.clone()]);
    match (&a, &b) {
        (Expr::Matrix(_), Expr::Matrix(_)) => matmul(&a, &b, d),
        (Expr::Matrix(_), Expr::Vector(v, false)) => match matmul(&a, &col(v), d)? {
            Expr::Matrix(rows) => Ok(Expr::Vector(rows.into_iter().map(|mut r| r.remove(0)).collect(), false)),
            _ => unreachable!(),
        },
        (Expr::Vector(v, true), Expr::Matrix(_)) => match matmul(&row(v), &b, d)? {
            Expr::Matrix(mut rows) => Ok(Expr::Vector(rows.remove(0), true)),
            _ => unreachable!(),
        },
        (Expr::Vector(v1, o1), Expr::Vector(v2, o2)) => {
            if v1.len() != v2.len() && !(!o1 && *o2) {
                return Err("(in LinearAlgebra:-Multiply) dimensions of the Vectors do not agree".into());
            }
            match (o1, o2) {
                (false, true) => matmul(&col(v1), &row(v2), d),
                (true, false) => {
                    let terms: Result<Vec<_>, _> =
                        v1.iter().zip(v2).map(|(x, y)| mul(vec![x.clone(), y.clone()], d)).collect();
                    add(terms?, d)
                }
                _ => {
                    let terms: Result<Vec<_>, _> =
                        v1.iter().zip(v2).map(|(x, y)| mul(vec![conj(x), y.clone()], d)).collect();
                    add(terms?, d)
                }
            }
        }
        (Expr::Matrix(_), Expr::Vector(..)) | (Expr::Vector(..), Expr::Matrix(_)) => {
            Err("(in LinearAlgebra:-Multiply) invalid arguments".into())
        }
        _ => mul(vec![a, b], d),
    }
}

/// Power with Maple automatic simplification.
pub fn pow(b: Expr, x: Expr, d: u32) -> SResult {
    if let Expr::Num(xn) = &x {
        if xn.is_exact_zero() {
            if let Expr::Matrix(rows) = &b {
                return Ok(identity(rows.len()));
            }
            return Ok(Expr::int(1));
        }
        if xn.is_one() {
            return Ok(b);
        }
    }
    match (&b, &x) {
        (Expr::Num(bn), _) if bn.is_one() => return Ok(b),
        (Expr::Num(bn), Expr::Num(xn)) => {
            if bn.is_zero() {
                if xn.is_negative() {
                    return Err("numeric exception: division by zero".into());
                }
                return Ok(b);
            }
            if let Some(r) = pow_num(bn, xn, d)? {
                return Ok(num(r));
            }
            // (-n)^(p/2) with a perfect square
            if let (true, Num::Rat(p, q)) = (bn.is_negative(), xn) {
                if *q == BigInt::from(2) {
                    if let Some(r) = pow_num(&bn.neg(), xn, d)? {
                        let ip = pow(Expr::Const("I".into()), num(Num::Int(p.clone())), d)?;
                        return mul(vec![num(r), ip], d);
                    }
                }
            }
            return Ok(Expr::Pow(Box::new(b), Box::new(x)));
        }
        (Expr::Const(c), Expr::Num(Num::Int(n))) if c == "I" => {
            let k: BigInt = ((n % 4) + 4) % 4;
            return Ok(match k.to_i64().unwrap_or(0) {
                0 => Expr::int(1),
                1 => b,
                2 => Expr::int(-1),
                _ => Expr::Mul(vec![Expr::int(-1), b]),
            });
        }
        (Expr::Pow(c, f), Expr::Num(Num::Int(_))) => {
            let e2 = mul(vec![(**f).clone(), x.clone()], d)?;
            return pow((**c).clone(), e2, d);
        }
        (Expr::Mul(v), Expr::Num(Num::Int(_))) => {
            let fs: Result<Vec<_>, _> = v.iter().map(|f| pow(f.clone(), x.clone(), d)).collect();
            return mul(fs?, d);
        }
        (Expr::Rel(RelOp::Eq, l, r), _) => {
            return Ok(Expr::eq(pow((**l).clone(), x.clone(), d)?, pow((**r).clone(), x.clone(), d)?));
        }
        (Expr::Matrix(rows), Expr::Num(Num::Int(n))) if !n.is_negative() => {
            if rows.len() != rows.first().map(|r| r.len()).unwrap_or(0) {
                return Err("(in rtable/Power) square Matrix required".into());
            }
            let mut k = n.to_u64().ok_or("exponent too large")?;
            let mut acc = identity(rows.len());
            let mut base = b.clone();
            while k > 0 {
                if k & 1 == 1 {
                    acc = matmul(&acc, &base, d)?;
                }
                k >>= 1;
                if k > 0 {
                    base = matmul(&base, &base, d)?;
                }
            }
            return Ok(acc);
        }
        (Expr::List(_), _) => return map_entries(&b, &|e| pow(e, x.clone(), d)),
        _ => {}
    }
    Ok(canon_complex(Expr::Pow(Box::new(b), Box::new(x)), d))
}

fn is_imag_unit(e: &Expr) -> bool {
    matches!(e, Expr::Const(c) if c == "I")
}

/// Value a + b*I of a complex numeric constant built from numbers and I.
fn as_complex(e: &Expr, d: u32) -> Option<(Num, Num)> {
    match e {
        Expr::Num(n) => Some((n.clone(), Num::zero())),
        Expr::Const(c) if c == "I" => Some((Num::zero(), Num::one())),
        Expr::Add(v) => {
            let (mut a, mut b) = (Num::zero(), Num::zero());
            for x in v {
                let (p, q) = as_complex(x, d)?;
                a = a.add(&p, d);
                b = b.add(&q, d);
            }
            Some((a, b))
        }
        Expr::Mul(v) => {
            let (mut a, mut b) = (Num::one(), Num::zero());
            for x in v {
                let (p, q) = as_complex(x, d)?;
                let na = a.mul(&p, d).add(&b.mul(&q, d).neg(), d);
                let nb = a.mul(&q, d).add(&b.mul(&p, d), d);
                a = na;
                b = nb;
            }
            Some((a, b))
        }
        Expr::Pow(base, x) => {
            let k = x.as_i64().filter(|k| k.abs() <= 64)?;
            let (p, q) = as_complex(base, d)?;
            let (mut a, mut b) = (Num::one(), Num::zero());
            for _ in 0..k.abs() {
                let na = a.mul(&p, d).add(&b.mul(&q, d).neg(), d);
                let nb = a.mul(&q, d).add(&b.mul(&p, d), d);
                a = na;
                b = nb;
            }
            if k < 0 {
                let den = a.mul(&a, d).add(&b.mul(&b, d), d);
                let inv = den.inv(d).ok()?;
                return Some((a.mul(&inv, d), b.neg().mul(&inv, d)));
            }
            Some((a, b))
        }
        _ => None,
    }
}

/// Maple evaluates arithmetic on complex numeric constants automatically.
fn canon_complex(e: Expr, d: u32) -> Expr {
    let mut has_i = false;
    e.walk(&mut |x| {
        if is_imag_unit(x) {
            has_i = true;
        }
    });
    if !has_i {
        return e;
    }
    let Some((a, b)) = as_complex(&e, d) else { return e };
    let i = Expr::Const("I".into());
    let imag = if b.is_one() {
        i
    } else {
        Expr::Mul(vec![Expr::Num(b.clone()), i])
    };
    let r = if b.is_zero() {
        Expr::Num(a)
    } else if a.is_exact_zero() {
        imag
    } else {
        Expr::Add(vec![Expr::Num(a), imag])
    };
    if r == e {
        e
    } else {
        r
    }
}

fn inf_sign(e: &Expr) -> Option<i8> {
    match e {
        Expr::Const(c) if c == "infinity" => Some(1),
        Expr::Mul(v) if v.len() == 2 && matches!(&v[1], Expr::Const(c) if c == "infinity") => match &v[0] {
            Expr::Num(n) if n.is_negative() => Some(-1),
            Expr::Num(_) => Some(1),
            _ => None,
        },
        _ => None,
    }
}

/// Re-apply automatic simplification bottom-up (after substitution or engine results).
pub fn resimplify(e: &Expr, d: u32) -> SResult {
    match e {
        Expr::Add(v) => {
            let v: Result<Vec<_>, _> = v.iter().map(|x| resimplify(x, d)).collect();
            add(v?, d)
        }
        Expr::Mul(v) => {
            let v: Result<Vec<_>, _> = v.iter().map(|x| resimplify(x, d)).collect();
            mul(v?, d)
        }
        Expr::Pow(b, x) => pow(resimplify(b, d)?, resimplify(x, d)?, d),
        Expr::Proc(_) => Ok(e.clone()),
        Expr::Set(v) => {
            let v: Result<Vec<_>, _> = v.iter().map(|x| resimplify(x, d)).collect();
            Ok(make_set(v?))
        }
        Expr::Seq(v) | Expr::List(v) => {
            let mut out = Vec::new();
            for x in v {
                out.extend(resimplify(x, d)?.into_seq());
            }
            Ok(if matches!(e, Expr::List(_)) { Expr::List(out) } else { Expr::from_seq(out) })
        }
        other => {
            let mut err = None;
            let r = other.map_children(&mut |c| match resimplify(c, d) {
                Ok(x) => x,
                Err(m) => {
                    err = Some(m);
                    c.clone()
                }
            });
            match err {
                Some(m) => Err(m),
                None => Ok(r),
            }
        }
    }
}

/// Ordering key for set elements: numbers by value first, then by printed form.
pub fn set_order(a: &Expr, b: &Expr) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => x.cmp_value(y),
        (Expr::Num(_), _) => Ordering::Less,
        (_, Expr::Num(_)) => Ordering::Greater,
        _ => crate::printer::print(a).cmp(&crate::printer::print(b)),
    }
}

/// Build a set: remove duplicates and sort.
pub fn make_set(v: Vec<Expr>) -> Expr {
    let mut out: Vec<Expr> = Vec::new();
    for x in v {
        for y in x.into_seq() {
            if !out.contains(&y) {
                out.push(y);
            }
        }
    }
    out.sort_by(set_order);
    Expr::Set(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: &str) -> Expr {
        Expr::sym(n)
    }

    #[test]
    fn collect_terms() {
        let e = add(vec![s("x"), s("x"), Expr::int(1), Expr::int(2)], 10).unwrap();
        assert_eq!(crate::printer::print(&e), "2*x+3");
        let e = add(vec![s("x"), Expr::Mul(vec![Expr::int(-1), s("x")])], 10).unwrap();
        assert_eq!(e, Expr::int(0));
    }

    #[test]
    fn products_and_powers() {
        let e = mul(vec![s("x"), s("x"), Expr::int(3)], 10).unwrap();
        assert_eq!(crate::printer::print(&e), "3*x^2");
        let e = pow(Expr::int(4), num(Num::rat(1.into(), 2.into()).unwrap()), 10).unwrap();
        assert_eq!(e, Expr::int(2));
        let e = pow(Expr::int(-4), num(Num::rat(1.into(), 2.into()).unwrap()), 10).unwrap();
        assert_eq!(crate::printer::print(&e), "2*I");
        let e = pow(Expr::Const("I".into()), Expr::int(2), 10).unwrap();
        assert_eq!(e, Expr::int(-1));
        let e = mul(vec![Expr::int(2), add(vec![s("x"), Expr::int(1)], 10).unwrap()], 10).unwrap();
        assert_eq!(crate::printer::print(&e), "2*x+2");
        assert!(pow(Expr::int(0), Expr::int(-1), 10).is_err());
    }
}
