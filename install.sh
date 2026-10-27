#!/usr/bin/env bash
#   AlgebraKing INSTALLER #
#
#   File:         [install.sh]
#
#   Author(s):    ['Pedro H.F Matias']
#   Site(s):      ['https://github.com/phfmatias']
#   Email(s):     ['pedrohfmatias@gmail.com']
#   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
#   Date:         ['27.10.2026']
#   Version:      ['0.0.1']
#   Status:       ['Development']
#   Language:     ['Bash']
#   Description:  ['Build AlgebraKing, link it into ~/.local/bin and register the MCP server in Claude Code.']
#   Usage:        ['./install.sh [--no-mcp] [--no-python-deps] [--prefix DIR] [-h]']

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local/bin"
DO_MCP=1
DO_PYDEPS=1

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    C_BLUE=$'\033[1;34m'; C_GREEN=$'\033[1;32m'; C_YELLOW=$'\033[1;33m'; C_RED=$'\033[1;31m'; C_OFF=$'\033[0m'
else
    C_BLUE=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_OFF=''
fi

step() { printf '%s==>%s %s\n' "$C_BLUE" "$C_OFF" "$*"; }
ok()   { printf '%s  ok%s %s\n' "$C_GREEN" "$C_OFF" "$*"; }
warn() { printf '%s  warning:%s %s\n' "$C_YELLOW" "$C_OFF" "$*" >&2; }
die()  { printf '%sError:%s %s\n' "$C_RED" "$C_OFF" "$*" >&2; exit 1; }

usage() {
    cat <<EOF
Usage: ./install.sh [options]

Build AlgebraKing from source, link the binary into a directory on your PATH and
register the MCP server in Claude Code. Safe to run again (idempotent).

Options:
  --no-mcp           do not register the MCP server in Claude Code
  --no-python-deps   do not check/install the Python packages (sympy, mpmath)
  --prefix DIR       directory for the symlink (default: ~/.local/bin)
  -h, --help         show this help
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --no-mcp) DO_MCP=0 ;;
        --no-python-deps) DO_PYDEPS=0 ;;
        --prefix)
            [[ $# -ge 2 ]] || die "--prefix needs a directory"
            PREFIX="$2"; shift ;;
        --prefix=*) PREFIX="${1#--prefix=}" ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown option: $1" ;;
    esac
    shift
done
PREFIX="${PREFIX/#\~/$HOME}"

cd "$ROOT"
[[ -f Cargo.toml && -f engine/engine.py ]] || die "run this script from the AlgebraKing repository (Cargo.toml not found in $ROOT)"

# ---------------------------------------------------------------- python ----
step "Checking Python (>= 3.8)"
PY="${ALGEBRAKING_PYTHON:-python3}"
command -v "$PY" >/dev/null 2>&1 || die "$PY not found. Install it first (Ubuntu: sudo apt install python3 python3-pip)."
if ! "$PY" -c 'import sys; sys.exit(0 if sys.version_info >= (3, 8) else 1)'; then
    die "Python >= 3.8 is required, found $("$PY" -c 'import sys; print("%d.%d" % sys.version_info[:2])')."
fi
ok "$("$PY" --version 2>&1)"

if [[ $DO_PYDEPS -eq 1 ]]; then
    step "Checking Python packages (sympy, mpmath)"
    if "$PY" -c 'import sympy, mpmath' >/dev/null 2>&1; then
        ok "sympy $("$PY" -c 'import sympy; print(sympy.__version__)'), mpmath $("$PY" -c 'import mpmath; print(mpmath.__version__)')"
    else
        echo "  sympy/mpmath missing: running  $PY -m pip install --user sympy mpmath"
        if ! "$PY" -m pip install --user sympy mpmath; then
            echo "  pip refused (externally managed environment?): retrying with --break-system-packages (user site only)"
            "$PY" -m pip install --user --break-system-packages sympy mpmath \
                || die "could not install sympy/mpmath. Try: sudo apt install python3-sympy python3-mpmath"
        fi
        "$PY" -c 'import sympy, mpmath' >/dev/null 2>&1 || die "sympy/mpmath still not importable after pip install."
        ok "installed sympy $("$PY" -c 'import sympy; print(sympy.__version__)')"
    fi
else
    step "Skipping Python packages (--no-python-deps)"
fi

# ----------------------------------------------------------------- rust -----
step "Checking Rust toolchain (cargo)"
if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi
if ! command -v cargo >/dev/null 2>&1; then
    command -v curl >/dev/null 2>&1 || die "cargo not found and curl is missing (sudo apt install curl), cannot install rustup."
    echo "  cargo not found: installing rustup (minimal profile) into $HOME/.cargo"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
    command -v cargo >/dev/null 2>&1 || die "rustup finished but cargo is still not on PATH."
fi
ok "$(cargo --version)"

step "Building (cargo build --release)"
cargo build --release
BIN="$ROOT/target/release/algebraking"
[[ -x "$BIN" ]] || die "build finished but $BIN was not produced."
ok "built $BIN"

# -------------------------------------------------------------- symlink -----
step "Linking binary into $PREFIX"
mkdir -p "$PREFIX"
LINK="$PREFIX/algebraking"
if [[ -e "$LINK" && ! -L "$LINK" ]]; then
    die "$LINK exists and is not a symlink; move it away and run again."
fi
ln -sfn "$BIN" "$LINK"
ok "$LINK -> $BIN"
case ":$PATH:" in
    *":$PREFIX:"*) ;;
    *)
        warn "$PREFIX is not on your PATH. Add this line to ~/.bashrc and open a new terminal:"
        echo "    export PATH=\"$PREFIX:\$PATH\""
        ;;
esac

# ------------------------------------------------------------ self-test -----
step "Self-test:  algebraking -e 'int(x^2,x);'"
OUT="$("$BIN" -e 'int(x^2,x);' 2>&1 || true)"
if [[ "$OUT" == "x^3/3" ]]; then
    ok "printed $OUT"
else
    die "self-test failed: expected x^3/3, got: ${OUT:-<nothing>} (is sympy installed for $PY? see docs/GUIA_USUARIO.md, Solução de problemas)"
fi

# ------------------------------------------------------------------ mcp -----
MCP_NOTE=""
if [[ $DO_MCP -eq 1 ]]; then
    step "Registering the MCP server in Claude Code"
    if ! command -v claude >/dev/null 2>&1; then
        warn "claude CLI not found: skipping. Register later with:"
        echo "    claude mcp add algebraking -s user -- $BIN mcp"
        MCP_NOTE="not registered (claude CLI not found)"
    elif claude mcp get algebraking >/dev/null 2>&1; then
        ok "already registered (algebraking), nothing to do. Inspect with: claude mcp get algebraking"
        MCP_NOTE="already registered"
    else
        claude mcp add algebraking -s user -- "$BIN" mcp
        ok "registered: claude mcp add algebraking -s user -- $BIN mcp"
        MCP_NOTE="registered (restart Claude Code sessions to load it)"
    fi
else
    step "Skipping MCP registration (--no-mcp)"
    MCP_NOTE="skipped (--no-mcp)"
fi

# -------------------------------------------------------------- summary -----
echo
printf '%sAlgebraKing installed!%s\n' "$C_GREEN" "$C_OFF"
echo "  binary : $BIN"
echo "  link   : $LINK"
echo "  MCP    : $MCP_NOTE"
echo
echo "Next steps:"
echo "  algebraking                          open the interactive session (REPL)"
echo "  algebraking -e 'diff(x*sin(x),x);'    evaluate one line"
echo "  algebraking examples/calculus.mpl    run a script"
echo "  In Claude Code: run /mcp and ask for a check, e.g. \"Check that d/dx x*sin(x) = sin(x) + x*cos(x)\""
echo "  Guide (Portuguese): docs/GUIA_USUARIO.md"
echo "  Update: git pull && ./install.sh      Uninstall: ./uninstall.sh"
