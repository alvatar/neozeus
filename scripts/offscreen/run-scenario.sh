#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"
# shellcheck source=./common.sh
source "$ROOT_DIR/scripts/offscreen/common.sh"

scenario=${1:?usage: $0 <scenario> <output-path> [intensity] [width] [height]}
out_path=${2:?usage: $0 <scenario> <output-path> [intensity] [width] [height]}
intensity=${3:-2.0}
shift 3 || true

if [[ $# -ge 2 ]]; then
    width=$1
    height=$2
else
    read -r width height < <(neozeus_offscreen_detect_resolution)
fi

neozeus_offscreen_prepare_env "neozeus-${scenario}"
trap neozeus_offscreen_cleanup_env EXIT
neozeus_offscreen_run_capture "$scenario" "$intensity" "$out_path" "$width" "$height"

echo "scenario=$scenario"
echo "capture=$out_path"
echo "resolution=${width}x${height}"
