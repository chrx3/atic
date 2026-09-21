# Recursos de la app

Medición del consumo real de Atic en reposo, con el método para volver a medirlo
y comparar. Hecha el **10-09-2026** sobre la **v0.4.30**, con el build de
desarrollo (`tauri dev`): el Rust va optimizado —el perfil `dev` del workspace ya
usa `optimized + debuginfo`—, pero el frontend corre sin empaquetar y con el
cliente de HMR.

Lo que **no** cambia entre dev y release son los hallazgos de comportamiento: las
animaciones eternas y los sondeos son código, no artefactos del dev server. Los
números de CPU del frontend, en cambio, deberían mejorar algo en release.

## Cómo se mide

Árbol completo de procesos (el binario **y** sus WebView2 hijos), memoria y CPU
por proceso. En PowerShell:

```powershell
$all = Get-CimInstance Win32_Process
$roots = @($all | Where-Object { $_.Name -eq 'atic-desktop.exe' } | Select-Object -ExpandProperty ProcessId)
$ids = @($roots)
do {
  $n = @($all | Where-Object { $ids -contains $_.ParentProcessId -and $ids -notcontains $_.ProcessId } |
    Select-Object -ExpandProperty ProcessId)
  $ids += $n
} while ($n.Count -gt 0)
Get-Process -Id $ids | Select-Object Id, ProcessName,
  @{n='MB'; e={[int]($_.WorkingSet64/1MB)}},
  @{n='PrivMB'; e={[int]($_.PrivateMemorySize64/1MB)}},
  @{n='CPU_s'; e={[math]::Round($_.CPU,1)}},
  @{n='Hilos'; e={$_.Threads.Count}}, Handles
```

Para la CPU, hay que muestrear dos veces y restar: `$_.CPU` es **tiempo acumulado**,
no un porcentaje. Diez o doce segundos de ventana alcanzan para ver lo sostenido.
Hay que incluir los `msedgewebview2` que cuelgan del árbol: mirar sólo
`atic-desktop.exe` esconde la mayor parte del costo.

## Memoria

| Componente | Working set | Privada |
|---|---|---|
| `atic-desktop.exe` (Rust) | 1.728 MB | — |
| 23 procesos WebView2 | ~2.340 MB | — |
| **Total del árbol** | **4.070 MB** | **3.134 MB** |

24 procesos en total: 1 binario, **11 renderers**, 3 procesos de GPU, 6
utilitarios y 3 crashpad. 689 hilos y 12.866 handles.

**Dos advertencias sobre estas cifras.** El working set **suma memoria
compartida**, así que las DLL de WebView2 se cuentan una vez por proceso: la
presión real sobre el sistema es menor. La columna privada no se comparte y por
eso es la cifra dura.

Y el 1,7 GB del proceso Rust **no es una fuga**: son los modelos Whisper
precargados a propósito (`modelo Whisper precargado model_id=medium`), y se
liberan solos tras 600 s de inactividad (`modelos Whisper liberados por idle`).
Esa memoria se paga mientras la app está en uso y desaparece si se la deja
quieta.

La cantidad de procesos merece su propio comentario: la app tiene del orden de
ocho ventanas-web (overlay, main, shelf, flip, launcher, lupa, annotate y el
panel de cupos) y cada una mantiene su propio renderer y compositor. Ahorrar
ventanas ahorra memoria y un compositor por vez.

## CPU en reposo

Sin tocar nada, en una ventana de doce segundos: **308 % de un núcleo**, o sea
**~19 % de una máquina de 16 núcleos**, sostenido.

| Proceso | CPU (% de un núcleo) |
|---|---|
| renderer WebView2 | 82,2 % |
| renderer WebView2 | 58,4 % |
| `atic-desktop.exe` (Rust) | 45,5 % |
| renderer WebView2 | 45,2 % |
| renderer WebView2 | 35,6 % |
| renderer WebView2 | 22,5 % |
| renderer WebView2 | 18,6 % |

El grueso es **frontend**: los renderers suman ~262 % contra 45 % del Rust.

## Qué lo explica

1. **La piel líquida respira para siempre.** `liquid/Skin.svelte:145` —
   `.skin.is-breathing { animation: skin-breathe 2.4s ease-in-out infinite }`. Vive
   en el overlay, que es la ventana que nunca se apaga, así que es un repintado
   continuo permanente.

2. **Once renderers.** Cada ventana-web sostiene su compositor y su heap;
   cualquier animación se multiplica por ventana.

3. **Sondeos en la ventana permanente.** `PillSurface.svelte:1758` mantiene un
   `setInterval` para detectar el hover de la isla, y
   `PillQuotaHost.svelte:289` corre otro **cada segundo** para refrescar tiempos
   relativos. `OverlaySurface.svelte:178` también sondea, pero 16 veces cada
   200 ms y después se apaga: ese está bien resuelto.

4. **Ocho `requestAnimationFrame` en `PillSurface.svelte`**, que es la ventana
   siempre visible.

5. **La franja del rail.** `RailDust.svelte` anima en bucle dentro de la ventana
   principal; es la candidata más probable a ser el renderer del 82 %.

## Veredicto

Para una utilidad que vive en la bandeja, **19 % de CPU en reposo es caro**: se
paga en batería y en ventilador. El problema no es la memoria —está mayormente
explicada y se libera sola— sino **el movimiento permanente**.

Ordenado por impacto y por facilidad:

1. **Que la piel no respire con la pill ociosa.** Es el ahorro más directo y no
   se nota: la respiración tiene sentido cuando *hay algo vivo* (grabando,
   transcribiendo, un agente corriendo), no siempre.
2. **Sacar el sondeo del hover.** El hover se detecta con `pointerenter` /
   `pointerleave`, no con un temporizador.
3. **Bajar el `setInterval` de un segundo de los cupos** si sólo alimenta tiempos
   relativos: con una reactividad por minuto alcanza.
4. **Revisar la franja del rail**: ¿puede parar cuando no hay nada que mostrar?
5. **Consolidar ventanas-web.** Es el trabajo más grande, pero cada una que se
   ahorra quita memoria y un compositor.

## Qué falta medir

- **El build de release**, que es el que usa la gente. Requiere cerrar la
  instancia de desarrollo primero: el plugin de instancia única no deja correr
  las dos.
- **La atribución por ventana.** Esta medición se hizo con la ventana principal
  abierta, así que `RailDust` está dentro de la cuenta. Cerrarla y volver a medir
  da el costo exacto de esa franja.
