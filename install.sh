#!/usr/bin/env bash
set -euo pipefail

script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
repo_root=$script_dir
install_dir="$HOME/local/bin"

if [[ $# -ne 0 ]]; then
    printf 'usage: %s\n' "$0" >&2
    exit 2
fi

cd "$repo_root"
cargo build --release --bins
mkdir -p "$install_dir"

mapfile -t bin_names < <(
    python3 - <<'PY'
import json
import subprocess

metadata = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        text=True,
    )
)
package = metadata["packages"][0]
for target in package["targets"]:
    if "bin" in target["kind"]:
        print(target["name"])
PY
)

for bin_name in "${bin_names[@]}"; do
    src="$repo_root/target/release/$bin_name"
    dst="$install_dir/$bin_name"
    install -m 0755 "$src" "$dst"
    printf 'installed %s\n' "$dst"
done
