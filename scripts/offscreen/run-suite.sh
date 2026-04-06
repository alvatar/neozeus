#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

declare -A LABELS=(
    [agent-list]="agent-list bloom"
    [working-contract]="working-state contract"
    [message-box]="message-box bloom layering"
    [task-dialog]="task-dialog bloom layering"
    [inspect-switch]="inspect switch visibility"
)

declare -A SCRIPTS=(
    [agent-list]="$ROOT_DIR/scripts/offscreen/verify-agent-list-bloom.sh"
    [working-contract]="$ROOT_DIR/scripts/offscreen/verify-working-state-contract.sh"
    [message-box]="$ROOT_DIR/scripts/offscreen/verify-message-box-bloom.sh"
    [task-dialog]="$ROOT_DIR/scripts/offscreen/verify-task-dialog-bloom.sh"
    [inspect-switch]="$ROOT_DIR/scripts/offscreen/verify-inspect-switch-latency.sh"
)

TARGETS=(agent-list working-contract message-box task-dialog inspect-switch)
if [[ $# -gt 0 ]]; then
    TARGETS=()
    for arg in "$@"; do
        case "$arg" in
            all)
                TARGETS=(agent-list working-contract message-box task-dialog inspect-switch)
                ;;
            agent-list|working-contract|message-box|task-dialog|inspect-switch)
                TARGETS+=("$arg")
                ;;
            *)
                echo "unknown offscreen suite target: $arg" >&2
                exit 2
                ;;
        esac
    done
fi

for target in "${TARGETS[@]}"; do
    echo ">>> offscreen suite: ${LABELS[$target]}"
    "${SCRIPTS[$target]}"
done

echo "offscreen suite: PASS"
