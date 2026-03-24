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
WINDOW_PNG=/tmp/neozeus-hud-orange-window.png
ANALYSIS_JSON=/tmp/neozeus-hud-orange-analysis.json
KEEPALIVE_SH=/tmp/neozeus-hud-orange-keepalive.sh
SWAY_WORKSPACE=${NEOZEUS_HUD_ORANGE_WORKSPACE:-8}
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
while [ "$i" -le 60 ]; do
  printf '__NZ_ORANGE_VISIBLE__%02d\n' "$i"
  sleep 0.1
  i=$((i + 1))
done
SH
chmod +x "$KEEPALIVE_SH"

cargo build >"$BUILD_LOG" 2>&1

cleanup_app
rm -f "$RUN_LOG" "$DEBUG_LOG" "$WINDOW_PNG" "$ANALYSIS_JSON"

neozeus_gui_prepare_isolated_app_env "neozeus-hud-orange-visible"
APP_PID=$(neozeus_gui_launch_isolated \
    "$APP" \
    "$RUN_LOG" \
    WAYLAND_DISPLAY= \
    WINIT_UNIX_BACKEND=x11 \
    NEOZEUS_WINDOW_TITLE="neozeus-hud-orange-$$" \
    NEOZEUS_WINDOW_MODE=windowed \
    NEOZEUS_WINDOW_SCALE_FACTOR=1.0 \
    NEOZEUS_AGENT_BLOOM_INTENSITY=0.0 \
    NEOZEUS_AUTOVERIFY_COMMAND="sh $KEEPALIVE_SH" \
    NEOZEUS_AUTOVERIFY_DELAY_MS="$AUTOVERIFY_DELAY_MS")

WINDOW_JSON=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "neozeus-hud-orange-$$")
CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")
neozeus_gui_place_window "$CON_ID" "$SWAY_WORKSPACE" 1400 900 40 40
neozeus_gui_focus_workspace "$SWAY_WORKSPACE"
neozeus_gui_wait_for_visible_con_id "$CON_ID" >/dev/null
sleep 2

swaymsg -t get_tree > /tmp/neozeus-hud-orange-tree.json
XID=$(python - "neozeus-hud-orange-$$" <<'PY'
import json
import sys

title = sys.argv[1]
with open('/tmp/neozeus-hud-orange-tree.json') as f:
    tree = json.load(f)
stack = [tree]
while stack:
    node = stack.pop()
    if not isinstance(node, dict):
        continue
    if node.get('name') == title:
        xid = node.get('window')
        if xid is None:
            raise SystemExit('orange verifier failed: NeoZeus window is not an X11/Xwayland window')
        print(hex(xid))
        raise SystemExit(0)
    stack.extend(node.get('nodes', []))
    stack.extend(node.get('floating_nodes', []))
raise SystemExit('orange verifier failed: window title not found in sway tree')
PY
)

maim -i "$XID" "$WINDOW_PNG"
cleanup_app

python - "$WINDOW_PNG" "$ANALYSIS_JSON" <<'PY'
import json
import math
import sys
from pathlib import Path
from PIL import Image

png_path = Path(sys.argv[1])
out_path = Path(sys.argv[2])
img = Image.open(png_path).convert('RGBA')
window_w, window_h = img.size

logical_w = 1400.0
logical_h = 900.0
scale_x = window_w / logical_w
scale_y = window_h / logical_h

target = (225, 129, 10)
wrong = (255, 177, 18)
boxes = {
    'title': (18, 8, 220, 52),
    'row': (18, 58, 300, 104),
}

def near(a, b, radius):
    return math.dist(a, b) <= radius

result = {
    'window_size': [window_w, window_h],
    'logical_scale': [scale_x, scale_y],
    'target': list(target),
    'wrong': list(wrong),
    'boxes': {},
}

for name, (x0, y0, x1, y1) in boxes.items():
    exact_target = 0
    exact_wrong = 0
    near_target = 0
    near_wrong = 0
    warm_pixels = 0
    for ly in range(y0, y1):
        for lx in range(x0, x1):
            x = min(window_w - 1, max(0, round(lx * scale_x)))
            y = min(window_h - 1, max(0, round(ly * scale_y)))
            rgb = img.getpixel((x, y))[:3]
            if rgb == target:
                exact_target += 1
            if rgb == wrong:
                exact_wrong += 1
            if near(rgb, target, 8.0):
                near_target += 1
            if near(rgb, wrong, 8.0):
                near_wrong += 1
            if rgb[0] >= 120 and 60 <= rgb[1] <= 150 and rgb[2] <= 24 and rgb[0] >= rgb[1] + 20:
                warm_pixels += 1

    result['boxes'][name] = {
        'exact_target': exact_target,
        'exact_wrong': exact_wrong,
        'near_target': near_target,
        'near_wrong': near_wrong,
        'warm_pixels': warm_pixels,
    }

    min_near_target = 200 if name == 'title' else 120
    min_warm_pixels = 400 if name == 'title' else 180
    if near_target < min_near_target:
        raise SystemExit(
            f'orange verifier failed: {name} near-target coverage too small: near_target={near_target}'
        )
    if exact_wrong != 0 or near_wrong != 0:
        raise SystemExit(
            f'orange verifier failed: {name} still contains wrong-yellow pixels: exact_wrong={exact_wrong} near_wrong={near_wrong}'
        )
    if warm_pixels < min_warm_pixels:
        raise SystemExit(
            f'orange verifier failed: {name} warm/orange coverage too small: warm_pixels={warm_pixels}'
        )

out_path.write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
PY

echo "hud orange verifier: PASS"
