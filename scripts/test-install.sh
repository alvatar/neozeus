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
    cp "$ROOT_DIR/prompts/APPEND_SYSTEM.md" "$repo_dir/prompts/APPEND_SYSTEM.md"
    chmod +x "$repo_dir/install.sh"
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

plain_repo="$TMP_ROOT/plain-repo"
plain_home="$TMP_ROOT/plain-home"
plain_fake_bin="$TMP_ROOT/plain-fake-bin"
mkdir -p "$plain_home" "$plain_fake_bin"
make_fake_repo "$plain_repo"
make_fake_cargo "$plain_fake_bin"

HOME="$plain_home" PATH="$plain_fake_bin:$PATH" bash "$plain_repo/install.sh"

for bin in neozeus neozeus-msg neozeus-tmux neozeus-worktree; do
    assert_file "$plain_home/.local/bin/$bin"
    assert_contains "$plain_home/.local/bin/$bin" "echo $bin"
done
assert_file "$plain_home/.pi/agent/APPEND_SYSTEM.md"
assert_contains "$plain_home/.pi/agent/APPEND_SYSTEM.md" "neozeus-worktree merge-finalize"
assert_contains "$plain_home/.pi/agent/APPEND_SYSTEM.md" "neozeus-tmux run --name"

wrap_repo="$TMP_ROOT/wrap-repo"
wrap_home="$TMP_ROOT/wrap-home"
wrap_fake_bin="$TMP_ROOT/wrap-fake-bin"
mkdir -p "$wrap_home/.local/bin" "$wrap_fake_bin"
make_fake_repo "$wrap_repo"
make_fake_cargo "$wrap_fake_bin"
cat > "$wrap_home/.local/bin/pi" <<'PI'
#!/usr/bin/env bash
echo "REAL-PI:$*"
PI
chmod +x "$wrap_home/.local/bin/pi"

HOME="$wrap_home" PATH="$wrap_fake_bin:$PATH" bash "$wrap_repo/install.sh" --wrap-pi

assert_file "$wrap_home/.local/bin/pi"
assert_file "$wrap_home/.local/bin/pi.neozeus-orig"
assert_file "$wrap_home/.neozeus/sandbox-paths.conf"
assert_contains "$wrap_home/.neozeus/sandbox-paths.conf" "~/code"
assert_contains "$wrap_home/.neozeus/sandbox-paths.conf" "/tmp"
assert_contains "$wrap_home/.local/bin/pi" "NeoZeus pi wrapper"
assert_contains "$wrap_home/.local/bin/pi" 'NEOZEUS_HOME_DIR="${HOME}/.neozeus"'
assert_contains "$wrap_home/.local/bin/pi" 'SANDBOX_CONF="${NEOZEUS_HOME_DIR}/sandbox-paths.conf"'

wrapper_output=$(HOME="$wrap_home" PATH="$wrap_fake_bin:$PATH" "$wrap_home/.local/bin/pi" --no-sandbox hello world)
[ "$wrapper_output" = "REAL-PI:hello world" ] || {
    echo "unexpected wrapper passthrough output: $wrapper_output" >&2
    exit 1
}

echo "install tests: ok"
