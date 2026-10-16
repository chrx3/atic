#!/usr/bin/env bash
# Arma la Atic de macOS para publicar: la pill GPUI como `Atic.app`
# (com.ciat.atic, el mismo identificador de la app de Tauri), universal
# (Apple Silicon + Intel), con el servidor MCP y el agente de Atic Code, y
# firmada. Es el `build-installer.ps1` de Windows para Mac.
#
# Con el mismo identificador y la misma firma («Atic Dev» de
# ~/.tauri/atic-signing-identity, ver docs/MACOS.md) macOS conserva los
# permisos que ya tenía la Atic de Tauri: el updater de Tauri cambia el
# contenido de /Applications/Atic.app por esta y la vuelve a abrir.
#
# Uso (desde cualquier carpeta):
#   bash scripts/build-pill-mac.sh                 # universal
#   ATIC_MAC_ARCHS=aarch64 bash scripts/build-pill-mac.sh   # solo Apple Silicon (pruebas)
#
# Deja target/pill-mac/Atic.app. Compila en ~/Library/Caches/atic-gpui-target
# (o $CARGO_TARGET_DIR), el mismo de `pill-dev-mac.sh`. Una arquitectura a la
# vez y con -j 4 (PILL_JOBS): con LTO completo, en 8 GB no da para más.

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
pill="$repo/prototypes/pill-gpui"
pill_target="${CARGO_TARGET_DIR:-$HOME/Library/Caches/atic-gpui-target}"
jobs="${PILL_JOBS:-4}"

if [[ ! -d "$repo/../gpui-m3" ]]; then
  echo "Falta ../gpui-m3 (los componentes de Atic Code): git clone https://github.com/chrx3/gpui-m3.git $(cd "$repo/.." && pwd)/gpui-m3" >&2
  exit 1
fi

version="$(node -pe "require('$repo/apps/desktop/src-tauri/tauri.conf.json').version")"
read -r -a archs <<< "${ATIC_MAC_ARCHS:-aarch64 x86_64}"

pills=()
mcps=()
for arch in "${archs[@]}"; do
  triple="$arch-apple-darwin"
  rustup target add "$triple" > /dev/null
  echo "→ compilando la pill para $triple (release, varios minutos)…"
  CARGO_TARGET_DIR="$pill_target" cargo build --manifest-path "$pill/Cargo.toml" --release --target "$triple" -j "$jobs"
  pills+=("$pill_target/$triple/release/pill-gpui")
  echo "→ compilando atic-mcp para ${triple}…"
  CARGO_TARGET_DIR="$repo/target" cargo build --manifest-path "$repo/Cargo.toml" -p atic-mcp --release --target "$triple" -j "$jobs"
  mcps+=("$repo/target/$triple/release/atic-mcp")
done

echo "→ empaquetando el agente de Atic Code…"
# Instala solo si falta: pnpm 10 corta `install` por el script de esbuild
# (ERR_PNPM_IGNORED_BUILDS) aunque ya esté todo.
if [[ ! -d "$pill/sidecar/node_modules" ]]; then
  (cd "$pill/sidecar" && pnpm install --frozen-lockfile > /dev/null)
fi
(cd "$pill/sidecar" && pnpm run build > /dev/null)

out="$repo/target/pill-mac"
app="$out/Atic.app"
contents="$app/Contents"
echo "→ armando $app ($version)"
rm -rf "$app"
mkdir -p "$contents/MacOS" "$contents/Resources/sidecar"
lipo -create -output "$contents/MacOS/atic-pill" "${pills[@]}"
lipo -create -output "$contents/MacOS/atic-mcp" "${mcps[@]}"
# Un script no es código firmable: va en Resources, no junto al ejecutable.
cp "$pill/sidecar/dist/agent.mjs" "$contents/Resources/sidecar/agent.mjs"
cp "$repo/apps/desktop/src-tauri/icons/icon.icns" "$contents/Resources/AppIcon.icns"
sed -e "s/@NAME@/Atic/g" -e "s/@ID@/com.ciat.atic/g" -e "s/@VERSION@/$version/g" \
  "$pill/macos/Info.plist" > "$contents/Info.plist"
printf 'APPL????' > "$contents/PkgInfo"

identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -z "$identity" && -f "$HOME/.tauri/atic-signing-identity" ]]; then
  identity="$(tr -d '\n' < "$HOME/.tauri/atic-signing-identity")"
fi
if [[ -z "$identity" || "$identity" == "-" ]]; then
  echo "Sin identidad de firma: firmo ad-hoc. Los que vienen de la Atic de Tauri tendrán que volver a dar los permisos." >&2
  identity="-"
elif ! security find-identity -v -p codesigning | grep -qF "$identity"; then
  echo "La identidad '$identity' no está en el Llavero (ver docs/MACOS.md)." >&2
  exit 1
fi
echo "→ firmando con «${identity}»"
codesign --force --options runtime --timestamp=none --sign "$identity" "$contents/MacOS/atic-mcp"
codesign --force --options runtime --timestamp=none --entitlements "$pill/macos/entitlements.plist" \
  --sign "$identity" "$app"
codesign --verify --deep --strict "$app"
lipo -info "$contents/MacOS/atic-pill"
echo "Lista: $app"
