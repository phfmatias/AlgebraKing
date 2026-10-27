#!/usr/bin/env bash
#   AlgebraKing UNINSTALLER #
#
#   File:         [uninstall.sh]
#
#   Author(s):    ['Pedro H.F Matias']
#   Site(s):      ['https://github.com/phfmatias']
#   Email(s):     ['pedrohfmatias@gmail.com']
#   Credits:      ['Copyright © 2026 LEEDMOL. All rights reserved.']
#   Date:         ['27.10.2026']
#   Version:      ['0.0.1']
#   Status:       ['Development']
#   Language:     ['Bash']
#   Description:  ['Remove the AlgebraKing symlink, the Claude Code MCP registration and the engine cache; never touches the repository.']
#   Usage:        ['./uninstall.sh [--prefix DIR] [-h]']

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PREFIX="${HOME}/.local/bin"

if [[ -t 1 && -z "${NO_COLOR:-}" ]]; then
    C_BLUE=$'\033[1;34m'; C_GREEN=$'\033[1;32m'; C_YELLOW=$'\033[1;33m'; C_RED=$'\033[1;31m'; C_OFF=$'\033[0m'
else
    C_BLUE=''; C_GREEN=''; C_YELLOW=''; C_RED=''; C_OFF=''
fi

step() { printf '%s==>%s %s\n' "$C_BLUE" "$C_OFF" "$*"; }
ok()   { printf '%s  ok%s %s\n' "$C_GREEN" "$C_OFF" "$*"; }
warn() { printf '%s  warning:%s %s\n' "$C_YELLOW" "$C_OFF" "$*" >&2; }
die()  { printf '%sError:%s %s\n' "$C_RED" "$C_OFF" "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --prefix)
            [[ $# -ge 2 ]] || die "--prefix needs a directory"
            PREFIX="$2"; shift ;;
        --prefix=*) PREFIX="${1#--prefix=}" ;;
        -h|--help)
            cat <<EOF
Usage: ./uninstall.sh [--prefix DIR]

Removes the algebraking symlink (default: ~/.local/bin), the Claude Code MCP
registration and the engine cache (~/.cache/algebraking). The repository and
target/ are left alone; delete the folder yourself if you want it gone.
EOF
            exit 0 ;;
        *) die "unknown option: $1" ;;
    esac
    shift
done
PREFIX="${PREFIX/#\~/$HOME}"

step "Removing symlink"
LINK="$PREFIX/algebraking"
if [[ -L "$LINK" ]]; then
    rm "$LINK"
    ok "removed $LINK"
elif [[ -e "$LINK" ]]; then
    warn "$LINK is not a symlink: left untouched"
else
    ok "no $LINK, nothing to do"
fi

step "Removing MCP registration"
if command -v claude >/dev/null 2>&1; then
    if claude mcp get algebraking >/dev/null 2>&1; then
        claude mcp remove algebraking -s user
        ok "claude mcp remove algebraking -s user"
    else
        ok "algebraking is not registered, nothing to do"
    fi
else
    ok "claude CLI not found, nothing to do"
fi

step "Removing engine cache"
CACHE="${XDG_CACHE_HOME:-$HOME/.cache}/algebraking"
if [[ -d "$CACHE" ]]; then
    rm -r "$CACHE"
    ok "removed $CACHE"
else
    ok "no $CACHE, nothing to do"
fi

echo
printf '%sAlgebraKing uninstalled.%s\n' "$C_GREEN" "$C_OFF"
echo "The repository was kept: $ROOT"
echo "REPL history (~/.algebraking_history) was kept; remove it by hand if you like."
