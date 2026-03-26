#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-task-dialog-bloom"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

OFF="$NEOZEUS_OFFSCREEN_ROOT/task-dialog-off.ppm"
ON="$NEOZEUS_OFFSCREEN_ROOT/task-dialog-on.ppm"
OFF_CROP="$NEOZEUS_OFFSCREEN_ROOT/task-dialog-off-crop.png"
ON_CROP="$NEOZEUS_OFFSCREEN_ROOT/task-dialog-on-crop.png"

neozeus_offscreen_run_capture "task-dialog-bloom" "0.0" "$OFF" "$width" "$height"
neozeus_offscreen_run_capture "task-dialog-bloom" "2.0" "$ON" "$width" "$height"
neozeus_offscreen_crop_task_dialog "$OFF" "$OFF_CROP" "$width" "$height"
neozeus_offscreen_crop_task_dialog "$ON" "$ON_CROP" "$width" "$height"

metric=$(neozeus_offscreen_compare_ae "$OFF_CROP" "$ON_CROP")
value=$(neozeus_offscreen_parse_metric "$metric")
python - "$value" <<'PY'
import sys
value = float(sys.argv[1])
threshold = 400.0
print(f"task_dialog_modal_diff={value:.0f}")
print(f"threshold={threshold:.0f}")
if value > threshold:
    raise SystemExit(1)
PY

echo "offscreen task-dialog bloom verification: PASS"
