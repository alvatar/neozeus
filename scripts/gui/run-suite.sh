#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

if [[ $# -eq 0 ]]; then
    exec "$ROOT_DIR/scripts/offscreen/run-suite.sh"
fi

translated=()
for arg in "$@"; do
    case "$arg" in
        all)
            translated+=(all)
            ;;
        bloom)
            translated+=(agent-list)
            ;;
        visible)
            echo "GUI visible-terminal verifier was removed. Use offscreen scenarios instead." >&2
            echo "Suggested replacement: ./scripts/offscreen/verify-inspect-switch-latency.sh" >&2
            exit 2
            ;;
        colors)
            echo "GUI terminal-color verifier was removed. There is no offscreen replacement yet." >&2
            exit 2
            ;;
        *)
            echo "unknown GUI suite target: $arg" >&2
            echo "usage: $0 [all|bloom]" >&2
            exit 2
            ;;
    esac
done

exec "$ROOT_DIR/scripts/offscreen/run-suite.sh" "${translated[@]}"
