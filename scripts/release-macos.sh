#!/usr/bin/env bash
# Compila el .dmg universal (Intel + Apple Silicon) y firma los artefactos
# del updater en esta Mac. Requiere Xcode Command Line Tools, Rust (rustup),
# Node/pnpm y, si vas a publicar, la CLI de GitHub (gh).
#
# No usa el certificado de pago de Apple Developer. Si existe
# ~/.tauri/atic-signing-identity, firma con ese certificado propio (creado una
# vez en el Llavero; ver docs/MACOS.md) y así macOS conserva los permisos TCC
# entre builds y actualizaciones. Si no existe, cae a firma ad-hoc
# (APPLE_SIGNING_IDENTITY=-), que es lo mínimo para Apple Silicon.
#
# Con cualquier firma no notarizada, la primera vez que se abra en otra Mac
# Gatekeeper la bloquea: clic derecho > Abrir, o `xattr -cr /Applications/Atic.app`.
#
# APPLE_SIGNING_IDENTITY exportado tiene prioridad sobre el archivo.
#
# Nota: con CI=true (o sin permiso de Automatización) el DMG se arma sin el
# AppleScript de Finder: mismos archivos, solo sin posición de iconos. En
# terminales interactivas el script lo deja tal cual para conservar el layout.
#
# La clave de firma del updater es la MISMA que usa release-windows.ps1.
# Copiala a esta Mac en:
#   ~/.tauri/atic-updater.key
#   ~/.tauri/atic-updater.password
#
# Uso (desde la raíz del repo):
#   bash scripts/release-macos.sh
#   bash scripts/release-macos.sh --publish
#
# --publish: si el release del tag vX.Y.Z no existe todavía lo crea (como
# hace release-windows.ps1); si ya existe (por ejemplo porque Windows publicó
# primero) sube los artefactos y fusiona latest.json con las plataformas que
# ya estén publicadas, en vez de pisarlas.

set -euo pipefail

PUBLISH=0
if [[ "${1:-}" == "--publish" ]]; then
  PUBLISH=1
fi

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo"

key_path="$HOME/.tauri/atic-updater.key"
pass_path="$HOME/.tauri/atic-updater.password"

if [[ ! -f "$key_path" ]]; then
  echo "Falta $key_path. Es la clave privada del updater; no está en git." >&2
  exit 1
fi
if [[ ! -f "$pass_path" ]]; then
  echo "Falta $pass_path. Guarda ahí la contraseña de la clave, una sola línea." >&2
  exit 1
fi

ver="$(node -pe "require('./apps/desktop/src-tauri/tauri.conf.json').version")"
if [[ -z "$ver" ]]; then
  echo "No pude leer version de tauri.conf.json" >&2
  exit 1
fi
tag="v$ver"

export TAURI_SIGNING_PRIVATE_KEY
TAURI_SIGNING_PRIVATE_KEY="$(cat "$key_path")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$(tr -d '\n' < "$pass_path")"

# Identidad de firma: manda APPLE_SIGNING_IDENTITY; si no está definida, se
# lee ~/.tauri/atic-signing-identity (certificado propio, ver docs/MACOS.md).
identity_file="$HOME/.tauri/atic-signing-identity"
if [[ -z "${APPLE_SIGNING_IDENTITY:-}" && -f "$identity_file" ]]; then
  APPLE_SIGNING_IDENTITY="$(tr -d '\n' < "$identity_file")"
fi
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && "$APPLE_SIGNING_IDENTITY" != "-" ]] &&
  ! security find-identity -v -p codesigning | grep -qF "$APPLE_SIGNING_IDENTITY"; then
  echo "La identidad '$APPLE_SIGNING_IDENTITY' no está en el Llavero." >&2
  echo "Revisá docs/MACOS.md (Firma local) o exportá APPLE_SIGNING_IDENTITY=- para ad-hoc." >&2
  exit 1
fi
# Firma ad-hoc si no hay identidad: sin esto Tauri no firma el bundle y en
# Apple Silicon Gatekeeper lo trata como app dañada.
export APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:--}"

rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Los sidecars llevan sufijo por triple: el build universal exige los dos.
# (`beforeBuildCommand` rehace el del host durante el build; acá se agregan.)
for triple in aarch64-apple-darwin x86_64-apple-darwin; do
  sh "$repo/scripts/build-mcp.sh" "$triple"
  sh "$repo/scripts/build-unix.sh" "$triple"
done

pushd apps/desktop > /dev/null
pnpm tauri build --bundles dmg,app --target universal-apple-darwin
popd > /dev/null

bundle_dir="$repo/target/universal-apple-darwin/release/bundle"
dmg_dir="$bundle_dir/dmg"
macos_dir="$bundle_dir/macos"

dmg="$(find "$dmg_dir" -maxdepth 1 -name "Atic_${ver}_*.dmg" | head -n 1)"
if [[ -z "$dmg" ]]; then
  echo "No encontré Atic_${ver}_*.dmg en $dmg_dir" >&2
  exit 1
fi

app_tar="$(find "$macos_dir" -maxdepth 1 -name "*.app.tar.gz" | head -n 1)"
if [[ -z "$app_tar" ]]; then
  echo "No encontré *.app.tar.gz en $macos_dir" >&2
  exit 1
fi
app_sig="${app_tar}.sig"
if [[ ! -f "$app_sig" ]]; then
  echo "No se generó $(basename "$app_tar").sig. Revisa la clave y la contraseña." >&2
  exit 1
fi

sig="$(tr -d '\n' < "$app_sig")"
url="https://github.com/chrx3/atic/releases/download/$tag/$(basename "$app_tar")"
pub_date="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
manifest_json="$dmg_dir/latest.json"

# Si el release ya existe (p. ej. Windows lo publicó primero), partimos de su
# latest.json para no perder las plataformas que ya tenga firmadas.
existing_path=""
if gh release view "$tag" > /dev/null 2>&1; then
  tmp_existing="$(mktemp)"
  if gh release download "$tag" -p latest.json -O "$tmp_existing" > /dev/null 2>&1; then
    existing_path="$tmp_existing"
  fi
fi

node -e '
const fs = require("fs");
const [, , existingPath, outPath, ver, sig, url, pubDate] = process.argv;
let manifest = { version: ver, notes: `Atic v${ver}`, pub_date: pubDate, platforms: {} };
if (existingPath && fs.existsSync(existingPath)) {
  try {
    const prev = JSON.parse(fs.readFileSync(existingPath, "utf8"));
    if (prev.platforms) manifest.platforms = prev.platforms;
  } catch {}
}
// Build universal: el mismo binario sirve para Intel y Apple Silicon.
manifest.platforms["darwin-x86_64"] = { signature: sig, url };
manifest.platforms["darwin-aarch64"] = { signature: sig, url };
fs.writeFileSync(outPath, JSON.stringify(manifest, null, 2));
' "$existing_path" "$manifest_json" "$ver" "$sig" "$url" "$pub_date"

echo "Listo: $dmg"
echo "      $app_tar"
echo "      $app_sig"
echo "      $manifest_json"

if [[ "$PUBLISH" -ne 1 ]]; then
  echo "Para publicar: bash scripts/release-macos.sh --publish"
  exit 0
fi

if ! gh release view "$tag" > /dev/null 2>&1; then
  git tag "$tag"
  git push origin HEAD
  git push origin "$tag"
  gh release create "$tag" --title "$tag" --latest --generate-notes -- "$dmg" "$app_tar" "$app_sig" "$manifest_json"
else
  gh release upload "$tag" "$dmg" "$app_tar" "$app_sig" "$manifest_json" --clobber
fi

echo "Release ${tag}: https://github.com/chrx3/atic/releases/tag/${tag}"
