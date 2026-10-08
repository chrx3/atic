# Compila la pill en desarrollo y la deja corriendo en lugar de la instalada
# (o de la de dev anterior). Para probar un cambio: correr de nuevo.
#
#   powershell -File scripts/pill-dev.ps1
#
# La pill de dev usa los mismos datos que la instalada, pero no toca el inicio
# con Windows. Para volver a la instalada: abrir «Atic» desde el menú Inicio.
$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$pill = Join-Path $raiz "prototypes\pill-gpui"
$target = Join-Path $env:LOCALAPPDATA "atic-gpui"

# La pill que corre bloquea su exe; renombrado, cargo puede escribir el nuevo
# y la vieja sigue andando hasta que se cierra abajo.
$exe = Join-Path $target "debug\pill-gpui.exe"
if (Test-Path $exe) {
    Remove-Item "$exe.old" -Force -ErrorAction SilentlyContinue
    Move-Item $exe "$exe.old" -Force
}

cargo build --manifest-path (Join-Path $pill "Cargo.toml") --target-dir $target
if ($LASTEXITCODE -ne 0) { throw "la pill no compiló ($LASTEXITCODE)" }

# Una sola pill por sesión: la que corra se cierra antes.
Get-Process atic-pill, pill-gpui -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400

# Desacoplada de esta consola: sobrevive aunque se cierre la terminal. La
# consola de `cmd` va oculta; la pill de dev escribe su log en
# %APPDATA%\ciat\atic\data\logs\pill.<fecha>.log.
$startup = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{ ShowWindow = [uint16]0 }
$result = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{
    CommandLine = "cmd /c `"`"$exe`" >nul 2>&1`""
    CurrentDirectory = $pill
    ProcessStartupInformation = $startup
}
if ($result.ReturnValue -ne 0) { throw "no se pudo abrir la pill ($($result.ReturnValue))" }
Write-Host "Pill de dev corriendo: $exe"
