#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/../.." && pwd)
cd "$repo_root"

tree_root=${1:-/tmp/neozeus-big-tree}
if [[ ! -d "$tree_root" ]]; then
  "$repo_root/scripts/perf/gen-big-tree.sh" "$tree_root" >/dev/null
fi

rm -f /tmp/neozeus-debug.log /tmp/neozeus-bulk-output.log
command=${NEOZEUS_BULK_COMMAND:-"eza --tree -a $tree_root; printf '__DONE__\\n'"}
delay_ms=${NEOZEUS_AUTOVERIFY_DELAY_MS:-500}
timeout_sec=${NEOZEUS_BULK_TIMEOUT_SEC:-12}

NEOZEUS_AUTOVERIFY_DELAY_MS="$delay_ms" \
NEOZEUS_AUTOVERIFY_COMMAND="$command" \
timeout "$timeout_sec" cargo run 2>&1 | tee /tmp/neozeus-bulk-output.log

echo '--- perf summary'
echo "debug_lines=$(wc -l </tmp/neozeus-debug.log 2>/dev/null || echo 0)"
python - <<'PY'
from pathlib import Path
import re
p = Path('/tmp/neozeus-debug.log')
text = p.read_text(errors='ignore') if p.exists() else ''
metrics = {
    'commands_queued': len(re.findall(r'command queued:', text)),
    'status_updates': len(re.findall(r'status snapshot:', text)),
}
for k, v in metrics.items():
    print(f'{k}={v}')
PY
