#!/usr/bin/env bash
set -euo pipefail

echo "GUI visible-terminal verifier removed: tests must use offscreen mode only." >&2
echo "Use ./scripts/offscreen/verify-inspect-switch-latency.sh or ./scripts/offscreen/run-scenario.sh inspect-switch-latency <output.ppm>." >&2
exit 2
