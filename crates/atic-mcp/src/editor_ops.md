Aplica una lista de operaciones al proyecto de la grabación abierta en el editor de Atic. Todo el lote es UN solo paso de deshacer («Cambios del asistente»): el usuario lo revierte con Cmd+Z y ve los cambios en vivo en la línea de tiempo. Es todo o nada: si una operación falla, no se aplica ninguna y la respuesta dice cuál y por qué. Devuelve el resultado por operación (`ok` o `error`) y el `state` resumido.

Antes de editar, mira con atic_editor_frames y atic_editor_transcript (y atic_editor_activity para clics, escritura y silencios), y revisa los índices actuales con atic_editor_state. Agrupa todo lo que puedas en un solo llamado.

Argumentos: `ops` (lista de operaciones, cada una un objeto con `op`), `label` opcional (nombre del paso de deshacer) y `video` opcional (ruta de la grabación; sin él, el editor que tuvo el foco al último).

Todos los tiempos son segundos del VIDEO ORIGINAL (los mismos que usa el proyecto), no del resultado tras cortes o velocidades. `rect` y los puntos de foco `x`/`y` van de 0 a 1, con origen arriba a la izquierda. Un valor fuera de rango se rechaza con un mensaje que dice el rango válido (no se acota en silencio).

Operaciones (`op`):

| `op` | Campos | Efecto |
| --- | --- | --- |
| `trim` | `start`, `end` | Deja solo ese tramo (recorte de inicio y final). |
| `clear_trim` | — | Quita el recorte. |
| `cut` | `start`, `end` | Quita un tramo del medio. |
| `zoom` | `start`, `end`, `scale?` (1,2–4, por defecto 2), `x?`, `y?` | Acerca la cámara. Sin `x`/`y` sigue al cursor; con ellos mira ese punto fijo. |
| `text` | `start`, `end`, `text`, `at?` (`top`/`center`/`bottom`), `size?` (0,5–3), `color?` (`#rrggbb`), `boxed?`, `bold?`, `caption?` | Texto o título por tiempo. |
| `speed` | `start`, `end`, `rate` (1,25–16) | Tramo más rápido. |
| `blur` | `start`, `end`, `rect:[x,y,w,h]`, `pixelate?` | Tapa una zona (datos privados). |
| `update` | `kind` (`cut`/`zoom`/`text`/`speed`/`blur`), `index`, `set:{…}` | Cambia campos de un bloque ya existente (los mismos nombres de arriba). |
| `remove` | `kind`, `index` | Borra un bloque. |
| `remove_all` | `kind` | Borra todos los de ese tipo (`zoom` con `auto_only: true` borra solo los automáticos). |
| `autofocus` | — | Propone zooms según clics y escritura (reemplaza los automáticos anteriores). |
| `speed_up_silences` | `min_len?` (s) | Acelera los tramos sin voz ni movimiento. |
| `cut_silences` | `min_len?` (s) | Corta esos tramos. |
| `camera_style` | `style` | Preajuste de cámara (los nombres salen de `state.camera.styles`). |
| `set` | `look?`, `cursor?`, `camera_opts?`, `motion?`, `export?` | Combina esos valores con los del proyecto (los números se acotan solos). Los nombres son los de `state`. |
| `generate_captions` | — | Subtítulos automáticos (lanza la transcripción si falta). |
| `seek` / `play` / `pause` | `time?` | Mueve o reproduce el cabezal (no cuenta como edición). |
| `undo` / `redo` | — | Deshace o rehace un paso (cuenta como un paso). |

Reglas de validación: tiempos dentro de `0..duración`, `start < end`, bloques de velocidad sin pisarse, textos y zooms con largo mínimo. Los `index` de `update` y `remove` son los que muestra `state`; dentro de un mismo lote, cada operación ve el resultado de las anteriores.

Ejemplos:

```json
{"ops": [
  {"op": "cut", "start": 12.0, "end": 15.5},
  {"op": "zoom", "start": 20.0, "end": 26.0, "scale": 2.5, "x": 0.3, "y": 0.4},
  {"op": "text", "start": 0.5, "end": 4.0, "text": "Así se instala", "at": "bottom", "boxed": true}
], "label": "Limpiar intro"}
```

```json
{"ops": [
  {"op": "update", "kind": "zoom", "index": 0, "set": {"scale": 3}},
  {"op": "remove", "kind": "text", "index": 1},
  {"op": "cut_silences", "min_len": 1.5}
]}
```
