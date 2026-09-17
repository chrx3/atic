#!/usr/bin/env bash
# Entorno de desarrollo en macOS/Linux. Espejo de `scripts/win-env.ps1`.
#
# Se carga con `source` antes de compilar o de correr la app:
#
#   . scripts/mac-env.sh
#   pnpm tauri dev
#
# Qué hace:
# - Detecta el wrapper de rustc (`sccache` en `~/.cargo/config.toml`). Con
#   sccache el incremental está PROHIBIDO: rustc no compila y el build falla
#   con «incremental compilation is prohibited». Si no hay wrapper, activa
#   `CARGO_INCREMENTAL=1`, que en macOS es estable y ahorra recompilar.
# - Busca `libclang` de Homebrew si el sistema no lo tiene a mano: lo necesita
#   `bindgen` para compilar whisper-rs. Si ya hay uno configurado, lo respeta.
# - Avisa si falta algo de la lista (cmake, clang, rustc, node, pnpm).
#
# No llama a `exit` para no cerrar la shell de quien lo carga.

_atic_ok() { command -v "$1" >/dev/null 2>&1; }

_atic_wrapper="${RUSTC_WRAPPER:-}"
if [ -z "$_atic_wrapper" ]; then
  for _atic_cfg in "$HOME/.cargo/config.toml" "$HOME/.cargo/config"; do
    [ -f "$_atic_cfg" ] || continue
    _atic_wrapper="$(sed -n 's/^[[:space:]]*rustc-wrapper[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' "$_atic_cfg" | head -1)"
    [ -n "$_atic_wrapper" ] && break
  done
fi

if [ -n "$_atic_wrapper" ]; then
  # sccache (y la mayoría de los wrappers) no admiten compilación incremental.
  if [ -n "${CARGO_INCREMENTAL:-}" ]; then
    echo "atic dev: «$_atic_wrapper» no admite CARGO_INCREMENTAL; lo quito."
    unset CARGO_INCREMENTAL
  fi
else
  export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"
fi

# --- libclang ----------------------------------------------------------------
if [ -z "${LIBCLANG_PATH:-}" ] && command -v brew >/dev/null 2>&1; then
  _atic_llvm="$(brew --prefix llvm 2>/dev/null || true)"
  if [ -n "$_atic_llvm" ] && [ -f "$_atic_llvm/lib/libclang.dylib" ]; then
    export LIBCLANG_PATH="$_atic_llvm/lib"
  fi
fi

# --- Avisos ------------------------------------------------------------------
_atic_falta=""
for _atic_tool in cmake clang rustc node pnpm; do
  _atic_ok "$_atic_tool" || _atic_falta="$_atic_falta $_atic_tool"
done

echo "atic dev (macOS/Linux)"
echo "  wrapper de rustc: ${_atic_wrapper:-(ninguno)}"
echo "  CARGO_INCREMENTAL=${CARGO_INCREMENTAL:-(no)}"
echo "  LIBCLANG_PATH=${LIBCLANG_PATH:-(no configurado)}"
if [ -n "$_atic_wrapper" ]; then
  echo "  con sccache, el ciclo interno es «cargo ck» (sin linkear): mira docs/DEVELOPMENT.md"
fi
if [ -n "$_atic_falta" ]; then
  echo "  falta:$_atic_falta  (mira docs/MACOS.md)"
fi

unset -f _atic_ok
unset _atic_wrapper _atic_cfg _atic_llvm _atic_falta _atic_tool
