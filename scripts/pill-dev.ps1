# Compila la pill en desarrollo y la deja corriendo en lugar de la instalada
# (o de la de dev anterior). Para probar un cambio: correr de nuevo.
#
#   powershell -File scripts/pill-dev.ps1
#   powershell -File scripts/pill-dev.ps1 -Watch   # recompila y reabre al guardar
#
# Con -Watch vigila la pill, `crates/` y `../gpui-m3`; al guardar compila y
# reabre la pill, de a una compilación: lo que se guarde mientras compila entra
# en la siguiente. Si no compila, la pill anterior sigue abierta. Ctrl+C sale.
#
# La pill de dev usa los mismos datos que la instalada, pero no toca el inicio
# con Windows. Para volver a la instalada: abrir «Atic» desde el menú Inicio.
param([switch]$Watch)

$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
$pill = Join-Path $raiz "prototypes\pill-gpui"
$target = Join-Path $env:LOCALAPPDATA "atic-gpui"
$exe = Join-Path $target "debug\pill-gpui.exe"

function Update-Pill {
    # La pill que corre bloquea su exe; renombrado, cargo puede escribir el nuevo
    # y la vieja sigue andando hasta que se cierra abajo.
    if (Test-Path $exe) {
        Remove-Item "$exe.old" -Force -ErrorAction SilentlyContinue
        Move-Item $exe "$exe.old" -Force
    }

    $reloj = [Diagnostics.Stopwatch]::StartNew()
    cargo build --manifest-path (Join-Path $pill "Cargo.toml") --target-dir $target
    if ($LASTEXITCODE -ne 0) { throw "la pill no compiló ($LASTEXITCODE)" }
    Write-Host ("Compilada en {0:N0} s" -f $reloj.Elapsed.TotalSeconds)

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
}

if (-not $Watch) {
    Update-Pill
    return
}

# Lo que cambia el ejecutable: código, manifiestos y lo que entra con
# `include_bytes!`/`include_str!`. Fuera, las carpetas de compilación y de Node.
function Test-Fuente([string]$ruta) {
    $ruta -match '\.(rs|toml|svg|png|ico|json)$' -and $ruta -notmatch '\\(target|node_modules|dist)\\'
}

$carpetas = @($pill, (Join-Path $raiz "crates"), (Join-Path (Split-Path -Parent $raiz) "gpui-m3")) |
    Where-Object { Test-Path $_ }
$vigilantes = foreach ($carpeta in $carpetas) {
    $vigilante = New-Object IO.FileSystemWatcher $carpeta
    $vigilante.IncludeSubdirectories = $true
    $vigilante.NotifyFilter = [IO.NotifyFilters]'FileName, LastWrite'
    foreach ($evento in 'Changed', 'Created', 'Deleted', 'Renamed') {
        Register-ObjectEvent $vigilante $evento | Out-Null
    }
    $vigilante.EnableRaisingEvents = $true
    $vigilante
}

try {
    try { Update-Pill } catch { Write-Host $_ -ForegroundColor Red }
    Write-Host "Vigilando cambios (Ctrl+C para salir)..." -ForegroundColor Cyan
    while ($true) {
        $evento = Wait-Event
        Remove-Event -EventIdentifier $evento.EventIdentifier
        $ruta = $evento.SourceEventArgs.FullPath
        if (-not (Test-Fuente $ruta)) { continue }

        # Junta en una sola compilación los guardados seguidos (varios
        # archivos, el formateo del editor).
        do {
            Start-Sleep -Milliseconds 400
            $mas = @(Get-Event -ErrorAction SilentlyContinue)
            $mas | Remove-Event
        } while ($mas.Count -gt 0)

        Write-Host ""
        Write-Host ("[{0:HH:mm:ss}] Cambió {1}" -f (Get-Date), $ruta.Substring((Split-Path -Parent $raiz).Length + 1)) -ForegroundColor Cyan
        try { Update-Pill } catch { Write-Host $_ -ForegroundColor Red }
    }
} finally {
    Get-EventSubscriber | Unregister-Event
    $vigilantes | ForEach-Object { $_.Dispose() }
}
