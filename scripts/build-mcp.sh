#!/bin/sh
# Compila el sidecar MCP y lo deja donde Tauri lo empaqueta.
# La primera vez hay que correrlo a mano (`pnpm mcp:build`).
# Sin argumentos usa el triple del host; con uno cross-compila (el DMG
# universal necesita `aarch64-apple-darwin` y `x86_64-apple-darwin`).
set -eu
raiz="$(dirname "$(dirname "$0")")"
destino_dir="$raiz/apps/desktop/src-tauri/binaries"
triple="${1:-$(rustc --print host-tuple)}"
mkdir -p "$destino_dir"
cargo build -p atic-mcp --release --target "$triple"
cp -f "$raiz/target/$triple/release/atic-mcp" "$destino_dir/atic-mcp-$triple"
echo "atic-mcp listo en $destino_dir/atic-mcp-$triple"
