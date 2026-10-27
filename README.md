<p align="center">
  <img src="assets/banner_Gold.png" alt="AlgebraKing" width="720">
</p>

<p align="center"><b>A Maple-style computer algebra system for the terminal, written from scratch, with an MCP server so Claude can check your calculations.</b></p>

<p align="center">
  <img src="https://img.shields.io/badge/Rust-2021-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/Python-%E2%89%A5%203.8-3776AB?logo=python&logoColor=white" alt="Python">
  <img src="https://img.shields.io/badge/engine-SymPy%201.14-3B5526" alt="SymPy">
  <img src="https://img.shields.io/badge/MCP-server-8A2BE2" alt="MCP">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT">
</p>

## Why

If you know Maple, you already know AlgebraKing: `int(x^2, x);`, `dsolve`,
`<<1|2>, <3|4>>`, `proc ... end proc`, `with(LinearAlgebra)` and `assuming` are parsed
and evaluated the way you expect. The *language* (parser, evaluation rules,
procedures, printing) is written in Rust; the *mathematics* is delegated to a SymPy
engine behind Maple's function names and return conventions. The main use case is
"conferir contas": Claude sends a step of your derivation to AlgebraKing through MCP
and gets back `equal`, `not_equal` (with a counterexample) or `undetermined`. It is
also a pleasant command-line CAS on its own.

## Install

```bash
git clone https://github.com/phfmatias/AlgebraKing.git
cd AlgebraKing
./install.sh
```

`install.sh` checks Python and SymPy, installs Rust (rustup, minimal profile) if
needed, builds the release binary, links it into `~/.local/bin` and registers the
MCP server in Claude Code. Options: `--no-mcp`, `--no-python-deps`, `--prefix DIR`.
Update with `git pull && ./install.sh`; remove with `./uninstall.sh`.

## 30-second demo

```text
$ algebraking
AlgebraKing 0.0.1 - Maple-style symbolic math (SymPy engine). Type quit; to leave.
> f := x -> x^2*exp(-x):
> int(f(x), x=0..infinity);
2
> diff(f(x), x);
   2  -x        -x
- x ⋅e   + 2⋅x⋅e
> A := <<1|2>, <3|4>>:
> with(LinearAlgebra):
> MatrixInverse(A);
⎡-2    1  ⎤
⎢         ⎥
⎣3/2  -1/2⎦
```

```console
$ algebraking -e 'solve({x + y = 3, x - y = 1}, {x, y}); series(exp(x), x=0, 5);'
{x = 2, y = 1}
1+x+x^2/2+x^3/6+x^4/24+O(x^5)
$ algebraking examples/calculus.mpl      # run a script
```

## Features

| Area | What you get |
|---|---|
| Language | Maple syntax and precedence, `proc`/`end proc` with `local`/`option remember`, arrow procedures, `D`, `@`, `@@`, `if`/`for`/`while`, `try`/`catch`, `seq`/`add`/`mul`/`map`/`select`/`zip`, `%`, `read`, `with(pkg)` and `pkg:-f` |
| Algebra | `simplify`, `expand`, `factor` (with extensions), `normal`, `combine`, `collect`, `convert`, `evalc`, `rationalize`, polynomial tools (`gcd`, `resultant`, `Groebner[Basis]`, ...) |
| Calculus | `diff`, `int`, `limit`, `series`/`taylor`, `sum`/`product`, `residue`, `implicitdiff`, `extrema`, `laplace`/`fourier`/`mellin` |
| Solving | `solve`, `fsolve`, `isolve`, `rsolve`, `dsolve` (ODE and systems), `pdsolve` (basic), `odetest` |
| Linear algebra | `Matrix`/`Vector` with `<...>` syntax, `Determinant`, `Eigenvalues`/`Eigenvectors` (exact and numeric), `LinearSolve`, `JordanForm`, `LUDecomposition`, `QRDecomposition`, `MatrixExponential`, ... |
| Special functions | `GAMMA`, `Zeta`, `erf`, Bessel, Airy, Legendre/Hermite/Laguerre/Chebyshev, `hypergeom`, elliptic integrals, `LambertW`, `Heaviside`, `Dirac`, `piecewise` |
| Numbers | exact rationals, big integers, software floats with `Digits` and `evalf[n]`, `ifactor`, `isprime`, `numtheory`, `combinat` |
| Verification | `verify` machinery behind the MCP tools: symbolic first, then numeric, with counterexample |

Anything that is not a language builtin goes to the engine, and unknown names come
back unevaluated, as in Maple. See [docs/GUIA_USUARIO.md](docs/GUIA_USUARIO.md) for the
full catalogue with runnable examples.

## Use it with Claude (MCP)

`./install.sh` registers the server automatically. Manually:

```bash
claude mcp add algebraking -s user -- "$(command -v algebraking)" mcp
claude mcp list          # algebraking should be listed as connected
```

Then ask Claude things like *"Confere se a derivada de x\*exp(x) é (x+1)\*exp(x)"*.

| Tool | Arguments | Purpose |
|---|---|---|
| `algebra_eval` | `code`, `session?` | Run Maple statements in a persistent session; returns 1-D results plus LaTeX |
| `verify_equal` | `lhs`, `rhs`, `assumptions?`, `session?` | `equal` / `not_equal` / `undetermined`, method, counterexample |
| `check_derivative` | `f`, `derivative`, `var` | Is `diff(f, var)` the claimed derivative? |
| `check_integral` | `integrand`, `antiderivative`, `var` | Does the antiderivative differentiate back to the integrand? |
| `check_solution` | `equations`, `solution` | Substitute the claimed solution into every equation |
| `check_ode_solution` | `ode`, `solution`, `func` | Like `odetest`, with initial conditions |
| `algebra_reset` | `session?` | `restart` a session |

A numeric `equal` is strong evidence, not a proof; the verdict always says which
method decided. Claude Desktop setup and example prompts are in the
[guide](docs/GUIA_USUARIO.md).

## Documentation

- [docs/GUIA_USUARIO.md](docs/GUIA_USUARIO.md): full user guide in Portuguese (install, language, math catalogue, MCP, differences from Maple, troubleshooting).
- [docs/design.md](docs/design.md): architecture, engine protocol, language rules, MCP design.

## Project layout

```
AlgebraKing/
  install.sh, uninstall.sh   installer / uninstaller
  Cargo.toml                 binary name: algebraking
  src/                       Rust: lexer, parser, interpreter, printer, engine client, REPL, MCP
  engine/engine.py           SymPy engine (embedded in the binary with include_str!)
  engine/test_engine.py      engine unit tests
  tests/                     Rust integration tests + golden Maple scripts
  examples/*.mpl             runnable examples
  assets/                    logos; make_logo.py regenerates them
  docs/                      user guide (pt-BR) and design notes
```

`assets/crown_mask.png` is the cached crown cut used by `assets/make_logo.py`, so the
script runs without the external source image (it still needs the Neuropol font).

## Testing

```bash
cargo test --release                      # Rust unit, CLI, golden and MCP tests
python3 -m unittest engine/test_engine.py # SymPy engine tests
```

Beyond the unit tests, AlgebraKing reproduced 257/257 verdicts of a real Maple 18
verification suite taken from a research project (all checks passed).

Environment variables: `ALGEBRAKING_PYTHON` (Python executable, default `python3`),
`ALGEBRAKING_TIMEOUT` (seconds per engine call, default 60), `ALGEBRAKING_DEBUG`
(forward the engine's stderr).

## Clean room

AlgebraKing contains no code, library, help text or data from Maple, and no Maple
binary, library (`.mla`) or help database was opened, searched or decompiled while
writing it. Compatibility comes only from Maple's public documentation and general
knowledge of the language. Maple is a trademark of Waterloo Maple Inc.; this project
is not affiliated with or endorsed by it.

## Contributing

Issues and pull requests are welcome: bug reports with a minimal `algebraking -e '...'`
line that reproduces the problem are the most useful. Adding a function to the engine
is one entry in the registry in `engine/engine.py` plus a test in
`engine/test_engine.py`.

## License

MIT, see [LICENSE](LICENSE).

## Credits

Pedro H.F Matias, LEEDMOL-UFG. The wordmark uses the Neuropol typeface (Typodermic),
rendered to PNG; the font file itself is not distributed.
