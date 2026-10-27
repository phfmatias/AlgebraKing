//   AlgebraKing //
//
//   File:         [ast.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Expression tree shared by the parser, the interpreter and the printer.']

use crate::number::Num;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelOp {
    Eq,
    Neq,
    Lt,
    Le,
}

/// One Maple object. Parsed code and computed values share this type
/// (in Maple, code is data); a few variants exist only before evaluation.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(Num),
    Sym(String),
    Str(String),
    Bool(bool),
    Fail,
    Const(String),
    Add(Vec<Expr>),
    Mul(Vec<Expr>),
    Pow(Box<Expr>, Box<Expr>),
    Fn(String, Vec<Expr>),
    Apply(Box<Expr>, Vec<Expr>),
    Idx(Box<Expr>, Vec<Expr>),
    Rel(RelOp, Box<Expr>, Box<Expr>),
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Xor(Box<Expr>, Box<Expr>),
    Implies(Box<Expr>, Box<Expr>),
    List(Vec<Expr>),
    Set(Vec<Expr>),
    Seq(Vec<Expr>),
    Range(Box<Expr>, Box<Expr>),
    Matrix(Vec<Vec<Expr>>),
    Vector(Vec<Expr>, bool),
    Proc(Rc<Proc>),
    Table(Vec<(Expr, Expr)>),
    Member(String, String),
    Compose(Vec<Expr>),
    Repeat(Box<Expr>, Box<Expr>),
    TypeAssert(Box<Expr>, Box<Expr>),
    In(Box<Expr>, Box<Expr>),
    Uneval(Box<Expr>),
    // parse-only nodes
    Hist(u8),
    Dot(Box<Expr>, Box<Expr>),
    Concat(Box<Expr>, Box<Expr>),
    Dollar(Option<Box<Expr>>, Box<Expr>),
    Assuming(Box<Expr>, Box<Expr>),
    AngleCol(Vec<Expr>),
    AngleRow(Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: Option<Expr>,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Proc {
    pub params: Vec<Param>,
    pub locals: Vec<String>,
    pub globals: Vec<String>,
    pub options: Vec<String>,
    pub body: Vec<Stmt>,
    pub arrow: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForLoop {
    pub var: Option<String>,
    pub from: Option<Expr>,
    pub by: Option<Expr>,
    pub to: Option<Expr>,
    pub over: Option<Expr>,
    pub cond: Option<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Expr(Expr),
    Assign(Vec<Expr>, Expr),
    If(Vec<(Expr, Vec<Stmt>)>, Option<Vec<Stmt>>),
    For(Box<ForLoop>),
    Return(Option<Expr>),
    Break,
    Next,
    Error(Vec<Expr>),
    Try(Vec<Stmt>, Vec<(Option<Expr>, Vec<Stmt>)>, Option<Vec<Stmt>>),
    Read(Expr),
}

/// A top-level statement and whether its terminator was `;` (show) or `:`.
#[derive(Clone, Debug, PartialEq)]
pub struct TopStmt {
    pub stmt: Stmt,
    pub show: bool,
}

pub const CONSTANTS: [&str; 6] = ["Pi", "I", "infinity", "gamma", "Catalan", "undefined"];

impl Expr {
    pub fn int(i: i64) -> Expr {
        Expr::Num(Num::int(i))
    }

    pub fn sym(s: &str) -> Expr {
        Expr::Sym(s.to_string())
    }

    pub fn null() -> Expr {
        Expr::Seq(vec![])
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Expr::Seq(v) if v.is_empty())
    }

    pub fn as_num(&self) -> Option<&Num> {
        match self {
            Expr::Num(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_num().and_then(|n| n.as_i64())
    }

    pub fn rel(op: RelOp, a: Expr, b: Expr) -> Expr {
        Expr::Rel(op, Box::new(a), Box::new(b))
    }

    pub fn eq(a: Expr, b: Expr) -> Expr {
        Expr::rel(RelOp::Eq, a, b)
    }

    pub fn func(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Fn(name.to_string(), args)
    }

    /// Raw (unsimplified) negation used by the parser.
    pub fn raw_neg(e: Expr) -> Expr {
        match e {
            Expr::Num(n) => Expr::Num(n.neg()),
            other => Expr::Mul(vec![Expr::int(-1), other]),
        }
    }

    /// Elements of an expression sequence (a non-sequence is a sequence of one).
    pub fn into_seq(self) -> Vec<Expr> {
        match self {
            Expr::Seq(v) => v,
            e => vec![e],
        }
    }

    /// Build a sequence, collapsing a single element.
    pub fn from_seq(mut v: Vec<Expr>) -> Expr {
        if v.len() == 1 {
            v.pop().unwrap()
        } else {
            Expr::Seq(v)
        }
    }

    pub fn contains_sym(&self, name: &str) -> bool {
        let mut found = false;
        self.walk(&mut |e| {
            if let Expr::Sym(s) = e {
                if s == name {
                    found = true;
                }
            }
        });
        found
    }

    /// Visit every sub-expression (pre-order), not entering procedure bodies.
    pub fn walk(&self, f: &mut dyn FnMut(&Expr)) {
        f(self);
        for c in self.children() {
            c.walk(f);
        }
    }

    pub fn children(&self) -> Vec<&Expr> {
        match self {
            Expr::Add(v) | Expr::Mul(v) | Expr::And(v) | Expr::Or(v) | Expr::List(v) | Expr::Set(v) | Expr::Seq(v) => {
                v.iter().collect()
            }
            Expr::Compose(v) | Expr::AngleCol(v) | Expr::AngleRow(v) => v.iter().collect(),
            Expr::Fn(_, v) => v.iter().collect(),
            Expr::Apply(h, v) | Expr::Idx(h, v) => std::iter::once(h.as_ref()).chain(v.iter()).collect(),
            Expr::Pow(a, b)
            | Expr::Rel(_, a, b)
            | Expr::Xor(a, b)
            | Expr::Implies(a, b)
            | Expr::Range(a, b)
            | Expr::Repeat(a, b)
            | Expr::TypeAssert(a, b)
            | Expr::In(a, b)
            | Expr::Dot(a, b)
            | Expr::Concat(a, b)
            | Expr::Assuming(a, b) => vec![a.as_ref(), b.as_ref()],
            Expr::Dollar(a, b) => a.iter().map(|x| x.as_ref()).chain(std::iter::once(b.as_ref())).collect(),
            Expr::Not(a) | Expr::Uneval(a) => vec![a.as_ref()],
            Expr::Matrix(rows) => rows.iter().flatten().collect(),
            Expr::Vector(v, _) => v.iter().collect(),
            Expr::Table(t) => t.iter().flat_map(|(k, v)| [k, v]).collect(),
            _ => vec![],
        }
    }

    /// Rebuild this node with each child mapped by `f` (structure only, no simplification).
    pub fn map_children(&self, f: &mut dyn FnMut(&Expr) -> Expr) -> Expr {
        let mv = |v: &Vec<Expr>, f: &mut dyn FnMut(&Expr) -> Expr| v.iter().map(f).collect::<Vec<_>>();
        let b = |e: &Expr, f: &mut dyn FnMut(&Expr) -> Expr| Box::new(f(e));
        match self {
            Expr::Add(v) => Expr::Add(mv(v, f)),
            Expr::Mul(v) => Expr::Mul(mv(v, f)),
            Expr::And(v) => Expr::And(mv(v, f)),
            Expr::Or(v) => Expr::Or(mv(v, f)),
            Expr::List(v) => Expr::List(mv(v, f)),
            Expr::Set(v) => Expr::Set(mv(v, f)),
            Expr::Seq(v) => Expr::Seq(mv(v, f)),
            Expr::Compose(v) => Expr::Compose(mv(v, f)),
            Expr::AngleCol(v) => Expr::AngleCol(mv(v, f)),
            Expr::AngleRow(v) => Expr::AngleRow(mv(v, f)),
            Expr::Fn(n, v) => Expr::Fn(n.clone(), mv(v, f)),
            Expr::Apply(h, v) => Expr::Apply(b(h, f), mv(v, f)),
            Expr::Idx(h, v) => Expr::Idx(b(h, f), mv(v, f)),
            Expr::Pow(x, y) => Expr::Pow(b(x, f), b(y, f)),
            Expr::Rel(op, x, y) => Expr::Rel(*op, b(x, f), b(y, f)),
            Expr::Xor(x, y) => Expr::Xor(b(x, f), b(y, f)),
            Expr::Implies(x, y) => Expr::Implies(b(x, f), b(y, f)),
            Expr::Range(x, y) => Expr::Range(b(x, f), b(y, f)),
            Expr::Repeat(x, y) => Expr::Repeat(b(x, f), b(y, f)),
            Expr::TypeAssert(x, y) => Expr::TypeAssert(b(x, f), b(y, f)),
            Expr::In(x, y) => Expr::In(b(x, f), b(y, f)),
            Expr::Dot(x, y) => Expr::Dot(b(x, f), b(y, f)),
            Expr::Concat(x, y) => Expr::Concat(b(x, f), b(y, f)),
            Expr::Assuming(x, y) => Expr::Assuming(b(x, f), b(y, f)),
            Expr::Dollar(x, y) => Expr::Dollar(x.as_ref().map(|x| b(x, f)), b(y, f)),
            Expr::Not(x) => Expr::Not(b(x, f)),
            Expr::Uneval(x) => Expr::Uneval(b(x, f)),
            Expr::Matrix(rows) => Expr::Matrix(rows.iter().map(|r| mv(r, f)).collect()),
            Expr::Vector(v, row) => Expr::Vector(mv(v, f), *row),
            Expr::Table(t) => Expr::Table(t.iter().map(|(k, v)| (f(k), f(v))).collect()),
            other => other.clone(),
        }
    }
}
