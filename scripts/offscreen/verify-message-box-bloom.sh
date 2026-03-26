#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-message-box-bloom"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

OFF="$NEOZEUS_OFFSCREEN_ROOT/message-box-off.ppm"
ON="$NEOZEUS_OFFSCREEN_ROOT/message-box-on.ppm"

neozeus_offscreen_run_capture "message-box-bloom" "0.0" "$OFF" "$width" "$height"
neozeus_offscreen_run_capture "message-box-bloom" "2.0" "$ON" "$width" "$height"

metric=$(neozeus_offscreen_compare_ae "$OFF" "$ON")
value=$(neozeus_offscreen_parse_metric "$metric")
python - "$value" <<'PY'
import sys
value = float(sys.argv[1])
threshold = 200.0
print(f"message_box_full_frame_diff={value:.0f}")
print(f"threshold={threshold:.0f}")
if value > threshold:
    raise SystemExit(1)
PY

echo "offscreen message-box bloom verification: PASS"
