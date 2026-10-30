//   AlgebraKing //
//
//   File:         [interp.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Maple evaluator: names, full evaluation, procedures, control flow and calls.']

use crate::ast::{Expr, Proc, RelOp, Stmt, TopStmt};
use crate::engine::{from_json, to_json, Engine};
use crate::number::Num;
use crate::parser::parse_program_partial;
use crate::printer::print;
use crate::simpl;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Instant;

const MAX_DEPTH: usize = 5000;

/// Exceptional control: a Maple error (message without "Error, ") or a quit request.
#[derive(Debug, Clone, PartialEq)]
pub enum Exc {
    Err(String),
    Quit,
}

impl From<String> for Exc {
    fn from(s: String) -> Exc {
        Exc::Err(s)
    }
}

impl From<&str> for Exc {
    fn from(s: &str) -> Exc {
        Exc::Err(s.to_string())
    }
}

pub type R<T> = Result<T, Exc>;

pub fn err<T>(msg: impl Into<String>) -> R<T> {
    Err(Exc::Err(msg.into()))
}

pub enum Flow {
    Normal(Expr),
    Break,
    Next,
    Return(Expr),
}

/// One piece of output produced while running statements.
#[derive(Debug, Clone, PartialEq)]
pub enum Out {
    Expr(Expr),
    Assign(String, Expr),
    Print(Expr),
    Text(String),
    Warning(String),
    Error(String),
}

pub struct Frame {
    pub vars: HashMap<String, Expr>,
    pub params: HashSet<String>,
    pub missing: HashMap<String, usize>,
    pub procname: String,
    pub args: Vec<Expr>,
    pub nparams: usize,
}

pub struct Interp {
    pub globals: HashMap<String, Expr>,
    pub frames: Vec<Frame>,
    pub engine: Rc<RefCell<Engine>>,
    pub history: Vec<Expr>,
    pub assumptions: Vec<Expr>,
    pub packages: Vec<String>,
    pub prettyprint: i64,
    pub warnlevel: i64,
    pub output: Vec<Out>,
    pub remember: HashMap<(usize, String), Expr>,
    pub defining: Option<String>,
    pub last_error: Option<String>,
    /// verification mode: sum/product/int reach the engine inert (Sum/Product/Int) so their conditions survive
    pub inert: bool,
    depth: usize,
    start: Instant,
}

pub fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{}{}", n, suffix)
}

impl Interp {
    pub fn new(engine: Rc<RefCell<Engine>>) -> Interp {
        let mut it = Interp {
            globals: HashMap::new(),
            frames: Vec::new(),
            engine,
            history: Vec::new(),
            assumptions: Vec::new(),
            packages: Vec::new(),
            prettyprint: 0,
            warnlevel: 3,
            output: Vec::new(),
            remember: HashMap::new(),
            defining: None,
            last_error: None,
            inert: false,
            depth: 0,
            start: Instant::now(),
        };
        it.reset_globals();
        it
    }

    fn reset_globals(&mut self) {
        self.globals.clear();
        self.globals.insert("Digits".into(), Expr::int(10));
        self.globals.insert("Order".into(), Expr::int(6));
    }

    pub fn restart(&mut self) {
        self.reset_globals();
        self.frames.clear();
        self.history.clear();
        self.assumptions.clear();
        self.packages.clear();
        self.remember.clear();
        self.last_error = None;
    }

    pub fn digits(&self) -> u32 {
        match self.globals.get("Digits").and_then(|d| d.as_i64()) {
            Some(d) if d > 0 => d.min(100_000) as u32,
            _ => 10,
        }
    }

    pub fn elapsed(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    // ------------------------------------------------------------------ running source

    /// Parse and run `src`; returns the output. Stops at the first error when `stop_on_error`.
    pub fn run(&mut self, src: &str, stop_on_error: bool) -> (Vec<Out>, bool) {
        self.output.clear();
        let (prog, perr) = parse_program_partial(src);
        let quit = self.run_parsed(&prog, stop_on_error);
        if let Some(e) = perr {
            if !quit && !(stop_on_error && self.output.iter().any(|o| matches!(o, Out::Error(_)))) {
                self.output.push(Out::Error(e.msg));
            }
        }
        (std::mem::take(&mut self.output), quit)
    }

    /// Run parsed statements, appending to `self.output`. Returns true on quit.
    pub fn run_parsed(&mut self, prog: &[TopStmt], stop_on_error: bool) -> bool {
        for ts in prog {
            self.frames.clear();
            self.depth = 0;
            match self.exec_top(&ts.stmt, ts.show) {
                Ok(()) => {}
                Err(Exc::Quit) => return true,
                Err(Exc::Err(m)) => {
                    self.last_error = Some(m.clone());
                    self.output.push(Out::Error(m));
                    if stop_on_error {
                        return false;
                    }
                }
            }
        }
        false
    }

    fn exec_top(&mut self, s: &Stmt, show: bool) -> R<()> {
        if let Stmt::Expr(Expr::Sym(n)) = s {
            match n.as_str() {
                "restart" => {
                    self.restart();
                    return Ok(());
                }
                "quit" | "done" | "stop" => return Err(Exc::Quit),
                _ => {}
            }
        }
        match self.exec(s, show, 0)? {
            Flow::Normal(_) | Flow::Return(_) => Ok(()),
            Flow::Break | Flow::Next => err("break or next not in loop"),
        }
    }

    /// The `read` statement: run the statements of a Maple source file (path relative to the
    /// current directory), each shown or not by its own terminator.
    fn read_file(&mut self, e: &Expr) -> R<()> {
        let path = match self.eval(e)? {
            Expr::Str(s) | Expr::Sym(s) => s,
            other => return err(format!("(in read) invalid file name: {}", print(&other))),
        };
        let src = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return err(format!("could not open `{}` for reading", path)),
        };
        let (prog, perr) = parse_program_partial(&src);
        let saved = std::mem::take(&mut self.frames);
        let mut r: R<()> = Ok(());
        for ts in &prog {
            r = self.exec_top(&ts.stmt, ts.show);
            if r.is_err() {
                break;
            }
        }
        self.frames = saved;
        r?;
        match perr {
            Some(pe) => err(format!("syntax error in file {}, {}", path, pe.msg)),
            None => Ok(()),
        }
    }

    // ------------------------------------------------------------------ statements

    fn exec_block(&mut self, b: &[Stmt], show: bool, level: usize) -> R<Flow> {
        let mut last = Expr::null();
        for s in b {
            match self.exec(s, show, level)? {
                Flow::Normal(v) => last = v,
                other => return Ok(other),
            }
        }
        Ok(Flow::Normal(last))
    }

    fn display(&self, show: bool, level: usize) -> bool {
        show && level <= 1 && self.frames.is_empty()
    }

    fn exec(&mut self, s: &Stmt, show: bool, level: usize) -> R<Flow> {
        match s {
            Stmt::Expr(e) => {
                let v = self.eval(e)?;
                if self.frames.is_empty() && level == 0 && !v.is_null() {
                    self.history.insert(0, v.clone());
                    self.history.truncate(3);
                }
                if self.display(show, level) && !v.is_null() {
                    self.output.push(Out::Expr(v.clone()));
                }
                Ok(Flow::Normal(v))
            }
            Stmt::Assign(targets, rhs) => {
                let v = self.exec_assign(targets, rhs)?;
                if self.display(show, level) {
                    let lhs = targets.iter().map(print).collect::<Vec<_>>().join(", ");
                    self.output.push(Out::Assign(lhs, v.clone()));
                }
                Ok(Flow::Normal(v))
            }
            Stmt::If(branches, els) => {
                for (c, body) in branches {
                    let cv = self.eval(c)?;
                    if self.truth(&cv)? {
                        return self.exec_block(body, show, level + 1);
                    }
                }
                match els {
                    Some(b) => self.exec_block(b, show, level + 1),
                    None => Ok(Flow::Normal(Expr::null())),
                }
            }
            Stmt::For(lp) => self.exec_for(lp, show, level),
            Stmt::Return(e) => {
                let v = match e {
                    Some(e) => self.eval(e)?,
                    None => Expr::null(),
                };
                Ok(Flow::Return(v))
            }
            Stmt::Break => Ok(Flow::Break),
            Stmt::Next => Ok(Flow::Next),
            Stmt::Error(args) => {
                if args.is_empty() {
                    return Err(Exc::Err(self.last_error.clone().unwrap_or_else(|| "unspecified error".into())));
                }
                let mut vals = Vec::new();
                for a in args {
                    vals.extend(self.eval(a)?.into_seq());
                }
                Err(Exc::Err(format_error_msg(&vals)))
            }
            Stmt::Read(e) => {
                self.read_file(e)?;
                Ok(Flow::Normal(Expr::null()))
            }
            Stmt::Try(body, catches, fin) => {
                let r = self.exec_block(body, show, level + 1);
                let r = match r {
                    Err(Exc::Err(m)) => {
                        self.last_error = Some(m.clone());
                        let bare = strip_in(&m).to_string();
                        let mut handled = None;
                        for (pat, cb) in catches {
                            let matches = match pat {
                                None => true,
                                Some(p) => {
                                    let pv = self.eval(p)?;
                                    pv.into_seq().iter().any(|x| match x {
                                        Expr::Str(s) => bare.starts_with(s.as_str()),
                                        _ => false,
                                    })
                                }
                            };
                            if matches {
                                handled = Some(self.exec_block(cb, show, level + 1));
                                break;
                            }
                        }
                        handled.unwrap_or(Err(Exc::Err(m)))
                    }
                    other => other,
                };
                if let Some(f) = fin {
                    self.exec_block(f, show, level + 1)?;
                }
                r
            }
        }
    }

    fn exec_for(&mut self, lp: &crate::ast::ForLoop, show: bool, level: usize) -> R<Flow> {
        let mut last = Expr::null();
        let d = self.digits();
        macro_rules! body {
            () => {{
                if let Some(c) = &lp.cond {
                    let cv = self.eval(c)?;
                    if !self.truth(&cv)? {
                        break;
                    }
                }
                match self.exec_block(&lp.body, show, level + 1)? {
                    Flow::Normal(v) => last = v,
                    Flow::Break => break,
                    Flow::Next => {}
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                }
            }};
        }
        if let Some(over) = &lp.over {
            let container = self.eval(over)?;
            let items = operands_for_iteration(&container);
            for it in items {
                if let Some(v) = &lp.var {
                    self.assign_name(v, it)?;
                }
                body!();
            }
            return Ok(Flow::Normal(last));
        }
        let from = match &lp.from {
            Some(e) => self.eval(e)?,
            None => Expr::int(1),
        };
        let by = match &lp.by {
            Some(e) => self.eval(e)?,
            None => Expr::int(1),
        };
        let to = match &lp.to {
            Some(e) => Some(self.eval(e)?),
            None => None,
        };
        if lp.var.is_none() && lp.from.is_none() && lp.by.is_none() && to.is_none() {
            // while loop or bare do loop
            loop {
                body!();
            }
            return Ok(Flow::Normal(last));
        }
        let (Expr::Num(mut i), Expr::Num(step)) = (from.clone(), by.clone()) else {
            return err("initial value and increment in for loop must be numeric");
        };
        let to_n = match &to {
            Some(Expr::Num(n)) => Some(n.clone()),
            Some(Expr::Const(c)) if c == "infinity" => None,
            Some(_) => return err("final value in for loop must be numeric or character"),
            None => None,
        };
        if step.is_zero() {
            return err("increment in for loop must be nonzero");
        }
        let up = !step.is_negative();
        loop {
            if let Some(t) = &to_n {
                let c = i.cmp_value(t);
                if (up && c == std::cmp::Ordering::Greater) || (!up && c == std::cmp::Ordering::Less) {
                    break;
                }
            }
            if let Some(v) = &lp.var {
                self.assign_name(v, Expr::Num(i.clone()))?;
            }
            body!();
            i = i.add(&step, d);
        }
        if let Some(v) = &lp.var {
            if lp.cond.is_none() {
                let cur = self.lookup_raw(v);
                if let Some(Expr::Num(c)) = cur {
                    if let Some(t) = &to_n {
                        let past = c.add(&step, d);
                        if (up && past.cmp_value(t) == std::cmp::Ordering::Greater)
                            || (!up && past.cmp_value(t) == std::cmp::Ordering::Less)
                        {
                            self.assign_name(v, Expr::Num(past))?;
                        }
                    }
                }
            }
        }
        Ok(Flow::Normal(last))
    }

    fn exec_assign(&mut self, targets: &[Expr], rhs: &Expr) -> R<Expr> {
        if targets.len() == 1 {
            if let (Expr::Sym(n), Expr::Proc(_)) = (&targets[0], rhs) {
                self.defining = Some(n.clone());
            }
        }
        let v = self.eval(rhs);
        self.defining = None;
        let v = v?;
        if targets.len() == 1 {
            self.assign_target(&targets[0], v.clone())?;
            return Ok(v);
        }
        let vals = v.clone().into_seq();
        if vals.len() != targets.len() {
            return err("ambiguous multiple assignment");
        }
        for (t, x) in targets.iter().zip(vals) {
            self.assign_target(t, x)?;
        }
        Ok(v)
    }

    fn assign_target(&mut self, t: &Expr, v: Expr) -> R<()> {
        match t {
            Expr::Sym(n) => {
                if let Expr::Sym(m) = &v {
                    if m == n {
                        return self.unassign(n);
                    }
                }
                self.check_assignable(n, &v)?;
                self.assign_name(n, v)
            }
            Expr::Idx(base, idx) => {
                let Expr::Sym(n) = base.as_ref() else {
                    return err("invalid left hand side of assignment");
                };
                let mut iv = Vec::new();
                for i in idx {
                    iv.extend(self.eval(i)?.into_seq());
                }
                let cur = self.lookup_raw(n);
                let newv = match cur {
                    Some(c) => set_entry(c, &iv, v)?,
                    None => Expr::Table(vec![(Expr::from_seq(iv), v)]),
                };
                self.assign_name(n, newv)
            }
            Expr::Const(c) => err(format!("attempting to assign to `{}` which is protected", c)),
            Expr::Bool(_) | Expr::Fail => err("attempting to assign to a protected name"),
            Expr::Uneval(inner) => self.assign_target(inner, v),
            _ => err("invalid left hand side of assignment"),
        }
    }

    fn check_assignable(&self, n: &str, v: &Expr) -> R<()> {
        let local = self.frames.last().map(|f| f.vars.contains_key(n) || f.params.contains(n)).unwrap_or(false);
        if local {
            if self.frames.last().unwrap().params.contains(n) {
                return err(format!("illegal use of a formal parameter `{}`", n));
            }
            return Ok(());
        }
        if crate::builtins::is_protected(n) {
            return err(format!("attempting to assign to `{}` which is protected", n));
        }
        if n == "Digits" {
            match v.as_i64() {
                Some(d) if d >= 1 => {}
                _ => return err("Digits must be assigned a positive integer"),
            }
        }
        if !matches!(v, Expr::Proc(_)) && v.contains_sym(n) {
            return err("recursive assignment");
        }
        Ok(())
    }

    pub fn unassign(&mut self, n: &str) -> R<()> {
        if let Some(f) = self.frames.last_mut() {
            if f.vars.contains_key(n) {
                f.vars.insert(n.to_string(), Expr::sym(n));
                return Ok(());
            }
        }
        match n {
            "Digits" => {
                self.globals.insert("Digits".into(), Expr::int(10));
            }
            "Order" => {
                self.globals.insert("Order".into(), Expr::int(6));
            }
            _ => {
                self.globals.remove(n);
            }
        }
        Ok(())
    }

    /// Bind a name in the innermost scope that owns it (local, else global).
    pub fn assign_name(&mut self, n: &str, v: Expr) -> R<()> {
        if let Some(f) = self.frames.last_mut() {
            if f.vars.contains_key(n) && !f.params.contains(n) {
                f.vars.insert(n.to_string(), v);
                return Ok(());
            }
            if f.params.contains(n) {
                return err(format!("illegal use of a formal parameter `{}`", n));
            }
        }
        self.globals.insert(n.to_string(), v);
        Ok(())
    }

    /// Stored value of a name without evaluation.
    pub fn lookup_raw(&self, n: &str) -> Option<Expr> {
        if let Some(f) = self.frames.last() {
            if let Some(v) = f.vars.get(n) {
                if let Expr::Sym(s) = v {
                    if s == n {
                        return None;
                    }
                }
                return Some(v.clone());
            }
        }
        self.globals.get(n).cloned()
    }

    /// The procedure a name refers to, if any.
    pub fn proc_of(&self, n: &str) -> Option<Rc<Proc>> {
        match self.lookup_raw(n) {
            Some(Expr::Proc(p)) => Some(p),
            _ => None,
        }
    }

    // ------------------------------------------------------------------ truth

    pub fn truth(&mut self, c: &Expr) -> R<bool> {
        match self.evalb(c)? {
            Expr::Bool(b) => Ok(b),
            Expr::Fail => Ok(false),
            other => err(format!("cannot determine if this expression is true or false: {}", print(&other))),
        }
    }

    /// Maple evalb: true/false/FAIL, or the expression when undecidable.
    pub fn evalb(&mut self, c: &Expr) -> R<Expr> {
        let tri = |b: Option<bool>| match b {
            Some(true) => Expr::Bool(true),
            Some(false) => Expr::Bool(false),
            None => Expr::Fail,
        };
        let as_tri = |e: &Expr| match e {
            Expr::Bool(b) => Some(Some(*b)),
            Expr::Fail => Some(None),
            _ => None,
        };
        Ok(match c {
            Expr::Bool(_) | Expr::Fail => c.clone(),
            Expr::Rel(op, a, b) => match op {
                RelOp::Eq => Expr::Bool(same_value(a, b)),
                RelOp::Neq => Expr::Bool(!same_value(a, b)),
                RelOp::Lt | RelOp::Le => match compare_real(a, b) {
                    Some(o) => Expr::Bool(match op {
                        RelOp::Lt => o == std::cmp::Ordering::Less,
                        _ => o != std::cmp::Ordering::Greater,
                    }),
                    None => c.clone(),
                },
            },
            Expr::Not(a) => {
                let x = self.evalb(a)?;
                match as_tri(&x) {
                    Some(t) => tri(t.map(|b| !b)),
                    None => Expr::Not(Box::new(x)),
                }
            }
            Expr::And(v) => {
                let mut rest = Vec::new();
                let mut fail = false;
                for x in v {
                    let r = self.evalb(x)?;
                    match as_tri(&r) {
                        Some(Some(false)) => return Ok(Expr::Bool(false)),
                        Some(Some(true)) => {}
                        Some(None) => fail = true,
                        None => rest.push(r),
                    }
                }
                if !rest.is_empty() {
                    return Ok(if rest.len() == 1 { rest.pop().unwrap() } else { Expr::And(rest) });
                }
                if fail {
                    Expr::Fail
                } else {
                    Expr::Bool(true)
                }
            }
            Expr::Or(v) => {
                let mut rest = Vec::new();
                let mut fail = false;
                for x in v {
                    let r = self.evalb(x)?;
                    match as_tri(&r) {
                        Some(Some(true)) => return Ok(Expr::Bool(true)),
                        Some(Some(false)) => {}
                        Some(None) => fail = true,
                        None => rest.push(r),
                    }
                }
                if !rest.is_empty() {
                    return Ok(if rest.len() == 1 { rest.pop().unwrap() } else { Expr::Or(rest) });
                }
                if fail {
                    Expr::Fail
                } else {
                    Expr::Bool(false)
                }
            }
            Expr::Xor(a, b) => {
                let (x, y) = (self.evalb(a)?, self.evalb(b)?);
                match (as_tri(&x), as_tri(&y)) {
                    (Some(Some(p)), Some(Some(q))) => Expr::Bool(p != q),
                    (Some(None), _) | (_, Some(None)) => Expr::Fail,
                    _ => Expr::Xor(Box::new(x), Box::new(y)),
                }
            }
            Expr::Implies(a, b) => {
                let (x, y) = (self.evalb(a)?, self.evalb(b)?);
                match (as_tri(&x), as_tri(&y)) {
                    (Some(Some(false)), _) | (_, Some(Some(true))) => Expr::Bool(true),
                    (Some(Some(true)), Some(Some(false))) => Expr::Bool(false),
                    _ => Expr::Implies(Box::new(x), Box::new(y)),
                }
            }
            other => other.clone(),
        })
    }

    // ------------------------------------------------------------------ evaluation

    /// Evaluate parsed code.
    pub fn eval(&mut self, e: &Expr) -> R<Expr> {
        self.ev(e, true)
    }

    fn enter(&mut self) -> R<()> {
        self.depth += 1;
        if self.depth > 4 * MAX_DEPTH {
            self.depth = 0;
            return err("too many levels of recursion");
        }
        Ok(())
    }

    pub fn ev(&mut self, e: &Expr, fresh: bool) -> R<Expr> {
        self.enter()?;
        let r = self.ev_inner(e, fresh);
        self.depth = self.depth.saturating_sub(1);
        r
    }

    pub fn eval_list(&mut self, v: &[Expr], fresh: bool) -> R<Vec<Expr>> {
        let mut out = Vec::with_capacity(v.len());
        for x in v {
            out.extend(self.ev(x, fresh)?.into_seq());
        }
        Ok(out)
    }

    fn ev_inner(&mut self, e: &Expr, fresh: bool) -> R<Expr> {
        let d = self.digits();
        match e {
            Expr::Num(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Fail | Expr::Const(_) => Ok(e.clone()),
            Expr::Member(..) | Expr::Table(_) => Ok(e.clone()),
            Expr::Sym(n) => self.eval_name(n),
            Expr::Uneval(x) => Ok((**x).clone()),
            Expr::Hist(k) => Ok(self.history.get(*k as usize - 1).cloned().unwrap_or_else(Expr::null)),
            Expr::Add(v) => {
                let t = self.eval_list(v, fresh)?;
                let r = simpl::add(t, d)?;
                self.post_numeric(r)
            }
            Expr::Mul(v) => {
                let t = self.eval_list(v, fresh)?;
                let r = simpl::mul(t, d)?;
                self.post_numeric(r)
            }
            Expr::Pow(b, x) => {
                let b = self.ev(b, fresh)?;
                let x = self.ev(x, fresh)?;
                self.power(b, x)
            }
            Expr::Dot(a, b) => {
                let a = self.ev(a, fresh)?;
                let b = self.ev(b, fresh)?;
                Ok(simpl::dot(a, b, d)?)
            }
            Expr::Fn(name, args) => {
                if fresh {
                    return self.call_named(name, args);
                }
                // full evaluation: stored calls are evaluated again (engine calls are memoized)
                let nargs = self.eval_list(args, false)?;
                self.call_values(name, nargs)
            }
            Expr::Apply(h, args) => {
                let head = self.ev(h, fresh)?;
                // evalf[n](e): e itself is evaluated with Digits = n
                if let Expr::Idx(b, idx) = &head {
                    if **b == Expr::sym("evalf") && idx.len() == 1 && self.lookup_raw("evalf").is_none() {
                        if let Some(n) = idx[0].as_i64().filter(|n| *n > 0) {
                            let a = self.with_digits(n, |me| me.eval_list(args, fresh))?;
                            return self.apply_value(head, a, "");
                        }
                    }
                }
                let a = self.eval_list(args, fresh)?;
                self.apply_value(head, a, "")
            }
            Expr::Idx(base, idx) => {
                let iv = self.eval_list(idx, fresh)?;
                if let Expr::Sym(n) = base.as_ref() {
                    if let Some(raw) = self.lookup_raw(n) {
                        if is_container(&raw) {
                            return select(&raw, &iv, Some(n));
                        }
                    }
                    let b = self.ev(base, fresh)?;
                    if is_container(&b) {
                        return select(&b, &iv, None);
                    }
                    return Ok(Expr::Idx(Box::new(b), iv));
                }
                let b = self.ev(base, fresh)?;
                if is_container(&b) {
                    return select(&b, &iv, None);
                }
                Ok(Expr::Idx(Box::new(b), iv))
            }
            Expr::Rel(op, a, b) => Ok(Expr::Rel(*op, Box::new(self.ev(a, fresh)?), Box::new(self.ev(b, fresh)?))),
            Expr::And(_) | Expr::Or(_) | Expr::Not(_) | Expr::Xor(..) | Expr::Implies(..) => {
                let inner = e.clone();
                let mut errs = None;
                let ev = inner.map_children(&mut |c| match self.ev(c, fresh) {
                    Ok(x) => x,
                    Err(x) => {
                        errs = Some(x);
                        c.clone()
                    }
                });
                if let Some(x) = errs {
                    return Err(x);
                }
                let has_bool = ev.children().iter().any(|c| matches!(c, Expr::Bool(_) | Expr::Fail));
                if has_bool {
                    self.evalb(&ev)
                } else {
                    Ok(ev)
                }
            }
            Expr::List(v) => Ok(Expr::List(self.eval_list(v, fresh)?)),
            Expr::Set(v) => Ok(simpl::make_set(self.eval_list(v, fresh)?)),
            Expr::Seq(v) => Ok(Expr::from_seq(self.eval_list(v, fresh)?)),
            Expr::Range(a, b) => Ok(Expr::Range(Box::new(self.ev(a, fresh)?), Box::new(self.ev(b, fresh)?))),
            Expr::Matrix(rows) => {
                let mut out = Vec::new();
                for r in rows {
                    let mut row = Vec::new();
                    for x in r {
                        row.push(self.ev(x, fresh)?);
                    }
                    out.push(row);
                }
                Ok(Expr::Matrix(out))
            }
            Expr::Vector(v, o) => {
                let mut out = Vec::new();
                for x in v {
                    out.push(self.ev(x, fresh)?);
                }
                Ok(Expr::Vector(out, *o))
            }
            Expr::Proc(p) => {
                if !fresh {
                    return Ok(e.clone());
                }
                Ok(Expr::Proc(Rc::new(self.make_closure(p))))
            }
            Expr::Compose(v) => {
                let mut out = Vec::new();
                for x in v {
                    out.push(self.ev(x, fresh)?);
                }
                Ok(Expr::Compose(out))
            }
            Expr::Repeat(a, b) => Ok(Expr::Repeat(Box::new(self.ev(a, fresh)?), Box::new(self.ev(b, fresh)?))),
            Expr::TypeAssert(a, b) => {
                Ok(Expr::TypeAssert(Box::new(self.ev(a, fresh)?), Box::new(self.ev(b, fresh)?)))
            }
            Expr::In(a, b) => Ok(Expr::In(Box::new(self.ev(a, fresh)?), Box::new(self.ev(b, fresh)?))),
            Expr::Concat(a, b) => self.concat(a, b),
            Expr::Dollar(a, b) => self.dollar(a.as_deref(), b),
            Expr::Assuming(a, b) => {
                let rels = self.eval(b)?;
                let saved = self.assumptions.clone();
                for r in rels.into_seq() {
                    self.push_assumption(r, true)?;
                }
                let r = self.eval(a);
                self.assumptions = saved;
                r
            }
            Expr::AngleCol(v) => {
                let items = self.eval_list(v, fresh)?;
                angle_build(items, false)
            }
            Expr::AngleRow(v) => {
                let items = self.eval_list(v, fresh)?;
                angle_build(items, true)
            }
        }
    }

    fn eval_name(&mut self, n: &str) -> R<Expr> {
        if let Some(f) = self.frames.last() {
            if let Some(v) = f.vars.get(n) {
                return Ok(v.clone());
            }
            if let Some(pos) = f.missing.get(n) {
                return err(format!(
                    "invalid input: {} uses a {} argument, {}, which is missing",
                    f.procname,
                    ordinal(*pos),
                    n
                ));
            }
            match n {
                "args" | "_passed" => return Ok(Expr::from_seq(f.args.clone())),
                "nargs" | "_npassed" => return Ok(Expr::int(f.args.len() as i64)),
                "procname" => return Ok(Expr::sym(&f.procname)),
                "_rest" => return Ok(Expr::from_seq(f.args.iter().skip(f.nparams).cloned().collect())),
                "_nrest" => return Ok(Expr::int(f.args.len().saturating_sub(f.nparams) as i64)),
                _ => {}
            }
        }
        match self.globals.get(n) {
            None => match n {
                "lasterror" => Ok(match &self.last_error {
                    Some(m) => Expr::Str(strip_in(m).to_string()),
                    None => Expr::sym(n),
                }),
                // lastexception = procedure name (0 at top level), message
                "lastexception" => Ok(match &self.last_error {
                    Some(m) => {
                        let msg = strip_in(m);
                        let place = if msg.len() < m.len() {
                            Expr::sym(&m[4..m.len() - msg.len() - 2])
                        } else {
                            Expr::int(0)
                        };
                        Expr::Seq(vec![place, Expr::Str(msg.to_string())])
                    }
                    None => Expr::sym(n),
                }),
                _ => Ok(Expr::sym(n)),
            },
            Some(Expr::Proc(_)) | Some(Expr::Table(_)) => Ok(Expr::sym(n)),
            Some(v) => {
                let v = v.clone();
                if !needs_reeval(&v) {
                    return Ok(v);
                }
                self.ev(&v, false)
            }
        }
    }

    /// Numeric post-processing: high-precision float powers go to evalf.
    fn post_numeric(&mut self, r: Expr) -> R<Expr> {
        if self.digits() > 15 {
            let mut has = false;
            r.walk(&mut |x| {
                if let Expr::Pow(b, e) = x {
                    if matches!((b.as_ref(), e.as_ref()), (Expr::Num(p), Expr::Num(q)) if p.is_float() || q.is_float())
                    {
                        has = true;
                    }
                }
            });
            if has {
                return self.engine_call("evalf", vec![r]);
            }
        }
        Ok(r)
    }

    pub fn power(&mut self, b: Expr, x: Expr) -> R<Expr> {
        let d = self.digits();
        if let (Expr::Matrix(_), Some(n)) = (&b, x.as_i64()) {
            if n < 0 {
                let inv = self.engine_call("MatrixInverse", vec![b])?;
                if n == -1 {
                    return Ok(inv);
                }
                return Ok(simpl::pow(inv, Expr::int(-n), d)?);
            }
        }
        let r = simpl::pow(b, x, d)?;
        self.post_numeric(r)
    }

    fn concat(&mut self, a: &Expr, b: &Expr) -> R<Expr> {
        let left = match a {
            Expr::Sym(n) => n.clone(),
            Expr::Concat(..) => match self.ev(a, true)? {
                Expr::Sym(n) => n,
                Expr::Str(s) => s,
                other => print(&other),
            },
            Expr::Str(s) => s.clone(),
            other => print(&self.eval(other)?),
        };
        let is_str = matches!(a, Expr::Str(_));
        let rb = self.eval(b)?;
        let piece = |x: &Expr| match x {
            Expr::Sym(s) | Expr::Str(s) => s.clone(),
            other => print(other),
        };
        let mk = |s: String| if is_str { Expr::Str(s) } else { Expr::Sym(s) };
        if let Expr::Range(lo, hi) = &rb {
            if let (Some(l), Some(h)) = (lo.as_i64(), hi.as_i64()) {
                let mut out = Vec::new();
                for i in l..=h {
                    out.push(self.eval(&mk(format!("{}{}", left, i)))?);
                }
                return Ok(Expr::from_seq(out));
            }
        }
        if let Expr::Seq(items) = &rb {
            let mut out = Vec::new();
            for x in items {
                out.push(self.eval(&mk(format!("{}{}", left, piece(x))))?);
            }
            return Ok(Expr::from_seq(out));
        }
        self.eval(&mk(format!("{}{}", left, piece(&rb))))
    }

    fn dollar(&mut self, a: Option<&Expr>, b: &Expr) -> R<Expr> {
        let Some(a) = a else {
            let r = self.eval(b)?;
            if let Expr::Range(lo, hi) = &r {
                if let (Some(l), Some(h)) = (lo.as_i64(), hi.as_i64()) {
                    return Ok(Expr::from_seq((l..=h).map(Expr::int).collect()));
                }
            }
            return err("unable to execute $");
        };
        if let Expr::Rel(RelOp::Eq, v, _) = b {
            if matches!(v.as_ref(), Expr::Sym(_)) {
                return self.seq_loop("$", a, b, None);
            }
        }
        let n = self.eval(b)?;
        match n.as_i64() {
            Some(k) if k >= 0 => {
                let mut out = Vec::new();
                for _ in 0..k {
                    out.extend(self.eval(a)?.into_seq());
                }
                Ok(Expr::from_seq(out))
            }
            _ => err("unable to execute $"),
        }
    }

    // ------------------------------------------------------------------ procedures

    /// Create a procedure value: implicit locals and capture of enclosing locals/params.
    fn make_closure(&mut self, p: &Proc) -> Proc {
        let mut q = p.clone();
        if !p.arrow {
            let mut assigned = Vec::new();
            collect_assigned(&p.body, &mut assigned);
            for n in assigned {
                if q.params.iter().any(|x| x.name == n)
                    || q.locals.contains(&n)
                    || q.globals.contains(&n)
                    || matches!(n.as_str(), "Digits" | "Order" | "printlevel")
                {
                    continue;
                }
                let place = match &self.defining {
                    Some(f) => format!("(in {}) ", f),
                    None => String::new(),
                };
                if self.warnlevel > 0 {
                    self.output.push(Out::Warning(format!("{}`{}` is implicitly declared local", place, n)));
                }
                q.locals.push(n);
            }
        }
        if let Some(f) = self.frames.last() {
            let mut map: HashMap<String, Expr> = HashMap::new();
            for (k, v) in &f.vars {
                if matches!(v, Expr::Sym(s) if s == k) {
                    continue;
                }
                if q.params.iter().any(|x| &x.name == k) || q.locals.contains(k) {
                    continue;
                }
                map.insert(k.clone(), v.clone());
            }
            if !map.is_empty() {
                q.body = q.body.iter().map(|s| subst_stmt(s, &map)).collect();
            }
        }
        q
    }

    pub fn invoke(&mut self, p: &Rc<Proc>, args: Vec<Expr>, name: &str) -> R<Expr> {
        if self.frames.len() >= MAX_DEPTH {
            return err("too many levels of recursion");
        }
        let pname = if name.is_empty() { "unknown".to_string() } else { name.to_string() };
        let remember = p.options.iter().any(|o| o == "remember" || o == "cache");
        let key = (Rc::as_ptr(p) as usize, args.iter().map(print).collect::<Vec<_>>().join(","));
        if remember {
            if let Some(v) = self.remember.get(&key) {
                return Ok(v.clone());
            }
        }
        let mut frame = Frame {
            vars: HashMap::new(),
            params: HashSet::new(),
            missing: HashMap::new(),
            procname: pname.clone(),
            args: args.clone(),
            nparams: p.params.len(),
        };
        for (i, prm) in p.params.iter().enumerate() {
            frame.params.insert(prm.name.clone());
            if i < args.len() {
                if let Some(t) = &prm.ty {
                    if let Some(false) = self.check_type(&args[i], t) {
                        return err(format!(
                            "(in {}) invalid input: {} expects its {} argument, {}, to be of type {}, but received {}",
                            pname,
                            pname,
                            ordinal(i + 1),
                            prm.name,
                            print(t),
                            print(&args[i])
                        ));
                    }
                }
                frame.vars.insert(prm.name.clone(), args[i].clone());
            } else if let Some(dv) = &prm.default {
                frame.vars.insert(prm.name.clone(), dv.clone());
            } else {
                frame.missing.insert(prm.name.clone(), i + 1);
            }
        }
        for l in &p.locals {
            frame.vars.insert(l.clone(), Expr::sym(l));
        }
        self.frames.push(frame);
        let saved_depth = self.depth;
        let r = self.exec_block(&p.body, false, 2);
        self.depth = saved_depth;
        self.frames.pop();
        let v = match r {
            Ok(Flow::Normal(v)) | Ok(Flow::Return(v)) => v,
            Ok(Flow::Break) | Ok(Flow::Next) => return err(format!("(in {}) break or next not in loop", pname)),
            Err(Exc::Err(m)) => {
                if m.starts_with("(in ") {
                    return Err(Exc::Err(m));
                }
                return Err(Exc::Err(format!("(in {}) {}", pname, m)));
            }
            Err(q) => return Err(q),
        };
        if remember {
            self.remember.insert(key, v.clone());
        }
        Ok(v)
    }

    /// Run `f` with the global Digits temporarily set to `n`.
    pub fn with_digits<T>(&mut self, n: i64, f: impl FnOnce(&mut Interp) -> R<T>) -> R<T> {
        let saved = self.globals.insert("Digits".into(), Expr::int(n));
        let r = f(self);
        match saved {
            Some(v) => self.globals.insert("Digits".into(), v),
            None => self.globals.remove("Digits"),
        };
        r
    }

    /// Apply any value to arguments, following Maple rules.
    pub fn apply_value(&mut self, head: Expr, args: Vec<Expr>, name: &str) -> R<Expr> {
        let d = self.digits();
        match head {
            Expr::Proc(p) => self.invoke(&p, args, name),
            Expr::Sym(n) => self.call_values(&n, args),
            Expr::Member(pkg, f) => self.call_package(&pkg, &f, args),
            Expr::Idx(b, idx) => match b.as_ref() {
                Expr::Sym(n) => self.apply_indexed(n, idx, args),
                _ => Ok(Expr::Apply(Box::new(Expr::Idx(b, idx)), args)),
            },
            Expr::Compose(fs) => {
                let mut cur = args;
                for f in fs.into_iter().rev() {
                    cur = self.apply_value(f, cur, "")?.into_seq();
                }
                Ok(Expr::from_seq(cur))
            }
            Expr::Repeat(f, n) => match n.as_i64() {
                Some(k) if k >= 0 => {
                    let orig = args.clone();
                    let mut cur = args;
                    for _ in 0..k {
                        cur = self.apply_value((*f).clone(), cur, "")?.into_seq();
                        // (D@@n)(y) stays in that form when D(y) is unknown
                        if *f == Expr::sym("D") && matches!(cur.as_slice(), [Expr::Fn(d, _)] if d == "D") {
                            return Ok(Expr::Apply(Box::new(Expr::Repeat(f, n)), orig));
                        }
                    }
                    Ok(Expr::from_seq(cur))
                }
                _ => Ok(Expr::Apply(Box::new(Expr::Repeat(f, n)), args)),
            },
            Expr::Num(_) | Expr::Const(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Fail => Ok(head),
            Expr::Add(v) => {
                let mut t = Vec::new();
                for x in v {
                    t.push(self.apply_value(x, args.clone(), "")?);
                }
                Ok(simpl::add(t, d)?)
            }
            Expr::Mul(v) => {
                let mut t = Vec::new();
                for x in v {
                    t.push(self.apply_value(x, args.clone(), "")?);
                }
                Ok(simpl::mul(t, d)?)
            }
            Expr::Pow(b, x) => {
                let b = self.apply_value(*b, args.clone(), "")?;
                let x = self.apply_value(*x, args, "")?;
                self.power(b, x)
            }
            Expr::List(v) => {
                let mut t = Vec::new();
                for x in v {
                    t.push(self.apply_value(x, args.clone(), "")?);
                }
                Ok(Expr::List(t))
            }
            other => Ok(Expr::Apply(Box::new(other), args)),
        }
    }

    /// `name(args)` with unevaluated argument code.
    fn call_named(&mut self, name: &str, raw: &[Expr]) -> R<Expr> {
        if self.lookup_raw(name).is_none() && crate::builtins::is_special_eval(name) {
            return self.special(name, raw);
        }
        let args = self.eval_list(raw, true)?;
        self.call_values(name, args)
    }

    /// `name(args)` with evaluated arguments.
    pub fn call_values(&mut self, name: &str, args: Vec<Expr>) -> R<Expr> {
        if let Some(v) = self.lookup_raw(name) {
            return match v {
                Expr::Proc(p) => self.invoke(&p, args, name),
                Expr::Table(_) => Ok(Expr::Fn(name.to_string(), args)),
                other => {
                    let head = if needs_reeval(&other) { self.ev(&other, false)? } else { other };
                    self.apply_value(head, args, name)
                }
            };
        }
        if let Some(r) = self.builtin(name, args.clone())? {
            return Ok(r);
        }
        if crate::builtins::package_of(name).is_some() {
            // the package loaded last by `with` wins, as in Maple
            let pkg = self.packages.iter().rev().find(|p| crate::builtins::exports_name(p, name)).cloned();
            if let Some(pkg) = pkg {
                return self.call_package(&pkg, name, args);
            }
            return Ok(Expr::Fn(name.to_string(), args));
        }
        self.engine_call(name, args)
    }

    pub fn call_package(&mut self, pkg: &str, f: &str, args: Vec<Expr>) -> R<Expr> {
        if !crate::builtins::is_package(pkg) {
            if let Some(Expr::Table(t)) = self.lookup_raw(pkg) {
                let key = Expr::sym(f);
                if let Some((_, v)) = t.iter().find(|(k, _)| *k == key) {
                    return self.apply_value(v.clone(), args, f);
                }
            }
            return err(format!("`{}` does not evaluate to a module", pkg));
        }
        if let Some(r) = self.package_builtin(pkg, f, args.clone())? {
            return Ok(r);
        }
        self.engine_call(&format!("{}:-{}", pkg, f), args)
    }

    // ------------------------------------------------------------------ engine

    pub fn opts(&self) -> Result<Value, String> {
        let d = self.digits();
        let a: Result<Vec<Value>, String> = self.assumptions.iter().map(|x| to_json(x, d)).collect();
        Ok(json!({"digits": d, "assume": a?}))
    }

    pub fn engine_request(&mut self, req: Value, ctx: &str) -> R<Value> {
        let r = self.engine.borrow_mut().request(req, ctx);
        r.map_err(Exc::Err)
    }

    /// Call a math function in the engine.
    pub fn engine_call(&mut self, name: &str, args: Vec<Expr>) -> R<Expr> {
        let name = match name {
            "sum" if self.inert => "Sum",
            "product" if self.inert => "Product",
            "int" if self.inert => "Int",
            other => other,
        };
        let d = self.digits();
        let short = name.rsplit(":-").next().unwrap_or(name).to_string();
        let mut ja = Vec::new();
        for a in &args {
            let a = self.inline_procs(a);
            ja.push(to_json(&a, d).map_err(|m| Exc::Err(format!("(in {}) {}", short, m)))?);
        }
        let req = json!({"op": "call", "name": name, "args": ja, "opts": self.opts()?});
        let res = self.engine_request(req, &short)?;
        Ok(from_json(&res, d)?)
    }

    /// Replace names of arrow procedures by the procedures themselves (for the engine).
    fn inline_procs(&self, e: &Expr) -> Expr {
        match e {
            Expr::Sym(n) => match self.proc_of(n) {
                Some(p) if p.arrow && matches!(p.body.as_slice(), [Stmt::Expr(_)]) => Expr::Proc(p),
                _ => e.clone(),
            },
            Expr::Proc(_) => e.clone(),
            other => other.map_children(&mut |c| self.inline_procs(c)),
        }
    }

    pub fn push_assumption(&mut self, r: Expr, additionally: bool) -> R<()> {
        let mut parts = Vec::new();
        match r {
            Expr::And(v) => parts.extend(v),
            other => parts.push(other),
        }
        for p in parts {
            match &p {
                Expr::Rel(..) | Expr::TypeAssert(..) | Expr::Or(_) | Expr::Not(_) | Expr::In(..) => {}
                other => return err(format!("(in assume) invalid assumption: {}", print(other))),
            }
            if !additionally {
                let vars = free_names(&p);
                self.assumptions.retain(|a| free_names(a).iter().all(|n| !vars.contains(n)));
            }
            if !self.assumptions.contains(&p) {
                self.assumptions.push(p);
            }
        }
        Ok(())
    }

    /// Text for an output item (pretty 2-D via the engine when requested).
    pub fn render(&mut self, o: &Out, pretty: bool) -> String {
        match o {
            Out::Expr(e) | Out::Print(e) => self.render_expr(e, pretty),
            Out::Assign(l, e) => {
                let t = self.render_expr(e, pretty);
                if t.contains('\n') {
                    format!("{} :=\n{}", l, t)
                } else {
                    format!("{} := {}", l, t)
                }
            }
            Out::Text(t) => t.clone(),
            Out::Warning(w) => format!("Warning, {}", w),
            Out::Error(m) => format!("Error, {}", m),
        }
    }

    pub fn render_expr(&mut self, e: &Expr, pretty: bool) -> String {
        let flat = print(e);
        if !pretty || matches!(e, Expr::Num(_) | Expr::Sym(_) | Expr::Str(_) | Expr::Bool(_) | Expr::Proc(_)) {
            return flat;
        }
        let d = self.digits();
        let Ok(j) = to_json(e, d) else { return flat };
        match self.engine.borrow_mut().request(json!({"op": "pretty", "expr": j, "width": 80}), "pretty") {
            Ok(Value::String(s)) if !s.trim().is_empty() => s.trim_end().to_string(),
            _ => flat,
        }
    }

    pub fn latex(&mut self, e: &Expr) -> R<String> {
        let d = self.digits();
        let j = to_json(e, d).map_err(Exc::Err)?;
        match self.engine_request(json!({"op": "latex", "expr": j}), "latex")? {
            Value::String(s) => Ok(s),
            other => Ok(other.to_string()),
        }
    }
}

// ------------------------------------------------------------------ helpers

fn format_error_msg(vals: &[Expr]) -> String {
    let text = |x: &Expr| match x {
        Expr::Str(s) => s.clone(),
        other => print(other),
    };
    let Some(Expr::Str(fmt)) = vals.first() else {
        return vals.iter().map(text).collect::<Vec<_>>().join(", ");
    };
    let chars: Vec<char> = fmt.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '%' && i + 1 < chars.len() {
            let mut j = i + 1;
            let neg = chars[j] == '-';
            if neg {
                j += 1;
            }
            if j < chars.len() && chars[j].is_ascii_digit() {
                let k = chars[j].to_digit(10).unwrap() as usize;
                if let Some(v) = vals.get(k) {
                    if neg {
                        out.push_str(&match v.as_i64() {
                            Some(n) if n > 0 => ordinal(n as usize),
                            _ => text(v),
                        });
                    } else {
                        out.push_str(&text(v));
                    }
                }
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn strip_in(m: &str) -> &str {
    if m.starts_with("(in ") {
        if let Some(p) = m.find(") ") {
            return &m[p + 2..];
        }
    }
    m
}

fn needs_reeval(v: &Expr) -> bool {
    let mut r = false;
    v.walk(&mut |x| {
        if matches!(x, Expr::Sym(_) | Expr::Fn(..) | Expr::Apply(..) | Expr::Idx(..)) {
            r = true;
        }
    });
    r
}

pub fn free_names(e: &Expr) -> Vec<String> {
    let mut out = Vec::new();
    e.walk(&mut |x| {
        if let Expr::Sym(s) = x {
            if !out.contains(s) {
                out.push(s.clone());
            }
        }
    });
    out
}

fn same_value(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => x.cmp_value(y) == std::cmp::Ordering::Equal,
        _ => a == b,
    }
}

fn compare_real(a: &Expr, b: &Expr) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    let inf = |e: &Expr| -> Option<i8> {
        match e {
            Expr::Const(c) if c == "infinity" => Some(1),
            Expr::Mul(v) if v.len() == 2 && matches!(&v[1], Expr::Const(c) if c == "infinity") => match &v[0] {
                Expr::Num(n) if n.is_negative() => Some(-1),
                Expr::Num(_) => Some(1),
                _ => None,
            },
            _ => None,
        }
    };
    match (a, b) {
        (Expr::Num(x), Expr::Num(y)) => Some(x.cmp_value(y)),
        _ => match (inf(a), inf(b)) {
            (Some(x), Some(y)) => Some(x.cmp(&y)),
            (Some(x), None) if matches!(b, Expr::Num(_)) => Some(if x > 0 { Ordering::Greater } else { Ordering::Less }),
            (None, Some(y)) if matches!(a, Expr::Num(_)) => Some(if y > 0 { Ordering::Less } else { Ordering::Greater }),
            _ => None,
        },
    }
}

pub fn is_container(e: &Expr) -> bool {
    matches!(e, Expr::List(_) | Expr::Set(_) | Expr::Seq(_) | Expr::Matrix(_) | Expr::Vector(..) | Expr::Table(_) | Expr::Str(_))
}

/// Elements visited by `for x in e` and `seq(f, x in e)`.
pub fn operands_for_iteration(e: &Expr) -> Vec<Expr> {
    match e {
        Expr::List(v) | Expr::Set(v) | Expr::Seq(v) | Expr::Add(v) | Expr::Mul(v) => v.clone(),
        Expr::Vector(v, _) => v.clone(),
        Expr::Matrix(rows) => rows.iter().flatten().cloned().collect(),
        Expr::Fn(_, a) => a.clone(),
        Expr::Pow(a, b) | Expr::Rel(_, a, b) | Expr::Range(a, b) => vec![(**a).clone(), (**b).clone()],
        Expr::Str(s) => s.chars().map(|c| Expr::Str(c.to_string())).collect(),
        other => vec![other.clone()],
    }
}

fn index_of(i: &Expr, n: usize) -> R<usize> {
    let Some(k) = i.as_i64() else { return err("invalid subscript selector") };
    let k = if k < 0 { n as i64 + k + 1 } else { k };
    if k < 1 || k as usize > n {
        return err("invalid subscript selector");
    }
    Ok(k as usize - 1)
}

fn range_of(i: &Expr, n: usize) -> R<Option<(usize, usize)>> {
    let Expr::Range(a, b) = i else { return Ok(None) };
    let (Some(lo), Some(hi)) = (a.as_i64(), b.as_i64()) else { return err("invalid subscript selector") };
    let fix = |k: i64| if k < 0 { n as i64 + k + 1 } else { k };
    let (lo, hi) = (fix(lo), fix(hi));
    if hi < lo {
        return Ok(Some((1, 0)));
    }
    if lo < 1 || hi as usize > n {
        return err("invalid subscript selector");
    }
    Ok(Some((lo as usize, hi as usize)))
}

/// Selection `e[i, ...]` on lists, sets, sequences, strings, Matrices, Vectors and tables.
pub fn select(e: &Expr, idx: &[Expr], name: Option<&str>) -> R<Expr> {
    match e {
        Expr::List(v) | Expr::Set(v) | Expr::Seq(v) => {
            if idx.is_empty() {
                return Ok(e.clone());
            }
            let mut cur: Expr = e.clone();
            for i in idx {
                let items = match &cur {
                    Expr::List(v) | Expr::Set(v) | Expr::Seq(v) => v.clone(),
                    _ => return err("invalid subscript selector"),
                };
                cur = match range_of(i, items.len())? {
                    Some((lo, hi)) => {
                        let part: Vec<Expr> = if hi >= lo { items[lo - 1..hi].to_vec() } else { vec![] };
                        match &cur {
                            Expr::List(_) => Expr::List(part),
                            Expr::Set(_) => Expr::Set(part),
                            _ => Expr::from_seq(part),
                        }
                    }
                    None => items[index_of(i, items.len())?].clone(),
                };
            }
            let _ = v;
            Ok(cur)
        }
        Expr::Str(s) => {
            let chars: Vec<char> = s.chars().collect();
            if idx.len() != 1 {
                return err("invalid subscript selector");
            }
            match range_of(&idx[0], chars.len())? {
                Some((lo, hi)) => {
                    Ok(Expr::Str(if hi >= lo { chars[lo - 1..hi].iter().collect() } else { String::new() }))
                }
                None => Ok(Expr::Str(chars[index_of(&idx[0], chars.len())?].to_string())),
            }
        }
        Expr::Vector(v, o) => {
            if idx.len() != 1 {
                return err("invalid subscript selector");
            }
            match range_of(&idx[0], v.len())? {
                Some((lo, hi)) => Ok(Expr::Vector(if hi >= lo { v[lo - 1..hi].to_vec() } else { vec![] }, *o)),
                None => {
                    if idx[0].as_i64().map(|k| k < 0).unwrap_or(false) {
                        return err("invalid subscript selector");
                    }
                    Ok(v[index_of(&idx[0], v.len())?].clone())
                }
            }
        }
        Expr::Matrix(rows) => {
            let m = rows.len();
            let n = rows.first().map(|r| r.len()).unwrap_or(0);
            let pick = |i: &Expr, len: usize| -> R<(Vec<usize>, bool)> {
                match range_of(i, len)? {
                    Some((lo, hi)) => Ok(((lo..=hi).map(|k| k - 1).collect(), true)),
                    None => {
                        if i.as_i64().map(|k| k < 1).unwrap_or(true) {
                            return err("invalid subscript selector");
                        }
                        Ok((vec![index_of(i, len)?], false))
                    }
                }
            };
            match idx.len() {
                1 => {
                    let (ri, is_range) = pick(&idx[0], m)?;
                    if is_range {
                        return Ok(Expr::Matrix(ri.iter().map(|&r| rows[r].clone()).collect()));
                    }
                    Ok(Expr::Vector(rows[ri[0]].clone(), true))
                }
                2 => {
                    let (ri, rr) = pick(&idx[0], m)?;
                    let (ci, cr) = pick(&idx[1], n)?;
                    match (rr, cr) {
                        (false, false) => Ok(rows[ri[0]][ci[0]].clone()),
                        (false, true) => Ok(Expr::Vector(ci.iter().map(|&c| rows[ri[0]][c].clone()).collect(), true)),
                        (true, false) => Ok(Expr::Vector(ri.iter().map(|&r| rows[r][ci[0]].clone()).collect(), false)),
                        (true, true) => Ok(Expr::Matrix(
                            ri.iter().map(|&r| ci.iter().map(|&c| rows[r][c].clone()).collect()).collect(),
                        )),
                    }
                }
                _ => err("invalid subscript selector"),
            }
        }
        Expr::Table(t) => {
            let key = Expr::from_seq(idx.to_vec());
            match t.iter().find(|(k, _)| *k == key) {
                Some((_, v)) => Ok(v.clone()),
                None => Ok(Expr::Idx(Box::new(name.map(Expr::sym).unwrap_or_else(|| e.clone())), idx.to_vec())),
            }
        }
        _ => Ok(Expr::Idx(Box::new(e.clone()), idx.to_vec())),
    }
}

/// Assign `c[idx] := v` returning the updated container.
fn set_entry(c: Expr, idx: &[Expr], v: Expr) -> R<Expr> {
    match c {
        Expr::List(mut items) => {
            if idx.len() != 1 {
                return err("invalid subscript selector");
            }
            let k = index_of(&idx[0], items.len())?;
            items[k] = v;
            Ok(Expr::List(items))
        }
        Expr::Vector(mut items, o) => {
            if idx.len() != 1 {
                return err("invalid subscript selector");
            }
            let k = index_of(&idx[0], items.len())?;
            items[k] = v;
            Ok(Expr::Vector(items, o))
        }
        Expr::Matrix(mut rows) => {
            if idx.len() != 2 {
                return err("invalid subscript selector");
            }
            let i = index_of(&idx[0], rows.len())?;
            let j = index_of(&idx[1], rows.first().map(|r| r.len()).unwrap_or(0))?;
            rows[i][j] = v;
            Ok(Expr::Matrix(rows))
        }
        Expr::Table(mut t) => {
            let key = Expr::from_seq(idx.to_vec());
            if let Some(slot) = t.iter_mut().find(|(k, _)| *k == key) {
                slot.1 = v;
            } else {
                t.push((key, v));
            }
            Ok(Expr::Table(t))
        }
        _ => Ok(Expr::Table(vec![(Expr::from_seq(idx.to_vec()), v)])),
    }
}

fn angle_build(items: Vec<Expr>, row: bool) -> R<Expr> {
    if items.iter().all(|x| !matches!(x, Expr::Matrix(_) | Expr::Vector(..))) {
        return Ok(Expr::Vector(items, row));
    }
    // block construction: convert every item to a Matrix and stack
    let as_matrix = |x: &Expr| -> Matrixish {
        match x {
            Expr::Matrix(r) => r.clone(),
            Expr::Vector(v, true) => vec![v.clone()],
            Expr::Vector(v, false) => v.iter().map(|e| vec![e.clone()]).collect(),
            other => vec![vec![other.clone()]],
        }
    };
    let blocks: Vec<Matrixish> = items.iter().map(as_matrix).collect();
    if row {
        let h = blocks[0].len();
        if blocks.iter().any(|b| b.len() != h) {
            return err("(in Matrix) dimensions of the blocks do not agree");
        }
        let rows = (0..h).map(|i| blocks.iter().flat_map(|b| b[i].clone()).collect()).collect();
        Ok(Expr::Matrix(rows))
    } else {
        let w = blocks[0].first().map(|r| r.len()).unwrap_or(0);
        if blocks.iter().any(|b| b.first().map(|r| r.len()).unwrap_or(0) != w) {
            return err("(in Matrix) dimensions of the blocks do not agree");
        }
        Ok(Expr::Matrix(blocks.into_iter().flatten().collect()))
    }
}

type Matrixish = Vec<Vec<Expr>>;

fn collect_assigned(b: &[Stmt], out: &mut Vec<String>) {
    for s in b {
        match s {
            Stmt::Assign(ts, _) => {
                for t in ts {
                    let n = match t {
                        Expr::Sym(n) => Some(n.clone()),
                        Expr::Idx(b, _) => match b.as_ref() {
                            Expr::Sym(n) => Some(n.clone()),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some(n) = n {
                        if !out.contains(&n) {
                            out.push(n);
                        }
                    }
                }
            }
            Stmt::If(br, els) => {
                for (_, b) in br {
                    collect_assigned(b, out);
                }
                if let Some(e) = els {
                    collect_assigned(e, out);
                }
            }
            Stmt::For(lp) => {
                if let Some(v) = &lp.var {
                    if !out.contains(v) {
                        out.push(v.clone());
                    }
                }
                collect_assigned(&lp.body, out);
            }
            Stmt::Try(b, c, f) => {
                collect_assigned(b, out);
                for (_, cb) in c {
                    collect_assigned(cb, out);
                }
                if let Some(f) = f {
                    collect_assigned(f, out);
                }
            }
            _ => {}
        }
    }
}

/// Substitute names by values inside code, respecting shadowing by inner procedures.
pub fn subst_code(e: &Expr, map: &HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Sym(n) => map.get(n).cloned().unwrap_or_else(|| e.clone()),
        Expr::Proc(p) => {
            let mut m2 = map.clone();
            for x in &p.params {
                m2.remove(&x.name);
            }
            for l in &p.locals {
                m2.remove(l);
            }
            if m2.is_empty() {
                return e.clone();
            }
            let mut q = (**p).clone();
            q.body = q.body.iter().map(|s| subst_stmt(s, &m2)).collect();
            Expr::Proc(Rc::new(q))
        }
        other => other.map_children(&mut |c| subst_code(c, map)),
    }
}

fn subst_target(t: &Expr, map: &HashMap<String, Expr>) -> Expr {
    match t {
        Expr::Sym(_) => t.clone(),
        Expr::Idx(b, i) => Expr::Idx(b.clone(), i.iter().map(|x| subst_code(x, map)).collect()),
        other => subst_code(other, map),
    }
}

pub fn subst_stmt(s: &Stmt, map: &HashMap<String, Expr>) -> Stmt {
    let blk = |b: &Vec<Stmt>| b.iter().map(|x| subst_stmt(x, map)).collect::<Vec<_>>();
    match s {
        Stmt::Expr(e) => Stmt::Expr(subst_code(e, map)),
        Stmt::Assign(t, v) => Stmt::Assign(t.iter().map(|x| subst_target(x, map)).collect(), subst_code(v, map)),
        Stmt::If(br, els) => Stmt::If(
            br.iter().map(|(c, b)| (subst_code(c, map), blk(b))).collect(),
            els.as_ref().map(blk),
        ),
        Stmt::For(lp) => {
            let mut l = (**lp).clone();
            l.from = l.from.map(|x| subst_code(&x, map));
            l.by = l.by.map(|x| subst_code(&x, map));
            l.to = l.to.map(|x| subst_code(&x, map));
            l.over = l.over.map(|x| subst_code(&x, map));
            l.cond = l.cond.map(|x| subst_code(&x, map));
            l.body = blk(&l.body);
            Stmt::For(Box::new(l))
        }
        Stmt::Return(e) => Stmt::Return(e.as_ref().map(|x| subst_code(x, map))),
        Stmt::Error(a) => Stmt::Error(a.iter().map(|x| subst_code(x, map)).collect()),
        Stmt::Read(e) => Stmt::Read(subst_code(e, map)),
        Stmt::Try(b, c, f) => Stmt::Try(
            blk(b),
            c.iter().map(|(p, cb)| (p.clone(), blk(cb))).collect(),
            f.as_ref().map(blk),
        ),
        other => other.clone(),
    }
}

impl Interp {
    /// Numeric value for printf and friends (uses evalf in the engine when needed).
    pub fn numeric_value(&mut self, e: &Expr) -> R<Num> {
        if let Expr::Num(n) = e {
            return Ok(n.clone());
        }
        match self.engine_call("evalf", vec![e.clone()])? {
            Expr::Num(n) => Ok(n),
            other => err(format!("expected a numeric value, got {}", print(&other))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run without the math engine; returns rendered 1-D output lines.
    fn run(src: &str) -> Vec<String> {
        let src = src.to_string();
        std::thread::Builder::new()
            .stack_size(1 << 30)
            .spawn(move || {
                let mut it = Interp::new(Rc::new(RefCell::new(Engine::new())));
                let (outs, _) = it.run(&src, false);
                outs.iter().map(|o| it.render(o, false)).collect()
            })
            .unwrap()
            .join()
            .unwrap()
    }

    fn one(src: &str) -> String {
        run(src).join("\n")
    }

    #[test]
    fn automatic_simplification() {
        assert_eq!(one("x+x;"), "2*x");
        assert_eq!(one("x*x*3;"), "3*x^2");
        assert_eq!(one("2*(x+1);"), "2*x+2");
        assert_eq!(one("-(x-y);"), "-x+y");
        assert_eq!(one("x - x;"), "0");
        assert_eq!(one("x/x;"), "1");
        assert_eq!(one("(x^2)^3;"), "x^6");
        assert_eq!(one("sqrt2 := 2^(1/2): sqrt2*sqrt2;"), "2");
        assert_eq!(one("(2*x)^2;"), "4*x^2");
        assert_eq!(one("4^(1/2); 8^(1/3); 12^(1/2); (-4)^(1/2);"), "2\n2\nsqrt(12)\n2*I");
        assert_eq!(one("I^2; I^3;"), "-1\n-I");
        assert_eq!(one("x^0; 0^0; 1^x;"), "1\n1\n1");
        assert_eq!(one("1/3 + 1/6; 2/4;"), "1/2\n1/2");
        assert_eq!(one("2^100;"), "1267650600228229401496703205376");
        assert_eq!(one("30!;"), "265252859812191058636308480000000");
        assert_eq!(one("x/2 + x/3;"), "5*x/6");
        assert_eq!(one("(a=b) + 1; 2*(a=b);"), "a+1 = b+1\n2*a = 2*b");
        assert_eq!(one("[1,2] + [3,4]; 3*[1,2];"), "[4, 6]\n[3, 6]");
        assert_eq!(one("(1+I)*(1-I); 1/(1+I); (2+3*I)^2;"), "2\n1/2-I/2\n-5+12*I");
        assert_eq!(one("infinity - 1; -3*infinity; infinity - infinity;"), "infinity\n-infinity\nundefined");
    }

    #[test]
    fn floats() {
        assert_eq!(one("0.1 + 0.2;"), "0.3");
        assert_eq!(one("1/3.0;"), "0.3333333333");
        assert_eq!(one("2.0*3; 2.*3;"), "6.0\n6.");
        assert_eq!(one("2.50 + 1;"), "3.50");
        assert_eq!(one("evalf(1/3); evalf(2/3);"), "0.3333333333\n0.6666666667");
        assert_eq!(one("Digits := 20: 1/3.0;"), "0.33333333333333333333");
        assert_eq!(one("evalf[5](1/3);"), "0.33333");
        assert_eq!(one("1e10; 1.5e-20; 2.5e3;"), "1.*10^10\n1.5*10^(-20)\n2500.");
        assert_eq!(one("2.0^0.5;"), "1.414213562");
        assert_eq!(one("x + 0.5*x;"), "1.5*x");
        assert_eq!(one("evalb(1 = 1.0); evalb(0.5 < 1/2);"), "true\nfalse");
    }

    #[test]
    fn names_and_evaluation() {
        assert_eq!(one("a := b; b := 3; a;"), "a := b\nb := 3\n3");
        assert_eq!(one("x := 5: x := 'x'; x;"), "x := x\nx");
        assert_eq!(one("y := 2: 'y'; ''y'';"), "y\n'y'");
        assert_eq!(one("a := a + 1;"), "Error, recursive assignment");
        assert_eq!(one("sin := 3;"), "Error, attempting to assign to `sin` which is protected");
        assert_eq!(one("Pi := 3;"), "Error, attempting to assign to `Pi` which is protected");
        assert_eq!(one("1/0;"), "Error, numeric exception: division by zero");
        assert_eq!(one("x^2^3;"), "Error, ambiguous use of `^`, please use parentheses");
        assert_eq!(one("x := 3: x := 'x': assigned(x); y := 1: assigned(y);"), "false\ntrue");
        assert_eq!(one("a, b := 1, 2; a+b;"), "a, b := 1, 2\n3");
        assert_eq!(one("x||1; i := 2: x||i; cat(\"ab\", 3); cat(a, b);"), "x1\nx2\n\"ab3\"\nab");
        assert_eq!(one("a||(1..3);"), "a1, a2, a3");
    }

    #[test]
    fn history() {
        assert_eq!(one("1+1; %+1; %%; %%%;"), "2\n3\n2\n2");
        assert_eq!(one("5: x := 3: %;"), "5");
    }

    #[test]
    fn sequences_and_containers() {
        assert_eq!(one("seq(i^2, i=1..5);"), "1, 4, 9, 16, 25");
        assert_eq!(one("seq(i, i=1..2, 0.5);"), "1, 1.5, 2.0");
        assert_eq!(one("seq(x[i], i in [a,b]);"), "x[a], x[b]");
        assert_eq!(one("seq(1..4); seq(k, k=[a,b,c]);"), "1, 2, 3, 4\na, b, c");
        assert_eq!(one("i := 7: seq(i, i=1..3); i;"), "1, 2, 3\n7");
        assert_eq!(one("add(i, i=1..100); mul(i, i=1..6); add(i^2, i in [1,2,3]);"), "5050\n720\n14");
        assert_eq!(one("x$3; [f(i)$i=1..3]; $2..4;"), "x, x, x\n[f(1), f(2), f(3)]\n2, 3, 4");
        assert_eq!(one("L := [a,b,c,d]: L[2]; L[-1]; L[2..3]; nops(L);"), "b\nd\n[b, c]\n4");
        assert_eq!(one("L := [a,b]: L[3];"), "Error, invalid subscript selector");
        assert_eq!(one("L := [1,2,3]: L[2] := 7: L;"), "[1, 7, 3]");
        assert_eq!(one("{c, a, b, a}; {3, 1, 2};"), "{a, b, c}\n{1, 2, 3}");
        assert_eq!(one("{1,2} union {2,3}; {1,2} intersect {2,3}; {1,2} minus {2};"), "{1, 2, 3}\n{2}\n{1}");
        assert_eq!(one("s := a, b: [s, c]; NULL; [NULL];"), "[a, b, c]\n[]");
        assert_eq!(one("member(b, [a,b]); numelems([1,2,3]);"), "true\n3");
        assert_eq!(one("T := table(): T[x] := 1: T[x]; T[y]; T; eval(T);"), "1\nT[y]\nT\ntable([x = 1])");
        assert_eq!(one("t[1] := 5: t[1]; t[2];"), "5\nt[2]");
        assert_eq!(one("\"hello\"[2..3];"), "\"el\"");
    }

    #[test]
    fn op_nops_subs() {
        assert_eq!(one("nops(x+y+z); op(2, x+y+z); op(x*y); op(0, f(x)); op(1, x^2);"), "3\ny\nx, y\nf\nx");
        assert_eq!(one("op(-1, [a,b,c]); op(2..3, [a,b,c]); op([2,1], [a, [b, c]]);"), "c\nb, c\nb");
        assert_eq!(one("op(1/3); op(x = y); nops(f(a,b,c)); lhs(a=b); rhs(a=b);"), "1, 3\nx, y\n3\na\nb");
        assert_eq!(one("subs(x=1, x^2+x); subs(x=y, y=2, x+y); subs({x=y, y=x}, x-y);"), "2\n4\ny-x");
        assert_eq!(one("subs(x=0, f(x)); eval(x^2+y, x=2); eval(x+y, [x=1, y=2]);"), "f(0)\ny+4\n3");
        assert_eq!(one("subsop(2=z, [a,b,c]); map(f, [a,b]); map(f, x+y); map2(f, a, [b,c]);"), "[a, z, c]\n[f(a), f(b)]\nf(x)+f(y)\n[f(a, b), f(a, c)]");
        assert_eq!(one("select(t -> t > 2, [1,2,3,4]); remove(t -> t > 2, [1,2,3,4]); zip((a,b)->a+b, [1,2], [3,4]);"), "[3, 4]\n[1, 2]\n[4, 6]");
        assert_eq!(one("whattype(x+y); whattype([1]); whattype(1/2); type(3, integer); type(x, name); type([1,2], list(integer));"), "`+`\nlist\nfraction\ntrue\ntrue\ntrue");
    }

    #[test]
    fn procedures() {
        assert_eq!(one("f := x -> x^2: f(3); f(a+1);"), "9\n(a+1)^2");
        assert_eq!(one("g := (x,y) -> x*y: g(2,3);"), "6");
        assert_eq!(
            one("fib := proc(n) option remember; if n < 2 then n else fib(n-1) + fib(n-2) end if end proc: fib(50);"),
            "12586269025"
        );
        assert_eq!(one("f := proc(x) local y; y := x+1; y^2 end proc: f(2); y;"), "9\ny");
        assert_eq!(one("f := proc(x) y := x end proc:"), "Warning, (in f) `y` is implicitly declared local");
        assert_eq!(one("f := proc(x, y) x + y end proc: f(1);"), "Error, (in f) invalid input: f uses a 2nd argument, y, which is missing");
        assert_eq!(one("f := proc(x) error \"bad value %1\", x end proc: f(3);"), "Error, (in f) bad value 3");
        assert_eq!(one("f := proc() nargs end proc: f(a,b,c); f();"), "3\n0");
        assert_eq!(one("f := proc() [args] end proc: f(1,2);"), "[1, 2]");
        assert_eq!(one("f := proc(n) if n = 0 then return 7 end if; n end proc: f(0); f(2);"), "7\n2");
        assert_eq!(one("h := x -> (y -> x + y): h(2)(5);"), "7");
        assert_eq!(one("r := proc(n) r(n+1) end proc: r(1);"), "Error, (in r) too many levels of recursion");
        assert_eq!(one("f := proc(x::integer) x end proc: f(1/2);"), "Error, (in f) invalid input: f expects its 1st argument, x, to be of type integer, but received 1/2");
        assert_eq!(one("f := x -> x^2: f;"), "f");
        assert_eq!(one("f := x -> x^2: eval(f);"), "x -> x^2");
        assert_eq!(one("(f@g)(x); (f@@2)(x); (2*f)(x); (f+g)(x); 3(x);"), "f(g(x))\nf(f(x))\n2*f(x)\nf(x)+g(x)\n3");
        assert_eq!(one("unapply(x^2+y, x)(3);"), "y+9");
        assert_eq!(one("`if`(1 < 2, a, b); `if`(false, a, b);"), "a\nb");
        assert_eq!(one("f := proc(n) local i, s; s := 0; for i to n do s := s + i end do; s end proc: f(10);"), "55");
    }

    #[test]
    fn control_flow() {
        assert_eq!(one("for i to 3 do i^2 end do;"), "1\n4\n9");
        assert_eq!(one("for i to 3 do i^2 end do:"), "");
        assert_eq!(one("s := 0: for i from 1 to 10 by 3 do s := s + i end do: s; i;"), "22\n13");
        assert_eq!(one("for i from 5 to 1 by -2 do i end do;"), "5\n3\n1");
        assert_eq!(one("n := 0: while n < 5 do n := n + 1 end do: n;"), "5");
        assert_eq!(one("for x in [a, b] do print(x) end do:"), "a\nb");
        assert_eq!(one("for i do if i > 3 then break end if end do: i;"), "4");
        assert_eq!(one("s := 0: for i to 5 do if i = 3 then next end if; s := s + i od: s;"), "12");
        assert_eq!(one("if x < 1 then a end if;"), "Error, cannot determine if this expression is true or false: x < 1");
        assert_eq!(one("if Pi < 4 then a end if;"), "Error, cannot determine if this expression is true or false: Pi < 4");
        assert_eq!(one("if 1 < 2 and 3 <> 4 then yes else no end if;"), "yes");
        assert_eq!(one("try error \"boom\" catch \"boom\": caught end try;"), "caught");
        assert_eq!(one("try 1/0 catch: lasterror end try;"), "\"numeric exception: division by zero\"");
        assert_eq!(one("try error \"x\" catch \"y\": 1 end try;"), "Error, x");
        assert_eq!(one("evalb(2 < 3); evalb(x = x); evalb(x < y); 1 < 2;"), "true\ntrue\nx < y\n1 < 2");
        assert_eq!(one("true and false; not true; x and true;"), "false\nfalse\nx");
    }

    #[test]
    fn matrices() {
        assert_eq!(one("<1,2,3>; <1|2|3>;"), "Vector[column]([1, 2, 3])\nVector[row]([1, 2, 3])");
        assert_eq!(one("<<1|2>,<3|4>>; <<1,3>|<2,4>>;"), "Matrix(2, 2, [[1, 2], [3, 4]])\nMatrix(2, 2, [[1, 2], [3, 4]])");
        assert_eq!(one("A := <<1|2>,<3|4>>: A.A; A + A; 2*A; A^2; A[2,1]; A[1];"),
            "Matrix(2, 2, [[7, 10], [15, 22]])\nMatrix(2, 2, [[2, 4], [6, 8]])\nMatrix(2, 2, [[2, 4], [6, 8]])\nMatrix(2, 2, [[7, 10], [15, 22]])\n3\nVector[row]([1, 2])");
        assert_eq!(one("<1,2>.<3,4>; <1|2>.<3,4>;"), "11\n11");
        assert_eq!(one("A := <<1|2>,<3|4>>: A*A;"), "Error, (in rtable/Product) invalid arguments; use `.` for matrix products");
        assert_eq!(one("Matrix(2, 2, (i,j) -> i+j); Matrix([[1,2],[3,4]]); Vector(3, i -> i^2); Matrix(2,2);"),
            "Matrix(2, 2, [[2, 3], [3, 4]])\nMatrix(2, 2, [[1, 2], [3, 4]])\nVector[column]([1, 4, 9])\nMatrix(2, 2, [[0, 0], [0, 0]])");
        assert_eq!(one("Matrix(2, 2, symbol = m); Vector[row]([a, b]);"), "Matrix(2, 2, [[m[1, 1], m[1, 2]], [m[2, 1], m[2, 2]]])\nVector[row]([a, b])");
        assert_eq!(one("A := Matrix(2,2): A[1,2] := 5: A;"), "Matrix(2, 2, [[0, 5], [0, 0]])");
        assert_eq!(one("Determinant(<<1|2>,<3|4>>);"), "Determinant(Matrix(2, 2, [[1, 2], [3, 4]]))");
        assert_eq!(one("with(LinearAlgebra): Transpose(<<1|2>,<3|4>>); LinearAlgebra:-RowDimension(<<1|2>>);"),
            "Matrix(2, 2, [[1, 3], [2, 4]])\n1");
        assert_eq!(one("<<1|2>,<3>>;"), "Error, (in Matrix) dimensions of the blocks do not agree");
    }

    #[test]
    fn printing_and_session() {
        assert_eq!(one("printf(\"%d|%5.2f|%-4s|%a|%e|%g\\n\", 42, 3.14159, \"ab\", x^2, 1.5, 0.0001);"), "42| 3.14|ab  |x^2|1.500000e+00|0.0001\n");
        assert_eq!(one("sprintf(\"%03d\", 7);"), "\"007\"");
        assert_eq!(one("lprint(x^2+1);"), "x^2+1\n");
        assert_eq!(one("print(a, b);"), "a, b");
        assert_eq!(one("interface(prettyprint = 0);"), "0");
        assert_eq!(one("x := 1: restart; x;"), "x");
        assert_eq!(one("assume(x > 0): about(x);"), "x:\n  is assumed to be: 0 < x\n");
        assert_eq!(one("assume(x > 0): assume(x < 5): about(x);"), "x:\n  is assumed to be: x < 5\n");
        assert_eq!(one("assume(x > 0): additionally(x < 5): about(x);"), "x:\n  is assumed to be: 0 < x, x < 5\n");
        assert_eq!(one("Digits := 0;"), "Error, Digits must be assigned a positive integer");
        assert_eq!(one("with(foo);"), "Error, (in with) foo is not a package");
        assert_eq!(one("10 mod 3; -7 mod 3; parse(\"1+2\");"), "1\n2\n3");
        assert_eq!(one("quit; 1;"), "");
    }
}
