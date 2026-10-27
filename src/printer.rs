//   AlgebraKing //
//
//   File:         [printer.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Maple 1-D printer with minimal parentheses by operator precedence.']

use crate::ast::{Expr, Proc, RelOp, Stmt};
use crate::number::Num;
use crate::parser::is_keyword;

// precedence levels, higher binds tighter
const P_SEQ: u8 = 0;
const P_ARROW: u8 = 1;
const P_IMPLIES: u8 = 2;
const P_OR: u8 = 3;
const P_AND: u8 = 4;
const P_NOT: u8 = 5;
const P_DOLLAR: u8 = 6;
const P_REL: u8 = 7;
const P_RANGE: u8 = 8;
const P_MOD: u8 = 9;
const P_ADD: u8 = 10;
const P_MUL: u8 = 11;
const P_POW: u8 = 12;
const P_ATOM: u8 = 14;

pub fn print(e: &Expr) -> String {
    pr(e, P_SEQ)
}

/// Print `e` in a context that needs at least precedence `ctx`.
fn pr(e: &Expr, ctx: u8) -> String {
    let (s, p) = render(e);
    if p < ctx {
        format!("({})", s)
    } else {
        s
    }
}

pub fn name_text(n: &str) -> String {
    let mut chars = n.chars();
    let ok = match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    };
    if ok && !is_keyword(n) {
        n.to_string()
    } else {
        format!("`{}`", n.replace('`', "``"))
    }
}

fn string_text(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn join(v: &[Expr], ctx: u8) -> String {
    v.iter().map(|x| pr(x, ctx)).collect::<Vec<_>>().join(", ")
}

fn is_negative_term(e: &Expr) -> bool {
    match e {
        Expr::Num(n) => n.is_negative(),
        Expr::Mul(v) => matches!(v.first(), Some(Expr::Num(n)) if n.is_negative()),
        _ => false,
    }
}

fn negate_term(e: &Expr) -> Expr {
    match e {
        Expr::Num(n) => Expr::Num(n.neg()),
        Expr::Mul(v) => {
            let Some(Expr::Num(n)) = v.first() else { return e.clone() };
            let n = n.neg();
            let mut rest: Vec<Expr> = v[1..].to_vec();
            if n.is_one() {
                if rest.len() == 1 {
                    return rest.pop().unwrap();
                }
                return Expr::Mul(rest);
            }
            rest.insert(0, Expr::Num(n));
            Expr::Mul(rest)
        }
        _ => e.clone(),
    }
}

fn num_prec(n: &Num) -> u8 {
    if n.is_negative() {
        return P_ADD;
    }
    match n {
        Num::Rat(..) => P_MUL,
        Num::Float(..) if n.to_maple().contains('*') => P_MUL,
        _ => P_ATOM,
    }
}

fn render_mul(v: &[Expr]) -> (String, u8) {
    let mut neg = false;
    let mut numer: Vec<String> = Vec::new();
    let mut denom: Vec<(String, u8)> = Vec::new();
    for (i, f) in v.iter().enumerate() {
        match f {
            Expr::Num(n) if i == 0 => {
                let n = if n.is_negative() {
                    neg = true;
                    n.neg()
                } else {
                    n.clone()
                };
                match &n {
                    Num::Rat(p, q) => {
                        if *p != 1.into() {
                            numer.push(p.to_string());
                        }
                        denom.push((q.to_string(), P_ATOM));
                    }
                    _ => {
                        if !n.is_one() {
                            numer.push(pr(&Expr::Num(n), P_MUL));
                        }
                    }
                }
            }
            Expr::Pow(b, x) if matches!(x.as_ref(), Expr::Num(n) if n.is_negative()) => {
                let Expr::Num(n) = x.as_ref() else { unreachable!() };
                let inv = Expr::Pow(b.clone(), Box::new(Expr::Num(n.neg())));
                let inv = if n.is_minus_one() { (**b).clone() } else { inv };
                let (s, p) = render(&inv);
                denom.push((s, p));
            }
            other => numer.push(pr(other, P_MUL)),
        }
    }
    let mut s = if numer.is_empty() { "1".to_string() } else { numer.join("*") };
    if !denom.is_empty() {
        let d = if denom.len() == 1 {
            let (ds, dp) = &denom[0];
            if *dp <= P_MUL {
                format!("({})", ds)
            } else {
                ds.clone()
            }
        } else {
            let parts: Vec<String> =
                denom.iter().map(|(ds, dp)| if *dp < P_MUL { format!("({})", ds) } else { ds.clone() }).collect();
            format!("({})", parts.join("*"))
        };
        s = format!("{}/{}", s, d);
    }
    if neg {
        (format!("-{}", s), P_ADD)
    } else {
        (s, P_MUL)
    }
}

fn render(e: &Expr) -> (String, u8) {
    match e {
        Expr::Num(n) => (n.to_maple(), num_prec(n)),
        Expr::Sym(s) => (name_text(s), P_ATOM),
        Expr::Str(s) => (string_text(s), P_ATOM),
        Expr::Bool(b) => (b.to_string(), P_ATOM),
        Expr::Fail => ("FAIL".into(), P_ATOM),
        Expr::Const(c) => (c.clone(), P_ATOM),
        Expr::Add(v) => {
            if v.is_empty() {
                return ("0".into(), P_ATOM);
            }
            let mut s = String::new();
            for (i, t) in v.iter().enumerate() {
                if i == 0 {
                    s.push_str(&pr(t, P_ADD));
                } else if is_negative_term(t) {
                    s.push('-');
                    s.push_str(&pr(&negate_term(t), P_ADD + 1));
                } else {
                    s.push('+');
                    s.push_str(&pr(t, P_ADD + 1));
                }
            }
            (s, P_ADD)
        }
        Expr::Mul(v) => {
            if v.is_empty() {
                return ("1".into(), P_ATOM);
            }
            render_mul(v)
        }
        Expr::Pow(b, x) => {
            if let Expr::Num(n) = x.as_ref() {
                if n.is_negative() {
                    return render_mul(std::slice::from_ref(e));
                }
                if let Num::Rat(p, q) = n {
                    if *p == 1.into() && *q == 2.into() {
                        return (format!("sqrt({})", pr(b, P_SEQ + 1)), P_ATOM);
                    }
                }
            }
            (format!("{}^{}", pr(b, P_POW + 1), pr(x, P_POW + 1)), P_POW)
        }
        Expr::Fn(name, args) => render_fn(name, args),
        Expr::Apply(h, args) => (format!("{}({})", pr(h, P_ATOM), join(args, P_SEQ + 1)), P_ATOM),
        Expr::Idx(h, idx) => (format!("{}[{}]", pr(h, P_ATOM), join(idx, P_SEQ + 1)), P_ATOM),
        Expr::Rel(op, a, b) => {
            let o = match op {
                RelOp::Eq => "=",
                RelOp::Neq => "<>",
                RelOp::Lt => "<",
                RelOp::Le => "<=",
            };
            (format!("{} {} {}", pr(a, P_REL + 1), o, pr(b, P_REL + 1)), P_REL)
        }
        Expr::And(v) => (v.iter().map(|x| pr(x, P_AND + 1)).collect::<Vec<_>>().join(" and "), P_AND),
        Expr::Or(v) => (v.iter().map(|x| pr(x, P_OR + 1)).collect::<Vec<_>>().join(" or "), P_OR),
        Expr::Not(a) => (format!("not {}", pr(a, P_NOT)), P_NOT),
        Expr::Xor(a, b) => (format!("{} xor {}", pr(a, P_OR), pr(b, P_OR + 1)), P_OR),
        Expr::Implies(a, b) => (format!("{} implies {}", pr(a, P_IMPLIES + 1), pr(b, P_IMPLIES + 1)), P_IMPLIES),
        Expr::List(v) => (format!("[{}]", join(v, P_SEQ + 1)), P_ATOM),
        Expr::Set(v) => (format!("{{{}}}", join(v, P_SEQ + 1)), P_ATOM),
        Expr::Seq(v) => {
            if v.is_empty() {
                return ("NULL".into(), P_ATOM);
            }
            (join(v, P_SEQ + 1), P_SEQ)
        }
        Expr::Range(a, b) => (format!("{} .. {}", pr(a, P_RANGE + 1), pr(b, P_RANGE + 1)), P_RANGE),
        Expr::Matrix(rows) => {
            let m = rows.len();
            let n = rows.first().map(|r| r.len()).unwrap_or(0);
            let body: Vec<String> = rows.iter().map(|r| format!("[{}]", join(r, P_SEQ + 1))).collect();
            (format!("Matrix({}, {}, [{}])", m, n, body.join(", ")), P_ATOM)
        }
        Expr::Vector(v, row) => {
            (format!("Vector[{}]([{}])", if *row { "row" } else { "column" }, join(v, P_SEQ + 1)), P_ATOM)
        }
        Expr::Proc(p) => render_proc(p),
        Expr::Table(t) => {
            let items: Vec<String> =
                t.iter().map(|(k, v)| format!("{} = {}", pr(k, P_REL + 1), pr(v, P_REL + 1))).collect();
            (format!("table([{}])", items.join(", ")), P_ATOM)
        }
        Expr::Member(p, n) => (format!("{}:-{}", name_text(p), name_text(n)), P_ATOM),
        Expr::Compose(v) => (v.iter().map(|x| pr(x, P_MUL + 1)).collect::<Vec<_>>().join("@"), P_MUL),
        Expr::Repeat(a, b) => (format!("{}@@{}", pr(a, P_POW + 1), pr(b, P_POW + 1)), P_POW),
        Expr::TypeAssert(a, b) => (format!("{}::{}", pr(a, P_RANGE), pr(b, P_RANGE)), P_REL),
        Expr::In(a, b) => (format!("{} in {}", pr(a, P_REL + 1), pr(b, P_REL + 1)), P_REL),
        Expr::Uneval(a) => (format!("'{}'", print(a)), P_ATOM),
        Expr::Hist(k) => ("%".repeat(*k as usize), P_ATOM),
        Expr::Dot(a, b) => (format!("{} . {}", pr(a, P_MUL), pr(b, P_MUL + 1)), P_MUL),
        Expr::Concat(a, b) => (format!("{} || {}", pr(a, P_ATOM), pr(b, P_ATOM)), P_ATOM - 1),
        Expr::Dollar(a, b) => match a {
            Some(a) => (format!("{} $ {}", pr(a, P_DOLLAR + 1), pr(b, P_DOLLAR + 1)), P_DOLLAR),
            None => (format!("$ {}", pr(b, P_DOLLAR + 1)), P_DOLLAR),
        },
        Expr::Assuming(a, b) => (format!("{} assuming {}", print(a), print(b)), P_SEQ),
        Expr::AngleCol(v) => (format!("<{}>", angle_items(v, ", ")), P_ATOM),
        Expr::AngleRow(v) => (format!("<{}>", angle_items(v, " | ")), P_ATOM),
    }
}

fn angle_items(v: &[Expr], sep: &str) -> String {
    // relations print with `<`, so a closing `>` is never ambiguous
    v.iter().map(|x| pr(x, P_SEQ + 1)).collect::<Vec<_>>().join(sep)
}

fn render_fn(name: &str, args: &[Expr]) -> (String, u8) {
    match (name, args.len()) {
        ("factorial", 1) => return (format!("{}!", pr(&args[0], P_ATOM)), P_POW + 1),
        ("mod", 2) => return (format!("{} mod {}", pr(&args[0], P_MOD + 1), pr(&args[1], P_MOD + 1)), P_MOD),
        ("union", 2) | ("minus", 2) => {
            return (format!("{} {} {}", pr(&args[0], P_ADD), name, pr(&args[1], P_ADD + 1)), P_ADD)
        }
        ("intersect", 2) => {
            return (format!("{} intersect {}", pr(&args[0], P_MUL), pr(&args[1], P_MUL + 1)), P_MUL)
        }
        ("subset", 2) => return (format!("{} subset {}", pr(&args[0], P_REL + 1), pr(&args[1], P_REL + 1)), P_REL),
        _ => {}
    }
    (format!("{}({})", name_text(name), join(args, P_SEQ + 1)), P_ATOM)
}

fn params_text(p: &Proc) -> String {
    p.params
        .iter()
        .map(|x| {
            let mut s = name_text(&x.name);
            if let Some(t) = &x.ty {
                s = format!("{}::{}", s, pr(t, P_RANGE));
            }
            if let Some(d) = &x.default {
                s = format!("{} := {}", s, pr(d, P_SEQ + 1));
            }
            s
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn render_proc(p: &Proc) -> (String, u8) {
    if p.arrow {
        if let [Stmt::Expr(body)] = p.body.as_slice() {
            let params = if p.params.len() == 1 && p.params[0].ty.is_none() {
                name_text(&p.params[0].name)
            } else {
                format!("({})", params_text(p))
            };
            return (format!("{} -> {}", params, pr(body, P_ARROW)), P_ARROW);
        }
    }
    let mut s = format!("proc({})", params_text(p));
    if !p.locals.is_empty() {
        s.push_str(&format!(" local {};", p.locals.iter().map(|x| name_text(x)).collect::<Vec<_>>().join(", ")));
    }
    if !p.globals.is_empty() {
        s.push_str(&format!(" global {};", p.globals.iter().map(|x| name_text(x)).collect::<Vec<_>>().join(", ")));
    }
    let opts: Vec<&String> = p.options.iter().filter(|o| *o != "operator" && *o != "arrow").collect();
    if !opts.is_empty() {
        s.push_str(&format!(" option {};", opts.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")));
    }
    let body = block_text(&p.body);
    if !body.is_empty() {
        s.push(' ');
        s.push_str(&body);
    }
    s.push_str(" end proc");
    (s, P_ATOM)
}

fn block_text(b: &[Stmt]) -> String {
    b.iter().map(stmt_text).collect::<Vec<_>>().join("; ")
}

/// One-line text of a statement.
pub fn stmt_text(s: &Stmt) -> String {
    match s {
        Stmt::Expr(e) => print(e),
        Stmt::Assign(t, v) => format!("{} := {}", join(t, P_SEQ + 1), print(v)),
        Stmt::If(branches, els) => {
            let mut out = String::new();
            for (i, (c, b)) in branches.iter().enumerate() {
                out.push_str(if i == 0 { "if " } else { " elif " });
                out.push_str(&print(c));
                out.push_str(" then ");
                out.push_str(&block_text(b));
            }
            if let Some(e) = els {
                out.push_str(" else ");
                out.push_str(&block_text(e));
            }
            out.push_str(" end if");
            out
        }
        Stmt::For(lp) => {
            let mut out = String::new();
            if let Some(v) = &lp.var {
                out.push_str(&format!("for {} ", name_text(v)));
            }
            if let Some(o) = &lp.over {
                out.push_str(&format!("in {} ", print(o)));
            }
            if let Some(x) = &lp.from {
                out.push_str(&format!("from {} ", print(x)));
            }
            if let Some(x) = &lp.by {
                out.push_str(&format!("by {} ", print(x)));
            }
            if let Some(x) = &lp.to {
                out.push_str(&format!("to {} ", print(x)));
            }
            if let Some(x) = &lp.cond {
                out.push_str(&format!("while {} ", print(x)));
            }
            out.push_str(&format!("do {} end do", block_text(&lp.body)));
            out
        }
        Stmt::Return(Some(e)) => format!("return {}", print(e)),
        Stmt::Return(None) => "return".into(),
        Stmt::Break => "break".into(),
        Stmt::Next => "next".into(),
        Stmt::Error(args) => format!("error {}", join(args, P_SEQ + 1)),
        Stmt::Read(e) => format!("read {}", print(e)),
        Stmt::Try(b, catches, fin) => {
            let mut out = format!("try {}", block_text(b));
            for (pat, cb) in catches {
                match pat {
                    Some(p) => out.push_str(&format!(" catch {}: {}", print(p), block_text(cb))),
                    None => out.push_str(&format!(" catch: {}", block_text(cb))),
                }
            }
            if let Some(f) = fin {
                out.push_str(&format!(" finally {}", block_text(f)));
            }
            out.push_str(" end try");
            out
        }
    }
}
