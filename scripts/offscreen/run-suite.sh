#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$ROOT_DIR"

run_agent_list=1
run_message_box=1
run_task_dialog=1
run_switch=1

if [[ $# -gt 0 ]]; then
    run_agent_list=0
    run_message_box=0
    run_task_dialog=0
    run_switch=0
    for arg in "$@"; do
        case "$arg" in
            all)
                run_agent_list=1
                run_message_box=1
                run_task_dialog=1
                run_switch=1
                ;;
            agent-list)
                run_agent_list=1
                ;;
            message-box)
                run_message_box=1
                ;;
            task-dialog)
                run_task_dialog=1
                ;;
            inspect-switch)
                run_switch=1
                ;;
            *)
                echo "unknown offscreen suite target: $arg" >&2
                exit 2
                ;;
        esac
    done
fi

if [[ "$run_agent_list" -eq 1 ]]; then
    echo ">>> offscreen suite: agent-list bloom"
    "$ROOT_DIR/scripts/offscreen/verify-agent-list-bloom.sh"
fi
if [[ "$run_message_box" -eq 1 ]]; then
    echo ">>> offscreen suite: message-box bloom layering"
    "$ROOT_DIR/scripts/offscreen/verify-message-box-bloom.sh"
fi
if [[ "$run_task_dialog" -eq 1 ]]; then
    echo ">>> offscreen suite: task-dialog bloom layering"
    "$ROOT_DIR/scripts/offscreen/verify-task-dialog-bloom.sh"
fi
if [[ "$run_switch" -eq 1 ]]; then
    echo ">>> offscreen suite: inspect switch visibility"
    "$ROOT_DIR/scripts/offscreen/verify-inspect-switch-latency.sh"
fi

echo "offscreen suite: PASS"
