# Historia del video nuevo: «Un bug en producción, sin salir de lo que hacías»

Escritorio simulado (monitor vertical 500×660 lógicos, los mismos de `Screen`) con programas genéricos (editor, navegador, chat, diseño). Cada herramienta de Atic resuelve un paso de la historia. Todo el contenido es inventado pero coherente entre escenas. Las escenas reutilizan la **UI real de Atic** ya construida en `video/src/lib/*` (Notch, Float, Launcher, Clipboard, TextosPanel, PizarraBar/Marks, ColorLoupe, Flip*, Agentes*), pero con **datos nuevos**: NO edites archivos existentes de `video/src/lib`, `video/src/scenes`, `video/src/short` ni `video/src/desk`; copia y adapta a archivos nuevos en `video/src/story/`.

## Guion de la historia (orden en el video corto)

1. **Algo se rompió** — el editor muestra la prueba fallando en la terminal; el notch nace. *(lo hago yo)*
2. **Todo en una pill** — la tira se abre y recorre las herramientas. *(yo)*
3. **Clipboard** — el usuario copia el error de la terminal (Ctrl+C sobre el texto); abre el historial (Ctrl+Shift+V) y la copia aparece arriba; se filtra con «error» y se fija.
4. **Pizarra** — en el navegador el tiempo de respuesta se dispara; Ctrl+Shift+X congela la pantalla; se marca con un círculo alrededor del pico y una flecha, y el texto «¿Desde cuándo?».
5. **Color** — en el programa de diseño, Ctrl+Shift+C: la lupa recorre la paleta y copia el hex del Primario `#FF6B3D`.
6. **Textos** — en el chat, Ctrl+Shift+S: se elige el texto guardado «Respuesta incidente» y se pega en el cuadro de respuesta.
7. **Flipboard** (2 compases) — Ctrl+Shift+B da vuelta la ventana del navegador; en el reverso se anota «Causa: pool de 5 conexiones» / «Fix: max 20 + timeout 5 s» a mano y se inserta desde el cajón (Clip) el error copiado.
8. **Agentes** (2 compases) — Ctrl+Shift+A: se abre Claude Code; se le escribe «Sube el pool de conexiones y arregla el timeout»; pide permiso desde la pill; Aprobar; la prueba pasa en la terminal («12 passed») y la pill dice «Listo».
9. **Apps** — Ctrl+Espacio: se escribe «cor» y aparece «Correo» para abrir el correo del incidente.
10. montaje, remate, cierre *(yo)*.

## Datos compartidos (úsalos tal cual)

- Error: `Error: connect ETIMEDOUT 10.0.4.12:5432` / `at pool.query (src/db.ts:88:14)` (ya están en `ERROR_LINES` de `video/src/desk/desk.tsx`).
- Pico del gráfico: 14:02, p95 980 ms (geometría en `chartGeometry` de `desk.tsx`).
- Marca: Primario `#FF6B3D`, Acento `#2F80ED`, Éxito `#27AE60`, Aviso `#F2C94C`, Tinta `#1C2430` (`SWATCHES`).
- Texto guardado (Textos): nombre «Respuesta incidente», alias «incidente», cuerpo: «Ya lo detectamos: es un problema en la base de datos y lo estamos corrigiendo. Te aviso en 10 minutos.» (otros textos guardados, inventa 3–4 más coherentes: firma, link de estado, dirección…).
- Prompt del agente: «Sube el pool de conexiones y arregla el timeout». Salida del agente: lee `src/db.ts`, edita `max: 5 → 20` y `connectionTimeoutMillis: 2000 → 5000` (diff), corre `npm test` (pide permiso de Bash) y termina con `12 passed`.
- Historial del clipboard (más nuevo arriba): el error copiado, `https://panel.tienda.cl/rendimiento`, `10.0.4.12`, una captura del gráfico (miniatura 44×34), `SELECT * FROM orders WHERE user = $1`, `#FF6B3D`.
- Launcher: recientes = Editor / Navegador / Mensajes; al escribir «cor» → «Correo» (Aplicación) primero; favoritos: iconos de Clipboard, Capturas, Pizarra (o los que uses).

## Estructura común de cada momento

- Archivo en `video/src/story/<Nombre>Moment.tsx` que exporta `<Nombre>Moment` (componente de escena) y, además, `<NOMBRE>_SEGMENTS: [number, number][]` (tramos [desde,hasta] en ms de la escena original que, reproducidos seguidos, cuentan la escena en **1.6 s** (2 compases = 3.2 s para Flipboard y Agentes; al velocidad ≈1.1–1.6×) y `<NOMBRE>_HERO_MS: number` (ms del mejor cuadro para el montaje).
- Patrón (ver `video/src/scenes/ClipboardScene.tsx` y `video/src/desk/DeskTest.tsx`): `<AbsoluteFill style={{ background: useStageBg() }}> <Caption .../> <Screen wallpaper={false} camera=...> <Desktop> ...ventanas... </Desktop> ...Notch/floats/cursor... </Screen> </AbsoluteFill>`. Usa `useMs()` de `../lib/time` para todo el tiempo (el video corto lo remapea con `TimeRemap`), no `useCurrentFrame`. La pill está pegada al techo (y=0): mantén `fy=0, ay=0` en la cámara.
- El escritorio completo (papel tapiz + barra de tareas) es `Desktop` de `../desk/desk`; las ventanas (`EditorApp`, `BrowserApp`, `ChatApp`, `DesignApp`, `AppWindow`, `TerminalPanel`) también. Puedes añadir ventanas nuevas propias en tu archivo.
- Cada escena tiene su propio cursor (puedes reutilizar `FlipCursor` de `../lib/FlipCursor`) y su cronología con constantes claras al principio.
- Fidelidad de la UI de Atic: mismas medidas, colores y tiempos que las escenas anteriores (fichas en `video/specs/*.md`). Solo cambian los datos y el contexto del escritorio.
- Composiciones de prueba: ya están registradas en `video/src/story/registry.tsx` (`Story-Clipboard`, `Story-Pizarra`, `Story-Color`, `Story-Textos`, `Story-Flipboard`, `Story-Agentes`, `Story-Apps`). Hoy cada `<Nombre>Moment.tsx` es un marcador vacío: SOBRESCRIBE tu archivo. No edites `registry.tsx` ni `Root.tsx`; si tu duración natural cambia, dilo en tu resumen final.
- Verifica con `npx remotion still src/index.ts Story-<Nombre> out/story-<nombre>-<n>.png --frame=<n>` (desde `video/`) y ABRE los PNG con Read. `npx tsc -p .` debe pasar.
