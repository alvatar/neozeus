#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

run_visible=1
run_colors=1
run_bloom=1

if [[ $# -gt 0 ]]; then
    run_visible=0
    run_colors=0
    run_bloom=0
    for arg in "$@"; do
        case "$arg" in
            visible)
                run_visible=1
                ;;
            colors)
                run_colors=1
                ;;
            bloom)
                run_bloom=1
                ;;
            all)
                run_visible=1
                run_colors=1
                run_bloom=1
                ;;
            *)
                echo "unknown GUI suite target: $arg" >&2
                echo "usage: $0 [all|visible|colors|bloom ...]" >&2
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

if [[ "$run_bloom" -eq 1 ]]; then
    echo ">>> GUI suite: agent-list bloom verifier"
    "$ROOT_DIR/scripts/gui/verify-agent-list-bloom.sh"
fi

echo "GUI suite: PASS"
