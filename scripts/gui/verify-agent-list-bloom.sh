#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./neozeus-safe.sh
source "$ROOT_DIR/scripts/gui/neozeus-safe.sh"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-bloom-build.log
RUN_LOG=/tmp/neozeus-bloom-run.log
OFF_PNG=/tmp/neozeus-agent-bloom-off.png
ON_PNG=/tmp/neozeus-agent-bloom-on.png
STABLE_A=/tmp/neozeus-agent-bloom-stable-a.png
STABLE_B=/tmp/neozeus-agent-bloom-stable-b.png
ANALYSIS_JSON=/tmp/neozeus-agent-bloom-analysis.json
ON_INTENSITY=${NEOZEUS_AGENT_BLOOM_VERIFY_ON_INTENSITY:-0.35}
SWAY_WORKSPACE=${NEOZEUS_AGENT_BLOOM_WORKSPACE:-8}

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

capture_stable_window() {
    local x=$1
    local y=$2
    local width=$3
    local height=$4
    local out=$5
    local capture_scale=$6
    local geom="${x},${y} ${width}x${height}"

    for _ in $(seq 1 20); do
        grim -s "$capture_scale" -g "$geom" "$STABLE_A"
        sleep 0.5
        grim -s "$capture_scale" -g "$geom" "$STABLE_B"
        local diff_raw
        diff_raw=$(metric_ae "$STABLE_A" "$STABLE_B")
        local diff
        diff=$(parse_metric "$diff_raw")
        if python - "$diff" <<'PY'
import sys
raise SystemExit(0 if float(sys.argv[1]) <= 500.0 else 1)
PY
        then
            mv "$STABLE_B" "$out"
            return 0
        fi
        sleep 0.4
    done

    mv "$STABLE_B" "$out"
}

cleanup_app() {
    if [[ -n "${APP_PID:-}" ]]; then
        neozeus_gui_cleanup_pid "$APP_PID"
        APP_PID=
    fi
    neozeus_gui_cleanup_isolated_app_env
}

trap cleanup_app EXIT

run_capture() {
    local tag=$1
    local intensity=$2
    local out_png=$3
    local window_scale=$4
    local capture_scale=$5
    local title="neozeus-bloom-${tag}-$$"

    cleanup_app
    rm -f "$RUN_LOG" "$STABLE_A" "$STABLE_B" "$out_png"

    neozeus_gui_prepare_isolated_app_env "neozeus-bloom-${tag}"
    APP_PID=$(neozeus_gui_launch_isolated \
        "$APP" \
        "$RUN_LOG" \
        NEOZEUS_WINDOW_TITLE="$title" \
        NEOZEUS_WINDOW_MODE=windowed \
        NEOZEUS_WINDOW_SCALE_FACTOR="$window_scale" \
        NEOZEUS_AGENT_BLOOM_INTENSITY="$intensity" \
        NEOZEUS_AUTOVERIFY_COMMAND='printf "__NZ_BLOOM__\n"' \
        NEOZEUS_AUTOVERIFY_DELAY_MS=400)

    local window_json
    window_json=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "$title")
    local con_id
    con_id=$(jq -r '.id' <<<"$window_json")

    neozeus_gui_place_window "$con_id" "$SWAY_WORKSPACE" 1400 900 40 40
    sleep 1.2

    window_json=$(neozeus_gui_find_window_by_con_id "$con_id")
    local x y width height capture_w capture_h
    x=$(jq -r '.x' <<<"$window_json")
    y=$(jq -r '.y' <<<"$window_json")
    width=$(jq -r '.width' <<<"$window_json")
    height=$(jq -r '.height' <<<"$window_json")
    capture_w=$(( width < 340 ? width : 340 ))
    capture_h=$(( height < 180 ? height : 180 ))

    capture_stable_window "$x" "$y" "$capture_w" "$capture_h" "$out_png" "$capture_scale"
}

cargo build >"$BUILD_LOG" 2>&1

WINDOW_SCALE=$(neozeus_gui_workspace_output_scale "$SWAY_WORKSPACE")
CAPTURE_SCALE="$WINDOW_SCALE"

run_capture off 0.0 "$OFF_PNG" "$WINDOW_SCALE" "$CAPTURE_SCALE"
run_capture on "$ON_INTENSITY" "$ON_PNG" "$WINDOW_SCALE" "$CAPTURE_SCALE"
cleanup_app

python - "$OFF_PNG" "$ON_PNG" "$ANALYSIS_JSON" <<'PY'
import json
import sys
from pathlib import Path
from PIL import Image

off_path = Path(sys.argv[1])
on_path = Path(sys.argv[2])
out_path = Path(sys.argv[3])

off = Image.open(off_path).convert("RGBA")
on = Image.open(on_path).convert("RGBA")
logical_size = (340, 180)
if off.size != logical_size:
    off = off.resize(logical_size, Image.Resampling.LANCZOS)
if on.size != logical_size:
    on = on.resize(logical_size, Image.Resampling.LANCZOS)

width, height = off.size
shell_w = min(300, width)
content_x = 20 + 1
content_y = 10 + 52
row_w = max(shell_w - 20 - 3, 0)
row_h = 28
main = (content_x, content_y + 2, max(row_w - 12 - 10, 12), max(row_h - 4, 10))
marker = (content_x + row_w - 12, content_y + 2, 12, max(row_h - 4, 10))
rects = [main, marker]
outer = 12
inner = 1
center_box = (width // 2 - 40, height // 2 - 16, 80, 32)

def in_rect(x, y, rect):
    rx, ry, rw, rh = rect
    return rx <= x < rx + rw and ry <= y < ry + rh

def in_expanded(x, y, rect, amount):
    rx, ry, rw, rh = rect
    return rx - amount <= x < rx + rw + amount and ry - amount <= y < ry + rh + amount

def pixel_delta(a, b):
    return sum(abs(int(a[i]) - int(b[i])) for i in range(3)) / 3.0

ring_values = []
inside_values = []
control_values = []
for y in range(height):
    for x in range(width):
        off_px = off.getpixel((x, y))
        on_px = on.getpixel((x, y))
        delta = pixel_delta(off_px, on_px)
        in_any_inner = any(in_expanded(x, y, rect, inner) for rect in rects)
        in_any_outer = any(in_expanded(x, y, rect, outer) for rect in rects)
        in_any_rect = any(in_rect(x, y, rect) for rect in rects)
        if in_any_outer and not in_any_inner:
            ring_values.append(delta)
        if in_any_rect:
            inside_values.append(delta)
        if in_rect(x, y, center_box):
            control_values.append(delta)

result = {
    "window": {"width": width, "height": height},
    "main_rect": {"x": main[0], "y": main[1], "w": main[2], "h": main[3]},
    "marker_rect": {"x": marker[0], "y": marker[1], "w": marker[2], "h": marker[3]},
    "ring_mean": sum(ring_values) / max(len(ring_values), 1),
    "ring_max": max(ring_values) if ring_values else 0.0,
    "ring_total": sum(ring_values),
    "inside_mean": sum(inside_values) / max(len(inside_values), 1),
    "control_mean": sum(control_values) / max(len(control_values), 1),
}
out_path.write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))

if result["ring_mean"] < 0.75 or result["ring_max"] < 4.0 or result["ring_total"] < 2000.0:
    raise SystemExit("agent-list bloom verifier failed: bloom ring delta too small")
if result["ring_mean"] < result["control_mean"] * 1.5:
    raise SystemExit("agent-list bloom verifier failed: bloom delta not localized to buttons")
PY

echo "agent-list bloom verifier: PASS"
