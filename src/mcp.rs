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

const RESULT_NOTE: &str = "Read the whole report, not only the verdict: `equal` means equal on the common domain, \
and `conditions` lists where that holds (x <> 1, abs(x) < 1, ...); `domain` says whether both sides have the same \
natural domain; a counterexample always lies inside the common domain; `suggestion` (if any) is an assumption under \
which equality was proved symbolically, found by a heuristic search, and does not change the verdict; `limitations` \
says what was not proved (numeric checks, undecided convergence or solution sets).";

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
    let field = json!({"type": "string", "enum": ["real", "complex"], "description": "Number field of the variables: real (default) or complex."});
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
            "description": format!("Decide whether lhs = rhs over the reals (default) or the complexes: verdict equal / not_equal / undetermined, the method (symbolic or numeric), the existence conditions of each side (denominators, ln, roots, tan poles, convergence of sum/product/int, which are kept inert), the domain comparison, the difference, a counterexample inside the common domain, and possibly a suggested assumption. Use it to check one step of a derivation. {} {}", SYNTAX_NOTE, RESULT_NOTE),
            "inputSchema": s(json!({
                "lhs": {"type": "string", "description": "Left-hand side in Maple syntax"},
                "rhs": {"type": "string", "description": "Right-hand side in Maple syntax"},
                "assumptions": {"type": "string", "description": "Optional, comma separated, e.g. \"x>0, n::integer\""},
                "field": field,
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
                "field": field,
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
                "field": field,
                "session": session
            }), &["integrand", "antiderivative", "var"])
        },
        {
            "name": "check_solution",
            "title": "Check a solution of equations",
            "description": format!("Check claimed solutions in the ORIGINAL equations: each candidate must lie in the domain of the equations (denominators, ln, roots over the reals) and satisfy every equation; rejected candidates are listed with the reason. For one equation in one unknown (or a polynomial system) it also says whether the accepted candidates are ALL the solutions, or that the solution set is an infinite family, or that completeness is not established. equations: one equation or a set/list such as {{x+y=3, x-y=1}}; an expression e means e = 0. solution: x=2, {{x=2, y=1}}, several candidates as [x=0, x=3] or [{{x=1,y=2}}, {{x=2,y=1}}]. {}", SYNTAX_NOTE),
            "inputSchema": s(json!({
                "equations": {"type": "string"},
                "solution": {"type": "string"},
                "assumptions": {"type": "string"},
                "field": field,
                "session": session
            }), &["equations", "solution"])
        },
        {
            "name": "check_step",
            "title": "Check one step of solving an equation",
            "description": format!("Compare two consecutive lines of a derivation. With two equations (e.g. before sqrt(x+1) = x-1, after x+1 = (x-1)^2) it computes both solution sets over the field with SymPy solveset (respecting the domain and assumptions) and classifies the step: equivalent / implies (after has extra, spurious solutions, e.g. squaring) / implied_by (solutions lost, e.g. dividing by an expression that can vanish) / not_equivalent / undetermined (solveset could not decide). Infinite families are reported as families (x = Pi*_Z1, _Z1 integer), never as finite lists. With two expressions it is a rewrite step and works like verify_equal with domains. {} {}", SYNTAX_NOTE, RESULT_NOTE),
            "inputSchema": s(json!({
                "before": {"type": "string", "description": "Earlier line: an equation (or an expression)"},
                "after": {"type": "string", "description": "Next line: an equation (or an expression)"},
                "var": {"type": "string", "description": "Unknown to solve for; optional when there is only one name"},
                "assumptions": {"type": "string", "description": "Optional, comma separated, e.g. \"x>0\""},
                "field": field,
                "session": session
            }), &["before", "after"])
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
                "field": field,
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

/// Parse and evaluate in verification mode (sum/product/int stay inert); also returns the parse tree
/// as EXPR when it holds no assigned names, so the engine can read conditions that evaluation
/// cancels (x/x -> 1, sqrt(x)^2 -> x), as Maple does.
fn parse_eval_check(it: &mut Interp, what: &str, src: &str) -> R<(Expr, Option<Value>)> {
    let e = parse_expr(src).map_err(|e| Exc::Err(format!("syntax error in {}: {}", what, e.msg)))?;
    let raw = raw_json(it, &e);
    it.output.clear();
    it.inert = true;
    let v = it.eval(&e);
    it.inert = false;
    Ok((v?, raw))
}

fn raw_json(it: &Interp, e: &Expr) -> Option<Value> {
    let mut pure = true;
    e.walk(&mut |x| match x {
        Expr::Sym(n) | Expr::Fn(n, _) if it.lookup_raw(n).is_some() => pure = false,
        Expr::Apply(h, _) if matches!(h.as_ref(), Expr::Sym(n) if it.lookup_raw(n).is_some()) => pure = false,
        Expr::Hist(_)
        | Expr::Dollar(..)
        | Expr::Assuming(..)
        | Expr::Proc(_)
        | Expr::Member(..)
        | Expr::Dot(..)
        | Expr::Concat(..)
        | Expr::Uneval(_)
        | Expr::Compose(_)
        | Expr::Repeat(..) => pure = false,
        _ => {}
    });
    if pure {
        crate::engine::to_json(e, it.digits()).ok()
    } else {
        None
    }
}

fn field_arg(args: &Value) -> R<String> {
    match arg_str(args, "field") {
        None => Ok("real".into()),
        Some(f) if f == "real" || f == "complex" => Ok(f),
        Some(f) => Err(Exc::Err(format!("field must be real or complex, got {}", f))),
    }
}

fn get_str<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str()).filter(|x| !x.is_empty())
}

fn str_list(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default()
}

fn point_text(p: &Value) -> String {
    match p {
        Value::Object(m) => m
            .iter()
            .map(|(k, v)| format!("{} = {}", k, v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string())))
            .collect::<Vec<_>>()
            .join(", "),
        other => other.to_string(),
    }
}

/// Text lines of a verification result (verdict first; every engine field that matters to the reader).
fn report_lines(res: &Value, d: u32, of_equations: bool) -> Vec<String> {
    let verdict = get_str(res, "verdict").unwrap_or("undetermined");
    let mut lines = vec![format!("verdict: {}", verdict)];
    if let Some(method) = get_str(res, "method") {
        let note = if method == "numeric" && verdict == "equal" {
            " (numerical check at random points, not a symbolic proof)"
        } else {
            ""
        };
        lines.push(format!("method: {}{}", method, note));
    }
    if let Some(f) = get_str(res, "field") {
        lines.push(format!("field: {}", f));
    }
    if res.get("conditions").is_some() {
        let c = str_list(res, "conditions");
        lines.push(if c.is_empty() {
            "conditions: none".into()
        } else {
            let why = if of_equations { "domain of the equations" } else { "the result holds only where these hold" };
            format!("conditions: {} ({})", c.join(", "), why)
        });
    }
    if let Some(dom) = res.get("domain").filter(|x| x.is_object()) {
        let same = match dom.get("same_domain") {
            Some(Value::Bool(true)) => "yes".to_string(),
            Some(Value::Bool(false)) => match get_str(dom, "differ_at") {
                Some(w) => format!("no, they differ where {}", w),
                None => "no".into(),
            },
            _ => "unknown".into(),
        };
        let part = |k: &str| get_str(dom, k).map(|t| format!("{}: {}", k, t));
        let parts: Vec<String> = ["lhs", "rhs", "common", "before", "after"].iter().filter_map(|k| part(k)).collect();
        if dom.get("same_domain").is_some() {
            lines.push(format!("domain: {}; same domain: {}", parts.join("; "), same));
        } else {
            lines.push(format!("domain: {}", parts.join("; ")));
        }
    }
    if let Some(diff) = res.get("difference").filter(|x| !x.is_null()) {
        if let Ok(e) = crate::engine::from_json(diff, d) {
            lines.push(format!("difference lhs - rhs: {}", print(&e)));
        }
    }
    if let Some(ce) = res.get("counterexample").filter(|x| x.as_object().is_some_and(|m| !m.is_empty())) {
        let mut l = format!("counterexample: {} (inside the common domain)", point_text(ce));
        if let (Some(a), Some(b)) = (get_str(res, "lhs_value"), get_str(res, "rhs_value")) {
            l.push_str(&format!("; lhs = {}, rhs = {}", a, b));
        }
        lines.push(l);
    }
    if let Some(w) = res.get("outside_domain").filter(|x| x.is_object()) {
        lines.push(format!(
            "outside the common real domain: at {} lhs = {}, rhs = {}",
            w.get("point").map(point_text).unwrap_or_default(),
            get_str(w, "lhs_value").unwrap_or("?"),
            get_str(w, "rhs_value").unwrap_or("?")
        ));
    }
    if let Some(sg) = get_str(res, "suggestion") {
        lines.push(format!("suggestion: {}", sg));
    }
    let notes = str_list(res, "notes");
    if notes.is_empty() {
        if let Some(n) = get_str(res, "note") {
            lines.push(format!("note: {}", n));
        }
    }
    for n in notes {
        lines.push(format!("note: {}", n));
    }
    if let Some(v) = get_str(res, "detail") {
        lines.push(format!("detail: {}", v));
    }
    for l in str_list(res, "limitations") {
        lines.push(format!("limitation: {}", l));
    }
    let a = str_list(res, "assumptions_used");
    if !a.is_empty() {
        lines.push(format!("assumptions used: {}", a.join(", ")));
    }
    lines
}

/// Candidate solutions: x=2 and {x=2, y=1} are one candidate; [x=0, x=3] (a name repeated) and
/// [{x=1}, {x=2}] are several.
fn candidates(sol: Expr) -> R<Vec<Vec<Expr>>> {
    let bad = |e: &Expr| Exc::Err(format!("solution must be equations like x = 2, got {}", print(e)));
    match sol {
        e @ Expr::Rel(RelOp::Eq, ..) => Ok(vec![vec![e]]),
        Expr::Set(v) | Expr::List(v) | Expr::Seq(v) => {
            if !v.is_empty() && v.iter().all(|x| matches!(x, Expr::Set(_) | Expr::List(_))) {
                return v
                    .into_iter()
                    .map(|c| match c {
                        Expr::Set(w) | Expr::List(w) if w.iter().all(|x| matches!(x, Expr::Rel(RelOp::Eq, ..))) => Ok(w),
                        other => Err(bad(&other)),
                    })
                    .collect();
            }
            if let Some(x) = v.iter().find(|x| !matches!(x, Expr::Rel(RelOp::Eq, ..))) {
                return Err(bad(x));
            }
            let names: Vec<&Expr> = v
                .iter()
                .filter_map(|x| match x {
                    Expr::Rel(_, l, _) => Some(l.as_ref()),
                    _ => None,
                })
                .collect();
            let repeated = names.iter().enumerate().any(|(i, n)| names[..i].contains(n));
            Ok(if repeated { v.into_iter().map(|e| vec![e]).collect() } else { vec![v] })
        }
        other => Err(bad(&other)),
    }
}

fn step_meaning(c: &str) -> &'static str {
    match c {
        "equivalent" => "both lines have the same solutions",
        "implies" => "every solution of before solves after, but after has extra (spurious) solutions",
        "implied_by" => "every solution of after solves before, but solutions of before are lost",
        "not_equivalent" => "the step both loses and adds solutions",
        _ => "could not decide",
    }
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

    /// Verify lhs = rhs over `field` and describe the result in a few lines.
    fn verify_report(
        it: &mut Interp,
        l: &Expr,
        r: &Expr,
        extra: &[Expr],
        field: &str,
        raws: (Option<Value>, Option<Value>),
    ) -> R<(String, String)> {
        let d = it.digits();
        let mut req = json!({
            "op": "verify",
            "lhs": crate::engine::to_json(l, d).map_err(Exc::Err)?,
            "rhs": crate::engine::to_json(r, d).map_err(Exc::Err)?
        });
        if let Some(x) = raws.0 {
            req["lhs_raw"] = x;
        }
        if let Some(x) = raws.1 {
            req["rhs_raw"] = x;
        }
        let res = it.check_request(req, extra, Some(field), "verify")?;
        let verdict = get_str(&res, "verdict").unwrap_or("undetermined").to_string();
        Ok((verdict, report_lines(&res, d, false).join("\n")))
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
                let field = field_arg(args)?;
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let (l, lraw) = parse_eval_check(it, "lhs", &ls)?;
                let (r, rraw) = parse_eval_check(it, "rhs", &rs)?;
                let (_, mut rep) = Self::verify_report(it, &l, &r, &extra, &field, (lraw, rraw))?;
                rep = format!("{}\nlhs: {}\nrhs: {}", rep, print(&l), print(&r));
                Ok((rep, false))
            }
            "check_derivative" => {
                let (fs, ds, vs) = (need(args, "f")?, need(args, "derivative")?, need(args, "var")?);
                let field = field_arg(args)?;
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let f = parse_eval(it, "f", &fs)?;
                let (claimed, craw) = parse_eval_check(it, "derivative", &ds)?;
                let v = parse_eval(it, "var", &vs)?;
                let computed = it.engine_call("diff", vec![f, v.clone()])?;
                let (verdict, rep) = Self::verify_report(it, &computed, &claimed, &extra, &field, (None, craw))?;
                let head = match verdict.as_str() {
                    "equal" => "The claimed derivative is correct.",
                    "not_equal" => "The claimed derivative is WRONG.",
                    _ => "Could not decide whether the claimed derivative is correct.",
                };
                Ok((format!("{}\ncomputed diff(f, {}): {}\n{}", head, print(&v), print(&computed), rep), false))
            }
            "check_integral" => {
                let (fs, aps, vs) = (need(args, "integrand")?, need(args, "antiderivative")?, need(args, "var")?);
                let field = field_arg(args)?;
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let (f, fraw) = parse_eval_check(it, "integrand", &fs)?;
                let big_f = parse_eval(it, "antiderivative", &aps)?;
                let v = parse_eval(it, "var", &vs)?;
                let dfv = it.engine_call("diff", vec![big_f, v.clone()])?;
                let (verdict, rep) = Self::verify_report(it, &dfv, &f, &extra, &field, (None, fraw))?;
                let head = match verdict.as_str() {
                    "equal" => "The antiderivative is correct (up to an additive constant).",
                    "not_equal" => "The antiderivative is WRONG: its derivative differs from the integrand.",
                    _ => "Could not decide whether the antiderivative is correct.",
                };
                Ok((format!("{}\ndiff(antiderivative, {}): {}\n{}", head, print(&v), print(&dfv), rep), false))
            }
            "check_solution" => {
                let (es, ss) = (need(args, "equations")?, need(args, "solution")?);
                let field = field_arg(args)?;
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let (eqs, eraw) = parse_eval_check(it, "equations", &es)?;
                let (sol, _) = parse_eval_check(it, "solution", &ss)?;
                let eq_list: Vec<Expr> = match eqs {
                    Expr::Set(v) | Expr::List(v) | Expr::Seq(v) => v,
                    other => vec![other],
                };
                let cands = candidates(sol)?;
                let d = it.digits();
                let js = |v: &[Expr]| -> R<Vec<Value>> {
                    v.iter().map(|x| crate::engine::to_json(x, d).map_err(Exc::Err)).collect()
                };
                let mut req = json!({
                    "op": "check_solution",
                    "equations": js(&eq_list)?,
                    "candidates": cands.iter().map(|c| js(c)).collect::<R<Vec<_>>>()?
                });
                // raw equations (before evaluation) only when they line up one to one
                let raw_list: Option<Vec<Value>> = match eraw {
                    Some(Value::Object(m)) => match m.get("set").or_else(|| m.get("list")).or_else(|| m.get("seq")) {
                        Some(Value::Array(a)) => Some(a.clone()),
                        _ => Some(vec![Value::Object(m)]),
                    },
                    _ => None,
                };
                if let Some(rl) = raw_list.filter(|rl| rl.len() == eq_list.len()) {
                    req["equations_raw"] = Value::Array(rl);
                }
                let res = it.check_request(req, &extra, Some(&field), "check_solution")?;
                let verdict = get_str(&res, "verdict").unwrap_or("undetermined");
                let one = cands.len() == 1;
                let rejected = str_list(&res, "rejected");
                let head = match (verdict, one) {
                    ("equal", true) => "The solution satisfies every equation.".to_string(),
                    ("equal", false) => "Every candidate satisfies every equation.".to_string(),
                    ("not_equal", true) => "The solution does NOT satisfy every equation (rejected).".to_string(),
                    ("not_equal", false) => format!("Some candidates are REJECTED: {}.", rejected.join("; ")),
                    _ => "Could not decide whether the solution satisfies every equation.".to_string(),
                };
                let mut lines = vec![head];
                for (i, eq) in eq_list.iter().enumerate() {
                    lines.push(format!("equation {}: {}", i + 1, print(eq)));
                }
                if let Some(cs) = res.get("candidates").and_then(|c| c.as_array()) {
                    for c in cs {
                        lines.push(format!(
                            "candidate {}: {}",
                            get_str(c, "point").unwrap_or("?"),
                            get_str(c, "status").unwrap_or("undetermined")
                        ));
                        for dl in str_list(c, "details") {
                            lines.push(format!("  {}", dl));
                        }
                    }
                }
                if let Some(a) = get_str(&res, "all_solutions") {
                    lines.push(format!("all solutions over the {}s: {}", field, a));
                }
                if let Some(c) = get_str(&res, "completeness") {
                    lines.push(format!("completeness: {}", c));
                }
                let mut rest = report_lines(&res, d, true);
                rest.retain(|l| !l.starts_with("verdict:"));
                lines.push(format!("verdict: {}", verdict));
                lines.extend(rest);
                Ok((lines.join("\n"), false))
            }
            "check_step" => {
                let (bs, as_) = (need(args, "before")?, need(args, "after")?);
                let field = field_arg(args)?;
                let it = self.session(args);
                let extra = Self::assumptions(it, args)?;
                let (b, braw) = parse_eval_check(it, "before", &bs)?;
                let (a, araw) = parse_eval_check(it, "after", &as_)?;
                let d = it.digits();
                let mut req = json!({
                    "op": "check_step",
                    "before": crate::engine::to_json(&b, d).map_err(Exc::Err)?,
                    "after": crate::engine::to_json(&a, d).map_err(Exc::Err)?
                });
                if let Some(vs) = arg_str(args, "var").filter(|v| !v.trim().is_empty()) {
                    let v = parse_expr(&vs).map_err(|e| Exc::Err(format!("syntax error in var: {}", e.msg)))?;
                    req["var"] = crate::engine::to_json(&v, d).map_err(Exc::Err)?;
                }
                if let Some(x) = braw {
                    req["before_raw"] = x;
                }
                if let Some(x) = araw {
                    req["after_raw"] = x;
                }
                let res = it.check_request(req, &extra, Some(&field), "check_step")?;
                let class = get_str(&res, "classification").unwrap_or("undetermined");
                let mut lines = vec![format!("classification: {} ({})", class, step_meaning(class))];
                if get_str(&res, "step") == Some("rewrite") {
                    lines.push("step: rewrite of an expression (checked like verify_equal)".into());
                }
                for (k, label) in [
                    ("solutions_before", "solutions before"),
                    ("solutions_after", "solutions after"),
                    ("spurious", "spurious solutions (added by the step)"),
                    ("lost", "lost solutions"),
                ] {
                    if let Some(v) = get_str(&res, k) {
                        lines.push(format!("{}: {}", label, v));
                    }
                }
                if res.get("family").and_then(|f| f.as_bool()) == Some(true) {
                    lines.push("note: infinite families are written with _Z1 standing for any integer".into());
                }
                let rewrite = get_str(&res, "step") == Some("rewrite");
                lines.extend(report_lines(&res, d, !rewrite));
                lines.push(format!("before: {}\nafter: {}", print(&b), print(&a)));
                Ok((lines.join("\n"), false))
            }
            "check_ode_solution" => {
                let (os, ss, fs) = (need(args, "ode")?, need(args, "solution")?, need(args, "func")?);
                let field = field_arg(args)?;
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
                    let (verdict, rep) = Self::verify_report(it, &l, &r, &extra, &field, (None, None))?;
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
