#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-bloom-build.log
RUN_LOG=/tmp/neozeus-bloom-run.log
OFF_PNG=/tmp/neozeus-agent-bloom-off.png
ON_PNG=/tmp/neozeus-agent-bloom-on.png
DEBUG_PNG=/tmp/neozeus-agent-bloom-debug.png
STABLE_A=/tmp/neozeus-agent-bloom-stable-a.png
STABLE_B=/tmp/neozeus-agent-bloom-stable-b.png
ANALYSIS_JSON=/tmp/neozeus-agent-bloom-analysis.json
ON_INTENSITY=${NEOZEUS_AGENT_BLOOM_VERIFY_ON_INTENSITY:-3.0}
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
            }))
            sys.exit(0)
        stack.extend(node.get("nodes", []))
        stack.extend(node.get("floating_nodes", []))
    time.sleep(0.25)
raise SystemExit(1)
PY
}

find_window_by_con_id() {
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

capture_stable_window() {
    local x=$1
    local y=$2
    local width=$3
    local height=$4
    local out=$5
    local geom="${x},${y} ${width}x${height}"

    for _ in $(seq 1 20); do
        grim -g "$geom" "$STABLE_A"
        sleep 0.5
        grim -g "$geom" "$STABLE_B"
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
        kill "$APP_PID" 2>/dev/null || true
        for _ in $(seq 1 20); do
            if ! kill -0 "$APP_PID" 2>/dev/null; then
                break
            fi
            sleep 0.2
        done
        if kill -0 "$APP_PID" 2>/dev/null; then
            kill -9 "$APP_PID" 2>/dev/null || true
        fi
        wait "$APP_PID" 2>/dev/null || true
        APP_PID=
    fi
}

cleanup() {
    cleanup_app
}
trap cleanup EXIT

run_capture() {
    local tag=$1
    local intensity=$2
    local debug_preview=$3
    local capture_scope=$4
    local out_png=$5
    local title="neozeus-bloom-${tag}-$$"

    cleanup_app
    rm -f "$RUN_LOG" "$STABLE_A" "$STABLE_B" "$out_png"

    NEOZEUS_WINDOW_TITLE="$title" \
    NEOZEUS_WINDOW_MODE=windowed \
    NEOZEUS_WINDOW_SCALE_FACTOR=1.0 \
    NEOZEUS_AGENT_BLOOM_INTENSITY="$intensity" \
    NEOZEUS_AGENT_BLOOM_DEBUG_PREVIEW="$debug_preview" \
    NEOZEUS_AUTOVERIFY_COMMAND='printf "__NZ_BLOOM__\n"' \
    NEOZEUS_AUTOVERIFY_DELAY_MS=400 \
    nohup "$APP" >"$RUN_LOG" 2>&1 </dev/null &
    APP_PID=$!

    local window_json
    window_json=$(find_window_by_pid_and_title "$APP_PID" "$title")
    local con_id
    con_id=$(jq -r '.id' <<<"$window_json")

    swaymsg "[con_id=${con_id}] move container to workspace number ${SWAY_WORKSPACE}" >/dev/null
    swaymsg "[con_id=${con_id}] floating enable" >/dev/null
    swaymsg "[con_id=${con_id}] resize set width 1400 px height 900 px" >/dev/null
    swaymsg "[con_id=${con_id}] move position 40 px 40 px" >/dev/null
    sleep 1.2

    window_json=$(find_window_by_con_id "$con_id")
    local x y width height capture_w capture_h capture_x capture_y
    x=$(jq -r '.x' <<<"$window_json")
    y=$(jq -r '.y' <<<"$window_json")
    width=$(jq -r '.width' <<<"$window_json")
    height=$(jq -r '.height' <<<"$window_json")

    if [[ "$capture_scope" == "full" ]]; then
        capture_x=$x
        capture_y=$y
        capture_w=$width
        capture_h=$height
    else
        capture_x=$x
        capture_y=$y
        capture_w=$(( width < 340 ? width : 340 ))
        capture_h=$(( height < 180 ? height : 180 ))
    fi

    capture_stable_window "$capture_x" "$capture_y" "$capture_w" "$capture_h" "$out_png"
}

cargo build >"$BUILD_LOG" 2>&1
pkill -f "$APP" 2>/dev/null || true

run_capture off 0.0 0 left "$OFF_PNG"
run_capture on "$ON_INTENSITY" 0 left "$ON_PNG"
run_capture debug "$ON_INTENSITY" 1 full "$DEBUG_PNG"
cleanup_app

python - "$OFF_PNG" "$ON_PNG" "$DEBUG_PNG" "$ANALYSIS_JSON" <<'PY'
import json
import math
import sys
from pathlib import Path
from PIL import Image

off_path = Path(sys.argv[1])
on_path = Path(sys.argv[2])
debug_path = Path(sys.argv[3])
out_path = Path(sys.argv[4])

off = Image.open(off_path).convert("RGBA")
on = Image.open(on_path).convert("RGBA")
debug = Image.open(debug_path).convert("RGBA")
logical_size = (340, 180)
debug_logical_size = (1400, 900)
if off.size != logical_size:
    off = off.resize(logical_size, Image.Resampling.LANCZOS)
if on.size != logical_size:
    on = on.resize(logical_size, Image.Resampling.LANCZOS)
if debug.size != debug_logical_size:
    debug = debug.resize(debug_logical_size, Image.Resampling.LANCZOS)

width, height = off.size
shell_w = min(300, width)
content_x = 20 + 1
content_y = 10 + 52
row_w = max(shell_w - 20 - 3, 0)
row_h = 28
main = (content_x, content_y + 2, max(row_w - 12 - 10, 12), max(row_h - 4, 10))
marker = (content_x + row_w - 12, content_y + 2, 12, max(row_h - 4, 10))
rects = [main, marker]
outer = 10
inner = 1

center_box = (width // 2 - 40, height // 2 - 16, 80, 32)

debug_preview_width = 210
debug_preview_height = 120
debug_preview_gap = 16
debug_preview_margin = 20
debug_total_width = debug_preview_width * 3 + debug_preview_gap * 2
debug_start_x = debug_logical_size[0] - debug_preview_margin - debug_total_width
debug_preview_y = debug_preview_margin
debug_source_rect = (debug_start_x, debug_preview_y, debug_preview_width, debug_preview_height)
debug_blur_rect = (debug_start_x + debug_preview_width + debug_preview_gap, debug_preview_y, debug_preview_width, debug_preview_height)
debug_composite_rect = (debug_start_x + (debug_preview_width + debug_preview_gap) * 2, debug_preview_y, debug_preview_width, debug_preview_height)

def in_rect(x, y, rect):
    rx, ry, rw, rh = rect
    return rx <= x < rx + rw and ry <= y < ry + rh

def in_expanded(x, y, rect, amount):
    rx, ry, rw, rh = rect
    return rx - amount <= x < rx + rw + amount and ry - amount <= y < ry + rh + amount

def pixel_delta(a, b):
    return sum(abs(int(a[i]) - int(b[i])) for i in range(3)) / 3.0

def region_mean_rgb(image, rect):
    rx, ry, rw, rh = rect
    total = 0.0
    count = 0
    for yy in range(ry, ry + rh):
        for xx in range(rx, rx + rw):
            px = image.getpixel((xx, yy))
            total += (float(px[0]) + float(px[1]) + float(px[2])) / 3.0
            count += 1
    return total / max(count, 1)

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
    "debug_source_mean": region_mean_rgb(debug, debug_source_rect),
    "debug_blur_mean": region_mean_rgb(debug, debug_blur_rect),
    "debug_composite_mean": region_mean_rgb(debug, debug_composite_rect),
}
out_path.write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))

ring_mean = result["ring_mean"]
ring_max = result["ring_max"]
ring_total = result["ring_total"]
control_mean = result["control_mean"]
debug_source_mean = result["debug_source_mean"]
debug_blur_mean = result["debug_blur_mean"]
debug_composite_mean = result["debug_composite_mean"]

if debug_source_mean < 1.0:
    raise SystemExit("agent-list bloom verifier failed: bloom source preview is empty")
if debug_blur_mean < 1.0:
    raise SystemExit("agent-list bloom verifier failed: bloom blur preview is empty")
if debug_composite_mean < 1.0:
    raise SystemExit("agent-list bloom verifier failed: bloom composite preview is empty")
if ring_mean < 1.0 or ring_max < 8.0 or ring_total < 4000.0:
    raise SystemExit("agent-list bloom verifier failed: halo delta too small")
if ring_mean < control_mean * 2.0:
    raise SystemExit("agent-list bloom verifier failed: halo delta not localized to buttons")
PY

echo "agent-list bloom verifier: PASS"
