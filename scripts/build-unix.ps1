# Compila el sidecar de comandos Unix y lo deja donde Tauri lo empaqueta.
# Igual que `build-mcp.ps1`: `tauri-build` exige que el binario exista al
# compilar, no solo al empaquetar, así que la primera vez hay que correrlo a
# mano (`pnpm unix:build`).
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$destinoDir = Join-Path $raiz "apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $destinoDir | Out-Null
cargo build -p atic-unix --release
$triple = (rustc --print host-tuple).Trim()
$origen = Join-Path $raiz "target\release\atic-unix.exe"
$destino = Join-Path $destinoDir "atic-unix-$triple.exe"
Copy-Item -Force $origen $destino
Write-Host "atic-unix listo en $destino"
