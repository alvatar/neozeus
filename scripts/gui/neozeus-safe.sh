#!/usr/bin/env bash
set -euo pipefail

neozeus_gui_find_window_by_pid_and_title() {
    local pid=$1
    local title=$2
    python - "$pid" "$title" <<'PY'
import json
import subprocess
import sys
import time

pid = int(sys.argv[1])
title = sys.argv[2]
for _ in range(120):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if not isinstance(node, dict):
            continue
        if node.get("pid") == pid and node.get("name") == title:
            rect = node["rect"]
            print(json.dumps({
                "id": node["id"],
                "x": rect["x"],
                "y": rect["y"],
                "width": rect["width"],
                "height": rect["height"],
                "focused": bool(node.get("focused")),
                "visible": bool(node.get("visible")),
            }))
            sys.exit(0)
        stack.extend(node.get("nodes", []))
        stack.extend(node.get("floating_nodes", []))
    time.sleep(0.25)
raise SystemExit(1)
PY
}

neozeus_gui_find_window_by_con_id() {
    local con_id=$1
    python - "$con_id" <<'PY'
import json
import subprocess
import sys
import time

con_id = int(sys.argv[1])
for _ in range(80):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if not isinstance(node, dict):
            continue
        if node.get("id") == con_id:
            rect = node["rect"]
            print(json.dumps({
                "id": node["id"],
                "x": rect["x"],
                "y": rect["y"],
                "width": rect["width"],
                "height": rect["height"],
                "focused": bool(node.get("focused")),
                "visible": bool(node.get("visible")),
            }))
            sys.exit(0)
        stack.extend(node.get("nodes", []))
        stack.extend(node.get("floating_nodes", []))
    time.sleep(0.1)
raise SystemExit(1)
PY
}

neozeus_gui_place_window() {
    local con_id=$1
    local workspace=${2:-8}
    local width=${3:-1400}
    local height=${4:-900}
    local x=${5:-40}
    local y=${6:-40}
    swaymsg "[con_id=${con_id}] move container to workspace number ${workspace}" >/dev/null
    swaymsg "[con_id=${con_id}] floating enable" >/dev/null
    swaymsg "[con_id=${con_id}] resize set width ${width} px height ${height} px" >/dev/null
    swaymsg "[con_id=${con_id}] move position ${x} px ${y} px" >/dev/null
}

neozeus_gui_cleanup_pid() {
    local pid=${1:-}
    if [[ -z "$pid" ]]; then
        return 0
    fi
    kill "$pid" 2>/dev/null || true
    for _ in $(seq 1 20); do
        if ! kill -0 "$pid" 2>/dev/null; then
            break
        fi
        sleep 0.2
    done
    if kill -0 "$pid" 2>/dev/null; then
        kill -9 "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
}
