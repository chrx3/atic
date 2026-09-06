# Compila el sidecar MCP y lo deja donde Tauri lo empaqueta.
# La primera vez hay que correrlo a mano (`pnpm mcp:build`): `tauri-build`
# exige que el binario exista al compilar, no solo al empaquetar.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$destinoDir = Join-Path $raiz "apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force -Path $destinoDir | Out-Null
cargo build -p atic-mcp --release
$triple = (rustc --print host-tuple).Trim()
$origen = Join-Path $raiz "target\release\atic-mcp.exe"
$destino = Join-Path $destinoDir "atic-mcp-$triple.exe"
Copy-Item -Force $origen $destino
Write-Host "atic-mcp listo en $destino"
