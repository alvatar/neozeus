#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TMP_ROOT="$(mktemp -d /tmp/neozeus-install-test-XXXXXX)"
trap 'rm -rf "$TMP_ROOT"' EXIT

assert_file() {
    local path="$1"
    [ -f "$path" ] || {
        echo "missing file: $path" >&2
        exit 1
    }
}

assert_contains() {
    local path="$1"
    local needle="$2"
    grep -F "$needle" "$path" >/dev/null || {
        echo "missing '$needle' in $path" >&2
        exit 1
    }
}

make_fake_repo() {
    local repo_dir="$1"
    mkdir -p "$repo_dir/prompts"
    cp "$ROOT_DIR/install.sh" "$repo_dir/install.sh"
    cp "$ROOT_DIR/install" "$repo_dir/install"
    cp "$ROOT_DIR/prompts/APPEND_SYSTEM.md" "$repo_dir/prompts/APPEND_SYSTEM.md"
    chmod +x "$repo_dir/install.sh" "$repo_dir/install"
}

make_fake_cargo() {
    local fake_bin="$1"
    cat > "$fake_bin/cargo" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if [ "$1" = "build" ]; then
    mkdir -p target/release
    for bin in neozeus neozeus-msg neozeus-tmux neozeus-worktree; do
        cat > "target/release/$bin" <<EOF
#!/usr/bin/env bash
echo $bin "\$@"
EOF
        chmod +x "target/release/$bin"
    done
    exit 0
fi

echo "unexpected cargo args: $*" >&2
exit 1
FAKE
    chmod +x "$fake_bin/cargo"
}

default_repo="$TMP_ROOT/default-repo"
default_home="$TMP_ROOT/default-home"
default_fake_bin="$TMP_ROOT/default-fake-bin"
mkdir -p "$default_home/.local/bin" "$default_fake_bin"
make_fake_repo "$default_repo"
make_fake_cargo "$default_fake_bin"
cat > "$default_home/.local/bin/pi" <<'PI'
#!/usr/bin/env bash
echo "REAL-PI:$*"
PI
chmod +x "$default_home/.local/bin/pi"

HOME="$default_home" PATH="$default_fake_bin:$PATH" "$default_repo/install"

for bin in neozeus neozeus-msg neozeus-tmux neozeus-worktree; do
    assert_file "$default_home/.local/bin/$bin"
    assert_contains "$default_home/.local/bin/$bin" "echo $bin"
done
assert_file "$default_home/.pi/agent/APPEND_SYSTEM.md"
assert_contains "$default_home/.pi/agent/APPEND_SYSTEM.md" "neozeus-worktree merge-finalize"
assert_contains "$default_home/.pi/agent/APPEND_SYSTEM.md" "neozeus-tmux run --name"
assert_file "$default_home/.local/bin/pi"
assert_file "$default_home/.local/bin/pi.neozeus-orig"
assert_file "$default_home/.neozeus/sandbox-paths.conf"
assert_contains "$default_home/.neozeus/sandbox-paths.conf" "~/code"
assert_contains "$default_home/.neozeus/sandbox-paths.conf" "/tmp"
assert_contains "$default_home/.local/bin/pi" "NeoZeus pi wrapper"
assert_contains "$default_home/.local/bin/pi" 'NEOZEUS_HOME_DIR="${HOME}/.neozeus"'
assert_contains "$default_home/.local/bin/pi" 'SANDBOX_CONF="${NEOZEUS_HOME_DIR}/sandbox-paths.conf"'

wrapper_output=$(HOME="$default_home" PATH="$default_fake_bin:$PATH" "$default_home/.local/bin/pi" --no-sandbox hello world)
[ "$wrapper_output" = "REAL-PI:hello world" ] || {
    echo "unexpected wrapper passthrough output: $wrapper_output" >&2
    exit 1
}

no_wrap_repo="$TMP_ROOT/no-wrap-repo"
no_wrap_home="$TMP_ROOT/no-wrap-home"
no_wrap_fake_bin="$TMP_ROOT/no-wrap-fake-bin"
mkdir -p "$no_wrap_home/.local/bin" "$no_wrap_fake_bin"
make_fake_repo "$no_wrap_repo"
make_fake_cargo "$no_wrap_fake_bin"
cat > "$no_wrap_home/.local/bin/pi" <<'PI'
#!/usr/bin/env bash
echo "REAL-PI:$*"
PI
chmod +x "$no_wrap_home/.local/bin/pi"

HOME="$no_wrap_home" PATH="$no_wrap_fake_bin:$PATH" "$no_wrap_repo/install" --no-wrap-pi

assert_file "$no_wrap_home/.local/bin/pi"
[ ! -e "$no_wrap_home/.local/bin/pi.neozeus-orig" ] || {
    echo "wrapper backup should not exist in --no-wrap-pi mode" >&2
    exit 1
}
[ ! -e "$no_wrap_home/.neozeus/sandbox-paths.conf" ] || {
    echo "sandbox config should not exist in --no-wrap-pi mode" >&2
    exit 1
}
assert_contains "$no_wrap_home/.local/bin/pi" "REAL-PI"

echo "install tests: ok"
