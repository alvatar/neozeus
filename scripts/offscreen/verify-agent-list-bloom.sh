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
left_crop = diff.crop((0, 0, int(min(340.0, width)), int(min(220.0, height))))
bbox = left_crop.getbbox()
if bbox is None:
    raise SystemExit(1)
focus = left_crop.crop(bbox)
vals = list(focus.getdata())
glow_mean = sum(sum(px) for px in vals) / (len(vals) * 3.0)
glow_peak = max(max(px) for px in vals)
glow_nonzero = sum(1 for px in vals if px != (0, 0, 0))
full_nonzero = sum(1 for px in left_crop.getdata() if px != (0, 0, 0))
print(f"agent_list_bbox={bbox}")
print(f"agent_list_glow_nonzero={glow_nonzero}")
print(f"agent_list_glow_mean={glow_mean:.2f}")
print(f"agent_list_glow_peak={glow_peak}")
print(f"agent_list_top_left_nonzero={full_nonzero}")
if glow_nonzero <= 1000:
    raise SystemExit(1)
if glow_mean <= 25.0:
    raise SystemExit(1)
if glow_peak <= 120:
    raise SystemExit(1)
PY

echo "offscreen agent-list bloom verification: PASS"
