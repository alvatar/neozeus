#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./neozeus-safe.sh
source "$ROOT_DIR/scripts/gui/neozeus-safe.sh"

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
        neozeus_gui_cleanup_pid "$APP_PID"
        APP_PID=
    fi
    neozeus_gui_cleanup_isolated_app_env
}
trap cleanup EXIT

cargo build >"$BUILD_LOG" 2>&1
rm -f "$RUN_LOG" "$DEBUG_LOG" "$BEFORE1" "$BEFORE2" "$AFTER" "$DIFF"

WINDOW_TITLE="neozeus-autoverify-$$"
AUTOVERIFY_COMMAND='clear; for i in $(seq 1 24); do echo "__NZ_AUTOVERIFY__$i"; done'
neozeus_gui_prepare_isolated_app_env "neozeus-visible"
APP_PID=$(neozeus_gui_launch_isolated \
    "$APP" \
    "$RUN_LOG" \
    __NV_PRIME_RENDER_OFFLOAD=1 \
    __VK_LAYER_NV_optimus=NVIDIA_only \
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json \
    WGPU_ADAPTER_NAME=nvidia \
    NEOZEUS_WINDOW_TITLE="$WINDOW_TITLE" \
    NEOZEUS_AUTOVERIFY_COMMAND="$AUTOVERIFY_COMMAND" \
    NEOZEUS_AUTOVERIFY_DELAY_MS=5000)

GUI_WORKSPACE=${NEOZEUS_GUI_WORKSPACE:-8}
CAPTURE_SCALE=$(neozeus_gui_workspace_output_scale "$GUI_WORKSPACE")

WINDOW_JSON=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "$WINDOW_TITLE")

CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")

neozeus_gui_place_window "$CON_ID" "$GUI_WORKSPACE" 1400 900 40 40
neozeus_gui_focus_workspace "$GUI_WORKSPACE"
sleep 1.0
WINDOW_JSON=$(neozeus_gui_wait_for_visible_con_id "$CON_ID")

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
    neozeus_gui_grim_capture "$CAPTURE_SCALE" "$GEOM" "$BEFORE1"
    sleep 0.7
    neozeus_gui_grim_capture "$CAPTURE_SCALE" "$GEOM" "$BEFORE2"
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
neozeus_gui_grim_capture "$CAPTURE_SCALE" "$GEOM" "$AFTER"

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
