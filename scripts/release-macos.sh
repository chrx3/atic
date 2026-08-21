#!/usr/bin/env bash
# Compila el .dmg universal (Intel + Apple Silicon) y firma los artefactos
# del updater en esta Mac. Requiere Xcode Command Line Tools, Rust (rustup),
# Node/pnpm y, si vas a publicar, la CLI de GitHub (gh).
#
# No firma con el certificado de Apple Developer: el .app queda con la firma
# ad-hoc que aplica Tauri por defecto. La primera vez que se abra en una Mac
# (propia o de otra persona), Gatekeeper va a bloquearlo por no estar
# notarizado — hay que hacer clic derecho > Abrir, o correr
# `xattr -cr /Applications/Atic.app`.
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

rustup target add aarch64-apple-darwin x86_64-apple-darwin

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
