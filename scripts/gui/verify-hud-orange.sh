#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./neozeus-safe.sh
source "$ROOT_DIR/scripts/gui/neozeus-safe.sh"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-hud-orange-build.log
RUN_LOG=/tmp/neozeus-hud-orange-run.log
DEBUG_LOG=/tmp/neozeus-debug.log
SOURCE_PPM=/tmp/neozeus-hud-orange-source.ppm
ANALYSIS_JSON=/tmp/neozeus-hud-orange-analysis.json
KEEPALIVE_SH=/tmp/neozeus-hud-orange-keepalive.sh
SWAY_WORKSPACE=${NEOZEUS_HUD_ORANGE_WORKSPACE:-8}
CAPTURE_DELAY_FRAMES=${NEOZEUS_HUD_ORANGE_CAPTURE_DELAY_FRAMES:-1}
AUTOVERIFY_DELAY_MS=${NEOZEUS_HUD_ORANGE_AUTOVERIFY_DELAY_MS:-400}

cleanup_app() {
    if [[ -n "${APP_PID:-}" ]]; then
        neozeus_gui_cleanup_pid "$APP_PID"
        APP_PID=
    fi
    neozeus_gui_cleanup_isolated_app_env
}

trap cleanup_app EXIT

cat >"$KEEPALIVE_SH" <<'SH'
#!/bin/sh
i=1
while [ "$i" -le 40 ]; do
  printf '__NZ_ORANGE_KEEPALIVE__%02d\n' "$i"
  sleep 0.1
  i=$((i + 1))
done
SH
chmod +x "$KEEPALIVE_SH"

cargo build >"$BUILD_LOG" 2>&1

WINDOW_SCALE=$(neozeus_gui_workspace_output_scale "$SWAY_WORKSPACE")
cleanup_app
rm -f "$RUN_LOG" "$DEBUG_LOG" "$SOURCE_PPM" "$ANALYSIS_JSON"

neozeus_gui_prepare_isolated_app_env "neozeus-hud-orange"
APP_PID=$(neozeus_gui_launch_isolated \
    "$APP" \
    "$RUN_LOG" \
    NEOZEUS_WINDOW_TITLE="neozeus-hud-orange-$$" \
    NEOZEUS_WINDOW_MODE=windowed \
    NEOZEUS_WINDOW_SCALE_FACTOR="$WINDOW_SCALE" \
    NEOZEUS_AGENT_BLOOM_INTENSITY=0.0 \
    NEOZEUS_CAPTURE_HUD_TEXTURE_PATH="$SOURCE_PPM" \
    NEOZEUS_CAPTURE_HUD_TEXTURE_DELAY_FRAMES="$CAPTURE_DELAY_FRAMES" \
    NEOZEUS_AUTOVERIFY_COMMAND="sh $KEEPALIVE_SH" \
    NEOZEUS_AUTOVERIFY_DELAY_MS="$AUTOVERIFY_DELAY_MS")

WINDOW_JSON=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "neozeus-hud-orange-$$")
CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")
neozeus_gui_place_window "$CON_ID" "$SWAY_WORKSPACE" 1400 900 40 40
neozeus_gui_focus_workspace "$SWAY_WORKSPACE"
neozeus_gui_wait_for_visible_con_id "$CON_ID" >/dev/null

for _ in $(seq 1 240); do
    [[ -f "$SOURCE_PPM" ]] && break
    sleep 0.25
done

if [[ ! -f "$SOURCE_PPM" ]]; then
    echo "orange verifier failed: source HUD capture did not produce $SOURCE_PPM" >&2
    tail -n 120 "$DEBUG_LOG" 2>/dev/null || true
    exit 1
fi

cleanup_app

python - "$SOURCE_PPM" "$ANALYSIS_JSON" <<'PY'
import json
import math
import sys
from pathlib import Path
from PIL import Image

ppm_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
img = Image.open(ppm_path).convert("RGBA")
if img.size != (1400, 900):
    raise SystemExit(f"orange verifier failed: unexpected HUD capture size {img.size}")

# Top-left logical HUD region containing the title and first agent row.
img = img.crop((0, 0, 340, 180))
target = (225, 129, 10)
wrong = (255, 177, 18)
boxes = {
    "title": (18, 8, 220, 52),
    "row": (18, 58, 300, 104),
}

def color_dist(a, b):
    return math.sqrt(sum((float(x) - float(y)) ** 2 for x, y in zip(a, b)))

result = {"image_size": list(img.size), "target": list(target), "wrong": list(wrong), "boxes": {}}
for name, (x0, y0, x1, y1) in boxes.items():
    exact_target = 0
    exact_wrong = 0
    near_target = 0
    near_wrong = 0
    warm_pixels = 0
    for y in range(y0, y1):
        for x in range(x0, x1):
            rgb = img.getpixel((x, y))[:3]
            if rgb == target:
                exact_target += 1
            if rgb == wrong:
                exact_wrong += 1
            if color_dist(rgb, target) <= 6.0:
                near_target += 1
            if color_dist(rgb, wrong) <= 6.0:
                near_wrong += 1
            if rgb[0] >= 120 and 35 <= rgb[1] <= 185 and rgb[2] <= 32 and rgb[0] >= rgb[1] + 15:
                warm_pixels += 1

    result["boxes"][name] = {
        "exact_target": exact_target,
        "exact_wrong": exact_wrong,
        "near_target": near_target,
        "near_wrong": near_wrong,
        "warm_pixels": warm_pixels,
    }

    if near_target < 200:
        raise SystemExit(
            f"orange verifier failed: {name} has too little target-orange coverage: near_target={near_target}"
        )
    if near_wrong != 0 or exact_wrong != 0:
        raise SystemExit(
            f"orange verifier failed: {name} still contains wrong-yellow pixels: near_wrong={near_wrong} exact_wrong={exact_wrong}"
        )
    if warm_pixels < 200:
        raise SystemExit(
            f"orange verifier failed: {name} warm/orange coverage too small: warm_pixels={warm_pixels}"
        )

out_path.write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
PY

echo "hud orange verifier: PASS"
