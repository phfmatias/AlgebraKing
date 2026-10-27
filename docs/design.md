# AlgebraKing — design

Date: 27.10.2026. Author: Pedro H.F Matias (LEEDMOL/UFG). Status: Development.

## Intent

A symbolic math program for Ubuntu whose language and behaviour follow Maple
(the user knows Maple well), written from scratch. Main use: Claude checks the
user's calculations through an MCP server ("conferir contas"). Second use: an
interactive terminal REPL that feels like command-line Maple.

**Clean room.** Nothing is taken from Maple's binaries, library (`.mla`) or help
database. Compatibility comes only from Maple's *public* documentation
(maplesoft.com/support/help) and general knowledge of the language. Do not open,
decompile or copy any Maple file.

## Goals

- Maple syntax and command names, so Maple code typed by the user runs as-is in
  most everyday cases.
- Correct answers first. When unsure, return the input unevaluated (Maple
  behaviour) or an error. Never a confident wrong answer.
- `verify` machinery that says "equal / not equal / undetermined" with a
  counterexample, so Claude can check a step of a derivation.
- MCP server over stdio, registered in Claude Code.

## Non-goals

- Maple's GUI, worksheets (.mw), plots window, Maplets, code generation, MapleSim.
- Byte-compatible output formatting with Maple. Close is enough.

## Architecture

```
            +---------------------------- algebraking (Rust binary) ---------------------------+
 user/REPL  |  lexer -> parser -> AST -> interpreter (env, procs, control flow, %, seq, map)   |
 Claude/MCP |       printer (Maple 1-D)        engine client (JSON lines over pipes)          |
            +---------------------------------------------|------------------------------------+
                                                          v
                                  engine/engine.py  (python3 + SymPy 1.14, persistent process)
                                  Maple-name function registry, verify, latex, pretty
```

- Rust owns the **language**: tokens, grammar, evaluation rules, variables,
  procedures, loops, `%`, packages/`with`, printing, MCP.
- Python owns the **mathematics**: every math function is a call into the engine.
  The engine never `eval`s strings: it builds SymPy objects from the JSON tree.
- `engine.py` is embedded in the binary with `include_str!` and written to
  `$XDG_CACHE_HOME/algebraking/engine-<hash>.py` (fallback `~/.cache/...`) on start.
  Python executable: env `ALGEBRAKING_PYTHON`, default `python3`. SymPy lives in
  the user site-packages, so do **not** pass `-I`/`-s` to python; use `-u`.

## Layout

```
AlgebraKing/
  Cargo.toml            binary name: algebraking
  src/main.rs           CLI dispatch
  src/{lexer,parser,ast,interp,printer,engine,mcp,repl}.rs
  engine/engine.py      SymPy engine (stdlib json + sympy + mpmath only)
  engine/test_engine.py unittest for the engine
  tests/                Rust integration tests (*.rs) + golden Maple snippets
  examples/*.mpl
  docs/design.md        this file
  README.md
```

CLI:
- `algebraking` → REPL (rustyline, history in `~/.algebraking_history`, prompt `> `).
- `algebraking file.mpl` → run a script, print results like the REPL.
- `algebraking -e "int(x^2,x);"` → evaluate a string.
- `algebraking mcp` → MCP server on stdio.

## Engine protocol (the contract)

One JSON object per line, both directions. Rust writes a request to the engine's
stdin, reads exactly one response line from stdout. The engine writes nothing
else to stdout (diagnostics go to stderr).

Request:
```json
{"id": 7, "op": "call", "name": "int", "args": [EXPR, ...], "opts": {"digits": 10, "assume": []}}
{"id": 8, "op": "verify", "lhs": EXPR, "rhs": EXPR, "opts": {"digits": 10, "assume": [EXPR, ...]}}
{"id": 9, "op": "latex",  "expr": EXPR}
{"id": 10, "op": "pretty", "expr": EXPR, "width": 80}
{"id": 11, "op": "ping"}
```
The engine is **stateless**. All session state travels in every request:
`opts.digits` is the current value of Maple's `Digits` (default 10) and
`opts.assume` is the list of assumptions active in the session (relations such as
`lt[0,x]` from `assume(x>0)`, or `{"fn":"type","args":[x,{"sym":"integer"}]}` from
`assume(x::integer)`). `verify` also accepts `opts`. Because of this, Rust may kill
and restart the engine at any time: each call has a timeout (env
`ALGEBRAKING_TIMEOUT`, default 60 s); on timeout Rust kills the process, restarts it
and reports `Error, (in name) computation timed out after N s`.

Response:
```json
{"id": 7, "ok": true,  "result": EXPR}
{"id": 7, "ok": false, "error": "(in int) wrong number (or type) of arguments"}
{"id": 8, "ok": true,  "result": {"verdict": "equal|not_equal|undetermined",
                                   "method": "symbolic|numeric|...",
                                   "difference": EXPR,
                                   "counterexample": {"x": "1.3", "...": "..."} }}
{"id": 9, "ok": true,  "result": "\\frac{x^{3}}{3}"}
```
Error strings follow Maple style without the leading `Error, ` (Rust adds it).

### EXPR (JSON expression tree)

| JSON | meaning |
|---|---|
| `{"int":"123"}` | integer, arbitrary size, decimal string |
| `{"rat":["1","3"]}` | rational p/q, q>1, reduced |
| `{"float":"1.234567890","digits":10}` | software float |
| `{"sym":"x"}` | name (also indexed names flattened like `"a[1]"` is NOT used, see idx) |
| `{"idx":[BASE,[I1,...]]}` | indexed name `a[1,2]` |
| `{"str":"text"}` | string |
| `{"bool":true}` / `{"bool":false}` / `{"fail":true}` | true/false/FAIL |
| `{"const":"Pi"}` | one of `Pi`, `I`, `infinity`, `gamma`, `Catalan`, `undefined` |
| `{"add":[E,...]}` | sum; subtraction is `add` with `mul[-1,b]` |
| `{"mul":[E,...]}` | product; division is `pow[b,-1]` |
| `{"pow":[B,E]}` | power |
| `{"fn":"sin","args":[E,...]}` | function application; unknown names stay unevaluated, e.g. `y(x)`; `exp`, `ln`, `sqrt` are fns |
| `{"fnidx":["evalf",[I...],[ARGS...]]}` | indexed call `evalf[20](x)`, `D[1](f)` |
| `{"eq":[A,B]}` `{"neq":[A,B]}` `{"lt":[A,B]}` `{"le":[A,B]}` | relations (`a>b` is `lt[b,a]`) |
| `{"and":[..]}` `{"or":[..]}` `{"not":E}` | logic |
| `{"list":[..]}` `{"set":[..]}` `{"seq":[..]}` | list, set, expression sequence (`NULL` = empty seq) |
| `{"range":[A,B]}` | `a..b` |
| `{"matrix":[[..],[..]]}` | Matrix, row-major |
| `{"vector":[..],"orient":"column"}` | Vector (`"row"` for row vectors) |
| `{"proc":{"params":["x","y"],"body":E}}` | arrow procedure `(x,y) -> body`, only when it can be expressed as one expression; engine calls that receive a proc (e.g. `D(f)`, `map`) may get it |

Engine conversion back to EXPR must normalise: `x - y` → `add[x, mul[-1,y]]`,
`x/y` → `mul[x,pow[y,-1]]`, `sqrt(x)` → `pow[x, rat[1,2]]` (printer shows `sqrt`),
`exp(1)` stays `fn exp`. `E` is **not** a constant in Maple (it is a name);
`exp(1)` is e.

## Language (Rust side)

Must parse and evaluate:

- statements end with `;` (show result) or `:` (suppress); last statement may lack terminator.
- comments `#...`; strings `"..."`; quoted names `` `a b` ``; unevaluation quotes `'x'` (`x := 'x'` unassigns).
- operators with Maple precedence: `||`, `$`, `!` (postfix factorial), `^` and `**`
  (non-associative in Maple: `a^b^c` is an error), unary `-`, `*` `/`, `+` `-`,
  `mod`, `..`, relations `= <> < <= > >=`, `not`, `and`, `or`, `xor`, `implies`,
  `->` (arrow procedure), `,` (sequence), `:=`, `%`, `%%`, `%%%`.
- `:-` member access (`LinearAlgebra:-Determinant(A)`), `pkg[f](args)`, `with(pkg)`
  (after `with`, short names of that package resolve; without it short names of
  `LinearAlgebra`/`inttrans`/`student`/`VectorCalculus` do not, like Maple).
- `f := x -> x^2`, `f := (x,y) -> x*y`, `unapply(expr, x)`, `D(f)`, `D[1](f)`,
  `(D@@2)(f)`, `f@g` composition, application of procs to symbolic args.
- `proc(params) local ...; global ...; option ...; ... return ...; end proc` (also `end`).
- `if c then ... elif ... else ... end if` (also `fi`), `for i from a to b by c do ... end do`
  (also `od`), `for x in L do`, `while c do`, `break`, `next`.
- `seq(f, i=a..b)`, `seq(f, x in L)`, `add`, `mul` (finite, numeric bounds: loop in Rust;
  symbolic bounds: forward to engine as `sum`/`product`), `map(f, L)`, `map2`, `select`,
  `remove`, `zip`, `nops`, `op`, `x[i]` indexing of lists/vectors/matrices, `$`
  (`x$3` → `x,x,x`, `f(i)$i=1..3`).
- Matrix/Vector angle-bracket syntax: `<1,2,3>` column vector, `<1|2|3>` row vector,
  `<<1|2>,<3|4>>` and `<<1,3>|<2,4>>` matrices.
- Evaluation: full evaluation of assigned names (with recursion guard), `eval(e)`,
  `eval(e, x=1)`, `eval(e, [x=1,y=2])`, `subs(x=1, e)` (Maple argument order).
- `read "file.mpl"` runs a source file (path relative to the current directory);
  elementwise `f~(args)` is `` `~`[f](args) ``; `assuming` binds looser than `,`
  also inside call arguments; `evalf(e, n)` and `evalf[n](e)` have special evaluation
  (`e` is evaluated with `Digits = n`). Float matrices (`Eigenvalues`, `Eigenvectors`)
  use a numeric mpmath solver at max(Digits, 15) digits, like Maple's LAPACK path.
- Session: `Digits := 30`, `restart`, `assume(x>0)`, `additionally`, `about`,
  `interface(prettyprint=0|1)`, `printf` with `%d %a %f %e %g %s`, `print`, `lprint`, `latex(e)`, `time()`.
- Any call whose name is not a language builtin or user proc goes to the engine
  (`op:"call"`). The engine returns the call unevaluated if it does not know the name.

Output (REPL): pretty 2-D (engine `pretty`) when `interface(prettyprint)` ≥ 1
(default 1 in REPL), else Maple 1-D from the Rust printer. Errors print
`Error, (in name) message` and the session keeps going. Script mode stops at the
first error with exit code 1. 1-D printer rules: `x^2`, `2*x`, `x/y`, `-x`,
`sqrt(x)`, `exp(x)`, `[a, b]`, `{a, b}`, `a = b`, `a <> b`, `Matrix(2, 2, [[1, 2], [3, 4]])`,
`Vector[column]([1, 2])`, minimal parentheses by precedence, floats printed with
the significant digits Maple would show (trailing `.` for floats like `2.`).

## Engine registry (Python side)

Maple semantics, argument order and return conventions (from Maple public help):

- arithmetic/simplification: `simplify` (incl. `simplify(e, trig)`, `symbolic`, `size`),
  `expand`, `factor` (incl. extensions `factor(e, I)`, `factor(e, sqrt(2))`), `normal`,
  `combine` (+ `trig`, `exp`, `ln`, `power`), `collect(e, x)`, `sort`, `rationalize`,
  `radnormal`, `evalc`, `evalf(e)`, `evalf(e, n)`, `evalf[n](e)`, `evalb`, `is`, `testeq`,
  `convert(e, parfrac, x)`, `convert(e, exp|trig|sincos|tan|ln|polynom|rational|list|set|float|radical|piecewise|Heaviside|binomial|factorial|GAMMA)`.
- polynomials: `coeff`, `coeffs`, `degree`, `ldegree`, `lcoeff`, `tcoeff`, `numer`, `denom`,
  `quo`, `rem`, `gcd`, `lcm`, `resultant`, `discrim`, `content`, `primpart`, `sqrfree`,
  `roots`, `RootOf` (via `CRootOf`/`RootOf` with `allvalues`), `Groebner[Basis]`, `interp`.
- number theory: `ifactor`, `ifactors`, `isprime`, `ithprime`, `nextprime`, `prevprime`,
  `igcd`, `ilcm`, `iquo`, `irem`, `mod`, `modp`, `mods`, `numtheory[phi]`, `divisors`,
  `binomial`, `factorial`, `floor`, `ceil`, `round`, `trunc`, `frac`, `abs`, `signum`, `max`, `min`.
- calculus: `diff(f, x)`, `diff(f, x, y)`, `diff(f, x$3)` (Rust expands `$`), `D`,
  `int(f, x)`, `int(f, x=a..b)`, `Int` (inert), `value`, `limit(f, x=a)`,
  `limit(f, x=a, left|right)`, `series(f, x=a, n)` (with `O(...)` term), `taylor`, `mtaylor`,
  `sum`, `product`, `Sum`, `Product`, `residue`, `singular`, `implicitdiff`, `extrema`,
  `maximize`, `minimize`, `student`/`Student[Calculus1]` basics if cheap.
- solving: `solve(eq, x)`, `solve({eqs}, {vars})` (Maple return shapes: sequence of
  values for one var; sets of equations `{x = 1, y = 2}` for systems; multiple solutions
  as an expression sequence), `fsolve`, `isolve`, `rsolve`, `solve` with inequalities,
  `dsolve(ode, y(x))`, `dsolve({ode, ics}, y(x))`, systems, `dsolve(..., numeric)` may
  return an error "not supported yet" rather than wrong output, `pdsolve` basic,
  `odetest`.
- linear algebra (`LinearAlgebra`): `Matrix`, `Vector`, `Determinant`, `MatrixInverse`,
  `Transpose`, `HermitianTranspose`, `Eigenvalues`, `Eigenvectors`, `Rank`, `Trace`,
  `NullSpace`, `ColumnSpace`, `RowSpace`, `LinearSolve`, `ReducedRowEchelonForm`,
  `GaussianElimination`, `LUDecomposition`, `QRDecomposition`, `CharacteristicPolynomial`,
  `MinimalPolynomial`, `JordanForm`, `DotProduct`, `CrossProduct`, `Norm`, `Normalize`,
  `IdentityMatrix`, `ZeroMatrix`, `DiagonalMatrix`, `MatrixExponential`, `MatrixPower`,
  `IsDefinite`, `GramSchmidt`, `SingularValues`, plus matrix arithmetic `A.B`, `A^(-1)`, `A+B`, `k*A`.
- transforms (`inttrans`): `laplace`, `invlaplace`, `fourier`, `invfourier`, `mellin`, `hilbert` if cheap.
- vector calculus (`VectorCalculus`): `Gradient`, `Divergence`, `Curl`, `Laplacian`,
  `Jacobian`, `Hessian`, `Wronskian`.
- special functions with Maple names: `GAMMA`, `Beta`, `Psi`, `Zeta`, `erf`, `erfc`,
  `Ei`, `Si`, `Ci`, `LambertW`, `BesselJ/Y/I/K`, `AiryAi/Bi`, `LegendreP/Q`,
  `HermiteH`, `LaguerreL`, `ChebyshevT/U`, `hypergeom`, `EllipticK/E/F`, `dilog`,
  `polylog`, `Heaviside`, `Dirac`, `piecewise`, `arcsin`.. `arccot`, `sinh`.. `arccoth`,
  `ln`, `log[b]`, `log10`, `exp`, `sqrt`, `surd`, `root`, `csgn`, `argument`, `Re`, `Im`,
  `conjugate`.
- misc: `nops`, `op` (on engine structures), `lhs`, `rhs`, `indets`, `has`, `type(e, t)` for
  common types (`integer`, `rational`, `numeric`, `polynom`, `symbol`, `list`, `set`,
  `equation`, `positive`, `constant`, `Matrix`), `whattype`, `assume`-aware simplification.

Function-name mapping goes through one table so adding a function is one line.

### verify algorithm

1. Elementwise for lists, sets (sorted), equations (`lhs-rhs` both sides), matrices.
2. `d = lhs - rhs`. Try cheap symbolic zero test (`d == 0`, `expand`, `cancel`,
   `simplify` with a timeout of a few seconds, `trigsimp`, `radsimp`) under the
   session's assumptions. Zero → `equal`, method `symbolic`.
3. Numeric test: 12 random points for the free symbols (real in [-3,3] and some
   complex points with |z|≤2, respecting assumptions such as `x>0`), evaluated with
   mpmath at 40 digits; skip points where either side is infinite/NaN. All |d| below
   `1e-25 * max(1,|lhs|)` → `equal`, method `numeric` (state that it is numeric).
   One point clearly non-zero → `not_equal` with that point as `counterexample`.
4. Otherwise `undetermined`. Never raise for a math reason.

## MCP server

Protocol: JSON-RPC 2.0 over stdio, newline-delimited, MCP revision `2025-06-18`
(accept older client revisions by echoing ours). Implements `initialize`,
`notifications/initialized`, `ping`, `tools/list`, `tools/call`. Server name
`algebraking`. Each server process keeps named sessions (default `"main"`), each an
interpreter + its engine state (`Digits`, assumptions, variables). Tool results are
text content; math also returned in LaTeX when useful. Tools:

| tool | input | does |
|---|---|---|
| `algebra_eval` | `code` (Maple syntax), `session?` | runs statements, returns each shown result in 1-D, plus LaTeX of the last |
| `verify_equal` | `lhs`, `rhs`, `assumptions?` (e.g. `"x>0, n::integer"`), `session?` | verdict + method + counterexample |
| `check_derivative` | `f`, `derivative`, `var` | verify `diff(f,var) = derivative` |
| `check_integral` | `integrand`, `antiderivative`, `var` | verify `diff(F,var) = integrand` (up to a constant) |
| `check_solution` | `equations`, `solution` | substitute, verify every equation |
| `check_ode_solution` | `ode`, `solution`, `func` (e.g. `y(x)`) | like Maple `odetest` |
| `algebra_reset` | `session?` | `restart` |

Tool descriptions must tell Claude to use Maple syntax and to send the user's step
exactly as written. Register with:
`claude mcp add algebraking -s user -- ~/MyApps/AlgebraKing/target/release/algebraking mcp`.

## Code style

File header (Rust and Python), English, date `27.10.2026`:

```
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
//   Description:  ['One short English sentence.']
```
Python uses `##` instead of `//` and `Language: ['Python']`. Comments sparse,
short, English. Rust follows Rust naming (snake_case, clippy clean). Python engine:
classes in CamelCase, methods camelCase, `.format()` or f-strings, no `except:` bare,
no type hints needed. User messages in English.
