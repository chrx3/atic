# Firma el instalador NSIS en esta máquina y arma latest.json.
#
# La clave NO va al repo. Vive en:
#   %USERPROFILE%\.tauri\atic-updater.key
#   %USERPROFILE%\.tauri\atic-updater.password
#
# Uso (desde la raíz del repo):
#   powershell -File scripts/release-windows.ps1
#   powershell -File scripts/release-windows.ps1 -Publish
#
# -Publish: tag vX.Y.Z, push y gh release --latest con exe + sig + latest.json.
# Si el release ya existe (p. ej. macOS publicó primero) sube los artefactos y
# fusiona latest.json con las plataformas ya publicadas, en vez de pisarlas.

[CmdletBinding()]
param(
    [switch]$Publish
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

# clang/bindgen para whisper-rs-sys (LIBCLANG_PATH + headers de MSVC/SDK).
. (Join-Path $PSScriptRoot "win-env.ps1")

$keyPath = Join-Path $env:USERPROFILE ".tauri\atic-updater.key"
$passPath = Join-Path $env:USERPROFILE ".tauri\atic-updater.password"

if (-not (Test-Path $keyPath)) {
    throw "Falta $keyPath. Es la clave privada del updater; no está en git."
}
if (-not (Test-Path $passPath)) {
    throw "Falta $passPath. Guarda ahí la contraseña de la clave, una sola línea."
}

$conf = Get-Content -Raw (Join-Path $repo "apps\desktop\src-tauri\tauri.conf.json") | ConvertFrom-Json
$ver = [string]$conf.version
if (-not $ver) { throw "No pude leer version de tauri.conf.json" }
$tag = "v$ver"

$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw $keyPath
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw $passPath).Trim()

Push-Location (Join-Path $repo "apps\desktop")
try {
    # El sidecar MCP lo exige tauri-build al compilar (además va en beforeBuildCommand).
    pnpm mcp:build
    if ($LASTEXITCODE -ne 0) { throw "pnpm mcp:build falló ($LASTEXITCODE)" }
    pnpm tauri build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw "pnpm tauri build falló ($LASTEXITCODE)" }
}
finally {
    Pop-Location
}

$nsis = Join-Path $repo "target\release\bundle\nsis"
$exe = Get-ChildItem -Path $nsis -Filter "Atic_${ver}_x64-setup.exe" | Select-Object -First 1
if (-not $exe) { throw "No encontré Atic_${ver}_x64-setup.exe en $nsis" }
$sigPath = "$($exe.FullName).sig"
if (-not (Test-Path $sigPath)) {
    throw "No se generó $($exe.Name).sig. Revisa la clave y la contraseña."
}

$sig = (Get-Content -Raw $sigPath).Trim()
$url = "https://github.com/chrx3/atic/releases/download/$tag/$($exe.Name)"

# Si el release ya existe (p. ej. macOS publicó primero), partimos de su
# latest.json para no perder las plataformas que ya tenga firmadas.
$releaseExists = $false
try {
    gh release view $tag 2>$null | Out-Null
    $releaseExists = ($LASTEXITCODE -eq 0)
} catch {
    $releaseExists = $false
}

$platforms = [ordered]@{}
if ($releaseExists) {
    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "atic-latest-$ver"
    New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
    try {
        gh release download $tag -p latest.json -D $tmpDir --clobber 2>$null | Out-Null
        $existingPath = Join-Path $tmpDir "latest.json"
        if ((Test-Path $existingPath) -and ($LASTEXITCODE -eq 0)) {
            $prev = Get-Content -Raw $existingPath | ConvertFrom-Json
            if ($prev.platforms) {
                foreach ($p in $prev.platforms.PSObject.Properties) {
                    $platforms[$p.Name] = [ordered]@{
                        signature = $p.Value.signature
                        url       = $p.Value.url
                    }
                }
            }
        }
    } catch {
        Write-Warning "No pude leer el latest.json existente; el manifest sale solo con Windows."
    }
}
$platforms["windows-x86_64"] = [ordered]@{
    signature = $sig
    url       = $url
}

$manifest = [ordered]@{
    version   = $ver
    notes     = "Atic $tag"
    pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = $platforms
}
$latest = Join-Path $nsis "latest.json"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 6), $utf8)

Write-Host "Listo: $($exe.FullName)"
Write-Host "      $sigPath"
Write-Host "      $latest"

if (-not $Publish) {
    Write-Host "Para publicar: powershell -File scripts/release-windows.ps1 -Publish"
    return
}

if (-not (git tag --list $tag)) {
    git tag $tag
    git push origin HEAD
    git push origin $tag
}

if ($releaseExists) {
    gh release upload $tag $exe.FullName $sigPath $latest --clobber
} else {
    gh release create $tag --title $tag --latest --generate-notes -- $exe.FullName $sigPath $latest
}
Write-Host "Release ${tag}: https://github.com/chrx3/atic/releases/tag/${tag}"
