#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "$0")/../.." && pwd)
if [[ $# -gt 0 ]]; then
    OUT_ROOT=$1
    shift
else
    OUT_ROOT=$(mktemp -d /tmp/neozeus-current-state-XXXXXX)
fi

cd "$ROOT_DIR"
cargo build >/tmp/neozeus-clone-state-build.log 2>&1
"$ROOT_DIR/target/debug/neozeus" clone-state capture-current --out-root "$OUT_ROOT" "$@"
printf '%s\n' "$OUT_ROOT"
