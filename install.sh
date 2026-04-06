#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="${HOME}/.local/bin"
PI_AGENT_DIR="${HOME}/.pi/agent"
APPEND_SYSTEM_SRC="${SCRIPT_DIR}/prompts/APPEND_SYSTEM.md"
APPEND_SYSTEM_DEST="${PI_AGENT_DIR}/APPEND_SYSTEM.md"
NEOZEUS_HOME_DIR="${HOME}/.neozeus"
SANDBOX_CONF="${NEOZEUS_HOME_DIR}/sandbox-paths.conf"

WRAP_PI=false
NO_BWRAP=false

for arg in "$@"; do
    case "$arg" in
        --wrap-pi)
            WRAP_PI=true
            ;;
        --no-bwrap)
            NO_BWRAP=true
            ;;
        *)
            echo "Unknown option: $arg" >&2
            echo "Usage: $0 [--wrap-pi] [--no-bwrap]" >&2
            exit 1
            ;;
    esac
done

echo "=== NeoZeus installer ==="
echo "binaries: install"
if $WRAP_PI; then
    echo "pi wrapper: enabled"
    if $NO_BWRAP; then
        echo "pi sandbox: disabled (--no-bwrap)"
    else
        echo "pi sandbox: bubblewrap (bwrap)"
    fi
else
    echo "pi wrapper: disabled"
    if $NO_BWRAP; then
        echo "⚠ --no-bwrap ignored because --wrap-pi is not enabled."
    fi
fi
echo ""

if $WRAP_PI && ! $NO_BWRAP && ! command -v bwrap >/dev/null 2>&1; then
    echo "⚠ bwrap not found. Wrapper will fall back to unsandboxed pi runs." >&2
    echo ""
fi

mkdir -p "$BIN_DIR"

BINARIES=(
    neozeus
    neozeus-msg
    neozeus-tmux
    neozeus-worktree
)

(
    cd "$SCRIPT_DIR"
    cargo build --release \
        --bin neozeus \
        --bin neozeus-msg \
        --bin neozeus-tmux \
        --bin neozeus-worktree
)

for bin_name in "${BINARIES[@]}"; do
    src="${SCRIPT_DIR}/target/release/${bin_name}"
    dst="${BIN_DIR}/${bin_name}"
    if [ ! -f "$src" ]; then
        echo "Missing built binary: $src" >&2
        exit 1
    fi
    install -m 0755 "$src" "$dst"
    echo "✓ Installed $dst"
done

mkdir -p "$PI_AGENT_DIR"
if [ -f "$APPEND_SYSTEM_SRC" ]; then
    cp "$APPEND_SYSTEM_SRC" "$APPEND_SYSTEM_DEST"
    echo "✓ Installed pi system prompt appendix at $APPEND_SYSTEM_DEST"
else
    echo "⚠ Missing prompt appendix source: $APPEND_SYSTEM_SRC" >&2
fi

install_pi_wrapper() {
    local pi_bin="$1"
    local pi_orig="$2"
    local wrapper_bwrap_enabled="$3"
    local pi_wrap_tmp="${pi_bin}.neozeus-wrap.tmp.$$"

    cat > "$pi_wrap_tmp" <<'WRAP'
#!/bin/bash
# --- NeoZeus pi wrapper ---
set -euo pipefail

PI_REAL="__PI_REAL__"
WRAPPER_BWRAP_ENABLED="__WRAPPER_BWRAP_ENABLED__"
NEOZEUS_HOME_DIR="${HOME}/.neozeus"
SANDBOX_CONF="${NEOZEUS_HOME_DIR}/sandbox-paths.conf"

export PATH="${HOME}/.local/bin:${PATH}"

: "${NPM_CONFIG_PREFIX:=${HOME}/.local}"
export NPM_CONFIG_PREFIX
export npm_config_prefix="$NPM_CONFIG_PREFIX"

: "${NPM_CONFIG_CACHE:=${HOME}/.npm}"
export NPM_CONFIG_CACHE
export npm_config_cache="$NPM_CONFIG_CACHE"

if [ ! -e "$PI_REAL" ] && [ ! -L "$PI_REAL" ]; then
    echo "NeoZeus pi wrapper error: original pi not found at $PI_REAL" >&2
    exit 1
fi

PASSTHROUGH_ARGS=()
NO_SANDBOX=false
for arg in "$@"; do
    if [ "$arg" = "--no-sandbox" ]; then
        NO_SANDBOX=true
    else
        PASSTHROUGH_ARGS+=("$arg")
    fi
done

if $NO_SANDBOX; then
    exec "$PI_REAL" "${PASSTHROUGH_ARGS[@]}"
fi

if [ "$WRAPPER_BWRAP_ENABLED" != "1" ]; then
    exec "$PI_REAL" "${PASSTHROUGH_ARGS[@]}"
fi

if ! command -v bwrap >/dev/null 2>&1; then
    echo "NeoZeus pi wrapper warning: bwrap not found; running without sandbox" >&2
    exec "$PI_REAL" "${PASSTHROUGH_ARGS[@]}"
fi

PI_AGENT_DIR="${HOME}/.pi/agent"
mkdir -p "$PI_AGENT_DIR/sessions" \
         "${HOME}/.npm" \
         "${HOME}/.local/bin" \
         "${HOME}/.local/lib/node_modules" \
         "${HOME}/.cargo" \
         "${HOME}/.rustup" \
         "${HOME}/.codex" \
         "${HOME}/.claude" \
         "$NEOZEUS_HOME_DIR"
touch "$PI_AGENT_DIR/auth.json" \
      "$PI_AGENT_DIR/mcp-cache.json" \
      "$PI_AGENT_DIR/mcp-npx-cache.json"

BWRAP_ARGS=()
MOUNT_DIRS=()

bwrap_bind() {
    local path="$1"
    [ -e "$path" ] || return 0
    BWRAP_ARGS+=("--bind" "$path" "$path")
    if [ -d "$path" ]; then
        MOUNT_DIRS+=("$path")
    fi
}

bwrap_ro() {
    local path="$1"
    [ -e "$path" ] || return 0
    BWRAP_ARGS+=("--ro-bind" "$path" "$path")
    if [ -d "$path" ]; then
        MOUNT_DIRS+=("$path")
    fi
}

bwrap_dev() {
    local path="$1"
    [ -e "$path" ] || return 0
    BWRAP_ARGS+=("--dev-bind" "$path" "$path")
}

path_is_mounted_dir() {
    local target="$1"
    for dir in "${MOUNT_DIRS[@]}"; do
        if [ "$target" = "$dir" ] || [[ "$target" == "$dir/"* ]]; then
            return 0
        fi
    done
    return 1
}

for d in "${HOME}" \
         "${HOME}/code" \
         "$NEOZEUS_HOME_DIR" \
         "${HOME}/.pi" \
         "$PI_AGENT_DIR" \
         "$PI_AGENT_DIR/sessions" \
         "${HOME}/.local" \
         "${HOME}/.local/bin" \
         "${HOME}/.local/lib" \
         "${HOME}/.local/lib/node_modules" \
         "${HOME}/.npm" \
         "${HOME}/.cargo" \
         "${HOME}/.rustup" \
         "${HOME}/.codex" \
         "${HOME}/.claude"; do
    BWRAP_ARGS+=("--dir" "$d")
done

for p in /usr /lib /lib64 /bin /sbin /etc /run /sys; do
    bwrap_ro "$p"
done

bwrap_bind "$NEOZEUS_HOME_DIR"
bwrap_bind "${HOME}/.pi"
bwrap_bind "${HOME}/.local/bin"
bwrap_bind "${HOME}/.local/lib/node_modules"
bwrap_bind "${HOME}/.npm"
bwrap_bind "${HOME}/.cargo"
bwrap_bind "${HOME}/.rustup"
bwrap_bind "${HOME}/.codex"
bwrap_bind "${HOME}/.claude"
bwrap_ro "${HOME}/.gitconfig"

bwrap_dev /dev/dri
bwrap_dev /dev/nvidiactl
bwrap_dev /dev/nvidia0
bwrap_dev /dev/nvidia1
bwrap_dev /dev/nvidia-modeset
bwrap_dev /dev/nvidia-uvm
bwrap_dev /dev/nvidia-uvm-tools
bwrap_dev /dev/nvidia-caps
bwrap_bind /dev/shm

if [ -f "$SANDBOX_CONF" ]; then
    while IFS= read -r line; do
        line="${line%%#*}"
        line="$(echo "$line" | xargs)"
        [ -z "$line" ] && continue

        expanded="${line/#\~/$HOME}"
        if [[ "$expanded" != /* ]]; then
            continue
        fi
        if [ ! -e "$expanded" ]; then
            echo "NeoZeus pi wrapper warning: sandbox path does not exist, skipping: $expanded" >&2
            continue
        fi
        bwrap_bind "$expanded"
    done < "$SANDBOX_CONF"
fi

BWRAP_ARGS+=("--chmod" "0111" "${HOME}")

BWRAP_CHDIR="/"
if path_is_mounted_dir "$PWD"; then
    BWRAP_CHDIR="$PWD"
elif path_is_mounted_dir "/tmp"; then
    BWRAP_CHDIR="/tmp"
fi

exec bwrap \
    --die-with-parent \
    --proc /proc \
    --dev /dev \
    --setenv GIT_SSH_COMMAND "ssh -F /dev/null -o StrictHostKeyChecking=no" \
    --setenv SSH_AUTH_SOCK "${SSH_AUTH_SOCK:-/run/user/$(id -u)/ssh-agent.socket}" \
    --chdir "$BWRAP_CHDIR" \
    "${BWRAP_ARGS[@]}" \
    "$PI_REAL" "${PASSTHROUGH_ARGS[@]}"
WRAP

    python3 - "$pi_wrap_tmp" "$pi_orig" "$wrapper_bwrap_enabled" <<'PY'
from pathlib import Path
import sys

wrapper_path = Path(sys.argv[1])
pi_orig = sys.argv[2]
wrapper_bwrap_enabled = sys.argv[3]
text = wrapper_path.read_text()
text = text.replace("__PI_REAL__", pi_orig)
text = text.replace("__WRAPPER_BWRAP_ENABLED__", wrapper_bwrap_enabled)
wrapper_path.write_text(text)
PY

    chmod +x "$pi_wrap_tmp"
    mv -f "$pi_wrap_tmp" "$pi_bin"
    echo "✓ Installed NeoZeus pi wrapper at $pi_bin"
}

if $WRAP_PI; then
    PI_BIN="${BIN_DIR}/pi"
    PI_ORIG="${BIN_DIR}/pi.neozeus-orig"
    WRAPPER_BWRAP_ENABLED=1

    if $NO_BWRAP; then
        WRAPPER_BWRAP_ENABLED=0
    fi

    if [ ! -e "$PI_BIN" ] && [ ! -L "$PI_BIN" ]; then
        echo "⚠ $PI_BIN not found; skipping pi wrapper" >&2
    else
        if grep -q "NeoZeus pi wrapper" "$PI_BIN" 2>/dev/null; then
            echo "✓ pi already wrapped by NeoZeus (refreshing wrapper)"
        else
            if [ -e "$PI_ORIG" ] || [ -L "$PI_ORIG" ]; then
                echo "✓ Backup already exists at $PI_ORIG (reusing)"
            else
                mv "$PI_BIN" "$PI_ORIG"
                echo "✓ Backed up original pi to $PI_ORIG"
            fi
        fi

        if [ "$WRAPPER_BWRAP_ENABLED" = "1" ]; then
            mkdir -p "$NEOZEUS_HOME_DIR"
            if [ ! -f "$SANDBOX_CONF" ]; then
                cat > "$SANDBOX_CONF" <<'SCONF'
# Writable paths for pi sandbox, one per line.
# ~ is expanded to $HOME. Lines starting with # are ignored.
# Each absolute path listed here is mounted read-write if it exists.
# Non-existent paths are skipped with a warning on stderr.
~/code
/tmp
SCONF
                echo "✓ Created default sandbox config: $SANDBOX_CONF"
            else
                echo "✓ Sandbox config already exists: $SANDBOX_CONF (preserved)"
            fi
        fi

        install_pi_wrapper "$PI_BIN" "$PI_ORIG" "$WRAPPER_BWRAP_ENABLED"
    fi
fi

echo ""
echo "Done."
