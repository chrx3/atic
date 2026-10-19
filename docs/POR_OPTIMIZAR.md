# Por optimizar

Lo que mantiene a la pill redibujando o sondeando sin necesidad (CPU y
batería), ordenado por impacto en uso normal: la pill quieta casi todo el
día, música a ratos y el cursor pasando cerca del notch. Sale de leer el
código; lo marcado como «sin confirmar» no se midió.

Las rutas son relativas a `prototypes/pill-gpui/src`. Para medir, abrir la
pill con `PILL_FPS=1` (escribe los cuadros por segundo en
`pill-salida.log`) y mirar la CPU con `ps -o %cpu= -p <pid>`.

Medido el 2026-10-19 en un MacBook Air de 8 GB, con un video de YouTube
sonando: la pill dibujaba 30 cuadros/s con ~5,7 % de CPU, aunque la onda se
veía casi plana.

## La pill

### 1. Los ojos redibujan por movimientos que no se ven (macOS y Windows)

- `main.rs` `moved_matters` (umbral `turned` de 0,02) y `advance`
  (`eyes_moving`, umbral 0,01).
- Los ojos se mueven como mucho ~1,5 px. Esos umbrales equivalen a ~0,03 px y
  ~0,015 px: lejos de la pill, casi cualquier movimiento del mouse pide
  cuadros a la frecuencia de la pantalla durante ~0,3 s. Mientras se usa el
  Mac, la pill redibuja casi sin parar.
- Arreglo: subir `turned` a ~0,15 y `eyes_moving` a ~0,05, o redondear el
  desplazamiento dibujado a 0,25 px y pedir cuadro solo si cambia. Se ve
  igual.

### 2. Con el cursor cerca del notch, ~20 redibujos por segundo sin cambios

- `main.rs` `tick`: a menos de `CALM_DISTANCE` (160 px) `calm` queda en
  `false`, el sondeo va a `TICK_ACTIVE` (16 ms) y `refresh_every = 3` llama a
  `cx.notify()` cada 48 ms aunque nada cambie. Pasa seguido: la barra de menú
  y las pestañas están arriba al centro.
- Arreglo: notificar solo si el cursor se movió, cambió algún estado o hay un
  reloj visible (grabación, dictado, progreso). Opcional: sondeo de 33 ms con
  el cursor cerca pero quieto más de 1 s.

### 3. Con la música en pausa, 30 cuadros/s durante 10 minutos

- `main.rs` `PAUSED_ART_FOR` (10 min) y el cálculo de `live_on` y
  `wave_only` en `tick`.
- En pausa `live` sigue en 1 y se activa `wave_only` (sondeo de 33 ms y
  `notify` en cada vuelta), pero las barras no se ven (`bars_shown` exige
  `music_playing`): solo queda la carátula quieta. Lo mismo grabando o
  dictando, que también esconden las barras.
- Arreglo: para pedir cuadros, usar
  `live.is_running(now) || (bars_shown() && live_shown > 0.01)`; en pausa la
  pill vuelve a la calma con la carátula dibujada.

### 4. El vigilante de agentes escanea cada 1 s, siempre (macOS y Windows)

- `agents.rs` `REFRESH_EVERY` (1 s).
- Cada segundo, aunque el notch esté cerrado: recorre las carpetas de
  `~/.claude/projects` y la fecha de cada `.jsonl`, revisa las sesiones de
  Codex, consulta el SQLite de OpenCode y toma la lista de procesos (en macOS
  lanza `ps -axww` como proceso nuevo; en Windows, Toolhelp).
- Arreglo: cada 1 s solo con un agente trabajando o el notch o la bandeja
  abiertos; si no, cada 5 s. La lista de procesos cada 5–10 s, o solo con
  Cursor abierto. Más adelante: eventos del sistema de archivos (FSEvents,
  `ReadDirectoryChangesW`).

### 5. La onda a 30 cuadros/s aunque esté tapada

- `main.rs` `tick` (`live_on` y `wave_only`).
- Sigue aunque la tira la tape (`live_shown` ≈ 0) o una app a pantalla
  completa oculte la pill. Sin el permiso de audio del sistema, además, el
  nivel es una onda sintética.
- Arreglo: no notificar con `live_shown(now) < 0.01`; con 20–24 cuadros/s las
  cuatro barras se ven casi igual.

### 6. Nada se detiene con la pantalla bloqueada o apagada

- Parpadeo (`schedule_blink` y `advance`), vistazo a los costados
  (`look_around`), el refresco de 1 s en calma y el sondeo de 50 ms.
- Nada los pausa con la sesión bloqueada, la pantalla dormida o una app a
  pantalla completa encima.
- Arreglo: pausar parpadeo, vistazo y onda con la sesión bloqueada o la
  pantalla dormida (macOS `NSWorkspaceScreensDidSleepNotification` y sesión
  bloqueada; Windows `WTS_SESSION_LOCK`) y tras ~5 min sin uso. El `notify` de
  1 Hz, solo con un reloj visible. El parpadeo, a 30 cuadros/s como máximo.

### 7. El medidor de audio de Windows lee a 30 Hz sin música

- `media.rs`, el hilo del medidor de Windows: lee el nivel cada 33 ms y cada
  ~0,5 s vuelve a crear el enumerador COM y el medidor. Nunca para.
- Arreglo: dormir 250–500 ms sin pista sonando (como ya hace el de macOS) y
  reabrir el dispositivo cada ~5 s o cuando cambie la salida.

## Las otras ventanas

### 8. Git cada 3 s en Atic Code y en el Espacio, con `notify` siempre

- `code/mod.rs` y `space/mod.rs`: `git status` cada 3 s (el Espacio con
  `--untracked-files=all`) y `cx.notify()` aunque nada cambie, también con la
  ventana sin foco u oculta.
- Arreglo: notificar solo si el resultado cambió; cada 3 s con la ventana
  activa, 15–30 s en segundo plano, y refrescar al volver el foco.

### 9. Animaciones que no paran

- `meetings/actions.rs` `pulse_dot`: usa `.repeat()` de GPUI, que pide un
  cuadro por refresco de pantalla (60–120 por segundo), con foco o sin él.
  También en `meetings/import.rs`. Arreglo: `motion::continuous_at(15.)` o
  3–4 pasos de opacidad.
- Indicadores de carga de Atic Code (`code/sidebar.rs`, `code/tools.rs`,
  `code/view.rs`): 30 cuadros/s con foco y 12 atrás, y cada uno redibuja toda
  la vista. Sin confirmar si GPUI deja de pintar con la ventana minimizada o
  tapada. Arreglo: un punto fijo en las filas del sidebar mientras la ventana
  no está activa.
- Formas del estado vacío de Code (`code/view.rs`): 30 cuadros/s; con 15 se
  ven igual.
- Barra de cuenta regresiva de deshacer (`meetings/actions.rs`): 6 s a la
  frecuencia de pantalla. Bastan 10–12 cuadros/s o pasos, como la bandeja de
  capturas.

## Menores

- `paste_queue.rs`: después del primer uso pregunta por la app del frente cada
  400 ms para siempre, aunque la cola esté vacía. Mirar la cola antes y
  terminar el hilo cuando esté vacía.
- El lector de lo que suena en macOS (el `osascript` de `media.rs`): consulta
  cada 0,5–1 s siempre. Bajar a 3–5 s tras ~1 min sin nada sonando. Sin
  confirmar si el proceso crece con la música sonando (se vio en 7–12 MB).
- `phone.rs` (solo con un celular vinculado): publica el historial del
  portapapeles cada 2 s sin mirar si cambió. Sin confirmar si el envío
  descarta repetidos.
- `meetings/detect.rs`: relee la configuración del disco cada 5 s; podría
  quedar en memoria.

## Ya está bien

- Parpadeo y vistazo piden cuadros solo mientras se mueven.
- La bandeja de capturas se achica en pasos y deja a la pill en calma.
- El texto que llega en Atic Code va a 30 cuadros/s como máximo (12 atrás).
- Las formas de gpui-m3 (`Shape`) se detienen en ventanas inactivas.
- Las consolas de agentes duermen si no hay ninguna abierta.
- El panel de Sistema deja de refrescarse al cerrarse.
- El portapapeles solo lee cuando cambia (`changeCount`).
