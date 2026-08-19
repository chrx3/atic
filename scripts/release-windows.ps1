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

[CmdletBinding()]
param(
    [switch]$Publish
)

$ErrorActionPreference = "Stop"

$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo

$keyPath = Join-Path $env:USERPROFILE ".tauri\atic-updater.key"
$passPath = Join-Path $env:USERPROFILE ".tauri\atic-updater.password"

if (-not (Test-Path $keyPath)) {
    throw "Falta $keyPath. Es la clave privada del updater; no está en git."
}
if (-not (Test-Path $passPath)) {
    throw "Falta $passPath. Guardá ahí la contraseña de la clave, una sola línea."
}

$conf = Get-Content -Raw (Join-Path $repo "apps\desktop\src-tauri\tauri.conf.json") | ConvertFrom-Json
$ver = [string]$conf.version
if (-not $ver) { throw "No pude leer version de tauri.conf.json" }
$tag = "v$ver"

$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw $keyPath
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content -Raw $passPath).Trim()

Push-Location (Join-Path $repo "apps\desktop")
try {
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
    throw "No se generó $($exe.Name).sig. Revisá la clave y la contraseña."
}

$sig = (Get-Content -Raw $sigPath).Trim()
$url = "https://github.com/chrx3/atic/releases/download/$tag/$($exe.Name)"
$manifest = [ordered]@{
    version   = $ver
    notes     = "Atic $tag"
    pub_date  = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
    platforms = [ordered]@{
        "windows-x86_64" = [ordered]@{
            signature = $sig
            url       = $url
        }
    }
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

git tag $tag
git push origin HEAD
git push origin $tag
gh release create $tag --title $tag --latest --generate-notes -- $exe.FullName $sigPath $latest
Write-Host "Release ${tag}: https://github.com/chrx3/atic/releases/tag/${tag}"
