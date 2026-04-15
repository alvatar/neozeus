#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-agent-context-bloom"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

OFF="$NEOZEUS_OFFSCREEN_ROOT/agent-context-off.ppm"
ON="$NEOZEUS_OFFSCREEN_ROOT/agent-context-on.ppm"

neozeus_offscreen_run_capture "agent-context-bloom" "0.0" "$OFF" "$width" "$height"
neozeus_offscreen_run_capture "agent-context-bloom" "2.0" "$ON" "$width" "$height"

python - "$OFF" "$ON" "$width" "$height" <<'PY'
from PIL import Image, ImageChops
import sys

off = Image.open(sys.argv[1]).convert('RGB')
on = Image.open(sys.argv[2]).convert('RGB')
width = int(sys.argv[3])
height = int(sys.argv[4])
diff = ImageChops.difference(off, on)

agent_crop = diff.crop((0, 0, int(min(340.0, width)), int(min(220.0, height))))
agent_nonzero = sum(1 for px in agent_crop.getdata() if px != (0, 0, 0))
print(f"agent_context_agent_crop_nonzero={agent_nonzero}")
if agent_nonzero <= 1000:
    raise SystemExit(1)

x0 = int(round(width * 0.18))
x1 = int(round(width * 0.36))
y0 = int(round(height * 0.04))
y1 = int(round(height * 0.20))
context_crop = diff.crop((x0, y0, x1, y1))
context_nonzero = sum(1 for px in context_crop.getdata() if px != (0, 0, 0))
print(f"agent_context_overlay_crop_nonzero={context_nonzero}")
print(f"agent_context_overlay_crop=({x0},{y0},{x1},{y1})")
if context_nonzero > 200:
    raise SystemExit(1)
PY

echo "offscreen agent-context bloom verification: PASS"
