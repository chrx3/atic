# Ficha visual: Launcher (Spotlight) y Clipboard (floats del overlay)

Fuente: codigo de `apps/desktop/src`. Rutas relativas a `apps/desktop/src/` salvo indicacion. Tema por defecto = **oscuro** (`data-theme="dark"`, `data-theme-base="dark"`). Nada de esto se vio corriendo: son valores leidos del CSS/TS. Lo no verificado esta marcado **[NO VERIFICADO]**.

Abreviaturas: `LF` = `lib/surfaces/overlay/launcher/LauncherFloat.svelte`, `CF` = `lib/surfaces/overlay/clipboard/ClipboardFloat.svelte`, `CHL` = `lib/ClipboardHistoryList.svelte`.

---

## 0. Base comun

### 0.1 Tipografia global
- `font-family` = `"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif` (`app.css:76-77` `--rb-font`, y `styles/scales.css:30-31` `--font-sans`; se aplica en `:root`, `app.css:263`). Monoespaciada: `"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace` (`styles/scales.css:33`).
- `-webkit-font-smoothing: antialiased`, `font-optical-sizing: auto` (`app.css:264-265`).
- `line-height` heredado = **1.5** **[NO VERIFICADO]**: viene del preflight de Tailwind (`styles/tailwind.css:1` hace `@import "tailwindcss"`); ni `LF` ni `CHL` lo fijan salvo donde se indica. `button/input` heredan `font` (preflight), asi que usan la misma familia.
- 1rem = 16px (la escala `text-*` de `scales.css` asume 16px: `--text-base` 0.8125rem = 13px).

### 0.2 Tokens resueltos (dos namespaces conviven)
El **launcher** usa la paleta nueva (`--text/--muted/--faint/--skin/--ok/--warn/--danger`, `styles/palettes/atic/dark.css`, `light.css`). El **clipboard** usa la paleta vieja `--rb-*` (`app.css:87-105` = claro, `app.css:337-372` = oscuro bajo `[data-theme-base="dark"]`) y mezcla ambas.

| Token | Oscuro | Claro | Donde |
|---|---|---|---|
| `--skin` (fondo de floats, opaco) | `#1a1a18` | `#f7f7f2` | dark.css:65 / light.css:42 |
| `--text` | `#f0f0ea` | `#171714` | dark.css:27, light.css:16 |
| `--muted` | `#a8a89e` | `#5f5f58` | idem |
| `--faint` | `#8f8f86` | `#6b6b63` | idem |
| `--line` | `rgb(240 240 234 / 10%)` | `rgb(28 28 24 / 11%)` | idem |
| `--ok` (verde: acciones) | `#6faf88` | `#3f7355` | idem |
| `--warn` (estrella favorita) | `#d4a84b` | `#946718` | idem |
| `--danger` | `#e85a52` | `#d6453d` | idem |
| `--surface` / `--surface-2` | `#1a1a18` / `#1e1e1b` | `#f7f7f2` / `#efefe8` | idem |
| `--bg` | `#121211` | `#ecece6` | idem |
| `--rb-text` | `#f0f0ea` | `#171714` | app.css:340 / :90 |
| `--rb-muted` | `#9a9a90` | `#5f5f58` | app.css:341 / :91 |
| `--rb-faint` | `#6e6e66` | `#8f8f86` | app.css:342 / :92 |
| `--rb-accent` | `#f0f0ea` | `#1a1a17` | app.css:351 / :102 |
| `--rb-focus` | `0 0 0 3px rgba(232,90,82,.22), 0 0 0 1px #e85a52` | `... rgba(229,72,63,.16) ... #d6453d` | app.css:371 / :258 |

Durezas de mezcla: `color-mix(in sRGB, X n%, transparent)` = `rgba(X, n/100)`. Valores planchados sobre `--skin` (dark / light) ya calculados: 5% texto `#252523`/`#ecece7`; 6% `#272725`/`#eaeae5`; 8% `#2b2b29`/`#e5e5e0`; 10% `#2f2f2d`/`#e1e1dc`; 12% (hover dot) `#343431`/`#dcdcd7`; ok 16% (dot accion) `#28322a`/`#dae2d9`; ok 18% `#29352c`/`#d6dfd6`.

### 0.3 Curvas y duraciones (fuente unica: `app.css:126-232`, `styles/motion.css`)
| Token | Valor |
|---|---|
| `--ease-smooth-out` | `cubic-bezier(0.22, 1, 0.36, 1)` (= `--ease-calm`, `--morph-ease`) |
| `--ease-liquid` | `cubic-bezier(0.5, 0, 0.2, 1)` |
| `--duration-quick` / `fast` / `medium` / `slow` | 75 / 125 / 150 / 200 ms |
| `--morph-open-dur` / `--morph-close-dur` | 110 / 100 ms |
| `--float-open-dur` / `--float-close-dur` | 150 (=`--duration-medium`) / 100 ms; `--float-scale` 0.55; `--float-travel` 18px (`app.css:172-177`) |
| `--launcher-bar-open-dur` / `--launcher-separate-dur` | 100 / 90 ms (`app.css:184-186`) |
| Iconos | `stroke="currentColor" fill="none" stroke-linecap/linejoin="round" viewBox="0 0 24 24"`, `strokeWidth` por defecto 1.75 (`ui/Icon.svelte`; MorphIcon 2 si no se pasa). Libreria: **lucide 1.29.0** via **morphicons 1.4.2** (`node_modules/morphicons/dist/react.js` existe, se puede usar en Remotion). |

### 0.4 Sombra y "piel" (aplica a los dos)
- Los floats compactos se pintan como una silueta SDF ("piel", `lib/liquid/Skin.svelte`): `fill`+`stroke 1.25` en `--skin`, con `filter: drop-shadow(0 10px 22px rgb(0 0 0 / 38%))` (`--shadow-goo`, `styles/scales.css:76`). Sin borde ni degradado.
- Ademas, en reposo el `div` recibe `background: var(--skin)` (`lib/surfaces/overlay/OverlaySurface.svelte:578-590`, regla `[data-float].is-shown:not(.is-joined, .is-expanding, .is-separating, .is-settling)`).
- `Kbd` (`lib/ui/Kbd.svelte`): `<span class="inline-flex gap-0.5">` con `<kbd>` `h 20px; min-w 20px; px 4px; font-size 11px/1.3 (text-xs); rounded 5px; border 1px var(--line); bg var(--surface-2) (#1e1e1b dark / #efefe8 light); color var(--muted); font-mono; centrado`. Cada tecla = un kbd; el combo se parte por `+` (`"Ctrl+Enter"` => 2 kbd; `"↑↓"` => 1 kbd).

---

## A. LAUNCHER (Ctrl+Space)

Atajo por defecto `CmdOrCtrl+Space` (`crates/core/src/config.rs:482`). Es un **float del overlay** (`role=dialog`, `data-float="launcher"`), NO una ventana. La ventana Tauri `/launcher` (`lib/surfaces/launcher/LauncherSurface.svelte`) esta **dormida** y usa Tailwind distinto (input `text-lg`, panel `bg-elevated`, filas 32px): **no usar como referencia visual** (`LauncherSurface.svelte:2-5`, `Features/launcher-spotlight.md:3`).

### A.1 Como aparece (importante para el video)
**No sale pegado a la pill**: nace **centrado en el monitor** (horizontal y vertical: `resolveSlot("center")`, `lib/surfaces/overlay/toolSlots.ts:98-101`, `LF:528-549`). La **pill vuela a un slot a la izquierda de la barra** (`center-left-of-launcher`: `x = centroX - 324/2 - 16 - anchoPill`, `y = centroY - altoPill/2`; `toolSlots.ts:114-120`, `LAUNCHER_PILL_GAP=16`), y en el centro nace una gota de 40px que se estira. Sin cuello liquido con la pill (`LF:726-732`, `pill-liquid-emerge.md` "acto A"). Pill idle = disco 40x40 (`edgeDock.test.ts:48`); su look esta en otra ficha.

### A.2 Coreografia (tiempos reales; el doc `Features/launcher-spotlight.md` dice 200ms pero el codigo manda: **420ms**)
Constantes: `LF:80-128`. Orden: `hidden -> birth -> favs -> ready`; cierre `tuck -> recede -> dismiss`.

| Fase | Que ocurre | Duracion / easing | Fuente |
|---|---|---|---|
| Gota | `.lf` capsula **40x40** (disco, `border-radius:999px`, fondo `--skin`) en el centro final. WAAPI `opacity 0->1`, `transform scale(0.82)->none` | **260ms**, `--ease-smooth-out` | `LF:98, 314-325` |
| Beat | queda quieta | **90ms** | `LF:100, 328` |
| Estirón | `width` 40 -> **324px** y `left` (mismo centro), alto fijo **40px**. Sin cambio de alto | **420ms**, `--ease-liquid` `cubic-bezier(0.5,0,0.2,1)` | `LF:108, 470-485` |
| Chrome | durante el birth `.lf-head` tiene `opacity:0` (solo se ve la capsula skin); al asentarse pasa a 1 | transition opacity **240ms** `--ease-smooth-out` (`--lf-chrome-dur`) | `LF:1639-1643, 1705-1709, 1771-1772` |
| Favoritos | cada dot: de `translateX(-23px) scale(.82) opacity 0` a `none / 1`. Delay = `i * 110ms` (inline `--launcher-fav-stagger:110ms`; el CSS default 90 se pisa) | por dot **380ms** `--ease-smooth-out` en transform y opacity; total `110*(n-1)+380` | `LF:112-114, 1826-1867` |
| Recientes (lista) | tras `ready`, si hay recientes el panel crece de alto (`40` -> `recentsHeight`) | WAAPI height **520ms** `cubic-bezier(0.45,0,0.2,1)`; solo al **crecer** (achicar y buscar = salto instantaneo) | `LF:119-120, 634-661` |
| Filas de recientes | cada `li`: `opacity 0->1`, `translateY(-6px)->0`, `animation 380ms var(--ease-smooth-out) both`, delay `120ms + min(i,10)*45ms` | | `LF:1668-1678` |
| Cierre: tuck | dots vuelven en orden inverso (delay `(n-1-i)*110`) | 380ms c/u | `LF:375-387, 1857-1861` |
| Cierre: recede | primero se vacia query y lista (alto salta a 40). Luego `width 324->40`, `left` al centro, `opacity 1->0`, `scale 1->0.82`, `fill: forwards` | **320ms**, `--ease-liquid` | `LF:110, 359-399, 491-516` |
| Sin animacion | `prefers-reduced-motion`: salta a reposo | | `LF:293-301` |

Al buscar (escribir): el alto salta directo a **360px** sin transicion (`LF:801`, `EXPANDED_H`); debounce de busqueda 120ms (`LF:155`). Modo emoji: alto **404px** (`EMOJI_H`). Ancho siempre 324.

### A.3 Estados del contenedor `.lf` (`LF:1639-1728`)
```css
.lf { position:absolute; display:flex; flex-direction:column; width:324px; height:var(--h); /* 40 | recentsHeight | 360 | 404 */
      box-sizing:border-box; border-radius:999px; background:transparent; color:var(--text); overflow:hidden; opacity:0 }
.lf.is-shown { opacity:1 }
/* fondo skin opaco en reposo por OverlaySurface (#1a1a18) */
.lf.is-expanded { /* hay query, modo emoji, o (recientes>0 y fase ready) */
  background:var(--skin); overflow:visible; border-radius:18px;
  box-shadow: 0 18px 48px color-mix(in sRGB, var(--text) 18%, transparent),
              inset 0 0 0 1px color-mix(in sRGB, var(--text) 10%, transparent); }
```
- Sombra expandida en oscuro = `0 18px 48px rgba(240,240,234,0.18)` (es un resplandor CLARO, porque usa `--text`) + anillo interior 1px `rgba(240,240,234,0.10)`. En claro: `rgba(23,23,20,0.18)` y anillo `rgba(23,23,20,.10)`. **Literal del codigo; verificar contra captura real** (`LF:1717-1719`).
- Compacto (solo barra): sin borde, sombra = drop-shadow de la piel (0.4).
- Estado "idle" tras abrir = **barra + dots + lista de Recientes** si hay alguna app abierta o lanzada antes (`showResults`, `LF:249, 1366-1368`). Sin recientes: solo barra + dots. Query vacia no devuelve resultados (`launcher.rs:941-944`).

### A.4 Arbol DOM (`LF:1361-1632`)
```
div.lf[role=dialog aria-label="Buscar apps"]                 324 x (40|H)
├─ div.lf-bar (40px alto, flex, gap 14px)                    border-bottom 1px rgba(text,.10) solo si .is-expanded
│  ├─ header.lf-head (100% x 40, radius 999, bg --skin, padding 0 5.6px 0 8px, gap 4.8px, cursor:grab)
│  │   ├─ span.lf-search-icon > Search 16px (stroke 1.7, color --muted)      [o chip .lf-mode "😀 Emojis" en modo emoji]
│  │   ├─ input.lf-input  flex:1  13px/1.2  color --text  placeholder --faint
│  │   └─ (button.lf-icon 28x28 con X 12px) si hay query | (span.lf-busy "…") mientras busca
│  └─ div.lf-favs (absolute; left: calc(100% + 15px); top:0; flex; gap 15px)
│      └─ button.lf-dot × N (max 8)  40x40 circulo, icono 20px
├─ p.lf-heading "RECIENTES"   (solo sin query)
├─ ul.lf-list (padding 5.6px; overflow:auto)
│   └─ li > div.lf-hit(.is-sel) > [button.lf-hit-main > (span.lf-hit-ico 32x32 > icono 18px) + span.lf-hit-text > (.lf-hit-title + .lf-hit-sub)] + button.lf-star 40x40 (Star 14px)
└─ footer.lf-foot  (borde-top 1px; hints con Kbd)
```

### A.5 Barra (compacta) — medidas
| Elemento | Valor | Fuente |
|---|---|---|
| Barra | 324 x **40**, radius 999, fondo `#1a1a18` | LF:22 (`launcher.rs:21-26`), 1730-1743, 1756-1769 |
| Padding interno head | `0 0.35rem 0 0.5rem` = 0 5.6px 0 8px; gap 0.3rem = **4.8px** | LF:1762-1764 |
| Icono lupa | lucide `Search` **16px**, strokeWidth **1.7**, color `--muted` | `LauncherIcon.svelte:37`, LF:1779-1784 |
| Input | `font-size .8125rem` = **13px**, `line-height 1.2`, color `--text`, sin borde/fondo, placeholder `--faint` | LF:1786-1800 |
| Placeholder | **"Buscar apps…"** (`overlay.searchPlaceholder`, es.ts:891) | |
| Boton limpiar | 1.75rem = **28x28**, radius .4rem = 6.4px, X **12px**, color `--faint`; hover `--text` + bg `rgba(text,.08)`; active scale .96 | LF:1894-1920, 1443 |
| "…" (buscando) | mono, .7rem = 11.2px, `--faint` | LF:1888-1892 |
| Foco teclado (`:focus-visible`) | `outline:2px solid color-mix(--ok 78%, --text); outline-offset:3px; box-shadow:0 0 0 4px rgba(ok,.18)` (no visible con raton) | LF:1922-1930 |
| Cursor | head `grab` (se arrastra), input `text` | |

### A.6 Favoritos (dots a la derecha de la barra)
- Posicion: `left: calc(100% + 15px)` de la barra, `top:0`; dots 40x40 separados **15px** (`FAVS_GAP_PX`, `DOT_GAP_PX`, LF:87-91). Ej. con 5 favoritos: x = barra.derecha + 15 + i*(40+15). Max **8** (`launcher.rs:18`).
- Dot: circulo 40x40, `background:var(--skin)`, color `--muted`, icono **20px** (apps: bitmap real del `.exe`; acciones: lucide sw 1.7). Hover: `color:--text; background:#343431 (mix text 12% / skin); transform:scale(1.06)`; active `scale(.96)`; transition color/background 75ms.
- Dot de **accion** (`.is-action`): `background:#28322a` (mix ok 16%/skin), `color:#6faf88`. Claro: `#dae2d9` / `#3f7355`.
- Sombra: drop-shadow de la piel (0.4), la piel los funde en la silueta.
- Tooltip al pasar (450ms de retardo, `tip.svelte.ts:67`): `.tip` con el titulo del fav: `font .72rem/1.3 weight 500; padding .26rem .46rem; radius .4rem; bg mix(surface 96%, bg); border 1px mix(line 80%); shadow 0 8px 22px rgba(0,0,0,.32); opacity fade 125ms` (`TipHost.svelte`).

### A.7 Fila de resultado (`LF:1965-2086`)
| Elemento | Valor |
|---|---|
| Lista | `padding .35rem` = 5.6px; `li` sin gap |
| `.lf-hit` | flex, gap .15rem = 2.4px, radius .5rem = **8px**; seleccionada: `background: rgba(240,240,234,.05)` (= `#252523`) ; transition bg 75ms |
| `.lf-hit-main` | padding `.45rem .35rem .45rem .55rem` = 7.2 5.6 7.2 8.8px; gap .7rem = **11.2px**; radius 8px; active `scale(.99)` |
| Caja de icono | **32x32**, radius .4rem = 6.4px, fondo `rgba(text,.06)` (`#272725`), color `--muted`; icono **18px** |
| Caja icono de **accion** | fondo `rgba(111,175,136,.18)` (`#29352c`), color `#6faf88` |
| Titulo | `.9rem` = **14.4px**, weight **600**, 1 linea con elipsis, color heredado `--text` |
| Subtitulo | `.7rem` = **11.2px**, `--muted`, elipsis; gap titulo/subtitulo .05rem = 0.8px |
| Estrella | 2.5rem = **40x40**, margin-right .15rem, radius 6.4px, `Star` **14px**; `--faint`, opacity **.55** (1 si fila seleccionada/hover/activa); hover bg `rgba(text,.08)` + `--text`; activa: `fill:currentColor; color:--warn` (#d4a84b) |
| Alto de fila | **~53.6px calculado** (7.2+21.6+0.8+16.8+7.2, con line-height 1.5) **[NO VERIFICADO]**; el codigo estima 44 (`contain-intrinsic-size:auto 44px`, `recentsHeight = 40+28+n*44+10`, LF:262, 1967) |
| Seleccion | mouse (`mouseenter`) y flechas mueven `selected` (cicla); solo una fila con fondo. No hay borde ni barra lateral |

Heading "RECIENTES": `padding .45rem .7rem .1rem; font .62rem (9.9px) weight 600; letter-spacing .08em; uppercase; color --faint` (LF:1944-1953).

Footer `.lf-foot` (LF:2219-2240): `padding .35rem .5rem .5rem` (5.6 8 8); `border-top:1px rgba(text,.10)`; `font-size .6rem` (9.6px), color `--faint`; `justify-content:space-between; gap .4rem`; cada hint `inline-flex gap .25rem` = `<Kbd> texto`. Altura ~34.6px (kbd 20px). En el ancho 324 el 4to hint puede recortarse unos px (overflow hidden) **[NO VERIFICADO]**.

### A.8 Textos exactos (es, `lib/core/i18n/es.ts:890-950`)
| Clave | Texto |
|---|---|
| `overlay.searchApps` (aria) | Buscar apps |
| `overlay.searchPlaceholder` | **Buscar apps…** |
| `overlay.clearSearch` | Limpiar la búsqueda |
| `overlay.recents` | Recientes (se muestra en MAYUSCULAS por CSS: **RECIENTES**) |
| `overlay.results` | Resultados |
| `overlay.searching` | Buscando… |
| `overlay.noResults` | Sin resultados (centrado, 13.6px `.85rem`, `--faint`, padding 24px 12px) |
| `overlay.navHint` / `openHint` / `closeHint` / `quitHint` | navegar / abrir / cerrar / cerrar app |
| Kbd del footer | `↑↓` (un kbd) + navegar · `Enter` + abrir · `Esc` + cerrar · (solo si la fila seleccionada es app) `Ctrl` `Enter` + cerrar app |
| `overlay.inUse` | En uso |
| `overlay.justNow` | Ahora |
| `overlay.openedAgo` | Abierta hace {when} — `when` = `"12 min"`, `"3 h"`, `"2 d"` (`span.min/h/d`; `spanFrom`: <90 min => min; <36 h => h; si no d) |
| `overlay.usedAgo` | Usada hace {when} |
| Tooltip dot | titulo del hit; aria "Abrir {title}" |
| Estrella aria | "Agregar {title} a favoritos" / "Quitar {title} de favoritos" |
| Modo emoji | chip "Emojis" (`overlay.emoji.mode`), placeholder "Buscar emojis…", hints "Enter pegar" / "Ctrl Enter copiar", secciones "Usados hace poco", "Caras y emociones", "Personas y cuerpo", "Animales y naturaleza", "Comida y bebida", "Viajes y lugares", "Actividades", "Objetos", "Símbolos" (Banderas oculto en Windows) |

Subtitulo segun contexto (`LF:1583-1585`): **sin query** (Recientes) = `recencyLabel`: En uso / Abierta hace X / Usada hace X / Ahora. **Con query** = `hit.subtitle` del backend: apps => **"Aplicación"** (`launcher.rs:1031-1037`); acciones => su descripcion (abajo).

### A.9 Tipos de resultados y ejemplos realistas
Ranking: prefijo/contiene/subsecuencia; acciones suman +15; max 24 resultados (`launcher.rs:17, 205-227`). La **calculadora va primero** si aplica (`launcher.rs:953-980`).

| Tipo | Icono (lucide) | Titulo / Subtitulo (es) |
|---|---|---|
| App (Menu Inicio) | bitmap real de la app 18px (fallback `AppWindow`) | "Google Chrome" / "Aplicación"; "Visual Studio Code" / "Aplicación"; "Spotify" |
| Calculadora | `Calculator` (caja verde, accion) | Titulo = resultado, ej. **"14"** para `2+3*4`, **"6.213712"** para `10 km to mi`, **"86"** para `30 c to f`; subtitulo **"Enter para copiar"** (`launcher.rs:964`). Divisas (opt-in): subtitulo `"{fuente} · Enter para copiar"`, ej. `valor oficial del 26 sep · Enter para copiar`. Sin estrella (sintetico) |
| Activar divisas | `Coins` | "Activar conversión de divisas" / "Ajustes → Launcher · consulta tasas en línea" |
| Dictar | `Mic` | "Dictar" / "Iniciar o detener dictado" |
| Capturar pantalla | `Crop` | "Capturar pantalla" / "Seleccionar ventana, región o monitor" |
| Dibujar en pantalla | `Pencil` | / "Congelar la pantalla y marcarla" |
| Elegir color | `Pipette` | / "Cuentagotas: un píxel de la pantalla al portapapeles" |
| Emojis | `Smile` | "Emojis" / "Buscar y pegar emojis (o escribe «:»)" |
| Historial de clipboard | `Clipboard` | / "Abrir el historial junto a la pill" |
| Textos guardados | `FileText` | / "Abrir fragmentos y bloc" |
| Agentes | `SquareTerminal` | / "Abrir la consola de agentes" |
| Sistema | `Cpu` | / "Volumen, recursos y pantallas junto a la pill" |
| Ajustes | `Settings` | / "Abrir la ventana principal de Atic" |
| Cerrar todas las apps | `Power` | / "Cerrar las apps abiertas (piden guardar lo que corresponda)" |
| Bloquear pantalla | `Lock` | / "Cerrar la sesión y pedir la contraseña" |
| Suspender | `Moon` | / "Suspender el equipo" |
| Silenciar o activar sonido | `VolumeX` | / "Alternar el silencio de la salida de audio" |
| Vaciar papelera | `Trash2` | / "Pide confirmación: no se puede deshacer" |
| Consola de agente | `SquareTerminal` | "Claude Code" / "Nueva consola de Claude Code (claude) en la pizarra" (solo los instalados) |

Mapa id -> icono: `lib/icons.ts:164-184` (`LAUNCHER_ICONS`). Las acciones llevan caja verde; las apps, caja neutra.

Escenarios sugeridos para el video: (1) vacio con 4-5 dots + Recientes ("Google Chrome — En uso", "Visual Studio Code — Abierta hace 12 min", "Spotify — Usada hace 3 h"); (2) escribir `chr` => "Google Chrome / Aplicación" seleccionado (fila con fondo `#252523`, estrella visible), footer con 4 hints; (3) escribir `2+3*4` => fila accion verde "14 / Enter para copiar"; (4) Ctrl+Enter sobre app la cierra (barra queda abierta).

### A.10 Modo emoji (`LF:1479-1549`, css 2098-2217), resumen
Escribir `:` con barra vacia. Alto 404. Chip `.lf-mode`: 25.6px alto, pill, `bg rgba(ok,.18)`, color `--ok`, 11.5px (`.72rem`) weight 600, texto "😀 Emojis". Barra de categorias: botones 28x28 radius 7.2px, emoji 15.2px, opacity .6 (1 activo/hover, bg `rgba(text,.08)`), boton de tono a la derecha. Grilla `repeat(8, 1fr)`, celda cuadrada, radius 8.8px, emoji **23.2px** (1.45rem); seleccionada bg `rgba(text,.10)`. Footer: nombre del emoji con emoji grande 15.2px + hints "Enter pegar", "Ctrl Enter copiar".

### A.11 Bloque copiable (React) — barra + fila
```css
.lf-bar{height:40px;width:324px;border-radius:999px;background:#1a1a18;display:flex;align-items:center;padding:0 5.6px 0 8px;gap:4.8px}
.lf-input{font:400 13px/1.2 Aptos,"Segoe UI Variable",sans-serif;color:#f0f0ea;background:transparent;border:0;outline:0;flex:1}
.lf-input::placeholder{color:#8f8f86}
.lf-panel{background:#1a1a18;border-radius:18px;box-shadow:0 18px 48px rgba(240,240,234,.18),inset 0 0 0 1px rgba(240,240,234,.10)}
.lf-hit{display:flex;align-items:center;gap:2.4px;border-radius:8px}
.lf-hit.sel{background:rgba(240,240,234,.05)}
.lf-hit-main{display:flex;align-items:center;gap:11.2px;padding:7.2px 5.6px 7.2px 8.8px;flex:1}
.lf-ico{width:32px;height:32px;border-radius:6.4px;background:rgba(240,240,234,.06);color:#a8a89e;display:grid;place-items:center}
.lf-ico.action{background:rgba(111,175,136,.18);color:#6faf88}
.lf-title{font-size:14.4px;font-weight:600;color:#f0f0ea}
.lf-sub{font-size:11.2px;color:#a8a89e}
.lf-foot{display:flex;justify-content:space-between;gap:6.4px;padding:5.6px 8px 8px;border-top:1px solid rgba(240,240,234,.10);color:#8f8f86;font-size:9.6px}
.kbd{display:inline-flex;align-items:center;justify-content:center;height:20px;min-width:20px;padding:0 4px;font:11px/1.3 "Cascadia Mono",monospace;color:#a8a89e;background:#1e1e1b;border:1px solid rgba(240,240,234,.10);border-radius:5px}
```

---

## B. CLIPBOARD (historial)

Atajo por defecto `CmdOrCtrl+Shift+V` (`crates/core/src/config.rs:441`); tambien rueda/tira de la pill y la accion "Historial de clipboard" del launcher. Float `data-float="clipboard"`, clase `cf float-emerge`. Tamano de Rust **312 x 372** (`panel_float.rs:13-19`, `PANEL_SHAPE`).

### B.1 Como aparece: SI sale de la pill ("acto B": fused grow -> separate)
La pill vuela al cursor y luego el panel nace **pegado a ella**. Lado por defecto: **debajo** de la pill (`side:"top"` => cuelga hacia abajo); si no cabe prueba arriba, derecha, izquierda (`floatPlace.ts:315-360`). Reposo: `x = pill.derecha - 18` (borde izq. del panel 18px antes del borde der. de la pill; si no cabe se alinea por la izquierda), `y = pill.y + pill.h + 16`.

| Fase | Que ocurre | Duracion / easing | Fuente |
|---|---|---|---|
| Semilla | disco **40x40** solapado **20px** sobre la pill (gap -20), un solo blob liquido; head/body con `opacity:0` (solo se ve la piel) | espera 2 frames + **60ms** (`SEED_HOLD_MS`) | CF:67, 239-243; floatPlace.ts:56-63, 385-410 |
| Grow (`expand`) | `width` 40 -> 312 y `height` 40 -> 372, borde hacia la pill clavado; en paralelo `transform`/`opacity` del `float-emerge` (de `translateY(-18px) scale(.55) opacity 0` a `none`) | w/h/left/top **100ms** `--ease-smooth-out` (`--launcher-bar-open-dur`); transform **150ms**; opacity **100ms** | CF:670-678; app.css:278-317 |
| Separate | el panel se aleja de la pill hasta **gap 16px** (> REACH 10px => el cuello liquido se corta); aparece el chrome (head + lista) | left/top **90ms** `--ease-smooth-out` (`--launcher-separate-dur`) | CF:264-275, 687-695 |
| `ready` | fondo `--skin`, radius 18, sombra por la piel (drop-shadow 0.4) | | |
| Cierre | `approach` (vuelve a gap 2px para re-fundir el cuello, 90ms) y luego `bubble.hide()`: fade+scale a `.55` con viaje 18px hacia la pill | 100ms `--ease-smooth-out` | CF:278-313; app.css:296-317 |

Duracion total apertura ~ 33ms + 60 + 100 + 33 + 90 = **~320ms**. Reduced-motion: salta al reposo. No se cierra al pegar; Esc, X o clic afuera cierran (salvo pin) (`Features/clipboard-historial.md:17-19`).

### B.2 Arbol DOM (`CF:557-629`, `CHL:405-660`)
```
div.cf.float-emerge  312x372  padding 7.2px 8px 8.8px  radius 18px  bg --skin  color --text
├─ header.cf-head   min-height 24px; margin-bottom 3.2px; flex-end; gap 8px
│   ├─ i.cf-grab  (abs, centro) 32x3px radius 999, bg color-mix(--rb-text 24%) = #4d4d4a
│   └─ div.cf-acts gap 2.4px:  [Pin 13px] [PanelTopClose 14px] [X 14px]   cada boton 28x28 radius 6.4px, color --faint, hover/on: --text + bg rgba(text,.08)
└─ div.cf-body > div.clip-list.is-compact.is-island  (flex col, gap 4.8px)
    ├─ div.clip-toolbar (row, gap 4.8px, padding 0 3.2px)
    │   ├─ label.clip-search-wrap > Search 12px (left 6.4px, --rb-muted) + input.clip-search
    │   └─ div.clip-toolbar-row (gap 2.4px): .clip-kinds [Layers|Type|Image botones 25.6 circulo, icono 12px] + .clip-filters [Star 14px 25.6px]
    └─ ul.clip-items (padding 0 2.4px 21.6px; overflow:auto)  li.clip-row × N (44px, margin-bottom 2px)
        ├─ div.clip-item (role=button; flex; gap 6.4px; padding 3.2px 6.4px; radius 12px)
        │   ├─ span.clip-thumb  (22x22 texto | 44x34 imagen | 22x22 swatch de color)
        │   └─ span.clip-body > span.clip-preview + span.clip-sub > span.clip-meta  [+ span.clip-quick (chips, solo imagen)]
        └─ div.clip-actions (columna, siempre visible): [Star 14px] [X 14px]  botones 22.4x22.4
```
Variante `is-island` (la que muestra el float) oculta: boton "Mostrar todos" (List), etiquetas de texto de los filtros y el contador `.clip-count` (`CHL:431,441,451,455,483`). Cabecera de pin/retach/cerrar: sin titulo (`CF:575-576`).

### B.3 Medidas
| Elemento | Valor | Fuente |
|---|---|---|
| Panel | 312 x 372, radius **18px**, padding `.45rem .5rem .55rem` = 7.2 / 8 / 8.8px | CF:643-668 |
| Header | min-height 1.5rem = 24px, margin-bottom .2rem = 3.2px; botones `1.75rem` = 28px, radius .4rem = 6.4px | CF:697-769 |
| Alto util lista | 372 - 7.2 - 24 - 3.2 - 25.6 - 4.8 - 8.8 = **~298px** (menos 21.6 de padding inferior => ~6 filas visibles) | calculado |
| Buscador | alto **1.6rem = 25.6px**, radius **999**, padding `0 .4rem 0 1.4rem` (0 6.4 0 22.4), fondo `rgba(240,240,234,.07)` (`#292927`), `font .625rem = 10px`, weight 500, color `--rb-text`; placeholder `--rb-muted` (#9a9a90); foco: `inset 0 0 0 1.5px rgba(accent,.7)`. Ancho calculado ~175px | CHL:702-721, 1114-1120 |
| Botones tipo (Todo/Texto/Imágenes) | circulo **25.6px**, icono **12px**, gap 2.4; inactivo `--rb-muted`; activo `bg rgba(240,240,234,.14)` (`#383835`) + color `--rb-accent`; hover `bg rgba(text,.08)` + `--rb-text`; active scale .96 | CHL:738-761, 1131-1137 |
| Boton favoritos (solo estrella) | 25.6px circulo, Star 14px (relleno si activo), mismo on/hover | CHL:769-790, 1139-1143 |
| Fila | alto **44px**, `margin: 0 0 2px` (stride 46), `overflow:hidden` | CHL:102-103, 1149-1153 |
| `.clip-item` | `gap .4rem` = 6.4; `padding .2rem .4rem` = 3.2 6.4; radius **12px**; sin borde ni fondo en reposo; hover/foco `background rgba(240,240,234,.08)` (`#2b2b29`); cursor grab | CHL:1155-1167 |
| Miniatura texto | **22x22**, radius 6, `bg rgba(text,.08)`, icono `Type` **13px** color `--rb-muted` | CHL:1169-1174, 910-914, 562-564 |
| Miniatura **imagen** | **44x34**, radius 6, `object-fit:cover`, `outline:1px solid rgb(255 255 255/10%)` offset -1 | CHL:1176-1180, 903-909 |
| Miniatura **color** | mismo 22x22, relleno = hex normalizado, aro interior `inset 0 0 0 1px rgb(255 255 255/22%), inset 0 0 0 1px rgb(0 0 0/18%)` | CHL:923-929, 238-242 |
| Preview (1a linea) | `.6875rem` = **11px**, weight 500, lh 1.25, 1 linea, elipsis, `--rb-text` (via `.cf` color `--text`) | CHL:940-947, 1182-1184 |
| Meta (2a linea) | `.5625rem` = **9px**, `--rb-muted`, tabular-nums; contenedor `.clip-sub` min-height 1.25rem = 20px | CHL:949-960, 1186-1188 |
| Acciones der. | columna gap 1.6px, cada boton 22.4px min, radius 6, iconos 14px `--rb-muted`; estrella fijada: color `--rb-accent` + `fill: currentColor`; hover bg `rgba(text,.08)` | CHL:1053-1080, 1190-1198 |
| Scroll | `.clip-items` overflow auto; ventana virtual (stride 46, overscan 4) | CHL:101-125 |
| Orden | fijados primero (`sort pinned desc`, `lib/domain/clipboard.svelte.ts:29`), luego cronologico | |

### B.4 Fila de TEXTO vs fila de IMAGEN
- **Texto**: `[Type 13px en caja 22x22]  "Primer texto copiado, recortado a una linea…"` / `"texto · 14:32"`. Preview = `item.preview`; vacio => "(vacío)".
- **Imagen**: `[miniatura 44x34]  "Imagen"` (preview de la imagen) / `"imagen · Ayer · 18:05"`; con origen captura: `imagen · 14:32 · Captura`. **Al pasar el mouse** la linea meta se desvanece (opacity 0, 75ms) y aparecen 3 chips en su lugar (`.clip-quick`, opacity 0->1, 75ms): `[✎ Dibujar] [↗ Abrir] [T Texto]` — iconos lucide `Pencil`, `ExternalLink`, `ScanText` **11px**; chip alto **20px**, radius .35rem = 5.6px, padding 0 5.1px, gap 2.9px, `bg rgba(240,240,234,.08)`, `color --rb-muted`, `font .58rem` = 9.3px weight 650; hover bg `.14` + `--rb-text`. El ancho del `.clip-sub` (~202px) es > 11.5rem (184px), asi que los rotulos se ven (`CHL:1041-1051`). En filas de texto la meta siempre queda visible.
- **Color** (texto que parsea como color): caja de 22x22 con el color y meta `"color · 09:12"`.
- Formato de la hora (`lib/core/format.ts:39-64`, locale es): hoy `HH:MM` (24h); ayer `Ayer · HH:MM`; <7 dias `"mar · 09:12"` (dia corto `Intl`); mas atras `"12 sept, 16:40"` (dia mes corto + hora). Meta = `kindLabel + fecha` con `kindLabel` = `"texto · "` / `"imagen · "` / `"color · "` (es.ts:1194-1196).

### B.5 Textos exactos (es, `page.clipboard.*` es.ts:1182-1226; `overlay.*`)
| Uso | Texto |
|---|---|
| Placeholder buscador | **Buscar…** (`page.clipboard.searchPlaceholder`) |
| aria buscador / tipo | Buscar en el historial / Tipo |
| Botones tipo (aria/tooltip) | Todo · Texto · Imágenes |
| Estrella filtro (tooltip) | Solo favoritos / Mostrar todos |
| Estrella fila (tooltip) | Fijar / Dejar de fijar |
| X fila | Borrar |
| Chips de imagen | Dibujar · Abrir · Texto (OCR leyendo: "Leyendo…" segun `page.captures.ocrReading`) |
| Header (tooltips) | Fijar arriba / Desfijar · Volver a la pill · Cerrar |
| Vacio | **El historial está vacío** + linea tenue **Copia algo y aparece aquí.** (12px `--rb-muted`, hint 11px `--rb-faint`, padding 12px 5.6px) |
| Filtro sin coincidencias | **Nada coincide** ; solo favoritos vacio: "No hay favoritos. Márcalos con la estrella." |
| Preview hover | (ver B.6) pista: "Clic para pegar · Arrastra a una consola o a otra app" |
| Atajos mostrados | ninguno en el float (no hay Kbd). En `Features` se documentan Ctrl+V / Ctrl+Shift+V como comportamiento interno |

### B.6 Busqueda, filtros, pins
- Busqueda en vivo sobre `preview + text` (`lib/core/clipboardSearch.ts`): normaliza (minusculas, sin tildes); coincide por substring o por **todas** las palabras (sin fuzzy). Filtro tipo: Todo/Texto/Imagenes. Filtro favoritos: alterna. Al filtrar el scroll vuelve arriba (`CHL:144-152`).
- Pin/estrella por fila persiste, sube el item arriba. X borra. Pin del float (chincheta del header) = siempre encima / no cerrar al perder foco.
- **Preview flotante de fila** (`ClipPreviewHost.svelte`): tras **600ms** de hover (400ms si ya hay uno abierto) aparece a la **derecha** de la fila (gap 10, cae a la izquierda si no cabe), centrado verticalmente: `radius .6rem; padding .5rem .6rem; bg color-mix(--surface 92%, transparent) + backdrop-filter blur(10px); box-shadow 0 0 0 1px mix(--line 80%), 0 6px 8px rgba(0,0,0,.22); fade+translateY(4px->0) 150ms smooth-out`. Texto: `.72rem/1.45 weight 450`, `pre-wrap`, max 18 lineas. Imagen: min-w 16rem, max-w 34rem, max-h 24rem, radius .35rem, damero de fondo. Color: 13rem x 5rem. Pie `.625rem --faint` con la pista y `border-top 1px`.
- Tooltip general: ver A.6.

### B.7 ClipboardPeek (hover del disco Clipboard en la pill) — `lib/surfaces/overlay/pill/ClipboardPeek.svelte`
Panel de vistazo (`PillPeekHost.svelte:639-655`): `.q-panel` **ancho 16rem = 256px**, `padding .55rem .7rem .6rem`, radius **20px**, `font .75rem/1.35`, color `--text`, fondo por la piel (fundido a la pill, mismo `--skin` + drop-shadow). Aparece con `.float-emerge` (150ms open, 100ms close, escala .55 y viaje 18px hacia la pill; hit-events tras `--float-open-dur`).
Contenido: hasta **3** ultimos items (`LAST=3`), `gap .15rem`, filas `padding .3rem .35rem; radius 10px; gap .5rem`, hover `bg rgba(text,.10)`; miniatura de imagen `2.75rem x 2rem` (44x32) radius 6; texto 1 linea con elipsis (color `--text`, blancos colapsados; imagen sin miniatura => "Imagen"); hora a la derecha `--faint .6875rem` (`formatListWhen`, ej. "14:32"). Enlace inferior **"Ver el historial completo"** (`--faint`, 11px, hover `--text`). Vacio: **"Todavía no has copiado nada"** (`--muted`). Cargando: `pill.quota.loading` ("Leyendo cupos…"). aria de fila: "Pegar {label}".

### B.8 Bloque copiable (React) — panel y fila
```css
.cf{position:absolute;width:312px;height:372px;box-sizing:border-box;padding:7.2px 8px 8.8px;border-radius:18px;background:#1a1a18;color:#f0f0ea;overflow:hidden;display:flex;flex-direction:column;
    filter:drop-shadow(0 10px 22px rgb(0 0 0/38%)) /* la sombra real es de la piel detras */}
.cf-head{display:flex;justify-content:flex-end;align-items:center;min-height:24px;margin-bottom:3.2px;position:relative}
.cf-icon{width:28px;height:28px;border-radius:6.4px;color:#8f8f86;display:grid;place-items:center}
.clip-search{height:25.6px;border-radius:999px;padding:0 6.4px 0 22.4px;background:rgba(240,240,234,.07);color:#f0f0ea;font:500 10px Aptos,sans-serif}
.clip-search::placeholder{color:#9a9a90}
.clip-kind{width:25.6px;height:25.6px;border-radius:999px;color:#9a9a90}
.clip-kind.on{background:rgba(240,240,234,.14);color:#f0f0ea}
.clip-row{height:44px;margin-bottom:2px;display:flex;align-items:stretch;gap:3.2px;overflow:hidden}
.clip-item{display:flex;flex:1;align-items:center;gap:6.4px;padding:3.2px 6.4px;border-radius:12px;background:transparent}
.clip-item:hover{background:rgba(240,240,234,.08)}
.clip-thumb{width:22px;height:22px;border-radius:6px;background:rgba(240,240,234,.08);display:grid;place-items:center;overflow:hidden}
.clip-thumb.img{width:44px;height:34px}
.clip-preview{font:500 11px/1.25 Aptos,sans-serif;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.clip-meta{font-size:9px;color:#9a9a90}
```

---

## C. Iconos (copiar en React) — lucide 1.29.0, `viewBox="0 0 24 24"`, `fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round"`
Tamanos y `strokeWidth`: launcher (lupa 16 sw 1.7; lista 18 sw 1.7; dots 20 sw 1.7; X 12 sw 1.75; Star 14 sw 1.75). Clipboard (Pin 13, PanelTopClose 14, X 14, Search 12, Layers/Type/Image 12, Type miniatura 13, Star 14, chips 11; todos sw 1.75). Para estrella activa: `fill="currentColor"` (`Icon fill`).

```
Search:        <path d="m21 21-4.34-4.34"/><circle cx="11" cy="11" r="8"/>
X:             <path d="M18 6 6 18"/><path d="m6 6 12 12"/>
Star:          <path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/>
AppWindow:     <rect x="2" y="4" width="20" height="16" rx="2"/><path d="M10 4v4"/><path d="M2 8h20"/><path d="M6 4v4"/>
Calculator:    <rect width="16" height="20" x="4" y="2" rx="2"/><line x1="8" x2="16" y1="6" y2="6"/><line x1="16" x2="16" y1="14" y2="18"/><path d="M16 10h.01"/><path d="M12 10h.01"/><path d="M8 10h.01"/><path d="M12 14h.01"/><path d="M8 14h.01"/><path d="M12 18h.01"/><path d="M8 18h.01"/>
Mic:           <path d="M12 19v3"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><rect x="9" y="2" width="6" height="13" rx="3"/>
Crop:          <path d="M6 2v14a2 2 0 0 0 2 2h14"/><path d="M18 22V8a2 2 0 0 0-2-2H2"/>
Pencil:        <path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/><path d="m15 5 4 4"/>
Pipette:       <path d="m12 9-8.414 8.414A2 2 0 0 0 3 18.828v1.344a2 2 0 0 1-.586 1.414A2 2 0 0 1 3.828 21h1.344a2 2 0 0 0 1.414-.586L15 12"/><path d="m18 9 .4.4a1 1 0 1 1-3 3l-3.8-3.8a1 1 0 1 1 3-3l.4.4 3.4-3.4a1 1 0 1 1 3 3z"/><path d="m2 22 .414-.414"/>
Smile:         <circle cx="12" cy="12" r="10"/><path d="M8 14s1.5 2 4 2 4-2 4-2"/><line x1="9" x2="9.01" y1="9" y2="9"/><line x1="15" x2="15.01" y1="9" y2="9"/>
Clipboard:     <rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/>
FileText:      <path d="M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z"/><path d="M14 2v5a1 1 0 0 0 1 1h5"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>
SquareTerminal:<path d="m7 11 2-2-2-2"/><path d="M11 13h4"/><rect width="18" height="18" x="3" y="3" rx="2" ry="2"/>
Cpu:           <path d="M12 20v2"/><path d="M12 2v2"/><path d="M17 20v2"/><path d="M17 2v2"/><path d="M2 12h2"/><path d="M2 17h2"/><path d="M2 7h2"/><path d="M20 12h2"/><path d="M20 17h2"/><path d="M20 7h2"/><path d="M7 20v2"/><path d="M7 2v2"/><rect x="4" y="4" width="16" height="16" rx="2"/><rect x="8" y="8" width="8" height="8" rx="1"/>
Settings:      <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915"/><circle cx="12" cy="12" r="3"/>
Coins:         <path d="M13.744 17.736a6 6 0 1 1-7.48-7.48"/><path d="M15 6h1v4"/><path d="m6.134 14.768.866-.5 2 3.464"/><circle cx="16" cy="8" r="6"/>
Power:         <path d="M12 2v10"/><path d="M18.4 6.6a9 9 0 1 1-12.77.04"/>
Lock:          <rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>
Moon:          <path d="M20.985 12.486a9 9 0 1 1-9.473-9.472c.405-.022.617.46.402.803a6 6 0 0 0 8.268 8.268c.344-.215.825-.004.803.401"/>
VolumeX:       <path d="M11 4.702a.705.705 0 0 0-1.203-.498L6.413 7.587A1.4 1.4 0 0 1 5.416 8H3a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h2.416a1.4 1.4 0 0 1 .997.413l3.383 3.384A.705.705 0 0 0 11 19.298z"/><line x1="22" x2="16" y1="9" y2="15"/><line x1="16" x2="22" y1="9" y2="15"/>
Trash2:        <path d="M10 11v6"/><path d="M14 11v6"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
Pin:           <path d="M12 17v5"/><path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"/>
PanelTopClose: <rect width="18" height="18" x="3" y="3" rx="2"/><path d="M3 9h18"/><path d="m9 16 3-3 3 3"/>
Layers:        <path d="M12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83z"/><path d="M2 12a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 12"/><path d="M2 17a1 1 0 0 0 .58.91l8.6 3.91a2 2 0 0 0 1.65 0l8.58-3.9A1 1 0 0 0 22 17"/>
Type:          <path d="M12 4v16"/><path d="M4 7V5a1 1 0 0 1 1-1h14a1 1 0 0 1 1 1v2"/><path d="M9 20h6"/>
Image:         <rect width="18" height="18" x="3" y="3" rx="2" ry="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.086-3.086a2 2 0 0 0-2.828 0L6 21"/>
ExternalLink:  <path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>
ScanText:      <path d="M3 7V5a2 2 0 0 1 2-2h2"/><path d="M17 3h2a2 2 0 0 1 2 2v2"/><path d="M21 17v2a2 2 0 0 1-2 2h-2"/><path d="M7 21H5a2 2 0 0 1-2-2v-2"/><path d="M7 8h8"/><path d="M7 12h10"/><path d="M7 16h6"/>
```
Los iconos de app del launcher (bitmaps reales del .exe, `launcherIconCache.ts`) NO son SVG: en el video hay que dibujar sustitutos (logos genericos) a 18px (lista) y 20px (dots), `border-radius:.2rem` (3.2px) en el `img`.

---

## D. Discrepancias y huecos
- `Features/launcher-spotlight.md` y `Features/pill-liquid-emerge.md` dicen estirón de 200ms; el codigo real es **420ms** (`BIRTH_DUR_MS`, LF:108). Prima el codigo.
- Alto de fila del launcher: 53.6px calculado vs 44px que asume el codigo; con 5+ recientes la lista scrollea. No verifique con captura.
- La fuente Aptos puede no estar instalada donde se renderice el video (Remotion): declarar el mismo stack (`Aptos, "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif`). No confirme cual fuente cae en esta maquina.
- Sombra expandida del launcher usa `--text` (resplandor claro en oscuro): confirmar con captura antes de replicar.
- No se encontro en el codigo ninguna insignia/badge propia del launcher ademas de `Kbd`; ni atajos mostrados en las filas; ni agrupacion "Fijados" en el float de clipboard (la clave `pinnedGroup` existe en es.ts pero `CHL` no la usa).
- No se ejecuto la app ni se tomaron capturas (la app instalada esta abierta; matarla para depurar por CDP habria cerrado sesiones del usuario).
