#!/usr/bin/env bash
# Renders every character layer on black and white, then mattes them into
# layers. Echo's beak is its mouth, so it has no smile or open layer.
set -euo pipefail
cd "$(dirname "$0")"
cargo build
BIN=${CARGO_TARGET_DIR:-target}/debug/echobird-supply-art
mkdir -p frames
for c in echo sprout twin zip puff; do
  for l in body eyes joy wince smile open; do
    if [ "$c" = echo ] && { [ "$l" = smile ] || [ "$l" = open ]; }; then continue; fi
    for bg in black white; do "$BIN" --char "$c" --layer "$l" --bg "$bg" --out "frames/$c-$l-$bg.png"; done
  done
done
python3 matte.py frames layers
