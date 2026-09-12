# Mide el consumo en reposo de Atic: CPU por delta, memoria y handles.
#
# Por qué un script y no cuatro comandos sueltos: `$_.CPU` es tiempo acumulado,
# no un porcentaje, así que hay que muestrear dos veces y restar. A mano eso da
# números que no se pueden comparar entre sesiones. Esto escribe un snapshot
# con fecha, y `-Comparar` difea los dos últimos para ver si un cambio mejoró
# o empeoró algo.
#
# Se mira el árbol COMPLETO (el binario y sus WebView2 hijos): mirar sólo
# `atic-desktop.exe` esconde la mayor parte del costo.
#
# Atribución: cada ventana corre sobre un perfil WebView2 distinto y eso viaja
# en la línea de comandos (`--user-data-dir`), así que el costo se puede separar
# por ventana sin ir cerrando una por una:
#
#   com.ciat.atic\overlay-webview          -> la placa click-through (siempre viva)
#   com.ciat.atic\capture-overlay-webview  -> la de captura (precargada al arrancar)
#   com.ciat.atic                          -> el perfil compartido: main, shelf,
#                                             annotate, lupa, flip y launcher
#
#   .\scripts\measure-runtime.ps1                          # 12 s de ventana
#   .\scripts\measure-runtime.ps1 -Ventana 30 -Etiqueta idle-largo
#   .\scripts\measure-runtime.ps1 -Comparar                # difea los últimos dos
#
# Los reportes van a `target/perf/` (ignorado por git, no ensucia el repo).

[CmdletBinding()]
param(
    # Nombre del ejecutable raíz, sin `.exe`.
    [string]$Nombre = "atic-desktop",
    # Segundos de ventana para el delta de CPU.
    [int]$Ventana = 12,
    # Etiqueta libre para el nombre del reporte (p. ej. `release-reposo`).
    [string]$Etiqueta = "",
    # En vez de medir, difear los dos snapshots más recientes.
    [switch]$Comparar
)

$ErrorActionPreference = "Stop"

$raiz = Split-Path -Parent $PSScriptRoot
$salidaDir = Join-Path $raiz "target\perf"
New-Item -ItemType Directory -Force -Path $salidaDir | Out-Null

# --- Comparación: difea los dos últimos snapshots -------------------------

if ($Comparar) {
    $previos = @(Get-ChildItem $salidaDir -Filter "*.json" | Sort-Object LastWriteTime)
    if ($previos.Count -lt 2) {
        throw "Hacen falta dos snapshots para comparar; hay $($previos.Count) en $salidaDir"
    }
    $viejo = Get-Content $previos[-2].FullName -Raw | ConvertFrom-Json
    $nuevo = Get-Content $previos[-1].FullName -Raw | ConvertFrom-Json

    function _Delta($campo) {
        $a = $viejo.resumen.$campo
        $b = $nuevo.resumen.$campo
        $dif = $b - $a
        $pct = if ($a -ne 0) { ($dif / $a) * 100 } else { 0 }
        [pscustomobject]@{
            Campo = $campo
            Antes = [math]::Round($a, 1)
            Ahora = [math]::Round($b, 1)
            Delta = [math]::Round($dif, 1)
            Pct   = [math]::Round($pct, 1)
        }
    }

    Write-Host ""
    Write-Host "Comparando:" -ForegroundColor Cyan
    Write-Host "  antes: $($viejo.etiqueta)  ($($viejo.fecha))" -ForegroundColor DarkGray
    Write-Host "  ahora: $($nuevo.etiqueta)  ($($nuevo.fecha))" -ForegroundColor DarkGray
    Write-Host ""
    @(
        _Delta "cpu_pct_un_nucleo"
        _Delta "cpu_pct_frontend"
        _Delta "cpu_pct_rust"
        _Delta "ws_total_mb"
        _Delta "privada_total_mb"
        _Delta "procesos"
        _Delta "hilos"
        _Delta "handles"
    ) | Format-Table -AutoSize
    return
}

# --- Datos del árbol ------------------------------------------------------

# El perfil sale del `--user-data-dir`. Ojo con los crashpad: su línea trae la
# ruta y después `/prefetch`, así que el corte tiene que ser en el espacio.
# El binario de Rust no tiene perfil: se etiqueta aparte para que no compita con
# las ventanas en la tabla por perfil.
function _Perfil([string]$cmd) {
    if (-not $cmd) { return "sin-perfil" }
    if ($cmd -notmatch '--user-data-dir="?([^"\s]+)') { return "sin-perfil" }
    $ruta = $Matches[1]
    if ($ruta -match 'com\.ciat\.atic\\([^\\]+)\\EBWebView') { return $Matches[1] }
    if ($ruta -match 'com\.ciat\.atic\\EBWebView') { return "compartido" }
    return (Split-Path $ruta -Leaf)
}

function _Tipo([string]$nombre, [string]$cmd) {
    # `Win32_Process.Name` viene con extensión (`msedgewebview2.exe`) y
    # `Get-Process.ProcessName` sin ella. Normalizo para no depender de cuál llegó.
    $base = [System.IO.Path]::GetFileNameWithoutExtension($nombre)
    if ($base -ne "msedgewebview2") { return "binario" }
    if ($cmd -match '--type=([a-z-]+)') { return $Matches[1] }
    return "browser"
}

# ParentProcessId viene como UInt32 y Get-Process.Id como Int32. En .NET
# `[uint32]5.Equals([int]5)` es false, así que una hashtable con claves UInt32
# no se encuentra desde un Id de Get-Process: hay que normalizar a [int].
# (Costó un rato de depuración: la columna de tipo salía vacía.)
function _ArbolProcesos([string]$nombreExe) {
    $todos = Get-CimInstance Win32_Process
    $raices = @($todos |
        Where-Object { $_.Name -eq "$nombreExe.exe" } |
        Select-Object -ExpandProperty ProcessId |
        ForEach-Object { [int]$_ })
    if ($raices.Count -eq 0) {
        throw "No hay ningún proceso '$nombreExe.exe' corriendo. Arrancá la app (release: .\target\release\$nombreExe.exe) y volvé a medir."
    }

    $ids = @($raices)
    do {
        $nuevos = @($todos |
            Where-Object { $ids -contains [int]$_.ParentProcessId -and $ids -notcontains [int]$_.ProcessId } |
            Select-Object -ExpandProperty ProcessId |
            ForEach-Object { [int]$_ })
        $ids += $nuevos
    } while ($nuevos.Count -gt 0)

    $tipo = @{}
    $perfil = @{}
    foreach ($p in $todos) {
        $id = [int]$p.ProcessId
        if ($ids -notcontains $id) { continue }
        $tipo[$id] = _Tipo $p.Name $p.CommandLine
        $perfil[$id] = if ($tipo[$id] -eq "binario") { "binario-rust" } else { _Perfil $p.CommandLine }
    }

    return [pscustomobject]@{ Ids = $ids; Tipo = $tipo; Perfil = $perfil }
}

function _Muestra([int[]]$ids) {
    $muestra = @{}
    foreach ($p in (Get-Process -Id $ids -ErrorAction SilentlyContinue)) {
        $muestra[[int]$p.Id] = [pscustomobject]@{
            Cpu     = $p.CPU          # segundos acumulados, sumados sobre todos los núcleos
            Ws      = $p.WorkingSet64
            Privada = $p.PrivateMemorySize64
            Hilos   = $p.Threads.Count
            Handles = $p.HandleCount
            Nombre  = $p.ProcessName
        }
    }
    return $muestra
}

$arbol = _ArbolProcesos $Nombre
Write-Host ""
Write-Host "Midiendo $($arbol.Ids.Count) procesos durante $Ventana s..." -ForegroundColor Cyan

$antes = _Muestra $arbol.Ids
Start-Sleep -Seconds $Ventana
$despues = _Muestra $arbol.Ids

# --- Cálculo --------------------------------------------------------------

$filas = @()
foreach ($id in $despues.Keys) {
    $d = $despues[$id]
    # Un proceso que nace dentro de la ventana no tiene CPU "antes": su delta
    # es su CPU entera, que sobreestima. Se marca en vez de mentir.
    $c = $antes[$id]
    $dcpu = if ($c) { $d.Cpu - $c.Cpu } else { $d.Cpu }
    $filas += [pscustomobject]@{
        Id      = $id
        Nombre  = $d.Nombre
        Tipo    = $arbol.Tipo[$id]
        Perfil  = $arbol.Perfil[$id]
        Cpu_pct = [math]::Round(($dcpu / $Ventana) * 100, 1)
        Ws_mb   = [int]($d.Ws / 1MB)
        Priv_mb = [int]($d.Privada / 1MB)
        Hilos   = $d.Hilos
        Handles = $d.Handles
        Nuevo   = -not $c
    }
}
$filas = $filas | Sort-Object Cpu_pct -Descending

function _Suma($f, $campo) {
    if (-not $f) { return 0 }
    return ($f | Measure-Object $campo -Sum).Sum
}

# "Frontend" es todo lo que cuelga del binario. Es la distinción que decide si
# el trabajo es de Svelte o de Rust.
$frontend = @($filas | Where-Object { $_.Tipo -ne "binario" })
$rust = @($filas | Where-Object { $_.Tipo -eq "binario" })

# Por perfil: acá se ve qué ventana cuesta, que es la pregunta que el doc dejó
# abierta.
$porPerfil = @($filas |
    Group-Object Perfil |
    ForEach-Object {
        $g = $_.Group
        [pscustomobject]@{
            Perfil   = $_.Name
            Cpu_pct  = [math]::Round((_Suma $g "Cpu_pct"), 1)
            Ws_mb    = _Suma $g "Ws_mb"
            Priv_mb  = _Suma $g "Priv_mb"
            Procesos = $g.Count
        }
    } | Sort-Object Cpu_pct -Descending)

$resumen = [pscustomobject]@{
    cpu_pct_un_nucleo = [math]::Round((_Suma $filas "Cpu_pct"), 1)
    cpu_pct_frontend  = [math]::Round((_Suma $frontend "Cpu_pct"), 1)
    cpu_pct_rust      = [math]::Round((_Suma $rust "Cpu_pct"), 1)
    ws_total_mb       = _Suma $filas "Ws_mb"
    privada_total_mb  = _Suma $filas "Priv_mb"
    procesos          = $filas.Count
    renderers         = @($filas | Where-Object { $_.Tipo -eq "renderer" }).Count
    gpu               = @($filas | Where-Object { $_.Tipo -eq "gpu-process" }).Count
    utilitarios       = @($filas | Where-Object { $_.Tipo -eq "utility" }).Count
    hilos             = _Suma $filas "Hilos"
    handles           = _Suma $filas "Handles"
    ventana_s         = $Ventana
}

# --- Reporte --------------------------------------------------------------

$sello = Get-Date -Format "yyyy-MM-dd_HHmmss"
$etiqueta = if ($Etiqueta) { $Etiqueta } else { "sin-etiqueta" }
$base = Join-Path $salidaDir "$sello`_$etiqueta"
$datos = [pscustomobject]@{
    fecha    = (Get-Date -Format "yyyy-MM-dd HH:mm:ss")
    etiqueta = $etiqueta
    exe      = $Nombre
    resumen  = $resumen
    perfiles = $porPerfil
    procesos = $filas
}
$datos | ConvertTo-Json -Depth 6 | Set-Content "$base.json" -Encoding UTF8

Write-Host ""
Write-Host "=== Reposo, $Ventana s ===" -ForegroundColor Green
Write-Host ("  CPU total          {0,8:N1} % de un núcleo" -f $resumen.cpu_pct_un_nucleo) -ForegroundColor Yellow
Write-Host ("    frontend         {0,8:N1} %" -f $resumen.cpu_pct_frontend)
Write-Host ("    Rust             {0,8:N1} %" -f $resumen.cpu_pct_rust)
Write-Host ("  Memoria WS total   {0,8:N0} MB" -f $resumen.ws_total_mb)
Write-Host ("  Memoria privada    {0,8:N0} MB" -f $resumen.privada_total_mb)
Write-Host ("  Procesos           {0,8:N0}  ({1} renderers, {2} gpu, {3} utilitarios)" -f `
        $resumen.procesos, $resumen.renderers, $resumen.gpu, $resumen.utilitarios)
Write-Host ("  Hilos / handles    {0,8:N0} / {1:N0}" -f $resumen.hilos, $resumen.handles)
Write-Host ""
Write-Host "Por perfil (ventana):" -ForegroundColor Cyan
$porPerfil | Format-Table -AutoSize
Write-Host "Top consumidores:" -ForegroundColor Cyan
$filas | Select-Object -First 10 Id, Tipo, Perfil, Cpu_pct, Ws_mb, Priv_mb | Format-Table -AutoSize
Write-Host "Reporte: $base.json" -ForegroundColor DarkGray
