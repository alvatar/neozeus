#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-agent-list-bloom"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

OFF="$NEOZEUS_OFFSCREEN_ROOT/agent-list-off.ppm"
ON="$NEOZEUS_OFFSCREEN_ROOT/agent-list-on.ppm"

neozeus_offscreen_run_capture "agent-list-bloom" "0.0" "$OFF" "$width" "$height"
neozeus_offscreen_run_capture "agent-list-bloom" "2.0" "$ON" "$width" "$height"

python - "$OFF" "$ON" <<'PY'
from PIL import Image, ImageChops
import sys

off = Image.open(sys.argv[1]).convert('RGB')
on = Image.open(sys.argv[2]).convert('RGB')
diff = ImageChops.difference(off, on)
width, height = off.size

shell_w = min(300.0, float(width))
left_rail = 20.0
header_h = 52.0
padding = 10.0
row_h = 28.0
marker_w = 12.0
marker_gap = 10.0

content_x = left_rail + 1.0
content_y = padding + header_h
content_w = max(shell_w - left_rail - 3.0, 0.0)
row_x = content_x
row_y = content_y
row_w = content_w
main_x = row_x
main_y = row_y + 2.0
main_w = max(row_w - marker_w - marker_gap, 12.0)
main_h = max(row_h - 4.0, 10.0)
marker_x = row_x + row_w - marker_w
marker_y = row_y + 2.0
marker_h = main_h
accent_x = row_x + 3.0
accent_y = row_y + 3.0
accent_w = 8.0
accent_h = max(row_h - 6.0, 10.0)
text_x = main_x + 28.0
text_y = main_y + 4.0
text_w = min(150.0, max(main_w - 56.0, 24.0))
text_h = min(14.0, max(main_h - 8.0, 8.0))

def crop(rect):
    x, y, w, h = rect
    return diff.crop((int(round(x)), int(round(y)), int(round(x + w)), int(round(y + h))))

def stats(img):
    vals = list(img.getdata())
    mean = sum(sum(px) for px in vals) / (len(vals) * 3.0)
    maxv = max(max(px) for px in vals)
    nonzero = sum(1 for px in vals if px != (0, 0, 0))
    return mean, maxv, nonzero

accent_stats = stats(crop((accent_x, accent_y, accent_w, accent_h)))
marker_stats = stats(crop((marker_x, marker_y, marker_w, marker_h)))
text_stats = stats(crop((text_x, text_y, text_w, text_h)))
full_stats = stats(diff.crop((0, 0, int(shell_w), int(min(220.0, height)))))

glow_nonzero = accent_stats[2] + marker_stats[2]
glow_mean = max(accent_stats[0], marker_stats[0])
text_mean = text_stats[0]
text_nonzero = text_stats[2]
print(f"agent_list_glow_nonzero={glow_nonzero}")
print(f"agent_list_glow_mean={glow_mean:.2f}")
print(f"agent_list_text_mean={text_mean:.2f}")
print(f"agent_list_text_nonzero={text_nonzero}")
print(f"agent_list_top_left_nonzero={full_stats[2]}")
if glow_nonzero <= 250:
    raise SystemExit(1)
if glow_mean <= 25.0:
    raise SystemExit(1)
if text_mean >= glow_mean * 0.45:
    raise SystemExit(1)
if text_nonzero >= glow_nonzero * 0.45:
    raise SystemExit(1)
PY

echo "offscreen agent-list bloom verification: PASS"
