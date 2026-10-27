//   AlgebraKing //
//
//   File:         [mcp.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['MCP server (JSON-RPC 2.0 over stdio) exposing evaluation and verification tools.']

use crate::ast::{Expr, RelOp};
use crate::engine::Engine;
use crate::interp::{Exc, Interp, Out, R};
use crate::parser::parse_expr;
use crate::printer::print;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::rc::Rc;

const PROTOCOL: &str = "2025-06-18";

const SYNTAX_NOTE: &str = "Use Maple syntax (e.g. x^2, sin(x), exp(x), ln(x), sqrt(x), Pi, I, \
diff(f, x), int(f, x), y(x) for unknown functions, assume via 'x>0'). Send the user's step exactly as \
written; do not simplify or correct it before checking.";

pub struct Server {
    engine: Rc<RefCell<Engine>>,
    sessions: HashMap<String, Interp>,
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn tool_list() -> Value {
    let s = |props: Value, req: &[&str]| json!({"type": "object", "properties": props, "required": req});
    let session = json!({"type": "string", "description": "Named session (default \"main\"); variables, Digits and assumptions persist per session."});
    json!([
        {
            "name": "algebra_eval",
            "title": "Evaluate Maple code",
            "description": format!("Run Maple-language statements (separated by ; or :) in a persistent session and return every displayed result in Maple 1-D notation, plus the LaTeX of the last result. Supports assignments, procedures, loops, with(LinearAlgebra), Digits, assume, etc. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({"code": {"type": "string", "description": "Maple statements, e.g. \"f := x^2*sin(x); diff(f, x);\""}, "session": session}), &["code"])
        },
        {
            "name": "verify_equal",
            "title": "Check that two expressions are equal",
            "description": format!("Decide whether lhs = rhs: returns verdict equal / not_equal / undetermined, the method (symbolic or numeric), the simplified difference and a counterexample point when not equal. Use it to check one step of a derivation. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "lhs": {"type": "string", "description": "Left-hand side in Maple syntax"},
                "rhs": {"type": "string", "description": "Right-hand side in Maple syntax"},
                "assumptions": {"type": "string", "description": "Optional, comma separated, e.g. \"x>0, n::integer\""},
                "session": session
            }), &["lhs", "rhs"])
        },
        {
            "name": "check_derivative",
            "title": "Check a derivative",
            "description": format!("Verify that diff(f, var) equals the claimed derivative; also returns the computed derivative. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "f": {"type": "string", "description": "Function to differentiate"},
                "derivative": {"type": "string", "description": "Claimed derivative"},
                "var": {"type": "string", "description": "Variable, e.g. x"},
                "assumptions": {"type": "string"},
                "session": session
            }), &["f", "derivative", "var"])
        },
        {
            "name": "check_integral",
            "title": "Check an antiderivative",
            "description": format!("Verify that diff(antiderivative, var) equals the integrand, i.e. that the antiderivative is correct up to an additive constant. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "integrand": {"type": "string"},
                "antiderivative": {"type": "string"},
                "var": {"type": "string"},
                "assumptions": {"type": "string"},
                "session": session
            }), &["integrand", "antiderivative", "var"])
        },
        {
            "name": "check_solution",
            "title": "Check a solution of equations",
            "description": format!("Substitute the claimed solution into each equation and verify every equation holds. equations: one equation or a set/list such as {{x+y=3, x-y=1}}; an expression e means e = 0. solution: x=2 or {{x=2, y=1}}. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "equations": {"type": "string"},
                "solution": {"type": "string"},
                "assumptions": {"type": "string"},
                "session": session
            }), &["equations", "solution"])
        },
        {
            "name": "check_ode_solution",
            "title": "Check a solution of an ODE",
            "description": format!("Like Maple odetest: substitute the solution func = expr into the ODE (and any initial conditions given in a set with it, e.g. {{diff(y(x),x)=y(x), y(0)=1}}) and verify the residuals vanish. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "ode": {"type": "string", "description": "ODE, e.g. diff(y(x), x, x) + y(x) = 0, or a set with initial conditions"},
                "solution": {"type": "string", "description": "e.g. y(x) = sin(x)"},
                "func": {"type": "string", "description": "Unknown function, e.g. y(x)"},
                "assumptions": {"type": "string"},
                "session": session
            }), &["ode", "solution", "func"])
        },
        {
            "name": "algebra_reset",
            "title": "Reset a session",
            "description": "Maple restart: clear variables, assumptions, packages and Digits of a session.",
            "inputSchema": s(json!({"session": session}), &[])
        }
    ])
}

fn arg_str(args: &Value, k: &str) -> Option<String> {
    args.get(k).and_then(|v| v.as_str()).map(|s| s.to_string())
}

fn need(args: &Value, k: &str) -> R<String> {
    arg_str(args, k).ok_or_else(|| Exc::Err(format!("missing required argument `{}`", k)))
}

fn parse_eval(it: &mut Interp, what: &str, src: &str) -> R<Expr> {
    let e = parse_expr(src).map_err(|e| Exc::Err(format!("syntax error in {}: {}", what, e.msg)))?;
    it.output.clear();
    it.eval(&e)
}

fn verdict_text(res: &Value) -> (String, String) {
    let verdict = res.get("verdict").and_then(|v| v.as_str()).unwrap_or("undetermined").to_string();
    let method = res.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
    (verdict, method)
}

impl Server {
    fn session(&mut self, args: &Value) -> &mut Interp {
        let name = arg_str(args, "session").unwrap_or_else(|| "main".into());
        let eng = self.engine.clone();
        self.sessions.entry(name).or_insert_with(|| Interp::new(eng))
    }

    fn assumptions(it: &mut Interp, args: &Value) -> R<Vec<Expr>> {
        match arg_str(args, "assumptions") {
            Some(a) if !a.trim().is_empty() => Ok(parse_eval(it, "assumptions", &a)?.into_seq()),
            _ => Ok(vec![]),
        }
    }

    /// Verify lhs = rhs and describe the result in a few lines.
    fn verify_report(it: &mut Interp, l: &Expr, r: &Expr, extra: &[Expr]) -> R<(String, String)> {
        let res = it.verify(l, r, extra)?;
        let d = it.digits();
        let (verdict, method) = verdict_text(&res);
        let mut lines = vec![format!("verdict: {}", verdict)];
        if !method.is_empty() {
            let note = if method == "numeric" && verdict == "equal" {
                " (numerical check at random points, not a symbolic proof)"
            } else {
                ""
            };
            lines.push(format!("method: {}{}", method, note));
        }
        if let Some(diff) = res.get("difference") {
            if !diff.is_null() {
                if let Ok(e) = crate::engine::from_json(diff, d) {
                    lines.push(format!("difference lhs - rhs: {}", print(&e)));
                }
            }
        }
        if let Some(Value::Object(ce)) = res.get("counterexample") {
            if !ce.is_empty() {
                let pts: Vec<String> = ce
                    .iter()
                    .map(|(k, v)| format!("{} = {}", k, v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string())))
                    .collect();
                lines.push(format!("counterexample: {}", pts.join(", ")));
            }
        }
        for k in ["lhs_value", "rhs_value", "note", "detail"] {
            if let Some(v) = res.get(k).and_then(|v| v.as_str()) {
                lines.push(format!("{}: {}", k, v));
            }
        }
        Ok((verdict, lines.join("\n")))
    }

    fn tool(&mut self, name: &str, args: &Value) -> R<(String, bool)> {
        match name {
            "algebra_eval" => {
                let code = need(args, "code")?;
                let it = self.session(args);
                let (outs, _) = it.run(&code, false);
                let mut text = String::new();
                let mut last: Option<Expr> = None;
                let mut is_error = false;
                for o in &outs {
                    if let Out::Text(t) = o {
                        text.push_str(t);
                        continue;
                    }
                    if !text.is_empty() && !text.ends_with('\n') {
                        text.push('\n');
                    }
                    match o {
                        Out::Expr(e) => last = Some(e.clone()),
                        Out::Assign(_, e) => last = Some(e.clone()),
                        Out::Error(_) => is_error = true,
                        _ => {}
                    }
                    text.push_str(&it.render(o, false));
                    text.push('\n');
                }
                if let Some(e) = last {
                    if !matches!(e, Expr::Proc(_)) {
                        if let Ok(l) = it.latex(&e) {
                            text.push_str(&format!("LaTeX: {}\n", l));
                        }
                    }
                }
                if text.is_empty() {
                    text = "(no output)".into();
                }
                Ok((text.trim_end().to_string(), is_error))
            }
            "verify_equal" => {
                let (ls, rs) = (need(args, "lhs")?, need(args, "rhs")?);
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let l = parse_eval(it, "lhs", &ls)?;
                let r = parse_eval(it, "rhs", &rs)?;
                let (_, mut rep) = Self::verify_report(it, &l, &r, &extra)?;
                rep = format!("{}\nlhs: {}\nrhs: {}", rep, print(&l), print(&r));
                Ok((rep, false))
            }
            "check_derivative" => {
                let (fs, ds, vs) = (need(args, "f")?, need(args, "derivative")?, need(args, "var")?);
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let f = parse_eval(it, "f", &fs)?;
                let claimed = parse_eval(it, "derivative", &ds)?;
                let v = parse_eval(it, "var", &vs)?;
                let computed = it.engine_call("diff", vec![f, v.clone()])?;
                let (verdict, rep) = Self::verify_report(it, &computed, &claimed, &extra)?;
                let head = match verdict.as_str() {
                    "equal" => "The claimed derivative is correct.",
                    "not_equal" => "The claimed derivative is WRONG.",
                    _ => "Could not decide whether the claimed derivative is correct.",
                };
                Ok((format!("{}\ncomputed diff(f, {}): {}\n{}", head, print(&v), print(&computed), rep), false))
            }
            "check_integral" => {
                let (fs, aps, vs) = (need(args, "integrand")?, need(args, "antiderivative")?, need(args, "var")?);
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let f = parse_eval(it, "integrand", &fs)?;
                let big_f = parse_eval(it, "antiderivative", &aps)?;
                let v = parse_eval(it, "var", &vs)?;
                let dfv = it.engine_call("diff", vec![big_f, v.clone()])?;
                let (verdict, rep) = Self::verify_report(it, &dfv, &f, &extra)?;
                let head = match verdict.as_str() {
                    "equal" => "The antiderivative is correct (up to an additive constant).",
                    "not_equal" => "The antiderivative is WRONG: its derivative differs from the integrand.",
                    _ => "Could not decide whether the antiderivative is correct.",
                };
                Ok((format!("{}\ndiff(antiderivative, {}): {}\n{}", head, print(&v), print(&dfv), rep), false))
            }
            "check_solution" => {
                let (es, ss) = (need(args, "equations")?, need(args, "solution")?);
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let eqs = parse_eval(it, "equations", &es)?;
                let sol = parse_eval(it, "solution", &ss)?;
                let eq_list: Vec<Expr> = match eqs {
                    Expr::Set(v) | Expr::List(v) | Expr::Seq(v) => v,
                    other => vec![other],
                };
                let sol_set = match sol {
                    Expr::Set(v) | Expr::List(v) | Expr::Seq(v) => Expr::Set(v),
                    e @ Expr::Rel(RelOp::Eq, ..) => Expr::Set(vec![e]),
                    other => return Err(Exc::Err(format!("solution must be equations like x = 2, got {}", print(&other)))),
                };
                let mut lines = Vec::new();
                let (mut all_eq, mut any_ne) = (true, false);
                for (i, eq) in eq_list.iter().enumerate() {
                    let sub = it.call_values("eval", vec![eq.clone(), sol_set.clone()])?;
                    let (l, r) = match &sub {
                        Expr::Rel(RelOp::Eq, a, b) => ((**a).clone(), (**b).clone()),
                        other => (other.clone(), Expr::int(0)),
                    };
                    let (verdict, rep) = Self::verify_report(it, &l, &r, &extra)?;
                    if verdict != "equal" {
                        all_eq = false;
                    }
                    if verdict == "not_equal" {
                        any_ne = true;
                    }
                    lines.push(format!("equation {}: {}\n  after substitution: {}\n  {}", i + 1, print(eq), print(&sub), rep.replace('\n', "\n  ")));
                }
                let head = if all_eq {
                    "The solution satisfies every equation."
                } else if any_ne {
                    "The solution does NOT satisfy every equation."
                } else {
                    "Could not decide whether the solution satisfies every equation."
                };
                Ok((format!("{}\n{}", head, lines.join("\n")), false))
            }
            "check_ode_solution" => {
                let (os, ss, fs) = (need(args, "ode")?, need(args, "solution")?, need(args, "func")?);
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let func = parse_expr(&fs).map_err(|e| Exc::Err(format!("syntax error in func: {}", e.msg)))?;
                let (fname, var) = match &func {
                    Expr::Fn(n, a) if a.len() == 1 && matches!(a[0], Expr::Sym(_)) => {
                        let Expr::Sym(v) = &a[0] else { unreachable!() };
                        (n.clone(), v.clone())
                    }
                    _ => return Err(Exc::Err("func must look like y(x)".into())),
                };
                let sol = parse_eval(it, "solution", &ss)?;
                let body = match sol {
                    Expr::Rel(RelOp::Eq, l, r) => {
                        if *l == Expr::Fn(fname.clone(), vec![Expr::sym(&var)]) {
                            *r
                        } else {
                            return Err(Exc::Err("solution must be explicit: y(x) = expression".into()));
                        }
                    }
                    other => other,
                };
                let ode_e = parse_expr(&os).map_err(|e| Exc::Err(format!("syntax error in ode: {}", e.msg)))?;
                let saved = it.globals.get(&fname).cloned();
                let procv = it.call_values("unapply", vec![body.clone(), Expr::sym(&var)])?;
                it.globals.insert(fname.clone(), procv);
                // evaluate each condition separately so equal residuals are not merged by a set
                let conds: Vec<Expr> = match &ode_e {
                    Expr::Set(v) | Expr::List(v) | Expr::Seq(v) => v.clone(),
                    other => vec![other.clone()],
                };
                let mut substituted: R<Vec<(Expr, Expr)>> = Ok(Vec::new());
                for c in &conds {
                    match it.eval(c) {
                        Ok(v) => {
                            if let Ok(acc) = substituted.as_mut() {
                                acc.push((c.clone(), v));
                            }
                        }
                        Err(e) => {
                            substituted = Err(e);
                            break;
                        }
                    }
                }
                match saved {
                    Some(v) => {
                        it.globals.insert(fname.clone(), v);
                    }
                    None => {
                        it.globals.remove(&fname);
                    }
                }
                let parts = substituted?;
                let mut lines = Vec::new();
                let (mut all_eq, mut any_ne) = (true, false);
                for (i, (orig, p)) in parts.iter().enumerate() {
                    let (l, r) = match p {
                        Expr::Rel(RelOp::Eq, a, b) => ((**a).clone(), (**b).clone()),
                        other => (other.clone(), Expr::int(0)),
                    };
                    let (verdict, rep) = Self::verify_report(it, &l, &r, &extra)?;
                    if verdict != "equal" {
                        all_eq = false;
                    }
                    if verdict == "not_equal" {
                        any_ne = true;
                    }
                    lines.push(format!(
                        "condition {}: {}\n  after substitution: {}\n  {}",
                        i + 1,
                        print(orig),
                        print(p),
                        rep.replace('\n', "\n  ")
                    ));
                }
                let head = if all_eq {
                    "The solution satisfies the ODE (residual 0)."
                } else if any_ne {
                    "The solution does NOT satisfy the ODE."
                } else {
                    "Could not decide whether the solution satisfies the ODE."
                };
                Ok((format!("{}\n{}", head, lines.join("\n")), false))
            }
            "algebra_reset" => {
                let it = self.session(args);
                it.restart();
                Ok(("Session reset.".into(), false))
            }
            other => Err(Exc::Err(format!("unknown tool `{}`", other))),
        }
    }

    fn handle(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let id = id?; // notifications get no answer
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "algebraking", "title": "AlgebraKing", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Symbolic math with Maple syntax. Use verify_equal / check_* tools to check each step of the user's calculation exactly as the user wrote it; use algebra_eval to compute."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tool_list()})),
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                let known = tool_list().as_array().unwrap().iter().any(|t| t["name"] == name.as_str());
                if !known {
                    Err((-32602, format!("Unknown tool: {}", name)))
                } else {
                    match self.tool(&name, &args) {
                        Ok((text, is_err)) => Ok(text_result(text, is_err)),
                        Err(Exc::Err(m)) => Ok(text_result(format!("Error, {}", m), true)),
                        Err(Exc::Quit) => Ok(text_result("quit is not available here".into(), true)),
                    }
                }
            }
            _ => Err((-32601, format!("Method not found: {}", method))),
        };
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, m)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}),
        })
    }
}

pub fn run_server() -> i32 {
    let mut server = Server { engine: Rc::new(RefCell::new(Engine::new())), sessions: HashMap::new() };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) if msg.is_object() => server.handle(&msg),
            Ok(_) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32600, "message": "Invalid Request"}})),
            Err(e) => Some(json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("Parse error: {}", e)}})),
        };
        if let Some(r) = reply {
            let _ = writeln!(stdout, "{}", r);
            let _ = stdout.flush();
        }
    }
    0
}
