#!/usr/bin/env bash
# Experimental manual helper for compositor-driven HUD interaction checks.
# Not part of the supported GUI suite because synthetic pointer motion is compositor/session sensitive.
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-hud-build.log
RUN_LOG=/tmp/neozeus-hud-run.log
DEBUG_LOG=/tmp/neozeus-debug.log
HUD_CONFIG_ROOT=/tmp/neozeus-hud-config-$$
HUD_LAYOUT_PATH="$HUD_CONFIG_ROOT/neozeus/hud-layout.v1"
WINDOW_TITLE="neozeus-hud-verify-$$"
INITIAL_WINDOW=/tmp/neozeus-hud-initial.png
TOOLBAR_DRAGGED=/tmp/neozeus-hud-toolbar-dragged.png
TOOLBAR_SETTLED=/tmp/neozeus-hud-toolbar-settled.png
AGENTLIST_DRAGGED=/tmp/neozeus-hud-agentlist-dragged.png
AGENTLIST_SETTLED=/tmp/neozeus-hud-agentlist-settled.png
PRE_ISOLATE_WINDOW=/tmp/neozeus-hud-pre-isolate.png
ISOLATED_WINDOW=/tmp/neozeus-hud-isolated.png
SHOW_ALL_WINDOW=/tmp/neozeus-hud-show-all.png
TYPING_BEFORE=/tmp/neozeus-hud-typing-before.png
TYPING_AFTER=/tmp/neozeus-hud-typing-after.png
DIFF_ISOLATE=/tmp/neozeus-hud-isolate-diff.png
DIFF_TYPING=/tmp/neozeus-hud-typing-diff.png

TOOLBAR_X=180
TOOLBAR_Y=24
AGENTLIST_X=960
AGENTLIST_Y=140

metric_ae() {
    local lhs=$1
    local rhs=$2
    compare -metric AE "$lhs" "$rhs" null: 2>&1 >/dev/null || true
}

parse_metric() {
    python - "$1" <<'PY'
import re
import sys
value = sys.argv[1]
match = re.search(r"[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?", value, re.IGNORECASE)
if not match:
    raise SystemExit(f"failed to parse metric: {value!r}")
print(match.group(0))
PY
}

wait_for_log_quiet() {
    local path=$1
    local stable=0
    local last_size=-1
    for _ in $(seq 1 60); do
        local size=0
        if [[ -f "$path" ]]; then
            size=$(stat -c %s "$path")
        fi
        if [[ "$size" -eq "$last_size" ]]; then
            stable=$((stable + 1))
        else
            stable=0
            last_size=$size
        fi
        if [[ "$stable" -ge 3 ]]; then
            return 0
        fi
        sleep 0.25
    done
    return 1
}

wait_for_log_pattern() {
    local pattern=$1
    local timeout=${2:-40}
    for _ in $(seq 1 "$timeout"); do
        if grep -q "$pattern" "$DEBUG_LOG" 2>/dev/null; then
            return 0
        fi
        sleep 0.25
    done
    return 1
}

log_count_fixed() {
    local pattern=$1
    rg -F -c "$pattern" "$DEBUG_LOG" 2>/dev/null || echo 0
}

wait_for_log_count_gt() {
    local pattern=$1
    local baseline=$2
    local timeout=${3:-40}
    for _ in $(seq 1 "$timeout"); do
        local count
        count=$(log_count_fixed "$pattern")
        if [[ "$count" -gt "$baseline" ]]; then
            return 0
        fi
        sleep 0.25
    done
    return 1
}

find_window_json() {
    python - "$APP_PID" "$WINDOW_TITLE" <<'PY'
import json
import subprocess
import sys
import time

pid = int(sys.argv[1])
window_title = sys.argv[2]
for _ in range(80):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            if node.get("pid") == pid and node.get("name") == window_title:
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

sys.exit(1)
PY
}

refresh_window_json() {
    python - "$CON_ID" <<'PY'
import json
import subprocess
import sys
import time

con_id = int(sys.argv[1])
for _ in range(40):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
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

sys.exit(1)
PY
}

window_field() {
    local field=$1
    jq -r ".$field" <<<"$WINDOW_JSON"
}

focus_window() {
    swaymsg "[con_id=${CON_ID}] move container to workspace current" >/dev/null
    swaymsg "[con_id=${CON_ID}] focus" >/dev/null
    for _ in $(seq 1 20); do
        WINDOW_JSON=$(refresh_window_json)
        if [[ $(window_field focused) == "true" && $(window_field visible) == "true" ]]; then
            return 0
        fi
        swaymsg "[con_id=${CON_ID}] focus" >/dev/null
        sleep 0.15
    done
    return 1
}

window_capture() {
    local path=$1
    grim -g "$(window_field x),$(window_field y) $(window_field width)x$(window_field height)" "$path"
}

terminal_capture() {
    local path=$1
    local x y width height crop_y crop_h crop_x crop_w
    x=$(window_field x)
    y=$(window_field y)
    width=$(window_field width)
    height=$(window_field height)
    crop_x=$((x + 340))
    crop_y=$((y + 180))
    crop_w=$((width - 420))
    crop_h=$((height - 240))
    grim -g "${crop_x},${crop_y} ${crop_w}x${crop_h}" "$path"
}

cursor_set_local() {
    local local_x=$1
    local local_y=$2
    local abs_x abs_y
    abs_x=$(( $(window_field x) + local_x ))
    abs_y=$(( $(window_field y) + local_y ))
    swaymsg "seat seat0 cursor set ${abs_x} ${abs_y}" >/dev/null
}

click_local() {
    local local_x=$1
    local local_y=$2
    cursor_set_local "$local_x" "$local_y"
    sleep 0.1
    swaymsg 'seat seat0 cursor press button1' >/dev/null
    sleep 0.05
    swaymsg 'seat seat0 cursor release button1' >/dev/null
    sleep 0.25
}

drag_local() {
    local start_x=$1
    local start_y=$2
    local end_x=$3
    local end_y=$4
    local delta_x delta_y
    delta_x=$((end_x - start_x))
    delta_y=$((end_y - start_y))
    cursor_set_local "$start_x" "$start_y"
    sleep 0.1
    swaymsg 'seat seat0 cursor press button1' >/dev/null
    sleep 0.2
    swaymsg "seat seat0 cursor move ${delta_x} ${delta_y}" >/dev/null
    sleep 0.2
    swaymsg 'seat seat0 cursor release button1' >/dev/null
}

button_center() {
    python - "$1" "$2" "$3" <<'PY'
import sys
module_x = float(sys.argv[1])
module_y = float(sys.argv[2])
label = sys.argv[3]
buttons = [
    "new terminal",
    "show all",
    "pixel perfect",
    "reset view",
    "pwd",
    "ls",
    "clear",
    "btop",
    "tmux",
    "0 toolbar",
    "1 agents",
]
content_x = module_x
content_y = module_y
cursor_x = content_x + 10.0
for current in buttons:
    width = max(72.0, len(current) * 8.0 + 20.0)
    if current == label:
        print(f"{cursor_x + width * 0.5:.0f} {content_y + 10.0 + 14.0:.0f}")
        sys.exit(0)
    cursor_x += width + 8.0
raise SystemExit(f"unknown button {label!r}")
PY
}

agent_row_center() {
    python - "$1" "$2" "$3" <<'PY'
import sys
module_x = float(sys.argv[1])
module_y = float(sys.argv[2])
row_index = int(sys.argv[3])
content_x = module_x + 10.0
content_y = module_y + 10.0
row_h = 28.0
print(f"{content_x + 140.0:.0f} {content_y + row_index * row_h + row_h * 0.5:.0f}")
PY
}

read_layout_rect() {
    python - "$HUD_LAYOUT_PATH" "$1" <<'PY'
import sys
path = sys.argv[1]
module_name = sys.argv[2]
with open(path, 'r', encoding='utf-8') as fh:
    for line in fh:
        parts = line.strip().split()
        if not parts or parts[0] != module_name:
            continue
        fields = {}
        for part in parts[1:]:
            if '=' not in part:
                continue
            key, value = part.split('=', 1)
            fields[key] = value
        print(fields.get('x', 'nan'), fields.get('y', 'nan'))
        sys.exit(0)
raise SystemExit(f"missing module line for {module_name}")
PY
}

assert_layout_close() {
    local module_name=$1
    local expect_x=$2
    local expect_y=$3
    local actual_x actual_y
    read -r actual_x actual_y < <(read_layout_rect "$module_name")
    python - "$module_name" "$actual_x" "$actual_y" "$expect_x" "$expect_y" <<'PY'
import sys
module_name, actual_x, actual_y, expect_x, expect_y = sys.argv[1:]
ax = float(actual_x)
ay = float(actual_y)
ex = float(expect_x)
ey = float(expect_y)
if abs(ax - ex) > 8.0 or abs(ay - ey) > 8.0:
    raise SystemExit(f"{module_name} layout mismatch: got ({ax}, {ay}), expected about ({ex}, {ey})")
PY
}

assert_metric_gt() {
    local lhs=$1
    local rhs=$2
    local threshold=$3
    local diff_path=${4:-}
    local raw metric
    raw=$(metric_ae "$lhs" "$rhs")
    metric=$(parse_metric "$raw")
    if [[ -n "$diff_path" ]]; then
        compare "$lhs" "$rhs" "$diff_path" >/dev/null 2>&1 || true
    fi
    python - "$metric" "$threshold" <<'PY'
import sys
metric = float(sys.argv[1])
threshold = float(sys.argv[2])
raise SystemExit(0 if metric > threshold else 1)
PY
}

launch_app() {
    rm -f "$RUN_LOG" "$DEBUG_LOG"
    __NV_PRIME_RENDER_OFFLOAD=1 \
    __VK_LAYER_NV_optimus=NVIDIA_only \
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json \
    WGPU_ADAPTER_NAME=nvidia \
    XDG_CONFIG_HOME="$HUD_CONFIG_ROOT" \
    NEOZEUS_WINDOW_TITLE="$WINDOW_TITLE" \
    nohup "$APP" >"$RUN_LOG" 2>&1 </dev/null &
    APP_PID=$!
    WINDOW_JSON=$(find_window_json)
    CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")
    focus_window
    sleep 1
    wait_for_log_quiet "$DEBUG_LOG"
}

stop_app() {
    if [[ -n "${APP_PID:-}" ]]; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
        APP_PID=
    fi
}

cleanup() {
    stop_app
}
trap cleanup EXIT

cargo build >"$BUILD_LOG" 2>&1
pkill -f "$APP" 2>/dev/null || true
rm -rf "$HUD_CONFIG_ROOT"
rm -f \
    "$INITIAL_WINDOW" "$TOOLBAR_DRAGGED" "$TOOLBAR_SETTLED" \
    "$AGENTLIST_DRAGGED" "$AGENTLIST_SETTLED" "$PRE_ISOLATE_WINDOW" \
    "$ISOLATED_WINDOW" "$SHOW_ALL_WINDOW" "$TYPING_BEFORE" \
    "$TYPING_AFTER" "$DIFF_ISOLATE" "$DIFF_TYPING"

launch_app
window_capture "$INITIAL_WINDOW"
focus_window

# Drag toolbar and verify visible motion.
drag_local 80 38 $((TOOLBAR_X + 56)) $((TOOLBAR_Y + 14))
window_capture "$TOOLBAR_DRAGGED"
sleep 0.8
window_capture "$TOOLBAR_SETTLED"
assert_metric_gt "$INITIAL_WINDOW" "$TOOLBAR_SETTLED" 5000
focus_window

# Drag agent list and verify visible motion + saved layout.
drag_local 80 118 $((AGENTLIST_X + 56)) $((AGENTLIST_Y + 14))
window_capture "$AGENTLIST_DRAGGED"
sleep 0.8
window_capture "$AGENTLIST_SETTLED"
assert_metric_gt "$TOOLBAR_SETTLED" "$AGENTLIST_SETTLED" 5000
wait_for_log_pattern 'hud: hud layout saved ' 50
for _ in $(seq 1 40); do
    [[ -f "$HUD_LAYOUT_PATH" ]] && break
    sleep 0.1
done
[[ -f "$HUD_LAYOUT_PATH" ]]
assert_layout_close DebugToolbar "$TOOLBAR_X" "$TOOLBAR_Y"
assert_layout_close AgentList "$AGENTLIST_X" "$AGENTLIST_Y"

stop_app
sleep 0.5

launch_app
wait_for_log_quiet "$DEBUG_LOG"

# Persisted toolbar position loads after restart and remains clickable.
read -r pwd_x pwd_y < <(button_center "$TOOLBAR_X" $((TOOLBAR_Y + 28)) "pwd")
pwd_count=$(log_count_fixed 'pty write command `pwd`')
click_local "$pwd_x" "$pwd_y"
wait_for_log_count_gt 'pty write command `pwd`' "$pwd_count" 50
wait_for_log_quiet "$DEBUG_LOG"

read -r new_term_x new_term_y < <(button_center "$TOOLBAR_X" $((TOOLBAR_Y + 28)) "new terminal")
spawn_count=$(log_count_fixed 'spawned terminal 2')
click_local "$new_term_x" "$new_term_y"
wait_for_log_count_gt 'spawned terminal 2' "$spawn_count" 50
wait_for_log_quiet "$DEBUG_LOG"

read -r show_all_x show_all_y < <(button_center "$TOOLBAR_X" $((TOOLBAR_Y + 28)) "show all")
read -r row1_x row1_y < <(agent_row_center "$AGENTLIST_X" $((AGENTLIST_Y + 28)) 0)
read -r row2_x row2_y < <(agent_row_center "$AGENTLIST_X" $((AGENTLIST_Y + 28)) 1)

# Agent list click focuses terminal 1 and isolates presentation.
focus1_count=$(log_count_fixed 'focused terminal 1')
isolate1_count=$(log_count_fixed 'hud visibility isolate 1')
click_local "$row1_x" "$row1_y"
wait_for_log_count_gt 'focused terminal 1' "$focus1_count" 50
wait_for_log_count_gt 'hud visibility isolate 1' "$isolate1_count" 50
wait_for_log_quiet "$DEBUG_LOG"
window_capture "$PRE_ISOLATE_WINDOW"
focus_window

# Show all, then isolate terminal 2 and verify the window changes.
show_all_count=$(log_count_fixed 'hud visibility show-all')
click_local "$show_all_x" "$show_all_y"
wait_for_log_count_gt 'hud visibility show-all' "$show_all_count" 50
wait_for_log_quiet "$DEBUG_LOG"
focus2_count=$(log_count_fixed 'focused terminal 2')
isolate2_count=$(log_count_fixed 'hud visibility isolate 2')
click_local "$row2_x" "$row2_y"
wait_for_log_count_gt 'focused terminal 2' "$focus2_count" 50
wait_for_log_count_gt 'hud visibility isolate 2' "$isolate2_count" 50
wait_for_log_quiet "$DEBUG_LOG"
window_capture "$ISOLATED_WINDOW"
assert_metric_gt "$PRE_ISOLATE_WINDOW" "$ISOLATED_WINDOW" 5000 "$DIFF_ISOLATE"
focus_window

# Show all again, then prove the previously hidden first terminal still exists.
show_all_count=$(log_count_fixed 'hud visibility show-all')
click_local "$show_all_x" "$show_all_y"
wait_for_log_count_gt 'hud visibility show-all' "$show_all_count" 50
wait_for_log_quiet "$DEBUG_LOG"
window_capture "$SHOW_ALL_WINDOW"
focus_window
focus1_count=$(log_count_fixed 'focused terminal 1')
isolate1_count=$(log_count_fixed 'hud visibility isolate 1')
click_local "$row1_x" "$row1_y"
wait_for_log_count_gt 'focused terminal 1' "$focus1_count" 50
wait_for_log_count_gt 'hud visibility isolate 1' "$isolate1_count" 50
wait_for_log_quiet "$DEBUG_LOG"

# Typing still repaints after HUD interactions.
terminal_capture "$TYPING_BEFORE"
focus_window
key_count=$(log_count_fixed 'key event:')
wtype 'echo __HUD_TYPING__'
wtype -k Return
wait_for_log_count_gt 'key event:' "$key_count" 50
wait_for_log_quiet "$DEBUG_LOG"
terminal_capture "$TYPING_AFTER"
assert_metric_gt "$TYPING_BEFORE" "$TYPING_AFTER" 1000 "$DIFF_TYPING"

echo "hud workflow verification: PASS"
echo "artifacts: $INITIAL_WINDOW $TOOLBAR_DRAGGED $TOOLBAR_SETTLED $AGENTLIST_DRAGGED $AGENTLIST_SETTLED $PRE_ISOLATE_WINDOW $ISOLATED_WINDOW $SHOW_ALL_WINDOW $TYPING_BEFORE $TYPING_AFTER $DIFF_ISOLATE $DIFF_TYPING $HUD_LAYOUT_PATH $RUN_LOG $DEBUG_LOG"
