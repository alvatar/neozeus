#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./neozeus-safe.sh
source "$ROOT_DIR/scripts/gui/neozeus-safe.sh"

APP="$ROOT_DIR/target/debug/neozeus"
BUILD_LOG=/tmp/neozeus-color-build.log
RUN_LOG=/tmp/neozeus-color-run.log
DEBUG_LOG=/tmp/neozeus-debug.log
TEXTURE_DUMP=/tmp/neozeus-texture.ppm
TEXTURE_PNG=/tmp/neozeus-color-texture.png
WINDOW_PNG=/tmp/neozeus-color-window.png
FIXTURE=/tmp/neozeus-color-smoke.sh

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
rm -f "$RUN_LOG" "$DEBUG_LOG" "$TEXTURE_DUMP" "$TEXTURE_PNG" "$WINDOW_PNG"

cat >"$FIXTURE" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

clear
printf '\e[0m'

label() {
  printf '%-8s' "$1"
}

ansi_fg_row() {
  local label_text=$1; shift
  label "$label_text"
  local code
  for code in "$@"; do
    printf "\e[%sm██\e[0m " "$code"
  done
  printf '\n'
}

ansi_bg_row() {
  local label_text=$1; shift
  label "$label_text"
  local code
  for code in "$@"; do
    printf "\e[%sm  \e[0m " "$code"
  done
  printf '\n'
}

idx_fg_row() {
  local label_text=$1; shift
  local idx
  label "$label_text"
  for idx in "$@"; do
    printf "\e[38;5;%sm██\e[0m " "$idx"
  done
  printf '\n'
}

idx_bg_row() {
  local label_text=$1; shift
  local idx
  label "$label_text"
  for idx in "$@"; do
    printf "\e[48;5;%sm  \e[0m " "$idx"
  done
  printf '\n'
}

rgb_fg_row() {
  local label_text=$1; shift
  local spec
  label "$label_text"
  for spec in "$@"; do
    IFS=, read -r r g b <<<"$spec"
    printf "\e[38;2;%s;%s;%sm██\e[0m " "$r" "$g" "$b"
  done
  printf '\n'
}

rgb_bg_row() {
  local label_text=$1; shift
  local spec
  label "$label_text"
  for spec in "$@"; do
    IFS=, read -r r g b <<<"$spec"
    printf "\e[48;2;%s;%s;%sm  \e[0m " "$r" "$g" "$b"
  done
  printf '\n'
}

ansi_fg_row FG16N 30 31 32 33 34 35 36 37
ansi_fg_row FG16B 90 91 92 93 94 95 96 97
ansi_bg_row BG16N 40 41 42 43 44 45 46 47
ansi_bg_row BG16B 100 101 102 103 104 105 106 107
idx_bg_row BG256A 16 21 46 51 196 201 226 231
idx_bg_row BG256B 232 235 239 243 247 251 255 244
rgb_bg_row BG24 255,0,0 0,255,0 0,0,255 255,255,0 255,0,255 0,255,255 255,128,0 128,0,255
idx_fg_row FG256A 16 21 46 51 196 201 226 231
idx_fg_row FG256B 232 235 239 243 247 251 255 244
rgb_fg_row FG24 255,0,0 0,255,0 0,0,255 255,255,0 255,0,255 0,255,255 255,128,0 128,0,255
EOF
chmod +x "$FIXTURE"

WINDOW_TITLE="neozeus-color-verify-$$"
AUTOVERIFY_COMMAND="bash $FIXTURE"
neozeus_gui_prepare_isolated_app_env "neozeus-colors"
APP_PID=$(neozeus_gui_launch_isolated \
    "$APP" \
    "$RUN_LOG" \
    __NV_PRIME_RENDER_OFFLOAD=1 \
    __VK_LAYER_NV_optimus=NVIDIA_only \
    VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json \
    WGPU_ADAPTER_NAME=nvidia \
    NEOZEUS_WINDOW_TITLE="$WINDOW_TITLE" \
    NEOZEUS_DUMP_TEXTURE=1 \
    NEOZEUS_AUTOVERIFY_COMMAND="$AUTOVERIFY_COMMAND" \
    NEOZEUS_AUTOVERIFY_DELAY_MS=1500)

GUI_WORKSPACE=${NEOZEUS_GUI_WORKSPACE:-8}
CAPTURE_SCALE=$(neozeus_gui_workspace_output_scale "$GUI_WORKSPACE")

WINDOW_JSON=$(neozeus_gui_find_window_by_pid_and_title "$APP_PID" "$WINDOW_TITLE")

CON_ID=$(jq -r '.id' <<<"$WINDOW_JSON")
neozeus_gui_place_window "$CON_ID" "$GUI_WORKSPACE" 1400 900 40 40
neozeus_gui_focus_workspace "$GUI_WORKSPACE"
sleep 1.0

WINDOW_JSON=$(neozeus_gui_wait_for_visible_con_id "$CON_ID")

for _ in $(seq 1 40); do
    grep -q 'auto-verify command dispatched' "$DEBUG_LOG" 2>/dev/null && break
    sleep 0.25
done
wait_for_log_quiet "$DEBUG_LOG"

X=$(jq -r '.x' <<<"$WINDOW_JSON")
Y=$(jq -r '.y' <<<"$WINDOW_JSON")
WIDTH=$(jq -r '.width' <<<"$WINDOW_JSON")
HEIGHT=$(jq -r '.height' <<<"$WINDOW_JSON")
neozeus_gui_grim_capture "$CAPTURE_SCALE" "${X},${Y} ${WIDTH}x${HEIGHT}" "$WINDOW_PNG"

grep -q 'auto-verify command dispatched: bash /tmp/neozeus-color-smoke.sh' "$DEBUG_LOG"
grep -q 'pty write command `bash /tmp/neozeus-color-smoke.sh`' "$DEBUG_LOG"
[[ -f "$TEXTURE_DUMP" ]]

python - "$TEXTURE_DUMP" "$TEXTURE_PNG" <<'PY'
import math
import sys
from pathlib import Path

ppm_path = Path(sys.argv[1])
png_path = Path(sys.argv[2])

raw = ppm_path.read_bytes()
if not raw.startswith(b'P6\n'):
    raise SystemExit('unexpected PPM header')
header_end = raw.find(b'\n255\n')
if header_end == -1:
    raise SystemExit('missing max value header')
size_line = raw[3:raw.find(b'\n', 3)].decode().strip()
width_s, height_s = size_line.split()
width = int(width_s)
height = int(height_s)
pixels = raw[header_end + len(b'\n255\n'):]
if len(pixels) != width * height * 3:
    raise SystemExit(f'ppm byte size mismatch: got {len(pixels)}, expected {width * height * 3}')

# Also write a PNG artifact if Pillow is available; ignore otherwise.
try:
    from PIL import Image
    Image.frombytes('RGB', (width, height), pixels).save(png_path)
except Exception:
    pass

CELL_W = 14
CELL_H = 24
START_COL = 8
TOL = 0.0

checks = {
    'BG256A': (4, [16, 21, 46, 51, 196, 201, 226, 231]),
    'BG256B': (5, [232, 235, 239, 243, 247, 251, 255, 244]),
    'BG24':   (6, [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0), (255, 0, 255), (0, 255, 255), (255, 128, 0), (128, 0, 255)]),
    'FG256A': (7, [16, 21, 46, 51, 196, 201, 226, 231]),
    'FG256B': (8, [232, 235, 239, 243, 247, 251, 255, 244]),
    'FG24':   (9, [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 0), (255, 0, 255), (0, 255, 255), (255, 128, 0), (128, 0, 255)]),
}

ansi = [
    (0x00, 0x00, 0x00), (0xcc, 0x55, 0x55), (0x55, 0xcc, 0x55), (0xcd, 0xcd, 0x55),
    (0x54, 0x55, 0xcb), (0xcc, 0x55, 0xcc), (0x7a, 0xca, 0xca), (0xcc, 0xcc, 0xcc),
    (0x55, 0x55, 0x55), (0xff, 0x55, 0x55), (0x55, 0xff, 0x55), (0xff, 0xff, 0x55),
    (0x55, 0x55, 0xff), (0xff, 0x55, 0xff), (0x55, 0xff, 0xff), (0xff, 0xff, 0xff),
]

def xterm(index):
    if index < 16:
        return ansi[index]
    if index < 232:
        ramp = [0, 0x5f, 0x87, 0xaf, 0xd7, 0xff]
        idx = index - 16
        blue = ramp[idx % 6]
        green = ramp[(idx // 6) % 6]
        red = ramp[(idx // 36) % 6]
        return (red, green, blue)
    grey = 0x08 + (index - 232) * 10
    return (grey, grey, grey)

def pixel(x, y):
    idx = (y * width + x) * 3
    return tuple(pixels[idx:idx + 3])

def sample(row, sample_idx):
    col = START_COL + sample_idx * 3
    x = col * CELL_W + CELL_W // 2
    y = row * CELL_H + CELL_H // 2
    return pixel(x, y)

def dist(a, b):
    return math.sqrt(sum((x - y) ** 2 for x, y in zip(a, b)))

all_ok = True
for name, (row, values) in checks.items():
    print(name)
    for i, value in enumerate(values):
        expected = xterm(value) if isinstance(value, int) else value
        actual = sample(row, i)
        d = dist(actual, expected)
        ok = d <= TOL
        all_ok &= ok
        print(f'  {i}: actual={actual} expected={expected} dist={d:.1f} ok={ok}')

print(f'ALL_OK={all_ok}')
if not all_ok:
    raise SystemExit(1)
PY

echo "terminal color verification: PASS"
echo "artifacts: $WINDOW_PNG $TEXTURE_DUMP $TEXTURE_PNG $RUN_LOG $DEBUG_LOG"
