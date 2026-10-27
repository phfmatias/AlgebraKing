//   AlgebraKing //
//
//   File:         [cli.rs]
//
//   Author(s):    ['Pedro H.F Matias']
//   Site(s):      ['https://github.com/phfmatias']
//   Email(s):     ['pedrohfmatias@gmail.com']
//   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
//   Date:         ['27.10.2026']
//   Version:      ['0.0.1']
//   Status:       ['Development']
//   Language:     ['Rust']
//   Description:  ['End-to-end tests of the algebraking binary (-e, scripts, golden files).']

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_algebraking")
}

/// True when python3 with SymPy is available (engine tests are skipped otherwise).
fn have_engine() -> bool {
    let py = std::env::var("ALGEBRAKING_PYTHON").unwrap_or_else(|_| "python3".into());
    Command::new(py).args(["-c", "import sympy"]).output().map(|o| o.status.success()).unwrap_or(false)
}

macro_rules! need_engine {
    () => {
        if !have_engine() {
            eprintln!("skipping: python3 with sympy is not available");
            return;
        }
    };
}

fn run_e(code: &str, envs: &[(&str, &str)]) -> (String, i32) {
    let mut c = Command::new(bin());
    c.args(["-e", code]);
    for (k, v) in envs {
        c.env(k, v);
    }
    let o = c.output().expect("cannot run algebraking");
    (String::from_utf8_lossy(&o.stdout).to_string(), o.status.code().unwrap_or(-1))
}

#[test]
fn task_smoke_test() {
    need_engine!();
    let (out, code) = run_e(
        "int(x^2,x); diff(sin(x)^2,x); solve(x^2-4=0,x); f:=x->x^3: D(f)(2); A:=<<1|2>,<3|4>>: LinearAlgebra:-Determinant(A);",
        &[],
    );
    assert_eq!(code, 0, "{}", out);
    assert_eq!(out, "x^3/3\n2*sin(x)*cos(x)\n2, -2\n12\n-2\n");
}

#[test]
fn calculus_and_algebra() {
    need_engine!();
    let cases = [
        ("int(x^2, x = 0 .. 1);", "1/3"),
        ("diff(x^5, x$3);", "60*x^2"),
        ("expand((x+1)^2);", "x^2+2*x+1"),
        ("factor(x^2-1);", "(x-1)*(x+1)"),
        ("solve({x+y=3, x-y=1}, {x, y});", "{x = 2, y = 1}"),
        ("limit(sin(x)/x, x = 0);", "1"),
        ("D(sin);", "cos"),
        ("(D@@2)(sin);", "-sin"),
        ("D(x -> x^3);", "x -> 3*x^2"),
        ("f := (x,y) -> x^2*y: D[1](f);", "(x, y) -> 2*x*y"),
        ("eval(diff(y(x), x), x = 0);", "D(y)(0)"),
        ("sqrt(12);", "2*sqrt(3)"),
        ("evalf(Pi);", "3.141592654"),
        ("Digits := 30: evalf(Pi);", "3.14159265358979323846264338328"),
        ("evalf[20](Pi);", "3.1415926535897932385"),
        ("eval(sin(x), x = 0); subs(x = 0, sin(x));", "0\nsin(0)"),
        ("a := subs(x = 0, cos(x)): eval(a);", "1"),
        ("add(k, k = 1 .. n);", "n^2/2+n/2"),
        ("with(LinearAlgebra): Determinant(<<1|2>,<3|4>>);", "-2"),
        ("A := <<2|1>,<1|2>>: A^(-1);", "Matrix(2, 2, [[2/3, -1/3], [-1/3, 2/3]])"),
        ("assume(x > 0): simplify(sqrt(x^2)); is(x > 0);", "x\ntrue"),
        ("simplify(sqrt(x^2)) assuming x < 0;", "-x"),
        ("verify(sin(x)^2 + cos(x)^2, 1); verify(x, x + 1);", "true\nfalse"),
        ("a := subs(x = 0, sin(x)): a;", "0"),
        ("g := D(f): f := x -> x^2: g;", "x -> 2*x"),
        ("eval(diff(y(x), x, x), x = 0);", "(D@@2)(y)(0)"),
        ("dsolve({diff(y(x),x$3)=0, y(0)=1, D(y)(0)=0, (D@@2)(y)(0)=2}, y(x));", "y(x) = x^2+1"),
        ("ifactor(360); expand(ifactor(360));", "2^3*3^2*5\n360"),
        ("log[2](8); numtheory[phi](10); series(exp(x), x = 0, 4);", "3\n4\n1+x+x^2/2+x^3/6+O(x^4)"),
        ("divisors(12); with(numtheory): divisors(12);", "divisors(12)\n{1, 2, 3, 4, 6, 12}"),
        ("g := x -> piecewise(x < 0, -x, x^2): g(-2); g(3);", "2\n9"),
        ("with(LinearAlgebra): with(Groebner): Basis([x^2-y, x*y-1], plex(x, y));", "[-y^2+x, y^3-1]"),
    ];
    for (code, want) in cases {
        let (out, status) = run_e(code, &[]);
        assert_eq!(status, 0, "{} -> {}", code, out);
        assert_eq!(out.trim_end(), want, "{}", code);
    }
}

#[test]
fn script_stops_at_first_error() {
    let (out, code) = run_e("1; 1/0; 2;", &[]);
    assert_eq!(code, 1);
    assert_eq!(out, "1\nError, numeric exception: division by zero\n");
    let (out, code) = run_e("1; x^2^3; 2;", &[]);
    assert_eq!(code, 1);
    assert_eq!(out, "1\nError, ambiguous use of `^`, please use parentheses\n");
}

#[test]
fn missing_python_is_an_error_not_a_crash() {
    let (out, code) = run_e("1 + 1; int(x, x);", &[("ALGEBRAKING_PYTHON", "/nonexistent/python3")]);
    assert_eq!(code, 1);
    assert!(out.starts_with("2\nError, cannot start the math engine"), "{}", out);
}

#[test]
fn timeout_kills_and_reports() {
    // a "python" that never answers
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("algebraking-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let py = dir.join("hang.sh");
    std::fs::write(&py, "#!/bin/sh\nexec sleep 30\n").unwrap();
    std::fs::set_permissions(&py, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (out, code) =
        run_e("1 + 1; int(x, x);", &[("ALGEBRAKING_TIMEOUT", "1"), ("ALGEBRAKING_PYTHON", py.to_str().unwrap())]);
    assert_eq!(code, 1);
    assert_eq!(out, "2\nError, (in int) computation timed out after 1 s\n");
}

fn golden_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("golden")
}

#[test]
fn golden_files() {
    let engine = have_engine();
    let mut checked = 0;
    for entry in std::fs::read_dir(golden_dir()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("mpl") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if name.starts_with("engine_") && !engine {
            eprintln!("skipping {} (no engine)", name);
            continue;
        }
        let want = std::fs::read_to_string(path.with_extension("out")).unwrap();
        let o = Command::new(bin()).arg(&path).output().unwrap();
        let got = String::from_utf8_lossy(&o.stdout);
        assert_eq!(got, want, "golden file {}", name);
        checked += 1;
    }
    assert!(checked >= 1);
}

#[test]
fn examples_run_cleanly() {
    need_engine!();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("mpl") {
            continue;
        }
        let o = Command::new(bin()).arg(&path).output().unwrap();
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(o.status.success(), "{} failed:\n{}", path.display(), out);
        assert!(!out.contains("Error,"), "{}:\n{}", path.display(), out);
    }
}
