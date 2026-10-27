//   AlgebraKing //
//
//   File:         [parser.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Recursive-descent parser with Maple operator precedence.']

use crate::ast::{Expr, ForLoop, Param, Proc, RelOp, Stmt, TopStmt};
use crate::lexer::{tokenize, Tok, Token};
use crate::number::Num;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub msg: String,
    pub incomplete: bool,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.msg)
    }
}

type PResult<T> = Result<T, ParseError>;

const KEYWORDS: [&str; 39] = [
    "and", "or", "not", "xor", "implies", "mod", "in", "if", "then", "elif", "else", "fi", "end", "do", "od", "for",
    "from", "to", "by", "while", "proc", "local", "global", "option", "options", "return", "break", "next", "error",
    "try", "catch", "finally", "description", "assuming", "union", "intersect", "minus", "subset", "read",
];

const BLOCK_END: [&str; 9] = ["end", "fi", "od", "else", "elif", "catch", "finally", "until", "Eof"];

pub fn is_keyword(s: &str) -> bool {
    KEYWORDS.contains(&s)
}

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    angle: usize,
}

/// Parse a whole program (sequence of statements).
#[cfg(test)]
pub fn parse_program(src: &str) -> PResult<Vec<TopStmt>> {
    let toks = tokenize(src).map_err(|e| ParseError { msg: e.msg, incomplete: e.incomplete })?;
    let mut p = Parser { toks, pos: 0, angle: 0 };
    p.program()
}

/// Parse statements up to the first syntax error; returns them and the error, if any.
pub fn parse_program_partial(src: &str) -> (Vec<TopStmt>, Option<ParseError>) {
    let toks = match tokenize(src) {
        Ok(t) => t,
        Err(e) => return (vec![], Some(ParseError { msg: e.msg, incomplete: e.incomplete })),
    };
    let mut p = Parser { toks, pos: 0, angle: 0 };
    let mut out = Vec::new();
    loop {
        match p.next_top() {
            Ok(Some(t)) => out.push(t),
            Ok(None) => return (out, None),
            Err(e) => return (out, Some(e)),
        }
    }
}

/// Parse a single expression (used by the MCP tools); a trailing `;` is allowed.
pub fn parse_expr(src: &str) -> PResult<Expr> {
    let toks = tokenize(src).map_err(|e| ParseError { msg: e.msg, incomplete: e.incomplete })?;
    let mut p = Parser { toks, pos: 0, angle: 0 };
    let e = p.expr_full()?;
    if p.is_op(";") || p.is_op(":") {
        p.pos += 1;
    }
    if !p.at_eof() {
        return Err(p.unexpected());
    }
    Ok(e)
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn at_eof(&self) -> bool {
        matches!(self.peek(), Tok::Eof)
    }

    fn is_op(&self, op: &str) -> bool {
        matches!(self.peek(), Tok::Op(o) if *o == op)
    }

    fn is_kw(&self, kw: &str) -> bool {
        matches!(self.peek(), Tok::Name(n) if n == kw)
    }

    fn eat_op(&mut self, op: &str) -> bool {
        if self.is_op(op) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_kw(&mut self, kw: &str) -> bool {
        if self.is_kw(kw) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn describe(t: &Tok) -> String {
        match t {
            Tok::Int(s) | Tok::Float(s) | Tok::Name(s) => format!("`{}`", s),
            Tok::QName(s) => format!("`{}`", s),
            Tok::Str(s) => format!("\"{}\"", s),
            Tok::Op(o) => format!("`{}`", o),
            Tok::Quote => "`'`".into(),
            Tok::Eof => "end of input".into(),
        }
    }

    fn unexpected(&self) -> ParseError {
        let t = &self.toks[self.pos];
        if matches!(t.tok, Tok::Eof) {
            return ParseError { msg: "unexpected end of input".into(), incomplete: true };
        }
        ParseError {
            msg: format!("syntax error, unexpected {} at line {}, column {}", Self::describe(&t.tok), t.line, t.col),
            incomplete: false,
        }
    }

    fn expect_op(&mut self, op: &str) -> PResult<()> {
        if self.eat_op(op) {
            Ok(())
        } else {
            let mut e = self.unexpected();
            if !e.incomplete {
                e.msg = format!("{} (expected `{}`)", e.msg, op);
            }
            Err(e)
        }
    }

    fn expect_kw(&mut self, kw: &str) -> PResult<()> {
        if self.eat_kw(kw) {
            Ok(())
        } else {
            let mut e = self.unexpected();
            if !e.incomplete {
                e.msg = format!("{} (expected `{}`)", e.msg, kw);
            }
            Err(e)
        }
    }

    fn at_block_end(&self) -> bool {
        match self.peek() {
            Tok::Eof => true,
            Tok::Name(n) => BLOCK_END.contains(&n.as_str()),
            _ => false,
        }
    }

    // ---------------------------------------------------------------- statements

    #[cfg(test)]
    fn program(&mut self) -> PResult<Vec<TopStmt>> {
        let mut out = Vec::new();
        while let Some(t) = self.next_top()? {
            out.push(t);
        }
        Ok(out)
    }

    fn next_top(&mut self) -> PResult<Option<TopStmt>> {
        while self.eat_op(";") || self.eat_op(":") {}
        if self.at_eof() {
            return Ok(None);
        }
        let stmt = self.statement()?;
        let show = if self.eat_op(";") {
            true
        } else if self.eat_op(":") {
            false
        } else if self.at_eof() {
            true
        } else {
            return Err(self.unexpected_after_statement());
        };
        Ok(Some(TopStmt { stmt, show }))
    }

    fn unexpected_after_statement(&self) -> ParseError {
        let mut e = self.unexpected();
        if !e.incomplete {
            e.msg = format!("{} (missing operator or `;`)", e.msg);
        }
        e
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        let mut out = Vec::new();
        loop {
            while self.eat_op(";") || self.eat_op(":") {}
            if self.at_block_end() {
                if self.at_eof() {
                    return Err(ParseError { msg: "unexpected end of input".into(), incomplete: true });
                }
                return Ok(out);
            }
            out.push(self.statement()?);
            if self.eat_op(";") || self.eat_op(":") {
                continue;
            }
            if self.at_block_end() {
                if self.at_eof() {
                    return Err(ParseError { msg: "unexpected end of input".into(), incomplete: true });
                }
                return Ok(out);
            }
            return Err(self.unexpected_after_statement());
        }
    }

    fn end_block(&mut self, short: &str, kw: &str) -> PResult<()> {
        if short != "end" && self.eat_kw(short) {
            return Ok(());
        }
        self.expect_kw("end")?;
        self.eat_kw(kw);
        Ok(())
    }

    fn statement(&mut self) -> PResult<Stmt> {
        if let Tok::Name(n) = self.peek().clone() {
            match n.as_str() {
                "if" => {
                    self.pos += 1;
                    return self.if_stmt();
                }
                "for" | "while" | "do" | "from" | "to" | "by" => return self.loop_stmt(),
                "return" => {
                    self.pos += 1;
                    if self.is_op(";") || self.is_op(":") || self.at_block_end() {
                        return Ok(Stmt::Return(None));
                    }
                    return Ok(Stmt::Return(Some(self.expr_seq()?)));
                }
                "break" => {
                    self.pos += 1;
                    return Ok(Stmt::Break);
                }
                "next" => {
                    self.pos += 1;
                    return Ok(Stmt::Next);
                }
                "error" => {
                    self.pos += 1;
                    if self.is_op(";") || self.is_op(":") || self.at_block_end() {
                        return Ok(Stmt::Error(vec![]));
                    }
                    return Ok(Stmt::Error(self.expr_seq()?.into_seq()));
                }
                "try" => {
                    self.pos += 1;
                    return self.try_stmt();
                }
                "read" => {
                    self.pos += 1;
                    return Ok(Stmt::Read(self.expr()?));
                }
                _ => {}
            }
        }
        let lhs = self.expr_full()?;
        if self.eat_op(":=") {
            let rhs = self.expr_full()?;
            return Ok(Stmt::Assign(lhs.into_seq(), rhs));
        }
        Ok(Stmt::Expr(lhs))
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let mut branches = Vec::new();
        let cond = self.expr()?;
        self.expect_kw("then")?;
        let body = self.block()?;
        branches.push((cond, body));
        let mut els = None;
        loop {
            if self.eat_kw("elif") {
                let c = self.expr()?;
                self.expect_kw("then")?;
                let b = self.block()?;
                branches.push((c, b));
            } else if self.eat_kw("else") {
                els = Some(self.block()?);
                self.end_block("fi", "if")?;
                break;
            } else {
                self.end_block("fi", "if")?;
                break;
            }
        }
        Ok(Stmt::If(branches, els))
    }

    fn loop_stmt(&mut self) -> PResult<Stmt> {
        let mut lp = ForLoop { var: None, from: None, by: None, to: None, over: None, cond: None, body: vec![] };
        if self.eat_kw("for") {
            match self.peek().clone() {
                Tok::Name(n) if !is_keyword(&n) => {
                    self.pos += 1;
                    lp.var = Some(n);
                }
                _ => return Err(self.unexpected()),
            }
            if self.eat_kw("in") {
                lp.over = Some(self.expr()?);
            }
        }
        loop {
            if self.eat_kw("from") {
                lp.from = Some(self.expr()?);
            } else if self.eat_kw("by") {
                lp.by = Some(self.expr()?);
            } else if self.eat_kw("to") {
                lp.to = Some(self.expr()?);
            } else if self.eat_kw("while") {
                lp.cond = Some(self.expr()?);
            } else {
                break;
            }
        }
        self.expect_kw("do")?;
        lp.body = self.block()?;
        self.end_block("od", "do")?;
        Ok(Stmt::For(Box::new(lp)))
    }

    fn try_stmt(&mut self) -> PResult<Stmt> {
        let body = self.block()?;
        let mut catches = Vec::new();
        let mut fin = None;
        loop {
            if self.eat_kw("catch") {
                let pat = if self.eat_op(":") {
                    None
                } else {
                    let p = self.expr_seq()?;
                    self.expect_op(":")?;
                    Some(p)
                };
                let b = self.block()?;
                catches.push((pat, b));
            } else if self.eat_kw("finally") {
                fin = Some(self.block()?);
            } else {
                self.end_block("end", "try")?;
                break;
            }
        }
        Ok(Stmt::Try(body, catches, fin))
    }

    // ---------------------------------------------------------------- expressions

    /// expression sequence with an optional trailing `assuming`
    fn expr_full(&mut self) -> PResult<Expr> {
        let e = self.expr_seq()?;
        if self.eat_kw("assuming") {
            let a = self.expr_seq()?;
            return Ok(Expr::Assuming(Box::new(e), Box::new(a)));
        }
        Ok(e)
    }

    fn expr_seq(&mut self) -> PResult<Expr> {
        let first = self.expr()?;
        if !self.is_op(",") {
            return Ok(first);
        }
        let mut items = vec![first];
        while self.eat_op(",") {
            items.push(self.expr()?);
        }
        Ok(Expr::Seq(items))
    }

    /// one expression (no top-level comma)
    pub fn expr(&mut self) -> PResult<Expr> {
        self.arrow()
    }

    fn arrow(&mut self) -> PResult<Expr> {
        let lhs = self.implies()?;
        if self.eat_op("->") {
            let params = Self::arrow_params(&lhs).ok_or_else(|| ParseError {
                msg: "invalid parameters for arrow procedure".into(),
                incomplete: false,
            })?;
            // the body may be an if statement: (a, b) -> if a = b then 1 else 0 end if
            let body = if self.is_kw("if") {
                self.pos += 1;
                self.if_stmt()?
            } else {
                Stmt::Expr(self.arrow()?)
            };
            let proc = Proc {
                params,
                locals: vec![],
                globals: vec![],
                options: vec!["operator".into(), "arrow".into()],
                body: vec![body],
                arrow: true,
            };
            return Ok(Expr::Proc(Rc::new(proc)));
        }
        Ok(lhs)
    }

    fn arrow_params(e: &Expr) -> Option<Vec<Param>> {
        let one = |e: &Expr| -> Option<Param> {
            match e {
                Expr::Sym(n) => Some(Param { name: n.clone(), ty: None, default: None }),
                Expr::TypeAssert(a, t) => match a.as_ref() {
                    Expr::Sym(n) => Some(Param { name: n.clone(), ty: Some((**t).clone()), default: None }),
                    _ => None,
                },
                _ => None,
            }
        };
        match e {
            Expr::Seq(v) => v.iter().map(one).collect(),
            other => one(other).map(|p| vec![p]),
        }
    }

    fn implies(&mut self) -> PResult<Expr> {
        let lhs = self.or()?;
        if self.eat_kw("implies") {
            let rhs = self.or()?;
            return Ok(Expr::Implies(Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn or(&mut self) -> PResult<Expr> {
        let mut lhs = self.and()?;
        loop {
            if self.eat_kw("or") {
                let rhs = self.and()?;
                lhs = match lhs {
                    Expr::Or(mut v) => {
                        v.push(rhs);
                        Expr::Or(v)
                    }
                    l => Expr::Or(vec![l, rhs]),
                };
            } else if self.eat_kw("xor") {
                let rhs = self.and()?;
                lhs = Expr::Xor(Box::new(lhs), Box::new(rhs));
            } else {
                return Ok(lhs);
            }
        }
    }

    fn and(&mut self) -> PResult<Expr> {
        let mut lhs = self.not()?;
        while self.eat_kw("and") {
            let rhs = self.not()?;
            lhs = match lhs {
                Expr::And(mut v) => {
                    v.push(rhs);
                    Expr::And(v)
                }
                l => Expr::And(vec![l, rhs]),
            };
        }
        Ok(lhs)
    }

    fn not(&mut self) -> PResult<Expr> {
        if self.eat_kw("not") {
            let e = self.not()?;
            return Ok(Expr::Not(Box::new(e)));
        }
        self.dollar()
    }

    fn dollar(&mut self) -> PResult<Expr> {
        if self.eat_op("$") {
            let rhs = self.relation()?;
            return Ok(Expr::Dollar(None, Box::new(rhs)));
        }
        let lhs = self.relation()?;
        if self.eat_op("$") {
            let rhs = self.relation()?;
            return Ok(Expr::Dollar(Some(Box::new(lhs)), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn relation(&mut self) -> PResult<Expr> {
        let lhs = self.type_assert()?;
        let op = match self.peek() {
            Tok::Op("=") => Some("="),
            Tok::Op("<>") => Some("<>"),
            Tok::Op("<") => Some("<"),
            Tok::Op("<=") => Some("<="),
            Tok::Op(">") if self.angle == 0 => Some(">"),
            Tok::Op(">=") if self.angle == 0 => Some(">="),
            Tok::Name(n) if n == "in" => Some("in"),
            Tok::Name(n) if n == "subset" => Some("subset"),
            _ => None,
        };
        let Some(op) = op else { return Ok(lhs) };
        self.pos += 1;
        let rhs = self.type_assert()?;
        let e = match op {
            "=" => Expr::rel(RelOp::Eq, lhs, rhs),
            "<>" => Expr::rel(RelOp::Neq, lhs, rhs),
            "<" => Expr::rel(RelOp::Lt, lhs, rhs),
            "<=" => Expr::rel(RelOp::Le, lhs, rhs),
            ">" => Expr::rel(RelOp::Lt, rhs, lhs),
            ">=" => Expr::rel(RelOp::Le, rhs, lhs),
            "in" => Expr::In(Box::new(lhs), Box::new(rhs)),
            _ => Expr::func("subset", vec![lhs, rhs]),
        };
        if matches!(self.peek(), Tok::Op("=") | Tok::Op("<>") | Tok::Op("<") | Tok::Op("<="))
            || (self.angle == 0 && matches!(self.peek(), Tok::Op(">") | Tok::Op(">=")))
        {
            return Err(self.unexpected());
        }
        Ok(e)
    }

    fn type_assert(&mut self) -> PResult<Expr> {
        let lhs = self.range()?;
        if self.eat_op("::") {
            let rhs = self.range()?;
            return Ok(Expr::TypeAssert(Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn range(&mut self) -> PResult<Expr> {
        let lhs = self.modulo()?;
        if self.eat_op("..") {
            let rhs = self.modulo()?;
            return Ok(Expr::Range(Box::new(lhs), Box::new(rhs)));
        }
        Ok(lhs)
    }

    fn modulo(&mut self) -> PResult<Expr> {
        let lhs = self.additive()?;
        if self.eat_kw("mod") {
            let rhs = self.additive()?;
            return Ok(Expr::func("mod", vec![lhs, rhs]));
        }
        Ok(lhs)
    }

    fn additive(&mut self) -> PResult<Expr> {
        let mut lhs = if self.eat_op("-") {
            Expr::raw_neg(self.multiplicative()?)
        } else {
            self.eat_op("+");
            self.multiplicative()?
        };
        loop {
            if self.eat_op("+") {
                let rhs = self.multiplicative()?;
                lhs = push_add(lhs, rhs);
            } else if self.eat_op("-") {
                let rhs = self.multiplicative()?;
                lhs = push_add(lhs, Expr::raw_neg(rhs));
            } else if self.eat_kw("union") {
                let rhs = self.multiplicative()?;
                lhs = Expr::func("union", vec![lhs, rhs]);
            } else if self.eat_kw("minus") {
                let rhs = self.multiplicative()?;
                lhs = Expr::func("minus", vec![lhs, rhs]);
            } else {
                return Ok(lhs);
            }
        }
    }

    fn multiplicative(&mut self) -> PResult<Expr> {
        let mut lhs = self.unary()?;
        loop {
            if self.eat_op("*") {
                let rhs = self.unary()?;
                lhs = push_mul(lhs, rhs);
            } else if self.eat_op("/") {
                let rhs = self.unary()?;
                lhs = push_mul(lhs, Expr::Pow(Box::new(rhs), Box::new(Expr::int(-1))));
            } else if self.eat_op(".") {
                let rhs = self.unary()?;
                lhs = Expr::Dot(Box::new(lhs), Box::new(rhs));
            } else if self.eat_op("@") {
                let rhs = self.unary()?;
                lhs = match lhs {
                    Expr::Compose(mut v) => {
                        v.push(rhs);
                        Expr::Compose(v)
                    }
                    l => Expr::Compose(vec![l, rhs]),
                };
            } else if self.eat_kw("intersect") {
                let rhs = self.unary()?;
                lhs = Expr::func("intersect", vec![lhs, rhs]);
            } else if self.eat_op("&*") {
                let rhs = self.unary()?;
                lhs = Expr::func("&*", vec![lhs, rhs]);
            } else {
                return Ok(lhs);
            }
        }
    }

    // unary minus inside a product, e.g. 2*-3
    fn unary(&mut self) -> PResult<Expr> {
        if self.eat_op("-") {
            return Ok(Expr::raw_neg(self.unary()?));
        }
        if self.eat_op("+") {
            return self.unary();
        }
        if self.is_kw("not") {
            self.pos += 1;
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        self.power()
    }

    fn power(&mut self) -> PResult<Expr> {
        let base = self.postfix()?;
        let is_pow = self.is_op("^");
        if is_pow || self.is_op("@@") {
            self.pos += 1;
            let exp = if self.eat_op("-") {
                Expr::raw_neg(self.postfix()?)
            } else {
                self.eat_op("+");
                self.postfix()?
            };
            if self.is_op("^") || self.is_op("@@") {
                let o = if is_pow { "^" } else { "@@" };
                return Err(ParseError {
                    msg: format!("ambiguous use of `{}`, please use parentheses", o),
                    incomplete: false,
                });
            }
            return Ok(if is_pow {
                Expr::Pow(Box::new(base), Box::new(exp))
            } else {
                Expr::Repeat(Box::new(base), Box::new(exp))
            });
        }
        Ok(base)
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            if self.is_op("(") {
                self.pos += 1;
                let args = self.inner(|p| p.args_until(")"))?;
                self.expect_op(")")?;
                e = match e {
                    Expr::Sym(n) => Expr::Fn(n, args),
                    other => Expr::Apply(Box::new(other), args),
                };
            } else if self.is_op("[") {
                self.pos += 1;
                let idx = self.inner(|p| p.args_until("]"))?;
                self.expect_op("]")?;
                e = Expr::Idx(Box::new(e), idx);
            } else if self.is_op("~") && matches!(self.toks.get(self.pos + 1).map(|t| &t.tok), Some(Tok::Op("("))) {
                // elementwise application f~(args) is `~`[f](args)
                self.pos += 2;
                let args = self.inner(|p| p.args_until(")"))?;
                self.expect_op(")")?;
                e = Expr::Apply(Box::new(Expr::Idx(Box::new(Expr::sym("~")), vec![e])), args);
            } else if self.eat_op("!") {
                e = Expr::func("factorial", vec![e]);
            } else if self.eat_op(":-") {
                let name = self.name_token()?;
                e = match e {
                    Expr::Sym(p) => Expr::Member(p, name),
                    _ => return Err(self.unexpected()),
                };
            } else if self.eat_op("||") {
                let rhs = self.primary()?;
                e = Expr::Concat(Box::new(e), Box::new(rhs));
            } else {
                return Ok(e);
            }
        }
    }

    fn name_token(&mut self) -> PResult<String> {
        match self.peek().clone() {
            Tok::Name(n) | Tok::QName(n) => {
                self.pos += 1;
                Ok(n)
            }
            _ => Err(self.unexpected()),
        }
    }

    fn inner<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        let saved = self.angle;
        self.angle = 0;
        let r = f(self);
        self.angle = saved;
        r
    }

    fn args_until(&mut self, close: &str) -> PResult<Vec<Expr>> {
        if self.is_op(close) {
            return Ok(vec![]);
        }
        // `assuming` binds looser than `,`: f(a, b assuming c, d) passes (a, b) assuming (c, d)
        Ok(self.expr_full()?.into_seq())
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.peek().clone();
        match t {
            Tok::Int(s) => {
                self.pos += 1;
                Ok(Expr::Num(Num::Int(s.parse().map_err(|_| self.unexpected())?)))
            }
            Tok::Float(s) => {
                self.pos += 1;
                Ok(Expr::Num(Num::parse_float(&s).ok_or_else(|| self.unexpected())?))
            }
            Tok::Str(s) => {
                self.pos += 1;
                Ok(Expr::Str(s))
            }
            Tok::QName(s) => {
                self.pos += 1;
                Ok(Expr::Sym(s))
            }
            Tok::Quote => {
                self.pos += 1;
                let e = self.inner(|p| p.expr_seq())?;
                if !matches!(self.peek(), Tok::Quote) {
                    return Err(self.unexpected());
                }
                self.pos += 1;
                Ok(Expr::Uneval(Box::new(e)))
            }
            Tok::Name(n) => {
                if n == "proc" {
                    self.pos += 1;
                    return self.proc_def();
                }
                if is_keyword(&n) {
                    return Err(self.unexpected());
                }
                self.pos += 1;
                Ok(match n.as_str() {
                    "true" => Expr::Bool(true),
                    "false" => Expr::Bool(false),
                    "FAIL" => Expr::Fail,
                    "NULL" => Expr::null(),
                    c if crate::ast::CONSTANTS.contains(&c) => Expr::Const(c.to_string()),
                    _ => Expr::Sym(n),
                })
            }
            Tok::Op("(") => {
                self.pos += 1;
                let e = self.inner(|p| if p.is_op(")") { Ok(Expr::null()) } else { p.expr_full() })?;
                self.expect_op(")")?;
                Ok(e)
            }
            Tok::Op("[") => {
                self.pos += 1;
                let v = self.inner(|p| p.args_until("]"))?;
                self.expect_op("]")?;
                Ok(Expr::List(v))
            }
            Tok::Op("{") => {
                self.pos += 1;
                let v = self.inner(|p| p.args_until("}"))?;
                self.expect_op("}")?;
                Ok(Expr::Set(v))
            }
            Tok::Op("<") => {
                self.pos += 1;
                self.angle_ctor()
            }
            Tok::Op("<>") => {
                self.pos += 1;
                Ok(Expr::AngleCol(vec![]))
            }
            Tok::Op("%") => {
                self.pos += 1;
                Ok(Expr::Hist(1))
            }
            Tok::Op("%%") => {
                self.pos += 1;
                Ok(Expr::Hist(2))
            }
            Tok::Op("%%%") => {
                self.pos += 1;
                Ok(Expr::Hist(3))
            }
            Tok::Op("-") => {
                self.pos += 1;
                Ok(Expr::raw_neg(self.power()?))
            }
            Tok::Op(":-") => {
                self.pos += 1;
                Ok(Expr::Sym(self.name_token()?))
            }
            _ => Err(self.unexpected()),
        }
    }

    fn angle_ctor(&mut self) -> PResult<Expr> {
        let saved = self.angle;
        self.angle = 1;
        let mut items = Vec::new();
        let mut sep: Option<&str> = None;
        if !self.is_op(">") {
            loop {
                items.push(self.expr()?);
                let s = if self.is_op(",") {
                    ","
                } else if self.is_op("|") {
                    "|"
                } else {
                    break;
                };
                if sep.is_some() && sep != Some(s) {
                    self.angle = saved;
                    return Err(ParseError {
                        msg: "cannot mix `,` and `|` in an angle-bracket constructor".into(),
                        incomplete: false,
                    });
                }
                sep = Some(s);
                self.pos += 1;
            }
        }
        self.angle = saved;
        self.expect_op(">")?;
        Ok(if sep == Some("|") { Expr::AngleRow(items) } else { Expr::AngleCol(items) })
    }

    fn proc_def(&mut self) -> PResult<Expr> {
        self.expect_op("(")?;
        let mut params = Vec::new();
        if !self.is_op(")") {
            loop {
                if self.eat_op("$") {
                    break;
                }
                let name = self.name_token()?;
                let ty = if self.eat_op("::") { Some(self.inner(|p| p.range())?) } else { None };
                let default = if self.eat_op(":=") { Some(self.inner(|p| p.expr())?) } else { None };
                params.push(Param { name, ty, default });
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        self.expect_op(")")?;
        // optional return type
        if self.eat_op("::") {
            self.range()?;
            self.eat_op(";");
        }
        let mut locals = Vec::new();
        let mut globals = Vec::new();
        let mut options = Vec::new();
        let mut init = Vec::new();
        loop {
            if self.eat_kw("local") || self.eat_kw("global") {
                let is_local = matches!(&self.toks[self.pos - 1].tok, Tok::Name(n) if n == "local");
                loop {
                    let n = self.name_token()?;
                    if self.eat_op("::") {
                        self.range()?;
                    }
                    if self.eat_op(":=") {
                        let v = self.expr()?;
                        init.push(Stmt::Assign(vec![Expr::Sym(n.clone())], v));
                    }
                    if is_local {
                        locals.push(n);
                    } else {
                        globals.push(n);
                    }
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.end_decl()?;
            } else if self.eat_kw("option") || self.eat_kw("options") {
                loop {
                    let e = self.expr()?;
                    options.push(match e {
                        Expr::Sym(n) => n,
                        other => format!("{:?}", other),
                    });
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.end_decl()?;
            } else if self.eat_kw("description") {
                self.expr_seq()?;
                self.end_decl()?;
            } else {
                break;
            }
        }
        let mut body = init;
        body.extend(self.block()?);
        self.end_block("end", "proc")?;
        Ok(Expr::Proc(Rc::new(Proc { params, locals, globals, options, body, arrow: false })))
    }

    fn end_decl(&mut self) -> PResult<()> {
        if self.eat_op(";") || self.eat_op(":") || self.at_block_end() {
            Ok(())
        } else {
            Err(self.unexpected())
        }
    }
}

fn push_add(lhs: Expr, rhs: Expr) -> Expr {
    match lhs {
        Expr::Add(mut v) => {
            v.push(rhs);
            Expr::Add(v)
        }
        l => Expr::Add(vec![l, rhs]),
    }
}

fn push_mul(lhs: Expr, rhs: Expr) -> Expr {
    match lhs {
        Expr::Mul(mut v) if !matches!(v.first(), Some(Expr::Num(n)) if n.is_minus_one() && v.len() == 2) => {
            v.push(rhs);
            Expr::Mul(v)
        }
        l => Expr::Mul(vec![l, rhs]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::printer::{print, stmt_text};

    fn pe(s: &str) -> String {
        print(&parse_expr(s).unwrap_or_else(|e| panic!("{}: {}", s, e.msg)))
    }

    /// parse -> print gives `want`, and printing is a fixed point.
    fn rt(src: &str, want: &str) {
        let got = pe(src);
        assert_eq!(got, want, "printing {}", src);
        assert_eq!(pe(&got), want, "re-parsing {}", got);
    }

    #[test]
    fn arithmetic_precedence() {
        let cases = [
            ("x+y*z", "x+y*z"),
            ("(x+y)*z", "(x+y)*z"),
            ("x-y", "x-y"),
            ("x-(y-z)", "x-(y-z)"),
            ("x - y + z", "x-y+z"),
            ("x/y", "x/y"),
            ("x/(y*z)", "x/(y*z)"),
            ("a/b/c", "a/(b*c)"),
            ("(a+b)/(c-d)", "(a+b)/(c-d)"),
            ("-x^2", "-x^2"),
            ("(-x)^2", "(-x)^2"),
            ("-2^2", "-2^2"),
            ("x^(-1)", "1/x"),
            ("x^-2", "1/x^2"),
            ("x^(1/2)", "x^(1/2)"),
            ("x^(y+1)", "x^(y+1)"),
            ("(x^y)^z", "(x^y)^z"),
            ("x*(-y)", "x*(-y)"),
            ("2*-3", "2*(-3)"),
            ("-1/2*x", "-x/2"),
            ("2*x**3", "2*x^3"),
            ("x^2 + 2*x + 1", "x^2+2*x+1"),
            ("sin(x)^2+cos(x)^2", "sin(x)^2+cos(x)^2"),
            ("exp(-x^2/2)", "exp(-x^2/2)"),
            ("n!", "n!"),
            ("(n+1)!", "(n+1)!"),
            ("a mod b", "a mod b"),
            ("a+b mod c", "a+b mod c"),
            ("(a mod b)+c", "(a mod b)+c"),
            ("A.B", "A . B"),
            ("2.5e-3*x", "0.0025*x"),
            ("1.", "1."),
            ("123456789012345678901234567890", "123456789012345678901234567890"),
        ];
        for (s, w) in cases {
            rt(s, w);
        }
    }

    #[test]
    fn relations_and_logic() {
        let cases = [
            ("a = b", "a = b"),
            ("a <> b", "a <> b"),
            ("a < b", "a < b"),
            ("a > b", "b < a"),
            ("a >= b", "b <= a"),
            ("x^2-1 = 0", "x^2-1 = 0"),
            ("not a and b", "not a and b"),
            ("a or b and c", "a or b and c"),
            ("(a or b) and c", "(a or b) and c"),
            ("not (a and b)", "not (a and b)"),
            ("a xor b", "a xor b"),
            ("a implies b", "a implies b"),
            ("x < 1 and y > 2", "x < 1 and 2 < y"),
            ("x in S", "x in S"),
            ("x::integer", "x::integer"),
            ("A union B minus C", "A union B minus C"),
            ("A intersect B", "A intersect B"),
            ("A subset B", "A subset B"),
        ];
        for (s, w) in cases {
            rt(s, w);
        }
    }

    #[test]
    fn structures() {
        let cases = [
            ("[1, 2, 3]", "[1, 2, 3]"),
            ("[]", "[]"),
            ("{a,b}", "{a, b}"),
            ("f(x,y)", "f(x, y)"),
            ("f()", "f()"),
            ("a[1,2]", "a[1, 2]"),
            ("a[i][j]", "a[i][j]"),
            ("1..n", "1 .. n"),
            ("x = 0..1", "x = 0 .. 1"),
            ("x$3", "x $ 3"),
            ("f(i)$i=1..3", "f(i) $ i = 1 .. 3"),
            ("$1..4", "$ 1 .. 4"),
            ("NULL", "NULL"),
            ("a, b, c", "a, b, c"),
            ("'x'", "'x'"),
            ("''x''", "''x''"),
            ("%", "%"),
            ("%%", "%%"),
            ("%%%", "%%%"),
            ("`a b`", "`a b`"),
            ("`if`", "`if`"),
            ("\"hello\"", "\"hello\""),
            ("\"say \"\"hi\"\"\"", "\"say \\\"hi\\\"\""),
            ("x || 1", "x || 1"),
            ("true and FAIL", "true and FAIL"),
            ("Pi*I + infinity", "Pi*I+infinity"),
        ];
        for (s, w) in cases {
            rt(s, w);
        }
    }

    #[test]
    fn operators_and_procedures() {
        let cases = [
            ("x -> x^2", "x -> x^2"),
            ("(x, y) -> x*y", "(x, y) -> x*y"),
            ("x -> y -> x+y", "x -> y -> x+y"),
            ("map(x -> x^2, L)", "map(x -> x^2, L)"),
            ("f@g", "f@g"),
            ("f@g@h", "f@g@h"),
            ("D@@2", "D@@2"),
            ("(D@@2)(f)", "(D@@2)(f)"),
            ("(D@@2)(f)(0)", "(D@@2)(f)(0)"),
            ("D(f)(0)", "D(f)(0)"),
            ("D[1](f)", "D[1](f)"),
            ("D[1,2](f)(x,y)", "D[1, 2](f)(x, y)"),
            ("evalf[20](Pi)", "evalf[20](Pi)"),
            ("LinearAlgebra:-Determinant(A)", "LinearAlgebra:-Determinant(A)"),
            ("LinearAlgebra[Determinant](A)", "LinearAlgebra[Determinant](A)"),
            ("(f+g)(x)", "(f+g)(x)"),
            ("diff(y(x), x$2)", "diff(y(x), x $ 2)"),
            ("int(x^2, x = 0 .. 1)", "int(x^2, x = 0 .. 1)"),
            ("x assuming x > 0", "x assuming 0 < x"),
            ("proc(x) x^2 end proc", "proc(x) x^2 end proc"),
            ("proc(x::integer, y := 2) x+y end", "proc(x::integer, y := 2) x+y end proc"),
            (
                "proc(x) local y; y := x^2; y+1 end proc",
                "proc(x) local y; y := x^2; y+1 end proc",
            ),
            (
                "proc(n) option remember; if n < 2 then n else f(n-1)+f(n-2) end if end proc",
                "proc(n) option remember; if n < 2 then n else f(n-1)+f(n-2) end if end proc",
            ),
            (
                "proc(L) local s, x; s := 0; for x in L do s := s+x end do; s end proc",
                "proc(L) local s, x; s := 0; for x in L do s := s+x end do; s end proc",
            ),
            (
                "proc() global g; g := 1; return g end proc",
                "proc() global g; g := 1; return g end proc",
            ),
        ];
        for (s, w) in cases {
            rt(s, w);
        }
    }

    #[test]
    fn angle_brackets() {
        let cases = [
            ("<1,2,3>", "<1, 2, 3>"),
            ("<1|2|3>", "<1 | 2 | 3>"),
            ("<<1|2>,<3|4>>", "<<1 | 2>, <3 | 4>>"),
            ("<<1,3>|<2,4>>", "<<1, 3> | <2, 4>>"),
            ("<a+b, f(x)>", "<a+b, f(x)>"),
            ("<x < 1, (y > 2)>", "<x < 1, 2 < y>"),
        ];
        for (s, w) in cases {
            rt(s, w);
        }
    }

    #[test]
    fn statements() {
        let st = |s: &str| -> Vec<String> {
            parse_program(s).unwrap().iter().map(|t| stmt_text(&t.stmt)).collect()
        };
        assert_eq!(st("f := x -> x^2;"), vec!["f := x -> x^2"]);
        assert_eq!(st("a, b := 1, 2;"), vec!["a, b := 1, 2"]);
        assert_eq!(st("if x > 0 then 1 elif x < 0 then -1 else 0 fi;"), vec!["if 0 < x then 1 elif x < 0 then -1 else 0 end if"]);
        assert_eq!(st("for i from 1 to n do s := s+i od;"), vec!["for i from 1 to n do s := s+i end do"]);
        assert_eq!(st("for i to 10 by 2 while i < 5 do i end do"), vec!["for i by 2 to 10 while i < 5 do i end do"]);
        assert_eq!(st("while x < 10 do x := 2*x end do:"), vec!["while x < 10 do x := 2*x end do"]);
        assert_eq!(st("do break end do"), vec!["do break end do"]);
        assert_eq!(st("try f(x) catch \"bad\": 0 finally g() end try;"), vec!["try f(x) catch \"bad\": 0 finally g() end try"]);
        assert_eq!(st("error \"oops %1\", x;"), vec!["error \"oops %1\", x"]);
        let p = parse_program("a; b: c").unwrap();
        assert_eq!(p.iter().map(|t| t.show).collect::<Vec<_>>(), vec![true, false, true]);
        assert_eq!(st("x := 'x';"), vec!["x := 'x'"]);
        assert_eq!(st("# comment only\n x;"), vec!["x"]);
        assert_eq!(st("read \"lib/h.mpl\":"), vec!["read \"lib/h.mpl\""]);
        assert_eq!(st("KD := (a, b) -> if a = b then 1 else 0 end if:"), vec!["KD := proc(a, b) if a = b then 1 else 0 end if end proc"]);
        assert_eq!(st("Re~(L);"), vec!["`~`[Re](L)"]);
        assert_eq!(st("f(a, b assuming c > 0, d > 0);"), vec!["f((a, b assuming 0 < c, 0 < d))"]);
    }

    #[test]
    fn errors() {
        let e = parse_expr("a^b^c").unwrap_err();
        assert_eq!(e.msg, "ambiguous use of `^`, please use parentheses");
        assert!(!e.incomplete);
        assert!(parse_program("f := proc(x)").unwrap_err().incomplete);
        assert!(parse_program("if x then").unwrap_err().incomplete);
        assert!(parse_program("[1, 2").unwrap_err().incomplete);
        assert!(!parse_program("1 + * 2;").unwrap_err().incomplete);
        assert!(!parse_program("a = b = c;").unwrap_err().incomplete);
        assert!(!parse_program("x y;").unwrap_err().incomplete);
        assert!(parse_program("<1, 2 | 3>").is_err());
        assert!(parse_program("f(x) := 1; end;").is_err());
    }
}
