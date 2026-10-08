# Ficha visual: Pizarra (annotate) y Color (lupa) de Atic

Fuente: repo `atic`, rutas relativas a `apps/desktop/` salvo indicación. Tema oscuro (paleta `dark`, la primaria; `styles/palettes/atic/dark.css`).
Todo lo marcado **(calculado)** sale de sumar CSS, no de una medición en pantalla. Lo marcado **(no encontrado)** no existe en el código.

Abreviaturas de archivo: `AS` = `src/lib/surfaces/annotate/AnnotateSurface.svelte`, `AD` = `.../annotateDraw.ts`, `AM` = `.../annotateModel.ts`, `CL` = `src/lib/surfaces/color/ColorLoupeSurface.svelte`, `CM` = `src/lib/features/color/colorMath.ts`, `CO` = `src/lib/surfaces/capture/CaptureOverlaySurface.svelte`, `SH` = `src/lib/surfaces/shelf/ShelfSurface.svelte`.

---

## 0. Tokens resueltos (tema oscuro) y escalas

Definidos en `src/styles/palettes/atic/dark.css`, `screen.css`, `scales.css`, `motion.css` y `src/app.css` (líneas 150-180, 233).

| Token | Valor resuelto |
| --- | --- |
| `--bg` | `#121211` |
| `--surface` | `#1a1a18` |
| `--surface-2` | `#1e1e1b` (fondo de barra, popover y chip de la pizarra) |
| `--elevated` | `#262622` (fondo de cada grupo de botones) |
| `--text` | `#f0f0ea` |
| `--muted` | `#a8a89e` |
| `--faint` | `#8f8f86` |
| `--line` | `rgb(240 240 234 / 10%)` |
| `--line-strong` | `rgb(240 240 234 / 18%)` |
| `--accent` | `#e8e8e0` (en oscuro el acento es tinta invertida, no un color) |
| `--on-accent` | `#121211` |
| `--danger` / `--rec` | `#e85a52` |
| `--ok` | `#6faf88` |
| `--skin` (color de la "gota" líquida) | `#1a1a18` |
| `--rb-accent` (barra TTL del shelf, oscuro) | `#f0f0ea` (`app.css:351`) |
| `--screen-scrim` | `rgb(0 0 0 / 28%)` |
| `--screen-select` | `#5ec8ff` |
| `--screen-select-edge` | `rgb(0 0 0 / 55%)` |
| `--screen-chip` | `rgb(0 0 0 / 75%)` |
| `--screen-ink` | `#fff` |
| `--screen-backdrop` | `#111` |
| `--radius-xs / sm / md / lg / pill` | `5px / 8px / 14px / 20px / 999px` |
| `--shadow-card` | `0 1px 2px rgb(0 0 0 / 20%)` |
| `--shadow-pop` | `0 8px 24px rgb(0 0 0 / 32%)` |
| `--shadow-float` | `0 24px 70px rgb(0 0 0 / 45%)` |
| `--shadow-goo` (sombra de la gota) | `0 10px 22px rgb(0 0 0 / 38%)` |
| `--duration-micro / quick / fast / medium / slow` | `40 / 75 / 125 / 150 / 200 ms` |
| `--ease-out` | `ease-out` (`app.css:233`) |
| `--ease-smooth-out` | `cubic-bezier(0.22, 1, 0.36, 1)` |
| `--distance-micro / --distance-base` | `4px / 8px` |
| `--scale-large` | `0.96` |
| Fuente de app `--rb-font` | `"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif` (`app.css:77`, aplicada en `:root` `app.css:263`) |
| Mono `--rb-mono` | `"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace` |

Notas:
- Las ventanas flotantes (`/capture-annotate`, `/color-loupe`, `/capture-overlay`, `/capture-shelf`) van con fondo transparente y **sin** `.atic-root` (`routes/+layout.svelte:isFloating`). Heredan la fuente de `:root` (Aptos, con caída a Segoe UI Variable en Windows). La lupa fuerza `12px/1.4 "Segoe UI", sans-serif` (`CL:739`).
- Los overlays sobre pantalla del usuario (`screen.css`, shelf) **no** siguen al tema: valores fijos.

---

# A. PIZARRA (annotate en modo `board`)

## A.1 Flujo paso a paso

1. **Activación.** Atajo global `Ctrl+Shift+X` (`CmdOrCtrl+Shift+X`, `crates/core/src/config.rs:480`; configurable en Ajustes > Atajos) o el gajo "Pizarra" de la rueda de la pill (`toolActions.ts:92`, `startBoard()`). Etiqueta `tools.board`: "Pizarra", corto "Marcar la pantalla", blurb "Congela la pantalla y la marcas con flechas y círculos, ahí donde está.", acción "Dibujar". Pista de Ajustes: "Congela la pantalla y deja marcarla. Esc la saca." (`es.ts:279`). Icono del gajo: Lucide `Pencil`.
2. **Congelado.** Rust (`src-tauri/src/annotate.rs:273-325`) esconde la ventana, captura el **monitor donde está el cursor** (sin puntero del sistema), compone la pill encima de la foto y guarda `board.png`. Abre la ventana `capture-annotate` cubriendo ese monitor (sin decoraciones, `always_on_top`, sin barra de tareas). Resultado visual: la pantalla parece no haber cambiado, pero está congelada y con cursor en cruz (`crosshair`).
3. **Barra flotante.** Aparece de inmediato (antes de que cargue la imagen) centrada horizontalmente sobre el **área útil** del monitor (sin barra de tareas) y a 14 px bajo su borde superior. El lienzo se hace visible con un fundido de opacidad 0 a 1 en 75 ms (`.canvas` transition, `AS:1294`) cuando la imagen carga. Mientras carga se ve el chip "Cargando la captura…".
4. **Ayuda.** Chip pastilla centrado, 14 px por encima del borde inferior del área útil: "Arrastra para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra". Se va con el primer trazo (fundido 125 ms).
5. **Dibujar.** Herramienta por defecto: **Flecha**, color **#ff3b30**, grosor nivel **2**. Arrastrar con clic izquierdo crea la forma; al soltar queda fijada. Teclas 1-7 (o p/f/c/r/m/t/x) cambian de herramienta. Se puede arrastrar la barra desde cualquier punto (umbral 4 px; cursor `grab`/`grabbing`).
6. **Salida.** `Enter` (o botón Copiar) copia el PNG al portapapeles; `Ctrl+Enter` (o botón Guardar) lo guarda como captura nueva (aparece en el shelf). Aparece el chip "Copiada al portapapeles" o "Guardada como captura nueva" en color `--text`, y a los **900 ms** se cierra la ventana (`NOTE_MS`, `AS:109`). Esc cierra; con trazos pide confirmar una vez: el botón X se vuelve rojo "¿Descartar?".
7. Volver a pulsar el atajo con la pizarra abierta la cierra (`toggle_board`).

Sin sonido ni animación de salida propios en el código de la pizarra (no encontrado): la ventana desaparece de golpe tras el aviso.

Atajos dentro del editor (`AS:684-742`, `AM:338`): `1/P` Lápiz, `2/F` Flecha, `3/C` Círculo, `4/R` Rectángulo, `5/M` Resaltador, `6/T` Texto, `7/X` Recortar; `Ctrl+Z` deshacer; `Ctrl+Shift+Z` o `Ctrl+Y` rehacer; `Enter` copiar; `Ctrl+Enter` guardar; `Esc` cerrar (si el popover de estilo está abierto, primero lo pliega).

## A.2 Estructura y layout (modo pizarra)

```
<div .editor.is-board>            /* ventana = monitor, padding 0, fondo transparente, sin sombra */
  <div .bar>                      /* position:absolute; z-index:2; flotante */
    <div .group> 7 × <button .tool> </div>          /* herramientas */
    <div .group.style> <button .tool.style-toggle> (punto de color + chevron)
                       [popover: fila 6 colores + fila 3 grosores] </div>
    <div .group> deshacer · rehacer · (recorte reset, solo si hay recorte) </div>
    <div .spacer>                                    /* flex:1 */
    <div .group.is-actions> Copiar (primario) · Guardar (icono) · Cerrar (icono) </div>
  </div>
  <div .stage> <canvas .canvas> (100% x 100%, a resolución natural del monitor) </div>
  [<div .text-input> contenteditable, position:fixed, solo al escribir]
  <p .status>  /* chip pastilla; en pizarra: top = borde inferior del área útil, translate(-50%, calc(-100% - 14px)) */
</div>
```

Posición de la barra en pizarra (`AS:197-209`, `1381-1395`):
- `left = (focus.x + focus.width/2) / imgW * 100 %` (centro del área útil del monitor)
- `top = focus.y / imgH * 100 %` (borde superior del área útil)
- `transform: translate(calc(-50% + shiftX), calc(14px + shiftY))`
- Chip de estado: `left` igual, `top = (focus.y + focus.height) / imgH * 100 %`, `transform: translate(-50%, calc(-100% - 14px))`.
- Con un solo monitor a 100% de escala equivale a: barra centrada en X, a 14 px del borde superior de la pantalla (si la barra de tareas está abajo); chip de estado centrado, 14 px sobre la barra de tareas.

## A.3 Dimensiones exactas (px CSS)

| Elemento | Valor | Fuente |
| --- | --- | --- |
| Barra (`.editor.is-board .bar`) | padding `8px 10px`, radio 14, gap 8 entre grupos, fondo `#1e1e1b`, sombra `0 24px 70px rgb(0 0 0/45%)`, `outline: 1px solid rgb(240 240 234/10%)` inset (`outline-offset:-1px`) | `AS:1381-1395` |
| Ancho total de la barra | ~504 px **(calculado)**: 20 padding + 212 herramientas + 40 estilo + 62 undo/redo + 138 acciones + 4 gaps × 8. Con recorte activo +30 | calculado |
| Alto total de la barra | 46 px **(calculado)**: 30 de grupo + 16 padding | calculado |
| `.group` | padding 2, radio 8, gap 2, fondo `#262622` | `AS:1033` |
| Grupo de herramientas | 4 + 7×28 + 6×2 = 212 × 30 | calculado |
| `.tool` | 28 × 26, radio 5, icono 15 px | `AS:1055`, `788` |
| `.style-toggle` | 36 × 26, gap 3, contiene punto + chevron 11 px | `AS:1100`, `809` |
| Punto de estilo `.style-dot` | diámetro `6 + nivel×2` = **10 / 12 / 14 px** (nivel 1/2/3), `box-shadow: 0 0 0 1px rgb(0 0 0/25%) inset`, fondo = color elegido | `AS:807`, `1106` |
| Grupo undo/redo | 2 botones 28×26 (62 × 30). Tercer botón "reset recorte" (icono Crop) solo si hay recorte | `AS:850-884` |
| `.group.is-actions` | padding 0, fondo transparente, gap 6 | `AS:1042` |
| `.action` | alto 26, padding `0 10px`, radio 5, fuente 11 px / 500, gap 6, icono 14 px | `AS:1195` |
| `.action.is-icon` | 26 × 26 | `AS:1213` |
| Botón Copiar | icono `Copy` 14 + texto "Copiar" ~74 px de ancho **(calculado)** | |
| Popover de estilo | `top: calc(100% + 6px); left: 0`, padding 6, gap 4, radio 14, fondo `#1e1e1b`, sombra float, outline 1px line, anima `popover-in` | `AS:1119` |
| Fila de colores | 6 × `.swatch` 26×26, gap 2 → 166 px de ancho; popover total 178 × 68 **(calculado)** | `AS:1166` |
| Fila de grosores | 3 × `.width` 32×26; línea interna 18 px de largo × `valor×2` px de alto (2/4/6 px), radio pill, `box-shadow: 0 0 0 1px rgb(0 0 0/25%)` | `AS:1149-1159` |
| Chip de estado `.status` | padding `5px 10px`, radio pill, fuente 11 px, color `--muted` (`--text` si es aviso, `--danger` si error), fondo `#1e1e1b`, sombra float, outline 1px line, `max-width: calc(100% - 48px)` | `AS:1303` |
| Cursor sobre lienzo | `crosshair`; sobre barra `grab` (`grabbing` al arrastrar) | `AS:1292`, `1388` |

## A.4 Herramientas: orden, iconos, etiquetas

Orden en la barra (`AS:83-106`), todas `Icon size=15`, `stroke-width 1.75` (`ui/Icon.svelte`), title `"{Etiqueta} ({tecla})"`:

| # | id | Etiqueta (`page.annotate.*`) | Icono Lucide | Tecla |
| --- | --- | --- | --- | --- |
| 1 | pen | Lápiz | `Pencil` | 1 / P |
| 2 | arrow | Flecha | `MoveUpRight` | 2 / F |
| 3 | ellipse | Círculo | `Circle` | 3 / C |
| 4 | rect | Rectángulo | `Square` | 4 / R |
| 5 | highlight | Resaltador | `Highlighter` | 5 / M |
| 6 | text | Texto | `Type` | 6 / T |
| 7 | crop | Recortar | `Crop` | 7 / X |

Luego: botón de estilo (punto + `ChevronDown`, title "Color y grosor del trazo", aria "Color y grosor", deshabilitado con Recortar), `Undo2` ("Deshacer (Ctrl+Z)"), `Redo2` ("Rehacer (Ctrl+Shift+Z)"), [`Crop` reset: "Volver a la imagen entera", aria "Quitar el recorte"], espaciador, `Copy` + "Copiar" (title "Copiar al portapapeles (Enter)"), `Save` (title "Guardar como captura nueva; aparece en el estante (Ctrl+Enter)", aria "Guardar"), `X` (title "Cerrar sin guardar (Esc)", aria "Cerrar"; con trazos y tras un primer clic: texto "¿Descartar?" sobre fondo `--danger`).

Estados de botón (`AS:1074-1093`):
- reposo: fondo transparente, color `--muted #a8a89e`
- hover: fondo `rgba(240,240,234,0.10)` (`color-mix(text 10%)`), color `--text`
- activo (`is-on`): fondo `rgba(232,232,224,0.22)` (`color-mix(accent 22%)`), color `--text`
- `:active`: `scale(0.96)`
- deshabilitado: opacidad 0.35
- transición: `background, color, transform` 75 ms `ease-out`
- `.action`: hover `rgba(240,240,234,0.14)`; `.is-primary` (Copiar): fondo `#e8e8e0`, texto `#121211`; deshabilitado opacidad 0.4

### Iconos (Lucide 1.29.0, `viewBox="0 0 24 24"`, `fill="none"`, `stroke="currentColor"`, `stroke-linecap="round"`, `stroke-linejoin="round"`, `stroke-width="1.75"`; render vía `morphicons` `MorphIcon`)

```svg
<!-- Pencil -->
<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>
<!-- MoveUpRight -->
<path d="M13 5H19V11"/><path d="M19 5L5 19"/>
<!-- Circle -->
<circle cx="12" cy="12" r="10"/>
<!-- Square -->
<rect width="18" height="18" x="3" y="3" rx="2"/>
<!-- Highlighter -->
<path d="m9 11-6 6v3h9l3-3"/><path d="m22 12-4.6 4.6a2 2 0 0 1-2.8 0l-5.2-5.2a2 2 0 0 1 0-2.8L14 4"/>
<!-- Type -->
<path d="M12 4v16"/><path d="M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2"/><path d="M9 20h6"/>
<!-- Crop -->
<path d="M6 2v14a2 2 0 0 0 2 2h14"/><path d="M18 22V8a2 2 0 0 0-2-2H2"/>
<!-- Undo2 -->
<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 5.5 5.5a5.5 5.5 0 0 1-5.5 5.5H11"/>
<!-- Redo2 -->
<path d="m15 14 5-5-5-5"/><path d="M20 9H9.5A5.5 5.5 0 0 0 4 14.5A5.5 5.5 0 0 0 9.5 20H13"/>
<!-- Copy -->
<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>
<!-- Save -->
<path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7"/><path d="M7 3v4a1 1 0 0 0 1 1h7"/>
<!-- X -->
<path d="M18 6 6 18"/><path d="m6 6 12 12"/>
<!-- ChevronDown (size 11) -->
<path d="m6 9 6 6 6-6"/>
```

Origen: `node_modules/lucide/dist/esm/icons/*.mjs`, catálogo importado en `src/lib/icons.ts`.

## A.5 Colores de trazo y grosores

Colores (`AM:92-99`), en este orden; por defecto el primero:

| # | Hex | Halo de texto (`haloFor`, luminancia > 0.55 = negro) |
| --- | --- | --- |
| 1 | `#ff3b30` (rojo, por defecto) | `#ffffffcc` (lum 0.39) |
| 2 | `#ffcc00` (amarillo) | `#00000099` (lum 0.79) |
| 3 | `#34c759` (verde) | `#00000099` (lum 0.63) |
| 4 | `#0a84ff` (azul) | `#ffffffcc` (lum 0.45) |
| 5 | `#ffffff` (blanco) | `#00000099` (lum 1.00) |
| 6 | `#1c1c1e` (casi negro) | `#ffffffcc` (lum 0.11) |

Swatch (`AS:1166-1193`): botón 26×26; disco de 18 px (`inset: 4px`) con `box-shadow: 0 0 0 1px rgb(0 0 0/25%) inset`. Seleccionado: disco de 22 px (`inset: 2px`) más anillo doble `0 0 0 2px #1e1e1b, 0 0 0 3px #f0f0ea` (transición `inset` 75 ms).

Grosores: 3 niveles (1, 2, 3), por defecto **2**. Grosor real en px de imagen = `max(1, round(nivel × 2 × max(1, imgW/1280)))` (`AM:124`).

| Ancho de imagen | Nivel 1 | Nivel 2 | Nivel 3 |
| --- | --- | --- | --- |
| 1280 | 2 | 4 | 6 |
| 1920 | 3 | 6 | 9 |
| 2560 | 4 | 8 | 12 |
| 3840 | 6 | 12 | 18 |

Tamaño de texto por nivel: 18 / 26 / 38 px × `max(1, imgW/1280)`, redondeado.

## A.6 Estilo exacto de cada forma (`annotateDraw.ts`)

Regla común (`AD:73-79`): `strokeStyle = fillStyle = color`, `lineWidth = grosor`, `lineCap = round`, `lineJoin = round`. **No hay sombra, contorno ni degradado en ninguna forma** salvo el texto (halo) y el resaltador (multiply). El lienzo se pinta a resolución natural del monitor, encima de la imagen.

| Forma | Cómo se pinta |
| --- | --- |
| Lápiz | Trazo libre. Puntos con distancia mínima 2 px de imagen; se unen con `quadraticCurveTo` por punto medio (`AD:205-230`). Un toque solo = círculo del grosor (línea de largo cero con cap round). Cero sombra. |
| Flecha | Línea de `from` al **cuello** + triángulo relleno. `head = min(max(grosor×3.6, 10), largo)`; semiancho de la base = `head×0.45` (base total = 0.9×head); cuello = punta − dirección×head. Triángulo cerrado con `fill()`, mismo color. Ejemplo: grosor 6 → punta de 21.6 px de largo, base 19.4 px. (`AM:273`, `AD:232`) |
| Círculo | `ellipse` inscrita en el rectángulo arrastrado (`from`-`to`), solo trazo, sin relleno. |
| Rectángulo | `rect` arrastrado, solo trazo, esquinas con `lineJoin: round`, sin relleno. |
| Resaltador | Mismo trazo libre con `lineWidth = grosor × 5`, `globalAlpha = 0.3`, `globalCompositeOperation = "multiply"` (deja ver lo de abajo). |
| Texto | Fuente `600 {size}px "Segoe UI", system-ui, -apple-system, "Helvetica Neue", Arial, sans-serif`, interlineado 1.25, `textBaseline: top`. Primero `strokeText` con `lineWidth = max(2, size/7)` y color halo (tabla arriba, `lineJoin round`), luego `fillText` del color elegido. Mientras se escribe: `contenteditable` posicionado idéntico, con `text-shadow: 0 0 2px halo, 0 0 2px halo`. Enter confirma, Shift+Enter salto de línea, Esc cancela. |
| Recorte (arrastre en vivo) | Velo `#00000080` (negro 50%) fuera del recuadro + borde blanco `#ffffff` de `max(1, imgW/1280 × 2)` px. Mínimo 24 px. Al soltar, el lienzo pasa a medir el recorte (sin animación en el código). |

Nada de animación de dibujado: la forma sigue al puntero cuadro a cuadro (sin easing) y al soltar queda tal cual.

## A.7 Textos exactos (`es.ts:1772-1804`, `page.annotate.*`)

| Clave | Texto |
| --- | --- |
| tools | Herramienta |
| pen / arrow / ellipse / rect / highlight / text / crop | Lápiz / Flecha / Círculo / Rectángulo / Resaltador / Texto / Recortar |
| cropReset / cropResetTitle | Quitar el recorte / Volver a la imagen entera |
| color / colorSwatch / width / widthValue | Color / Color {swatch} / Grosor / Grosor {value} |
| style / styleTitle | Color y grosor / Color y grosor del trazo |
| undo / undoTitle | Deshacer / Deshacer (Ctrl+Z) |
| redo / redoTitle | Rehacer / Rehacer (Ctrl+Shift+Z) |
| copy / copyTitle | Copiar / Copiar al portapapeles (Enter) |
| save / saveTitle | Guardar / Guardar como captura nueva; aparece en el estante (Ctrl+Enter) |
| closeTitle | Cerrar sin guardar (Esc) |
| discard / discardQ | Descartar el dibujo / ¿Descartar? |
| help | Arrastra para dibujar · Enter copia · Ctrl+Enter guarda · Esc cierra |
| loading | Cargando la captura… |
| resize | Redimensionar |
| openFail / canvasFail | No se pudo abrir la captura / No se pudo leer el lienzo |
| copiedClip | Copiada al portapapeles |
| savedNew | Guardada como captura nueva |
| `page.common.close` | Cerrar |

## A.8 Animaciones de la pizarra

| Qué | Duración / curva | Fuente |
| --- | --- | --- |
| Botones (hover/activo/color) | 75 ms `ease-out`; `:active` `scale(0.96)` | `AS:1068` |
| Popover de estilo al abrir | `popover-in`: de `opacity 0; translateY(-4px)` a reposo, 75 ms `ease-out`. Sin animación de cierre (desaparece) | `AS:1133-1141` |
| Anillo de swatch (`inset`) | 75 ms `ease-out` | `AS:1184` |
| Lienzo listo | `opacity 0 -> 1`, 75 ms `ease-out` | `AS:1294` |
| Chip de estado | `opacity` y `transform` 125 ms `ease-out`. En pizarra la `transform` es fija (`translate(-50%, calc(-100% - 14px))`), solo hace fundido | `AS:1324`, `1401-1405` |
| Aviso de copia/guardado | visible 900 ms y cierra la ventana | `AS:109` |
| `prefers-reduced-motion` | quita todas las transiciones | `AS:1446` |

## A.9 Modo panel (Dibujar desde el shelf; contexto)

Misma barra pero **no flotante**: ventana propia del tamaño de la captura. `CHROME_H 44`, `PADDING 10`, ocupa hasta 92% del área útil, mínimo 560×260 (`annotate.rs:37-52`). `.editor`: padding 10, radio 14, fondo `#1e1e1b`, sombra float, outline 1px line, gap 8. El `.stage` lleva damero: `repeating-conic-gradient(rgba(240,240,234,.04) 0% 25%, transparent 0% 50%) 50% / 16px 16px`, radio 8. Canvas con radio 5. Asa de redimensionado `.grip` 16×16 abajo a la derecha (dos diagonales, `--muted`, opacidad .5). Ventana angosta (<=600 px): Copiar queda solo con icono.

---

# A2. Selección de región de captura (`CaptureOverlaySurface.svelte`) — paso previo al shelf

Atajo `Ctrl+Shift+4`. Rust congela un JPEG del escritorio; la ventana lo pinta a pantalla completa (`object-fit: fill`), cursor del sistema oculto (`cursor:none`), **puntero propio** (SVG abajo). Detalle en `CO`.

Flujo: 1) atajo; 2) velo `rgb(0 0 0/28%)` sobre todo (fade `opacity` 125 ms `ease-smooth-out` al revelar); 3) mover el mouse ilumina la **ventana** bajo el cursor (agujero recortado con borde punteado cian); 4) arrastrar >4 px cambia a **región**; `Espacio` = monitor entero; `Enter` confirma; `Esc` o clic derecho o clic en vacío cancela; 5) al soltar, el recorte se **levanta y vuela** al lugar del shelf.

| Elemento | Estilo (`CO`) |
| --- | --- |
| Velo | `--screen-scrim` `rgb(0 0 0/28%)`; el "agujero" es una `box-shadow: 0 0 0 100000px` |
| Borde de selección `.cap-hole` | `2px dashed #5ec8ff`; `box-shadow: 0 0 0 1px rgb(0 0 0/55%), inset 0 0 0 1px rgb(255 255 255/22%), 0 0 0 100000px rgb(0 0 0/28%)` |
| Vértices en L `.cap-v` | tamaño `min(22px, 32cqw/cqh)`, borde 3 px sólido `#5ec8ff`, `filter: drop-shadow(0 0 0.6px rgb(0 0 0/55%))`; posicionados a -2 px de cada esquina |
| Chip de medidas `.cap-size` | encima de la esquina superior izquierda (`translateY(calc(-100% - 6px))`); fondo `rgb(0 0 0/75%)`, texto `#fff`, 12 px/1.25, mono `Cascadia Mono`, tabular, padding `2px 8px`, radio 5, outline `1px rgb(255 255 255/10%)`; texto `{w} × {h}` redondeado (ej. `1280 × 720`). Entrada: `cap-chip-in` 125 ms `ease-smooth-out` de `opacity 0` y 4 px más arriba |
| Nombre de ventana `.cap-name` | igual que el chip, peso 650, encima del de medidas (gap 4); solo en modo ventana |
| Ayuda `.cap-help` | `position: fixed; bottom: 2rem (32px)`; **sigue al cursor solo en X** (`left = cursor.x`, `translateX(-50%)`); padding `6px 14px`, radio 8, fondo `rgb(0 0 0/75%)`, texto `#fff` 14 px, sombra `0 8px 24px rgb(0 0 0/28%)`, outline `1px rgb(255 255 255/10%)`. Aparece con `opacity 0->1` y `translateY(4px)->0`, 125 ms `ease-smooth-out`, con delay 40 ms |
| Texto de ayuda | "Clic: ventana · Arrastrar: región · Espacio: pantalla · Esc cancela" (`page.captureHud.help`, `es.ts:1808`) |
| Puntero propio `.cap-pointer` | SVG 18×24 que sigue al cursor (`transform: translate(x,y)`) |
| Vuelo `.cap-fly` | Fase 1 (**320 ms**, `ease-smooth-out`): `translateY(-14px) scale(1.03)` y `--shadow-float`, radio 7 (`5+2`), outline `rgb(255 255 255/18%)`. Espera 320 ms. Fase 2 (**150 ms** `ease-smooth-out`): `left/top/width/height` van al rect de la miniatura del shelf, `--shadow-pop`, radio 5, y `cap-fly-blur` (blur 0 -> 4px al 40% -> 0). Fondo tras el vuelo: solo velo `rgb(0 0 0/28%)`, sin agujero. Ayuda y puntero se ocultan |
| Cancelación | solo se funde el velo (125 ms), sin vuelo |

```svg
<!-- Puntero propio del overlay de captura (CO:617-625) -->
<svg width="18" height="24" viewBox="0 0 18 24" fill="none">
  <path d="M1.2 1.2 1.2 20.2 6.1 15.4 9.4 23.1 12.2 21.9 8.8 14.1 16.2 13.9Z"
        fill="#fff" stroke="#111" stroke-width="1.4" stroke-linejoin="round"/>
</svg>
```

---

# A3. Shelf flotante de capturas (`ShelfSurface.svelte`)

Aparece tras cada captura (y tras guardar desde la pizarra). Ventana `capture-shelf`: **208 × 136**, sin marco, transparente, siempre encima (`tauri.conf.json:27-42`), colocada a **16 px** de la esquina inferior derecha del área útil (izquierda si Ajustes > Capturas > "De qué lado" = Izq.; `floating.rs` `CORNER_MARGIN 16`). Miniatura 192 × 120 con `--shelf-pad: 8px` de margen (`SH:71-73`).

| Elemento | Estilo |
| --- | --- |
| Aparición | `opacity 0->1`, `translateY(8px) scale(0.96)` -> reposo; origen `100% 100%`; **150 ms** `ease-smooth-out` al entrar; **125 ms** al salir (`SH:948-1000`). Se va sola a los 20 s (`capture_shelf_timeout_seconds`, 0 = no se va); pausa con hover |
| Miniatura `.shelf-thumb` | 192×120, fondo `rgb(8 8 7/94%)`, radio 8, `box-shadow: 0 4px 12px rgb(0 0 0/38%)`, `outline: 1px solid rgb(255 255 255/16%)` inset, imagen `object-fit: contain` centrada. Cursor `grab`; `:active` `scale(0.98)` (75 ms) |
| Velo de hover `.shelf-veil` | cubre la miniatura (inset 8), radio 8, `rgb(0 0 0/38%)`, `opacity` 0->1 en 75 ms `ease-out` al hover |
| Botones esquina `.shelf-dot` | 28×28 círculos, fondo `rgb(18 18 16/82%)`, icono `#f4f4ee` 14 px, sombra `0 1px 2px rgb(0 0 0/40%)`, outline `rgb(255 255 255/16%)`. Arriba-izq. (top 8 / left 8): `X` (title "Descartar"); arriba-der.: `Folder` ("Carpeta"). Aparecen con hover: opacidad 0->1, `scale(.96)->1`, 75 ms |
| Acciones centrales `.shelf-center` | columna centrada (gap 5, ancho mín. 5.5rem = 88 px): **Copiar** (`Copy` 13) / **Dibujar** (`Pencil` 13) / **Texto** (`ScanText` 13). Cada `.shelf-sub`: alto mín. 26, padding `0 12px`, radio 8, fondo `rgb(18 18 16/78%)`, texto `#f4f4ee` 12 px / 650, outline `rgb(255 255 255/16%)`; hover fondo `rgb(8 8 7/90%)`. Entran con `opacity 0->1` y `translateY(4px)->0`, 75 ms |
| Aviso `.shelf-note` | pastilla centrada sobre la miniatura: fondo `rgb(18 18 16/82%)`, 11 px / 650, padding `5px 10px`; ok `rgb(157 255 196)`, error `rgb(255 180 173)`. Textos: "Copiada al portapapeles" (copiar, se ve 900 ms y cierra), "Texto copiado al portapapeles" / "No se encontró texto en la captura" / "No se pudo extraer el texto" (OCR) |
| Barra TTL `.shelf-ttl` | abajo, alto 2, `left/right 8`, `bottom 4`, `color-mix(--rb-accent 75%, transparent)` = `rgba(240,240,234,.75)` en oscuro; `scaleX(1)->0` lineal durante el TTL; pausa en hover |
| Arrastre | Al arrastrar >5 px la miniatura queda al 20% de opacidad y un **ghost** 192×120 (`drop-shadow(0 4px 8px rgb(0 0 0/40%))`) sigue al cursor; soltarlo al borde (56 px) lo descarta (vuela 280 px y desvanece 125 ms); soltarlo sobre otra app pega la imagen |
| Textos | `shelf.recent` "Captura reciente", `shelf.open` "Abrir la captura {label}", `shelf.text` "Texto", `shelf.copy` "Copiar", `shelf.draw` "Dibujar", `shelf.folder` "Carpeta", `shelf.dismiss` "Descartar", `shelf.copied` "Copiada al portapapeles", `shelf.reading` "Leyendo…", `shelf.tip` "Clic: abrir · Arrastra para soltar · Al borde o Esc: descartar" (`es.ts:1022-1032`; el tip es solo lector de pantalla, oculto visualmente) |

Iconos usados (Lucide, mismo formato): `X`, `Folder`, `Copy`, `Pencil`, `ScanText`:

```svg
<!-- Folder --><path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"/>
<!-- ScanText --><path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/><path d="M7 8h8"/><path d="M7 12h10"/><path d="M7 16h6"/>
```

## A3.b Vistazo de Capturas en la pill (`CapturesPeek.svelte`)

Panel al pasar el ratón por el gajo Capturas: cuadrícula de **3 miniaturas** (últimas), `grid-template-columns: repeat(3, minmax(0,1fr))`, gap `0.4rem`; cada una `aspect-ratio 4/3`, radio 8, `inset 0 0 0 1px rgba(240,240,234,.12)`, `object-fit: cover`; etiqueta debajo 11 px (`0.6875rem`) `--faint`, centrada (hora, tabular). Hover: `scale(1.04)` (75 ms `ease-smooth-out`). Clic copia (etiqueta pasa a "Copiado" en `--ok #6faf88`, cierra a 650 ms); arrastrar la suelta como archivo. Enlace inferior "Nueva captura" (11 px `--faint`, hover `--text`). Vacío: "Todavía no hay capturas". En vertical: una columna, `aspect-ratio 16/9`. Textos `pill.peek.*` (`es.ts:761-764`): `copied` "Copiado", `captureCopy` "Copiar la captura de las {label}". Sin fondo propio: lo pinta la piel de la isla.

---

# B. COLOR (cuentagotas + lupa)

## B.1 Flujo paso a paso

1. **Activación.** Atajo `Ctrl+Shift+C` (`CmdOrCtrl+Shift+C`, `config.rs:481`, hint Ajustes: "Lee el píxel bajo el cursor y lo copia. R abre la rosa.") o el gajo "Color" de la rueda (`startColorPicker()`, icono Lucide `Pipette`). Textos de herramienta: label "Color", corto "Cuentagotas", blurb "Elige un color de la pantalla, píxel a píxel, o desde la rosa cromática. Se copia al portapapeles.", acción "Elegir color".
2. **No se congela la pantalla**: se lee en vivo. La pill queda visible (click-through). Aparece la **lupa** (una "gota" líquida oscura) al lado del cursor: la esquina de la gota queda a **28 px** del cursor, abajo-derecha; si no cabe se voltea a la izquierda/arriba (`OFFSET 28`, `color_picker.rs:29,1340-1350`). Sigue al cursor (bucle 16 ms; muestreo cada 33 ms, ~30 fps).
3. **Lectura.** Muestra un parche de **13×13 px** de pantalla, ampliado. El botón "valor" muestra el hex del píxel central, sobre un fondo de ese mismo color. Mientras no llega el primer parche: cuadro apagado (opacidad .45) y texto "Leyendo color…".
4. **Copiar.** Clic izquierdo en cualquier lado (un hook global se traga el clic), o Enter: copia el valor (por defecto hex, ej. `#3A82F6`) al portapapeles como texto, suena el beep de captura si "sonidos de UI" está activo, y la lupa se despide con destello (B.6). No hay toast; el color se guarda en "recientes" (máx. 8, `localStorage` clave `atic-color-recent`) y el vistazo de la pill lo muestra.
5. **Editar (rosa cromática).** `R` (o botón "Editar") despliega debajo la rosa: rueda de matiz + cuadro saturación/brillo + sliders + entrada HEX + 12 matices + recientes + botón "Copiar {valor}". `R` otra vez (botón "Volver") la pliega. En rosa el muestreo en vivo se pausa y se edita el color congelado.
6. **Cancelar.** `Esc` o botón "Cancelar · Esc": la gota se encoge y se desvanece sin copiar.

## B.2 Ventana y layout

- Ventana `color-loupe`: transparente, sin marco, siempre encima, sin foco (`tauri.conf.json:61-75`). Tamaño compacto **312 × 160** = contenido 264×112 + `PAD 24` por lado (`color_picker.rs:32-34`). Rosa abierta: **344 × 588** (296×540 + 48).
- `.stage`: `padding: 24px` (aire para la sombra), texto `--text #f0f0ea`, `font: 12px/1.4 "Segoe UI", sans-serif`, `user-select: none`.
- La **silueta** ("gota") no es CSS: es un SVG (`Skin.svelte`) que traza la unión suavizada (smooth-min `BLEND 24`, celda 6, 4 pasadas de suavizado) de dos formas medidas del DOM: el **cuerpo** (`.body`, rect con radio **24**) y el **lóbulo** (canvas de la lupa engordado **12 px** por lado, radio **22**), lo que crea un cuello líquido donde la lupa "asoma" por la izquierda. Relleno y trazo `var(--skin)` = `#1a1a18` (`stroke-width 1.25`), `fill-rule: evenodd`, `filter: drop-shadow(0 10px 22px rgb(0 0 0/38%))`. Mientras lee, respira: `filter: brightness(1.04 - 0.04·cos(2π·t/2.4s))`, es decir de 1.00 a 1.08 cada 2.4 s (`Skin.svelte:135`). **No lleva borde ni fondo CSS propio.**

```
.stage (312 x 160, padding 24)
└─ .body (padding 10px 14px 10px 30px; ~264 x ~103)   <- cuerpo de la gota, radio 24
   ├─ .preview (flex, gap 12, align center)
   │   ├─ canvas.grid  64x64  margin-left:-26  (asoma; lóbulo = +12 px por lado, radio 22)
   │   └─ .preview-content (flex 1, ~170 px)
   │       ├─ button.read   [swatch 12x12] [#3A82F6]     <- fondo = color, texto = tinta
   │       └─ .bar (margin-top 6, gap 8): <p.help> + button.rose-btn "Editar"
   ├─ (rosa, si está abierta)
   └─ button.cancel  "Cancelar · Esc"  (derecha, margin-top 5)
```

Posiciones **(calculadas)**: cuerpo x = 24..288 (relativo a la ventana); canvas x = 28..92 (queda 4 px dentro del borde izquierdo del cuerpo), lóbulo x = 16..104 (asoma 8 px por fuera); columna de contenido x = 104..274 (170 px). Alto del cuerpo ~103 px: 10 padding + 64 lupa + 19 cancelar + 10 padding.

## B.3 La lupa (píxeles ampliados)

| Propiedad | Valor | Fuente |
| --- | --- | --- |
| Parche de origen | **13 × 13** píxeles de pantalla alrededor del cursor (`GRID 13`) | `color_picker.rs:28` |
| Canvas interno | 130 × 130 (`CELL 10` × 13); cada píxel = celda de 10×10 rellena con `rgb(r,g,b)` | `CL:40-41`, `131-147` |
| Tamaño mostrado | **64 × 64 px** CSS (`flex: 0 0 64px`); aumento efectivo **~4.92×** por píxel de pantalla | `CL:897-902` |
| Escalado | `image-rendering: pixelated` (píxeles nítidos, sin suavizado) | `CL:911` |
| Retícula / píxel central | **No hay cuadrícula ni cruz.** Solo un cuadrado sobre la celda central (celda 6,6): `strokeRect(60+1, 60+1, 8, 8)` con `lineWidth 2`, color = tinta según luminancia del píxel central (`#111` o `#fff`, `inkOn`). En pantalla equivale a un marco ~3.9 px de lado exterior ~4.9 px | `CL:143-146` |
| Forma | `border-radius: 15px`; `outline: 1px solid rgba(240,240,234,.14)` (`color-mix(text 14%)`) | `CL:914-915` |
| Estado sin lectura | `opacity: .45` (transición 180 ms) | `CL:938` |
| Al empezar a leer | `sample-open` 320 ms `ease-smooth-out` (`cubic-bezier(.22,1,.36,1)`): de `opacity .35; scale(.86)` a `1` | `CL:920-935` |

Elección de tinta (`CM:224`): `inkOn` devuelve `#111` si el contraste con `#111` supera al de `#fff`, si no `#fff`. Aproximadamente, `#111` para colores claros, `#fff` para oscuros.

## B.4 Valor mostrado, formato y chip de color

Botón `.read` (`CL:964-982`, marcado `CL:476-488`):
- `display:flex; align-items:center; gap:8px; max-width:100%; padding:5px 8px; border:0; border-radius:6px; font: 600 12px/1.4 "Cascadia Mono", monospace; overflow-wrap:anywhere`.
- Fondo = **el color leído** (`style:background={hex}`), color de texto = tinta (`#111`/`#fff`).
- Contiene un chip `.swatch` de **12 × 12 px**, radio 3, fondo = mismo color, `outline: 1px solid currentColor`; y el texto del valor (`data-numeric`).
- Altura ~27 px **(calculado)**.

Formato: el estado `format` es `"hex"` por defecto y solo se cambia dentro de la rosa pulsando una de las 3 líneas de código; la compacta siempre muestra `formatColor(rgb, format)` (`CM:217`):

| Formato | Ejemplo exacto | Función |
| --- | --- | --- |
| hex (por defecto) | `#3A82F6` (mayúsculas, `#`, 6 dígitos) | `rgbToHex` |
| rgb | `rgb(58, 130, 246)` | `formatRgb` |
| hsl | `hsl(217, 91%, 60%)` (h redondeado, s y l en %) | `formatHsl` |

Lo que se copia es exactamente ese `value` (Rust lo valida y normaliza: hex a mayúsculas; `rgb(...)`/`hsl(...)` tal cual; `color_picker.rs:1148`). Se copia como **texto plano**, no como color.

Textos bajo el valor (`.bar`): `<p.help>` 10 px `--muted #a8a89e` "Clic o Enter: copiar · R: editar" (en rosa: "Ajusta el color · Enter: copiar · R: volver") y botón `.rose-btn` "Editar" (en rosa "Volver"): 11 px, padding `3px 6px`, radio 5, fondo `rgba(240,240,234,.08)`; hover / pulsado `rgba(240,240,234,.15)`. Botón `.cancel`: 10 px `--muted`, sin fondo, alineado a la derecha, "Cancelar · Esc".

## B.5 Rosa cromática (segunda vista, tecla R)

Ventana crece a 344×588; la gota se estira con CSS (`transition:slide` de Svelte, **220 ms**, easing por defecto de `slide` = `cubicOut`) y al cerrar Rust espera 260 ms antes de achicar la ventana. En rosa se ocultan la lupa y el botón `.read`.

| Elemento | Estilo | Fuente |
| --- | --- | --- |
| Contenedor `.rose` | `margin-top:14px; padding-top:12px; border-top:1px solid rgb(240 240 234/10%)` | `CL:1011` |
| Cabecera | swatch grande **40×40**, radio 8, outline `1px --line`, fondo = color (clic = copiar) + 3 líneas de código apiladas, 11 px/1.5 mono: `#3A82F6`, `rgb(58, 130, 246)`, `hsl(217, 91%, 60%)`; la activa `--text` peso 600, las demás `--muted` | `CL:1017-1052` |
| Anillo de matiz `.wheel` | **176 × 176**, centrado, `conic-gradient(from 0deg, red, yellow, lime, cyan, blue, magenta, red)` con máscara que deja un aro de **14 px** de grosor. Cero (rojo) arriba | `CL:1054-1079` |
| Perilla de matiz | 10×10 blanca, `box-shadow: 0 0 0 1px black`, `transform: rotate(matiz°) translateY(-81px)` | `CL:1081` |
| Cuadro S/V `.sv` | canvas **104 × 104** (render 112×112) a `inset 36px`, radio 6; x = saturación 0..1, y = brillo 1..0; sobre matiz actual | `CL:1095`, `149-169` |
| Perilla S/V | 10×10 blanca con anillo negro 1px, en `left = 36 + s×104`, `top = 36 + (1−v)×104` | `CL:611-615` |
| Sliders | dos filas grid `72px 1fr 36px`, gap 6, 11 px `--muted`, `accent-color: --text`: "Saturación" y "Brillo" con valor en % a la derecha | `CL:1116-1137` |
| Entrada HEX | etiqueta "HEX", input (fondo `--bg #121211`, borde `1px --line`, radio 5, padding `5px 7px`, mono, placeholder `#RRGGBB`, `maxlength 7`) + botón "Aplicar" (fondo `rgba(240,240,234,.08)`, radio 5, padding `6px 8px`) | `CL:1139-1163` |
| Matices | 12 círculos **17×17**, gap 6, con outline `1px --line`, a 0°,30°,…,330° (S=1, V=1) | `CL:1165`, `CM:234` |
| Recientes | etiqueta "Recientes" 10 px `--muted`, hasta 8 círculos 17×17 | `CL:1179-1188` |
| Botón grande `.copy` | ancho completo, padding `8px 10px`, radio 6, fondo `--text #f0f0ea`, texto `--bg #121211`, peso 600, hover opacidad .9; texto "Copiar {valor}" ("Copiando…" mientras copia) | `CL:1190-1204` |

Textos `page.colorHud.*` (`es.ts:1811-1836`): edit "Editar", back "Volver", loading "Leyendo color…", copying "Copiando…", copy "Copiar {value}", copied "Copiado {value}" **(clave existente pero sin uso en el código)**, apply "Aplicar", cancel "Cancelar · Esc", help "Clic o Enter: copiar · R: editar", helpRose "Ajusta el color · Enter: copiar · R: volver", hue "Matiz", saturation "Saturación", brightness "Brillo", hex "HEX", rgb "RGB", hsl "HSL", recent "Recientes", rose "Rosa", roseOpen "Abrir rosa cromática", roseClose "Cerrar rosa cromática", roseAria "Rosa cromática", invalidHex "Escribe seis dígitos hexadecimales, por ejemplo #3A82F6.", copyError "No se pudo copiar: {error}. Puedes reintentar.", protocolError "La interfaz y el motor de color no coinciden. Reinicia Atic.", timeout "El motor de color no respondió. Pulsa Esc para salir.".

## B.6 Animaciones de la lupa

| Momento | Animación | Fuente |
| --- | --- | --- |
| Entrada (`phase = in`, 300 ms) | `.stage`: `loupe-fade-in` **220 ms `ease-out`** (`opacity 0 -> 1`). `.body`: `loupe-grow` **260 ms `cubic-bezier(.22,1,.36,1)`** (`scale(.84) -> 1`, `transform-origin: 32px 32px`). La piel se deforma con el cuerpo porque se mide cuadro a cuadro. Escala en `.body`, opacidad en `.stage` (importante para no duplicar escala) | `CL:770-777`, `796-814` |
| Viva (`phase = live`) | La gota "respira" (brillo 1.00-1.08, ciclo 2.4 s) solo mientras muestrea y sin rosa abierta | `CL:123`, `Skin.svelte` |
| Abrir/cerrar rosa | `slide` 220 ms; la gota se estira con el contenido | `CL:60`, `517` |
| Cancelar (Esc) | `.stage.is-out`: `loupe-fade-out` **200 ms** `cubic-bezier(.4,0,1,1)` (`opacity 1 -> 0`); `.body`: `loupe-shrink` 200 ms mismo easing (`scale 1 -> .9`) | `CL:779-785`, `816-834` |
| **Copiado** (`is-out.is-copied`) | Destello: `loupe-flash` **200 ms** `cubic-bezier(.4,0,1,1)`: 0% `opacity 1`, **35% `filter: brightness(1.3)`**, 100% `opacity 0`. `.body`: `loupe-pop` 200 ms: 0% `scale 1`, **35% `scale(1.05)`**, 100% `scale(.94)`. La ventana se esconde a los 200 ms (`CLOSE_DELAY`). La gota se hincha y destella con el color leído aún visible, luego desaparece | `CL:787-865`, `color_picker.rs:39` |
| Nueva sesión en la misma ventana | `copied=false; phase=in` (nace de nuevo) | `CL:186-188` |
| `prefers-reduced-motion` | quita las animaciones de `.body` y de `.grid.is-ready` | `CL:868` |

Sonido al copiar: `beep::SoundAction::Capture` si `ui_sounds` está activo (`color_picker.rs:1123-1139`); no hay archivo/duración en el frontend.

## B.7 Vistazo de Color en la pill (`ColorPeek.svelte`)

Aparece al pasar el ratón por el gajo Color (icono `Pipette`). Contenido (columna, gap `0.4rem`):
- Lista de **recientes** (hasta 8, los que guardó `pushRecentColor`): botones circulares **1.6rem (25.6 px)**, `border-radius: 999px`, fondo = color, `box-shadow: inset 0 0 0 1px rgba(240,240,234,.18)`; gap `0.35rem`, con `flex-wrap`. Hover y "copiado": `scale(1.14)` (75 ms `ease-smooth-out`). En vertical: cuadrícula de 4 columnas de 1.6rem.
- Línea de hex: punto de `0.6rem` (9.6 px) del color + hex en mayúsculas tabular (muestra el apuntado, o el último si nada apuntado; al copiar añade " · Copiado" en `--ok #6faf88`).
- Enlace inferior "Tomar un color" (`0.6875rem` = 11 px, `--faint #8f8f86`, hover `--text`).
- Al copiar, el panel se cierra a los **650 ms**.
- Vacío: "Todavía no has tomado colores". Aria de cada muestra: "Copiar {hex}" (`es.ts:765-767`).
- Sin fondo propio: lo pinta la piel de la isla de la pill.

Icono `Pipette` (gajo del catálogo, `icons.ts`):

```svg
<path d="m12 9-8.414 8.414A2 2 0 0 0 3 18.828v1.344a2 2 0 0 1-.586 1.414A2 2 0 0 1 3.828 21h1.344a2 2 0 0 0 1.414-.586L15 12"/>
<path d="m18 9 .4.4a1 1 0 1 1-3 3l-3.8-3.8a1 1 0 1 1 3-3l.4.4 3.4-3.4a1 1 0 1 1 3 3z"/>
<path d="m2 22 .414-.414"/>
```

---

## C. Lo no encontrado o dudoso

- No hay sonido, animación de salida ni transición de "congelado" en la pizarra en el frontend (el congelado es instantáneo: la ventana aparece ya con la foto). Si el video necesita un "flash", es invención.
- Alto/ancho reales de la barra de la pizarra y de la gota de la lupa no están fijados en el código (se miden en el DOM). Los valores marcados **(calculado)** son estimaciones con fuente Aptos/Segoe 11-12 px; verificar con una captura si el pixel-perfect importa.
- El color exacto del contenido debajo (foto de escritorio) depende del usuario: el video deberá pintar su propio "escritorio" y componer la pill encima.
- `page.colorHud.copied` ("Copiado {value}") está definido pero no se muestra en ningún lugar.
- Icono de la barra usa `stroke-width 1.75` (por defecto de `ui/Icon.svelte`), no el 2 típico de Lucide.
- Fuente: Aptos solo existe si el equipo la tiene (Windows 11 reciente la trae); si no cae a Segoe UI Variable. La lupa usa Segoe UI explícito.
