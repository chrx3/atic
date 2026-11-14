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
#
# Es la alternativa manual al release de GitHub Actions (release.yml, ver
# docs/CI.md): usa uno u otro para una versión. Ojo: el push del tag también
# dispara release.yml, que falla en su preflight si este script ya publicó el
# release (no lo toca) o, si todavía no, deja un borrador aparte.

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

$ver = (Select-String -Path (Join-Path $repo "apps\atic\Cargo.toml") -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $ver) { throw "No pude leer la versión de apps/atic/Cargo.toml" }
$tag = "v$ver"

# Cada versión publicada lleva sus Novedades (lo que la app muestra al
# actualizar): sin la entrada en el changelog no se publica.
$changelog = Get-Content -Raw -Encoding UTF8 (Join-Path $repo "apps\atic\assets\changelog.json") | ConvertFrom-Json
if (-not ($changelog.versions | Where-Object { $_.version -eq $ver })) {
    throw "Falta la entrada de $ver en apps/atic/assets/changelog.json (ver AGENTS.md, Novedades)."
}

# Los tests de los crates compartidos (los mismos del CI) antes de compilar.
cargo test --locked -p atic-core -p atic-calc -p atic-agents -p atic-mcp -p atic-sign
if ($LASTEXITCODE -ne 0) { throw "los tests fallaron ($LASTEXITCODE)" }

# En Windows la app es la pill GPUI: instalador NSIS propio, sin Tauri
# (scripts/build-installer.ps1). La firma minisign es la misma de siempre; se
# hace con `atic-sign` (`crates/sign`).
& (Join-Path $PSScriptRoot "build-installer.ps1")
if ($LASTEXITCODE -ne 0) { throw "build-installer falló ($LASTEXITCODE)" }

$nsis = Join-Path $repo "target\installer"
$exe = Get-ChildItem -Path $nsis -Filter "Atic_${ver}_x64-setup.exe" | Select-Object -First 1
if (-not $exe) { throw "No encontré Atic_${ver}_x64-setup.exe en $nsis" }
# La firma minisign que comprueba el updater (`crates/sign`).
cargo run -q -p atic-sign --release -- $exe.FullName --version $ver --target windows-x86_64 --key $keyPath --password-file $passPath
if ($LASTEXITCODE -ne 0) { throw "la firma falló ($LASTEXITCODE)" }
$sigPath = "$($exe.FullName).sig"
if (-not (Test-Path $sigPath)) {
    throw "No se generó $($exe.Name).sig. Revisa la clave y la contraseña."
}

$url = "https://github.com/chrx3/atic/releases/download/$tag/$($exe.Name)"

# Si el release ya existe (p. ej. macOS publicó primero), partimos de su
# latest.json para no perder las plataformas que ya tenga firmadas
# (scripts/updater-manifest.mjs, el mismo del release de GitHub Actions). Si
# GitHub falla al listar o al bajarlo, se detiene: un error de red no es un
# release que no existe.
$tags = gh api --paginate 'repos/{owner}/{repo}/releases?per_page=100' --jq '.[].tag_name'
if ($LASTEXITCODE -ne 0) { throw "No pude listar los releases de GitHub ($LASTEXITCODE)." }
$releaseExists = @($tags) -contains $tag

# «-»: sin previo (PowerShell 5.1 no le pasa un "" a node).
$previous = "-"
if ($releaseExists) {
    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "atic-latest-$ver"
    New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
    $previous = Join-Path $tmpDir "latest.json"
    Remove-Item -Force -ErrorAction SilentlyContinue $previous
    gh release download $tag -p latest.json -O $previous
    if ($LASTEXITCODE -ne 0) { throw "No pude bajar el latest.json de $tag ($LASTEXITCODE): no se arma uno con menos plataformas." }
}
$latest = Join-Path $nsis "latest.json"
$pubDate = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ")
node (Join-Path $PSScriptRoot "updater-manifest.mjs") $previous $latest $ver $pubDate windows-x86_64 $sigPath $url
if ($LASTEXITCODE -ne 0) { throw "No se pudo armar latest.json ($LASTEXITCODE)." }

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
