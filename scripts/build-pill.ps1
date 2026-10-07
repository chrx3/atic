# Compila la pill GPUI en release y la deja donde Tauri la empaqueta como
# sidecar (`binaries/atic-pill-<triple>.exe`). Solo la usa el instalador con
# la pill nativa (`tauri.pill.conf.json`); `pnpm dev` no la necesita.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$pill = Join-Path $raiz "prototypes\pill-gpui"
$destinoDir = Join-Path $raiz "apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $destinoDir | Out-Null
# Su propio target: la pill está fuera del workspace y no comparte Cargo.lock.
$target = Join-Path $raiz "target\pill-gpui"
cargo build --release --manifest-path (Join-Path $pill "Cargo.toml") --target-dir $target
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$triple = (rustc --print host-tuple).Trim()
$origen = Join-Path $target "release\pill-gpui.exe"
$destino = Join-Path $destinoDir "atic-pill-$triple.exe"
Copy-Item -Force $origen $destino
Write-Host "atic-pill listo en $destino"
