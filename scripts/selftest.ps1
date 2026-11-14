# Corre la autoprueba de arranque de la pill (PILL_SELFTEST=arranque, ver
# apps/atic/src/selftest.rs) con un ejecutable ya instalado, muestra el JSON
# y falla si la pill no terminó bien. Es el paso «Verify» del release.
#
# Uso:
#   powershell -File scripts/selftest.ps1 -Exe "$env:LOCALAPPDATA\Atic\atic-pill.exe"
#
# Por omisión exige el agente de Atic Code, atic-mcp y la ventana
# (PILL_SELFTEST_EXIGE=sidecar,mcp,ventana): en el instalador tienen que estar.
# Se pisa con la variable PILL_SELFTEST_EXIGE o PILL_SELFTEST_OMITE.
#
# Si la pill muere a medias, el JSON queda con lo que alcanzó y «en_curso»
# dice qué estaba comprobando.
#
# Puede correr junto a la pill abierta: la autoprueba no toma el candado de
# instancia única ni toca la que está corriendo.

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Exe,
    [string]$Out = (Join-Path ([System.IO.Path]::GetTempPath()) "atic-selftest.json")
)

$ErrorActionPreference = "Stop"

$plazo = if ($env:PILL_SELFTEST_PLAZO) { [int]$env:PILL_SELFTEST_PLAZO } else { 120 }
Remove-Item -Force -ErrorAction SilentlyContinue $Out

$env:PILL_SELFTEST = "arranque"
$env:PILL_SELFTEST_SALIDA = $Out
$env:PILL_SELFTEST_PLAZO = "$plazo"
if ($null -eq $env:PILL_SELFTEST_EXIGE) { $env:PILL_SELFTEST_EXIGE = "sidecar,mcp,ventana" }

# La pill de release no tiene consola: el resultado sale por el JSON.
$process = Start-Process -FilePath $Exe -PassThru
# Sin pedir el handle antes de esperar, ExitCode queda vacío.
$null = $process.Handle
# La pill se rinde sola al vencer su plazo; esto es por si ni eso alcanza.
if (-not $process.WaitForExit(($plazo + 60) * 1000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    Write-Warning "La autoprueba no terminó ni con su plazo ($plazo s): se cerró a la fuerza."
}
$status = $process.ExitCode
Remove-Item Env:PILL_SELFTEST, Env:PILL_SELFTEST_SALIDA

if (-not (Test-Path $Out) -or (Get-Item $Out).Length -eq 0) {
    throw "La pill terminó con $status y no escribió $Out (¿se cayó al arrancar?)."
}
$json = Get-Content -Raw -Encoding UTF8 $Out
Write-Host $json
$report = $json | ConvertFrom-Json
if ($env:GITHUB_STEP_SUMMARY) {
    $rows = @($report.comprobaciones | ForEach-Object { "| $($_.nombre) | $($_.estado) | $($_.ms) | $(([string]$_.detalle) -replace '\|', '/') |" })
    if ($report.en_curso) { $rows += "| $($report.en_curso) | no terminó | | la pill salió (código $status) sin terminar esta comprobación |" }
    $estado = if ($report.ok) { "ok" } else { "FALLA" }
    $text = @("### Autoprueba $($report.os)/$($report.arch) $($report.version): $estado", "", "| Comprobación | Estado | ms | Detalle |", "|---|---|---|---|") + $rows + ""
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::AppendAllText($env:GITHUB_STEP_SUMMARY, ($text -join "`n") + "`n", $utf8)
}
if ($report.en_curso) {
    Write-Warning "La pill salió con $status a medias: el JSON es parcial (mira «en_curso»)."
    if ($status -eq 0) { $status = 1 }
}
if ($status -ne 0) {
    throw "La autoprueba falló (código $status): mira las comprobaciones con estado «falla»."
}
Write-Host "Autoprueba ok: $Out"
