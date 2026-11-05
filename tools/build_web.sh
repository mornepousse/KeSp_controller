#!/bin/sh
# Builds the browser version into web/. Serve that folder over http on localhost
# or https, and open it in Chrome or Edge: Web Serial exists nowhere else.
set -e
cd "$(dirname "$0")/.."

WANTED=$(grep -A1 'name = "wasm-bindgen"$' Cargo.lock | tail -1 | cut -d'"' -f2)
TOOL=target/wasm-tools/bin/wasm-bindgen
if [ ! -x "$TOOL" ] || [ "$("$TOOL" --version | cut -d' ' -f2)" != "$WANTED" ]; then
  cargo install wasm-bindgen-cli --version "$WANTED" --locked --root target/wasm-tools
fi

# The Rust toolchain here has no linker for this target: borrow one for this build.
if command -v lld >/dev/null 2>&1; then
  cargo build --release --target wasm32-unknown-unknown
else
  nix shell nixpkgs#lld -c cargo build --release --target wasm32-unknown-unknown
fi
"$TOOL" --target web --no-typescript --out-dir web/pkg \
  target/wasm32-unknown-unknown/release/kesp-controller.wasm
ls -la web/pkg
