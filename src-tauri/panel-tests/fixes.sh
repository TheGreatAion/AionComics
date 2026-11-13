#!/usr/bin/env bash
# fixes.sh: the panels fixed by hand in the app, printed as answer key lines. Add the ones you
# want to key.jsonl (the app must have been run at least once; its database is only read).
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/.."
export PATH="${PATH//\"/}"
cargo test --release --lib hand_fixes -- --ignored --nocapture 2>/dev/null | grep '^{'
