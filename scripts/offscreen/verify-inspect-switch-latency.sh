#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

neozeus_offscreen_prepare_env "neozeus-inspect-switch"
trap neozeus_offscreen_cleanup_env EXIT
read -r width height < <(neozeus_offscreen_detect_resolution)

BASE="$NEOZEUS_OFFSCREEN_ROOT/agent-list-base.ppm"
SWITCHED="$NEOZEUS_OFFSCREEN_ROOT/inspect-switched.ppm"

neozeus_offscreen_run_capture "agent-list-bloom" "0.0" "$BASE" "$width" "$height"
neozeus_offscreen_run_capture "inspect-switch-latency" "0.0" "$SWITCHED" "$width" "$height"

metric=$(neozeus_offscreen_compare_ae "$BASE" "$SWITCHED")
value=$(neozeus_offscreen_parse_metric "$metric")
python - "$value" <<'PY'
import sys
value = float(sys.argv[1])
threshold = 5000.0
print(f"inspect_switch_diff={value:.0f}")
print(f"threshold={threshold:.0f}")
if value <= threshold:
    raise SystemExit(1)
PY

echo "offscreen inspect-switch verification: PASS"
