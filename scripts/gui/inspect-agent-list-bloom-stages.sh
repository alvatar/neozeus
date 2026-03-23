#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./neozeus-safe.sh
source "$ROOT_DIR/scripts/gui/neozeus-safe.sh"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-bloom-stages-build.log
RUN_LOG=/tmp/neozeus-bloom-stages-run.log
SCREENSHOT=/tmp/neozeus-bloom-stages-window.png
SOURCE_PNG=/tmp/neozeus-bloom-stage-source.png
SMALL_PNG=/tmp/neozeus-bloom-stage-small.png
WIDE_PNG=/tmp/neozeus-bloom-stage-wide.png
FINAL_PNG=/tmp/neozeus-bloom-stage-final-agent-list.png
ANALYSIS_JSON=/tmp/neozeus-bloom-stage-analysis.json
INTENSITY=${NEOZEUS_AGENT_BLOOM_STAGE_INTENSITY:-1.35}
SWAY_WORKSPACE=${NEOZEUS_AGENT_BLOOM_WORKSPACE:-8}

cleanup_app() {
    if [[ -n "${APP_PID:-}" ]]; then
        neozeus_gui_cleanup_pid "$APP_PID"
        APP_PID=
    fi
    neozeus_gui_cleanup_isolated_app_env
}

trap cleanup_app EXIT

capture_stable_window() {
    local x=$1
    local y=$2
    local width=$3
    local height=$4
    local out=$5
    local capture_scale=$6
    local geom="${x},${y} ${width}x${height}"
    local a=/tmp/neozeus-bloom-stages-a.png
    local b=/tmp/neozeus-bloom-stages-b.png

    for _ in $(seq 1 20); do
        grim -s "$capture_scale" -g "$geom" "$a"
        sleep 0.5
        grim -s "$capture_scale" -g "$geom" "$b"
        local diff_raw
        diff_raw=$(compare -metric AE "$a" "$b" null: 2>&1 >/dev/null || true)
        local diff
        diff=$(python - "$diff_raw" <<'PY'
import re
import sys
m = re.search(r"[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?", sys.argv[1], re.I)
print(m.group(0) if m else "999999")
PY
)
        if python - "$diff" <<'PY'
import sys
raise SystemExit(0 if float(sys.argv[1]) <= 500.0 else 1)
PY
        then
            mv "$b" "$out"
            rm -f "$a"
            return 0
        fi
        sleep 0.4
    done

    mv "$b" "$out"
    rm -f "$a"
}

cargo build >"$BUILD_LOG" 2>&1

WINDOW_SCALE=$(neozeus_gui_workspace_output_scale "$SWAY_WORKSPACE")
CAPTURE_SCALE="$WINDOW_SCALE"

cleanup_app
rm -f "$RUN_LOG" "$SCREENSHOT" "$SOURCE_PNG" "$SMALL_PNG" "$WIDE_PNG" "$FINAL_PNG" "$ANALYSIS_JSON"
neozeus_gui_prepare_isolated_app_env "neozeus-bloom-stages"
APP_PID=$(neozeus_gui_launch_isolated \
    "$APP" \
    "$RUN_LOG" \
    NEOZEUS_WINDOW_TITLE="neozeus-bloom-stages-$$" \
    NEOZEUS_WINDOW_MODE=windowed \
    NEOZEUS_WINDOW_SCALE_FACTOR="$WINDOW_SCALE" \
    NEOZEUS_AGENT_BLOOM_INTENSITY="$INTENSITY" \
    NEOZEUS_AGENT_BLOOM_DEBUG_PREVIEWS=1 \
    NEOZEUS_AUTOVERIFY_COMMAND='printf "__NZ_BLOOM_STAGES__\n"' \
    NEOZEUS_AUTOVERIFY_DELAY_MS=400)

window_json=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "neozeus-bloom-stages-$$")
con_id=$(jq -r '.id' <<<"$window_json")
neozeus_gui_place_window "$con_id" "$SWAY_WORKSPACE" 1400 900 40 40
sleep 1.5
window_json=$(neozeus_gui_find_window_by_con_id "$con_id")
x=$(jq -r '.x' <<<"$window_json")
y=$(jq -r '.y' <<<"$window_json")
width=$(jq -r '.width' <<<"$window_json")
height=$(jq -r '.height' <<<"$window_json")

capture_stable_window "$x" "$y" "$width" "$height" "$SCREENSHOT" "$CAPTURE_SCALE"
cleanup_app

python - "$SCREENSHOT" "$SOURCE_PNG" "$SMALL_PNG" "$WIDE_PNG" "$FINAL_PNG" "$ANALYSIS_JSON" <<'PY'
import json
import sys
from pathlib import Path
from PIL import Image

screen_path = Path(sys.argv[1])
source_path = Path(sys.argv[2])
small_path = Path(sys.argv[3])
wide_path = Path(sys.argv[4])
final_path = Path(sys.argv[5])
analysis_path = Path(sys.argv[6])

img = Image.open(screen_path).convert("RGBA")
width, height = img.size
preview_w = 160
preview_h = 120
margin = 16
preview_gap = 12
preview_total_w = preview_w * 3 + preview_gap * 2
preview_x = width - margin - preview_total_w
preview_y = margin
final_crop = (0, 0, min(width, 360), min(height, 220))

rects = {
    "source": (preview_x, preview_y, preview_x + preview_w, preview_y + preview_h),
    "small": (preview_x + preview_w + preview_gap, preview_y, preview_x + preview_w * 2 + preview_gap, preview_y + preview_h),
    "wide": (preview_x + (preview_w + preview_gap) * 2, preview_y, preview_x + preview_total_w, preview_y + preview_h),
    "final": final_crop,
}
outputs = {
    "source": source_path,
    "small": small_path,
    "wide": wide_path,
    "final": final_path,
}

stats = {}
for name, rect in rects.items():
    crop = img.crop(rect)
    outputs[name].write_bytes(b"")
    crop.save(outputs[name])
    values = list(crop.getdata())
    max_rgb = max(max(px[:3]) for px in values)
    mean_rgb = sum(sum(px[:3]) / 3.0 for px in values) / max(len(values), 1)
    red_dominance = sum((px[0] - max(px[1], px[2])) for px in values) / max(len(values), 1)
    nonblack = sum(1 for px in values if max(px[:3]) > 12)
    stats[name] = {
        "rect": {"x": rect[0], "y": rect[1], "w": rect[2] - rect[0], "h": rect[3] - rect[1]},
        "max_rgb": max_rgb,
        "mean_rgb": mean_rgb,
        "red_dominance": red_dominance,
        "nonblack_pixels": nonblack,
    }

analysis_path.write_text(json.dumps(stats, indent=2))
print(json.dumps(stats, indent=2))
PY

echo "bloom stage captures written:"
echo "  $SCREENSHOT"
echo "  $SOURCE_PNG"
echo "  $SMALL_PNG"
echo "  $WIDE_PNG"
echo "  $FINAL_PNG"
echo "  $ANALYSIS_JSON"
