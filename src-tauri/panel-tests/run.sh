#!/usr/bin/env bash
# run.sh <label> [list]: the panel finder on every page in a list (pages.txt unless another is
# named). What it found goes to out/<label>.txt, pictures of it to out/<label>-sheets.
# Compare two runs with compare.py; score a run against the answer key with score.sh.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
list="${2:-$here/pages.txt}"
[ -f "$here/$list" ] && list="$here/$list" # a list named without its folder is one of these
mkdir -p "$here/out"
cd "$here/.."
export PATH="${PATH//\"/}" # a stray quote in this machine's PATH breaks cargo's helpers
AION_PANELS_LIST="$list" cargo test --release --lib sample_panels -- --ignored --nocapture > "$here/out/$1.txt" 2>&1 || { tail -20 "$here/out/$1.txt"; exit 1; }
rm -rf "$here/out/$1-sheets"
mv "${TEMP:-/tmp}/aion-panels-sample" "$here/out/$1-sheets"
rm -rf "$here/out/$1-sheets/listings" "$here/out/$1-sheets/pages"
echo "$(grep -c ' panels in ' "$here/out/$1.txt") pages; sheets in out/$1-sheets"
