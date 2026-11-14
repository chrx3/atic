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
# Con cargo 1.100, ort-sys no encuentra dónde copiar los DLL de ONNX Runtime
# (ver win-env.ps1): se vuelve a la estructura anterior de la compilación.
$env:__CARGO_TEMPORARY_BUILD_DIR_NEW_LAYOUT_OPT_OUT = "1"
$raiz = Split-Path -Parent $PSScriptRoot
$pill = Join-Path $raiz "apps\atic"
$target = Join-Path $env:LOCALAPPDATA "atic-gpui"
$exe = Join-Path $target "debug\pill-gpui.exe"

# La pill de dev usa el agente de Atic Code (`sidecar\agent.mjs`) y el servidor MCP
# (`atic-mcp.exe`) que estén junto a su exe. Si quedan viejos, Atic Code habla con
# código de otra versión: con un `atic-mcp` sin `ATIC_HUB_TOKEN`, los subagentes se
# creaban sin madre (no salían en su chip) y la delegación fallaba por tiempo.
# Se compilan solo si cambió su código y se copian en cada pasada.
function Update-Helpers {
    $debug = Join-Path $target "debug"

    $sidecar = Join-Path $pill "sidecar"
    $agente = Join-Path $sidecar "dist\agent.mjs"
    $fuentes = Get-ChildItem $sidecar -File -Filter *.mjs | Where-Object { $_.Name -ne "build.mjs" }
    $viejo = -not (Test-Path $agente) -or
        ($fuentes | Where-Object { $_.LastWriteTime -gt (Get-Item $agente).LastWriteTime })
    if ($viejo) {
        Write-Host "Compilando el agente de Atic Code..."
        pnpm --dir $sidecar run build
        if ($LASTEXITCODE -ne 0) { throw "el agente no compiló ($LASTEXITCODE)" }
    }
    $destino = Join-Path $debug "sidecar"
    New-Item -ItemType Directory -Force $destino | Out-Null
    Copy-Item $agente (Join-Path $destino "agent.mjs") -Force

    # Sin cambios, cargo no compila nada y tarda un par de segundos.
    cargo build -p atic-mcp --release --manifest-path (Join-Path $raiz "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "atic-mcp no compiló ($LASTEXITCODE)" }
    $mcp = Join-Path $raiz "target\release\atic-mcp.exe"
    $junto = Join-Path $debug "atic-mcp.exe"
    $igual = (Test-Path $junto) -and (Get-FileHash $mcp).Hash -eq (Get-FileHash $junto).Hash
    if (-not $igual) {
        # Lo usan las CLI abiertas: renombrado, el nuevo entra sin cerrarlas.
        if (Test-Path $junto) {
            Remove-Item "$junto.old" -Force -ErrorAction SilentlyContinue
            Move-Item $junto "$junto.old" -Force
        }
        Copy-Item $mcp $junto -Force
        Write-Host "atic-mcp al día junto a la pill de dev"
    }
}

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
    Update-Helpers

    # Una sola pill por sesión: la que corra se cierra antes.
    # Sin cortar si una ya se cerró sola entre buscarla y cerrarla.
    Get-Process atic-pill, pill-gpui -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
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
# `include_bytes!`/`include_str!`. Fuera, las carpetas de compilación y de Node,
# y `tweaks.json`, que la pill relee sola sin reabrirse.
function Test-Fuente([string]$ruta) {
    $ruta -match '\.(rs|toml|svg|png|ico|json)$' -and $ruta -notmatch '\\(target|node_modules|dist)\\' -and
        $ruta -notmatch '\\tweaks\.json$'
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
            # Lo que escribe otra compilación en `target/` no alarga la espera.
            $mas = @($mas | Where-Object { Test-Fuente $_.SourceEventArgs.FullPath })
        } while ($mas.Count -gt 0)

        Write-Host ""
        Write-Host ("[{0:HH:mm:ss}] Cambió {1}" -f (Get-Date), $ruta.Substring((Split-Path -Parent $raiz).Length + 1)) -ForegroundColor Cyan
        try { Update-Pill } catch { Write-Host $_ -ForegroundColor Red }
    }
} finally {
    Get-EventSubscriber | Unregister-Event
    $vigilantes | ForEach-Object { $_.Dispose() }
}
