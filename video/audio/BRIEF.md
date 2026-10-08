# Encargo: música frenética para un video de 30 s (síntesis por código)

Eres un productor/programador de audio. Debes **componer y sintetizar con código** una pista electrónica frenética para un video de producto (herramienta de escritorio "Atic"). No hay samples ni internet: todo se genera con **Python 3.11 + numpy** (ya instalados; no instales nada). Escribe SOLO dentro de esta carpeta (el directorio actual). No toques nada fuera.

## Entregables (todos en esta carpeta)

1. `build_track.py`: script reproducible (semilla fija) que genera todo lo demás.
2. `track.wav`: pista final estéreo, 44100 Hz, 16 bit, duración EXACTA **28.8 s** (1 270 080 muestras). Pico máximo ≈ -1 dBFS, sin clipping, empieza y termina limpia (fade de 30 ms al final si hace falta; la cola de reverb debe apagarse a cero en 28.8 s).
3. `beats.json`: `{ "bpm":150, "beatSec":0.4, "barSec":1.6, "durationSec":28.8, "bars":[...18 objetos {bar, startSec, section, energy0to10}...], "kicks":[tiempos en s], "snares":[...], "hits":[...golpes/impactos especiales con {t, kind}] }`. Es lo que usará el editor de video para sincronizar cortes: los tiempos deben ser EXACTOS a lo que suena.
4. SFX sueltos (WAV mono/estéreo 44.1 kHz, sin música, cola corta): `sfx_whoosh.wav` (0.5 s, barrido de ruido filtrado ascendente), `sfx_hit.wav` (impacto grave 0.9 s), `sfx_click.wav` (clic de tecla mecánica 0.08 s), `sfx_pop.wav` (0.15 s), `sfx_riser.wav` (1.6 s, sube de tono/brillo), `sfx_swoosh_down.wav` (0.4 s, descendente).

## Música: 150 BPM, 18 compases de 4/4 (compás = 1.6 s), tonalidad Fa# menor (o similar oscura)

Estilo: electro / techno melódico / "phonk-drum" agresivo, actual, muy rítmico, para redes (Reels/TikTok). Fuerte, limpio, con mucho punch en los graves y sidechain (bombeo) del bajo y los acordes contra el bombo. Sin voces.

Mapa de compases (respétalo, cada corte de video caerá en tiempo fuerte):

| Compás | Segundos | Sección | Qué suena |
|---|---|---|---|
| 1 | 0.0–1.6 | HOOK | Golpe seco de sub/bajo + bombo en t=0.0 (impacto grave con cola corta), silencio casi total después; solo un riser de ruido agudo que empieza a subir al final del compás. |
| 2 | 1.6–3.2 | BUILD | Riser creciente (ruido + tono), hi-hats en corcheas que pasan a semicorcheas en la 2ª mitad, redoble de caja en el último tiempo (semicorcheas crecientes). Pico de tensión. |
| 3–4 | 3.2–6.4 | DROP A | Cae el beat en t=3.2 con un impacto fuerte (crash + sub). Bombo four-on-the-floor, palmas/caja en 2 y 4, hi-hat abierto a contratiempo, bajo de corcheas con filtro, sidechain marcado. |
| 5–6 | 6.4–9.6 | DROP B | Suma arpegio "pluck" de semicorcheas (escala menor), percusión extra (shaker/rim). |
| 7–8 | 9.6–12.8 | DROP C | Suma acordes stab sincopados y un lead corto; en el compás 8 sube un barrido de filtro (sweep) hacia el compás 9. |
| 9–10 | 12.8–16.0 | DROP D | Todo activo, variaciones de caja (fills de 1/16 al final de cada compás), el arpegio cambia de nota. |
| 11 | 16.0–17.6 | LIFT | Sube tensión: redoble de caja en semicorcheas + riser, el bombo sigue; termina en una micro-pausa de 1/16 antes del compás 12. |
| 12–13 | 17.6–20.8 | CLÍMAX | Crash en t=17.6. Máxima densidad: bombo, caja doble, hi-hats 16ths, bajo, arpegio, lead brillante, acordes. Es el momento más fuerte de la pista. |
| 14–16 | 20.8–25.6 | BREAKDOWN | Cae el bombo. Quedan bajo largo/sub pulsante, pad oscuro, algo de textura; energía baja pero con tensión. En el último tiempo del compás 16 un riser corto y **silencio de 1/8 de compás (0.2 s)** justo antes del compás 17. |
| 17–18 | 25.6–28.8 | FINAL | Impacto final enorme en t=25.6 (bombo + sub + crash) con reverb larga; en el compás 18 la cola decae hasta apagarse en 28.8 s exactos. |

## Requisitos técnicos

- Sintetiza todo tú mismo: bombo (seno con barrido de pitch + click), sub, bajo (saw/cuadrada + lowpass con envolvente), caja/palmas (ruido + tono), hi-hats (ruido filtrado), pluck del arpegio, stabs, lead, pad, risers, crash (ruido largo filtrado). Usa un pequeño reverb/delay hecho con convolución de ruido decaído o líneas de retardo.
- Sidechain: multiplica bajo/pad/acordes por una envolvente que se cierra en cada bombo.
- Un limitador suave (tanh o soft-clip) en la salida maestra y normalización de pico a -1 dBFS.
- Determinista (semilla fija). Tiempos de eventos alineados a la cuadrícula: beat = 0.4 s, semicorchea = 0.1 s.

## Verificación obligatoria antes de terminar

1. Ejecuta `python build_track.py` sin errores.
2. Comprueba con Python (módulo `wave`) que `track.wav` tiene 44100 Hz, 2 canales, 16 bit y 1 270 080 muestras.
3. Imprime la energía RMS (dBFS) por compás y confirma que la curva coincide con el mapa (compases 12–13 los más fuertes, 14–16 más bajos, 1 casi vacío salvo el golpe inicial).
4. Confirma que `beats.json` es JSON válido y que `kicks` contiene 4 bombos por compás en los compases 3–13 y 17 (+ el de t=0).
5. Al final, escribe un resumen corto (≤10 líneas) con lo que hiciste y cualquier limitación.
