#!/usr/bin/env bash
# Compila la Atic de macOS (la pill GPUI, universal: Intel + Apple Silicon),
# el .dmg y los artefactos firmados del updater en esta Mac. Requiere Xcode
# Command Line Tools, Rust (rustup), Node/pnpm y, si vas a publicar, la CLI de
# GitHub (gh).
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

ver="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/apps/atic/Cargo.toml" | head -1)"
if [[ -z "$ver" ]]; then
  echo "No pude leer la versión de apps/atic/Cargo.toml" >&2
  exit 1
fi
tag="v$ver"

# Cada versión publicada lleva sus Novedades (lo que la app muestra al
# actualizar): sin la entrada en el changelog no se publica.
if ! node -e 'const v=process.argv[1];const d=require("./apps/atic/assets/changelog.json");process.exit(d.versions.some(r=>r.version===v)?0:1)' "$ver"; then
  echo "Falta la entrada de $ver en apps/atic/assets/changelog.json (ver AGENTS.md → Novedades)." >&2
  exit 1
fi

# Los tests de los crates compartidos (los mismos del CI) antes de compilar.
# Con `set -e`, si fallan el script termina aquí.
cargo test --locked -p atic-core -p atic-calc -p atic-agents -p atic-mcp -p atic-sign

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

# En macOS la app es la pill GPUI, como en Windows: sin Tauri
# (scripts/build-pill-mac.sh). La firma minisign es la misma de siempre; se
# hace con `atic-sign` (`crates/sign`).
bash "$repo/scripts/build-pill-mac.sh"
# El `.app.tar.gz` del updater y el `.dmg`.
bash "$repo/scripts/package-mac.sh"

out="$repo/target/pill-mac"
dmg_dir="$out"
app_tar="$out/Atic.app.tar.gz"
dmg="$out/Atic_${ver}_universal.dmg"

# La firma minisign que comprueba el updater (`crates/sign`).
cargo run -q -p atic-sign --release -- "$app_tar" --version "$ver" --target darwin --key "$key_path" --password-file "$pass_path"
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
manifest_tmp="$(mktemp -d)"
trap 'rm -rf "$manifest_tmp"' EXIT
# Si GitHub falla, se detiene: no confundir un error de red o permisos con
# un release que todavía no existe y generar un manifiesto sin Windows.
gh api --paginate 'repos/{owner}/{repo}/releases?per_page=100' --jq '.[].tag_name' > "$manifest_tmp/tags"
if grep -Fxq -- "$tag" "$manifest_tmp/tags"; then
  existing_path="$manifest_tmp/previous.json"
  gh release download "$tag" -p latest.json -O "$existing_path"
fi

node "$repo/scripts/mac-updater-manifest.mjs" \
  "$existing_path" "$manifest_json" "$ver" "$sig" "$url" "$pub_date"

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
