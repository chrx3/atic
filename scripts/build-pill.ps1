# Compila la app (la pill GPUI) en release para el instalador de Windows.
# Deja target\pill-gpui\release\pill-gpui.exe, que `build-installer.ps1`
# copia como atic-pill.exe.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$app = Join-Path $raiz "apps\atic"
# Su propio target: la app está fuera del workspace y no comparte Cargo.lock.
$target = Join-Path $raiz "target\pill-gpui"
cargo build --release --manifest-path (Join-Path $app "Cargo.toml") --target-dir $target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "atic-pill listo en $(Join-Path $target 'release\pill-gpui.exe')"
