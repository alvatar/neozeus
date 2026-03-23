#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

APP="$ROOT_DIR/target/debug/neozeus"
RUN_LOG=/tmp/neozeus-agent-bloom-debug-run.log
WORKSPACE=${NEOZEUS_AGENT_BLOOM_WORKSPACE:-8}
TITLE=${NEOZEUS_AGENT_BLOOM_DEBUG_TITLE:-neozeus-bloom-debug}
WINDOW_WIDTH=${NEOZEUS_AGENT_BLOOM_DEBUG_WINDOW_WIDTH:-1600}
WINDOW_HEIGHT=${NEOZEUS_AGENT_BLOOM_DEBUG_WINDOW_HEIGHT:-1000}
WINDOW_X=${NEOZEUS_AGENT_BLOOM_DEBUG_WINDOW_X:-40}
WINDOW_Y=${NEOZEUS_AGENT_BLOOM_DEBUG_WINDOW_Y:-40}
INTENSITY=${NEOZEUS_AGENT_BLOOM_INTENSITY:-3.0}

find_window_by_pid_and_title() {
    local pid=$1
    local title=$2
    python - "$pid" "$title" <<'PY'
import json
import subprocess
import sys
import time

pid = int(sys.argv[1])
title = sys.argv[2]
for _ in range(160):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if not isinstance(node, dict):
            continue
        if node.get("pid") == pid and node.get("name") == title:
            print(node["id"])
            sys.exit(0)
        stack.extend(node.get("nodes", []))
        stack.extend(node.get("floating_nodes", []))
    time.sleep(0.1)
raise SystemExit(1)
PY
}

cargo build >/tmp/neozeus-agent-bloom-debug-build.log 2>&1

NEOZEUS_WINDOW_TITLE="$TITLE" \
NEOZEUS_WINDOW_MODE=windowed \
NEOZEUS_WINDOW_SCALE_FACTOR=1.0 \
NEOZEUS_AGENT_BLOOM_DEBUG_PREVIEW=1 \
NEOZEUS_AGENT_BLOOM_INTENSITY="$INTENSITY" \
nohup "$APP" >"$RUN_LOG" 2>&1 </dev/null &
APP_PID=$!

con_id=$(find_window_by_pid_and_title "$APP_PID" "$TITLE")
swaymsg "[con_id=${con_id}] move container to workspace number ${WORKSPACE}" >/dev/null
swaymsg "[con_id=${con_id}] floating enable" >/dev/null
swaymsg "[con_id=${con_id}] resize set width ${WINDOW_WIDTH} px height ${WINDOW_HEIGHT} px" >/dev/null
swaymsg "[con_id=${con_id}] move position ${WINDOW_X} px ${WINDOW_Y} px" >/dev/null

printf 'workspace=%s\n' "$WORKSPACE"
printf 'title=%s\n' "$TITLE"
printf 'pid=%s\n' "$APP_PID"
printf 'log=%s\n' "$RUN_LOG"
printf 'intensity=%s\n' "$INTENSITY"
printf 'debug previews: source | blur | composite contribution\n'
printf 'kill: kill %s\n' "$APP_PID"
