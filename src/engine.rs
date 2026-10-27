//   AlgebraKing //
//
//   File:         [engine.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['Client for the SymPy engine process (JSON lines) and EXPR tree conversion.']

use crate::ast::{Expr, Param, Proc, RelOp, Stmt};
use crate::number::Num;
use crate::simpl;
use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const ENGINE_SRC: &str = include_str!("../engine/engine.py");

// ------------------------------------------------------------------ EXPR conversion

fn float_json_text(m: &num_bigint::BigInt, e: i64) -> String {
    if (-60..=0).contains(&e) {
        let neg = m < &num_bigint::BigInt::from(0);
        let ds = if neg { (-m).to_string() } else { m.to_string() };
        let k = (-e) as usize;
        let ds = if ds.len() <= k { format!("{}{}", "0".repeat(k + 1 - ds.len()), ds) } else { ds };
        let (a, b) = ds.split_at(ds.len() - k);
        let body = if b.is_empty() { format!("{}.0", a) } else { format!("{}.{}", a, b) };
        if neg {
            format!("-{}", body)
        } else {
            body
        }
    } else {
        format!("{}e{}", m, e)
    }
}

/// Convert an expression to the engine's JSON tree.
pub fn to_json(e: &Expr, digits: u32) -> Result<Value, String> {
    let list = |v: &[Expr]| -> Result<Vec<Value>, String> { v.iter().map(|x| to_json(x, digits)).collect() };
    Ok(match e {
        Expr::Num(Num::Int(i)) => json!({"int": i.to_string()}),
        Expr::Num(Num::Rat(p, q)) => json!({"rat": [p.to_string(), q.to_string()]}),
        Expr::Num(Num::Float(m, x)) => json!({"float": float_json_text(m, *x), "digits": digits}),
        Expr::Sym(s) => json!({"sym": s}),
        Expr::Str(s) => json!({"str": s}),
        Expr::Bool(b) => json!({"bool": b}),
        Expr::Fail => json!({"fail": true}),
        Expr::Const(c) => json!({"const": c}),
        Expr::Add(v) => json!({"add": list(v)?}),
        Expr::Mul(v) => json!({"mul": list(v)?}),
        Expr::Pow(b, x) => json!({"pow": [to_json(b, digits)?, to_json(x, digits)?]}),
        Expr::Fn(n, a) => json!({"fn": n, "args": list(a)?}),
        Expr::Apply(h, a) => match h.as_ref() {
            Expr::Idx(b, idx) if matches!(b.as_ref(), Expr::Sym(_)) => {
                let Expr::Sym(n) = b.as_ref() else { unreachable!() };
                json!({"fnidx": [n, list(idx)?, list(a)?]})
            }
            Expr::Sym(n) => json!({"fn": n, "args": list(a)?}),
            Expr::Member(_, n) => json!({"fn": n, "args": list(a)?}),
            // D(f)(x) and similar: the head is itself an EXPR object
            other => json!({"fn": to_json(other, digits)?, "args": list(a)?}),
        },
        Expr::Idx(b, idx) => json!({"idx": [to_json(b, digits)?, list(idx)?]}),
        Expr::Rel(op, a, b) => {
            let k = match op {
                RelOp::Eq => "eq",
                RelOp::Neq => "neq",
                RelOp::Lt => "lt",
                RelOp::Le => "le",
            };
            json!({k: [to_json(a, digits)?, to_json(b, digits)?]})
        }
        Expr::And(v) => json!({"and": list(v)?}),
        Expr::Or(v) => json!({"or": list(v)?}),
        Expr::Not(a) => json!({"not": to_json(a, digits)?}),
        Expr::Xor(a, b) => json!({"fn": "xor", "args": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::Implies(a, b) => json!({"fn": "implies", "args": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::List(v) => json!({"list": list(v)?}),
        Expr::Set(v) => json!({"set": list(v)?}),
        Expr::Seq(v) => json!({"seq": list(v)?}),
        Expr::Range(a, b) => json!({"range": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::Matrix(rows) => {
            let r: Result<Vec<Value>, String> = rows.iter().map(|r| list(r).map(Value::Array)).collect();
            json!({"matrix": r?})
        }
        Expr::Vector(v, row) => json!({"vector": list(v)?, "orient": if *row {"row"} else {"column"}}),
        Expr::Proc(p) => proc_json(p, digits)?,
        Expr::TypeAssert(a, b) => json!({"fn": "type", "args": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::In(a, b) => json!({"fn": "in", "args": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::Uneval(a) => to_json(a, digits)?,
        Expr::Member(p, n) => json!({"sym": format!("{}:-{}", p, n)}),
        Expr::Compose(v) => json!({"fn": "@", "args": list(v)?}),
        Expr::Repeat(a, b) => json!({"fn": "@@", "args": [to_json(a, digits)?, to_json(b, digits)?]}),
        Expr::Table(_) => return Err("cannot pass a table to the math engine".into()),
        other => return Err(format!("cannot pass `{}` to the math engine", crate::printer::print(other))),
    })
}

fn proc_json(p: &Proc, digits: u32) -> Result<Value, String> {
    match p.body.as_slice() {
        [Stmt::Expr(body)] if p.locals.is_empty() => {
            let params: Vec<&str> = p.params.iter().map(|x| x.name.as_str()).collect();
            Ok(json!({"proc": {"params": params, "body": to_json(body, digits)?}}))
        }
        _ => Err("cannot pass a multi-statement procedure to the math engine".into()),
    }
}

/// Integer, or integer base with a positive integer exponent.
fn is_int_power(e: &Expr) -> bool {
    match e {
        Expr::Num(Num::Int(_)) => true,
        Expr::Pow(b, x) => {
            matches!(b.as_ref(), Expr::Num(Num::Int(_)))
                && matches!(x.as_ref(), Expr::Num(Num::Int(k)) if *k > num_bigint::BigInt::from(0))
        }
        _ => false,
    }
}

fn bad(v: &Value) -> String {
    format!("engine returned an invalid expression: {}", v)
}

fn big(v: &Value) -> Result<num_bigint::BigInt, String> {
    v.as_str().and_then(|s| s.trim().parse().ok()).ok_or_else(|| bad(v))
}

/// Convert an engine JSON tree back to an expression (with automatic simplification).
pub fn from_json(v: &Value, digits: u32) -> Result<Expr, String> {
    let obj: &Map<String, Value> = v.as_object().ok_or_else(|| bad(v))?;
    let arr = |k: &str| -> Result<&Vec<Value>, String> { obj.get(k).and_then(|x| x.as_array()).ok_or_else(|| bad(v)) };
    let items = |k: &str| -> Result<Vec<Expr>, String> { arr(k)?.iter().map(|x| from_json(x, digits)).collect() };
    let two = |k: &str| -> Result<(Expr, Expr), String> {
        let a = arr(k)?;
        if a.len() != 2 {
            return Err(bad(v));
        }
        Ok((from_json(&a[0], digits)?, from_json(&a[1], digits)?))
    };
    let key = obj
        .keys()
        .find(|k| *k != "digits" && *k != "orient" && *k != "args")
        .cloned()
        .ok_or_else(|| bad(v))?;
    Ok(match key.as_str() {
        "int" => Expr::Num(Num::Int(big(&obj["int"])?)),
        "rat" => {
            let a = arr("rat")?;
            if a.len() != 2 {
                return Err(bad(v));
            }
            Expr::Num(Num::rat(big(&a[0])?, big(&a[1])?)?)
        }
        "float" => {
            let s = obj["float"].as_str().ok_or_else(|| bad(v))?;
            match s {
                "inf" | "+inf" | "Infinity" => Expr::Const("infinity".into()),
                "-inf" | "-Infinity" => Expr::Mul(vec![Expr::int(-1), Expr::Const("infinity".into())]),
                "nan" | "NaN" => Expr::Const("undefined".into()),
                _ => {
                    let n = Num::parse_float(s).ok_or_else(|| bad(v))?;
                    let d = obj.get("digits").and_then(|x| x.as_u64()).map(|x| x as u32).unwrap_or(digits);
                    match n {
                        Num::Float(m, e) => Expr::Num(Num::float(m, e, d.max(1))),
                        other => Expr::Num(other),
                    }
                }
            }
        }
        "sym" => Expr::Sym(obj["sym"].as_str().ok_or_else(|| bad(v))?.to_string()),
        "str" => Expr::Str(obj["str"].as_str().ok_or_else(|| bad(v))?.to_string()),
        "bool" => Expr::Bool(obj["bool"].as_bool().ok_or_else(|| bad(v))?),
        "fail" => Expr::Fail,
        "const" => {
            let c = obj["const"].as_str().ok_or_else(|| bad(v))?;
            match c {
                "E" | "e" => Expr::func("exp", vec![Expr::int(1)]),
                _ => Expr::Const(c.to_string()),
            }
        }
        "add" => simpl::add(items("add")?, digits)?,
        "mul" => {
            let v = items("mul")?;
            // an integer factorization (ifactor) is kept unevaluated, like Maple's ``(p)^k
            if v.len() > 1 && v.iter().all(is_int_power) {
                Expr::Mul(v)
            } else {
                simpl::mul(v, digits)?
            }
        }
        "pow" => {
            let (b, x) = two("pow")?;
            let raw = Expr::Pow(Box::new(b.clone()), Box::new(x.clone()));
            if is_int_power(&raw) && !matches!(&x, Expr::Num(Num::Int(k)) if *k == 1.into()) {
                raw
            } else {
                simpl::pow(b, x, digits)?
            }
        }
        "fn" => {
            let args = match obj.get("args") {
                Some(Value::Array(a)) => a.iter().map(|x| from_json(x, digits)).collect::<Result<Vec<_>, _>>()?,
                _ => vec![],
            };
            match &obj["fn"] {
                Value::String(name) => Expr::Fn(name.clone(), args),
                head @ Value::Object(_) => Expr::Apply(Box::new(from_json(head, digits)?), args),
                _ => return Err(bad(v)),
            }
        }
        "fnidx" => {
            let a = arr("fnidx")?;
            if a.len() != 3 {
                return Err(bad(v));
            }
            let name = a[0].as_str().ok_or_else(|| bad(v))?;
            let idx = a[1].as_array().ok_or_else(|| bad(v))?;
            let args = a[2].as_array().ok_or_else(|| bad(v))?;
            let idx: Vec<Expr> = idx.iter().map(|x| from_json(x, digits)).collect::<Result<_, _>>()?;
            let args: Vec<Expr> = args.iter().map(|x| from_json(x, digits)).collect::<Result<_, _>>()?;
            Expr::Apply(Box::new(Expr::Idx(Box::new(Expr::sym(name)), idx)), args)
        }
        "idx" => {
            let a = arr("idx")?;
            if a.len() != 2 {
                return Err(bad(v));
            }
            let base = from_json(&a[0], digits)?;
            let idx: Vec<Expr> =
                a[1].as_array().ok_or_else(|| bad(v))?.iter().map(|x| from_json(x, digits)).collect::<Result<_, _>>()?;
            Expr::Idx(Box::new(base), idx)
        }
        "eq" | "neq" | "lt" | "le" => {
            let (a, b) = two(&key)?;
            let op = match key.as_str() {
                "eq" => RelOp::Eq,
                "neq" => RelOp::Neq,
                "lt" => RelOp::Lt,
                _ => RelOp::Le,
            };
            Expr::rel(op, a, b)
        }
        "and" => Expr::And(items("and")?),
        "or" => Expr::Or(items("or")?),
        "not" => Expr::Not(Box::new(from_json(&obj["not"], digits)?)),
        "list" => Expr::List(items("list")?.into_iter().flat_map(|x| x.into_seq()).collect()),
        "set" => simpl::make_set(items("set")?),
        "seq" => Expr::from_seq(items("seq")?.into_iter().flat_map(|x| x.into_seq()).collect()),
        "range" => {
            let (a, b) = two("range")?;
            Expr::Range(Box::new(a), Box::new(b))
        }
        "matrix" => {
            let rows = arr("matrix")?;
            let mut out = Vec::new();
            for r in rows {
                let r = r.as_array().ok_or_else(|| bad(v))?;
                out.push(r.iter().map(|x| from_json(x, digits)).collect::<Result<Vec<_>, _>>()?);
            }
            Expr::Matrix(out)
        }
        "vector" => {
            let row = obj.get("orient").and_then(|x| x.as_str()) == Some("row");
            Expr::Vector(items("vector")?, row)
        }
        "proc" => {
            let p = obj["proc"].as_object().ok_or_else(|| bad(v))?;
            let params = p
                .get("params")
                .and_then(|x| x.as_array())
                .ok_or_else(|| bad(v))?
                .iter()
                .map(|x| x.as_str().map(|s| Param { name: s.to_string(), ty: None, default: None }))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| bad(v))?;
            let body = from_json(p.get("body").ok_or_else(|| bad(v))?, digits)?;
            Expr::Proc(Rc::new(Proc {
                params,
                locals: vec![],
                globals: vec![],
                options: vec!["operator".into(), "arrow".into()],
                body: vec![Stmt::Expr(body)],
                arrow: true,
            }))
        }
        _ => return Err(bad(v)),
    })
}

// ------------------------------------------------------------------ process

fn fnv64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn engine_path() -> Result<PathBuf, String> {
    let base = match std::env::var_os("XDG_CACHE_HOME") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => {
            let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
            PathBuf::from(home).join(".cache")
        }
    };
    let dir = base.join("algebraking");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {}", dir.display(), e))?;
    let path = dir.join(format!("engine-{:016x}.py", fnv64(ENGINE_SRC)));
    if !path.exists() {
        let tmp = dir.join(format!("engine-{}.tmp", std::process::id()));
        std::fs::write(&tmp, ENGINE_SRC).map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
        std::fs::rename(&tmp, &path).map_err(|e| format!("cannot write {}: {}", path.display(), e))?;
        // drop copies of older engine versions
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let name = ent.file_name().to_string_lossy().to_string();
                if name.starts_with("engine-") && name.ends_with(".py") && ent.path() != path {
                    let _ = std::fs::remove_file(ent.path());
                }
            }
        }
    }
    Ok(path)
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<String>,
}

pub struct Engine {
    proc: Option<Running>,
    cache: std::collections::HashMap<String, Value>,
    next_id: u64,
    pub timeout: Duration,
    stderr_tail: Arc<Mutex<VecDeque<String>>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Engine {
        let secs = std::env::var("ALGEBRAKING_TIMEOUT").ok().and_then(|s| s.parse::<f64>().ok()).unwrap_or(60.0);
        Engine {
            proc: None,
            cache: std::collections::HashMap::new(),
            next_id: 1,
            timeout: Duration::from_secs_f64(secs.max(0.1)),
            stderr_tail: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    fn start(&mut self) -> Result<(), String> {
        let path = engine_path()?;
        let python = std::env::var("ALGEBRAKING_PYTHON").unwrap_or_else(|_| "python3".into());
        let mut child = Command::new(&python)
            .arg("-u")
            .arg(&path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("cannot start the math engine ({}): {}", python, e))?;
        let stdin = child.stdin.take().ok_or("engine stdin unavailable")?;
        let stdout = child.stdout.take().ok_or("engine stdout unavailable")?;
        let stderr = child.stderr.take().ok_or("engine stderr unavailable")?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(l) => {
                        if tx.send(l).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        let tail = self.stderr_tail.clone();
        let debug = std::env::var_os("ALGEBRAKING_DEBUG").is_some();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if debug {
                    eprintln!("[engine] {}", line);
                }
                let mut t = tail.lock().unwrap();
                t.push_back(line);
                if t.len() > 20 {
                    t.pop_front();
                }
            }
        });
        self.proc = Some(Running { child, stdin, rx });
        Ok(())
    }

    pub fn kill(&mut self) {
        if let Some(mut r) = self.proc.take() {
            let _ = r.child.kill();
            let _ = r.child.wait();
        }
    }

    fn stderr_text(&self) -> String {
        std::thread::sleep(Duration::from_millis(50));
        let t = self.stderr_tail.lock().unwrap();
        let lines: Vec<&String> = t.iter().rev().take(3).collect();
        lines.into_iter().rev().cloned().collect::<Vec<_>>().join(" | ")
    }

    /// Send one request (an object without "id") and return its "result".
    /// Errors are Maple-style messages without the leading "Error, ".
    /// The engine is stateless, so successful answers are memoized by request text.
    pub fn request(&mut self, req: Value, ctx: &str) -> Result<Value, String> {
        let key = req.to_string();
        if let Some(v) = self.cache.get(&key) {
            return Ok(v.clone());
        }
        let r = self.request_uncached(req, ctx)?;
        if self.cache.len() >= 20_000 {
            self.cache.clear();
        }
        self.cache.insert(key, r.clone());
        Ok(r)
    }

    fn request_uncached(&mut self, mut req: Value, ctx: &str) -> Result<Value, String> {
        for attempt in 0..2 {
            if self.proc.is_none() {
                self.start()?;
            }
            let id = self.next_id;
            self.next_id += 1;
            req["id"] = json!(id);
            let line = format!("{}\n", req);
            let r = self.proc.as_mut().unwrap();
            if r.stdin.write_all(line.as_bytes()).and_then(|_| r.stdin.flush()).is_err() {
                let why = self.stderr_text();
                self.kill();
                if attempt == 0 {
                    continue;
                }
                return Err(format!("(in {}) the math engine is not running: {}", ctx, why));
            }
            loop {
                let rx = &self.proc.as_ref().unwrap().rx;
                match rx.recv_timeout(self.timeout) {
                    Ok(text) => {
                        let Ok(resp) = serde_json::from_str::<Value>(&text) else { continue };
                        if resp.get("id").and_then(|x| x.as_u64()) != Some(id) {
                            continue;
                        }
                        if resp.get("ok").and_then(|x| x.as_bool()) == Some(true) {
                            return Ok(resp.get("result").cloned().unwrap_or(Value::Null));
                        }
                        let msg = resp.get("error").and_then(|x| x.as_str()).unwrap_or("unknown engine error");
                        return Err(msg.to_string());
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        self.kill();
                        return Err(format!(
                            "(in {}) computation timed out after {} s",
                            ctx,
                            fmt_secs(self.timeout.as_secs_f64())
                        ));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let why = self.stderr_text();
                        self.kill();
                        return Err(format!("(in {}) the math engine stopped: {}", ctx, why));
                    }
                }
            }
        }
        Err(format!("(in {}) the math engine is not running", ctx))
    }
}

fn fmt_secs(s: f64) -> String {
    if s.fract() == 0.0 {
        format!("{}", s as u64)
    } else {
        format!("{}", s)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_expr;

    #[test]
    fn json_round_trip() {
        for src in ["x^2+2*x+1", "sin(x)/y", "[1, 2.5, \"s\"]", "{a, b}", "a[1, 2]", "x = 1 .. 2", "f -> f"] {
            let e = crate::simpl::resimplify(&parse_expr(src).unwrap(), 10).unwrap();
            let Ok(j) = to_json(&e, 10) else { continue };
            let back = from_json(&j, 10).unwrap();
            assert_eq!(crate::printer::print(&back), crate::printer::print(&e), "{}", src);
        }
        assert_eq!(float_json_text(&250.into(), -2), "2.50");
        assert_eq!(float_json_text(&5.into(), -3), "0.005");
        assert_eq!(float_json_text(&2.into(), 0), "2.0");
    }
}
