#!/bin/sh
# Compila el sidecar de comandos Unix y lo deja donde Tauri lo empaqueta.
# La primera vez hay que correrlo a mano (`pnpm unix:build`).
set -eu
raiz="$(dirname "$(dirname "$0")")"
destino_dir="$raiz/apps/desktop/src-tauri/binaries"
mkdir -p "$destino_dir"
cargo build -p atic-unix --release
triple="$(rustc --print host-tuple)"
cp -f "$raiz/target/release/atic-unix" "$destino_dir/atic-unix-$triple"
echo "atic-unix listo en $destino_dir/atic-unix-$triple"
