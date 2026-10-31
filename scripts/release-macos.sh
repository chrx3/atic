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
# hace con el firmador del CLI de Tauri, que solo se usa como herramienta.
# El `.app.tar.gz` es lo que baja el updater de Tauri de las instalaciones
# viejas (cambia el contenido de /Applications/Atic.app por este) y también
# el de la pill (`updater.rs`).
bash "$repo/scripts/build-pill-mac.sh"

out="$repo/target/pill-mac"
app="$out/Atic.app"
dmg_dir="$out"
app_tar="$out/Atic.app.tar.gz"
rm -f "$app_tar" "$app_tar.sig"
# Sin los `._*` de los atributos extendidos dentro del archivo.
COPYFILE_DISABLE=1 tar -C "$out" -czf "$app_tar" Atic.app

# La firma minisign que comprueba el updater (`crates/sign`).
cargo run -q -p atic-sign --release -- "$app_tar" --key "$key_path" --password-file "$pass_path"
app_sig="${app_tar}.sig"
if [[ ! -f "$app_sig" ]]; then
  echo "No se generó $(basename "$app_tar").sig. Revisa la clave y la contraseña." >&2
  exit 1
fi

# El .dmg para instalar desde cero: la app y un atajo a Aplicaciones.
dmg="$out/Atic_${ver}_universal.dmg"
stage="$out/dmg"
rm -rf "$stage" "$dmg"
mkdir -p "$stage"
cp -R "$app" "$stage/Atic.app"
ln -s /Applications "$stage/Applications"
# Tamaño explícito: sin -size, hdiutil de macOS 27 calcula mal el volumen y
# falla con "No queda espacio en el dispositivo" aunque sobre disco.
stage_mb="$(du -sm "$stage" | cut -f1)"
hdiutil create -volname "Atic" -srcfolder "$stage" -size "$((stage_mb * 3 / 2 + 50))m" -ov -format UDZO "$dmg" > /dev/null
rm -rf "$stage"

sig="$(tr -d '\n' < "$app_sig")"
url="https://github.com/chrx3/atic/releases/download/$tag/$(basename "$app_tar")"
pub_date="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
manifest_json="$dmg_dir/latest.json"

# Si el release ya existe (p. ej. Windows lo publicó primero), partimos de su
# latest.json para no perder las plataformas que ya tenga firmadas.
existing_path=""
if gh release view "$tag" > /dev/null 2>&1; then
  tmp_existing="$(mktemp)"
  if gh release download "$tag" -p latest.json -O "$tmp_existing" --clobber > /dev/null 2>&1; then
    existing_path="$tmp_existing"
  fi
fi

node -e '
const fs = require("fs");
const [existingPath, outPath, ver, sig, url, pubDate] = process.argv.slice(1);
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
