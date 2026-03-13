#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-autoverify-build.log
RUN_LOG=/tmp/neozeus-autoverify-run.log
DEBUG_LOG=/tmp/neozeus-debug.log
BEFORE1=/tmp/neozeus-autoverify-before-1.png
BEFORE2=/tmp/neozeus-autoverify-before-2.png
AFTER=/tmp/neozeus-autoverify-after.png
DIFF=/tmp/neozeus-autoverify-diff.png

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
    for _ in $(seq 1 40); do
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
        sleep 0.4
    done
    return 1
}

cleanup() {
    if [[ -n "${APP_PID:-}" ]]; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT

cargo build >"$BUILD_LOG" 2>&1
pkill -f "$APP" 2>/dev/null || true
rm -f "$RUN_LOG" "$DEBUG_LOG" "$BEFORE1" "$BEFORE2" "$AFTER" "$DIFF"

WINDOW_TITLE="neozeus-autoverify-$$"
AUTOVERIFY_COMMAND='clear; for i in $(seq 1 24); do echo "__NZ_AUTOVERIFY__$i"; done'
__NV_PRIME_RENDER_OFFLOAD=1 \
__VK_LAYER_NV_optimus=NVIDIA_only \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json \
WGPU_ADAPTER_NAME=nvidia \
NEOZEUS_WINDOW_TITLE="$WINDOW_TITLE" \
NEOZEUS_AUTOVERIFY_COMMAND="$AUTOVERIFY_COMMAND" \
NEOZEUS_AUTOVERIFY_DELAY_MS=5000 \
nohup "$APP" >"$RUN_LOG" 2>&1 </dev/null &
APP_PID=$!

WINDOW_JSON=$(python - "$APP_PID" "$WINDOW_TITLE" <<'PY'
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
                }))
                sys.exit(0)
            stack.extend(node.get("nodes", []))
            stack.extend(node.get("floating_nodes", []))
    time.sleep(0.25)

sys.exit(1)
PY
)

CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")

# Make the target window visible on the currently focused workspace/output.
swaymsg "[con_id=${CON_ID}] move container to workspace current" >/dev/null
swaymsg "[con_id=${CON_ID}] focus" >/dev/null

WINDOW_JSON=$(python - "$CON_ID" <<'PY'
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
    time.sleep(0.25)

sys.exit(1)
PY
)

for _ in $(seq 1 20); do
    FOCUSED=$(jq -r '.focused' <<<"$WINDOW_JSON")
    VISIBLE=$(jq -r '.visible' <<<"$WINDOW_JSON")
    if [[ "$FOCUSED" == "true" && "$VISIBLE" == "true" ]]; then
        break
    fi
    swaymsg "[con_id=${CON_ID}] focus" >/dev/null
    sleep 0.25
    WINDOW_JSON=$(python - "$CON_ID" <<'PY'
import json
import subprocess
import sys
import time

con_id = int(sys.argv[1])
for _ in range(20):
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
)
done

X=$(jq -r '.x' <<<"$WINDOW_JSON")
Y=$(jq -r '.y' <<<"$WINDOW_JSON")
WIDTH=$(jq -r '.width' <<<"$WINDOW_JSON")
HEIGHT=$(jq -r '.height' <<<"$WINDOW_JSON")

# Exclude the egui toolbar/debug strip at the top; verify the terminal plane region only.
CROP_Y=$((Y + 120))
CROP_H=$((HEIGHT - 120))
GEOM="${X},${CROP_Y} ${WIDTH}x${CROP_H}"

BASELINE_RAW=
for _ in $(seq 1 5); do
    wait_for_log_quiet "$DEBUG_LOG"
    grim -g "$GEOM" "$BEFORE1"
    sleep 0.7
    grim -g "$GEOM" "$BEFORE2"
    BASELINE_RAW=$(metric_ae "$BEFORE1" "$BEFORE2")
    BASELINE=$(parse_metric "$BASELINE_RAW")
    if python - "$BASELINE" <<'PY'
import sys
raise SystemExit(0 if float(sys.argv[1]) <= 20000.0 else 1)
PY
    then
        break
    fi
    sleep 1
    BASELINE_RAW=
 done

if [[ -z "$BASELINE_RAW" ]]; then
    echo "failed to capture stable baseline for visible verifier" >&2
    exit 1
fi

BASE_UPLOADS=$(rg -c 'texture sync: image uploaded' "$DEBUG_LOG" 2>/dev/null || echo 0)

for _ in $(seq 1 30); do
    grep -q 'auto-verify command dispatched' "$DEBUG_LOG" 2>/dev/null && break
    sleep 0.3
done
sleep 3
for _ in $(seq 1 40); do
    CURRENT_UPLOADS=$(rg -c 'texture sync: image uploaded' "$DEBUG_LOG" 2>/dev/null || echo 0)
    if [[ "$CURRENT_UPLOADS" -gt "$BASE_UPLOADS" ]]; then
        break
    fi
    sleep 0.25
done
wait_for_log_quiet "$DEBUG_LOG"
grim -g "$GEOM" "$AFTER"

POST_RAW=$(metric_ae "$BEFORE2" "$AFTER")
compare "$BEFORE2" "$AFTER" "$DIFF" >/dev/null 2>&1 || true

python - "$BASELINE_RAW" "$POST_RAW" <<'PY'
import re
import sys

def parse_metric(value: str) -> float:
    match = re.search(r"[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?", value, re.IGNORECASE)
    if not match:
        raise SystemExit(f"failed to parse metric: {value!r}")
    return float(match.group(0))

baseline = parse_metric(sys.argv[1])
post = parse_metric(sys.argv[2])
threshold = max(20000.0, baseline * 3.0)
print(f"baseline_ae={baseline:.0f}")
print(f"post_ae={post:.0f}")
print(f"threshold={threshold:.0f}")
if post <= threshold:
    sys.exit(1)
PY

grep -q 'auto-verify command dispatched' "$DEBUG_LOG"
grep -q 'pty write command `clear; for i in $(seq 1 24); do echo "__NZ_AUTOVERIFY__$i"; done`' "$DEBUG_LOG"

echo "visible terminal verification: PASS"
echo "artifacts: $BEFORE1 $BEFORE2 $AFTER $DIFF $RUN_LOG $DEBUG_LOG"
