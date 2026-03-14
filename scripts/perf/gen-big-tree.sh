#!/usr/bin/env bash
set -euo pipefail

root=${1:-/tmp/neozeus-big-tree}
dirs=${DIRS:-60}
subs=${SUBS:-8}
files=${FILES:-18}

rm -rf "$root"
for ((i=0; i<dirs; i++)); do
  for ((j=0; j<subs; j++)); do
    dir=$(printf '%s/dir_%03d/sub_%02d' "$root" "$i" "$j")
    mkdir -p "$dir"
    for ((k=0; k<files; k++)); do
      printf '%d-%d-%d\n' "$i" "$j" "$k" >"$(printf '%s/file_%02d.txt' "$dir" "$k")"
    done
  done
done

echo "$root"
