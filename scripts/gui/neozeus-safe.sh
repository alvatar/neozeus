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

neozeus_gui_wait_for_visible_con_id() {
    local con_id=$1
    local window_json
    for _ in $(seq 1 80); do
        window_json=$(neozeus_gui_find_window_by_con_id "$con_id") || true
        if [[ -n "$window_json" ]] && jq -e '.visible == true' >/dev/null <<<"$window_json"; then
            printf '%s\n' "$window_json"
            return 0
        fi
        sleep 0.1
    done
    return 1
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

neozeus_gui_focus_workspace() {
    local workspace=${1:-8}
    swaymsg "workspace number ${workspace}" >/dev/null
}

neozeus_gui_workspace_output() {
    local workspace=${1:-8}
    swaymsg -t get_workspaces | jq -r --argjson workspace "$workspace" '
        .[] | select(.num == $workspace) | .output
    ' | head -n 1
}

neozeus_gui_output_scale() {
    local output_name=${1:-}
    if [[ -z "$output_name" ]]; then
        echo 1.0
        return 0
    fi
    local scale
    scale=$(swaymsg -t get_outputs | jq -r --arg output "$output_name" '
        .[] | select(.name == $output) | .scale
    ' | head -n 1)
    if [[ -z "$scale" || "$scale" == "null" ]]; then
        echo 1.0
        return 0
    fi
    echo "$scale"
}

neozeus_gui_workspace_output_scale() {
    local workspace=${1:-8}
    local output_name
    output_name=$(neozeus_gui_workspace_output "$workspace")
    neozeus_gui_output_scale "$output_name"
}

neozeus_gui_grim_capture() {
    local scale=$1
    local geom=$2
    local out=$3
    local physical_geom
    physical_geom=$(python - "$scale" "$geom" <<'PY'
import math
import re
import sys

scale = float(sys.argv[1])
geom = sys.argv[2]
match = re.fullmatch(r"\s*(\d+)\s*,\s*(\d+)\s+(\d+)x(\d+)\s*", geom)
if not match:
    raise SystemExit(f"invalid grim geometry: {geom!r}")
x, y, w, h = (int(value) for value in match.groups())
px = int(round(x * scale))
py = int(round(y * scale))
pw = int(round(w * scale))
ph = int(round(h * scale))
print(f"{px},{py} {pw}x{ph}")
PY
)
    timeout 10s grim -g "$physical_geom" "$out"
}

neozeus_gui_prepare_isolated_app_env() {
    local prefix=${1:-neozeus-gui}
    NEOZEUS_GUI_ISO_ROOT=$(mktemp -d "/tmp/${prefix}-XXXXXX")
    NEOZEUS_GUI_ISO_HOME="$NEOZEUS_GUI_ISO_ROOT/home"
    NEOZEUS_GUI_ISO_XDG_CONFIG_HOME="$NEOZEUS_GUI_ISO_ROOT/xdg-config"
    NEOZEUS_GUI_ISO_XDG_STATE_HOME="$NEOZEUS_GUI_ISO_ROOT/xdg-state"
    NEOZEUS_GUI_ISO_XDG_CACHE_HOME="$NEOZEUS_GUI_ISO_ROOT/xdg-cache"
    NEOZEUS_GUI_ISO_ZDOTDIR="$NEOZEUS_GUI_ISO_ROOT/zdotdir"
    NEOZEUS_GUI_ISO_KITTY_DIR="$NEOZEUS_GUI_ISO_ROOT/kitty"
    NEOZEUS_GUI_ISO_HISTFILE="$NEOZEUS_GUI_ISO_ROOT/history"
    NEOZEUS_GUI_ISO_ZSHENV="$NEOZEUS_GUI_ISO_ZDOTDIR/.zshenv"
    NEOZEUS_GUI_ISO_DAEMON_SOCKET="$NEOZEUS_GUI_ISO_ROOT/daemon.sock"

    mkdir -p \
        "$NEOZEUS_GUI_ISO_HOME" \
        "$NEOZEUS_GUI_ISO_XDG_CONFIG_HOME" \
        "$NEOZEUS_GUI_ISO_XDG_STATE_HOME" \
        "$NEOZEUS_GUI_ISO_XDG_CACHE_HOME" \
        "$NEOZEUS_GUI_ISO_ZDOTDIR" \
        "$NEOZEUS_GUI_ISO_KITTY_DIR"
    : >"$NEOZEUS_GUI_ISO_ZSHENV"
}

neozeus_gui_cleanup_isolated_app_env() {
    if [[ -n "${NEOZEUS_GUI_ISO_ROOT:-}" ]]; then
        neozeus_gui_cleanup_daemon_for_socket "${NEOZEUS_GUI_ISO_DAEMON_SOCKET:-}"
        rm -rf "$NEOZEUS_GUI_ISO_ROOT"
        unset \
            NEOZEUS_GUI_ISO_ROOT \
            NEOZEUS_GUI_ISO_HOME \
            NEOZEUS_GUI_ISO_XDG_CONFIG_HOME \
            NEOZEUS_GUI_ISO_XDG_STATE_HOME \
            NEOZEUS_GUI_ISO_XDG_CACHE_HOME \
            NEOZEUS_GUI_ISO_ZDOTDIR \
            NEOZEUS_GUI_ISO_KITTY_DIR \
            NEOZEUS_GUI_ISO_HISTFILE \
            NEOZEUS_GUI_ISO_ZSHENV \
            NEOZEUS_GUI_ISO_DAEMON_SOCKET
    fi
}

neozeus_gui_launch_isolated() {
    local app=$1
    local run_log=$2
    shift 2
    env \
        HOME="$NEOZEUS_GUI_ISO_HOME" \
        XDG_CONFIG_HOME="$NEOZEUS_GUI_ISO_XDG_CONFIG_HOME" \
        XDG_STATE_HOME="$NEOZEUS_GUI_ISO_XDG_STATE_HOME" \
        XDG_CACHE_HOME="$NEOZEUS_GUI_ISO_XDG_CACHE_HOME" \
        ZDOTDIR="$NEOZEUS_GUI_ISO_ZDOTDIR" \
        ZSHENV="$NEOZEUS_GUI_ISO_ZSHENV" \
        HISTFILE="$NEOZEUS_GUI_ISO_HISTFILE" \
        KITTY_CONFIG_DIRECTORY="$NEOZEUS_GUI_ISO_KITTY_DIR" \
        SHELL="/bin/sh" \
        BASH_ENV="/dev/null" \
        ENV="/dev/null" \
        NEOZEUS_DAEMON_SOCKET_PATH="$NEOZEUS_GUI_ISO_DAEMON_SOCKET" \
        "$@" \
        nohup "$app" >"$run_log" 2>&1 </dev/null &
    echo $!
}

neozeus_gui_cleanup_daemon_for_socket() {
    local socket_path=${1:-}
    if [[ -z "$socket_path" ]]; then
        return 0
    fi
    python - "$socket_path" <<'PY' | while read -r daemon_pid; do
import subprocess
import sys

socket_path = sys.argv[1]
for line in subprocess.check_output(["ps", "-eo", "pid=,args="]).decode().splitlines():
    parts = line.strip().split(None, 1)
    if len(parts) != 2:
        continue
    pid, args = parts
    if " neozeus daemon --socket " not in f" {args} ":
        continue
    if f"--socket {socket_path}" not in args:
        continue
    print(pid)
PY
        kill "$daemon_pid" 2>/dev/null || true
        for _ in $(seq 1 20); do
            if ! kill -0 "$daemon_pid" 2>/dev/null; then
                break
            fi
            sleep 0.2
        done
        if kill -0 "$daemon_pid" 2>/dev/null; then
            kill -9 "$daemon_pid" 2>/dev/null || true
        fi
        wait "$daemon_pid" 2>/dev/null || true
    done
}

neozeus_gui_cleanup_pid() {
    local pid=${1:-}
    if [[ -z "$pid" ]]; then
        neozeus_gui_cleanup_daemon_for_socket "${NEOZEUS_GUI_ISO_DAEMON_SOCKET:-}"
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
    neozeus_gui_cleanup_daemon_for_socket "${NEOZEUS_GUI_ISO_DAEMON_SOCKET:-}"
}
