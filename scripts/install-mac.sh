#!/usr/bin/env bash
# Build and install Graphite on an Apple Silicon Mac.
# Run from anywhere: bash scripts/install-mac.sh
# Takes ~10-20 min the first time (Rust release build). Safe to re-run.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail() { echo "ERROR: $*" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] && [ "$(uname -m)" = "arm64" ] || fail "Apple Silicon macOS required"
xcode-select -p >/dev/null 2>&1 || fail "Xcode command line tools missing: run  xcode-select --install"
command -v cargo >/dev/null || fail "Rust missing: install from https://rustup.rs  then open a new shell"
command -v node  >/dev/null || fail "Node.js missing: brew install node"
command -v cmake >/dev/null || fail "cmake missing: brew install cmake"

echo "==> 1/4 llama-helper sidecar (required by the bundle)"
(cd llama-helper && cargo build --release --features metal)
mkdir -p frontend/src-tauri/binaries
cp target/release/llama-helper frontend/src-tauri/binaries/llama-helper-aarch64-apple-darwin

echo "==> 2/4 frontend dependencies"
(cd frontend && npx --yes pnpm@10 install)

echo "==> 3/4 app bundle"
# STRIP=none and no-OpenSSL are required on current macOS (proc-macro dylib
# link error; brew openssl breaks llama.cpp httplib). The updater-signing
# "no private key" error at the very end is expected: the .app is already built.
# Stable signing identity keeps macOS privacy grants (Accessibility, Screen
# Recording) across rebuilds. Create it once with scripts/make-signing-cert.sh;
# without it the build is ad-hoc signed and macOS asks again after each install.
if security find-identity -p codesigning 2>/dev/null | grep -q "Graphite Local"; then
  export APPLE_SIGNING_IDENTITY="Graphite Local"
  echo "Signing with the local 'Graphite Local' identity"
fi

(cd frontend && CARGO_PROFILE_RELEASE_STRIP=none CMAKE_DISABLE_FIND_PACKAGE_OpenSSL=ON \
  npx pnpm@10 run tauri:build -- --bundles app) || echo "(build command exited non-zero; checking for the bundle)"

APP="$ROOT/target/release/bundle/macos/Graphite.app"
[ -d "$APP" ] || fail "bundle not found at $APP - read the build output above"

echo "==> 4/4 install to /Applications"
osascript -e 'tell application "Graphite" to quit' >/dev/null 2>&1 || true
for _ in $(seq 1 15); do pgrep -x meetily >/dev/null || break; sleep 1; done
if [ -d /Applications/Graphite.app ]; then
  mkdir -p "$HOME/meetily-backup"
  rm -rf "$HOME/meetily-backup/meetily-previous.app"
  cp -R /Applications/Graphite.app "$HOME/meetily-backup/meetily-previous.app"
  rm -rf /Applications/Graphite.app
fi
cp -R "$APP" /Applications/Graphite.app
xattr -cr /Applications/Graphite.app   # unsigned build: clear quarantine
open /Applications/Graphite.app
echo "Done. Graphite is running (menu bar) and will launch at login."
