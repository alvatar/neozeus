#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

run_visible=1
run_colors=1

if [[ $# -gt 0 ]]; then
    run_visible=0
    run_colors=0
    for arg in "$@"; do
        case "$arg" in
            visible)
                run_visible=1
                ;;
            colors)
                run_colors=1
                ;;
            all)
                run_visible=1
                run_colors=1
                ;;
            *)
                echo "unknown GUI suite target: $arg" >&2
                echo "usage: $0 [all|visible|colors ...]" >&2
                exit 2
                ;;
        esac
    done
fi

if [[ "$run_visible" -eq 1 ]]; then
    echo ">>> GUI suite: visible terminal verifier"
    "$ROOT_DIR/scripts/gui/verify-visible-terminal.sh"
fi

if [[ "$run_colors" -eq 1 ]]; then
    echo ">>> GUI suite: terminal color verifier"
    "$ROOT_DIR/scripts/gui/verify-terminal-colors.sh"
fi

echo "GUI suite: PASS"
