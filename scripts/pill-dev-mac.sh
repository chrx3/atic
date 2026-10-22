#!/usr/bin/env bash
# La pill GPUI en macOS: compila, la empaqueta como .app, la firma y la abre.
# Es el `pill-dev.ps1` de Windows para Mac.
#
# Empaquetada, la pill tiene su propio nombre en Ajustes del Sistema (los
# permisos de Accesibilidad, Pantalla y Micrófono quedan a nombre de ella y no
# de la terminal), avisos con clic y su ícono. Firmada con la identidad propia
# de ~/.tauri/atic-signing-identity («Atic Dev», ver docs/MACOS.md) macOS
# conserva los permisos entre compilaciones; sin ella, firma ad-hoc y hay que
# volver a darlos después de cada compilación.
#
# Uso (desde cualquier carpeta):
#   bash scripts/pill-dev-mac.sh            # debug, cierra la anterior y abre
#   bash scripts/pill-dev-mac.sh --release  # optimizada
#   bash scripts/pill-dev-mac.sh --no-open  # solo compila y empaqueta
#   bash scripts/pill-dev-mac.sh --install  # además la copia a /Applications
#   PILL_FAKE_NOTCH=1 bash scripts/pill-dev-mac.sh  # simula el notch de una MacBook
#
# Las variables `PILL_*` de la terminal pasan a la pill abierta.
#
# Compila en ~/Library/Caches/atic-gpui-target (o $CARGO_TARGET_DIR) para no
# chocar con `tauri dev`. La .app queda en <target>/<perfil>/Atic Pill.app.

set -euo pipefail

profile=debug
open_app=1
install=0
for arg in "$@"; do
  case "$arg" in
    --release) profile=release ;;
    --no-open) open_app=0 ;;
    --install) install=1 ;;
    *) echo "Opción desconocida: $arg" >&2; exit 1 ;;
  esac
done

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pill="$repo/prototypes/pill-gpui"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/Library/Caches/atic-gpui-target}"

if [[ ! -d "$repo/../gpui-m3" ]]; then
  echo "Falta ../gpui-m3 (los componentes de Atic Code): git clone https://github.com/chrx3/gpui-m3.git $(cd "$repo/.." && pwd)/gpui-m3" >&2
  exit 1
fi

# El perfil dev de la pill usa incremental; sccache lo prohíbe y el build falla
# («incremental compilation is prohibited»). Con un wrapper, sin incremental.
wrapper="${RUSTC_WRAPPER:-}"
if [[ -z "$wrapper" && -f "$HOME/.cargo/config.toml" ]]; then
  wrapper="$(sed -n 's/^[[:space:]]*rustc-wrapper[[:space:]]*=[[:space:]]*"\(.*\)".*/\1/p' "$HOME/.cargo/config.toml" | head -1)"
fi
[[ -n "$wrapper" ]] && export CARGO_INCREMENTAL=0

cargo_args=(build --manifest-path "$pill/Cargo.toml" -j "${PILL_JOBS:-4}")
[[ "$profile" == release ]] && cargo_args+=(--release)
echo "→ compilando la pill ($profile)…"
cargo "${cargo_args[@]}"

# El agente de Atic Code, empaquetado en un solo archivo.
if [[ ! -f "$pill/sidecar/dist/agent.mjs" || "$pill/sidecar/agent.mjs" -nt "$pill/sidecar/dist/agent.mjs" ]]; then
  echo "→ empaquetando el agente de Atic Code…"
  (cd "$pill/sidecar" && pnpm install --frozen-lockfile >/dev/null && pnpm run build >/dev/null)
fi

# La voz de macOS y Apple Intelligence: un ayudante en Swift (macOS 26+).
apple_bin="$CARGO_TARGET_DIR/$profile/atic-apple"
if [[ ! -x "$apple_bin" || "$pill/macos/apple/main.swift" -nt "$apple_bin" ]]; then
  echo "→ compilando atic-apple (Swift)…"
  swiftc -parse-as-library -O -target "$(uname -m)-apple-macos26.0" "$pill/macos/apple/main.swift" -o "$apple_bin" ||
    echo "No se pudo compilar atic-apple: la pill sigue con Groq." >&2
fi

version="$(node -pe "require('$repo/apps/desktop/src-tauri/tauri.conf.json').version" 2>/dev/null || echo 0.0.0)"
name="Atic Pill"
id="com.ciat.atic.pill"
app="$CARGO_TARGET_DIR/$profile/$name.app"
contents="$app/Contents"

echo "→ armando $app"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Resources/sidecar"
cp "$CARGO_TARGET_DIR/$profile/pill-gpui" "$contents/MacOS/atic-pill"
# Un script no es código firmable: va en Resources, no junto al ejecutable.
cp "$pill/sidecar/dist/agent.mjs" "$contents/Resources/sidecar/agent.mjs"
cp "$repo/apps/desktop/src-tauri/icons/icon.icns" "$contents/Resources/AppIcon.icns"
[[ -x "$apple_bin" ]] && cp "$apple_bin" "$contents/MacOS/atic-apple"
# El servidor MCP de Atic, si está compilado (cargo build -p atic-mcp --release).
for mcp in "$repo/target/release/atic-mcp" "/Applications/Atic.app/Contents/MacOS/atic-mcp"; do
  if [[ -x "$mcp" ]]; then
    cp "$mcp" "$contents/MacOS/atic-mcp"
    break
  fi
done
sed -e "s/@NAME@/$name/g" -e "s/@ID@/$id/g" -e "s/@VERSION@/$version/g" \
  "$pill/macos/Info.plist" > "$contents/Info.plist"
printf 'APPL????' > "$contents/PkgInfo"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$identity" && -f "$HOME/.tauri/atic-signing-identity" ]]; then
  identity="$(tr -d '\n' < "$HOME/.tauri/atic-signing-identity")"
fi
if [[ -n "$identity" && "$identity" != "-" ]] && ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "La identidad '$identity' no está en el Llavero: firmo ad-hoc." >&2
  identity=""
fi
identity="${identity:--}"
echo "→ firmando con «${identity}»"
for bin in "$contents/MacOS/atic-mcp" "$contents/MacOS/atic-apple"; do
  [[ -f "$bin" ]] && codesign --force --options runtime --sign "$identity" "$bin"
done
codesign --force --options runtime --entitlements "$pill/macos/entitlements.plist" \
  --sign "$identity" "$app"

if [[ "$install" == 1 ]]; then
  echo "→ copiando a /Applications"
  rm -rf "/Applications/$name.app"
  cp -R "$app" "/Applications/$name.app"
  app="/Applications/$name.app"
fi

if [[ "$open_app" == 1 ]]; then
  # Cierra la pill que esté abierta (la de dev, la instalada en Atic.app o la
  # suelta) y abre la nueva. Si queda una viva, la nueva ve el candado de
  # instancia única y se cierra sola.
  pattern='\.app/Contents/MacOS/atic-pill$|atic-gpui-target/.*/pill-gpui$'
  pkill -f "$pattern" 2>/dev/null || true
  for _ in {1..20}; do
    pgrep -f "$pattern" >/dev/null || break
    sleep 0.25
  done
  pkill -9 -f "$pattern" 2>/dev/null || true
  # `open` no hereda el entorno de la terminal: las variables de la pill
  # (PILL_FAKE_NOTCH, PILL_DEV, PILL_DEBUG, PILL_FPS…) se pasan a mano.
  open_args=()
  while IFS='=' read -r var _; do
    open_args+=(--env "$var=${!var}")
  done < <(env | grep -E '^PILL_[A-Z_]+=' || true)
  open ${open_args[@]+"${open_args[@]}"} "$app"
  echo "→ abierta: $app"
fi
