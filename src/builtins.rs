//   AlgebraKing //
//
//   File:         [builtins.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Language-level builtins (seq, map, op, eval, subs, D, printf, packages, ...).']

use crate::ast::{Expr, Param, Proc, RelOp, Stmt};
use crate::interp::{err, is_container, operands_for_iteration, Exc, Interp, Out, R};
use crate::number::Num;
use crate::printer::print;
use crate::simpl;
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{One, Signed, ToPrimitive, Zero};
use serde_json::{json, Value};
use std::rc::Rc;

// ------------------------------------------------------------------ packages

const LINALG: &[&str] = &[
    "Add", "Adjoint", "BackwardSubstitute", "BandMatrix", "Basis", "BezoutMatrix", "BidiagonalForm",
    "CharacteristicMatrix", "CharacteristicPolynomial", "CholeskyDecomposition", "Column", "ColumnDimension",
    "ColumnOperation", "ColumnSpace", "CompanionMatrix", "ConditionNumber", "ConstantMatrix", "ConstantVector",
    "Copy", "CreatePermutation", "CrossProduct", "DeleteColumn", "DeleteRow", "Determinant", "Diagonal",
    "DiagonalMatrix", "Dimension", "Dimensions", "DotProduct", "EigenConditionNumbers", "Eigenvalues",
    "Eigenvectors", "Equal", "ForwardSubstitute", "FrobeniusForm", "GaussianElimination", "GenerateEquations",
    "GenerateMatrix", "GivensRotationMatrix", "GramSchmidt", "HankelMatrix", "HermiteForm", "HermitianTranspose",
    "HessenbergForm", "HilbertMatrix", "HouseholderMatrix", "IdentityMatrix", "IntersectionBasis", "IsDefinite",
    "IsOrthogonal", "IsSimilar", "IsUnitary", "JordanBlockMatrix", "JordanForm", "KroneckerProduct",
    "LUDecomposition", "LeastSquares", "LinearSolve", "Map", "Map2", "MatrixAdd", "MatrixExponential",
    "MatrixFunction", "MatrixInverse", "MatrixMatrixMultiply", "MatrixNorm", "MatrixPower", "MatrixScalarMultiply",
    "MatrixVectorMultiply", "MinimalPolynomial", "Minor", "Multiply", "Norm", "Normalize", "NullSpace",
    "OuterProductMatrix", "Permanent", "Pivot", "PopovForm", "QRDecomposition", "RandomMatrix", "RandomVector",
    "Rank", "RationalCanonicalForm", "ReducedRowEchelonForm", "Row", "RowDimension", "RowOperation", "RowSpace",
    "ScalarMatrix", "ScalarMultiply", "ScalarVector", "SchurForm", "SingularValues", "SmithForm", "SubMatrix",
    "SubVector", "SumBasis", "SylvesterMatrix", "ToeplitzMatrix", "Trace", "Transpose", "TridiagonalForm",
    "UnitVector", "VandermondeMatrix", "VectorAdd", "VectorAngle", "VectorMatrixMultiply", "VectorNorm",
    "VectorScalarMultiply", "ZeroMatrix", "ZeroVector", "Zip",
];
const INTTRANS: &[&str] = &[
    "addtable", "fourier", "fouriercos", "fouriersin", "hankel", "hilbert", "invfourier", "invhilbert",
    "invlaplace", "invmellin", "laplace", "mellin", "savetable",
];
const VECCALC: &[&str] = &[
    "ArcLength", "Curl", "Curvature", "Del", "DirectionalDiff", "Divergence", "Flux", "Gradient", "Hessian",
    "Jacobian", "Laplacian", "LineInt", "Nabla", "PathInt", "ScalarPotential", "SetCoordinates", "SurfaceInt",
    "TangentVector", "Torsion", "VectorField", "VectorPotential", "Wronskian", "CrossProduct", "DotProduct",
];
const STUDENT: &[&str] = &[
    "changevar", "completesquare", "distance", "Doubleint", "equate", "integrand", "intercept", "intparts",
    "isolate", "leftbox", "leftsum", "Lineint", "makeproc", "middlebox", "middlesum", "midpoint", "powsubs",
    "rightbox", "rightsum", "showtangent", "simpson", "slope", "summand", "trapezoid", "Tripleint",
];
const NUMTHEORY: &[&str] = &[
    "bigomega", "cyclotomic", "divisors", "factorset", "invphi", "issqrfree", "jacobi", "lambda", "legendre",
    "mlog", "mobius", "mroot", "msqrt", "nthpow", "order", "phi", "pprimroot", "primroot", "quadres", "safeprime",
    "sigma", "tau", "thue",
];
const GROEBNER: &[&str] = &[
    "Basis", "HilbertPolynomial", "IsZeroDimensional", "LeadingCoefficient", "LeadingMonomial", "LeadingTerm",
    "NormalForm", "Reduce", "Solve", "Walk",
];
const COMBINAT: &[&str] = &[
    "bell", "cartprod", "character", "choose", "composition", "conjpart", "decodepart", "encodepart",
    "fibonacci", "firstperm", "inttovec", "lastpart", "list2multiset", "multinomial", "nextperm", "numbcomb",
    "numbcomp", "numbpart", "numbperm", "partition", "permute", "powerset", "prevperm", "randcomb", "randpart",
    "randperm", "rankperm", "stirling1", "stirling2", "subsets", "unrankperm", "vectoint",
];
const LISTTOOLS: &[&str] = &[
    "BinaryPlace", "BinarySearch", "Categorize", "DotProduct", "Enumerate", "FindMaximalElement",
    "FindMinimalElement", "Flatten", "FlattenOnce", "Interleave", "MakeUnique", "Occurrences", "Reverse", "Rotate",
    "Search", "SearchAll", "SelectFirst", "Split", "Transpose",
];
const DETOOLS: &[&str] = &["odeadvisor", "DEplot", "dfieldplot", "phaseportrait", "reduceOrder", "Wronskian"];

pub const PACKAGES: &[(&str, &[&str])] = &[
    ("LinearAlgebra", LINALG),
    ("inttrans", INTTRANS),
    ("VectorCalculus", VECCALC),
    ("student", STUDENT),
    ("numtheory", NUMTHEORY),
    ("Groebner", GROEBNER),
    ("combinat", COMBINAT),
    ("ListTools", LISTTOOLS),
    ("DEtools", DETOOLS),
];

pub fn is_package(p: &str) -> bool {
    PACKAGES.iter().any(|(n, _)| *n == p)
        || matches!(p, "Student" | "plots" | "PDEtools" | "Statistics" | "StringTools" | "polytools")
}

fn exports(p: &str) -> &'static [&'static str] {
    PACKAGES.iter().find(|(n, _)| *n == p).map(|(_, e)| *e).unwrap_or(&[])
}

pub fn exports_name(p: &str, name: &str) -> bool {
    exports(p).contains(&name)
}

/// First package (in table order) that exports `name`.
pub fn package_of(name: &str) -> Option<&'static str> {
    PACKAGES.iter().find(|(_, e)| e.contains(&name)).map(|(n, _)| *n)
}

const SPECIAL_EVAL: &[&str] = &["seq", "add", "mul", "assigned", "evaln", "if", "ifelse", "unassign", "evalf"];

pub fn is_special_eval(n: &str) -> bool {
    SPECIAL_EVAL.contains(&n)
}

const PROTECTED: &[&str] = &[
    "sin", "cos", "tan", "sec", "csc", "cot", "sinh", "cosh", "tanh", "sech", "csch", "coth", "arcsin", "arccos",
    "arctan", "arcsec", "arccsc", "arccot", "arcsinh", "arccosh", "arctanh", "exp", "ln", "log", "log10", "sqrt",
    "abs", "int", "diff", "solve", "simplify", "expand", "factor", "sum", "product", "limit", "series", "evalf",
    "D", "Int", "Diff", "Sum", "Product", "Limit", "seq", "add", "mul", "map", "op", "nops", "subs", "eval",
    "Matrix", "Vector", "print", "printf", "lprint", "assume", "is", "binomial", "factorial", "max", "min",
    "floor", "ceil", "round", "trunc", "frac", "signum", "dsolve", "fsolve", "taylor", "coeff", "degree", "numer",
    "denom", "normal", "collect", "convert", "type", "whattype", "with", "unapply", "evalb", "evalc", "Re", "Im",
    "conjugate", "argument", "GAMMA", "Zeta", "erf", "piecewise", "Heaviside", "Dirac", "list", "set", "integer",
    "rational", "float", "string", "symbol", "name", "true", "false", "FAIL", "NULL", "lhs", "rhs", "select",
    "remove", "zip", "map2", "args", "nargs", "procname", "table", "restart", "time", "latex", "about",
];

pub fn is_protected(n: &str) -> bool {
    PROTECTED.contains(&n)
}

// ------------------------------------------------------------------ helpers

fn ints_of(e: &Expr) -> Option<i64> {
    e.as_i64()
}

fn as_list(e: &Expr) -> Option<Vec<Expr>> {
    match e {
        Expr::List(v) | Expr::Set(v) => Some(v.clone()),
        _ => None,
    }
}

fn eqs_of(e: &Expr) -> R<Vec<(Expr, Expr)>> {
    let mut out = Vec::new();
    let items = match e {
        Expr::List(v) | Expr::Set(v) | Expr::Seq(v) => v.clone(),
        other => vec![other.clone()],
    };
    for x in items {
        match x {
            Expr::Rel(RelOp::Eq, a, b) => out.push((*a, *b)),
            other => {
                return err(format!(
                    "invalid input: substitution must be an equation, but received {}",
                    print(&other)
                ))
            }
        }
    }
    Ok(out)
}

/// Structural replacement of whole sub-expressions.
fn replace(e: &Expr, pairs: &[(Expr, Expr)]) -> Expr {
    if let Some((_, r)) = pairs.iter().find(|(l, _)| l == e) {
        return r.clone();
    }
    e.map_children(&mut |c| replace(c, pairs))
}

const BINDERS: &[&str] = &["int", "Int", "sum", "Sum", "product", "Product", "limit", "Limit", "seq", "add", "mul"];

/// Replacement for `eval(e, x = a)`: respects bound variables and turns diff into D at points.
fn eval_replace(e: &Expr, pairs: &[(Expr, Expr)]) -> Expr {
    if let Some((_, r)) = pairs.iter().find(|(l, _)| l == e) {
        return r.clone();
    }
    if let Expr::Fn(name, args) = e {
        if BINDERS.contains(&name.as_str()) && args.len() >= 2 {
            let bound = match &args[1] {
                Expr::Sym(_) => Some(args[1].clone()),
                Expr::Rel(RelOp::Eq, v, _) if matches!(v.as_ref(), Expr::Sym(_)) => Some((**v).clone()),
                _ => None,
            };
            if let Some(b) = bound {
                let inner: Vec<(Expr, Expr)> = pairs.iter().filter(|(l, _)| *l != b).cloned().collect();
                let mut out = vec![eval_replace(&args[0], &inner)];
                out.push(match &args[1] {
                    Expr::Rel(RelOp::Eq, v, r) => Expr::eq((**v).clone(), eval_replace(r, pairs)),
                    other => other.clone(),
                });
                out.extend(args[2..].iter().map(|a| eval_replace(a, pairs)));
                return Expr::Fn(name.clone(), out);
            }
        }
        if name == "diff" && args.len() >= 2 {
            if let Expr::Fn(g, gargs) = &args[0] {
                if gargs.len() == 1 && args[1..].iter().all(|v| *v == gargs[0]) {
                    if let Some((_, pt)) = pairs.iter().find(|(l, _)| *l == gargs[0]) {
                        if !matches!(pt, Expr::Sym(_)) {
                            let n = args.len() - 1;
                            let head = if n == 1 {
                                Expr::Fn("D".into(), vec![Expr::sym(g)])
                            } else {
                                Expr::Apply(
                                    Box::new(Expr::Repeat(Box::new(Expr::sym("D")), Box::new(Expr::int(n as i64)))),
                                    vec![Expr::sym(g)],
                                )
                            };
                            return Expr::Apply(Box::new(head), vec![pt.clone()]);
                        }
                    }
                }
            }
        }
    }
    e.map_children(&mut |c| eval_replace(c, pairs))
}

fn contains_unknown_derivative(e: &Expr) -> bool {
    let mut found = false;
    e.walk(&mut |x| match x {
        Expr::Fn(n, _) if n == "diff" || n == "Diff" || n == "D" => found = true,
        Expr::Apply(h, _) if matches!(h.as_ref(), Expr::Fn(n, _) if n == "D") => found = true,
        _ => {}
    });
    found
}

fn arrow(params: Vec<String>, body: Expr) -> Expr {
    Expr::Proc(Rc::new(Proc {
        params: params.into_iter().map(|name| Param { name, ty: None, default: None }).collect(),
        locals: vec![],
        globals: vec![],
        options: vec!["operator".into(), "arrow".into()],
        body: vec![Stmt::Expr(body)],
        arrow: true,
    }))
}

fn operands(e: &Expr) -> R<Vec<Expr>> {
    Ok(match e {
        Expr::Add(v) | Expr::Mul(v) | Expr::List(v) | Expr::Set(v) | Expr::And(v) | Expr::Or(v) => v.clone(),
        Expr::Fn(_, a) => a.clone(),
        Expr::Apply(_, a) => a.clone(),
        Expr::Idx(_, i) => i.clone(),
        Expr::Pow(a, b) | Expr::Rel(_, a, b) | Expr::Range(a, b) | Expr::Xor(a, b) | Expr::Implies(a, b) => {
            vec![(**a).clone(), (**b).clone()]
        }
        Expr::Not(a) => vec![(**a).clone()],
        Expr::Num(Num::Rat(p, q)) => vec![Expr::Num(Num::Int(p.clone())), Expr::Num(Num::Int(q.clone()))],
        Expr::Num(Num::Float(m, x)) => vec![Expr::Num(Num::Int(m.clone())), Expr::int(*x)],
        Expr::Vector(v, _) => vec![Expr::int(v.len() as i64), Expr::Set(enumerate_entries(v))],
        Expr::Matrix(r) => vec![
            Expr::int(r.len() as i64),
            Expr::int(r.first().map(|x| x.len()).unwrap_or(0) as i64),
            Expr::Set(
                r.iter()
                    .enumerate()
                    .flat_map(|(i, row)| {
                        row.iter().enumerate().filter(|(_, x)| **x != Expr::int(0)).map(move |(j, x)| {
                            Expr::eq(Expr::Seq(vec![Expr::int(i as i64 + 1), Expr::int(j as i64 + 1)]), x.clone())
                        })
                    })
                    .collect(),
            ),
        ],
        Expr::Table(t) => vec![Expr::List(t.iter().map(|(k, v)| Expr::eq(k.clone(), v.clone())).collect())],
        Expr::Seq(_) => return err("invalid input: op expects 1 or 2 arguments, but received a sequence"),
        other => vec![other.clone()],
    })
}

fn enumerate_entries(v: &[Expr]) -> Vec<Expr> {
    v.iter()
        .enumerate()
        .filter(|(_, x)| **x != Expr::int(0))
        .map(|(i, x)| Expr::eq(Expr::int(i as i64 + 1), x.clone()))
        .collect()
}

fn op_zero(e: &Expr) -> Expr {
    match e {
        Expr::Add(_) => Expr::sym("+"),
        Expr::Mul(_) => Expr::sym("*"),
        Expr::Pow(..) => Expr::sym("^"),
        Expr::Fn(n, _) => Expr::sym(n),
        Expr::Apply(h, _) | Expr::Idx(h, _) => (**h).clone(),
        Expr::List(_) => Expr::sym("list"),
        Expr::Set(_) => Expr::sym("set"),
        Expr::Rel(op, ..) => Expr::sym(match op {
            RelOp::Eq => "=",
            RelOp::Neq => "<>",
            RelOp::Lt => "<",
            RelOp::Le => "<=",
        }),
        Expr::Range(..) => Expr::sym(".."),
        Expr::And(_) => Expr::sym("and"),
        Expr::Or(_) => Expr::sym("or"),
        Expr::Not(_) => Expr::sym("not"),
        Expr::Num(Num::Int(_)) => Expr::sym("Integer"),
        Expr::Num(Num::Rat(..)) => Expr::sym("Fraction"),
        Expr::Num(Num::Float(..)) => Expr::sym("Float"),
        Expr::Sym(_) => Expr::sym("symbol"),
        Expr::Str(_) => Expr::sym("string"),
        Expr::Matrix(_) => Expr::sym("Matrix"),
        Expr::Vector(_, row) => Expr::Idx(Box::new(Expr::sym("Vector")), vec![Expr::sym(if *row { "row" } else { "column" })]),
        _ => Expr::null(),
    }
}

/// Rebuild `e` from new operands (same node kind).
fn rebuild(e: &Expr, ops: Vec<Expr>, d: u32) -> R<Expr> {
    Ok(match e {
        Expr::Add(_) => simpl::add(ops, d)?,
        Expr::Mul(_) => simpl::mul(ops, d)?,
        Expr::Pow(..) if ops.len() == 2 => {
            let mut it = ops.into_iter();
            simpl::pow(it.next().unwrap(), it.next().unwrap(), d)?
        }
        Expr::List(_) => Expr::List(ops),
        Expr::Set(_) => simpl::make_set(ops),
        Expr::Fn(n, _) => Expr::Fn(n.clone(), ops),
        Expr::Apply(h, _) => Expr::Apply(h.clone(), ops),
        Expr::Idx(h, _) => Expr::Idx(h.clone(), ops),
        Expr::Rel(op, ..) if ops.len() == 2 => Expr::rel(*op, ops[0].clone(), ops[1].clone()),
        Expr::Range(..) if ops.len() == 2 => Expr::Range(Box::new(ops[0].clone()), Box::new(ops[1].clone())),
        Expr::And(_) => Expr::And(ops),
        Expr::Or(_) => Expr::Or(ops),
        Expr::Not(_) if ops.len() == 1 => Expr::Not(Box::new(ops[0].clone())),
        _ => return err("cannot rebuild this expression"),
    })
}

fn whattype(e: &Expr) -> Expr {
    let s = match e {
        Expr::Num(Num::Int(_)) => "integer",
        Expr::Num(Num::Rat(..)) => "fraction",
        Expr::Num(Num::Float(..)) => "float",
        Expr::Sym(_) => "symbol",
        Expr::Str(_) => "string",
        Expr::Bool(_) | Expr::Fail => "boolean",
        Expr::Const(_) => "symbol",
        Expr::Add(_) => "+",
        Expr::Mul(_) => "*",
        Expr::Pow(..) => "^",
        Expr::Fn(..) | Expr::Apply(..) => "function",
        Expr::Idx(..) => "indexed",
        Expr::Rel(RelOp::Eq, ..) => "=",
        Expr::Rel(RelOp::Neq, ..) => "<>",
        Expr::Rel(RelOp::Lt, ..) => "<",
        Expr::Rel(RelOp::Le, ..) => "<=",
        Expr::And(_) => "and",
        Expr::Or(_) => "or",
        Expr::Not(_) => "not",
        Expr::List(_) => "list",
        Expr::Set(_) => "set",
        Expr::Seq(_) => "exprseq",
        Expr::Range(..) => "..",
        Expr::Matrix(_) => "Matrix",
        Expr::Vector(_, true) => return Expr::Idx(Box::new(Expr::sym("Vector")), vec![Expr::sym("row")]),
        Expr::Vector(_, false) => return Expr::Idx(Box::new(Expr::sym("Vector")), vec![Expr::sym("column")]),
        Expr::Proc(_) => "procedure",
        Expr::Table(_) => "table",
        Expr::Member(..) => "module",
        _ => "function",
    };
    Expr::sym(s)
}

type ArgBuilder = Box<dyn Fn(Expr) -> Vec<Expr>>;

// printf-style formatting -------------------------------------------------------

fn c_exp(v: f64, prec: usize, upper: bool) -> String {
    let s = format!("{:.*e}", prec, v);
    let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
    let ev: i32 = e.parse().unwrap_or(0);
    let out = format!("{}e{}{:02}", m, if ev < 0 { '-' } else { '+' }, ev.abs());
    if upper {
        out.to_uppercase()
    } else {
        out
    }
}

fn c_general(v: f64, prec: usize, alt: bool, upper: bool) -> String {
    let p = if prec == 0 { 1 } else { prec };
    if v == 0.0 {
        return "0".into();
    }
    let x = v.abs().log10().floor() as i32;
    let mut s = if x < -4 || x >= p as i32 {
        c_exp(v, p - 1, upper)
    } else {
        format!("{:.*}", (p as i32 - 1 - x).max(0) as usize, v)
    };
    if !alt {
        if let Some(epos) = s.find(['e', 'E']) {
            let (m, e) = s.split_at(epos);
            let m = if m.contains('.') { m.trim_end_matches('0').trim_end_matches('.') } else { m };
            s = format!("{}{}", m, e);
        } else if s.contains('.') {
            s = s.trim_end_matches('0').trim_end_matches('.').to_string();
        }
    }
    s
}

impl Interp {
    fn text_of(&self, e: &Expr) -> String {
        match e {
            Expr::Str(s) => s.clone(),
            other => print(other),
        }
    }

    /// Procedures and tables print as their value (Maple last-name evaluation is undone).
    fn named_value(&self, e: &Expr) -> Expr {
        match e {
            Expr::Sym(n) => match self.lookup_raw(n) {
                Some(v @ (Expr::Proc(_) | Expr::Table(_))) => v,
                _ => e.clone(),
            },
            other => other.clone(),
        }
    }

    pub fn format(&mut self, fmt: &str, args: &[Expr]) -> R<String> {
        let chars: Vec<char> = fmt.chars().collect();
        let mut out = String::new();
        let mut ai = 0usize;
        let mut i = 0usize;
        let next = |ai: &mut usize| -> R<Expr> {
            let a = args.get(*ai).cloned();
            *ai += 1;
            a.ok_or_else(|| Exc::Err("(in printf) insufficient arguments for format".into()))
        };
        while i < chars.len() {
            let c = chars[i];
            if c != '%' {
                out.push(c);
                i += 1;
                continue;
            }
            i += 1;
            if i >= chars.len() {
                out.push('%');
                break;
            }
            if chars[i] == '%' {
                out.push('%');
                i += 1;
                continue;
            }
            let (mut left, mut plus, mut space, mut zero, mut alt) = (false, false, false, false, false);
            while i < chars.len() && "-+ 0#".contains(chars[i]) {
                match chars[i] {
                    '-' => left = true,
                    '+' => plus = true,
                    ' ' => space = true,
                    '0' => zero = true,
                    _ => alt = true,
                }
                i += 1;
            }
            let mut width = 0usize;
            if i < chars.len() && chars[i] == '*' {
                width = next(&mut ai)?.as_i64().unwrap_or(0).max(0) as usize;
                i += 1;
            }
            while i < chars.len() && chars[i].is_ascii_digit() {
                width = width * 10 + chars[i].to_digit(10).unwrap() as usize;
                i += 1;
            }
            let mut prec: Option<usize> = None;
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                let mut p = 0usize;
                if i < chars.len() && chars[i] == '*' {
                    p = next(&mut ai)?.as_i64().unwrap_or(0).max(0) as usize;
                    i += 1;
                }
                while i < chars.len() && chars[i].is_ascii_digit() {
                    p = p * 10 + chars[i].to_digit(10).unwrap() as usize;
                    i += 1;
                }
                prec = Some(p);
            }
            while i < chars.len() && "lhLqQ".contains(chars[i]) && !(chars[i] == 'q' || chars[i] == 'Q') {
                i += 1;
            }
            if i >= chars.len() {
                break;
            }
            let conv = chars[i];
            i += 1;
            let numeric = "dieEfgGxXoc".contains(conv);
            let body = match conv {
                'd' | 'i' => {
                    let a = next(&mut ai)?;
                    match &a {
                        Expr::Num(Num::Int(n)) => n.to_string(),
                        _ => {
                            let n = self.numeric_value(&a)?;
                            match n {
                                Num::Int(n) => n.to_string(),
                                _ => return err("(in printf) integer expected for integer format"),
                            }
                        }
                    }
                }
                'x' | 'X' | 'o' => {
                    let a = next(&mut ai)?;
                    let Some(Num::Int(n)) = a.as_num().cloned() else {
                        return err("(in printf) integer expected for integer format");
                    };
                    match conv {
                        'x' => n.to_str_radix(16),
                        'X' => n.to_str_radix(16).to_uppercase(),
                        _ => n.to_str_radix(8),
                    }
                }
                'e' | 'E' | 'f' | 'F' | 'g' | 'G' => {
                    let a = next(&mut ai)?;
                    let v = self.numeric_value(&a)?.to_f64();
                    let p = prec.unwrap_or(6);
                    match conv {
                        'e' | 'E' => c_exp(v, p, conv == 'E'),
                        'f' | 'F' => format!("{:.*}", p, v),
                        _ => c_general(v, p, alt, conv == 'G'),
                    }
                }
                'a' | 'A' => {
                    let a = next(&mut ai)?;
                    let s = print(&a);
                    match prec {
                        Some(p) => s.chars().take(p).collect(),
                        None => s,
                    }
                }
                's' | 'S' => {
                    let a = next(&mut ai)?;
                    let s = self.text_of(&a);
                    match prec {
                        Some(p) => s.chars().take(p).collect(),
                        None => s,
                    }
                }
                'c' => {
                    let a = next(&mut ai)?;
                    match &a {
                        Expr::Str(s) => s.chars().next().map(|c| c.to_string()).unwrap_or_default(),
                        other => match other.as_i64().and_then(|n| char::from_u32(n as u32)) {
                            Some(c) => c.to_string(),
                            None => print(other),
                        },
                    }
                }
                'q' | 'Q' => {
                    let rest: Vec<String> = args[ai.min(args.len())..].iter().map(print).collect();
                    ai = args.len();
                    rest.join(", ")
                }
                'm' => print(&next(&mut ai)?),
                other => return err(format!("(in printf) invalid format specifier `%{}`", other)),
            };
            let body = if numeric && !body.starts_with('-') && (plus || space) {
                format!("{}{}", if plus { '+' } else { ' ' }, body)
            } else {
                body
            };
            let len = body.chars().count();
            if len < width {
                let pad = width - len;
                if left {
                    out.push_str(&body);
                    out.push_str(&" ".repeat(pad));
                } else if zero && numeric {
                    let (sign, digits) = if body.starts_with(['-', '+', ' ']) { body.split_at(1) } else { ("", body.as_str()) };
                    out.push_str(sign);
                    out.push_str(&"0".repeat(pad));
                    out.push_str(digits);
                } else {
                    out.push_str(&" ".repeat(pad));
                    out.push_str(&body);
                }
            } else {
                out.push_str(&body);
            }
        }
        Ok(out)
    }

    // ------------------------------------------------------------------ special evaluation

    pub fn special(&mut self, name: &str, raw: &[Expr]) -> R<Expr> {
        match name {
            "seq" | "add" | "mul" => match raw.len() {
                1 => {
                    let v = self.eval(&raw[0])?;
                    let items = match &v {
                        Expr::Range(a, b) => match (ints_of(a), ints_of(b)) {
                            (Some(l), Some(h)) => (l..=h).map(Expr::int).collect(),
                            _ => return err(format!("(in {}) unable to execute {}", name, name)),
                        },
                        other => operands_for_iteration(other),
                    };
                    self.finish_seq(name, items)
                }
                2 | 3 => self.seq_loop(name, &raw[0], &raw[1], raw.get(2)),
                _ => err(format!("invalid input: {} expects 1, 2 or 3 arguments", name)),
            },
            "assigned" => {
                let Some(a) = raw.first() else { return err("invalid input: assigned expects 1 argument") };
                let a = match a {
                    Expr::Uneval(x) => (**x).clone(),
                    other => other.clone(),
                };
                Ok(Expr::Bool(match &a {
                    Expr::Sym(n) => self.lookup_raw(n).is_some(),
                    Expr::Idx(b, idx) => {
                        let mut iv = Vec::new();
                        for i in idx {
                            iv.extend(self.eval(i)?.into_seq());
                        }
                        match b.as_ref() {
                            Expr::Sym(n) => match self.lookup_raw(n) {
                                Some(Expr::Table(t)) => {
                                    let key = Expr::from_seq(iv);
                                    t.iter().any(|(k, _)| *k == key)
                                }
                                Some(c) if is_container(&c) => true,
                                _ => false,
                            },
                            _ => false,
                        }
                    }
                    _ => false,
                }))
            }
            "evaln" => match raw.first() {
                Some(Expr::Uneval(x)) => Ok((**x).clone()),
                Some(Expr::Idx(b, idx)) => {
                    let mut iv = Vec::new();
                    for i in idx {
                        iv.extend(self.eval(i)?.into_seq());
                    }
                    Ok(Expr::Idx(b.clone(), iv))
                }
                Some(Expr::Concat(..)) => {
                    let s = self.eval(&raw[0])?;
                    Ok(s)
                }
                Some(other) => Ok(other.clone()),
                None => err("invalid input: evaln expects 1 argument"),
            },
            "if" | "ifelse" => {
                if raw.len() != 3 {
                    return err("(in if) invalid number of arguments");
                }
                let c = self.eval(&raw[0])?;
                if self.truth(&c)? {
                    self.eval(&raw[1])
                } else {
                    self.eval(&raw[2])
                }
            }
            "evalf" => {
                // evalf(e, n): e itself is evaluated with Digits = n, as in Maple
                if raw.len() == 2 {
                    let n = self.eval(&raw[1])?;
                    if let Some(k) = n.as_i64().filter(|k| *k > 0) {
                        let e = self.with_digits(k, |me| me.eval(&raw[0]))?;
                        return self.call_values("evalf", vec![e, n]);
                    }
                }
                let args = self.eval_list(raw, true)?;
                self.call_values("evalf", args)
            }
            "unassign" => {
                for a in raw {
                    let n = match a {
                        Expr::Uneval(x) => (**x).clone(),
                        other => self.eval(other)?,
                    };
                    match n {
                        Expr::Sym(s) => self.unassign(&s)?,
                        other => return err(format!("(in unassign) cannot unassign {}", print(&other))),
                    }
                }
                Ok(Expr::null())
            }
            _ => err(format!("unknown special function {}", name)),
        }
    }

    fn finish_seq(&mut self, kind: &str, items: Vec<Expr>) -> R<Expr> {
        let d = self.digits();
        Ok(match kind {
            "add" => simpl::add(items, d)?,
            "mul" => simpl::mul(items, d)?,
            _ => Expr::from_seq(items.into_iter().flat_map(|x| x.into_seq()).collect()),
        })
    }

    /// seq/add/mul/$ with an index specification `i = a..b`, `i = L` or `i in L`.
    pub fn seq_loop(&mut self, kind: &str, body: &Expr, spec: &Expr, step: Option<&Expr>) -> R<Expr> {
        let (var, container) = match spec {
            Expr::Rel(RelOp::Eq, v, r) => match v.as_ref() {
                Expr::Sym(n) => (n.clone(), r.as_ref()),
                _ => return err(format!("(in {}) invalid index specification", kind)),
            },
            Expr::In(v, r) => match v.as_ref() {
                Expr::Sym(n) => (n.clone(), r.as_ref()),
                _ => return err(format!("(in {}) invalid index specification", kind)),
            },
            _ => {
                // seq(f, n) / add(f, L) without a variable: iterate the operands
                let c = self.eval(spec)?;
                let items = operands_for_iteration(&c);
                let mut out = Vec::new();
                for _ in items {
                    out.push(self.eval(body)?);
                }
                return self.finish_seq(kind, out);
            }
        };
        let is_in = matches!(spec, Expr::In(..));
        let c = self.eval(container)?;
        let saved = self.lookup_binding(&var);
        let result = (|| -> R<Expr> {
            if let (false, Expr::Range(a, b)) = (is_in, &c) {
                let (Expr::Num(lo), Expr::Num(hi)) = (a.as_ref(), b.as_ref()) else {
                    if kind == "add" || kind == "mul" {
                        return self.symbolic_sum(kind, body, &var, &c);
                    }
                    return err(format!("(in {}) unable to execute {}", kind, kind));
                };
                let st = match step {
                    Some(s) => match self.eval(s)? {
                        Expr::Num(n) if !n.is_zero() => n,
                        _ => return err(format!("(in {}) step must be a nonzero number", kind)),
                    },
                    None => Num::one(),
                };
                let d = self.digits();
                let up = !st.is_negative();
                let mut i = lo.clone();
                let mut out = Vec::new();
                let mut count = 0u64;
                loop {
                    let o = i.cmp_value(hi);
                    if (up && o == std::cmp::Ordering::Greater) || (!up && o == std::cmp::Ordering::Less) {
                        break;
                    }
                    count += 1;
                    if count > 10_000_000 {
                        return err(format!("(in {}) too many iterations", kind));
                    }
                    self.assign_name(&var, Expr::Num(i.clone()))?;
                    out.push(self.eval(body)?);
                    i = i.add(&st, d);
                }
                return self.finish_seq(kind, out);
            }
            let items = operands_for_iteration(&c);
            let mut out = Vec::new();
            for it in items {
                self.assign_name(&var, it)?;
                out.push(self.eval(body)?);
            }
            self.finish_seq(kind, out)
        })();
        self.restore_binding(&var, saved);
        result
    }

    fn symbolic_sum(&mut self, kind: &str, body: &Expr, var: &str, range: &Expr) -> R<Expr> {
        self.restore_binding(var, None);
        let b = self.eval(body)?;
        let f = if kind == "add" { "sum" } else { "product" };
        self.engine_call(f, vec![b, Expr::eq(Expr::sym(var), range.clone())])
    }

    fn lookup_binding(&self, n: &str) -> Option<Expr> {
        if let Some(f) = self.frames.last() {
            if let Some(v) = f.vars.get(n) {
                return Some(v.clone());
            }
        }
        self.globals.get(n).cloned()
    }

    fn restore_binding(&mut self, n: &str, v: Option<Expr>) {
        if let Some(f) = self.frames.last_mut() {
            if f.vars.contains_key(n) {
                f.vars.insert(n.to_string(), v.unwrap_or_else(|| Expr::sym(n)));
                return;
            }
        }
        match v {
            Some(x) => {
                self.globals.insert(n.to_string(), x);
            }
            None => {
                self.globals.remove(n);
            }
        }
    }

    // ------------------------------------------------------------------ D operator

    pub fn d_operator(&mut self, idx: Option<Vec<Expr>>, f: Expr) -> R<Expr> {
        let unevaluated = |idx: &Option<Vec<Expr>>, f: &Expr| match idx {
            None => Expr::Fn("D".into(), vec![f.clone()]),
            Some(i) => Expr::Apply(Box::new(Expr::Idx(Box::new(Expr::sym("D")), i.clone())), vec![f.clone()]),
        };
        let fval = match &f {
            Expr::Sym(n) => match self.lookup_raw(n) {
                Some(v @ Expr::Proc(_)) => v,
                _ => f.clone(),
            },
            other => other.clone(),
        };
        if let Expr::Num(_) = fval {
            return Ok(Expr::int(0));
        }
        let params: Vec<String> = match &fval {
            Expr::Proc(p) => p.params.iter().map(|x| x.name.clone()).collect(),
            _ => match &idx {
                None => vec!["x".into()],
                Some(_) => return Ok(unevaluated(&idx, &f)),
            },
        };
        let wrt: Vec<Expr> = match &idx {
            None => {
                if params.len() != 1 {
                    return Ok(unevaluated(&idx, &f));
                }
                vec![Expr::sym(&params[0])]
            }
            Some(ix) => {
                let mut w = Vec::new();
                for i in ix {
                    match i.as_i64() {
                        Some(k) if k >= 1 && (k as usize) <= params.len() => w.push(Expr::sym(&params[k as usize - 1])),
                        _ => return err("(in D) invalid index"),
                    }
                }
                w
            }
        };
        if wrt.is_empty() {
            return Ok(f);
        }
        let syms: Vec<Expr> = params.iter().map(|p| Expr::sym(p)).collect();
        let body = match self.apply_value(fval.clone(), syms.clone(), "D") {
            Ok(b) => b,
            Err(Exc::Quit) => return Err(Exc::Quit),
            Err(_) => return Ok(unevaluated(&idx, &f)),
        };
        let mut dargs = vec![body];
        dargs.extend(wrt);
        let dv = self.engine_call("diff", dargs)?;
        if contains_unknown_derivative(&dv) {
            return Ok(unevaluated(&idx, &f));
        }
        if params.len() == 1 {
            // operator form, e.g. D(sin) = cos and (D@@2)(sin) = -sin
            let is_g = |e: &Expr| matches!(e, Expr::Fn(_, a) if a.len() == 1 && a[0] == syms[0]);
            match &dv {
                Expr::Fn(g, _) if is_g(&dv) => return Ok(Expr::sym(g)),
                Expr::Mul(v) if v.len() == 2 && matches!(v[0], Expr::Num(_)) && is_g(&v[1]) => {
                    let Expr::Fn(g, _) = &v[1] else { unreachable!() };
                    return Ok(Expr::Mul(vec![v[0].clone(), Expr::sym(g)]));
                }
                _ => {}
            }
        }
        Ok(arrow(params, dv))
    }

    // ------------------------------------------------------------------ indexed calls

    /// Elementwise application f~(a, b, ...): containers are traversed in step, other
    /// arguments are passed unchanged to every call.
    pub fn elementwise(&mut self, f: Expr, args: Vec<Expr>) -> R<Expr> {
        let elems = |e: &Expr| -> Option<Vec<Expr>> {
            match e {
                Expr::List(v) | Expr::Set(v) | Expr::Vector(v, _) => Some(v.clone()),
                Expr::Matrix(rows) => Some(rows.iter().flatten().cloned().collect()),
                _ => None,
            }
        };
        let Some(shape_at) = args.iter().position(|a| elems(a).is_some()) else {
            return self.apply_value(f, args, "~");
        };
        let n = elems(&args[shape_at]).unwrap().len();
        let mut cols: Vec<Option<Vec<Expr>>> = Vec::new();
        for a in &args {
            let c = elems(a);
            if let Some(v) = &c {
                if v.len() != n {
                    return err("(in ~) containers must have the same dimensions");
                }
            }
            cols.push(c);
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let call: Vec<Expr> = args
                .iter()
                .zip(&cols)
                .map(|(a, c)| c.as_ref().map(|v| v[i].clone()).unwrap_or_else(|| a.clone()))
                .collect();
            out.push(self.apply_value(f.clone(), call, "~")?);
        }
        Ok(match &args[shape_at] {
            Expr::List(_) => Expr::List(out),
            Expr::Set(_) => simpl::make_set(out),
            Expr::Vector(_, row) => Expr::Vector(out, *row),
            Expr::Matrix(rows) => {
                let w = rows.first().map(|r| r.len()).unwrap_or(0).max(1);
                Expr::Matrix(out.chunks(w).map(|c| c.to_vec()).collect())
            }
            _ => unreachable!(),
        })
    }

    pub fn apply_indexed(&mut self, base: &str, idx: Vec<Expr>, args: Vec<Expr>) -> R<Expr> {
        if self.lookup_raw(base).is_some() {
            let head = self.eval(&Expr::Idx(Box::new(Expr::sym(base)), idx.clone()))?;
            if head != Expr::Idx(Box::new(Expr::sym(base)), idx.clone()) {
                return self.apply_value(head, args, base);
            }
        }
        match base {
            "~" if idx.len() == 1 => self.elementwise(idx[0].clone(), args),
            "D" => {
                if args.len() != 1 {
                    return err("(in D) expecting 1 argument");
                }
                self.d_operator(Some(idx), args[0].clone())
            }
            "evalf" if args.len() == 1 && idx.len() == 1 => {
                let n = idx[0].as_i64().filter(|n| *n > 0).ok_or("(in evalf) invalid precision")?;
                if let Expr::Num(x) = &args[0] {
                    return Ok(Expr::Num(x.to_float(n as u32)));
                }
                let saved = self.globals.get("Digits").cloned();
                self.globals.insert("Digits".into(), Expr::int(n));
                let r = self.engine_call("evalf", args);
                if let Some(s) = saved {
                    self.globals.insert("Digits".into(), s);
                }
                r
            }
            "Vector" | "Matrix" => {
                let row = idx.first().map(|i| *i == Expr::sym("row")).unwrap_or(false);
                let r = self.construct(base, args.clone(), row)?;
                match r {
                    Some(v) => Ok(v),
                    None => self.engine_fnidx(base, idx, args),
                }
            }
            _ => {
                if is_package(base) {
                    if let Some(Expr::Sym(f)) = idx.first() {
                        let f = f.clone();
                        return self.call_package(base, &f, args);
                    }
                }
                self.engine_fnidx(base, idx, args)
            }
        }
    }

    fn engine_fnidx(&mut self, base: &str, idx: Vec<Expr>, args: Vec<Expr>) -> R<Expr> {
        let d = self.digits();
        let mut ji = Vec::new();
        for i in &idx {
            ji.push(crate::engine::to_json(i, d).map_err(Exc::Err)?);
        }
        let mut ja = Vec::new();
        for a in &args {
            ja.push(crate::engine::to_json(a, d).map_err(|m| Exc::Err(format!("(in {}) {}", base, m)))?);
        }
        // indexed call f[i](args) sent as an fnidx expression to evaluate; the engine's
        // top-level "idx" request field is not honoured for every name (log[2](8) -> log(8))
        let head = json!({"fnidx": [base, ji, ja]});
        let req = json!({"op": "call", "name": "eval", "args": [head], "opts": self.opts()?});
        let res = self.engine_request(req, base)?;
        Ok(crate::engine::from_json(&res, d)?)
    }

    // ------------------------------------------------------------------ constructors

    /// Matrix/Vector constructors handled in Rust; None means "let the engine try".
    fn construct(&mut self, kind: &str, args: Vec<Expr>, row: bool) -> R<Option<Expr>> {
        let mut pos = Vec::new();
        let mut symbol: Option<Expr> = None;
        let mut shape_identity = false;
        let mut fill: Option<Expr> = None;
        for a in args {
            match &a {
                Expr::Rel(RelOp::Eq, k, v) if matches!(k.as_ref(), Expr::Sym(_)) => {
                    let Expr::Sym(key) = k.as_ref() else { unreachable!() };
                    match key.as_str() {
                        "symbol" => symbol = Some((**v).clone()),
                        "shape" => shape_identity = **v == Expr::sym("identity"),
                        "fill" => fill = Some((**v).clone()),
                        "orientation" => {}
                        "datatype" | "storage" | "readonly" | "attributes" => {}
                        _ => return Ok(None),
                    }
                }
                _ => pos.push(a),
            }
        }
        let zero = fill.clone().unwrap_or_else(|| Expr::int(0));
        if kind == "Vector" {
            let (n, init) = match pos.as_slice() {
                [Expr::List(v)] => (v.len(), Some(Expr::List(v.clone()))),
                [Expr::Vector(v, _)] => (v.len(), Some(Expr::List(v.clone()))),
                [n] if n.as_i64().is_some() => (n.as_i64().unwrap().max(0) as usize, None),
                [n, init] if n.as_i64().is_some() => (n.as_i64().unwrap().max(0) as usize, Some(init.clone())),
                [] => (0, None),
                _ => return Ok(None),
            };
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let v = match (&init, &symbol) {
                    (Some(Expr::List(v)), _) => v.get(i).cloned().unwrap_or_else(|| zero.clone()),
                    (Some(f @ (Expr::Proc(_) | Expr::Sym(_))), _) => {
                        self.apply_value(f.clone(), vec![Expr::int(i as i64 + 1)], "Vector")?
                    }
                    (Some(x), _) => x.clone(),
                    (None, Some(s)) => Expr::Idx(Box::new(s.clone()), vec![Expr::int(i as i64 + 1)]),
                    (None, None) => zero.clone(),
                };
                out.push(v);
            }
            return Ok(Some(Expr::Vector(out, row)));
        }
        let (m, n, init) = match pos.as_slice() {
            [Expr::List(rows)] => {
                if rows.iter().all(|r| matches!(r, Expr::List(_))) {
                    let n = rows.iter().map(|r| as_list(r).unwrap().len()).max().unwrap_or(0);
                    (rows.len(), n, Some(Expr::List(rows.clone())))
                } else {
                    (1, rows.len(), Some(Expr::List(vec![Expr::List(rows.clone())])))
                }
            }
            [Expr::Matrix(r)] => return Ok(Some(Expr::Matrix(r.clone()))),
            [k] if k.as_i64().is_some() => {
                let k = k.as_i64().unwrap().max(0) as usize;
                (k, k, None)
            }
            [a, b] if a.as_i64().is_some() && b.as_i64().is_some() => {
                (a.as_i64().unwrap().max(0) as usize, b.as_i64().unwrap().max(0) as usize, None)
            }
            [a, init] if a.as_i64().is_some() => {
                let k = a.as_i64().unwrap().max(0) as usize;
                (k, k, Some(init.clone()))
            }
            [a, b, init] if a.as_i64().is_some() && b.as_i64().is_some() => {
                (a.as_i64().unwrap().max(0) as usize, b.as_i64().unwrap().max(0) as usize, Some(init.clone()))
            }
            _ => return Ok(None),
        };
        let mut rows = Vec::with_capacity(m);
        for i in 0..m {
            let mut r = Vec::with_capacity(n);
            for j in 0..n {
                let v = if shape_identity {
                    Expr::int((i == j) as i64)
                } else {
                    match (&init, &symbol) {
                        (Some(Expr::List(v)), _) => {
                            if v.iter().all(|x| matches!(x, Expr::List(_))) {
                                v.get(i)
                                    .and_then(as_list)
                                    .and_then(|row| row.get(j).cloned())
                                    .unwrap_or_else(|| zero.clone())
                            } else {
                                v.get(i * n + j).cloned().unwrap_or_else(|| zero.clone())
                            }
                        }
                        (Some(f @ (Expr::Proc(_) | Expr::Sym(_))), _) => self.apply_value(
                            f.clone(),
                            vec![Expr::int(i as i64 + 1), Expr::int(j as i64 + 1)],
                            "Matrix",
                        )?,
                        (Some(x), _) => x.clone(),
                        (None, Some(s)) => {
                            Expr::Idx(Box::new(s.clone()), vec![Expr::int(i as i64 + 1), Expr::int(j as i64 + 1)])
                        }
                        (None, None) => zero.clone(),
                    }
                };
                r.push(v);
            }
            rows.push(r);
        }
        Ok(Some(Expr::Matrix(rows)))
    }

    // ------------------------------------------------------------------ types

    /// Rust-side type test; None when the engine should decide.
    pub fn check_type(&mut self, e: &Expr, t: &Expr) -> Option<bool> {
        let num = e.as_num();
        Some(match t {
            Expr::Sym(s) => match s.as_str() {
                "anything" => true,
                "integer" => matches!(num, Some(Num::Int(_))),
                "posint" => matches!(num, Some(Num::Int(n)) if n.is_positive()),
                "negint" => matches!(num, Some(Num::Int(n)) if n.is_negative()),
                "nonnegint" => matches!(num, Some(Num::Int(n)) if !n.is_negative()),
                "nonposint" => matches!(num, Some(Num::Int(n)) if !n.is_positive()),
                "even" => matches!(num, Some(Num::Int(n)) if n.is_even()),
                "odd" => matches!(num, Some(Num::Int(n)) if n.is_odd()),
                "rational" => matches!(num, Some(Num::Int(_)) | Some(Num::Rat(..))),
                "fraction" => matches!(num, Some(Num::Rat(..))),
                "float" => matches!(num, Some(Num::Float(..))),
                "numeric" => num.is_some(),
                "extended_numeric" => num.is_some() || matches!(e, Expr::Const(c) if c == "infinity" || c == "undefined"),
                "positive" | "negative" | "nonnegative" | "nonpositive" | "nonneg" if num.is_some() => {
                    let n = num.unwrap();
                    match s.as_str() {
                        "positive" => !n.is_negative() && !n.is_zero(),
                        "negative" => n.is_negative(),
                        "nonpositive" => n.is_negative() || n.is_zero(),
                        _ => !n.is_negative(),
                    }
                }
                "symbol" => matches!(e, Expr::Sym(_)),
                "name" => matches!(e, Expr::Sym(_) | Expr::Idx(..)),
                "indexed" => matches!(e, Expr::Idx(..)),
                "string" => matches!(e, Expr::Str(_)),
                "list" => matches!(e, Expr::List(_)),
                "set" => matches!(e, Expr::Set(_)),
                "exprseq" => matches!(e, Expr::Seq(_)),
                "procedure" | "operator" => match e {
                    Expr::Proc(_) => true,
                    Expr::Sym(n) => self.proc_of(n).is_some(),
                    _ => false,
                },
                "function" => matches!(e, Expr::Fn(..) | Expr::Apply(..)),
                "equation" | "=" => matches!(e, Expr::Rel(RelOp::Eq, ..)),
                "<>" => matches!(e, Expr::Rel(RelOp::Neq, ..)),
                "<" => matches!(e, Expr::Rel(RelOp::Lt, ..)),
                "<=" => matches!(e, Expr::Rel(RelOp::Le, ..)),
                "relation" => matches!(e, Expr::Rel(..)),
                "boolean" => matches!(
                    e,
                    Expr::Bool(_) | Expr::Fail | Expr::Rel(..) | Expr::And(_) | Expr::Or(_) | Expr::Not(_)
                ),
                "range" | ".." => matches!(e, Expr::Range(..)),
                "Matrix" | "matrix" => matches!(e, Expr::Matrix(_)),
                "Vector" | "vector" => matches!(e, Expr::Vector(..)),
                "table" => matches!(e, Expr::Table(_)),
                "+" => matches!(e, Expr::Add(_)),
                "*" => matches!(e, Expr::Mul(_)),
                "^" => matches!(e, Expr::Pow(..)),
                "and" => matches!(e, Expr::And(_)),
                "or" => matches!(e, Expr::Or(_)),
                "not" => matches!(e, Expr::Not(_)),
                "truefalse" => matches!(e, Expr::Bool(_)),
                "constant" | "realcons" if !simpl::has_symbol(e) && s == "constant" => {
                    !matches!(e, Expr::List(_) | Expr::Set(_) | Expr::Str(_) | Expr::Proc(_))
                }
                "algebraic" => matches!(
                    e,
                    Expr::Num(_) | Expr::Sym(_) | Expr::Const(_) | Expr::Add(_) | Expr::Mul(_) | Expr::Pow(..)
                        | Expr::Fn(..) | Expr::Idx(..) | Expr::Apply(..)
                ),
                "polynom" if matches!(e, Expr::Num(_) | Expr::Sym(_)) => true,
                _ => return None,
            },
            Expr::Fn(n, a) if a.len() == 1 && (n == "list" || n == "set") => {
                let items = match (n.as_str(), e) {
                    ("list", Expr::List(v)) | ("set", Expr::Set(v)) => v.clone(),
                    _ => return Some(false),
                };
                for x in &items {
                    match self.check_type(x, &a[0]) {
                        Some(true) => {}
                        Some(false) => return Some(false),
                        None => return None,
                    }
                }
                true
            }
            Expr::Set(alts) => {
                let mut unknown = false;
                for t2 in alts {
                    match self.check_type(e, t2) {
                        Some(true) => return Some(true),
                        Some(false) => {}
                        None => unknown = true,
                    }
                }
                if unknown {
                    return None;
                }
                false
            }
            Expr::Fn(n, a) if n == "And" || n == "Or" => {
                let mut res = Vec::new();
                for t2 in a {
                    res.push(self.check_type(e, t2)?);
                }
                if n == "And" {
                    res.iter().all(|x| *x)
                } else {
                    res.iter().any(|x| *x)
                }
            }
            Expr::Fn(n, a) if n == "Not" && a.len() == 1 => !self.check_type(e, &a[0])?,
            Expr::Idx(b, i) if **b == Expr::sym("Vector") && i.len() == 1 => match e {
                Expr::Vector(_, row) => (*row) == (i[0] == Expr::sym("row")),
                _ => false,
            },
            _ => return None,
        })
    }

    // ------------------------------------------------------------------ builtin dispatch

    pub fn package_builtin(&mut self, pkg: &str, f: &str, args: Vec<Expr>) -> R<Option<Expr>> {
        let d = self.digits();
        if pkg != "LinearAlgebra" {
            return Ok(None);
        }
        Ok(Some(match (f, args.as_slice()) {
            ("Transpose", [Expr::Matrix(r)]) => {
                let n = r.first().map(|x| x.len()).unwrap_or(0);
                Expr::Matrix((0..n).map(|j| r.iter().map(|row| row[j].clone()).collect()).collect())
            }
            ("Transpose", [Expr::Vector(v, o)]) => Expr::Vector(v.clone(), !o),
            ("RowDimension", [Expr::Matrix(r)]) => Expr::int(r.len() as i64),
            ("ColumnDimension", [Expr::Matrix(r)]) => Expr::int(r.first().map(|x| x.len()).unwrap_or(0) as i64),
            ("Dimension", [Expr::Matrix(r)]) | ("Dimensions", [Expr::Matrix(r)]) => Expr::Seq(vec![
                Expr::int(r.len() as i64),
                Expr::int(r.first().map(|x| x.len()).unwrap_or(0) as i64),
            ]),
            ("Dimension", [Expr::Vector(v, _)]) => Expr::int(v.len() as i64),
            ("IdentityMatrix", [n]) if n.as_i64().is_some() => simpl::identity(n.as_i64().unwrap().max(0) as usize),
            ("ZeroMatrix", [m]) if m.as_i64().is_some() => {
                let k = m.as_i64().unwrap().max(0) as usize;
                Expr::Matrix(vec![vec![Expr::int(0); k]; k])
            }
            ("ZeroMatrix", [m, n]) if m.as_i64().is_some() && n.as_i64().is_some() => Expr::Matrix(vec![
                vec![Expr::int(0); n.as_i64().unwrap().max(0) as usize];
                m.as_i64().unwrap().max(0) as usize
            ]),
            ("ZeroVector", [n]) if n.as_i64().is_some() => {
                Expr::Vector(vec![Expr::int(0); n.as_i64().unwrap().max(0) as usize], false)
            }
            ("Row", [Expr::Matrix(_), i]) if i.as_i64().is_some() => {
                crate::interp::select(&args[0], std::slice::from_ref(i), None)?
            }
            ("Column", [Expr::Matrix(r), j]) if j.as_i64().is_some() => {
                let n = r.first().map(|x| x.len()).unwrap_or(0);
                let jj = j.as_i64().unwrap();
                if jj < 1 || jj as usize > n {
                    return err("(in LinearAlgebra:-Column) invalid column index");
                }
                Expr::Vector(r.iter().map(|row| row[jj as usize - 1].clone()).collect(), false)
            }
            ("Multiply", [a, b]) | ("MatrixMatrixMultiply", [a, b]) | ("MatrixVectorMultiply", [a, b]) => {
                simpl::dot(a.clone(), b.clone(), d)?
            }
            ("Add", [a, b]) | ("MatrixAdd", [a, b]) | ("VectorAdd", [a, b]) => {
                simpl::add(vec![a.clone(), b.clone()], d)?
            }
            ("ScalarMultiply", [a, k]) | ("MatrixScalarMultiply", [a, k]) | ("VectorScalarMultiply", [a, k]) => {
                simpl::mul(vec![k.clone(), a.clone()], d)?
            }
            ("DiagonalMatrix", [Expr::List(v)]) => Expr::Matrix(
                (0..v.len())
                    .map(|i| (0..v.len()).map(|j| if i == j { v[i].clone() } else { Expr::int(0) }).collect())
                    .collect(),
            ),
            ("Equal", [a, b]) => Expr::Bool(a == b),
            _ => return Ok(None),
        }))
    }

    /// Language builtins; Ok(None) means "not a builtin".
    pub fn builtin(&mut self, name: &str, args: Vec<Expr>) -> R<Option<Expr>> {
        let d = self.digits();
        let n = args.len();
        let r = match name {
            "eval" => match n {
                1 => match &args[0] {
                    Expr::Sym(s) => match self.lookup_raw(s) {
                        Some(v @ (Expr::Proc(_) | Expr::Table(_))) => v,
                        _ => self.ev(&args[0], true)?,
                    },
                    other => self.ev(other, true)?,
                },
                2 => {
                    if args[1].as_i64().is_some() {
                        return Ok(Some(self.ev(&args[0], true)?));
                    }
                    let pairs = eqs_of(&args[1])?;
                    let r = eval_replace(&args[0], &pairs);
                    let r = simpl::resimplify(&r, d)?;
                    self.ev(&r, true)?
                }
                _ => return err("invalid input: eval expects 1 or 2 arguments"),
            },
            "subs" => {
                if n < 2 {
                    return err("invalid input: subs expects at least 2 arguments");
                }
                let mut cur = args[n - 1].clone();
                for spec in &args[..n - 1] {
                    let pairs = eqs_of(spec)?;
                    cur = replace(&cur, &pairs);
                }
                simpl::resimplify(&cur, d)?
            }
            "unapply" => {
                if n < 2 {
                    return err("invalid input: unapply expects at least 2 arguments");
                }
                let mut params = Vec::new();
                for a in &args[1..] {
                    let items = match a {
                        Expr::List(v) => v.clone(),
                        other => vec![other.clone()],
                    };
                    for x in items {
                        match x {
                            Expr::Sym(s) => params.push(s),
                            other => {
                                return err(format!("(in unapply) variables must be names, received {}", print(&other)))
                            }
                        }
                    }
                }
                arrow(params, args[0].clone())
            }
            "D" => {
                if n != 1 {
                    return err("(in D) expecting 1 argument");
                }
                self.d_operator(None, args[0].clone())?
            }
            "nops" => {
                if n != 1 {
                    return err("invalid input: nops expects 1 argument");
                }
                match &args[0] {
                    Expr::Matrix(_) | Expr::Vector(..) => Expr::int(operands(&args[0])?.len() as i64),
                    Expr::Num(Num::Int(_)) | Expr::Sym(_) | Expr::Str(_) | Expr::Const(_) | Expr::Bool(_) => {
                        Expr::int(1)
                    }
                    other => Expr::int(operands(other)?.len() as i64),
                }
            }
            "op" => self.op(&args)?,
            "subsop" => {
                if n < 2 {
                    return err("invalid input: subsop expects at least 2 arguments");
                }
                let mut target = args[n - 1].clone();
                for spec in &args[..n - 1] {
                    let Expr::Rel(RelOp::Eq, i, v) = spec else { return err("(in subsop) invalid specification") };
                    let mut ops = operands(&target)?;
                    let Some(k) = i.as_i64() else { return err("(in subsop) invalid operand index") };
                    let len = ops.len() as i64;
                    let k = if k < 0 { len + k + 1 } else { k };
                    if k == 0 {
                        target = match &target {
                            Expr::Fn(_, a) => match v.as_ref() {
                                Expr::Sym(s) => Expr::Fn(s.clone(), a.clone()),
                                h => Expr::Apply(Box::new(h.clone()), a.clone()),
                            },
                            _ => return err("(in subsop) invalid operand index"),
                        };
                        continue;
                    }
                    if k < 1 || k > len {
                        return err("(in subsop) improper op or subscript selector");
                    }
                    ops[k as usize - 1] = (**v).clone();
                    let ops: Vec<Expr> = ops.into_iter().filter(|x| !x.is_null()).collect();
                    target = rebuild(&target, ops, d)?;
                }
                target
            }
            "lhs" | "rhs" => {
                if n != 1 {
                    return err(format!("invalid input: {} expects 1 argument", name));
                }
                match &args[0] {
                    Expr::Rel(_, a, b) | Expr::Range(a, b) => {
                        if name == "lhs" {
                            (**a).clone()
                        } else {
                            (**b).clone()
                        }
                    }
                    other => {
                        return err(format!(
                            "invalid input: {} received {}, which is not valid for its 1st argument",
                            name,
                            print(other)
                        ))
                    }
                }
            }
            "map" | "map2" => self.map(name, args)?,
            "select" | "remove" | "selectremove" => self.select_fn(name, args)?,
            "zip" => {
                if n < 3 {
                    return err("invalid input: zip expects 3 or 4 arguments");
                }
                let (f, a, b) = (args[0].clone(), &args[1], &args[2]);
                let (va, vb, mk_vec) = match (a, b) {
                    (Expr::List(x), Expr::List(y)) => (x.clone(), y.clone(), None),
                    (Expr::Vector(x, o), Expr::Vector(y, _)) => (x.clone(), y.clone(), Some(*o)),
                    _ => return err("(in zip) arguments must be lists or Vectors"),
                };
                let len = if n == 4 { va.len().max(vb.len()) } else { va.len().min(vb.len()) };
                let mut out = Vec::new();
                for i in 0..len {
                    let x = va.get(i).cloned().unwrap_or_else(|| args[3].clone());
                    let y = vb.get(i).cloned().unwrap_or_else(|| args[3].clone());
                    out.push(self.apply_value(f.clone(), vec![x, y], "zip")?);
                }
                match mk_vec {
                    Some(o) => Expr::Vector(out, o),
                    None => Expr::List(out),
                }
            }
            "member" => {
                if n < 2 {
                    return err("invalid input: member expects 2 or 3 arguments");
                }
                let items = operands_for_iteration(&args[1]);
                Expr::Bool(items.contains(&args[0]))
            }
            "numelems" => match &args[..] {
                [Expr::List(v)] | [Expr::Set(v)] | [Expr::Vector(v, _)] => Expr::int(v.len() as i64),
                [Expr::Matrix(r)] => Expr::int((r.len() * r.first().map(|x| x.len()).unwrap_or(0)) as i64),
                [Expr::Table(t)] => Expr::int(t.len() as i64),
                _ => return err("invalid input: numelems expects a list, set, table or rtable"),
            },
            "union" | "intersect" | "minus" | "subset" => {
                let sets: Option<Vec<Vec<Expr>>> = args
                    .iter()
                    .map(|a| match a {
                        Expr::Set(v) => Some(v.clone()),
                        _ => None,
                    })
                    .collect();
                let Some(sets) = sets else { return Ok(Some(Expr::Fn(name.into(), args))) };
                if sets.is_empty() {
                    return Ok(Some(Expr::Set(vec![])));
                }
                match name {
                    "union" => simpl::make_set(sets.into_iter().flatten().collect()),
                    "intersect" => {
                        let mut acc = sets[0].clone();
                        for s in &sets[1..] {
                            acc.retain(|x| s.contains(x));
                        }
                        Expr::Set(acc)
                    }
                    "minus" if sets.len() == 2 => {
                        Expr::Set(sets[0].iter().filter(|x| !sets[1].contains(x)).cloned().collect())
                    }
                    "subset" if sets.len() == 2 => Expr::Bool(sets[0].iter().all(|x| sets[1].contains(x))),
                    _ => return err(format!("invalid input: {} expects 2 arguments", name)),
                }
            }
            "print" => {
                let shown: Vec<Expr> = args.iter().map(|a| self.named_value(a)).collect();
                self.output.push(Out::Print(Expr::from_seq(shown)));
                Expr::null()
            }
            "lprint" => {
                let s = args.iter().map(|a| print(&self.named_value(a))).collect::<Vec<_>>().join(", ");
                self.output.push(Out::Text(format!("{}\n", s)));
                Expr::null()
            }
            "printf" | "sprintf" | "nprintf" => {
                let Some(Expr::Str(fmt)) = args.first() else {
                    return err(format!("(in {}) format must be a string", name));
                };
                let s = self.format(fmt, &args[1..])?;
                match name {
                    "printf" => {
                        self.output.push(Out::Text(s));
                        Expr::null()
                    }
                    "sprintf" => Expr::Str(s),
                    _ => Expr::Sym(s),
                }
            }
            "latex" => {
                if n < 1 {
                    return err("invalid input: latex expects 1 argument");
                }
                let s = self.latex(&args[0])?;
                self.output.push(Out::Text(format!("{}\n", s)));
                Expr::null()
            }
            "time" => {
                if n == 0 {
                    let t = self.elapsed();
                    Expr::Num(Num::Float(BigInt::from((t * 1000.0).round() as i64), -3))
                } else {
                    return Ok(None);
                }
            }
            "assume" | "additionally" => {
                let mut i = 0;
                let mut pending = Vec::new();
                while i < n {
                    let a = &args[i];
                    match (a, args.get(i + 1)) {
                        (Expr::Sym(_), Some(t)) if !matches!(t, Expr::Rel(..)) => {
                            pending.push(Expr::TypeAssert(Box::new(a.clone()), Box::new(t.clone())));
                            i += 2;
                        }
                        _ => {
                            pending.push(a.clone());
                            i += 1;
                        }
                    }
                }
                for p in pending {
                    self.push_assumption(p, name == "additionally")?;
                }
                Expr::null()
            }
            "about" => {
                for a in &args {
                    let names = crate::interp::free_names(a);
                    let rel: Vec<String> = self
                        .assumptions
                        .iter()
                        .filter(|x| crate::interp::free_names(x).iter().any(|y| names.contains(y)))
                        .map(print)
                        .collect();
                    let text = if rel.is_empty() {
                        format!("{}:\n  nothing known about this object\n", print(a))
                    } else {
                        format!("{}:\n  is assumed to be: {}\n", print(a), rel.join(", "))
                    };
                    self.output.push(Out::Text(text));
                }
                Expr::null()
            }
            "kernelopts" => {
                // only the informational keys; settings are accepted and ignored
                let mut last = Expr::null();
                for a in &args {
                    last = match a {
                        Expr::Sym(k) if k == "version" => {
                            Expr::Str(format!("AlgebraKing {}", env!("CARGO_PKG_VERSION")))
                        }
                        Expr::Sym(k) if k == "cputime" => {
                            Expr::Num(Num::Float(BigInt::from((self.elapsed() * 1000.0).round() as i64), -3))
                        }
                        _ => Expr::null(),
                    };
                }
                last
            }
            "interface" => {
                let mut last = Expr::null();
                for a in &args {
                    match a {
                        Expr::Rel(RelOp::Eq, k, v) if **k == Expr::sym("prettyprint") => {
                            last = Expr::int(self.prettyprint);
                            self.prettyprint = match v.as_ref() {
                                Expr::Bool(b) => *b as i64,
                                other => other.as_i64().unwrap_or(1),
                            };
                        }
                        Expr::Sym(k) if k == "prettyprint" => last = Expr::int(self.prettyprint),
                        Expr::Rel(RelOp::Eq, k, v) if **k == Expr::sym("warnlevel") => {
                            last = Expr::int(self.warnlevel);
                            self.warnlevel = v.as_i64().unwrap_or(3);
                        }
                        Expr::Sym(k) if k == "warnlevel" => last = Expr::int(self.warnlevel),
                        _ => {}
                    }
                }
                last
            }
            "with" => {
                let Some(Expr::Sym(p)) = args.first() else {
                    return err("(in with) invalid input: expecting a package name");
                };
                if !is_package(p) {
                    return err(format!("(in with) {} is not a package", p));
                }
                let p = p.clone();
                self.packages.retain(|x| *x != p);
                self.packages.push(p.clone());
                let ex = exports(&p);
                let names: Vec<Expr> = if n > 1 {
                    args[1..].to_vec()
                } else {
                    let mut v: Vec<&str> = ex.to_vec();
                    v.sort();
                    v.into_iter().map(Expr::sym).collect()
                };
                Expr::List(names)
            }
            "evalb" => {
                if n != 1 {
                    return err("invalid input: evalb expects 1 argument");
                }
                self.evalb(&args[0])?
            }
            "type" => {
                if n != 2 {
                    return err("invalid input: type expects 2 arguments");
                }
                match self.check_type(&args[0], &args[1]) {
                    Some(b) => Expr::Bool(b),
                    None => return Ok(None),
                }
            }
            "whattype" => {
                if n == 1 {
                    whattype(&args[0])
                } else {
                    Expr::sym("exprseq")
                }
            }
            "cat" => {
                let text: String = args.iter().map(|a| self.text_of(a)).collect();
                match args.first() {
                    Some(Expr::Str(_)) => Expr::Str(text),
                    _ => Expr::Sym(text),
                }
            }
            "length" => match args.as_slice() {
                [Expr::Str(s)] => Expr::int(s.chars().count() as i64),
                [Expr::Num(Num::Int(i))] => Expr::int(if i.is_zero() { 0 } else { i.abs().to_string().len() as i64 }),
                [Expr::Sym(s)] => Expr::int(s.chars().count() as i64),
                _ => return Ok(None),
            },
            "parse" => match args.as_slice() {
                [Expr::Str(s)] => {
                    let e = crate::parser::parse_expr(s).map_err(|e| Exc::Err(format!("(in parse) {}", e.msg)))?;
                    self.eval(&e)?
                }
                _ => return err("(in parse) expecting a string"),
            },
            "convert" => {
                if n < 2 {
                    return Ok(None);
                }
                match (&args[0], &args[1]) {
                    (e, Expr::Sym(t)) if t == "string" => Expr::Str(self.text_of(e)),
                    (e, Expr::Sym(t)) if t == "name" || t == "symbol" => Expr::Sym(self.text_of(e)),
                    (Expr::Set(v), Expr::Sym(t)) | (Expr::Seq(v), Expr::Sym(t)) | (Expr::Vector(v, _), Expr::Sym(t))
                        if t == "list" =>
                    {
                        Expr::List(v.clone())
                    }
                    (Expr::Matrix(r), Expr::Sym(t)) if t == "listlist" => {
                        Expr::List(r.iter().map(|x| Expr::List(x.clone())).collect())
                    }
                    (Expr::List(v), Expr::Sym(t)) if t == "set" => simpl::make_set(v.clone()),
                    (Expr::List(v), Expr::Sym(t)) if t == "+" => simpl::add(v.clone(), d)?,
                    (Expr::List(v), Expr::Sym(t)) if t == "*" => simpl::mul(v.clone(), d)?,
                    (Expr::List(v), Expr::Sym(t)) if t == "Vector" => Expr::Vector(v.clone(), false),
                    (Expr::List(v), Expr::Sym(t)) if t == "Matrix" && v.iter().all(|x| matches!(x, Expr::List(_))) => {
                        self.construct("Matrix", vec![args[0].clone()], false)?.unwrap_or_else(|| args[0].clone())
                    }
                    _ => return Ok(None),
                }
            }
            "table" => match args.as_slice() {
                [] => Expr::Table(vec![]),
                [Expr::List(v)] | [Expr::Set(v)] => {
                    let mut t = Vec::new();
                    for (i, x) in v.iter().enumerate() {
                        match x {
                            Expr::Rel(RelOp::Eq, k, val) => t.push(((**k).clone(), (**val).clone())),
                            other => t.push((Expr::int(i as i64 + 1), other.clone())),
                        }
                    }
                    Expr::Table(t)
                }
                _ => return Ok(None),
            },
            "copy" if n == 1 => args[0].clone(),
            "Matrix" | "Vector" => match self.construct(name, args.clone(), false)? {
                Some(v) => v,
                None => return Ok(None),
            },
            "evalf" | "evalhf" => match args.as_slice() {
                [Expr::Num(x)] => Expr::Num(x.to_float(if name == "evalhf" { 15 } else { d })),
                [Expr::Num(x), k] if name == "evalf" && k.as_i64().map(|v| v > 0).unwrap_or(false) => {
                    Expr::Num(x.to_float(k.as_i64().unwrap() as u32))
                }
                [e, k] if name == "evalf" && k.as_i64().map(|v| v > 0).unwrap_or(false) => {
                    return Ok(Some(self.apply_indexed("evalf", vec![k.clone()], vec![e.clone()])?));
                }
                [e] if name == "evalhf" => {
                    return Ok(Some(self.apply_indexed("evalf", vec![Expr::int(15)], vec![e.clone()])?));
                }
                _ => return Ok(None),
            },
            "factorial" => match args.as_slice() {
                [Expr::Num(Num::Int(k))] => {
                    if k.is_negative() {
                        return err("numeric exception: division by zero");
                    }
                    let Some(m) = k.to_u64().filter(|m| *m <= 200_000) else {
                        return err("(in factorial) argument too large");
                    };
                    let mut acc = BigInt::one();
                    for i in 2..=m {
                        acc *= i;
                    }
                    Expr::Num(Num::Int(acc))
                }
                _ => return Ok(None),
            },
            "mod" | "modp" => match args.as_slice() {
                [Expr::Num(Num::Int(a)), Expr::Num(Num::Int(m))] if !m.is_zero() => {
                    let m = m.abs();
                    Expr::Num(Num::Int(a.mod_floor(&m)))
                }
                _ => return Ok(None),
            },
            "max" | "min" => {
                let items: Vec<Expr> = args.iter().flat_map(|a| a.clone().into_seq()).collect();
                if items.is_empty() || !items.iter().all(|x| matches!(x, Expr::Num(_))) {
                    return Ok(None);
                }
                let mut best = items[0].clone();
                for x in &items[1..] {
                    let (Expr::Num(a), Expr::Num(b)) = (x, &best) else { unreachable!() };
                    let o = a.cmp_value(b);
                    if (name == "max" && o == std::cmp::Ordering::Greater) || (name == "min" && o == std::cmp::Ordering::Less)
                    {
                        best = x.clone();
                    }
                }
                best
            }
            "sort" => match args.as_slice() {
                [Expr::List(v)] if v.iter().all(|x| matches!(x, Expr::Num(_))) => {
                    let mut v = v.clone();
                    v.sort_by(simpl::set_order);
                    Expr::List(v)
                }
                [Expr::List(v)] if v.iter().all(|x| matches!(x, Expr::Str(_))) => {
                    let mut v = v.clone();
                    v.sort_by_key(print);
                    Expr::List(v)
                }
                [Expr::List(v), Expr::Sym(o)] if (o == ">" || o == "<") && v.iter().all(|x| matches!(x, Expr::Num(_))) => {
                    let mut v = v.clone();
                    v.sort_by(simpl::set_order);
                    if o == ">" {
                        v.reverse();
                    }
                    Expr::List(v)
                }
                _ => return Ok(None),
            },
            "verify" if n == 2 => {
                let res = self.verify(&args[0], &args[1], &[])?;
                match res.get("verdict").and_then(|v| v.as_str()) {
                    Some("equal") => Expr::Bool(true),
                    Some("not_equal") => Expr::Bool(false),
                    _ => Expr::Fail,
                }
            }
            "series" | "taylor" if n == 2 => {
                let mut a = args.clone();
                a.push(self.globals.get("Order").cloned().unwrap_or_else(|| Expr::int(6)));
                return Ok(Some(self.engine_call(name, a)?));
            }
            "restart" => {
                self.restart();
                Expr::null()
            }
            "quit" | "done" | "stop" => return Err(Exc::Quit),
            "error" | "ERROR" => {
                let text: Vec<String> = args.iter().map(|a| self.text_of(a)).collect();
                return err(text.join(", "));
            }
            "RETURN" => Expr::from_seq(args),
            _ => return Ok(None),
        };
        Ok(Some(r))
    }

    fn op(&mut self, args: &[Expr]) -> R<Expr> {
        match args.len() {
            1 => Ok(Expr::from_seq(operands(&args[0])?)),
            2 => {
                let (sel, e) = (&args[0], &args[1]);
                match sel {
                    Expr::List(path) => {
                        let mut cur = e.clone();
                        for p in path {
                            cur = self.op(&[p.clone(), cur])?;
                        }
                        Ok(cur)
                    }
                    Expr::Range(a, b) => {
                        let ops = operands(e)?;
                        let len = ops.len() as i64;
                        let fix = |k: i64| if k < 0 { len + k + 1 } else { k };
                        let (Some(lo), Some(hi)) = (a.as_i64(), b.as_i64()) else {
                            return err("(in op) improper op or subscript selector");
                        };
                        let (lo, hi) = (fix(lo), fix(hi));
                        if hi < lo {
                            return Ok(Expr::null());
                        }
                        if lo < 1 || hi > len {
                            return err("(in op) improper op or subscript selector");
                        }
                        Ok(Expr::from_seq(ops[lo as usize - 1..hi as usize].to_vec()))
                    }
                    _ => {
                        let Some(k) = sel.as_i64() else { return err("(in op) improper op or subscript selector") };
                        if k == 0 {
                            return Ok(op_zero(e));
                        }
                        let ops = operands(e)?;
                        let len = ops.len() as i64;
                        let kk = if k < 0 { len + k + 1 } else { k };
                        if kk < 1 || kk > len {
                            return err(format!(
                                "(in op) improper op or subscript selector: op({}, {}) where nops = {}",
                                k,
                                print(e),
                                len
                            ));
                        }
                        Ok(ops[kk as usize - 1].clone())
                    }
                }
            }
            _ => err("invalid input: op expects 1 or 2 arguments"),
        }
    }

    fn map(&mut self, name: &str, args: Vec<Expr>) -> R<Expr> {
        let d = self.digits();
        let (f, target, call_args): (Expr, Expr, ArgBuilder) = if name == "map" {
            if args.len() < 2 {
                return err("invalid input: map expects at least 2 arguments");
            }
            let extra = args[2..].to_vec();
            (args[0].clone(), args[1].clone(), Box::new(move |x| {
                let mut v = vec![x];
                v.extend(extra.clone());
                v
            }))
        } else {
            if args.len() < 3 {
                return err("invalid input: map2 expects at least 3 arguments");
            }
            let first = args[1].clone();
            let extra = args[3..].to_vec();
            (args[0].clone(), args[2].clone(), Box::new(move |x| {
                let mut v = vec![first.clone(), x];
                v.extend(extra.clone());
                v
            }))
        };
        let ap = |me: &mut Interp, x: Expr| me.apply_value(f.clone(), call_args(x), "map");
        Ok(match &target {
            Expr::List(v) => {
                let mut out = Vec::new();
                for x in v {
                    out.extend(ap(self, x.clone())?.into_seq());
                }
                Expr::List(out)
            }
            Expr::Set(v) => {
                let mut out = Vec::new();
                for x in v {
                    out.push(ap(self, x.clone())?);
                }
                simpl::make_set(out)
            }
            Expr::Seq(v) => {
                let mut out = Vec::new();
                for x in v {
                    out.push(ap(self, x.clone())?);
                }
                Expr::from_seq(out)
            }
            Expr::Vector(v, o) => {
                let mut out = Vec::new();
                for x in v {
                    out.push(ap(self, x.clone())?);
                }
                Expr::Vector(out, *o)
            }
            Expr::Matrix(rows) => {
                let mut out = Vec::new();
                for r in rows {
                    let mut row = Vec::new();
                    for x in r {
                        row.push(ap(self, x.clone())?);
                    }
                    out.push(row);
                }
                Expr::Matrix(out)
            }
            Expr::Table(t) => {
                let mut out = Vec::new();
                for (k, v) in t {
                    out.push((k.clone(), ap(self, v.clone())?));
                }
                Expr::Table(out)
            }
            Expr::Fn(fname, a) => {
                let mut out = Vec::new();
                for x in a {
                    out.push(ap(self, x.clone())?);
                }
                self.call_values(fname, out)?
            }
            Expr::Add(_) | Expr::Mul(_) | Expr::Pow(..) | Expr::Rel(..) | Expr::Range(..) | Expr::And(_)
            | Expr::Or(_) | Expr::Not(_) => {
                let mut out = Vec::new();
                for x in operands(&target)? {
                    out.push(ap(self, x)?);
                }
                let r = rebuild(&target, out, d)?;
                self.ev(&r, false)?
            }
            other => ap(self, other.clone())?,
        })
    }

    fn select_fn(&mut self, name: &str, args: Vec<Expr>) -> R<Expr> {
        let d = self.digits();
        if args.len() < 2 {
            return err(format!("invalid input: {} expects at least 2 arguments", name));
        }
        let f = args[0].clone();
        let target = args[1].clone();
        let extra = args[2..].to_vec();
        let items = match &target {
            Expr::List(v) | Expr::Set(v) | Expr::Add(v) | Expr::Mul(v) | Expr::Seq(v) => v.clone(),
            Expr::Fn(_, a) => a.clone(),
            other => return err(format!("(in {}) cannot select from {}", name, print(other))),
        };
        let (mut yes, mut no) = (Vec::new(), Vec::new());
        for x in items {
            let mut a = vec![x.clone()];
            a.extend(extra.clone());
            let r = self.apply_value(f.clone(), a, name)?;
            let ok = match self.evalb(&r)? {
                Expr::Bool(b) => b,
                Expr::Fail => false,
                other => {
                    return err(format!(
                        "({}) selecting function must return true or false, received {}",
                        name,
                        print(&other)
                    ))
                }
            };
            if ok {
                yes.push(x);
            } else {
                no.push(x);
            }
        }
        let wrap = |v: Vec<Expr>| -> R<Expr> {
            Ok(match &target {
                Expr::List(_) => Expr::List(v),
                Expr::Set(_) => Expr::Set(v),
                Expr::Add(_) => simpl::add(v, d)?,
                Expr::Mul(_) => simpl::mul(v, d)?,
                Expr::Fn(n, _) => Expr::Fn(n.clone(), v),
                _ => Expr::from_seq(v),
            })
        };
        Ok(match name {
            "select" => wrap(yes)?,
            "remove" => wrap(no)?,
            _ => Expr::Seq(vec![wrap(yes)?, wrap(no)?]),
        })
    }

    /// Call the engine's verify (lhs = rhs) with extra assumptions.
    pub fn verify(&mut self, lhs: &Expr, rhs: &Expr, extra: &[Expr]) -> R<Value> {
        let d = self.digits();
        let l = crate::engine::to_json(lhs, d).map_err(Exc::Err)?;
        let r = crate::engine::to_json(rhs, d).map_err(Exc::Err)?;
        self.check_request(json!({"op": "verify", "lhs": l, "rhs": r}), extra, None, "verify")
    }

    /// Send a verification request (verify, check_step, check_solution) with the session assumptions
    /// plus `extra`; `field` ("real" or "complex") selects the domain-aware verification.
    pub fn check_request(&mut self, mut req: Value, extra: &[Expr], field: Option<&str>, ctx: &str) -> R<Value> {
        let saved = self.assumptions.clone();
        for a in extra {
            if let Err(e) = self.push_assumption(a.clone(), true) {
                self.assumptions = saved;
                return Err(e);
            }
        }
        let opts = self.opts();
        self.assumptions = saved;
        let mut opts = opts?;
        if let Some(f) = field {
            opts["field"] = json!(f);
        }
        req["opts"] = opts;
        self.engine_request(req, ctx)
    }
}
