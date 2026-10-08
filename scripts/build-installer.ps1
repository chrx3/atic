# Arma el instalador de Windows sin Tauri: la pill GPUI y sus sidecars en un
# NSIS propio (installer/atic.nsi). Deja
# target\installer\Atic_<versión>_x64-setup.exe.
#
# La versión sale de apps/desktop/src-tauri/tauri.conf.json, la misma que
# usan el script de release y la pill.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz

$conf = Get-Content -Raw (Join-Path $raiz "apps\desktop\src-tauri\tauri.conf.json") | ConvertFrom-Json
$ver = [string]$conf.version
if (-not $ver) { throw "No pude leer version de tauri.conf.json" }

cargo build -p atic-mcp -p atic-unix --release
if ($LASTEXITCODE -ne 0) { throw "cargo build de los sidecars falló ($LASTEXITCODE)" }
& (Join-Path $PSScriptRoot "build-pill.ps1")
if ($LASTEXITCODE -ne 0) { throw "build-pill falló ($LASTEXITCODE)" }

$out = Join-Path $raiz "target\installer"
$stage = Join-Path $out "stage"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -Force (Join-Path $raiz "target\pill-gpui\release\pill-gpui.exe") (Join-Path $stage "atic-pill.exe")
Copy-Item -Force (Join-Path $raiz "target\release\atic-mcp.exe") (Join-Path $stage "atic-mcp.exe")
Copy-Item -Force (Join-Path $raiz "target\release\atic-unix.exe") (Join-Path $stage "atic-unix.exe")
Copy-Item -Force (Join-Path $raiz "apps\desktop\src-tauri\icons\icon.ico") (Join-Path $stage "icon.ico")

# El makensis que baja el CLI de Tauri, o el del PATH.
$makensis = Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"
if (-not (Test-Path $makensis)) {
    $makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
}
if (-not $makensis) { throw "No encontré makensis (NSIS)." }

$exe = Join-Path $out "Atic_${ver}_x64-setup.exe"
& $makensis /V2 "/DVERSION=$ver" "/DSRC=$stage" "/DOUT=$exe" (Join-Path $raiz "installer\atic.nsi")
if ($LASTEXITCODE -ne 0) { throw "makensis falló ($LASTEXITCODE)" }
Write-Host "Instalador listo: $exe"
