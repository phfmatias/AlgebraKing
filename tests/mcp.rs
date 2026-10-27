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
//   Description:  ['MCP server tests: handshake, tools/list and the verification tools over stdio.']

use serde_json::{json, Value};
use std::io::Write;
use std::process::{Command, Stdio};

fn have_engine() -> bool {
    let py = std::env::var("ALGEBRAKING_PYTHON").unwrap_or_else(|_| "python3".into());
    Command::new(py).args(["-c", "import sympy"]).output().map(|o| o.status.success()).unwrap_or(false)
}

/// Send all messages, close stdin, return the responses keyed by id.
fn session(msgs: &[Value], envs: &[(&str, &str)]) -> Vec<Value> {
    let mut c = Command::new(env!("CARGO_BIN_EXE_algebraking"));
    c.arg("mcp").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in envs {
        c.env(k, v);
    }
    let mut child = c.spawn().unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        for m in msgs {
            writeln!(stdin, "{}", m).unwrap();
        }
    }
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).expect("each line must be one JSON object"))
        .collect()
}

fn call(id: i64, tool: &str, args: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "method": "tools/call", "params": {"name": tool, "arguments": args}})
}

fn text(r: &Value) -> String {
    r["result"]["content"][0]["text"].as_str().unwrap_or("").to_string()
}

fn init() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
           "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}})
}

#[test]
fn handshake_and_tool_list() {
    let r = session(
        &[
            init(),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "ping"}),
            json!({"jsonrpc": "2.0", "id": 4, "method": "no/such/method"}),
            call(5, "no_such_tool", json!({})),
        ],
        &[],
    );
    assert_eq!(r.len(), 5, "notifications must not be answered: {:?}", r);
    assert_eq!(r[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(r[0]["result"]["serverInfo"]["name"], "algebraking");
    assert!(r[0]["result"]["capabilities"]["tools"].is_object());
    let names: Vec<&str> = r[1]["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    for want in [
        "algebra_eval",
        "verify_equal",
        "check_derivative",
        "check_integral",
        "check_solution",
        "check_ode_solution",
        "algebra_reset",
    ] {
        assert!(names.contains(&want), "missing tool {}", want);
    }
    for t in r[1]["result"]["tools"].as_array().unwrap() {
        assert_eq!(t["inputSchema"]["type"], "object");
    }
    assert_eq!(r[2]["result"], json!({}));
    assert_eq!(r[3]["error"]["code"], -32601);
    assert_eq!(r[4]["error"]["code"], -32602);
}

#[test]
fn parse_error_reply() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_algebraking"))
        .arg("mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.as_mut().unwrap(), "{{not json").unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let v: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    assert_eq!(v["error"]["code"], -32700);
}

#[test]
fn eval_without_engine_keeps_session_state() {
    let r = session(
        &[
            init(),
            call(2, "algebra_eval", json!({"code": "a := 3: f := x -> x^2 + a;"})),
            call(3, "algebra_eval", json!({"code": "f(2); b := 1/0; 7;"})),
            call(4, "algebra_eval", json!({"code": "a;", "session": "other"})),
            call(5, "algebra_reset", json!({})),
            call(6, "algebra_eval", json!({"code": "a;"})),
        ],
        &[("ALGEBRAKING_PYTHON", "/nonexistent/python3")],
    );
    assert!(text(&r[1]).starts_with("f := x -> x^2+a"), "{}", text(&r[1]));
    let t = text(&r[2]);
    assert!(t.starts_with("7\nError, numeric exception: division by zero\n7"), "{}", t);
    assert_eq!(r[2]["result"]["isError"], true);
    assert!(text(&r[3]).starts_with('a'));
    assert!(text(&r[5]).starts_with('a'));
}

#[test]
fn verification_tools() {
    if !have_engine() {
        eprintln!("skipping: python3 with sympy is not available");
        return;
    }
    let r = session(
        &[
            init(),
            json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            call(3, "verify_equal", json!({"lhs": "sin(x)^2+cos(x)^2", "rhs": "1"})),
            call(4, "verify_equal", json!({"lhs": "(x+1)^2", "rhs": "x^2+1"})),
            call(5, "verify_equal", json!({"lhs": "sqrt(x^2)", "rhs": "x", "assumptions": "x>0"})),
            call(6, "check_derivative", json!({"f": "x*ln(x)", "derivative": "ln(x)+1", "var": "x"})),
            call(7, "check_derivative", json!({"f": "x*ln(x)", "derivative": "ln(x)", "var": "x"})),
            call(8, "check_integral", json!({"integrand": "x*exp(x)", "antiderivative": "(x-1)*exp(x)", "var": "x"})),
            call(9, "check_solution", json!({"equations": "{x+y=3, x-y=1}", "solution": "{x=2, y=1}"})),
            call(10, "check_solution", json!({"equations": "x^2-4", "solution": "x=3"})),
            call(
                11,
                "check_ode_solution",
                json!({"ode": "{diff(y(x),x,x)+y(x)=0, y(0)=0, D(y)(0)=1}", "solution": "y(x)=sin(x)", "func": "y(x)"}),
            ),
            call(12, "check_ode_solution", json!({"ode": "diff(y(x),x) = 2*y(x)", "solution": "y(x)=exp(3*x)", "func": "y(x)"})),
            call(13, "algebra_eval", json!({"code": "int(x*sin(x), x);"})),
        ],
        &[],
    );
    let by_id = |id: i64| r.iter().find(|m| m["id"] == id).unwrap_or_else(|| panic!("no reply {}", id));
    assert!(text(by_id(3)).starts_with("verdict: equal"), "{}", text(by_id(3)));
    let t4 = text(by_id(4));
    assert!(t4.starts_with("verdict: not_equal") && t4.contains("counterexample:"), "{}", t4);
    assert!(text(by_id(5)).starts_with("verdict: equal"), "{}", text(by_id(5)));
    assert!(text(by_id(6)).starts_with("The claimed derivative is correct."));
    assert!(text(by_id(7)).starts_with("The claimed derivative is WRONG."));
    assert!(text(by_id(8)).starts_with("The antiderivative is correct"));
    assert!(text(by_id(9)).starts_with("The solution satisfies every equation."));
    assert!(text(by_id(10)).starts_with("The solution does NOT satisfy"), "{}", text(by_id(10)));
    assert!(text(by_id(11)).starts_with("The solution satisfies the ODE"), "{}", text(by_id(11)));
    assert!(text(by_id(12)).starts_with("The solution does NOT satisfy the ODE"), "{}", text(by_id(12)));
    let t13 = text(by_id(13));
    assert!(t13.starts_with("-x*cos(x)+sin(x)") && t13.contains("LaTeX:"), "{}", t13);
}

/// A python wrapper whose first launch hangs; later launches run the real python3.
fn hanging_python() -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("algebraking-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("first-run-done");
    let _ = std::fs::remove_file(&marker);
    let script = dir.join("python-wrapper.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ ! -e '{m}' ]; then touch '{m}'; exec sleep 30; fi\nexec python3 \"$@\"\n",
            m = marker.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

#[test]
fn timeout_restarts_engine() {
    if !have_engine() {
        eprintln!("skipping: python3 with sympy is not available");
        return;
    }
    let py = hanging_python();
    let r = session(
        &[init(), call(2, "algebra_eval", json!({"code": "int(x, x); int(x^2, x);"}))],
        &[("ALGEBRAKING_TIMEOUT", "1"), ("ALGEBRAKING_PYTHON", py.to_str().unwrap())],
    );
    let t = text(&r[1]);
    assert!(t.starts_with("Error, (in int) computation timed out after 1 s\nx^3/3"), "{}", t);
}
