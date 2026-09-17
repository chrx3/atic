#!/usr/bin/env bash
# Arranca Atic en desarrollo, igual que `apps/desktop/dev-atic.bat` en Windows.
#
#   ./dev-atic.sh            # ventana principal + pill
#   ./dev-atic.sh --trace    # además, trazas de geometría (ver README)
#
# Carga `scripts/mac-env.sh` (incremental + libclang) antes de `tauri dev`.
set -euo pipefail

AQUI="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RAIZ="$(cd "$AQUI/../.." && pwd)"

# shellcheck source=../../scripts/mac-env.sh
. "$RAIZ/scripts/mac-env.sh"

if [ "${1:-}" = "--trace" ]; then
  export RUST_LOG="info,pill_geo=debug,paste_geo=debug"
fi

cd "$AQUI"
exec pnpm tauri dev
