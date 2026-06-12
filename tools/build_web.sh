#!/usr/bin/env bash
# Build MineRust for the browser (WebAssembly) and stage it under web/.
#
#   ./tools/build_web.sh        # release build → web/minerust.wasm
#   python3 -m http.server -d web 8000   # then open http://localhost:8000
#
# The single-player game runs fully in the browser. Native-only features that
# need raw TCP sockets or OS threads (LAN multiplayer, connecting to a
# Minecraft server) are inert on the web — browsers can't open raw sockets.
set -euo pipefail
cd "$(dirname "$0")/.."

rustup target add wasm32-unknown-unknown >/dev/null 2>&1 || true
echo "building wasm32-unknown-unknown (release)…"
cargo build --release --target wasm32-unknown-unknown

cp target/wasm32-unknown-unknown/release/minerust.wasm web/minerust.wasm
echo "staged web/minerust.wasm ($(du -h web/minerust.wasm | cut -f1))"
echo "serve it:  python3 -m http.server -d web 8000"
echo "then open: http://localhost:8000"
