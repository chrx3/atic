# Ficha visual: Textos (snippets + bloc) y Flipboard (tablero de la ventana volteada)

Fuente: código de `apps/desktop/src` (Svelte). Todo lo de abajo sale de leer el código; lo que no
se pudo confirmar está marcado como **NO ENCONTRADO / ESTIMADO**.

Abreviaturas de ruta (todas bajo `apps/desktop/src/`):

| Sigla | Ruta |
| --- | --- |
| `WFS` | `lib/surfaces/windowFlip/WindowFlipSurface.svelte` |
| `FB` | `lib/surfaces/windowFlip/FlipBoard.svelte` (3326 líneas: script 1–1549, markup 1551–2258, style 2260–3326) |
| `FIP` | `lib/surfaces/windowFlip/FlipInsertPreview.svelte` |
| `FL` | `lib/surfaces/windowFlip/flipLayout.ts` |
| `SF` | `lib/surfaces/overlay/snippets/SnippetsFloat.svelte` |
| `FPL` | `lib/surfaces/overlay/snippets/FlipPagesList.svelte` |
| `SL` | `lib/SnippetsList.svelte` |
| `SP` | `lib/surfaces/overlay/pill/SnippetsPeek.svelte` |
| `APP` | `app.css` |
| `ES` | `lib/core/i18n/es.ts` |

---

## 0. Tokens de color resueltos (los dos namespaces conviven)

**Tema por defecto.** La config trae `ui_theme = "system"` (`crates/core/src/config.rs:491`): sigue al SO. El diseño
se hace en oscuro y el claro se deriva (`styles/palettes/atic/dark.css:1-14`, `light.css:1-8`). Para el video: **oscuro**.
`applyTheme` escribe `data-theme` y `data-theme-base` en `<html>` (`lib/theme.ts:166-167`).

Hay dos familias de variables. **Textos (float)** usa las nuevas (`--text`, `--surface`…); **SnippetsList, FlipPagesList y
todo el Flipboard** usan las viejas `--rb-*` (definidas en `APP:75-118` claro y `APP:329-366` oscuro).

### 0.1 Namespace nuevo (`styles/palettes/atic/dark.css:17-71`, `light.css:10-53`)

| Token | Oscuro | Claro |
| --- | --- | --- |
| `--bg` | `#121211` | `#ecece6` |
| `--surface` | `#1a1a18` | `#f7f7f2` |
| `--surface-2` | `#1e1e1b` | `#efefe8` |
| `--elevated` | `#262622` | `#ffffff` |
| `--text` | `#f0f0ea` | `#171714` |
| `--muted` | `#a8a89e` | `#5f5f58` |
| `--faint` | `#8f8f86` | `#6b6b63` |
| `--line` | `rgb(240 240 234 / 10%)` | `rgb(28 28 24 / 11%)` |
| `--accent` | `#e8e8e0` | `#1a1a17` |
| `--skin` (relleno de todo float líquido) | `#1a1a18` | `#f7f7f2` |

### 0.2 Namespace viejo `--rb-*` (`APP:75-118`, `APP:329-366`)

| Token | Oscuro | Claro |
| --- | --- | --- |
| `--rb-bg0` | `#121211` | `#ecece6` |
| `--rb-bg1` | `#1a1a18` | `#e2e2da` |
| `--rb-surface` | `#1e1e1b` | `#f7f7f2` |
| `--rb-surface-2` | `#262622` | `#efefe8` |
| `--rb-surface-elevated` | `#2a2a26` | `#ffffff` |
| `--rb-text` | `#f0f0ea` | `#171714` |
| `--rb-muted` | `#9a9a90` | `#5f5f58` |
| `--rb-faint` | `#6e6e66` | `#8f8f86` |
| `--rb-border` | `rgba(246,246,241,.10)` | `rgba(28,28,24,.11)` |
| `--rb-accent` | `#f0f0ea` | `#1a1a17` |
| `--rb-record` (rojo/peligro) | `#e85a52` | `#d6453d` |
| `--rb-record-soft` | `#3a2220` | `#f7e6e3` |
| `--rb-ok` (verde) | `#6faf88` | `#3f7355` |
| `--rb-hairline` | `color-mix(text 12%, transparent)` = `rgba(240,240,234,.12)` | `rgba(23,23,20,.12)` |
| `--rb-focus` | `0 0 0 3px rgba(232,90,82,.22), 0 0 0 1px #e85a52` | `0 0 0 3px rgba(229,72,63,.16), 0 0 0 1px #d6453d` |
| `--rb-radius` / `-sm` / `-xs` | 14px / 8px / 5px | igual |

(`APP:236` hairline, `APP:254-256` radios; `--rb-focus` en `APP:259` claro y `APP:365` oscuro.)

### 0.3 Tipografía global

| Token | Valor (`APP:77-79`, `styles/scales.css`) |
| --- | --- |
| `--rb-font` / `--font-sans` | `"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif` |
| `--rb-display` | `"Avenir Next", "Aptos Display", "Helvetica Neue", sans-serif` (solo el h1 "Tablero") |
| `--rb-mono` | `"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace` |
| Suavizado | `-webkit-font-smoothing: antialiased` (`APP:265`) |

Aptos no viene con todos los Windows; en Windows 11 sin Office cae a **Segoe UI Variable**. Para el render conviene fijar
`Segoe UI Variable` (o Inter como sustituto) y usar los tamaños de abajo tal cual. NO ENCONTRADO qué fuente tenía cargada
la máquina donde se diseñó.

### 0.4 Curvas y duraciones (fuente única: `APP:132-232`, `styles/motion.css`)

| Token | Valor |
| --- | --- |
| `--ease-smooth-out` | `cubic-bezier(0.22, 1, 0.36, 1)` |
| `--duration-micro / quick / fast / medium / slow / very-slow` | 40 / 75 / 125 / 150 / 200 / 250 ms |
| `--float-open-dur` | `var(--duration-medium)` = **150 ms** (`APP:172`) |
| `--float-close-dur` | `var(--morph-close-dur)` = **100 ms** (`APP:173`) |
| `--launcher-bar-open-dur` / `--launcher-separate-dur` | 100 ms / 90 ms (`APP:183-184`) |
| `--float-scale` / `--float-travel` | 0.55 / 18px (`APP:176-177`) |
| `--scale-large`, `--blur-small`, `--distance-base`, `--distance-micro` | 0.96, 2px, 8px, 4px |
| Sombra de la piel `--shadow-goo` | `0 10px 22px rgb(0 0 0 / 38%)` (`styles/scales.css`) |

Transiciones Svelte (`lib/motion.ts`):

| Nombre | Efecto | Duración | Easing |
| --- | --- | --- | --- |
| `emerge` (`motion.ts:183-205`) | `opacity t; translateY(u*8px) scale(0.97+0.03t); blur(u*2px)` | entra 200 ms (`slow`), sale 125 ms (`fast`) | `cubicOut` (Svelte) |
| `tabPanel` (`motion.ts:207-227`) | `opacity t; translateY(u*4px)`, sin blur | 125 ms simétrico | ease-in-out cuadrático (`t<.5 ? 2t² : 1-(-2t+2)²/2`) |

`u = 1 - t`. Todo se apaga con `prefers-reduced-motion` (el grabado de referencia asume movimiento normal).

---

# PARTE B (va primero porque es lo que el video tiene que explicar): el FLIPBOARD

## B.1 Qué es y para qué sirve

**"Flipboard" es el reverso de cualquier ventana.** Con un atajo global, Atic saca una foto de la ventana que tienes
al frente y la **da vuelta en 3D** como una tarjeta; al otro lado hay un **tablero de notas** (papel punteado) donde
puedes escribir, hacer listas, dibujar con lápiz/resaltador, pegar imágenes y texto desde el portapapeles, tus Textos,
tus capturas y los resúmenes de tus reuniones. Al volver (Esc) la ventana original reaparece intacta.

Puntos que el video debe dejar claros (todos verificados en código):

- **Es un solo tablero compartido para todas las ventanas** ("Es el mismo tablero en todas las ventanas",
  `ES:943`; `window_flip.rs:24-27` `BOARD_NOTE`). El título dice "Tablero" y debajo "sobre {ventana}" solo como referencia
  de dónde se abrió (`WFS:356-376`).
- **Se guarda solo** (debounce 350 ms, `WFS:173-183`); aparece la píldora "Guardado" 1.6 s (`WFS:188-194`).
- **Páginas = celdas 4:3 de 1200×900 px en una tira horizontal**; el "+" de la tira agrega una celda a la derecha
  (`FL:67-68`, `FB:650-660`). Máximo `TABLERO_MAX = 12000` px = 10 páginas (`FL:75`).
- **Se exporta** a PNG, JPEG, PDF, Word y PowerPoint; cada celda es una página (`ES:1017`).
- El copy del producto: ajuste de atajos dice "Voltear ventana — Gira la ventana al frente para anotar en el reverso.
  Esc la devuelve." (`ES:282-283`).
- Es un **prototipo** según el doc del módulo Rust ("Prototipo: tapa la ventana activa, la 'da vuelta' y muestra notas",
  `window_flip.rs:1`). **No hay `Features/*.md` ni plan sobre esto** (solo aparece "voltear ventana" en `docs/MACOS.md:16`
  y "flip" en `docs/RECURSOS.md:62`). Existe una crítica de diseño en `.impeccable/critique/2026-09-08T15-52-37Z__…windowflip.md`
  (habla de "la coreografía del flip" como lo mejor de la app).

### Cómo se abre

| Vía | Detalle |
| --- | --- |
| **Atajo global** | Por defecto **`CmdOrCtrl+Shift+B`** → en Windows **Ctrl+Shift+B** (`crates/core/src/config.rs:483`, `ShortcutsSection.svelte:93-97`). Es un toggle; ignora repetición <700 ms (`window_flip.rs:146-165`). |
| Desde Textos → pestaña **Tablero** | Clic en la miniatura de una página: voltea la ventana del frente y abre el tablero **justo en esa página** (`FPL:58-66`, `window_flip.rs:247-270`). Si ya está abierto solo navega a la página. |
| Rueda de la pill | **NO tiene gajo en la rueda** (no aparece en `core/tools.ts`, `ToolId` no incluye flip). |
| Launcher | NO ENCONTRADO. |

**Importante para el guion:** el flip **no emerge de la pill**. La tarjeta se monta sobre la propia ventana del usuario.
Solo la vía "Textos → Tablero" nace de la pill (porque nace el float de Textos, ver Parte A).

### Flujo de uso típico (guion recomendado, paso a paso)

1. El usuario trabaja en cualquier app (ej. un navegador con un dashboard).
2. Pulsa **Ctrl+Shift+B**. Se captura la ventana (PNG) y la tarjeta gira 400 ms mostrando el reverso.
3. Se ve el papel punteado casi vacío con el mensaje **"Elige el Lápiz para esbozar, o empieza con una nota."** y botón
   **"Añadir texto"**.
4. Toca **Texto** (T) y clic en el papel: aparece un cuadro de texto 280×96 con foco; escribe. Se ve "Guardado".
5. Toca **Lápiz** (P): se abre la paleta de 10 colores + "+"; elige uno; dibuja un círculo/subrayado sobre el papel
   (trazo 2.6). Resaltador (H) = trazo 12 al 40 % de alfa. Borrador (E).
6. Abre **Insertar** (cajón derecho, 4 pestañas Clip/Textos/Fotos/Reun.): clic en un ítem → tarjeta de vista previa
   "Esto se va a añadir al tablero" → **Añadir**; o arrastra el ítem al papel (el papel se marca con contorno).
7. **Lista** (L): checklist con casillas; Enter añade ítem.
8. Pulsa el **"+" de la tira inferior** para agregar una página; la vista se desliza (125 ms) y una línea destella.
9. **Exportar** ▸ PDF / Word / PowerPoint / PNG / JPEG.
10. **Esc** (dos veces si hay algo seleccionado) o **Volver** o clic fuera de la tarjeta: la tarjeta gira 320 ms de vuelta
    y reaparece la ventana original.

Orden de Esc (`FB:1460-1490`, `WFS:289-294`): cierra vista previa → menú exportar → paleta → deselecciona/vuelve a "Mover"
→ recién ahí voltea la tarjeta de vuelta.

Atajos de herramienta (`FB:1497-1530`): V mover · P/D lápiz · H resaltador · E borrador · T texto · L lista ·
Supr/Retroceso borra el bloque seleccionado · Ctrl+Z deshacer · Ctrl+Shift+Z / Ctrl+Y rehacer · Ctrl+rueda zoom.

---

## B.2 Geometría de la escena (overlay + tarjeta)

Lo que existe físicamente: una ventana **overlay transparente** más grande que la ventana ajena. La tarjeta ocupa
exactamente el rectángulo de la ventana ajena.

| Regla | Valor | Fuente |
| --- | --- | --- |
| Margen de aire del overlay por lado | `max(88px, ancho/16)` en X y `max(88px, alto/16)` en Y | `window_flip.rs:1019-1029` |
| Se recorta a la pantalla virtual | sí (se corre el overlay, no la tarjeta) | `window_flip.rs:1031-1060` |
| Posición de la tarjeta | `left/top/width/height` en % del overlay | `WFS:329-332`, `window_flip.rs:1062-1071` |
| Radio de la tarjeta y de cada cara | **8px** | `WFS:431`, `WFS:525` |
| Fondo del overlay | transparente (se ve el escritorio a los lados) | `WFS:391-403` |

Ejemplo numérico útil (ventana 1280×800 en pantalla 1920×1080): aire = 88 → overlay 1456×976; tarjeta a `left 6.04 %`,
`top 9.02 %`, `width 87.9 %`, `height 82 %`. Para el video basta con una tarjeta de ~1280×800 centrada sobre un escritorio.

### Estructura DOM (simplificada)

```
.root (100%×100%, transparente; mousedown fuera de .card → cierra)
└─ .stage  (perspective: max(1200px, 4.5×cardW); perspective-origin: centro de la tarjeta)
   └─ .card (absolute; preserve-3d; transform-origin center; radio 8)
      ├─ .face.front  (captura PNG de la ventana, object-fit: fill; rotateY(0) translateZ(1px))
      └─ .face.back   (rotateY(180deg) translateZ(1px); flex column; fondo --rb-bg1; --pad 18px)
         ├─ FlipBoard (.tablero)
         │   ├─ .barra  (toolbar)
         │   ├─ .cuerpo (grid: .vista + .cajon)
         │   │    ├─ .vista → .escena → .marco-paginas → .papel (puntos, líneas de página, tinta SVG, .objeto…)
         │   │    └─ .cajon (Insertar)
         │   └─ .tira   (miniaturas de página + "+")
         └─ p.guardado-flote  (píldora "✓ Guardado", abajo-izquierda)
```

`WFS:312-388` (markup).

---

## B.3 Animación de volteo (parámetros exactos)

Constantes (`WFS:33-45`): `IDA_MS = 400`, `VUELTA_MS = 320`, `RESPALDO_MS = 500`, `FOTO_ESPERA_MS = 600`,
`CAMARA = 4.5`, `HUNDIDO = 0.5` (los dos en anchos de tarjeta).

| Parámetro | Valor |
| --- | --- |
| `perspective` (en `.stage`) | `max(1200px, 4.5 × anchoTarjeta px)` (`WFS:417`). Ej.: tarjeta 1280 → 5760 px |
| `perspective-origin` | centro de la tarjeta: `((cardLeft + cardW/2)×100 %, (cardTop + cardH/2)×100 %)` (`WFS:84-85`, `421`) |
| `transform-origin` de la tarjeta | `center center`; eje **Y** |
| Hundido (`translateZ` a 90°) | `-0.5 × anchoTarjeta px` (`WFS:318`). Ej.: 1280 → `-640px` |
| Duración ida (frente→reverso) | **400 ms** |
| Duración vuelta (reverso→frente) | **320 ms** |
| `animation` | `… linear both` (la curva va por tramo, dentro de los keyframes) |
| Orden en `transform` | `translateZ(...)` **antes** de `rotateY(...)` (crítico: al revés la tarjeta "se va de paseo" lateralmente) |

```css
/* WFS:405-490 */
.stage {
  --giro-entrada: cubic-bezier(0.45, 0, 0.725, 0.5);   /* primera mitad de cubic-bezier(.45,0,.55,1) */
  --giro-salida:  cubic-bezier(0.275, 0.5, 0.55, 1);   /* segunda mitad */
  perspective: max(1200px, var(--camara));              /* --camara = 4.5 × cardW */
  perspective-origin: var(--foco-x) var(--foco-y);
}
.card { transform-style: preserve-3d; transform: rotateY(0deg); transform-origin: center center; border-radius: 8px; }
.card.ready.flipped { transform: rotateY(180deg); }
.card.ready.al-reverso { animation: girar-al-reverso 400ms linear both; }
.card.ready.al-frente  { animation: girar-al-frente  320ms linear both; }

@keyframes girar-al-reverso {
  0%   { transform: translateZ(0) rotateY(0deg);                         animation-timing-function: var(--giro-entrada); }
  50%  { transform: translateZ(var(--hundido)) rotateY(90deg);           animation-timing-function: var(--giro-salida); }
  100% { transform: translateZ(0) rotateY(180deg); }
}
@keyframes girar-al-frente {
  0%   { transform: translateZ(0) rotateY(180deg);                       animation-timing-function: var(--giro-entrada); }
  50%  { transform: translateZ(var(--hundido)) rotateY(90deg);           animation-timing-function: var(--giro-salida); }
  100% { transform: translateZ(0) rotateY(0deg); }
}
.face { position:absolute; inset:0; overflow:hidden; border-radius:8px; backface-visibility:hidden;
        box-shadow: 0 22px 48px rgb(0 0 0 / 32%), 0 2px 8px rgb(0 0 0 / 18%); }   /* sombra por cara, no en la tarjeta */
.front { transform: rotateY(0deg)   translateZ(1px); }
.back  { transform: rotateY(180deg) translateZ(1px); background: var(--rb-bg1); color: var(--rb-text); }
```

**Fórmula para Remotion** (t = progreso lineal 0..1 de los 400 ms; `H = 0.5·cardW`):

```
si t ≤ 0.5:  u = bezier(0.45, 0, 0.725, 0.5)(t / 0.5);          rotY = 90·u;         z = -H·u
si t >  0.5: v = bezier(0.275, 0.5, 0.55, 1)((t - 0.5) / 0.5);  rotY = 90 + 90·v;    z = -H·(1 - v)
transform = `translateZ(${z}px) rotateY(${rotY}deg)`
```

La vuelta usa las mismas dos curvas con `rotY = 180 - (…)` (de 180 a 90 a 0) y 320 ms. A 90° la tarjeta es un canto
(invisible); ahí cambia la cara visible (`backface-visibility: hidden`). Si el usuario cierra a mitad de la ida, la
animación en vuelo se **invierte** con `Animation.reverse()` (`WFS:217-224`).

### Secuencia de apertura (`WFS:115-163`)

1. Llega la vista (`window-flip-open`): la tarjeta está `visibility: hidden`, `showBack=false`.
2. Espera la foto del frente (máx 600 ms; si no llega, **no se voltea nada** y se cierra).
3. `cardReady = true` (`visibility: visible`, `will-change: transform`) → 1 frame → `present` (la tapa nativa cubre la ventana viva).
4. 2 frames → `conceal` (se oculta la ventana real) → 2 frames más.
5. `giro = "reverso"; showBack = true` → arranca el keyframe de 400 ms.
6. Al terminar (`animationend` sobre `.card`), `giro = ""` y la tarjeta queda en `rotateY(180deg)`.

### Secuencia de cierre (`WFS:203-233`)

`showBack=false; giro="frente"` → 320 ms → `closeWindowFlip()` (reaparece la ventana real). Respaldo a los 500 ms.
Disparadores: **Esc**, botón **Volver**, **clic en el aire fuera de `.card`** (`WFS:242-245`), **perder foco hacia una app ajena**
(150 ms de gracia, solo con el giro asentado, `WFS:254-264`), o pulsar de nuevo el atajo.

---

## B.4 Layout del reverso (dimensiones)

Variables (`WFS:549-571`, `FB:2261-2264`):

| Variable | Valor |
| --- | --- |
| `--pad` (gutter lateral) | **18px** (12px si `compacta`) |
| `--cajon-w` | `clamp(124px, 18vw, 168px)` (`vw` = ancho del overlay) → 168 en ventanas grandes |
| `--mini-w` | `clamp(52px, 7vw, 84px)` → 84 en ventanas grandes |

Modos de espacio (`WFS:82-83`): `compacta = anchoTarjeta < 1000 || altoTarjeta < 430` (sin etiquetas en herramientas, pad 12);
`accionesSoloIcono = anchoTarjeta < 1400 || compacta` (Exportar/Insertar/Volver solo icono). Para una tarjeta de 1280
→ herramientas **con** texto, acciones **solo icono**. Para 1400+ → todo con texto.

### Alturas (tarjeta 1280×800, sin compacta) — ESTIMADO a partir de las reglas

| Zona | Alto | Cómo sale |
| --- | --- | --- |
| Barra | 44px | botones `.rb-btn` `min-height: 2.25rem`=**36px** + `padding-bottom: 8px`; **no hay padding superior** (los botones tocan el borde superior de la tarjeta; `FB:2286-2294`) |
| Vista (papel) | ≈ 677px | resto |
| Tira de páginas | ≈ 79px | `6px` arriba + miniatura 84×63 + `10px` abajo (`FB:2569-2578`) |

**Encuadre inicial:** `zoom = min(1, (vw-24)/papelW, (vh-24)/papelH)` (`FB:546-557`). Con vista ≈1256×677 y papel 1200×900 →
**zoom ≈ 0.7256 → botón "73%"**; el papel queda 871×653 centrado, con el fondo `--rb-bg1` alrededor. Al abrir el cajón
la vista pierde 168 px pero el zoom sigue 73 % (lo limita el alto).
Límites de zoom: 0.3–2.4 (`FL:11-12`); botones ±15 % (`/1.15`, `*1.15`), Ctrl+rueda ×0.92/×1.08.

### Barra (toolbar) — `FB:1559-1767`, `FB:2286-2438`

```
.barra: display:flex; flex-wrap:wrap; align-items:center; justify-content:center; gap:12px; padding:0 18px 8px
├─ .encabezado (max-width 34%)  → h1 "Tablero" + píldora "sobre {ventana}"
├─ .grupo.herramientas (gap 2px, position:relative)  → pastilla deslizante + 6 botones
└─ .grupo (gap 2px) → Deshacer, Rehacer, Alejar, [73%], Acercar, [Quitar si hay selección], Exportar▾, Insertar, Volver
```

**Botón base `.rb-btn`** (`APP:465-483`): `display:inline-flex; align-items:center; justify-content:center; gap:.4rem (6.4px);
min-height:2.25rem (36px); border-radius:999px; padding:.4rem .9rem (6.4px 14.4px); font-size:.8125rem (13px); font-weight:500;
line-height:1.2; transition: background/color/box-shadow .15s ease, transform .12s ease`. `:active` → `scale(.98)`.
`:disabled` → `opacity:.45`. En `.ico` el gap pasa a 5px y el fondo a transparente (`FB:2403-2416`).

| Variante | Color texto | Fondo | Hover |
| --- | --- | --- | --- |
| `rb-btn-ghost` (la mayoría) | `--rb-muted` `#9a9a90` | transparente | texto `#f0f0ea`, fondo `rgba(240,240,234,.06)` |
| `rb-btn-soft` (Volver, "Añadir texto", "Añadir") | `--rb-text` `#f0f0ea` | `rgba(240,240,234,.06)` | `rgba(240,240,234,.10)` |
| `rb-btn-danger` (papelera) | `--rb-record` `#e85a52` | transparente | fondo `#3a2220` |

**Herramientas** (`FB:345-376`; icono 15px; etiqueta 13px/500; título nativo `"{label} ({tecla})"`):

| id | Etiqueta ES | Tecla | Icono (Lucide) |
| --- | --- | --- | --- |
| select | Mover | V | `MousePointer2` |
| draw | Lápiz | P | `Pencil` (+ punto de tinta) |
| highlight | Resaltador | H | `Highlighter` (+ punto de tinta) |
| eraser | Borrador | E | `Eraser` |
| text | Texto | T | `Type` |
| check | Lista | L | `ListChecks` |

- **Pastilla deslizante** (`FB:2383-2401`): span absoluto `top:0; bottom:0; border-radius:999px; background: rgba(240,240,234,.10)`;
  se mueve/ensancha con `transform` y `width` en **150 ms** (`--duration-medium`) `--ease-smooth-out`; opacidad 125 ms.
  El botón activo queda con texto `--rb-text`.
- **Punto de tinta** (`FB:2423-2433`): 7×7 px, `border-radius:999px`, abajo-derecha del icono (`right:-3px; bottom:-3px`),
  `box-shadow: 0 0 0 1.5px var(--rb-surface)` (`#1e1e1b`), color = tinta actual.
- **Grupo derecho:** Deshacer `Undo2` y Rehacer `Redo2` (deshabilitados al 45 % si no hay historial, máx 50 pasos),
  Alejar `Minus`, **`73%`** (botón texto, `min-width:3.2rem` = 51.2px, `font-variant-numeric: tabular-nums`, restablece encuadre),
  Acercar `Plus`, Quitar `Trash2` (aparece con `emerge` solo con selección), Exportar `Download` (gira `LoaderCircle` 900 ms lineal
  mientras exporta; en ≥1400 muestra "Exportar"), Insertar `PanelRightOpen`↔`PanelRightClose` (en ≥1400 muestra "Insertar"),
  Volver `ArrowLeft` (`rb-btn-soft`; en ≥1400 muestra "Volver").
- **Encabezado** (`WFS:597-641`): `h1` "Tablero": `font: 600 13px/1.3 var(--rb-display)`; a su derecha píldora `.sobre`:
  `padding:2px 8px 2px 3px; border-radius:999px; background:#262622; font:500 11px; color: rgba(240,240,234,.72)`;
  dentro, icono de la app (16×16, radius 4, `outline:1px solid rgba(255,255,255,.14)`; si no hay icono `AppWindow` 11px sobre `#2a2a26`)
  y el texto `sobre {título}` (ej. "sobre Google Chrome"; se recorta con `…`). En `compacta` se oculta el texto.
  Tooltip nativo: "Es el mismo tablero en todas las ventanas".

**Paleta de colores** (`FB:1593-1631`, `3147-3228`): aparece bajo la barra al elegir Lápiz/Resaltador (segundo clic la alterna),
con `emerge`. `position:absolute; top:calc(100% + 8px); left: <x del botón activo>; max-width:232px; padding:8px; gap:4px;
flex-wrap; background:#1e1e1b; border:1px solid rgba(240,240,234,.12); border-radius:8px; box-shadow:0 8px 24px rgb(0 0 0 / 22%)`.
Cada muestra `.lapiz` 24×24, radius 5; el color es un `::after` circular `inset:5px` (`inset:4px` al hover, **`inset:3px` + anillo doble
`0 0 0 2px #1e1e1b, 0 0 0 3px #f0f0ea` si elegido**), con `inset 0 0 0 1px rgb(0 0 0 / 25%)`; `:active` scale .96. Última muestra: "+" con borde
discontinuo 1.5px (color libre; `<input type=color>` oculto).

| Nombre | Hex |
| --- | --- |
| Rojo (por defecto) | `#e5483f` |
| Naranja | `#d9622b` |
| Arena | `#d6b48a` |
| Dorado | `#946718` |
| Verde | `#3f7355` |
| Turquesa | `#2f8f83` |
| Azul | `#526d83` |
| Violeta | `#7a5ea8` |
| Rosa | `#c14a7a` |
| Negro | `#1c1917` |

Trazos (`FB:139-141`, `FL:conAlfa`): lápiz `stroke-width 2.6`; resaltador `12` con color `+ "66"` (alfa 0.4); `stroke-linecap/linejoin: round`;
borrador radio 16 (`FL:32`).

**Menú de exportar** (`FB:1719-1742`, `2307-2370`): `absolute; top:calc(100% + 8px); right:0; min-width:148px; padding:6px; background:#1e1e1b;
border:1px solid hairline; radius 8; shadow 0 8px 24px rgb(0 0 0 / 22%)`, con `emerge`. Ítems (`padding:6px 10px; radius 6; 12.5px; gap 8;` hover
`rgba(240,240,234,.08)`) con icono 14px en `--rb-muted`: **PNG** (`FileImage`), **JPEG** (`Camera`), **PDF** (`FileType`), **Word** (`FileText`),
**PowerPoint** (`Presentation`). Pie 10.5px muted: "Cada celda del tablero es una página." Avisos tras exportar (11.5px muted, fila sobre el tablero):
"Exportado" (2.4 s) o "No se pudo exportar: {error}" (8 s) o "No se pudieron leer {n} imágenes." (6 s).

### Vista y papel — `FB:2440-2560`

| Elemento | Valores |
| --- | --- |
| `.vista` | fondo `--rb-bg1` (`#1a1a18` oscuro / `#e2e2da` claro), `overflow:hidden`, cursor `grab` (`crosshair` con lápiz/resaltador/borrador) |
| `.escena` | `transform: translate(panX,panY) scale(zoom)`, origen centro; con `.animando` transiciona `transform` **125 ms** smooth-out |
| `.papel` | `width 1200×N páginas` × `900 px` (en unidades de papel), `border-radius:8px`, `box-shadow: 0 2px 8px rgb(0 0 0 / 28%)`, `overflow:hidden` |
| Color de hoja | **oscuro:** `color-mix(sRGB, #2a2a26 70%, #f0f0ea)` = **`rgb(101,101,97)` `#656561`** (¡gris medio, más claro que el fondo!); **claro:** `#ffffff` |
| Puntos de la cuadrícula | `radial-gradient(circle, punto 1px, transparent 1.25px)`, `background-size: 24px 24px`, `background-position: 12px 12px`. `punto`: oscuro `rgba(240,240,234,.22)`, claro `rgba(23,23,20,.18)` |
| Separador de página | línea vertical 1px `--rb-hairline` con `opacity .8` cada 1200 px |
| Destello de página nueva | barra 2px `--rb-accent`, opacidad 0→1→0, `1.2s ease-out`, **2 iteraciones**, se retira a 1.4 s |
| Al arrastrar algo sobre el papel | contorno `1.5px solid rgba(240,240,234,.35)`, `outline-offset:-2px` |

(El cálculo del gris de la hoja es aritmético: 0.7·(42,42,38)+0.3·(240,240,234) = (101.4,101.4,96.8). Conviene verificarlo
a ojo contra una captura real: NO HAY captura del tablero en `docs/assets`.)

**Estado vacío** (`FB:1811-1825`, `2741-2764`): bloque centrado `position:absolute; inset:26% 15% auto` dentro del papel,
`flex column; align-items:center; gap:12px; 13px/1.45; color --rb-text; text-align:center; max-width 36ch`, entra con `emerge`:

- Texto: **"Elige el Lápiz para esbozar, o empieza con una nota."**
- Botón `rb-btn rb-btn-soft`: **"Añadir texto"** (solo si la herramienta no es lápiz/resaltador/borrador; inserta un cuadro en `(centro, 42 % alto)`).

### Objetos del tablero — `FB:2786-3028`

| Objeto | Tamaño por defecto | Notas |
| --- | --- | --- |
| Texto | **280×96** (`FL:34-35`) | `textarea` 13.5px/1.5, padding `18px 12px 10px`, encoge la fuente hasta 8.5px si no cabe (`FL ajustarFuente`), crece en alto al escribir |
| Lista | **260×120** (`FL:36-37`) | padding `18px 10px 6px`; fila `min-height 28px; gap 8px`; casilla 14×14 `accent-color:#6faf88`; texto 13px; marcada → `--rb-faint` `#6e6e66` + `line-through`; botón "Añadir ítem" 11.5px muted (`min-height 28px`, radius 5, hover `rgba(…,.08)`) |
| Imagen | ancho natural (mín 80, máx 1200-48=1152) | `object-fit: fill`, contorno `1px rgba(255,255,255,.10)` inset |

Marco común `.objeto`: `border-radius:5px; background: var(--hoja)` (igual que el papel, es decir **sin fondo distinto**); `outline:1px solid transparent`;
hover → outline `rgba(240,240,234,.22)`; **seleccionado** → `outline:1.5px solid rgba(240,240,234,.45)`, `overflow:visible`, al frente.
Transición de outline 125 ms. Aparece con `emerge` (200 ms).

Chrome de cada objeto (visible con hover/selección/foco):
- **Agarre** superior: `height:20px`, ancho completo, `background: rgba(240,240,234,.08)`, radio `5px 5px 0 0`, icono `GripVertical` 12px `--rb-muted`; opacity 0→1 en 125 ms.
- **Quitar** (×): 20×20, `top:3px; right:3px`, `border-radius:999px`, fondo `#1e1e1b`, borde hairline, icono `X` 11px muted.
- **Asa de tamaño** (solo seleccionado): 16×16, `right:-7px; bottom:-7px`, `border:2px solid #1e1e1b; border-radius:3px; background:#f0f0ea; cursor:nwse-resize`;
  aparece con `opacity 0→1, scale .25→1, blur 4px→0` en 125 ms smooth-out.
- Placeholder de textos: **"Escribe aquí. Se guarda solo y lo ves desde cualquier ventana."** (itálica, color `--rb-muted`); placeholder de ítem: "Ítem".

### Cajón "Insertar" — `FB:2002-2153`, `3030-3290`

- Se abre con `grid-template-columns: minmax(0,1fr) 0fr → minmax(0,1fr) 168px` en **200 ms** smooth-out (`FB:2440-2450`).
  Borde izquierdo hairline (transiciona de transparente en 200 ms). Contenido: `opacity 0→1; translateX(8px→0); blur(2px→0)` en **150 ms**
  al abrir / **100 ms** al cerrar. Padding `0 18px 0 10px`, gap 6px.
- **Pestañas** (grid 2×2, sticky arriba, fondo `--rb-bg1`, borde inferior hairline, gap 3px): **Clip · Textos · Fotos · Reun.**;
  `10px/600`, `padding 4px 2px`, radio 5, inactiva `--rb-muted`, activa texto `--rb-text` + fondo `rgba(240,240,234,.10)`.
  (Titles/aria: Portapapeles, Textos, Capturas, Reuniones.) El cuerpo cambia con `tabPanel` (125 ms).
- **Ítem `.recorte`**: ancho completo, `padding 5px 7px; border-radius 5px; background #262622; font 11.5px; line-height 1.35; máx 2 líneas`,
  hover `rgba(240,240,234,.10)`; cursor `grab`; draggable. Con nombre: `.recorte-nombre` 10.5px/600 `--rb-text` (Textos; Reuniones muestra la etiqueta
  "Resumen reunión DD-MM-AA" y debajo el título). Imagen: `padding 4px`, `img` 100 % ancho, máx alto 70px, radio 3, `object-fit: cover`.
- **Vacíos:** Clip "Todavía no copiaste nada" · Textos "No tienes textos guardados" · Fotos "Todavía no hay capturas" · Reun. "No hay reuniones con resumen"
  (11.5px `--rb-muted`).
- Muestra máx 40 ítems de Clip y Fotos.

### Vista previa antes de insertar — `FIP` (todo el archivo)

`.velo` cubre el tablero (`position:absolute; inset:0; z-index:20; padding:16px; background: color-mix(--rb-bg1 55%, transparent)`),
fade 125 ms. `.carta`: `width:min(420px,100%); max-height:min(520px,100%); padding:12px; gap:10px; background:#1e1e1b; border:1px solid hairline;
border-radius:8px; box-shadow:0 12px 40px rgb(0 0 0 / 28%)`; entra `opacity 0→1, scale .96→1, blur 2px→0` en 125 ms smooth-out; sale 75 ms.
Contenido: kicker **"Esto se va a añadir al tablero"** (11px/600 muted) → cuerpo (texto en `pre` 12.5px/1.45, o nombre 13px/600, o imagen máx 280px) →
botones a la derecha: **"Cancelar"** (`rb-btn-ghost`) y **"Añadir"** (`rb-btn-soft`). Al confirmar el ítem cae en el **centro visible** del papel, queda seleccionado y (si es texto) recibe foco.

### Tira de páginas — `FB:2156-2257`, `2569-2739`

`.tira`: `flex; center; gap 6px; padding 6px 18px 10px`. Cada `.mini`: **ancho 84px** (`--mini-w`), `aspect-ratio 4/3` (→ 84×63), `border:1px solid hairline`,
radio 5, fondo `--rb-surface-elevated` `#2a2a26`. Hover: `translateY(-2px)` en 125 ms. **Activa:** `border-color:#f0f0ea; box-shadow:0 0 0 1px #f0f0ea`.
Contenido de la miniatura = barritas (texto: renglones de 2px con `rgba(240,240,234,.38)`, último al 55 %; lista: barritas + casilla 4px), fotos reales y la tinta como SVG.
Número de página abajo-derecha: `9px/600 muted` con `text-shadow: 0 0 3px #2a2a26`. Botón **"+"** ("Añadir página"): mismo tamaño, `border-style:dashed`, icono `Plus` 15px muted, hover borde/color `--rb-text`.
Botón **×** ("Quitar la página vacía") 16×16 en `top:-5px; right:-5px` solo sobre la última página si está vacía (icono `X` 10px).

### Píldora "Guardado" — `WFS:496-512`

`position:absolute; bottom:14px; left:18px; padding:4px 9px; border-radius:999px; background:#1e1e1b; color:#6faf88; font-size:11.5px;
box-shadow:0 2px 10px rgb(0 0 0 / 20%); gap:4px`; icono `Check` 12px; texto **"Guardado"**; entra/sale con `emerge`. Visible 1.6 s tras cada autosave (350 ms de debounce).

---

## B.5 Iconos usados (Lucide 1.29, vía `morphicons` → `<MorphIcon>`; `$ui/Icon.svelte`)

Render base (`node_modules/morphicons/dist/MorphIcon.svelte:71-85`, `Icon.svelte:14-32`):
`<svg xmlns width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">`.
**El `stroke-width` por defecto de la app es 1.75, no 2.** Tamaños: 15 en la barra, 14 export/×/panel, 13–14 en el float de Textos, 12 agarre/check/search,
11 quitar-bloque/AppWindow del encabezado, 10 la × de la tira.

Contenido interior de cada `<svg>` (exacto, extraído de `node_modules/lucide/dist/esm/icons/*.mjs`):

| Icono (nombre en código → lucide) | Interior SVG |
| --- | --- |
| MousePointer2 | `<path d="M4.037 4.688a.495.495 0 0 1 .651-.651l16 6.5a.5.5 0 0 1-.063.947l-6.124 1.58a2 2 0 0 0-1.438 1.435l-1.579 6.126a.5.5 0 0 1-.947.063z"/>` |
| Pencil | `<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>` |
| Highlighter | `<path d="m9 11-6 6v3h9l3-3"/><path d="m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4"/>` |
| Eraser | `<path d="M21 21H8a2 2 0 0 1-1.42-.587l-3.994-3.999a2 2 0 0 1 0-2.828l10-10a2 2 0 0 1 2.829 0l5.999 6a2 2 0 0 1 0 2.828L12.834 21"/><path d="m5.082 11.09 8.828 8.828"/>` |
| Type | `<path d="M12 4v16"/><path d="M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2"/><path d="M9 20h6"/>` |
| ListChecks | `<path d="M13 5h8"/><path d="M13 12h8"/><path d="M13 19h8"/><path d="m3 17 2 2 4-4"/><path d="m3 7 2 2 4-4"/>` |
| Undo2 | `<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>` |
| Redo2 | `<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"/>` |
| Minus | `<path d="M5 12h14"/>` |
| Plus | `<path d="M5 12h14"/><path d="M12 5v14"/>` |
| Trash2 | `<path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>` |
| Download | `<path d="M12 15V3"/><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><path d="m7 10 5 5 5-5"/>` |
| PanelRightOpen | `<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/><path d="m10 15-3-3 3-3"/>` |
| PanelRightClose | `<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M15 3v18"/><path d="m8 9 3 3-3 3"/>` |
| ArrowLeft | `<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>` |
| GripVertical | `<circle cx="9" cy="12" r="1"/><circle cx="9" cy="5" r="1"/><circle cx="9" cy="19" r="1"/><circle cx="15" cy="12" r="1"/><circle cx="15" cy="5" r="1"/><circle cx="15" cy="19" r="1"/>` |
| X | `<path d="M18 6 6 18"/><path d="m6 6 12 12"/>` |
| Check | `<path d="M20 6 9 17l-5-5"/>` |
| LoaderCircle | `<path d="M21 12a9 9 0 1 1-6.219-8.56"/>` (rota 360° en 900 ms lineal infinito) |
| AppWindow | `<rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 4v4"/><path d="M2 8h20"/><path d="M6 4v4"/>` |
| FileImage (PNG) | `<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><circle cx="10" cy="12" r="2"/><path d="m20 17-1.296-1.296a2.41 2.41 0 0 0-3.408 0L9 22"/>` |
| Camera (JPEG) | `<path d="M13.997 4a2 2 0 0 1 1.76 1.05l.486.9A2 2 0 0 0 18.003 7H20a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V9a2 2 0 0 1 2-2h1.997a2 2 0 0 0 1.759-1.048l.489-.904A2 2 0 0 1 10.004 4z"/><circle cx="12" cy="13" r="3"/>` |
| FileType (PDF) | `<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M11 18h2"/><path d="M12 12v6"/><path d="M9 13v-.5a.5.5 0 0 1 .5-.5h5a.5.5 0 0 1 .5.5v.5"/>` |
| FileText (Word) | `<path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>` |
| Presentation (PowerPoint) | `<path d="M2 3h20"/><path d="M21 3v11a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V3"/><path d="m7 21 5-5 5 5"/>` |

Plantilla: `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">…</svg>`.
Nota: `MorphIcon` fusiona todos los trazos en un único `<path>`; visualmente es idéntico.

---

## B.6 Contenido de ejemplo realista (INVENTADO; no hay datos de ejemplo en el repo)

Tablero (página 1, papel 1200×900, tinta roja `#e5483f` + una nota naranja):

- Texto 280×96 en `(24, 24)`: `Revisión semanal — planta Sur`
- Lista 260×120 en `(24, 140)`, ítems: `Revisar alarmas de anoche` (marcado) · `Confirmar visita a terreno` · `Enviar minuta al cliente`
- Texto 280×96 en `(340, 24)`: `Preguntar por el sensor de pH que no reporta desde el lunes`
- Trazo a lápiz: un círculo (≈120 px de diámetro) alrededor de ese texto y una flecha; un subrayado con resaltador amarillo/arena `#d6b48a` (alfa 0.4).
- Una imagen pegada 480×300 (captura de gráfico) en `(340, 160)`.
- Página 2 vacía (miniatura con "2" y el "+" a la derecha).

Ventana de fondo (frente de la tarjeta): cualquier app; para la píldora usa `sobre Google Chrome` o `sobre Excel`.

---

# PARTE A: TEXTOS (float de Textos + Notas)

## A.1 Qué es

La herramienta **"Textos"** de la pill (`core/tools.ts`: label "Textos", short "Guardados a mano", blurb "Los textos que escribes siempre, listos para pegar.
Más un bloc para notas sueltas.", acción "Ver textos"). Icono en la rueda: **`AlignLeft`** (`lib/icons.ts:TOOL_ICONS.snippets`; interior SVG
`<path d="M21 5H3"/><path d="M15 12H3"/><path d="M17 19H3"/>`). Es un **float independiente del overlay** con tres pestañas:
**Tablero · Textos · Notas** (`SF:595-625`). Pegar un texto cierra el float y escribe en la app de atrás (`SL:53-65`). La expansión por trigger
tipado **no está implementada** (`Features/snippets.md`). Los textos se crean/editan en la ventana principal (labels: "Nuevo texto", "Pegar en la app activa",
`ES:1235-1237`); el float solo lista y pega.

Pestaña inicial: **"Tablero"** (`SF:96`, `tab = "board"`; comentario: "Textos es la puerta a sus páginas").

## A.2 Cómo emerge de la pill (acto B "fused grow → separate", `Features/pill-liquid-emerge.md`, `SF:102-320`)

Panel fijo de **312×372 px** (`panel_float.rs:13-16`, `PANEL_SHAPE`). Fases:

| Fase | Qué se ve | Duración / parámetro |
| --- | --- | --- |
| previo | La pill vuela al slot (~110 ms `--flight-dur`) | — |
| `expand` | Nace una **semilla-disco de 40×40** (`PANEL_GROW_SEED`) **solapada 20 px** sobre el borde de la pill (`SEED_OVERLAP_PX`), fundida a ella (un solo blob). Hold 60 ms + 2 frames. Luego **crece width/height** a 312×372 con el borde hacia la pill clavado | `--launcher-bar-open-dur` **100 ms**, `--ease-smooth-out` (`SF:729-735`) |
| `separate` | El panel se aleja de la pill hasta un hueco de **16 px** (`PANEL_RESTING_GAP_PX`): el cuello del líquido se estira y **corta** (REACH = 12 px con BLEND 24) | `--launcher-separate-dur` **90 ms** (`left/top`), ancho/alto 75 ms |
| `ready` | Interacción normal | — |
| cierre | `approach` (vuelve a 2 px = `FUSED_GAP_PX`, a tamaño lleno, 90 ms) → `dismiss` con fade de opacidad **100 ms** (no hay "shrink" a semilla: el panel llega a la pill y se apaga) | `SF:286-320` |

Detalles: durante `expand`/`shrink`, cabecera y cuerpo tienen `opacity:0` (solo se ve la silueta líquida); el contenido aparece al empezar `separate`
(sin transición propia: "pop"). Opacidad global del float: 100 ms al cerrar, **150 ms** al abrir. Constantes de la simulación líquida: `BLEND=24`, `CELL=6`, `SMOOTH=4` (`lib/liquid/constants.ts:20-31`).
El lado (arriba/abajo/izq/der) depende de dónde esté la pill: NO ENCONTRADO un valor único.

**Aspecto de la caja** (lo pinta la "piel" líquida, no CSS): rectángulo redondeado **radio 20 px** (`CORNER = 20`, `SF:72`),
relleno **`--skin`** = **`#1a1a18`** oscuro / `#f7f7f2` claro, **sin borde** (solo un `stroke` de 1.25 px del mismo color), con
`filter: drop-shadow(0 10px 22px rgb(0 0 0 / 38%))` (`Skin.svelte:100-125`). El `.sf` en sí es transparente (`SF:709-712`), `border-radius:18px` solo recorta contenido.

## A.3 Estructura y dimensiones del float (`SF:569-689`, estilos `691-890`)

```
.sf  (312×372, padding .45rem .5rem .55rem = 7.2px 8px 8.8px; flex column; color --text)
├─ header.sf-head (min-height 2rem=32px; gap .35rem=5.6px; margin-bottom .35rem)
│   ├─ .sf-tabs (gap .3rem=4.8px): [Tablero] [Textos] [Notas]
│   ├─ .sf-drag (flex:1)   ← zona de arrastre (mover el panel)
│   └─ .sf-acts (gap .15rem=2.4px): [Pin 13px] [PanelTopClose 14px] [X 14px]
└─ .sf-body (flex:1) → .sf-pane (una por pestaña, con tabPanel 125 ms)
```

| Elemento | Valores |
| --- | --- |
| Pestaña `.sf-tab` | `min-height:1.75rem`=28px; `padding:.2rem .55rem` (3.2×8.8px); `border-radius:999px`; `font: 600 .6875rem` = **11px**; inactiva `color --muted #a8a89e`, sin fondo; **activa** `color --accent #e8e8e0`, `background: color-mix(--accent 12%, transparent)` = `rgba(232,232,224,.12)`; transición 75 ms; `:active` `scale(.96)` |
| Botón icono `.sf-icon` | 28×28 (`1.75rem`), `border-radius:.4rem`=6.4px, color `--faint #8f8f86`; hover/`is-on` (pin activo) color `--text #f0f0ea` + fondo `color-mix(--text 8%, transparent)` = `rgba(240,240,234,.08)`; `:active` scale .96; transición 75 ms |
| Tooltips (`use:tip`) | aparecen a los **450 ms**; `.tip`: `padding .26rem .46rem; radius .4rem; font 500 .72rem (11.5px)/1.3; bg ≈ #1a1a18; border 1px rgb(240 240 234 / 8%)(80 % de --line); shadow 0 8px 22px rgb(0 0 0 / 32%)`; fade 125 ms (`TipHost.svelte:78-110`) |
| Textos de tooltip | Pin: **"Fijar arriba"** / **"Desfijar"** · PanelTopClose: **"Volver a la pill"** · X: **"Cerrar"** |
| aria-label del diálogo | "Textos y notas" |

### Pestaña "Tablero" (`FPL`)

`.fp`: `padding 6px`, scroll vertical. Fila `.fp-item`: `flex; align-items:center; gap:10px; padding:6px; border-radius:8px`, hover fondo `--rb-bg0` `#121211` (125 ms).
Miniatura `.mini` **64px de ancho**, `aspect-ratio 4/3` (64×48), borde hairline, radio 5, fondo `#2a2a26` (mismo dibujo que la tira: barritas, fotos, tinta).
Título **"Página {n}"** 12px/600; debajo el primer renglón de texto de esa página (11px `--rb-muted`, una línea con `…`) o **"Página en blanco"**.
Tooltip nativo: "Voltear la ventana del frente y abrir la página {n}". Vacío (centrado, 12px muted, padding 12×8):
**"El tablero está vacío. Voltea una ventana para empezar a anotar."** Clic → cierra el float y voltea la ventana del frente abriendo esa página.

### Pestaña "Textos" (`SL` en modo `compact` + `island`, `SL:82-153`, `155-355`)

```
.snip-list.is-island (gap .3rem=4.8px)
├─ .snip-toolbar (padding 0 .15rem) → label.snip-search: Search 12px + input
└─ ul.snip-items (gap .15rem=2.4px; scroll) → li > button.snip-item
```

| Elemento | Valores |
| --- | --- |
| Buscador | `height:1.6rem`=25.6px; `border-radius:999px`; `padding:0 .45rem`; fondo `rgba(240,240,234,.07)`; icono `Search` 12px `--rb-muted`; input `font-size:.625rem`=**10px**, color `--rb-text`. Placeholder **"Buscar por nombre o palabra…"** (sin estilo propio → color de placeholder por defecto de Chromium; NO ENCONTRADO valor explícito) |
| Ítem `.snip-item` | columna, `gap:.15rem`=2.4px; `border-radius:.45rem`=7.2px; `padding:.3rem .4rem`=4.8×6.4px; **fondo `--rb-bg0` `#121211`** (más oscuro que la piel `#1a1a18`; claro `#ecece6`); texto `--rb-text`; ancho completo |
| Nombre | `.75rem`=**12px**, weight **650** |
| Alias (si hay) | `.6875rem`=11px, color `--rb-accent` `#f0f0ea`, unidos con " · " |
| Vista previa | `.6875rem`=11px, color `--rb-muted` `#9a9a90`, primera línea no vacía, una línea con elipsis (máx 120 chars) |
| Hover | `border-color` sin borde definido ⇒ **sin cambio visible** (ojo: no inventar un hover). Tooltip con el cuerpo completo a los 450 ms |
| Pegando | `opacity:.6; pointer-events:none` |
| Vacío (sin textos) | 13px `--rb-muted`, margen `.5rem 0 0`: **"Guarda los textos que escribes seguido —tu firma, un saludo, una plantilla— y pégalos desde la pill sin volver a tipearlos."** |
| Sin resultados | **"Sin coincidencias."** · Cargando: **"Cargando…"** |

(Estos textos están **hardcodeados en español** en `SL:88-109`, no vienen de `es.ts`.)

Ejemplos realistas (INVENTADOS):

| Nombre | Alias | Cuerpo (vista previa = 1ª línea) |
| --- | --- | --- |
| Firma correo | firma · sig | `Saludos cordiales,` / `Camila Rojas` / `Ingeniera de proyectos · TSG Enviro` |
| Respuesta soporte | soporte | `Hola, gracias por escribirnos. Ya estamos revisando tu caso y te respondemos hoy mismo.` |
| Link agenda | agenda | `https://calendly.com/camila-rojas/30min` |
| Dirección oficina | oficina | `Av. Apoquindo 3000, piso 12, Las Condes` |
| Plantilla minuta | minuta | `Asistentes: …` / `Acuerdos: …` / `Próximos pasos: …` |

### Pestaña "Notas" (bloc, `SF:677-685`, `865-877`)

Un solo `textarea.sf-scratch` que llena el panel: `flex:1; border:0; border-radius:.45rem (7.2px); padding:.4rem .5rem (6.4×8px);
background: color-mix(--bg 80%, transparent)` = `rgba(18,18,17,.80)` sobre la piel ⇒ ≈ `#141412`; `color --text`; `font-family: inherit; font-size:.75rem` = **12px**;
sin resize ni outline. Placeholder **"Notas temporales…"** (aria "Bloc de notas"; color de placeholder por defecto de Chromium). Guarda solo con debounce de **600 ms** (`domain/snippets.svelte.ts:11`).
Ejemplo de contenido: `Llamar a proveedor de sensores` / `Cambiar la clave del wifi de la planta` / `- pedir cotización 2 repuestos`.

## A.4 Vistazo de Textos en la pill (`SP`, hover sobre el gajo)

Panel de peek: `width:16rem`=256px; `padding:.55rem .7rem .6rem`; radio 20 (piel); `font-size:.75rem`=12px, `line-height 1.35`; color `--text`
(`PillPeekHost.svelte:639-655`). Muestra los **3 últimos editados** (no los más usados): fila `padding .3rem .35rem; radius 10px; gap .05rem`; nombre 12px/600
(elipsis); cuerpo colapsado a una línea, 11px `--faint #8f8f86`; hover fondo `rgba(240,240,234,.10)` (75 ms). Enlace pie **"Ver todos los textos"** (11px `--faint`, hover `--text`).
Vacío: **"Todavía no tienes textos guardados"**. aria de cada fila: "Pegar {nombre}". Entra con `.float-emerge` (`scale .55` + viaje 18 px + blur 2 px → 150 ms, cierre 100 ms).

---

## C. Qué NO se encontró / advertencias para el render

1. No hay doc de producto del Flipboard (ni `Features/*.md`, ni plan): todo el "qué es" sale del código y de `ES`. Es un módulo marcado "prototipo".
2. No hay capturas de referencia del tablero ni del float de Textos en `docs/assets` (solo pill/notch/rueda). El color de la hoja en oscuro (`#656561`) es calculado, no observado.
3. El **flip NO emerge de la pill** ni tiene gajo en la rueda; solo atajo (Ctrl+Shift+B) o "Textos → Tablero → página".
4. No hay animación tipo "pasar hoja": las **páginas** son celdas de una tira horizontal navegadas con desplazamiento de 125 ms; el único "volteo" 3D es el de la ventana entera (B.3).
5. `overlay.windowFlip.hint` ("Esc o {shortcut} para volver") existe en `ES` pero **ya no se dibuja** (`WFS:80` comenta que el pie se eliminó).
6. Fuente real (Aptos/Segoe UI Variable) y su renderizado en el equipo de origen: no verificable desde el código.
7. Lado y posición exactos del float de Textos respecto a la pill: dependen de dónde esté la pill (`placePanelResting`), no hay valor fijo.
8. Estimaciones marcadas como ESTIMADO (alturas de la barra/tira, anchos de botones con texto) salen de las reglas CSS, no de mediciones.
