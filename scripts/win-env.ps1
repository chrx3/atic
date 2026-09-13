# Configura el entorno de compilación Rust/Tauri en Windows.
#
# bindgen (que usa whisper-rs-sys) necesita libclang y los headers de MSVC y
# del Windows SDK. Si clang no los encuentra, bindgen falla y whisper-rs-sys
# cae a los bindings bundled (generados en Linux), que no compilan en MSVC
# (E0080: attempt to compute `12_usize - 16_usize`).
#
# Uso desde la raíz del repo:
#   . .\scripts\win-env.ps1      # dot-source, para la sesión actual
#   pnpm tauri dev
#
# release-windows.ps1 lo carga solo; no hace falta llamarlo a mano.

$llvmBin = $env:LIBCLANG_PATH
if (-not ($llvmBin -and (Test-Path (Join-Path $llvmBin "libclang.dll")))) {
    $llvmBin = @(
        "$env:ProgramFiles\LLVM\bin"
        "${env:ProgramFiles(x86)}\LLVM\bin"
    ) | Where-Object { Test-Path (Join-Path $_ "libclang.dll") } | Select-Object -First 1
}
if (-not $llvmBin) {
    throw "No encontré LLVM (libclang.dll). Instalalo desde https://releases.llvm.org/ y reintentá."
}
$clangExe = Join-Path $llvmBin "clang.exe"
$clangRes = (& $clangExe -print-resource-dir | Select-Object -First 1).Trim()

$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$msvcInclude = $null
if (Test-Path $vswhere) {
    $msvcInclude = @(& $vswhere -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -latest -find "VC/Tools/MSVC/*/include" 2>$null) |
        ForEach-Object { $_.Trim() } | Where-Object { $_ } | Select-Object -First 1
}
if (-not $msvcInclude) {
    $msvcInclude = Get-ChildItem @(
        "$env:ProgramFiles\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\include"
        "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\*\VC\Tools\MSVC\*\include"
    ) -Directory -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending | Select-Object -First 1 -ExpandProperty FullName
}

$sdkRoot = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows Kits\Installed Roots" -Name KitsRoot10 -ErrorAction SilentlyContinue).KitsRoot10
if (-not $sdkRoot) { $sdkRoot = "${env:ProgramFiles(x86)}\Windows Kits\10" }
$sdkInc = Get-ChildItem (Join-Path $sdkRoot "Include") -Directory -ErrorAction SilentlyContinue |
    Sort-Object Name -Descending | Select-Object -First 1

if (-not $msvcInclude -or -not $sdkInc) {
    throw "No encontré los headers de MSVC o del Windows SDK; bindgen no puede generar los bindings de whisper."
}

$clangArgs = @(
    "-isystem `"$(Join-Path $clangRes 'include')`""
    "-isystem `"$msvcInclude`""
)
foreach ($part in "ucrt", "shared", "um", "winrt") {
    $clangArgs += "-isystem `"$(Join-Path $sdkInc.FullName $part)`""
}

$env:LIBCLANG_PATH = $llvmBin
$env:BINDGEN_EXTRA_CLANG_ARGS = $clangArgs -join " "

Write-Host "clang/bindgen listo: LLVM en $llvmBin; MSVC $msvcInclude; SDK $($sdkInc.Name)"
