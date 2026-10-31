# Arma el instalador de Windows sin Tauri: la pill GPUI y sus sidecars en un
# NSIS propio (installer/atic.nsi). Deja
# target\installer\Atic_<versión>_x64-setup.exe.
#
# La versión sale de apps/atic/Cargo.toml, la misma que usan el script de
# release y la app.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz

$ver = (Select-String -Path (Join-Path $raiz "apps\atic\Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $ver) { throw "No pude leer la versión de apps/atic/Cargo.toml" }

cargo build -p atic-mcp -p atic-unix --release
if ($LASTEXITCODE -ne 0) { throw "cargo build de los sidecars falló ($LASTEXITCODE)" }
& (Join-Path $PSScriptRoot "build-pill.ps1")
if ($LASTEXITCODE -ne 0) { throw "build-pill falló ($LASTEXITCODE)" }
# El agente de Atic Code: sin él, la pill instalada solo lo encuentra en el
# PC donde se compiló (busca el repo en la ruta de compilación).
$sidecar = Join-Path $raiz "apps\atic\sidecar"
pnpm --dir $sidecar install --frozen-lockfile
if ($LASTEXITCODE -ne 0) { throw "pnpm install del sidecar falló ($LASTEXITCODE)" }
pnpm --dir $sidecar run build
if ($LASTEXITCODE -ne 0) { throw "build del sidecar falló ($LASTEXITCODE)" }

$out = Join-Path $raiz "target\installer"
$stage = Join-Path $out "stage"
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item -Force (Join-Path $raiz "target\pill-gpui\release\pill-gpui.exe") (Join-Path $stage "atic-pill.exe")
Copy-Item -Force (Join-Path $raiz "target\release\atic-mcp.exe") (Join-Path $stage "atic-mcp.exe")
Copy-Item -Force (Join-Path $raiz "target\release\atic-unix.exe") (Join-Path $stage "atic-unix.exe")
Copy-Item -Force (Join-Path $raiz "apps\atic\assets\app\icon.ico") (Join-Path $stage "icon.ico")
New-Item -ItemType Directory -Force -Path (Join-Path $stage "sidecar") | Out-Null
Copy-Item -Force (Join-Path $sidecar "dist\agent.mjs") (Join-Path $stage "sidecar\agent.mjs")

# makensis: el del PATH, el de NSIS instalado (winget install NSIS.NSIS) o
# el que quedó de cuando se usaba el CLI de Tauri.
$makensis = (Get-Command makensis -ErrorAction SilentlyContinue).Source
foreach ($candidato in @(
        (Join-Path ${env:ProgramFiles(x86)} "NSIS\makensis.exe"),
        (Join-Path $env:ProgramFiles "NSIS\makensis.exe"),
        (Join-Path $env:LOCALAPPDATA "tauri\NSIS\makensis.exe"))) {
    if (-not $makensis -and (Test-Path $candidato)) { $makensis = $candidato }
}
if (-not $makensis) { throw "No encontré makensis (NSIS)." }

$exe = Join-Path $out "Atic_${ver}_x64-setup.exe"
& $makensis /V2 "/DVERSION=$ver" "/DSRC=$stage" "/DOUT=$exe" (Join-Path $raiz "installer\atic.nsi")
if ($LASTEXITCODE -ne 0) { throw "makensis falló ($LASTEXITCODE)" }
Write-Host "Instalador listo: $exe"
