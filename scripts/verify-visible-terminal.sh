#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
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

__NV_PRIME_RENDER_OFFLOAD=1 \
__VK_LAYER_NV_optimus=NVIDIA_only \
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json \
WGPU_ADAPTER_NAME=nvidia \
NEOZEUS_AUTOVERIFY_COMMAND='printf "__NZ_AUTOVERIFY__\n"' \
NEOZEUS_AUTOVERIFY_DELAY_MS=5000 \
nohup "$APP" >"$RUN_LOG" 2>&1 </dev/null &
APP_PID=$!

WINDOW_JSON=$(python - "$APP_PID" <<'PY'
import json
import subprocess
import sys
import time

pid = int(sys.argv[1])
for _ in range(80):
    tree = json.loads(subprocess.check_output(["swaymsg", "-t", "get_tree"]))
    stack = [tree]
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            if node.get("pid") == pid:
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
X=$(jq -r '.x' <<<"$WINDOW_JSON")
Y=$(jq -r '.y' <<<"$WINDOW_JSON")
WIDTH=$(jq -r '.width' <<<"$WINDOW_JSON")
HEIGHT=$(jq -r '.height' <<<"$WINDOW_JSON")

# Exclude the egui toolbar/debug strip at the top; verify the terminal plane region only.
CROP_Y=$((Y + 120))
CROP_H=$((HEIGHT - 120))
GEOM="${X},${CROP_Y} ${WIDTH}x${CROP_H}"

wait_for_log_quiet "$DEBUG_LOG"
swaymsg "[con_id=${CON_ID}] focus" >/dev/null
sleep 0.5
grim -g "$GEOM" "$BEFORE1"
sleep 0.7
grim -g "$GEOM" "$BEFORE2"

for _ in $(seq 1 30); do
    grep -q 'auto-verify command dispatched' "$DEBUG_LOG" 2>/dev/null && break
    sleep 0.3
done
wait_for_log_quiet "$DEBUG_LOG"
grim -g "$GEOM" "$AFTER"

BASELINE_RAW=$(metric_ae "$BEFORE1" "$BEFORE2")
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
grep -q 'pty write command `printf "__NZ_AUTOVERIFY__\\n"`' "$DEBUG_LOG"

echo "visible terminal verification: PASS"
echo "artifacts: $BEFORE1 $BEFORE2 $AFTER $DIFF $RUN_LOG $DEBUG_LOG"
