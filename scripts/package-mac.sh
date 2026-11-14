#!/usr/bin/env bash
# Empaqueta la Atic.app que deja build-pill-mac.sh para publicar:
# - target/pill-mac/Atic.app.tar.gz: lo que baja el updater de Tauri de las
#   instalaciones viejas (cambia el contenido de /Applications/Atic.app por
#   este) y también el de la pill (`updater.rs`).
# - target/pill-mac/Atic_<versión>_universal.dmg: para instalar desde cero.
#
# No firma nada: la firma minisign del updater va aparte (atic-sign). Lo usan
# scripts/release-macos.sh y el release de GitHub Actions (release.yml).

set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ver="$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/apps/atic/Cargo.toml" | head -1)"
out="$repo/target/pill-mac"
app="$out/Atic.app"
if [[ ! -d "$app" ]]; then
  echo "Falta $app: corre antes scripts/build-pill-mac.sh" >&2
  exit 1
fi

app_tar="$out/Atic.app.tar.gz"
rm -f "$app_tar" "$app_tar.sig"
# Sin los `._*` de los atributos extendidos dentro del archivo.
COPYFILE_DISABLE=1 tar -C "$out" -czf "$app_tar" Atic.app

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

echo "Empaquetada: $app_tar"
echo "             $dmg"
