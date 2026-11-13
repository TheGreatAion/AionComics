#!/usr/bin/env bash
# score.sh [key]: the panel finder against the answer key (key.jsonl unless another is named).
# Prints each page it gets wrong and how many it gets right.
set -e
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
export PATH="${PATH//\"/}"
AION_PANELS_KEY="${1:-$here/key.jsonl}" cargo test --release --lib score_panels -- --ignored --nocapture 2>&1 | grep -v "^\(running\|test \|$\)" | grep -v "Compiling\|Finished\|Running\|warning"
