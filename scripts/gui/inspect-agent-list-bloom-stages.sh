#!/usr/bin/env bash
set -euo pipefail

echo "GUI bloom-stage inspector removed: tests must use offscreen mode only." >&2
echo "Use ./scripts/offscreen/run-scenario.sh agent-list-bloom <output.ppm> [intensity] for offscreen captures." >&2
exit 2
