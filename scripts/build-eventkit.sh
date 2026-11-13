#!/usr/bin/env bash
# Compile the Calendar-app helper and place it where Tauri bundles sidecars.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT="$ROOT/frontend/src-tauri/binaries/meetily-eventkit-aarch64-apple-darwin"
mkdir -p "$(dirname "$OUT")"
swiftc -O -target arm64-apple-macos14.0 \
  "$ROOT/frontend/src-tauri/eventkit/meetily-eventkit.swift" -o "$OUT"
echo "built $OUT"
