#!/usr/bin/env bash
set -euo pipefail

NEOZEUS_HOST_HOME=${NEOZEUS_HOST_HOME:-$HOME}
NEOZEUS_HOST_XDG_CONFIG_HOME=${NEOZEUS_HOST_XDG_CONFIG_HOME:-${XDG_CONFIG_HOME:-}}
NEOZEUS_HOST_XDG_STATE_HOME=${NEOZEUS_HOST_XDG_STATE_HOME:-${XDG_STATE_HOME:-}}
NEOZEUS_HOST_XDG_CACHE_HOME=${NEOZEUS_HOST_XDG_CACHE_HOME:-${XDG_CACHE_HOME:-}}
NEOZEUS_HOST_CARGO_HOME=${NEOZEUS_HOST_CARGO_HOME:-${CARGO_HOME:-}}
NEOZEUS_HOST_RUSTUP_HOME=${NEOZEUS_HOST_RUSTUP_HOME:-${RUSTUP_HOME:-}}

neozeus_offscreen_detect_resolution() {
    if [[ -n "${NEOZEUS_OFFSCREEN_WIDTH:-}" && -n "${NEOZEUS_OFFSCREEN_HEIGHT:-}" ]]; then
        printf '%s %s\n' "$NEOZEUS_OFFSCREEN_WIDTH" "$NEOZEUS_OFFSCREEN_HEIGHT"
        return 0
    fi
    if command -v swaymsg >/dev/null 2>&1 && command -v jq >/dev/null 2>&1; then
        local focused_output
        focused_output=$(swaymsg -t get_workspaces | jq -r '.[] | select(.focused == true) | .output' | head -n 1)
        if [[ -n "$focused_output" && "$focused_output" != "null" ]]; then
            local dims
            dims=$(swaymsg -t get_outputs | jq -r --arg output "$focused_output" '
                .[] | select(.name == $output and .active == true) | "\(.current_mode.width) \(.current_mode.height)"
            ' | head -n 1)
            if [[ -n "$dims" ]]; then
                printf '%s\n' "$dims"
                return 0
            fi
        fi
    fi
    printf '1920 1200\n'
}

neozeus_offscreen_prepare_env() {
    local prefix=${1:-neozeus-offscreen}
    NEOZEUS_OFFSCREEN_ROOT=$(mktemp -d "/tmp/${prefix}-XXXXXX")
    export NEOZEUS_OFFSCREEN_ROOT
    export HOME="$NEOZEUS_OFFSCREEN_ROOT/home"
    export XDG_CONFIG_HOME="$NEOZEUS_OFFSCREEN_ROOT/xdg-config"
    export XDG_STATE_HOME="$NEOZEUS_OFFSCREEN_ROOT/xdg-state"
    export XDG_CACHE_HOME="$NEOZEUS_OFFSCREEN_ROOT/xdg-cache"
    export ZDOTDIR="$NEOZEUS_OFFSCREEN_ROOT/zdotdir"
    export ZSHENV="$ZDOTDIR/.zshenv"
    export HISTFILE="$NEOZEUS_OFFSCREEN_ROOT/history"
    export KITTY_CONFIG_DIRECTORY="$NEOZEUS_OFFSCREEN_ROOT/kitty"
    export SHELL="/bin/sh"
    export BASH_ENV="/dev/null"
    export ENV="/dev/null"
    export NEOZEUS_DAEMON_SOCKET_PATH="$NEOZEUS_OFFSCREEN_ROOT/daemon.sock"
    export NEOZEUS_DEBUG_LOG_PATH="$NEOZEUS_OFFSCREEN_ROOT/debug.log"
    export TMUX_TMPDIR="$NEOZEUS_OFFSCREEN_ROOT/tmux"
    unset TMUX
    mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME" "$ZDOTDIR" "$KITTY_CONFIG_DIRECTORY" "$TMUX_TMPDIR"
    : >"$ZSHENV"
}

neozeus_offscreen_cleanup_env() {
    if [[ -n "${TMUX_TMPDIR:-}" ]] && command -v tmux >/dev/null 2>&1; then
        TMUX_TMPDIR="$TMUX_TMPDIR" tmux kill-server >/dev/null 2>&1 || true
    fi
    if [[ -n "${NEOZEUS_OFFSCREEN_ROOT:-}" && -d "${NEOZEUS_OFFSCREEN_ROOT:-}" ]]; then
        rm -rf "$NEOZEUS_OFFSCREEN_ROOT"
    fi
}

neozeus_offscreen_run_capture() {
    local scenario=$1
    local intensity=$2
    local out_path=$3
    local width=$4
    local height=$5
    local verify_delay=${6:-2}
    local capture_delay=${7:-12}
    if [[ $# -gt 7 ]]; then
        shift 7
    else
        set --
    fi

    local root_dir
    root_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
    local app="$root_dir/target/debug/neozeus"
    local run_log="$NEOZEUS_OFFSCREEN_ROOT/run-${scenario}-${intensity}.log"

    env \
        HOME="$NEOZEUS_HOST_HOME" \
        XDG_CONFIG_HOME="$NEOZEUS_HOST_XDG_CONFIG_HOME" \
        XDG_STATE_HOME="$NEOZEUS_HOST_XDG_STATE_HOME" \
        XDG_CACHE_HOME="$NEOZEUS_HOST_XDG_CACHE_HOME" \
        CARGO_HOME="$NEOZEUS_HOST_CARGO_HOME" \
        RUSTUP_HOME="$NEOZEUS_HOST_RUSTUP_HOME" \
        cargo build >"$NEOZEUS_OFFSCREEN_ROOT/build.log" 2>&1
    rm -f "$out_path"

    env \
        HOME="$HOME" \
        XDG_CONFIG_HOME="$XDG_CONFIG_HOME" \
        XDG_STATE_HOME="$XDG_STATE_HOME" \
        XDG_CACHE_HOME="$XDG_CACHE_HOME" \
        ZDOTDIR="$ZDOTDIR" \
        ZSHENV="$ZSHENV" \
        HISTFILE="$HISTFILE" \
        KITTY_CONFIG_DIRECTORY="$KITTY_CONFIG_DIRECTORY" \
        SHELL="$SHELL" \
        BASH_ENV="$BASH_ENV" \
        ENV="$ENV" \
        BEVY_ASSET_ROOT="$root_dir" \
        TMUX_TMPDIR="$TMUX_TMPDIR" \
        NEOZEUS_DAEMON_SOCKET_PATH="$NEOZEUS_DAEMON_SOCKET_PATH" \
        NEOZEUS_OUTPUT_MODE=offscreen \
        NEOZEUS_OFFSCREEN_WIDTH="$width" \
        NEOZEUS_OFFSCREEN_HEIGHT="$height" \
        NEOZEUS_VERIFY_SCENARIO="$scenario" \
        NEOZEUS_VERIFY_DELAY_FRAMES="$verify_delay" \
        NEOZEUS_AGENT_BLOOM_INTENSITY="$intensity" \
        NEOZEUS_CAPTURE_FINAL_FRAME_PATH="$out_path" \
        NEOZEUS_CAPTURE_FINAL_FRAME_DELAY_FRAMES="$capture_delay" \
        NEOZEUS_EXIT_AFTER_CAPTURE=1 \
        "$@" \
        /usr/bin/taskset -c 0 "$app" >"$run_log" 2>&1

    if [[ ! -f "$out_path" ]]; then
        echo "offscreen capture missing: $out_path" >&2
        tail -200 "$run_log" >&2 || true
        return 1
    fi
}

neozeus_offscreen_crop_message_box() {
    local src=$1
    local dst=$2
    local width=$3
    local height=$4
    python - "$src" "$dst" "$width" "$height" <<'PY'
import subprocess, sys
src, dst = sys.argv[1], sys.argv[2]
width, height = int(sys.argv[3]), int(sys.argv[4])
box_w = min(max(width * 0.70, 520.0), 1560.0)
box_h = min(max(height * 0.38, 240.0), 700.0)
x = int(round(width * 0.5 - box_w * 0.5))
y = 8
w = int(round(box_w))
h = int(round(box_h))
subprocess.check_call(["magick", src, "-crop", f"{w}x{h}+{x}+{y}", "+repage", dst])
PY
}

neozeus_offscreen_crop_task_dialog() {
    local src=$1
    local dst=$2
    local width=$3
    local height=$4
    python - "$src" "$dst" "$width" "$height" <<'PY'
import subprocess, sys
src, dst = sys.argv[1], sys.argv[2]
width, height = int(sys.argv[3]), int(sys.argv[4])
box_w = min(max(width * 0.84, 520.0), 1680.0)
box_h = min(max(height * 0.52, 240.0), 760.0)
x = int(round(width * 0.5 - box_w * 0.5))
y = 8
w = int(round(box_w))
h = int(round(box_h))
subprocess.check_call(["magick", src, "-crop", f"{w}x{h}+{x}+{y}", "+repage", dst])
PY
}

neozeus_offscreen_crop_agent_list() {
    local src=$1
    local dst=$2
    local width=$3
    local height=$4
    local crop_w=$(( width < 340 ? width : 340 ))
    local crop_h=$(( height < 220 ? height : 220 ))
    magick "$src" -crop "${crop_w}x${crop_h}+0+0" +repage "$dst"
}

neozeus_offscreen_crop_center_terminal() {
    local src=$1
    local dst=$2
    local width=$3
    local height=$4
    local crop_w=$(( width / 3 ))
    local crop_h=$(( height / 3 ))
    local x=$(( (width - crop_w) / 2 ))
    local y=$(( (height - crop_h) / 2 ))
    magick "$src" -crop "${crop_w}x${crop_h}+${x}+${y}" +repage "$dst"
}

neozeus_offscreen_compare_ae() {
    local lhs=$1
    local rhs=$2
    compare -metric AE "$lhs" "$rhs" null: 2>&1 >/dev/null || true
}

neozeus_offscreen_parse_metric() {
    python - "$1" <<'PY'
import re, sys
value = sys.argv[1]
match = re.search(r"[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?", value, re.I)
if not match:
    raise SystemExit(f"failed to parse metric: {value!r}")
print(match.group(0))
PY
}
