# Panel de sistema

**Estado:** `hecho`

## Resumen

Controles del equipo desde la pill: CPU y RAM con lista de apps, volumen
(maestro en ambos SO, por app solo en Windows), brillo por monitor y cierre
de programas. Abre como cara de la isla, igual que clipboard, y se puede
despegar a un float.

## Cómo se usa

- **La rueda del mouse sobre la pill cambia el volumen**, sin abrir nada. Es el
  gesto que el panel nunca debió pedir: el cursor ya está ahí y la pill vive
  siempre arriba. Sobre una cara abierta (historial, consola) la rueda sigue
  siendo de ella. El porcentaje aparece un segundo en la misma cápsula donde la
  pill cuenta el resto de sus avisos.
- Rueda de la pill → **Sistema**, o buscar `Sistema` en el launcher.
- Cuatro pestañas: recursos, audio, pantalla y avisos.
- Fila rápida: bloquear, suspender, silencio, vaciar papelera y **café**.
  Vaciar pide confirmación propia: es lo único irreversible del panel.
- **Café** mantiene despierto el equipo y la pantalla mientras esté puesto. No
  se persiste: un "no te duermas" que sobrevive a un reinicio es una batería
  vacía al día siguiente sin que nadie sepa por qué. Se suelta al cerrar Atic.
- **Clic en una fila trae la app al frente.** Cerrar y forzar aparecen al pasar
  por encima, para no competir con el nombre.
- Cerrar una app manda el cierre normal (como el aspa) y avisa qué pasó.
  **Forzar cierre** pide confirmación y no se puede deshacer.
- Un filtro acota la lista, y el interruptor **Segundo plano** muestra los
  procesos sin ventana.
- El pin del float la deja siempre encima, como agentes o clipboard.

## Avisos: el equipo te busca a ti

El panel hay que acordarse de abrirlo, y uno se acuerda cuando ya está
sufriendo. Un hilo en Rust toma el pulso cada 5 s —CPU y memoria, **sin**
enumerar procesos— y si el umbral se sostiene, la pill lo dice con la misma
cápsula que usa para el aviso de actualización. El clic abre el panel y baja el
chip; el aviso vuelve si el problema se va y regresa.

Tres decisiones que hacen que no sea ruido:

1. **Sostenido, no pico.** Abrir una app deja la CPU en 100% por dos segundos;
   eso no es un problema. Por defecto hay que estar arriba dos minutos.
2. **Histéresis al bajar.** El aviso no se apaga apenas cruza el umbral hacia
   abajo, sino 8 puntos más abajo: si no, un valor que baila alrededor del
   umbral enciende y apaga el chip cada cinco segundos.
3. **El culpable se busca solo cuando hay algo que contar.** Saber si la CPU
   está alta cuesta dos llamadas; saber *quién* cuesta recorrer los ~500
   procesos. Lo segundo se paga una vez, al encender el aviso.

Los umbrales viven en la pestaña **Avisos** (0 = no vigilar esa métrica) y se
guardan en la config; el vigilante los relee en el siguiente latido.

## Qué cuenta como una app

La lista no son procesos sueltos: son **bundles**. Un helper de Chrome o de un
Electron vive dentro del `.app` que lo contiene, así que su CPU y su memoria
suman a esa app — sin eso, un Chrome de 4 GB se mostraba con los 300 MB de su
proceso principal.

Lo que no vive en un bundle (`node`, `cargo`, `rust-analyzer`, un demonio) es
**segundo plano**: no aparece salvo que enciendas el interruptor o lo busques
por nombre, porque son decenas y tapan lo demás. Se pueden cerrar igual
(`TERM`) o forzar (`KILL`) si son software del usuario; nada que viva en
`/System`, `/usr` (salvo `/usr/local`), `/bin` o `/sbin` se ofrece para cerrar.

Cada pestaña se refresca a su ritmo y solo la que se ve: recursos cada 1,5 s,
audio cada 2,5 s, pantalla al entrar. Enumerar los procesos del equipo mientras
miras el volumen no tiene sentido.

## Código

- [`apps/desktop/src-tauri/src/system_control/`](../apps/desktop/src-tauri/src/system_control/) — snapshot, audio, pantallas y cierre
- [`apps/desktop/src-tauri/src/system_actions.rs`](../apps/desktop/src-tauri/src/system_actions.rs) — lock / sleep / mute / trash (Windows y macOS)
- [`apps/desktop/src/lib/features/system/SystemPanel.svelte`](../apps/desktop/src/lib/features/system/SystemPanel.svelte) — UI del panel
- [`apps/desktop/src/lib/surfaces/overlay/system/SystemFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/system/SystemFloat.svelte) — float despegable

## Código pertinente

- [`system_control/snapshot.rs`](../apps/desktop/src-tauri/src/system_control/snapshot.rs) —
  `identify()` decide a qué app pertenece un proceso y si se puede tocar (puro y con tests).
- [`core/systemList.ts`](../apps/desktop/src/lib/core/systemList.ts) — filtro,
  búsqueda y orden de la lista (puro y con tests).
- [`system_control/watch.rs`](../apps/desktop/src-tauri/src/system_control/watch.rs) —
  el vigilante; `Sostenido::tick` es puro y se prueba con un reloj de mentira.
- [`system_control/awake.rs`](../apps/desktop/src-tauri/src/system_control/awake.rs) —
  café (`caffeinate` en macOS, `SetThreadExecutionState` en Windows).
- [`domain/systemAlerts.svelte.ts`](../apps/desktop/src/lib/domain/systemAlerts.svelte.ts) —
  los avisos vivos; se monta con la pill, no con el panel.

## Pendiente / siguiente

- [ ] Volumen por app en macOS (no hay API pública)
- [ ] Elegir dispositivo de salida en macOS (eso sí se puede, y es lo que más se busca)
- [ ] Resolución / Hz por monitor
- [ ] Brillo de monitores externos (DDC); hoy solo la pantalla interna
- [ ] Reiniciar / apagar con confirmación propia
- [ ] Historial corto de CPU/RAM (sparkline) en vez de dos barras planas
- [ ] Batería: porcentaje, tiempo restante y modo de bajo consumo
- [ ] Umbral de disco lleno y de batería baja (el vigilante ya tiene el molde)
- [ ] Marcar en la lista los procesos que lanzó Atic (consolas de agentes) y
      llevar a su consola en vez de ofrecer matarlos a ciegas

## Relacionado

- [system-actions.md](system-actions.md)
- [pill-shell.md](pill-shell.md)
- [pill-liquid-emerge.md](pill-liquid-emerge.md)
- [launcher-spotlight.md](launcher-spotlight.md)
