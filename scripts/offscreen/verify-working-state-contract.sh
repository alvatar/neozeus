#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-working-state-contract"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

IDLE="$NEOZEUS_OFFSCREEN_ROOT/working-state-idle.ppm"
WORKING="$NEOZEUS_OFFSCREEN_ROOT/working-state-working.ppm"

neozeus_offscreen_run_capture "working-state-idle" "0.0" "$IDLE" "$width" "$height" 2 0
neozeus_offscreen_run_capture "working-state-working" "0.0" "$WORKING" "$width" "$height" 2 0

python - "$IDLE" "$WORKING" <<'PY'
from PIL import Image, ImageChops
import sys

idle = Image.open(sys.argv[1]).convert('RGB')
working = Image.open(sys.argv[2]).convert('RGB')
diff = ImageChops.difference(idle, working)
bbox = diff.getbbox()
if bbox is None:
    raise SystemExit(1)
vals = list(diff.getdata())
nonzero = sum(1 for px in vals if px != (0, 0, 0))
green = [px for px in vals if px[1] > px[0] and px[1] > px[2] and px[1] > 20]
green_count = len(green)
green_peak = max((px[1] for px in green), default=0)
print(f"working_state_bbox={bbox}")
print(f"working_state_nonzero={nonzero}")
print(f"working_state_green_count={green_count}")
print(f"working_state_green_peak={green_peak}")
if nonzero <= 3000:
    raise SystemExit(1)
if green_count <= 1500:
    raise SystemExit(1)
if green_peak <= 120:
    raise SystemExit(1)
PY

echo "offscreen working-state contract verification: PASS"
