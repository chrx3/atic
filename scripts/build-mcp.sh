#!/bin/sh
# Compila el sidecar MCP y lo deja donde Tauri lo empaqueta.
# La primera vez hay que correrlo a mano (`pnpm mcp:build`).
set -eu
raiz="$(dirname "$(dirname "$0")")"
destino_dir="$raiz/apps/desktop/src-tauri/binaries"
mkdir -p "$destino_dir"
cargo build -p atic-mcp --release
triple="$(rustc --print host-tuple)"
cp -f "$raiz/target/release/atic-mcp" "$destino_dir/atic-mcp-$triple"
echo "atic-mcp listo en $destino_dir/atic-mcp-$triple"
