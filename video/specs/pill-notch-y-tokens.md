# Ficha visual: pill en modo notch, tokens, iconos, etiquetas y atajos

Fuente: código del repo Atic (Tauri + Svelte). Todo lo de esta ficha sale del código; donde el código y los `.md` de diseño discrepan, **gana el código** y se marca con ⚠. Lo marcado **(derivado)** lo calculé leyendo CSS/TS, **no** lo medí en la app en ejecución (no había CDP disponible). Lo marcado **(no encontrado)** no lo hallé.

Rutas abreviadas (todas bajo `apps/desktop/src/` salvo indicación):

| Alias | Ruta |
|---|---|
| `PS` | `lib/surfaces/overlay/pill/PillSurface.svelte` |
| `PSTAGE` | `lib/surfaces/overlay/pillStage.ts` |
| `PLAN` | `lib/surfaces/overlay/pill/pillPlan.ts` |
| `APPCSS` | `app.css` |
| `SCALES` | `styles/scales.css` |
| `MARK` | `lib/AticMark.svelte` |
| `SKIN` | `lib/liquid/Skin.svelte` |

---

## 0. Discrepancias y advertencias (leer primero)

| # | Qué | Doc dice | Código dice | Cita |
|---|---|---|---|---|
| 1 | Duración de abrir/cerrar la isla (`--island-open-dur`) | 190 ms (`docs/DISENO_PILL.md:181`) | **240 ms** | `APPCSS:208`, `lib/motion.ts:MOTION_FALLBACK islandOpen=240` |
| 2 | Logo de aviso `islandCueMark` | 14 px (`DISENO_PILL.md:120,272`) | **18 px** | `PSTAGE:68` |
| 3 | Grosor de la pestaña cerrada | 40 px (`islandThick`) | La caja de contenido es 124×40, pero la ventana suma `pad` 4 y la silueta medida (`.p-island-skin`) es **124×44** en el canto superior **(derivado)**. Ver 2.1. | `PSTAGE:40,17`, `PLAN:297-301`, `PS:4864,6377-6391` |
| 4 | Tokens del acto B "emerge" (`--morph-*-dur`, `--flight-dur`) | 150 ms (`Features/pill-liquid-emerge.md:230-243`) | **110 / 100 / 110 ms** | `APPCSS:126-146` |
| 5 | "El ritmo en dos tramos de 620 ms" (`MORPH_SPLIT 0.48`, `cubic-bezier(0.45,0,0.2,1)`) | — | **No es de la pill.** Solo existe en `lib/features/agents/BoardList.svelte:101-102` (panel de la pizarra) y `LauncherFloat.svelte:120` (`RECENTS_EASE`). La isla del notch usa **un solo tramo de 240 ms** (2.6). | grep `MORPH_SPLIT` |
| 6 | Gap entre botones de tira | comentario CSS dice "6 px" | **2 px** | comentario `PS:6668`, valor `PSTAGE:86` |
| 7 | `docs/assets/atic-header-notch.png` (banner) | — | Es arte de marketing **antiguo** (marca al centro, borde "ondulado" entre celdas, sin la marca como primera celda). No usarlo como referencia de forma. | inspección visual |
| 8 | Descripción corta de Agentes | `tools.ts:103`: "Consola con interfaz" | La UI usa `es.ts`: **"Terminales con agentes"** | `lib/core/tools.ts:100-108` vs `lib/core/i18n/es.ts:58-63` |

**Desmentido (importante para el video):** en el modo acoplado (`surface === "edge"`) la silueta que se dibuja es **solo el rectángulo de la isla** (más un pequeño "hombro" contra el canto). Las herramientas **no** son gotas fundidas: los comentarios de `PS:6658-6681` sobre "gotas que se funden" son de un diseño anterior. En el código actual, `skinShapes` en `edge` publica solo `island` + paredes + avisos (`PS:1270-1304`); los botones de herramienta solo cambian opacidad/transform del glifo. Y el render usa `SKIN_BLEND = 0` (uniones duras, `lib/surfaces/overlay/OverlaySurface.svelte:75`).

---

## 1. Tokens y tema

### 1.1 Tema por defecto

- Config por defecto `ui_theme: "system"` → sigue `prefers-color-scheme` del SO (`crates/core/src/config.rs:491`, saneado en `:967-970`; `lib/theme.ts:91-94`).
- Con el atributo `data-theme` ausente manda el **oscuro** ("la paleta primaria", `styles/palettes/atic/dark.css:1-15`, selector `:root, [data-theme="dark"]`). El claro se deriva (`light.css:1-10`).
- Temas existentes: `system, light, sepia, mist, graphite, midnight, dark, claude, claude-dark, custom` (`lib/theme.ts:26-37`). Para el video: **oscuro** como principal, **claro** como variante.
- La pill flota sobre el escritorio; el overlay es transparente (`APPCSS:36-44`: `html, body { background: transparent }`).

### 1.2 Paleta resuelta (hex/rgba reales)

`dark.css:23-65`, `light.css:12-44`. `--skin` es el color de la silueta de la pill (opaco, mate, sin borde ni degradé).

| Token | Oscuro | Claro | Uso en la pill |
|---|---|---|---|
| `--skin` | `#1a1a18` | `#f7f7f2` | **Relleno de la isla/notch** (`dark.css:65`, `light.css:42`) |
| `--bg` | `#121211` | `#ecece6` | fondo app |
| `--surface` | `#1a1a18` | `#f7f7f2` | tooltip (mezcla 96 % con bg) |
| `--surface-2` | `#1e1e1b` | `#efefe8` | |
| `--elevated` | `#262622` | `#ffffff` | |
| `--text` | `#f0f0ea` | `#171714` | **Marca AticMark**, título de cara, glifo en hover |
| `--muted` | `#a8a89e` | `#5f5f58` | **Icono de herramienta en reposo**, descripción de cara |
| `--faint` | `#8f8f86` | `#6b6b63` | texto terciario |
| `--line` | `rgb(240 240 234 / 10%)` | `rgb(28 28 24 / 11%)` | hairline (la pill no lo usa) |
| `--line-strong` | `rgb(240 240 234 / 18%)` | `rgb(28 28 24 / 20%)` | |
| `--accent` | `#e8e8e0` | `#1a1a17` | monocromo: acento = tinta invertida |
| `--on-accent` | `#121211` | `#f7f7f2` | |
| `--rec` | `#e85a52` | `#d6453d` | grabación, "rechazar", error |
| `--ok` | `#6faf88` | `#3f7355` | "aprobar", pegado, update listo |
| `--warn` | `#d4a84b` | `#946718` | transcribiendo |
| `--danger` | `#e85a52` | `#d6453d` | |
| `--info` | `#8fa9b8` | `#47708a` | icono de update |
| `--mic` / `--sys` | `#6faf88` / `#8fa9b8` | `#3f7355` / `#47708a` | pistas de audio |
| `--agent-accent` | `#a8a89e` | `#5f5f58` | |
| `--rb-record` (legado, lo usa el cuadrado de grabar de la marca) | `#e85a52` (`APPCSS:344`) | `#d6453d` (`APPCSS:94`) | `MARK:550` |

Estados sobre la pill se hacen con `color-mix(in sRGB, var(--rec) 18%, transparent)` etc. (ej. `PS:7699-7712`).

Otros temas por si acaso (`--skin`): graphite `#323230`, midnight `#222732`, sepia `#e9e1d2`, mist `#e3e7ea`, claude `#f4f3ee`, claude-dark `#2a2825` (`styles/palettes/atic/*.css`).

### 1.3 Tipografía

- Familias (`SCALES:30-33`, y legado `APPCSS:77-79`):
  - `--font-sans`: `"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif`
  - `--font-display`: `"Avenir Next", "Aptos Display", "Helvetica Neue", sans-serif`
  - `--font-mono`: `"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace`
  - **No se cargan web-fonts** (no hay `@font-face`; `app.html`/`static/` no traen fuentes). En Windows resuelve a Aptos si está instalada (Office/Win11), si no a Segoe UI Variable. Para el video conviene Aptos o Segoe UI Variable.
- Escala (`SCALES:38-54`): micro 10 px (lh 1.2, ls .1em), xs 11 px (1.3), sm 12 px (1.45), base 13 px (1.5, cuerpo real), md 14 px, lg 17 px (ls −.02em), xl 22 px (ls −.025em).
- Pesos: 400 / 500 / 600 / **650** (bold) (`SCALES:57-60`). Tracking caps `0.1em` (`SCALES:65`).
- En `.atic-root`: `-webkit-font-smoothing: antialiased`, `text-rendering: optimizeLegibility` (`styles/base.css`).
- Texto dentro de la pill (todos `font-family: var(--font-sans)`):

| Elemento | Tamaño | Peso | Otros | Cita |
|---|---|---|---|---|
| Título de cara "El agente espera tu permiso" | 0.75rem = 12 px | 600 | lh 1.2, color `--text` | `PS:7640-7650` |
| Descripción de cara | 12 px | 400 | lh 1.2, `--muted`, `strong` en `--text` 600 | `PS:7652-7669` |
| Botón Rechazar/Aprobar | 0.6875rem = 11 px | 600 | MAYÚSCULAS, ls .06em, alto 2rem=32 px, radio 999 | `PS:7674-7690` |
| Etiqueta estado dictado | 11 px | 500 | MAYÚSCULAS, ls .1em | `PS:7540-7550` |
| Fila de agente en cara live | 11 px | 600 | lh 1 | `PS:7409-7419` |
| Pestañas de la cara Textos | 11 px | 600 | | `PS:7605-7620` |
| Tooltip | 0.72rem ≈ 11.5 px | 500 | lh 1.3 | `lib/surfaces/overlay/TipHost.svelte:78-100` |
| Contador de aviso (`is-count`) | 0.75rem = 12 px | 650 | tabular-nums | `PS:7912-7918` |

### 1.4 Radios, sombras, espaciado

- Radios (`SCALES:20-24`): xs 5 px, sm 8, md 14, lg 20, pill 999. La isla **no** usa estos: su radio sale de `islandNotchRadius` (2.3).
- Sombras (`SCALES:71-76`): `--shadow-card: 0 1px 2px rgb(0 0 0 / 20%)`, `--shadow-pop: 0 8px 24px rgb(0 0 0 / 32%)`, `--shadow-float: 0 24px 70px rgb(0 0 0 / 45%)`, **`--shadow-goo: 0 10px 22px rgb(0 0 0 / 38%)`** ← la de la pill. Se aplica como `filter: drop-shadow(0 10px 22px rgb(0 0 0 / 38%))` sobre el path de la silueta (no `box-shadow`), `SKIN` `style:filter="drop-shadow({shadow})"`.
- Espaciado base Tailwind `--spacing: 4px` (`SCALES:16`).
- Botones de la tira: **sin fondo, sin borde** en reposo ni en hover (solo cambian color y escala).

### 1.5 Movimiento (valores reales de `APPCSS:75-232`; `styles/motion.css` repite varios y `APPCSS` va después)

| Token | Valor | Cita |
|---|---|---|
| `--duration-stagger` | 20 ms | `APPCSS:159` |
| `--duration-micro` | 40 ms | `:160` |
| `--duration-quick` | 75 ms | `:161` |
| `--duration-fast` | 125 ms | `:162` |
| `--duration-medium` | 150 ms | `:163` |
| `--duration-slow` | 200 ms | `:164` |
| `--duration-very-slow` | 250 ms | `:170` |
| `--morph-open-dur` / `--morph-close-dur` | 110 / 100 ms | `:126-127` |
| `--morph-fade-dur` | 80 ms | `:130` |
| `--morph-quick-dur` | 60 ms | `:132` |
| `--morph-stagger` | 16 ms | `:138` |
| `--morph-scale` / `--morph-blur` | 0.97 / 2 px | `:139-140` |
| `--panel-dur` / `--flight-dur` | 110 / 110 ms | `:143,146` |
| `--float-open-dur` / `--float-close-dur` | 150 / 100 ms; `--float-scale` 0.55; `--float-travel` 18 px | `:172-178` |
| `--launcher-bar-open-dur` / `-separate-dur` / `-fav-stagger` | 100 / 90 / 90 ms | `:183-185` |
| **`--island-open-dur`** | **240 ms** | `:208` |
| **`--island-stagger`** | **26 ms** | `:197` |
| `--island-rise` | 7 px (definido; no lo encontré usado en la tira actual) | `:199` |
| `--island-shut-scale` / `--island-shut-squeeze` | 0.91 / 0.22 | `:230-231` |
| `--duration-spin` | 800 ms | `styles/motion.css` |
| `--goo-grow` | 1.68 px | `:252` |

Curvas:

| Nombre | Valor | Uso |
|---|---|---|
| `--ease-island` | `cubic-bezier(0.33, 1.38, 0.46, 1)` | **caja de la isla** (width/height/left/top). Overshoot pico +4.8 % en t≈0.55 (`APPCSS:213`) |
| `--ease-liquid` | `cubic-bezier(0.5, 0, 0.2, 1)` | glifos de la tira, fade de la marca (`:220`) |
| `--ease-smooth-out` = `--morph-ease` = `--float-ease` = `--ease-morph` = `--ease-calm` | `cubic-bezier(0.22, 1, 0.36, 1)` | hover/press de botones, floats (`:128,232`, `SCALES:82-83`) |
| `opacityFade` | ease-in-out cuadrática (`t<.5 ? 2t² : 1−(−2t+2)²/2`) | entrada/salida de caras, 80 ms (`lib/motion.ts:133-147`) |
| `emerge` | `cubicOut` (Svelte) + `translateY(u*8px) scale(0.97+0.03t) blur(u*2px)`; abre 200 ms, cierra 125 ms | (`lib/motion.ts:158-181`) |
| `tabPanel` | ease-in-out, 125 ms, `translateY(u*4px)` | cambio de pestaña en caras |

Valores de la curva `island` a t=0,.1,…,1: `0, .386, .690, .895, 1.004, 1.044, 1.046, 1.032, 1.016, 1.004, 1` (calculado). `liquid`: `0, .016, .082, .250, .535, .747, .866, .935, .975, .994, 1`.

`prefers-reduced-motion`: `holdMotionClock`/`opacityFade`/`emerge` devuelven duración 0; CSS global fuerza `animation-duration: .01ms` (`APPCSS:1485-1488`). Reloj compartido `--clock` a 24 Hz para animaciones en bucle (`lib/motion.ts:191-262`).

### 1.6 Bloque CSS listo

```css
:root {
  /* oscuro (paleta primaria) */
  --skin:#1a1a18; --text:#f0f0ea; --muted:#a8a89e; --faint:#8f8f86;
  --rec:#e85a52; --ok:#6faf88; --warn:#d4a84b; --info:#8fa9b8;
  --shadow-goo:0 10px 22px rgb(0 0 0 / 38%);
  --font-sans:"Aptos","Avenir Next","Helvetica Neue","Segoe UI Variable",sans-serif;
  --ease-island:cubic-bezier(0.33,1.38,0.46,1);
  --ease-liquid:cubic-bezier(0.5,0,0.2,1);
  --ease-smooth-out:cubic-bezier(0.22,1,0.36,1);
  --island-open-dur:240ms; --island-stagger:26ms;
  --duration-quick:75ms; --duration-fast:125ms;
}
[data-theme="light"] {
  --skin:#f7f7f2; --text:#171714; --muted:#5f5f58; --faint:#6b6b63;
  --rec:#d6453d; --ok:#3f7355; --warn:#946718; --info:#47708a;
}
```

---

## 2. Pill en modo notch (acoplada al borde superior)

Refs de diseño: `docs/DISENO_PILL.md`, `Features/pill-shell.md`, `Features/pill-liquid-emerge.md`, `Features/liquid.md` (todos leídos). "Notch" = `surface === "edge"` con `dock.edge === "top"` (`PLAN:55,89`). Por defecto la pill vuelve "arriba al centro" (`lib/core/i18n/es.ts:homeHint`, `settings.pill.homeHint`); en Windows el borde es el techo real del monitor (sin barra arriba). Se acopla al soltarla a ≤ 28 px del borde (`edgeDock.ts:29`), se suelta a > 64 px (`:37`).

### 2.1 Modelo geométrico (cómo se calcula lo que se ve)

1. `contentFor` devuelve el tamaño de **contenido** (`PLAN:144-307`). `windowFor` suma `pad = 4` por lado → ventana = contenido + 8 (`PSTAGE:182-187`).
2. `.p-root` es esa ventana, `padding: 4px`, `overflow: hidden` (`PS:6130-6139`). `.p-island` es `position:absolute; inset: 4px` pero en canto `top` se pisa con `top: 0` (`PS:6377-6391`). Es decir la isla ocupa **ancho = contenido, alto = contenido + 4** (el pad inferior; el pad superior queda "dentro del techo").
3. La silueta se toma midiendo `<i class="p-island-skin">` (`PS:4902-4907`, `tracker.track("island")`) cada cuadro, así que **sigue la transición CSS de tamaño**.
4. `notchShape(box, "top", r)` = caja redondeada con `y − r` y alto `+ r` (el redondeo superior queda fuera de pantalla), radio `r = islandNotchRadius(box)` = `min(w,h)/2` si eso ≤ 30, si no `islandClipR = 22` (`PLAN:369-372`, `lib/liquid/geometry.ts:48-60`). O sea: **solo las esquinas inferiores son redondas**; los lados bajan rectos desde el techo.
5. Piso: `clampDockedTabRect` impide que el rebote deje la pestaña cerrada con menos de `faceTabH` (40) de grosor (`geometry.ts:72-`, `PS:1281-1283`). `min-width/min-height` inline en el root = ventana de la pestaña cerrada (`PS:4864-4865`).

### 2.2 Dimensiones exactas (px) — canto superior, Windows, sin avisos

`PILL` en `PSTAGE:15-177`. "Silueta" = rectángulo que realmente pinta el skin **(derivado, ver 2.1)**.

| Estado | Contenido (`contentFor`) | Ventana (+8) | Silueta visible (ancho × alto) | Radio esquinas inferiores | Cita |
|---|---|---|---|---|---|
| **Cerrada (idle)** | 124 × 40 (`islandLong` × `islandThick`) | 132 × 48 | **124 × 44** | **22** (`min(124,44)/2`) | `PLAN:297-301` |
| Cerrada con aviso (agente/update) | `islandCueLong(n)` × 42 (`islandCueThick`) | +8 | ancho × 46 | ≈23 | `PLAN:39-45,209-211` |
| **Abierta (tira)** con N celdas | (`46N − 2`) × 44 (`islandTool`) | +8 | (`46N−2`) × **48** | **24** | `PLAN:23-26,275-296` |
| Cara agente (permiso) | max(largo,280) × (40+104) | +8 | **280 × 148** | 22 | `PLAN:214-216` |
| Cara dictado | max(largo,232) × (40+72) | +8 | 232 × 116 | 22 | `PLAN:219-223` |
| Cara live (avisos ambientales) | max(largo,232) × (40 + 32·filas) | +8 | 232 × (44+32·filas) | 22 | `PLAN:226-232` |
| Cara Clipboard / Textos | max(largo,280) × (40+252) | +8 | **280 × 296** | 22 | `PLAN:264-268` |
| Cara Sistema | max(largo,300) × (40+430) | +8 | 300 × 474 | 22 | `PLAN:252-257` |
| Cara Personalizar | max(largo,360) × (40+290) | +8 | 360 × 334 | 22 | `PLAN:258-263` |
| Cara Agentes (lanzador / consola / selector de carpetas) | 400×(40+188) / 440×(40+496) / 680×(40+620) | +8 | 400×232 / 440×540 / 680×664 | 22 | `PLAN:233-251` |

Tira abierta por número de celdas N (marca + herramientas + Ventana): `long = N·44 + (N−1)·2`.

| N | 6 | 7 | 8 | 9 | **10 (default)** |
|---|---|---|---|---|---|
| long / ancho silueta | 274 | 320 | 366 | 412 | **458** |
| ventana | 282×52 | 328×52 | 374×52 | 420×52 | 466×52 |

Con la config por defecto (`pill_tools = []` ⇒ todas, `crates/core/src/config.rs:496`, `pillLayout` en `lib/core/pillTools.ts`) la tira tiene **N = 10**: marca + 8 herramientas + Ventana (`windowOnFirst: true`, sin "Más" porque `more` está vacío; `pillTools.ts:185-198`, `PS:1036-1073`, `islandSlots` `PS:2168-2175`). Si suena música suma una celda (Play/Pause) y si hay update otra.

Otras dimensiones (`PSTAGE`): `bar` 52 (disco flotante en reposo), `wheel` 252 (escenario de la rueda; disco 232 + 10 por lado), `panelW/H` 312/332, `islandMark` 32, `islandCueBtn` 26, `islandCueMark` 18 ⚠, `islandCueMsgW` 96, `islandLyricW` 200, `islandLyricHangW/H` 340/80, `islandGap` 2, `islandClipR` 22, `recDrop` 36 / `recDropGap` 8 / `recDropNeck` 10 (solo en la rueda), `wheelLiveHang` 28, `agentStackRow` 28.

Posición: centrada horizontalmente en el monitor (pivote `dockTop` conserva `y` y recentra `x`, `pillCssStage.ts:285-288`, `PLAN:750-779`), pegada al techo (`y = 0` del área útil).

### 2.3 Silueta como SVG (listo para React)

Coordenadas de pantalla: `cx` = centro X, y=0 es el techo.

```ts
// Cuerpo del notch. W ancho visible, H alto visible, r radio de las esquinas inferiores.
export const notchPath = (cx: number, W: number, H: number, r: number) => {
  const l = cx - W / 2, rt = cx + W / 2;
  return `M${l} 0V${H - r}A${r} ${r} 0 0 0 ${l + r} ${H}H${rt - r}A${r} ${r} 0 0 0 ${rt} ${H - r}V0Z`;
};
// "Hombros" (menisco contra el techo): pared de 40 px de alto, 12 px más ancha por lado,
// solo se ven los 8 px que se meten bajo el techo; radio 20 (pillShape de alto 40).
export const shoulderPath = (cx: number, W: number) => {
  const l = cx - W / 2 - 12, rt = cx + W / 2 + 12;
  return `M${l + 4} 0A20 20 0 0 0 ${l + 20} 8H${rt - 20}A20 20 0 0 0 ${rt - 4} 0Z`;
};
// Cerrada:  notchPath(cx,124,44,22) + shoulderPath(cx,124)
// Abierta:  notchPath(cx,458,48,24) + shoulderPath(cx,458)
// Caras:    notchPath(cx,280,148,22) (permiso) ...
```

Render: `fill = var(--skin)`, `stroke = var(--skin)`, `stroke-width 1.25`, `stroke-linejoin/linecap round`, `fill-rule evenodd`, luego `filter: drop-shadow(0 10px 22px rgb(0 0 0 / 38%))` (`SKIN:94-136`). El stroke "no es un borde de contraste, redondea el aliasing de marching squares (celda 6 px)". **Sin filete blanco, sin degradé: "el notch de Apple es un recorte mate"** (comentario `SKIN:~100`).

Origen de los hombros: `edgeWallRect("top")` → `x = pill.x − 12`, `y = work.y − 40 + 8 = −32`, `w = pill.w + 24`, `h = 40` (`edgeDock.ts:560-561,578,649-690`), con `flare = MENISCUS_FLARE = 12` (`edgeDock.ts:578`, usado en `PS:1289-1296`). Como el render tiene `SKIN_BLEND = 0`, es una unión dura; el contorno real pasa por marching squares (`CELL 6`, `SMOOTH 4`, `lib/liquid/constants.ts`) y suaviza levemente la esquina cóncava. Es un detalle de ~8 × 6 px por lado; opcional en el video.

### 2.4 Estado idle (pestaña cerrada)

- Una sola cosa dentro: **AticMark de 32 px**, `strokeWidth 1.6`, `alive` (cara viva), color `--text` (`PS:4914-4929`).
- `.p-island-along` = banda de altura `--face-tab-h` (40; 42 con aviso) pegada al techo (`PS:6483-6506`), centro de la marca en **y = 20** (desde el techo) y x = centro. Sobran 4 px de silueta bajo la banda.
- Fondo `--skin`, sombra goo. Nada de texto ni etiquetas.
- Con avisos (agente, update, música, volumen, sistema): la pestaña se alarga a lo largo del borde; botón de aviso 26 px al lado de la marca, gap 2–3 px; `islandCueLong = max(124, 32+2+cues+msgW+12)` (`PLAN:39-45`). Estados del chip de agente: `working` late (opacidad 0.55→1 con `--clock`, 1.8 s), `waiting` fondo `--rec` 16 %, `ready` fondo `--ok` 14 % (`PS:7846-7866`). Ver 2.10.
- Marca clicable pero **en reposo el clic no hace nada** (`DISENO_PILL.md:93`, `PS:6549-6553`); grabando/dictando la marca es el stop.
- La marca "mira" al cursor del escritorio, parpadea y se mece (2.7).

### 2.5 Estado expandido (tira de herramientas) al pasar el mouse

- Disparo: Rust sondea el cursor cada `ISLAND_HOVER_MS = 100` ms (`PS:3060`). Con update pendiente espera 180 ms (`PLAN:556`). Al salir, "linger" de **400 ms** (700 ms en la página "Más") antes de cerrar (`PLAN:546-548`).
- Layout (fila centrada, gap 2, sin padding lateral; `PS:6642-6647`): celda 0 = **marca** (AticMark 32, `strokeWidth 1.6`, `alive`, color `--text`), luego una celda por herramienta, luego Ventana. Cada celda: botón **44 × 44**, `border-radius 999`, fondo transparente, `color: var(--muted)`, icono **22 px, trazo 1.6** (`PS:5145`, `PS:6682-6716`). Vertical: la fila (44) queda centrada en la silueta de 48 → 2 px arriba y abajo.
- Orden por defecto (`lib/core/tools.ts:63-140`, `WHEEL_TOOLS` = sin `shortcutOnly`): **marca · Reuniones · Clipboard · Textos · Agentes · Sistema · Capturas · Pizarra · Color · Ventana**. (Dictado y Apps/launcher son solo atajo: no están.)
- Hover de una celda: `color: var(--text)`, `transform: scale(1.14)`, transición de transform 240 ms `--ease-liquid`, color 75 ms `--ease-smooth-out` (`PS:6738-6746`). No hay fondo ni anillo. `::before` cubre el filete de 2 px entre celdas (`PS:6750`).
- Herramientas con "vistazo" (Agentes, Sistema, Clipboard, Capturas, Color, Textos, música) **no llevan tooltip**: su hover abre un panel que cuelga de la tira y hace crecer la isla hacia adentro (`peekTools.ts:9-19`, `PLAN:288-292`, `.p-island-peek` `PS:6989-7017`); espera 450 ms en frío, 120 ms al cambiar de herramienta, gracia de cierre 400 ms (`toolPeek.svelte.ts:105-115`). Reuniones, Pizarra y Ventana sí usan tooltip `"{label} — {short}"` (`PS:5122-5130`).
- Tooltip (`TipHost.svelte:78-100`): `position fixed`, `border 1px color-mix(var(--line) 80%)`, radio 0.4rem = 6.4 px, padding .26rem .46rem, fondo `color-mix(in sRGB, var(--surface) 96%, var(--bg))`, sombra `0 8px 22px rgb(0 0 0 / 32%)`, texto `--text` 0.72rem/500, fade 125 ms; aparece tras 450 ms (`tip.svelte.ts:67`), 320 ms de "caliente".
- Click en una herramienta: abre su cara/acción (no analizado en detalle); click derecho abre Personalizar (`PS:5140-5143`).

### 2.6 Animación de apertura y cierre (secuencia exacta)

**No hay ritmo en dos tramos aquí** (ver ⚠ 5). Es una transición CSS simple:

1. **Caja** (`.p-root.is-docked`, `PS:6171-6181`): `transition: width, height, left, top` cada una **240 ms `cubic-bezier(0.33,1.38,0.46,1)`**. Como `left` se anima con la misma curva y el pivote centra, la isla crece simétrica desde el centro del techo: de 132×48 (ventana) a 466×52 (N=10). Pico de overshoot +4.8 % del delta a t≈55–60 % (≈ +16 px de ancho). Al cerrar es la transición inversa; el rebote no baja del piso de la pestaña (`min-width/height`).
2. **Marca de la pestaña cerrada** (`.p-island-along`): `opacity 1→0` en 240 ms `--ease-liquid` cuando `islandOpen` (`PS:6483-6494,6544-6547`). Se va mientras las celdas aparecen.
3. **Celdas de la tira** (`.p-island-tool`): reposo = `opacity 0; transform: translate(bunch·0.22, 0) scale(0.91)` donde `bunch = ((N−1)/2 − i)·46 px` (se amontonan hacia el centro); abierta = `opacity 1; transform: none`. `transition: transform, opacity 240 ms --ease-liquid`, con **`transition-delay = s · 26 ms`** y `s = |(N−1)/2 − i|` (distancia al centro; el centro sale primero). Para N=10: delays 117, 91, 65, 39, 13, 13, 39, 65, 91, 117 ms (celdas 0..9). Fin total ≈ 117 + 240 = **357 ms**. El delay también aplica al cerrar (`PS:6682-6721`).
4. Cambio de página "Más": animación `island-tool-swap` (opacidad desde 0, 240 ms, mismos delays).
5. Cara de contenido (permiso, dictado, clipboard…): la caja crece con la misma transición (1) y el contenido entra con `opacityFade` (80 ms ease-in-out, solo opacidad) (`PS:5211-5218` etc.).
6. Otros gestos:
   - Asentarse al acoplar ("gota contra el cristal"): `p-seat-y` 125 ms `--ease-smooth-out`, `scaleY 1→.86 (38 %)→1`, origen `center top` (`PS:6227-6262`).
   - Nacimiento al arrancar: `.is-boot .p-island {opacity 0; scale .55}` → `is-birthing` transiciona opacity/transform 240 ms `--ease-liquid` (`PS:6209-6223`).
   - Vista previa del imán: `scale(1.1, 1.2)`, origen `center top`, transición 150 ms `--ease-smooth-out` (`PS:6288-6291,6412-6421`).
   - Despegue con rebote: `island-detach-y` 360 ms `--ease-liquid`, delay 200 ms, `scaleY: 1 → .9 (30 %) → 1.05 (62 %) → .99 (84 %) → 1` (`PS:6308-6345`).
7. El ritmo "620 ms en dos tramos, sin rebote, simétrico" que el usuario aprobó (memoria del proyecto) **no está en la pill**; si se quiere usar en el video para el notch, habría que decidirlo como licencia creativa (la pill real usa 240 ms con rebote leve).

Receta Remotion (60 fps recomendado; 26 ms = 1.6 fotogramas a 60 fps):

```ts
const ISLAND = { openMs: 240, staggerMs: 26, easeBox: [0.33,1.38,0.46,1], easeGlyph: [0.5,0,0.2,1] };
// caja: interpolate(progress, ..., {easing: Easing.bezier(...ISLAND.easeBox)})
// celda i (N celdas): delay = Math.abs((N-1)/2 - i) * 26 ms
//   opacity = bezier(easeGlyph)(t), translateX = bunch*0.22*(1-t), scale = 0.91 + 0.09*t
// marca cerrada: opacity 1->0 en 240 ms easeGlyph, sin delay
```

### 2.7 AticMark (logo) — SVG completo

Componente: `lib/AticMark.svelte` (cuadrícula 24, el mismo trazo que los iconos). Marcado real (`MARK:465-517`):

```tsx
// Reposo "vivo" (cara con ojos). size=32, strokeWidth=1.6 en la pill; disco flotante 32 con 1.5; por defecto 24 y 1.25.
<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
     strokeWidth={1.6} strokeLinecap="butt" aria-hidden style={{ overflow: 'visible', display: 'block' }}>
  <g /* .am-head */ style={{ transformBox: 'view-box', transformOrigin: '12px 12px',
        transform: `translate(${leanX}px, ${leanY}px) rotate(${rot + sway}deg)` }}>
    <circle cx="12" cy="12" r="5.5" />
    <path d="M17.5 6.5V17.5" />
    {/* ojo izquierdo */}
    <g transform="translate(10.12 11.15)">
      <ellipse cx="0" cy="0" rx="0.88" ry="1.32" fill="currentColor" stroke="none" />
      <path d="M 0.836 1.241 Q 0.312 1.684 -0.062 1.861" fill="none" stroke="currentColor"
            strokeWidth="0.14" strokeLinecap="round" strokeLinejoin="round" />
    </g>
    {/* ojo derecho */}
    <g transform="translate(13.88 11.15)">
      <ellipse cx="0" cy="0" rx="0.88" ry="1.32" fill="currentColor" stroke="none" />
      <path d="M -0.836 1.241 Q -0.312 1.684 0.062 1.861" fill="none" stroke="currentColor"
            strokeWidth="0.14" strokeLinecap="round" strokeLinejoin="round" />
    </g>
  </g>
</svg>
```

Variantes de estado (reemplazan a los ojos, `MARK:496-514`, CSS `MARK:543-591`):

```tsx
{/* grabando: cuadrado rojo */}
<rect x="9.8" y="9.8" width="4.4" height="4.4" rx="1.15" fill="var(--rb-record)" stroke="none"/>
{/* dictando: tres barras (animación scaleY 1↔0.55, 0.9 s steps(27), delays 0/.15/.3 s) */}
<rect x="9.15" y="10.05" width="1.25" height="3.9" rx="0.62"/>
<rect x="11.38" y="9.05" width="1.25" height="5.9" rx="0.62"/>
<rect x="13.6" y="10.05" width="1.25" height="3.9" rx="0.62"/>
{/* entrada: 75 ms ease-smooth-out, from opacity 0, scale .25, blur 4px */}
```

`stroke-width` va en unidades del viewBox: a 32 px con 1.6 ⇒ ≈2.13 px de trazo real. La 'a' es un círculo de radio 5.5 + asta vertical tangente en x=17.5, de y 6.5 a 17.5 (ink centrado en (12,12)).

Comportamiento de la cara viva (`MARK:63-78,181-187,231-248,382-444`):
- Constantes: centro ojos `(12, 11.15)`; separación en reposo `1.88` (ojos en 10.12/13.88), mirada máx `1.18`, giro máx `12°`, `rx` reposo `.88`, `ry` reposo `1.32`, junto `1.22`, lejos `2.05`.
- Cursor: `reach = clamp((dist − face·0.18)/(face·1.35), 0, 1)` con `face = ancho_px/2`; `lookX = nx·1.18·reach`, `lookY = ny·1.18·reach`, `rot = nx·12·reach`, `lean = nx·(size·0.05)·reach` (y ×0.4), `spread = lerp(1.22, 2.05, reach)`, `rx = lerp(.98, .634, reach·|nx|)`, `ry = lerp(1.40, 1.214, reach)`.
- Suavizado por cuadro (calibrado a 180 Hz, escalado por dt): cuerpo `0.08` (0.045 con `lag`), mirada `0.16` (0.055 con `lag`), forma `0.14`, párpado `0.42`. Se dibuja a 30 cuadros/s.
- Vaivén: `sway = sin(t_ms/1100) · 1.2°` (periodo ≈ 6.9 s).
- Parpadeo: cada 2.6–9 s (aleatorio) el párpado va a `0.1` durante 60–110 ms (`ry = max(.08, ry·lid)`); 22 % de doble parpadeo tras 160 ms; un clic sobre la marca parpadea 80 ms. Con `lid ≤ .88` se oculta la ojera.
- Sin cursor: ojos centrados y quietos (solo sway/parpadeo).

Ícono de app (`docs/assets/atic-mark.svg`, completo):

```svg
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 128 128" role="img" aria-label="Atic">
  <rect width="128" height="128" rx="28" fill="#111110"/>
  <g fill="none" stroke="#F4F1EA" stroke-width="6.5" stroke-linecap="butt">
    <circle cx="64" cy="64" r="29"/>
    <path d="M93 35v58"/>
  </g>
</svg>
```

### 2.8 Efecto líquido

**Producción = campo de distancia (SDF) + marching squares** (`lib/liquid/Skin.svelte`, `sdf.ts`, `contour.ts`, `trace.ts`), **no** el filtro SVG. Parámetros (`lib/liquid/constants.ts`): `BLEND = 24` (perilla `smin`), `CELL = 6` px, `SMOOTH = 4` pasadas, `REACH = BLEND/2 = 12` px (hueco máximo con cuello), `INFLUENCE = 24`. Render del skin con `blend = 0` (`OverlaySurface.svelte:75`): **sin filetes de fusión entre formas en el notch**. `smin(a,b,k) = k<=0 ? min(a,b) : b(1−h)+ah − k·h(1−h)`, `h = clamp(.5 + .5(b−a)/k, 0, 1)` (`sdf.ts:78-82`).
Además: `drop-shadow(--shadow-goo)` sobre el path ya fundido; margen `SHADOW_PAD = 48` en el SVG; respiración (`breathe`, mientras graba/dicta/un agente trabaja): `filter: brightness(1.04 − 0.04·cos(clock/2.4·1turn))`, es decir brillo 1.00–1.08 con periodo 2.4 s (`SKIN` estilos finales).

**Filtro SVG legacy** (`lib/GooFilter.svelte`, usado hoy solo por `ParticleWheel` compacta y como referencia). Parámetros copiados:

```html
<svg width="0" height="0" aria-hidden="true" focusable="false" style="position:absolute;overflow:hidden">
  <defs>
    <filter id="goo" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB">
      <feGaussianBlur in="SourceGraphic" stdDeviation="6" result="blur"/>
      <feColorMatrix in="blur" type="matrix"
        values="1 0 0 0 0  0 1 0 0 0  0 0 1 0 0  0 0 0 18 -7" result="goo"/>
      <feComposite in="SourceGraphic" in2="goo" operator="atop"/>
    </filter>
  </defs>
</svg>
```

`GOO_SIGMA = 6` (`GooFilter.svelte:10`), umbral de alfa 7/18 ≈ 0.389, alcance `1.72·σ ≈ 10.3 px`, `GOO_GROW = 0.28·σ = 1.68 px` de engorde por lado (`:23`, `APPCSS:252`). Regla: el filtro va sobre las siluetas sin contenido; texto e iconos van en otra capa encima; todas las formas fundidas del mismo color `--skin` (`Features/liquid.md`).

Para el notch del video: un rectángulo redondeado abajo + hombros (2.3) es fiel; el "líquido" real solo se ve en la rueda, en floats (acto B) y en el globo de agentes.

### 2.9 Caras de la isla ("notch cargado")

Reglas (`docs/DISENO_PILL.md:209-228`): una sola cara, transitoria; nunca convive con la tira ("la cara gana"); un solo blob (la tarjeta cuelga con gap 0 y la skin llena la caja entera); tamaño fijo al abrir (`islandCardW/H`...); solo cantos horizontales para `agent` (en laterales se degrada); colapsar (clic afuera / Esc) vuelve a la pestaña; la marca sigue arriba en su banda de 40 px. `IslandFace` = `tab | agent | dictation | live | clipboard | snippets | system | agents | customize` (`PLAN:61-70`). Tamaños en la tabla de 2.2. Layout común: `.p-island.is-face .p-island-body { padding-top: var(--face-tab-h) }` (`PS:6915-6921`); las herramientas se ocultan (`PS:7019-7021`). `.p-face`: `padding 10px 12px`, `gap 6px`, columna centrada (`PS:7028-7042`).

- **Cara `agent` (permiso pendiente)** (`PS:5650-5697`, textos `es.ts page.agents.permission`): 280 px de ancho, tarjeta de 104 px bajo la banda de 40. Fila cabecera: `AgentLogo` 16 px + **"El agente espera tu permiso"** (12/600, `--text`). Descripción: `<strong>{tool}</strong> · {description}` (12 px, `--muted`, una línea con elipsis). Dos botones píldora de 32 px de alto, gap 8, `flex 1`: **RECHAZAR** (fondo `--rec` 18 %, anillo inset 1 px `--rec` 42 %, texto `--rec`) y **APROBAR** (fondo `--ok` 18 %, texto `--ok`); 11 px/600/MAYÚSCULAS/ls .06em; press `scale(.96)` en 75 ms (`PS:7674-7734`). Auto-abre con un pedido nuevo.
- **Cara `dictation`** (`PS:5699-5716`, `PS:7469-7551`): 232 × 72 bajo la pestaña. Escuchando: `Waveform` de 8 barras variante `voice`, ancho 8rem = 128 px, alto 0.65rem ≈ 10.4 px, centrada. Otros estados: icono Mic 16 px (trazo 1.5) + etiqueta MAYÚSCULAS 11 px/500/ls .1em: "Transcribiendo…" (`--warn`), "Pegado" (`--ok`), "Error" (`--rec`), "Dictar" (`--muted`) (`es.ts pill.*:672-700`). En laterales: 176 px de ancho.
- **Cara `live` (ambiental, agentes trabajando)** (`PS:5211-5290`, `PS:7046-7175`): 232 px de ancho, una fila por agente de 32 px. Fila: estado (aro giratorio 12 px, borde 2 px, `rotate(clock/0.8·1turn)`; check 14 px trazo 2.2 si `ready`; "!" si espera) + logos 16 px + etiqueta 11/600 + animación `p-live-act` de 3 puntos; hover con fondo y radio `22 − 8 = 14`. Solo aparece con la tira cerrada; el hover abre la tira.
- **Cara `clipboard` / `snippets` / `system` / `customize` / `agents`**: panel con grab-bar arriba (barrita 2rem × 3 px, radio 999, `color-mix(--rb-text 24%)`, hover 55 %; `PS:6590-6629`) y el contenido de la herramienta (`ClipboardHistoryList`, `SnippetsList` con pestañas Tablero/Textos/Notas, `SystemPanel`, `PillCustomize`, `AgentLauncher`). `padding: 4px 10px 0` en clipboard/textos/sistema; agentes `6px 8px 8px` (`PS:7420-7444`). Contenido interior no se detalla aquí.
- Todas usan `transition: opacityFade` (80 ms) al montar/desmontar y la transición de caja de 2.6.

### 2.10 Avisos en la pestaña (resumen)

Botones de aviso 26 × 26 (`--island-cue-btn`) al lado de la marca; logos de agente 18 px; update = icono Download (18 px trazo 1.8 en la tira; `size 17, strokeWidth 1.9` en la pestaña) color `--info`, `Check` `--ok` cuando está listo, `--muted` ocupado (`PS:5013-5033,6902-6908`); música = carátula 18 px (radio 5) + ecualizador de 3 barras 12×12 (`p-media-eq`, 0.9/0.7/1.1 s, `scaleY .25↔1`); texto de aviso con elipsis en tramo fijo de 96 px. No cuelgan hacia adentro. Desaparecen de la pestaña cuando la tira abre.

### 2.11 Disco flotante en reposo (no notch, por completitud)

52 px de diámetro (`bar`), silueta = pastilla `pillShape` (radio 26), `AticMark` 32 px trazo 1.5 `alive`, color `--text`, fondo `--skin`, misma sombra. Grabando/dictando se alarga a cápsula (punto rojo, cronómetro, ondas) (`DISENO_PILL.md:69-79`, `PLAN:303-307`). Rueda: escenario 252 × 252, disco 232.

---

## 3. Iconos de herramientas

Librería: **Lucide** (paquete `lucide` **v1.29.0**, datos `IconNode`), renderizada con **morphicons v1.4.2** (`lib/icons.ts:1-3,140-162`, `lib/ToolIcon.svelte`, `lib/ui/Icon.svelte`; morphicons dibuja todo el icono como un único `<path>`, equivalente visual al set de elementos de Lucide). Marcado común (`node_modules/morphicons/dist/MorphIcon.svelte`, bloque `<svg>` final):

```tsx
<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor"
     strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round">…</svg>
```

En la tira del notch: `size = 22`, `strokeWidth = 1.6` (`PS:5145`). Play/Pause `20 / 1.7`, Download/Check de update `18 / 1.8`. Si cambia el prop `icon` (ej. Play↔Pause) morphicons anima el `d` con resorte "snappy".

| Herramienta (`id`) | Icono Lucide | Archivo lucide | Elementos SVG (viewBox 0 0 24 24) |
|---|---|---|---|
| Reuniones `meetings` | `CircleDot` | `circle-dot` | `<circle cx="12" cy="12" r="10"/>` `<circle cx="12" cy="12" r="1"/>` |
| Dictado `dictation` (solo atajo; cara dictado) | `Mic` | `mic` | `<path d="M12 19v3"/>` `<path d="M19 10v2a7 7 0 0 1-14 0v-2"/>` `<rect x="9" y="2" width="6" height="13" rx="3"/>` |
| Clipboard `clipboard` | `Clipboard` | `clipboard` | `<rect width="8" height="4" x="8" y="2" rx="1" ry="1"/>` `<path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>` |
| Textos `snippets` | `AlignLeft` (= `TextAlignStart`) | `text-align-start` | `<path d="M21 5H3"/>` `<path d="M15 12H3"/>` `<path d="M17 19H3"/>` |
| Agentes `agents` | `SquareTerminal` | `square-terminal` | `<path d="m7 11 2-2-2-2"/>` `<path d="M11 13h4"/>` `<rect width="18" height="18" x="3" y="3" rx="2" ry="2"/>` |
| Sistema `system` | `Cpu` | `cpu` | 12 patas `<path d="M12 20v2"/> <path d="M12 2v2"/> <path d="M17 20v2"/> <path d="M17 2v2"/> <path d="M2 12h2"/> <path d="M2 17h2"/> <path d="M2 7h2"/> <path d="M20 12h2"/> <path d="M20 17h2"/> <path d="M20 7h2"/> <path d="M7 20v2"/> <path d="M7 2v2"/>` + `<rect x="4" y="4" width="16" height="16" rx="2"/>` `<rect x="8" y="8" width="8" height="8" rx="1"/>` |
| Capturas `captures` | `Crop` | `crop` | `<path d="M6 2v14a2 2 0 0 0 2 2h14"/>` `<path d="M18 22V8a2 2 0 0 0-2-2H2"/>` |
| Pizarra `board` | `Pencil` | `pencil` | `<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>` `<path d="m15 5 4 4"/>` |
| Color `color` | `Pipette` | `pipette` | `<path d="m12 9-8.414 8.414A2 2 0 0 0 3 18.828v1.344a2 2 0 0 1-.586 1.414A2 2 0 0 1 3.828 21h1.344a2 2 0 0 0 1.414-.586L15 12"/>` `<path d="m18 9 .4.4a1 1 0 1 1-3 3l-3.8-3.8a1 1 0 1 1 3-3l.4.4 3.4-3.4a1 1 0 1 1 3 3z"/>` `<path d="m2 22 .414-.414"/>` |
| Apps `launcher` (solo atajo Ctrl+Space) | `Search` | `search` | `<path d="m21 21-4.34-4.34"/>` `<circle cx="11" cy="11" r="8"/>` |
| Ventana `window` (última celda de la tira) | `AppWindow` | `app-window` | `<rect x="2" y="4" width="20" height="16" rx="2"/>` `<path d="M10 4v4"/>` `<path d="M2 8h20"/>` `<path d="M6 4v4"/>` |
| Más `more` (solo si hay herramientas detrás de "Más") | `Ellipsis` | `ellipsis` | `<circle cx="12" cy="12" r="1"/>` `<circle cx="19" cy="12" r="1"/>` `<circle cx="5" cy="12" r="1"/>` |
| Personalizar `customize` (página "Más") | `SlidersHorizontal` | `sliders-horizontal` | `<path d="M10 5H3"/> <path d="M12 19H3"/> <path d="M14 3v4"/> <path d="M16 17v4"/> <path d="M21 12h-9"/> <path d="M21 19h-5"/> <path d="M21 5h-7"/> <path d="M8 10v4"/> <path d="M8 12H3"/>` |
| Volver `back` | `ArrowLeft` | `arrow-left` | `<path d="m12 19-7-7 7-7"/>` `<path d="M19 12H5"/>` |
| Pill `pill` (nav de Ajustes) | `Pill` | `pill` | `<path d="m10.5 20.5 10-10a4.95 4.95 0 1 0-7-7l-10 10a4.95 4.95 0 1 0 7 7Z"/>` `<path d="m8.5 8.5 7 7"/>` |
| Música (celda) | `Play` / `Pause` | `play` / `pause` | Play: `<path d="M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"/>` · Pause: `<rect x="14" y="3" width="5" height="18" rx="1"/>` `<rect x="5" y="3" width="5" height="18" rx="1"/>` |
| Update | `Download` / `Check` | `download` / `check` | Download: `<path d="M12 15V3"/> <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/> <path d="m7 10 5 5 5-5"/>` · Check: `<path d="M20 6 9 17l-5-5"/>` |
| Quitar aviso live | `X` | `x` | `<path d="M18 6 6 18"/>` `<path d="m6 6 12 12"/>` |

Fuente de los datos: `apps/desktop/node_modules/lucide/dist/esm/icons/*.mjs`. Mapeo de ids a iconos: `lib/icons.ts:140-162` (`TOOL_ICONS`). Otros mapas que no cambian la tira: `LAUNCHER_ICONS` (`icons.ts:164-185`), `AGENT_ICONS`.
Nota: los `<svg>` del wheel/ParticleWheel usan los mismos iconos (mismos ids).

---

## 4. Etiquetas en español y atajos

### 4.1 Herramientas (`lib/core/i18n/es.ts:26-99`; `localizeTool` las aplica)

Tooltip / aria en la tira = `"{label} — {short}"`; `aria-label` = `"{label}. {short}"` (`PS:5122-5134`).

| `id` | `label` | `short` | `actionLabel` | `blurb` |
|---|---|---|---|---|
| meetings | Reuniones | Grabar y resumir | Grabar (record "Grabar" / stop "Parar") | Audio del PC, transcripción local y resúmenes editables. |
| dictation | Dictado | Voz a texto | Dictar (start "Dictar" / stop "Terminar") | Habla y pega texto en cualquier app con un atajo. |
| clipboard | Clipboard | Historial | Ver historial | Historial local de texto e imágenes; atajo para pegar desde la pill. |
| snippets | Textos | Guardados a mano | Ver textos | Los textos que escribes siempre, listos para pegar. Más un bloc para notas sueltas. |
| agents | Agentes | Terminales con agentes | Abrir consola | Tus agentes CLI en terminales con pestañas, splits y sesiones. |
| system | Sistema | Volumen y recursos | Abrir controles | Volumen, CPU, RAM y pantallas. Cierra apps y cambia el brillo desde la pill. |
| captures | Capturas | Pantalla | Tomar captura | Recortes rápidos al portapapeles y al shelf flotante. |
| board | Pizarra | Marcar la pantalla | Dibujar | Congela la pantalla y la marcas con flechas y círculos, ahí donde está. |
| color | Color | Cuentagotas | Elegir color | Elige un color de la pantalla, píxel a píxel, o desde la rosa cromática. Se copia al portapapeles. |
| launcher | Apps | Programas del sistema | Buscar apps | Abre apps y acciones del PC. Mismo launcher que Ctrl+Space (tipo Spotlight). |

Cadenas de la pill (`es.ts:672-…`): `pill.more` "Más" · `pill.moreHint` "Las que dejaste en el segundo anillo" · `pill.wheelBack` "Volver" · `pill.openMain` "Ventana principal" · `pill.openMainHint` "Abrir la app" · `pill.customize` "Personalizar" · `pill.customizeHint` "Ordenar y elegir herramientas" · `pill.stopRecord` "Detener grabación" · `pill.stopDictate` "Detener dictado" · `pill.dictatingHint` "Dictando · clic para detener" · `pill.listening` "Dictando…" · `pill.transcribing` "Transcribiendo…" · `pill.pasted` "Pegado" · `pill.paste` "Pegar" · `pill.dismiss` "Descartar" · `pill.clickTools` "Clic para las herramientas" · `pill.hoverTools` "Pasa el mouse para las herramientas" · `pill.dragMove` "Arrastra para mover" · `pill.toolsWithShortcut` "{shortcut} · herramientas" · `pill.permission` "permiso" · `pill.working` "El agente está trabajando" · `pill.chipWorking` "Trabajando…" · `pill.waiting` "El agente espera tu permiso" · `pill.ready` "Listo". Rueda: `tools.wheelCaption` "Herramientas", `wheelClose` "Cerrar", `wheelAria` "Herramientas Atic".
La marca (botón) usa `markAction.label` (Detener grabación/dictado o etiqueta neutra; no lo rastreé).

### 4.2 Atajos por defecto (`crates/core/src/config.rs:434-483` y saneado `:861-952`)

`CmdOrCtrl` se muestra "Ctrl" en Windows/Linux y "⌘" en macOS; `Alt` "Alt"/"⌥"; `Shift` "Shift"/"⇧" (`lib/core/hotkeys.ts:98-121`); texto corrido `formatShortcutText` = "Ctrl + Shift + R".

| Acción | Config | Valor por defecto | Etiqueta es.ts (`settings.shortcuts`, `es.ts:251-278`) |
|---|---|---|---|
| Grabar / parar reunión | `global_shortcut` | `CmdOrCtrl+Shift+R` | "Grabar / parar" — "Empieza y termina una grabación desde cualquier app." |
| Dictar | `dictation_shortcut` | `CmdOrCtrl+Shift+D` (modo por defecto `push_to_talk`, `config.rs:445`) | "Dictar" — "Habla y el texto se pega donde estabas." |
| Traer la pill al cursor | `summon_pill_shortcut` | `CmdOrCtrl+Shift+P` | "Traer la pill" — "La acerca al cursor." |
| **Rueda de herramientas** | `pill_radial_shortcut` | **`Alt+Z`** (no `Alt+Space`: es del SO, `config.rs:437-440,870-876`) | "Rueda de herramientas" — "Mantenlo apretado y suelta sobre la que quieras." |
| Historial del portapapeles | `clipboard_shortcut` | `CmdOrCtrl+Shift+V` | "Historial del portapapeles" |
| Textos guardados | `snippets_shortcut` | `CmdOrCtrl+Shift+S` | "Textos guardados" |
| Consola de agentes | `agents_shortcut` | `CmdOrCtrl+Shift+A` | "Consola de agentes" — "Abre o cierra el chat de agentes junto a la pill." |
| Captura de pantalla | `screenshot_shortcut` | `CmdOrCtrl+Shift+4` | "Captura de pantalla" |
| Pizarra | `board_shortcut` | `CmdOrCtrl+Shift+X` | "Dibujar en pantalla" — "Congela la pantalla y deja marcarla. Esc la saca." |
| Color | `color_shortcut` | `CmdOrCtrl+Shift+C` | "Elegir color" — "Lee el píxel bajo el cursor y lo copia. R abre la rosa." |
| Launcher (Spotlight) | `launcher_shortcut` | **`CmdOrCtrl+Space`** | "Launcher" — "Buscar y abrir apps, como Spotlight." |
| Voltear ventana | `window_flip_shortcut` | `CmdOrCtrl+Shift+B` | "Voltear ventana" — "Gira la ventana al frente para anotar en el reverso. Esc la devuelve." |

Registro de teclas globales en `apps/desktop/src-tauri/src/shortcuts.rs` (`:203-207,307,353,433`). Título/pista de la sección: "Atajos globales" — "Funcionan en cualquier app, no solo con Atic al frente." (`es.ts:252-253`). Otros atajos internos: `Ctrl+K` buscar (`chrome.search`), `Ctrl+N` nueva consola (`es.ts:1289-1292`), Esc cierra caras/rueda.

---

## 5. Constantes TypeScript listas para pegar

```ts
export const PILL = {
  pad: 4, bar: 52, wheel: 252, panelW: 312, panelH: 332,
  islandThick: 40, islandCueThick: 42, islandLong: 124, islandMark: 32,
  islandCueBtn: 26, islandCueMark: 18, islandCueMsgW: 96,
  islandTool: 44, islandGap: 2,
  islandCardW: 280, islandCardH: 104, islandClipW: 280, islandClipH: 252,
  islandSysW: 300, islandSysH: 430, islandCustomW: 360, islandCustomH: 290,
  islandAgentsSetupW: 400, islandAgentsSetupH: 188, islandAgentsW: 440, islandAgentsH: 496,
  islandBrowseW: 680, islandBrowseH: 620, islandDictW: 232, islandDictH: 72, islandDictSideW: 176,
  islandLiveRow: 32, islandLiveSideW: 30, islandClipR: 22,
} as const;                                  // apps/desktop/src/lib/surfaces/overlay/pillStage.ts:15-177

export const islandStripLong = (n: number) => (n <= 0 ? PILL.bar : n * PILL.islandTool + (n - 1) * PILL.islandGap);
export const notchRadius = (w: number, h: number) => { const cap = Math.min(w, h) / 2; return cap > PILL.islandClipR + 8 ? PILL.islandClipR : cap; };

export const COLORS = {
  dark:  { skin:'#1a1a18', text:'#f0f0ea', muted:'#a8a89e', faint:'#8f8f86', rec:'#e85a52', ok:'#6faf88', warn:'#d4a84b', info:'#8fa9b8' },
  light: { skin:'#f7f7f2', text:'#171714', muted:'#5f5f58', faint:'#6b6b63', rec:'#d6453d', ok:'#3f7355', warn:'#946718', info:'#47708a' },
} as const;

export const SHADOW_GOO = '0 10px 22px rgb(0 0 0 / 38%)';
export const TOOLS_DEFAULT = ['meetings','clipboard','snippets','agents','system','captures','board','color'] as const; // + 'window' al final de la tira
```

---

## 6. No encontrado o sin verificar

- **Medidas en la app en ejecución**: no pude leer el DOM real (sin puerto CDP 9222/9223; había una `atic-desktop.exe` en marcha, no la toqué). La silueta cerrada de 124×44 y abierta de 48 de alto salen de leer CSS/TS (2.1); conviene confirmarlas con una captura antes de fijar pixeles definitivos (si la real fuera 124×40 el resto de la ficha no cambia).
- Posición vertical exacta del techo en el monitor del usuario: `y = 0` del área útil asumido para Windows sin barra superior.
- No revisé el interior de `ClipboardHistoryList`, `SnippetsList`, `SystemPanel`, `AgentLauncher`, `PillCustomize`, ni los "vistazos" (`*Peek.svelte`): sus contenidos son sub-UI aparte.
- `markAction.label` (tooltip de la marca) no lo rastreé.
- `--island-rise` está definido (`APPCSS:199`) pero no lo vi consumido en el código actual de la tira.
- Mac: el notch puede colgarse bajo la barra de menú (`edgeDock.ts:109-113`); no se cubre aquí.
