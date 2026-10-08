# Ficha visual: agentes (pill, ventana, float) y consola (xterm, chat, tarjetas)

Fuente: código del repo Atic, tema **oscuro** (paleta `dark`, `data-theme-base="dark"`). Lo marcado **(derivado)** sale de sumar CSS/TS, no de medirlo en la app en ejecución (no reinicié ni toqué la Atic del usuario: había agentes vivos). Lo marcado **(no encontrado)** no existe en el código. Donde un `.md` de diseño y el código discrepan, gana el código (⚠).

Rutas abreviadas. `L/` = `apps/desktop/src/lib/`, `S/` = `apps/desktop/src/styles/`, `APP` = `apps/desktop/src/app.css`.

| Alias | Ruta |
|---|---|
| `FLOAT` | `L/surfaces/overlay/agents/AgentsFloat.svelte` |
| `LAUNCH` | `L/features/agents/AgentLauncher.svelte` |
| `CONS` | `L/features/agents/ConsolePanel.svelte` |
| `CHAT` | `L/features/agents/AgentChatPanel.svelte` |
| `BOARD` / `CARD` / `BLIST` / `BCOMP` / `BZOOM` / `BMAP` | `L/features/agents/AgentsBoard` / `BoardCard` / `BoardList` / `BoardComposer` / `BoardZoom` / `BoardMinimap` `.svelte` |
| `TERMV` | `L/features/agents/TerminalView.svelte` |
| `CONV` / `MSG` / `TOOL` / `COLLAB` | `L/AgentConversation` / `AgentMessage` / `AgentToolCard` / `AgentCollabCard` `.svelte` |
| `ACT` / `WORK` / `FILES` / `PERM` / `ASK` | `L/features/agents/ChatActivity` / `ChatWork` / `EditedFiles` / `ChatPermission` / `ChatQuestion` `.svelte` |
| `PS` | `L/surfaces/overlay/pill/PillSurface.svelte` (8997 líneas) |
| `PEND` / `CHIP` / `PEEK` | `L/surfaces/overlay/pill/PillPending.svelte` / `pillAgentChip.ts` / `AgentsPeek.svelte` |
| `ES` | `L/core/i18n/es.ts` (`pill` :672, `page.agents` :1254, `launcher` :1259, `permission` :1281, `board` :1288, `window` :1350, `chat` :1392, `console` :1632) |

Hermanas ya escritas (no repetir): `pill-notch-y-tokens.md` (geometría del notch, caras `agent` y `live`, AticMark, tokens) y `pizarra-y-color.md` (es la herramienta de anotar, **no** la pizarra de agentes).

---

## 0. Advertencias y hallazgos (leer primero)

| # | Hallazgo | Cita |
|---|---|---|
| 1 | **Camino vivo hoy = la VENTANA de agentes, no el float.** Rueda, atajo (`agents_shortcut`, default `CmdOrCtrl+Shift+A`) y catálogo llaman `agentsEnsureWindow()`: ventana del SO de **1120 × 760** (inner), mín. 680 × 480, título "Consolas de agentes", perfil webview propio; dentro va **`AgentsBoard`** (pizarra de tarjetas), no `ConsolePanel`. | `PS:3955-3960`, `agents_window.rs:70-102`, `routes/agents/+page.svelte`, `AgentsWindow.svelte`, `shortcuts.rs:390-404`, `docs/PLAN_VENTANA_AGENTES.md` |
| 2 | **Float (`AgentsFloat` → `AgentLauncher` → `ConsolePanel` con rail de pestañas) y la cara `agents` de la isla siguen en el código y montados** (`OverlaySurface.svelte:463`), pero `activateTool("agents")` retorna antes de llegar a ellos (`PS:3953-3961`). El plan los marca "Fase C — retiro". No encontré un disparador vivo. Los documento igual porque el brief los pide y son la única superficie con **rail de N pestañas + chat con interfaz**. | `PLAN_VENTANA_AGENTES.md` §C |
| 3 | El **chat con interfaz** (`AgentChatPanel`) vive hoy: (a) en fichas del float/isla (menú `+` → icono globo "Abrir X como chat"), (b) en la **pizarra** solo como tarjetas hijas de solo lectura (sesiones que abre otro agente por MCP) con botones de permiso (`decides`). Las TUIs del usuario en la pizarra son `TerminalView` (xterm). | `CONS:3565-3574`, `BOARD:1561` |
| 4 | `docs/DISENO_PILL.md` dice `islandCueMark` = 14 px; el código dice **18**. Además la descripción corta de la tool en `es.ts` es "Terminales con agentes" (no "Consola con interfaz" de `tools.ts`). | `pillStage.ts:68`, `ES:tools.agents` |
| 5 | Sin uso (no los uses en el video): `AgentMark.svelte` (logos pixel-art, ningún import), `AgentModelsModal.svelte`, `HubSessions.svelte` (`Features/agentes.md` lo confirma). | grep de imports |
| 6 | `docs/PLAN_EXPERIENCIA_CONSOLAS.md` es **propuesta** ("No hay código escrito"): bandeja M1, resumen de turno M2, detección de atasco M3, worktrees… **no existen en la UI**. No los dibujes. | `PLAN_EXPERIENCIA_CONSOLAS.md:3-4` |
| 7 | `docs/demos/agentes.html` es una maqueta de marketing (chips "Opus ⌄ / Esfuerzo alto ⌄ / Preguntar ⌄", "trabajando…"). Los textos reales del composer son otros (ver §7.6). Sirve solo como ejemplo de contenido (§8). | `agentes.html:221-252` |
| 8 | `.launch` (botón CTA del lanzador) **no declara `border-radius`**; con el preflight de Tailwind sería 0 (cuadrado). Verificar con captura antes de redondearlo. | `LAUNCH:1293-1310` |
| 9 | En `ConsolePanel`, `.console-desk { --agent-accent: var(--rb-record) }` ⇒ la ficha activa del rail se tiñe de **rojo** (`#352220` derivado), y `.icon-btn.is-on` usa `#e85a52`. No es el acento gris de la paleta. | `CONS:5485-5492,5533-5535,5745-5748` |
| 10 | `--rb-*` (árbol viejo, usado por casi todo lo de agentes) solo tiene dos variantes; con `data-theme-base="dark"` valen los del §1.2. Los tokens nuevos (`--text`, `--muted`, `--accent`, `--skin`, `--rec`, `--ok`, `--warn`) son la paleta `dark.css`. **Los dos difieren** (p. ej. `--muted` `#a8a89e` vs `--rb-muted` `#9a9a90`); cada componente usa uno u otro, indicado abajo. | `APP:329-366`, `S/palettes/atic/dark.css` |
| 11 | El contenido del TUI de cada CLI (banner de Claude Code, prompt de Codex…) **no está en el repo**: lo pinta el CLI dentro del xterm. Solo hay pistas en `AgentMark.svelte` (comentario: la mascota de Claude sale de la ASCII que imprime el CLI al arrancar). | — |

---

## 1. Tokens resueltos (tema oscuro)

### 1.1 Paleta nueva (`S/palettes/atic/dark.css`, `:root`)

| Token | Valor | Token | Valor |
|---|---|---|---|
| `--bg` | `#121211` | `--muted` | `#a8a89e` |
| `--surface` | `#1a1a18` | `--faint` | `#8f8f86` |
| `--surface-2` | `#1e1e1b` | `--line` | `rgb(240 240 234 / 10%)` |
| `--elevated` | `#262622` | `--line-strong` | `rgb(240 240 234 / 18%)` |
| `--text` | `#f0f0ea` | `--accent` | `#e8e8e0` (`--on-accent` `#121211`) |
| `--skin` | `#1a1a18` (color de toda superficie líquida: pill, float, cara) | `--rec` / `--danger` | `#e85a52` (`--danger-soft` `#2d1a19`) |
| `--ok` | `#6faf88` (`--ok-soft` `#1c2a22`) | `--warn` | `#d4a84b` (`--warn-soft` `#2b2416`) |
| `--info` | `#8fa9b8` (`--info-soft` `#1a2328`) | `--agent-accent` (default) | `#a8a89e` |

Consola: `[data-palette="console"]` (`S/palettes/atic/console.css`) repite exactamente `--bg/--surface/--surface-2/--elevated/--text/--muted/--faint/--line/--accent/--skin` de arriba (es la tinta oscura fija); no cambia nada en tema oscuro.

Acento por backend (`S/palettes/atic/agents.css`, solo donde hay `[data-agent]`, p. ej. `.chat[data-agent]`):

| `data-agent` | `--agent-accent` |
|---|---|
| `claude-code` | `#da7756` |
| `opencode` | `#7fae86` |
| `codex` | `#8fa9b8` |
| `cursor` | `#a88fc4` |

### 1.2 Legado `--rb-*` con `data-theme-base="dark"` (`APP:329-366`)

| Token | Valor | Token | Valor |
|---|---|---|---|
| `--rb-bg0` | `#121211` | `--rb-muted` | `#9a9a90` |
| `--rb-bg1` | `#1a1a18` | `--rb-faint` | `#6e6e66` |
| `--rb-surface` | `#1e1e1b` | `--rb-border` | `rgba(246,246,241,.10)` |
| `--rb-surface-2` | `#262622` | `--rb-border-strong` | `rgba(246,246,241,.18)` |
| `--rb-surface-elevated` | `#2a2a26` | `--rb-record` | `#e85a52` (soft `#3a2220`) |
| `--rb-sidebar` | `#171714` | `--rb-ok` | `#6faf88` |
| `--rb-panel` | `#22221e` | `--rb-warn` | `#d4a84b` |
| `--rb-text` | `#f0f0ea` | `--rb-info` / `--rb-sys` | `#7ea0bc` |
| `--rb-accent` | `#f0f0ea` (`--rb-on-accent` `#121211`) | `--rb-mic` | `#6faf88` |
| `--rb-shadow` | `0 24px 70px rgba(0,0,0,.45)` | `--rb-focus` | `0 0 0 3px rgba(232,90,82,.22), 0 0 0 1px #e85a52` |

El chat re-mapea alias locales (`CHAT:1090-1102`, igual en `PEND:125-133`): `--coral = var(--accent)` (#e8e8e0), `--text = --rb-text`, `--dim = --rb-muted`, `--faint = --rb-faint`, `--line = --rb-border`, `--card = --rb-surface-2` (#262622), `--code = --rb-surface-2`, `--hover = rb-text 6 %`, `--add = --rb-ok`, `--del = --rb-record`. Así `MSG`, `TOOL`, `COLLAB` se ven con esos valores.

### 1.3 Tipografía

| Uso | Familia | Cita |
|---|---|---|
| UI (`--font-sans` = `--rb-font`) | `"Aptos", "Avenir Next", "Helvetica Neue", "Segoe UI Variable", sans-serif` (en Windows sin Aptos cae a Segoe UI Variable) | `S/scales.css:33-34`, `APP:77` |
| Mono UI (`--font-mono` = `--rb-mono`) | `"Cascadia Mono", "SFMono-Regular", "Roboto Mono", monospace` | `S/scales.css:36` |
| Mono en tarjetas/markdown | `ui-monospace, "Cascadia Mono", Consolas, monospace` | `TOOL:212`, `MSG:110` |
| xterm | `Cascadia Mono, SFMono-Regular, Menlo, Consolas, monospace` | `CONS:2757`, `TERMV:133` |

Base 1rem = 16 px (no hay `font-size` en `html`). `--text-base` 13 px/1.5, `sm` 12, `xs` 11, `micro` 10 (`S/scales.css:42-62`). Pesos: 400/500/600/650 (`--font-weight-bold: 650`); el chat usa además 560, 620, 680, 700.

### 1.4 Radios, sombras, curvas, tiempos

| Token | Valor |
|---|---|
| radios | `xs 5`, `sm 8`, `md 14`, `lg 20`, `pill 999` px (`scales.css:18-22`); legado `--rb-radius 14`, `sm 8`, `xs 5` |
| `--shadow-card` | `0 1px 2px rgb(0 0 0 / 20%)` |
| `--shadow-pop` | `0 8px 24px rgb(0 0 0 / 32%)` |
| `--shadow-float` | `0 24px 70px rgb(0 0 0 / 45%)` |
| `--shadow-goo` (sombra de la piel) | `0 10px 22px rgb(0 0 0 / 38%)` |
| `--ease-smooth-out` / `--ease-morph` / `--ease-calm` | `cubic-bezier(0.22, 1, 0.36, 1)` |
| `--ease-island` | `cubic-bezier(0.33, 1.38, 0.46, 1)` (rebote) — `APP:213` |
| `--ease-liquid` | `cubic-bezier(0.5, 0, 0.2, 1)` |
| duraciones | `micro 40`, `quick 75`, `fast 125`, `medium 150`, `slow 200`, `very-slow 250`, `--morph-open 110`, `--morph-close 100`, `--float-open 150`, `--float-close 100`, `--island-open 240` ms |
| `--float-scale` / `--float-travel` / `--float-blur` | `0.55` / `18px` / `2px` (`APP:176-177`) |

---

## 2. Logos e iconos

### 2.1 Logos de agente (`L/features/agents/AgentLogo.svelte`)

`<span class="agent-logo" style="--agent-logo-size:{N}px">`: caja cuadrada `N × N`, `display:grid; place-items:center`. Claude va como `<img>` a color; el resto como `mask` (`mask: url(...) center / contain no-repeat; background: currentColor`), o sea **heredan el color del texto** (`#f0f0ea` normal, `--rec`/`--ok`/`--muted` en chips). Sin logo conocido: icono Lucide `SquareTerminal`.

| Agente (`cli`) | Archivo | viewBox | Render |
|---|---|---|---|
| Claude Code (`claude`, `claude-code`) | `apps/desktop/static/agents/claude.svg` | `0 0 24 24` | `<img>` a color, `fill="#D97757"` fijo (1 path, 1614 B) |
| Codex (`codex`, `openai`) | `static/agents/openai.svg` | `0 0 24 24` | máscara `currentColor` (1605 B, nudo de OpenAI) |
| Cursor (`cursor-agent`, `cursor`) | `static/agents/cursor.svg` | `0 0 24 24` | máscara (cubo isométrico, 465 B) |
| OpenCode (`opencode`) | `static/agents/opencode.svg` | `0 0 24 24` | máscara (`M16 6H8v12h8V6zm4 16H4V2h16v20z`) |
| Antigravity (`agy`) | `static/agents/antigravity.svg` | `0 0 24 24` | máscara (`M12 2.1 3.2 21h4.4L12 9.2 16.4 21h4.4L12 2.1z`) |
| Grok (`grok`, `xai`) | `static/agents/grok.svg` | `0 0 24 24` | máscara |
| Gemini (`gemini`) | `static/agents/gemini.svg` | `0 0 24 24` | máscara; **no** está en `AGENTS` del catálogo |
| Marca Claude alternativa | `static/brands/claude.svg` / `.png` | `0 0 100 100` `fill="#da7756"` | no la usa `AgentLogo` |

Orden y nombres del catálogo (`agentCatalog.ts:37-90`): **Claude Code** (`claude`), **OpenCode** (`opencode`), **Codex** (`codex`), **Cursor** (`cursor-agent`), **Antigravity** (`agy`), **Grok** (`grok`). El brief pide Claude Code / Codex / Cursor / OpenCode: usa ese subconjunto, en el orden del catálogo si muestras la grilla (Claude, OpenCode, Codex, Cursor).

Tamaños de render por contexto:

| Dónde | px |
|---|---|
| Lanzador float, celda del picker | 22 (caja 1.4 rem ≈ 22.4) |
| Estado vacío de la pizarra (tarjeta 112 px) | 22 |
| Cabecera de tarjeta de la pizarra | 14 |
| Lista lateral (árbol / plegada) | 15 / 16 |
| Rail del float (ficha) | 18 (grupo: 14 solapados); cabecera `active-agent-logo` 18; menú `+` 14; velo de arranque 28; chat vacío 30 |
| Chip flotante de la pill | 11 (13 con el dock achicado) |
| Cue en el notch | 18 (`PILL.islandCueMark`) |
| Fila `live` de la isla | 16; cara `agent` 16; cabecera `PillPending` 14; anillo de cupos 16 |

Claude SVG completo (1 path; cópialo tal cual de `static/agents/claude.svg`, ya está en el repo; empieza `M4.709 15.955l4.72-2.647.08-.23…` y termina `…-1.312-.006.006z`).

Los otros cuatro que pide el brief, completos:

```svg
<!-- opencode.svg -->
<svg fill="currentColor" fill-rule="evenodd" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M16 6H8v12h8V6zm4 16H4V2h16v20z"/></svg>
<!-- cursor.svg -->
<svg fill="currentColor" fill-rule="evenodd" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M22.106 5.68L12.5.135a.998.998 0 00-.998 0L1.893 5.68a.84.84 0 00-.419.726v11.186c0 .3.16.577.42.727l9.607 5.547a.999.999 0 00.998 0l9.608-5.547a.84.84 0 00.42-.727V6.407a.84.84 0 00-.42-.726zm-.603 1.176L12.228 22.92c-.063.108-.228.064-.228-.061V12.34a.59.59 0 00-.295-.51l-9.11-5.26c-.107-.062-.063-.228.062-.228h18.55c.264 0 .428.286.296.514z"/></svg>
<!-- antigravity.svg -->
<svg fill="currentColor" fill-rule="evenodd" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M12 2.1 3.2 21h4.4L12 9.2 16.4 21h4.4L12 2.1z"/></svg>
<!-- grok.svg -->
<svg fill="currentColor" fill-rule="evenodd" viewBox="0 0 24 24" xmlns="http://www.w3.org/2000/svg"><path d="M2.5 2h4.6l14.4 20h-4.6L2.5 2zm18.9 0L14 12.3l-2.3-3.2L16.8 2h4.6zM2.6 22l5.1-7.1 2.3 3.2L7.2 22H2.6z"/></svg>
```

OpenAI (Codex) SVG: 1605 B, un solo `<path>` (`static/agents/openai.svg`); cópialo del archivo (empieza `M9.205 8.658v-2.26c0-.19.072-.333.238-.428…`).

### 2.2 Iconos de UI (Lucide, paquete `lucide` ^1.29 vía `morphicons` — `L/ui/Icon.svelte`, `L/icons.ts`)

Envoltorio: `<svg viewBox="0 0 24 24" width=S height=S fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">` (default `Icon`: size 16, strokeWidth 1.75; `AgentIcons` usa 11 px / 1.8; `ToolIcon` de la pill usa 1.5). Nodos (`d` o forma):

| Icono | Contenido | Se usa en (px) |
|---|---|---|
| X | `M18 6 6 18` · `m6 6 12 12` | cerrar tarjeta 13, cerrar ficha 9, chrome 11 |
| Plus | `M5 12h14` · `M12 5v14` | `+` rail 12, stepper 13, nueva consola 14 |
| Minus | `M5 12h14` | stepper 13, minimizar 12 |
| Folder | `M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z` | carpeta 15/12/13 |
| ChevronRight | `m9 18 6-6-6-6` | carpeta → 13-14 |
| ArrowRight | `M5 12h14` · `m12 5 7 7-7 7` | CTA del lanzador 14 |
| ArrowLeft | `m12 19-7-7 7-7` · `M19 12H5` | "Agentes" (volver) 13 |
| ArrowUp | `m5 12 7-7 7 7` · `M12 19V5` | enviar 15 |
| Square | `<rect x=3 y=3 w=18 h=18 rx=2>` | detener 11, maximizar chrome 11 |
| SquareTerminal | `m7 11 2-2-2-2` · `M11 13h4` · `<rect x=3 y=3 w=18 h=18 rx=2 ry=2>` | herramienta Agentes, consola nueva |
| MessageSquare | `M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z` | abrir como chat 12 |
| Plug | `M12 22v-5` · `M15 8V2` · `M17 8a1 1 0 0 1 1 1v4a4 4 0 0 1-4 4h-4a4 4 0 0 1-4-4V9a1 1 0 0 1 1-1z` · `M9 8V2` | chip MCP 13 |
| Keyboard | 7 puntos `M6 8h.01 M8 12h.01 M10 8h.01 M12 12h.01 M14 8h.01 M16 12h.01 M18 8h.01` · `M7 16h10` · `<rect x=2 y=4 w=20 h=16 rx=2>` | atajos 13 |
| Activity | `M22 12h-2.48a2 2 0 0 0-1.93 1.46l-2.35 8.36a.25.25 0 0 1-.48 0L9.24 2.18a.25.25 0 0 0-.48 0l-2.35 8.36A2 2 0 0 1 4.49 12H2` | uso 13 |
| Pin | `M12 17v5` · `M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z` | fijar 12 |
| SquareArrowOutUpRight | `M21 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h6` · `m21 3-9 9` · `M15 3h6v6` | sacar a ventana 13 |
| Pill | `m10.5 20.5 10-10a4.95 4.95 0 1 0-7-7l-10 10a4.95 4.95 0 1 0 7 7Z` · `m8.5 8.5 7 7` | devolver a la pill 13 |
| Paperclip | `m16 6-8.414 8.586a2 2 0 0 0 2.829 2.829l8.414-8.586a4 4 0 1 0-5.657-5.657l-8.379 8.551a6 6 0 1 0 8.485 8.485l8.379-8.551` | adjuntar 14 |
| History (= RotateCcwClock) | `M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8` · `M3 3v5h5` · `M12 7v5l4 2` | historial |
| Check | `M20 6 9 17l-5-5` | fila `ready` 14 (trazo 2.2) |
| Maximize2 | `M15 3h6v6` · `m21 3-7 7` · `m3 21 7-7` · `M9 21H3v-6` | maximizar tarjeta 12 |
| Minimize2 | `m14 10 7-7` · `M20 10h-6V4` · `m3 21 7-7` · `M4 14h6v6` | restaurar 12 |
| Forward | `m15 17 5-5-5-5` · `M4 18v-2a4 4 0 0 1 4-4h12` | encargar a otro agente 12 |
| PanelLeftClose | `<rect 3 3 18 18 rx2>` · `M9 3v18` · `m16 15-3-3 3-3` | achicar lista 15 |
| PanelLeftOpen | idem con `m14 9 3 3-3 3` | mostrar lista 15 |
| Maximize | `M8 3H5a2 2 0 0 0-2 2v3` · `M21 8V5a2 2 0 0 0-2-2h-3` · `M3 16v3a2 2 0 0 0 2 2h3` · `M16 21h3a2 2 0 0 0 2-2v-3` | ver todas 14 |
| RotateCcw | `M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8` · `M3 3v5h5` | esfuerzo: restablecer 14 |

Iconos por `ToolKind` en la tarjeta de herramienta (`AGENT_ICONS`, `icons.ts:186-207`; `AgentIcons` 11 px, trazo 1.8, color `--faint`): `read` FileText (`M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z` · `M14 2v5a1 1 0 0 0 1 1h5` · `M10 9H8` · `M16 13H8` · `M16 17H8`), `edit`/`move` Pencil (`M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z` · `m15 5 4 4`), `delete` Trash2, `search` Search (`m21 21-4.34-4.34` · circle 11,11 r8), `execute` SquareTerminal, `think` Lightbulb, `fetch` Globe, `switch_mode` ArrowLeftRight, `other` Plus. Modos de permiso: ShieldCheck / Shield / ShieldMinus / ShieldOff (este último `#d4a24c`).

Mapeo herramienta→tipo (`agents/model.rs:82-93`): Read→read; Edit/Write/MultiEdit→edit; Grep/Glob→search; Bash→execute; WebFetch/WebSearch→fetch; Task/Agent→collab (tarjeta `COLLAB`); TodoWrite/ExitPlanMode→think.

---

## 3. Flujo típico paso a paso (camino vivo: pill → ventana → pizarra)

Ver el estado de la pill acoplada al borde superior (notch) en `pill-notch-y-tokens.md`; aquí solo lo de agentes.

| # | Qué ve el usuario | Detalle / cita |
|---|---|---|
| 1 | Pill en reposo (notch 124×40 arriba, o disco de 52 px flotante). Pasa el mouse: se abre la tira; entre las herramientas está **Agentes** (icono SquareTerminal). Alternativa: atajo `Ctrl+Shift+A`. | `tools.agents` `ES:39-42`: label "Agentes", short "Terminales con agentes", action "Abrir consola" |
| 2 | Clic en Agentes: la rueda/tira se cierra y se abre una **ventana del SO** de 1120×760 (primera vez ≈1,8 s; si ya se usó, precalentada a los 8 s del arranque). | `PS:3953-3961`, `agents_window.rs:36-68` |
| 3 | **Pizarra vacía**: punteado sobre `#121211`, al centro: título "La pizarra está vacía", ayuda, chip de carpeta, stepper "1 consola", una fila de tarjetas de agente (Claude Code, OpenCode, Codex, Cursor…, Terminal). A la izquierda, lista flotante (232 px) con "+ Nueva consola"; abajo, el composer chiquito (420 px) deshabilitado "Toca una consola para escribirle". | §5.6 |
| 4 | Elige carpeta (chip → `FolderBrowser`) y cantidad (− / N consolas / +). Clic en la tarjeta **Claude Code**: nace una tarjeta de consola (rebote 320 ms), la lista lateral suma "Claude Code", el composer se habilita con la etiqueta del agente. Dentro: "Arrancando…" (12 px `--rb-muted`) hasta que el CLI pinta su TUI. | `openConsole` `BOARD:726-762`; `TERMV` `.boot` |
| 5 | Varias consolas: stepper N>1 abre N tarjetas en grilla; o `Nueva consola` en la lista → elegir agente → selector de carpeta → tarjeta cascada (+28 px). Cada tarjeta tiene su barra (logo, nombre, punto de estado, acciones) y se arrastra/redimensiona; minimapa y zoom flotantes. | `placeNew` `agentBoard.ts:75-99` |
| 6 | **Escribir**: el composer se ensancha 420→680 px (340 ms, rebote de isla) y sube 2 px; escribe "Revisa por qué falla el build"; al enviar sale una **chispa** (10 px, `#e8e8e0` con glow) del botón hacia la tarjeta (520 ms) y el texto entra al PTY (Enter 40 ms después). | `BCOMP:156-190`, `BOARD:865-907` |
| 7 | **Trabajando**: el TUI muestra su spinner; el punto de la tarjeta late (`#e8e8e0`, 1,2 s) con texto "Trabajando…"; en la lista lateral igual. En la **pill** aparece el aviso (ver §4): chip "Trabajando…" con el logo respirando, y la piel de la pill "respira" (brillo). | §4.2, `PS:1355` |
| 8 | **Respuesta / tarjeta de herramienta**: si es un chat (ficha de chat en el float, o tarjeta hija), aparece el hilo: texto del agente, bloque "Leyó 3 · editó 1 · ejecutó 2" plegable, tarjetas de herramienta al abrir (§7). En TUI el mismo trabajo lo pinta el CLI. | §7 |
| 9 | **Permiso desde la pill**: el agente se detiene; el chip pasa a rojo "permiso"; con la pill flotante cuelga debajo (8 px de gap, 360 px de ancho) la **tarjeta de pendientes**; en el notch se despliega la cara `agent` 280×104. Botones Rechazar / Aprobar siempre / Aprobar (tarjeta) o RECHAZAR / APROBAR (cara). El agente sigue al decidir. | §4.4 |
| 10 | **Aviso al terminar**: chip verde "Listo" (o la última frase del agente, recortada) que entra con fade+subida 4 px+blur 2 px (250 ms); en la pizarra la tarjeta destella un anillo (1,1 s) y el punto se pone verde con "Respondió". Clic en el chip = enfoca la ventana/tarjeta y lo marca visto. (En el float legado: toast "«Claude Code» terminó su turno".) | `PS:8781-8786,8919-8931`, `CARD:104-131,260-283` |

Variante legado (float): pill → panel de 400×208 (lanzador) → "Abrir Claude Code" → panel crece a 680×520 con rail de pestañas a la izquierda (`+` = terminal o chat), barra arriba, terminal a la derecha. Ver §6.

---

## 4. Pill: semáforo y estados de agente

### 4.1 Tabla canónica del semáforo

Tono del chip (`CHIP:3-4`, prioridad `waiting 3 > working 2 > ready 1`, ordena urgentes arriba; máx. 4 chips apilados `CHIP:260`):

| Estado | Token | Hex | Chip flotante (`.p-agent`) | Cue en notch (`.p-island-cue`) | Fila `live` | Punto tarjeta pizarra | Ficha rail (chat) |
|---|---|---|---|---|---|---|---|
| **waiting** (espera permiso/pregunta) | `--rec` | `#e85a52` | fondo `rec 16 %` (`#3b2421` sobre skin), texto "permiso" | fondo rec 16 %, logo `--rec` | texto `--rec`, "!" | `--accent` con halo `0 0 0 3px accent 30 %` | punto `--rb-warn` `#d4a84b`, texto `--rb-warn` "Espera tu permiso" |
| **working** | `--muted`/`--warn` | chip `#a8a89e`; fila `#d4a84b` | fondo transparente, color `--muted`, logo con opacidad 0,55↔1 (T=1,8 s); texto preview o "Trabajando…"/"Contestando…" | logo pulsa igual | color `--warn` + opacidad pulsante; aro giratorio 12 px | `--accent` `#e8e8e0` late (1,2 s, opacidad 1→.35) | punto `--accent` latiendo 1,2 s |
| **ready** | `--ok` | `#6faf88` | fondo `ok 14 %` (`#262f28`), texto "Listo" o la última frase; entra con `p-agent-ready-in` | fondo ok 14 % | color `--ok`, check 14 px | punto `--rb-ok` "Respondió" (si `attention`), si no `rgba(240,240,234,.22)` | "Lista" sin punto; "Respondió" (sin leer) = punto verde `#6faf88` |
| **failed** | `--rb-record` | `#e85a52` | — | — | — | — | punto rojo con halo 22 % |
| **ended** | — | — | — | — | — | sin punto, texto `--rb-faint` "Terminó" | "Terminada" |

Textos (`ES pill:672-722`): `pill.permission` "permiso", `pill.ready` "Listo", `pill.chipWorking` "Trabajando…", `pill.chipAnswering` "Contestando…", `pill.working` "El agente está trabajando", `pill.waiting` "El agente espera tu permiso", `pill.dismiss` "Descartar", `pill.goToAgent` "Ir a {name} en su terminal", `pill.openConsole` "Abrir la consola de agentes", `pill.pendingOpen` "Abrir".
Actividad (fila `live`, `ES pill.activity`): "Pensando…", "Escribiendo…", "Editando…"/"Editando {detail}", "Leyendo…"/"Leyendo {detail}", "Buscando…", "Ejecutando…", "Delegando…"/"Delegando: {detail}", "Usando una herramienta…".

### 4.2 Chip flotante (pill/disco con aviso) — `PS:8662-8812`

Se apila a la derecha del disco dentro de `.p-bar-slot` (gap 8 px); columna `.p-agent-stack` gap 4 px, alineada a la izquierda.

```css
.p-agent {                /* PS:8675 */
  position: relative; display: inline-flex; align-items: center;
  min-height: 1.35rem;    /* 21.6px */
  max-width: 9.5rem;      /* 152px */
  gap: 0.18rem; padding: 0 0.34rem 0 0.28rem;   /* 2.9px; 0 5.4px 0 4.5px */
  border: 0; border-radius: 999px;
  background: color-mix(in sRGB, var(--accent) 12%, transparent);  /* neutro, no se usa */
  color: var(--accent);
  transition: transform 75ms cubic-bezier(.22,1,.36,1), background 75ms …, color 75ms …;
}
.p-agent:active { transform: scale(.96); }
.p-agent-ico { width: .85rem; height: .85rem; opacity: .92; }   /* logo 11px */
.p-agent-count, .p-agent-msg { font-size: .625rem; /*10px*/ font-weight: 650; line-height: 1; font-variant-numeric: tabular-nums; }
.p-agent-msg { max-width: 18ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.p-agent.is-waiting { background: color-mix(in sRGB, #e85a52 16%, transparent); color: #e85a52; }
.p-agent.is-working { background: transparent; color: #a8a89e; }
.p-agent.is-working .p-agent-ico { opacity: calc(.55 + .45*(.5 - .5*cos(clock/1.8 * 1turn))); }  /* 0.55↔1.0, T = 1.8 s */
.p-agent.is-ready   { background: color-mix(in sRGB, #6faf88 14%, transparent); color: #6faf88;
                      animation: p-agent-ready-in 250ms cubic-bezier(.22,1,.36,1) both; }
@keyframes p-agent-ready-in { from { opacity:0; transform: translateY(4px); filter: blur(2px);} to { opacity:1; transform:none; filter: blur(0);} }
```

Dock achicado (float legado minimizado): `.is-dock` sin fondo, logo 13, texto 11 px (`PS:8731-8754`). Hit ≥40 px por `::after`.

**Respiración de la piel**: mientras haya un chip `working`, `liquid.breathe = true` ⇒ `.skin.is-breathing { filter: brightness(1.04 − 0.04·cos(clock/2.4·1turn)) }` = brillo 1,00↔1,08 con T = 2,4 s (`PS:1355`, `liquid/Skin.svelte:148-150`). Fill de la piel `--skin` `#1a1a18`, sombra `drop-shadow(0 10px 22px rgb(0 0 0 / 38%))`, trazo del mismo color 1,25 px.

### 4.3 Aviso en el notch acoplado (cue) — `PS:7754-7960`, medidas en `pillStage.ts:40-77`

Botón `26 × auto` (`--island-cue-btn` 26) al lado de la marca, `border-radius: 999`, logo `AgentLogo 18`; con mensaje (solo el chip 1º en eje horizontal): logo + `<span class="p-island-cue-msg">` 10 px/650/`letter-spacing .02em`, tramo fijo de 96 px con elipsis. Estados: `is-waiting` fondo rec 16 % color `--rec`; `is-ready` fondo ok 14 % color `--ok`; `is-working` sin fondo, opacidad pulsante 0,55↔1 (T 1,8 s). Más de 3 logos ⇒ 2 logos + "+N" (`--muted`, 10 px/700, celda 18 px). El aviso se desmonta cuando la tira abre.

**Cara `live`** (chips activos con la tira cerrada; hover): 232 px de ancho, filas de 32 px, ver `pill-notch-y-tokens.md` §2.9. Estilos exactos de fila en `PS:7155-7215`: `gap 8, padding 0 8, radius calc(22−8)=14`, hover `text 10 %`, `active scale(.985)`, etiqueta 11 px/600. Animaciones de la actividad (3 `<i>` de 3 px en caja 14×10, todas por el reloj `--clock`): pensando (puntos que respiran, T 1,2 s, desfase .15/.3), escribiendo (saltan −3 px, T .9 s), editando (trazo que crece 14 px en 70 % de 1,1 s), leyendo (barra 2×10 que barre 12 px, T 2,2 s), ejecutando (cursor 6×9 parpadea T .9 s), buscando (punto que orbita), delegando (dos puntos se cruzan 6 px, T 2 s), otra (aro 9 px gira T .9 s).

### 4.4 Permiso desde la pill

**(a) Pill flotante/disco → `PillPending`** (`PEND`, `PS:2501-2536,6100-6114`): tarjeta anclada **debajo** de la pastilla con `gap 8 px`, **ancho 360 px**, alto ≈88 estimado (si no cabe abajo sube). Aparece con `.float-emerge`: `opacity 0→1` y `scale(.55)`→1 con `translateY(∓18px)`, origen en `--tail` (centro de la pill), abrir **150 ms** (`--float-open-dur: --duration-medium`), cerrar **100 ms**, `cubic-bezier(.22,1,.36,1)`; el cuello líquido la une a la pill si el hueco ≤ REACH (`PS:2519-2530`).

```css
.pending {                      /* PEND:124-150 */
  display: flex; flex-direction: column; gap: 8px;
  max-height: min(460px, 70vh); overflow-y: auto;
  border-radius: 16px; padding: 10px;
  background: #1e1e1b;                     /* --rb-surface */
  color: #f0f0ea; font: 13px "Aptos",…;
  box-shadow: 0 0 0 1px color-mix(in sRGB, #f0f0ea 10%, transparent),
              0 16px 40px -16px rgb(0 0 0 / 55%);
}
.head  { display:flex; align-items:center; gap:8px; padding:0 2px; }   /* logo 14 · .who 12.5/600 · .pager 11px faint · .open ml:auto 11.5px muted, radius 6, pad 3 8 */
```

Cabecera: `AgentLogo 14` + nombre ("Claude Code", o la etiqueta de la sesión) + si hay varios pendientes `‹ 1 / 3 ›` (botones 22×22, 14 px, tabulares) + botón "Abrir" a la derecha.
Cuerpo = `ChatPermission` (`PERM:128-242`):

```css
.perm { display:flex; flex-direction:column; gap:8px; border-radius:14px; padding:12px;
  background: color-mix(in sRGB, #e8e8e0 8%, #1e1e1b);          /* ≈ #2e2e2b */
  box-shadow: inset 0 0 0 1px color-mix(in sRGB, #e8e8e0 30%, transparent), 0 8px 24px -16px rgb(0 0 0/50%); }
.title { font: 600 13px; margin:0 }            /* "Claude Code quiere usar Bash" */
.desc  { color:#9a9a90; font-size:12.5px }      /* description del pedido, opcional */
.preview { max-height:220px; border-radius:8px; padding:8px 10px; background:#121211; color:#f0f0ea;
  font: 11.5px/1.5 "Cascadia Mono",…; white-space:pre-wrap }   /* "$ cargo check --locked -p atic-desktop"  o diff: líneas "+" #6faf88 / "-" #e85a52 (máx 14) */
.actions { display:flex; justify-content:flex-end; gap:6px }
.btn { min-height:30px; border-radius:8px; padding:0 12px; font:600 12px; background: color-mix(in sRGB,#f0f0ea 8%,transparent); }
.btn.is-primary { background:#e8e8e0; color:#121211 }  /* Aprobar; hover 88 % accent → #e9e9e1 */
```

Botones en orden: **Rechazar · Aprobar siempre · Aprobar** (primario). Pregunta (`AskUserQuestion`) = `ChatQuestion` (`ASK`): mismo marco (fondo accent 7 % ≈ `#2c2c29`, ring accent 28 %), chip de cabecera (accent 18 %, 11 px/600), pregunta 13.5/600, opciones (radio 10, pad 8 10, fondo text 5 %; seleccionada accent 14 % + ring 45 %; marca 14 px círculo o cuadrado r4), input "O escribe tu respuesta…", botones Saltar (ghost) / Atrás / Siguiente|Enviar respuestas. Plan (`ChatPlan`): título "Plan de {name}", Rechazar / Aprobar plan.

**(b) Notch acoplado → cara `agent`** (280×104 bajo la banda de 40): ver `pill-notch-y-tokens.md` §2.9 (título "El agente espera tu permiso", `<strong>Bash</strong> · descripción`, botones píldora 32 px "RECHAZAR" rec / "APROBAR" ok, 11 px/600/MAYÚSCULAS/ls .06em). `PS:5650-5697,7644-7734`.

### 4.5 Cupos de agentes (hover de la herramienta) — `PEEK`

Anillos por agente: caja 2.15 rem = **34,4 px**, `<svg viewBox="0 0 40 40">` círculo `r=17`, `pathLength=100`, trazo 3, rotado −90°; pista `--text 13 %` (`#363633` sobre skin; hover 26 %); relleno `--accent` (`stroke-dasharray: max(pct,1) 100`, `stroke-linecap: round`), ámbar `--warn` ≥ 60 %, rojo `--danger` ≥ 85 % (`pillQuota.ts:93-94`); logo 16 px al centro; debajo "%": 11 px/600 `--muted`, tabulares ("—" sin datos). Detalle bajo un filete `--line`: nombre 600 `--text`, plan `--faint` 11 px a la derecha, líneas "Ventana …… **NN%** · en 2 h" (11 px; % 650). Transición de trazo 125 ms / dasharray 200 ms. Textos: `pill.quota.*` (`ES:784-808`: "5 h", "Semana", "Opus sem.", "Sonnet sem.", "en {when}", "hace {when}", "Leyendo cupos…").

---

## 5. Ventana de agentes (pizarra) — camino vivo

### 5.1 Ventana

OS window 1120×760 inner (mín. 680×480), marco nativo de Windows (decoraciones por defecto de Tauri; **no** verifiqué si la barra de título es clara u oscura, depende del SO). Contenido `.agents-window { display:flex; height:100dvh; background:var(--bg) #121211; color:var(--text) }` con `AgentsBoard` a pantalla completa + `ToastStack` (`AgentsWindow.svelte`). Tema/idioma llegan por evento (perfil webview propio).

### 5.2 Fondo y plano (`BOARD:1833-1878`)

```css
.board { position:relative; height:100%; overflow:hidden; background-color:#121211;
  background-image: radial-gradient(color-mix(in sRGB,#f0f0ea 13%, transparent) 1px, transparent 1.4px);
  background-size: 24px 24px (× zoom); background-position: cam.x cam.y;   /* dots: 24; grid: 48; plain: none */
  color:#f0f0ea; font-family: var(--rb-font); cursor:grab; }
.board.is-grid { background-image: linear-gradient(to right, rgba(240,240,234,.07) 1px,transparent 1px), linear-gradient(to bottom, rgba(240,240,234,.07) 1px,transparent 1px); }
.plane { position:absolute; top:0; left:0; width:0; height:0; transform-origin:0 0; transform: translate(cam.x px, cam.y px) scale(cam.zoom); }
.plane.is-flying { transition: transform 250ms cubic-bezier(.22,1,.36,1); }   /* encuadres: MIN_ZOOM .15, MAX_ZOOM 1.6 */
```

Insets que tapan los flotantes (para encuadrar): `top 56, right 12, bottom 96, left (lista plegada 52 | 232) + 24` (`BOARD:500-505`).

### 5.3 Tarjeta de consola (`CARD:247-...`)

```css
.card { position:absolute; display:flex; flex-direction:column; overflow:hidden; border-radius:12px; background:#121211;
  box-shadow: 0 0 0 1px rgba(240,240,234,.12), 0 18px 48px -20px rgb(0 0 0/55%); transition: box-shadow 150ms ease; }
.card.is-active { box-shadow: 0 0 0 1.5px rgba(232,232,224,.70), 0 24px 60px -18px rgb(0 0 0/65%); }
.card.is-ping   { animation: card-ping 1100ms cubic-bezier(.22,1,.36,1); }     /* anillo 2px accent 85 %→ ring 18px accent 0 % */
.bar  { display:flex; align-items:center; gap:8px; height:32px; padding:0 6px 0 12px; background:#1e1e1b; color:#9a9a90; font-size:12px; cursor:grab; }
.card.is-active .bar { color:#f0f0ea }
.label { flex:1; font-weight:560; ellipsis }
.close /* y acciones */ { width:24px; height:24px; border-radius:6px; color:#6e6e66; }  .close:hover { background: rgba(240,240,234,.09); color:#f0f0ea }
.body { position:relative; flex:1; min-height:0 }                 /* TerminalView o AgentChatPanel */
.grip.is-e/.is-w { top:32px; bottom:12px; width:8px }  .is-s { left:12px; right:12px; height:8px }  .is-se/.is-sw { 14×14 }
```

Barra (izq→der): `AgentLogo 14` · nombre · `ConsoleStateDot` con texto (ver abajo) · acciones (Forward 12 "Encargar lo seleccionado a otro agente", Paperclip 12 "Archivos de esta sesión") · Maximize2 12 · X 13. Tamaño por defecto **760 × 480**, mín. 420×260; la primera se coloca centrada en el área libre: con ventana 1120×760 ⇒ `x=302, y=120` en coordenadas de pizarra **(derivado, `placeNew` + insets)**; siguientes en cascada +28 px (`CASCADE`), hijos MCP 520×420 a 140 px a la derecha del padre (`agentBoard.ts:462-480`).

`ConsoleStateDot` (`ConsoleStateDot.svelte`): `gap 6`, texto 11 px `--rb-muted`; punto **6 px**; `working`: `--accent`, `pulse 1.2s ease-in-out infinite` (`50%{opacity:.35}`), texto "Trabajando…"; `waiting`: punto `--accent` + `box-shadow 0 0 0 3px accent 30 %`, texto `--rb-text` "Espera tu permiso"; `unread` (ready + atención): punto `--rb-ok` `#6faf88`, texto `--rb-text` "Respondió"; `ready`: punto `rgba(240,240,234,.22)` sin texto; `ended`: sin punto, "Terminó" `--rb-faint`.

Animación de nacer/irse (svelte transition): **in** 320 ms `backOut`, `opacity: min(1, t·1.8)`, `translateY(u·6px) scale(.94+.06t)`; **out** 140 ms `cubicIn`, `opacity t`, `scale(.96+.04t)`; `transform-origin: 50% 60%` (`CARD:83-105`).

### 5.4 Terminal dentro de la tarjeta (`TERMV`)

`.terminal { position:relative; height:100%; background:#121211 }` y `.host { position:absolute; inset: 8px 4px 4px 12px }` (⇒ el lienzo de xterm queda con un marco de `#121211`: 8 arriba, 12 izq., 4 der./abajo; su fondo propio es `#151715`, §6.3). Fuente `BASE_FONT 12.5` px (+zoom), `lineHeight 1.12`, cursor parpadeante, mínimo 80×24 celdas al abrir (`TERMV:130-140`, consts `:73-76`). Arranque: `.boot` texto centrado 12 px `--rb-muted` "Arrancando…".

### 5.5 Lista lateral (`BLIST:272-...`) — flotante, `left:12 top:12 bottom:16`, alineada abajo (`align-items:flex-end`); si `ancho < 1200` sube `bottom: 88`

```css
.list { display:flex; flex-direction:column; gap:8px; width:232px; max-height:100%; padding:8px; border-radius:16px;
  background: color-mix(in sRGB,#1e1e1b 82%, transparent);                      /* ≈ #1c1c19 sobre #121211 */
  box-shadow: 0 0 0 1px rgba(240,240,234,.10), 0 18px 40px -18px rgb(0 0 0/55%);
  backdrop-filter: blur(18px) saturate(1.2); }
.list.is-collapsed { width:52px; padding:6px }
/* top: botón "+ Nueva consola" (h34, radius 8, fondo text 7 %, 13px/600) + fold 34×34 (PanelLeftClose 15) */
/* árbol: cabecera "CONSOLAS ABIERTAS" 11px/600 uppercase ls .02em faint; fila: pad 7 8, radio 10, logo 15 + nombre 12.5px + ConsoleStateDot; activa = text 10 %, hover 6 %; × de 11 px al hover */
/* foot: borde superior text 7 %; chip carpeta (h30, 12px muted) + engranaje 30×30 */
```

Menú "nueva consola" (`ChatPopover` 240 px): filas logo 15 + nombre 12.5 px + "Terminal del sistema". Popover base (`ChatPopover.svelte:pop`): radio 12, pad 6, fondo `--rb-surface-elevated` `#2a2a26`, ring text 10 %, sombra `0 12px 32px -8px rgb(0 0 0/45%)`.

### 5.6 Estado vacío (`BOARD:1612-1680`, estilos `:2081-2240`)

`.empty` centrado (`translate(-50%,-60%)`), ancho `min(520px, 100% − 32px)`. Título 18 px/600 "La pizarra está vacía"; ayuda 13 px `--rb-muted` "Elige la carpeta y cuántas consolas, y toca el agente para abrirlas."; chip carpeta (h34, radio 10, fondo `#1e1e1b`, ring text 8 %, nombre 600 `--rb-text` + ruta 11.5 px faint + ChevronRight 13; hover ring accent 50 %); stepper (h34, radio 10, pad 3, botones 28 px, "1 consola" 12.5/600 mín. 76 px); tarjetas de agente `112 px` de ancho, radio 14, pad `14 8`, gap 8 entre logo (22) y nombre (12.5 px), fondo `#1e1e1b`, ring text 8 %, hover ring accent 50 %, `:active scale .96`; la última, "Terminal" (icono SquareTerminal 22, texto muted).

### 5.7 Composer de la pizarra (`BCOMP:156-289`) — flotante abajo, `bottom:16`, centrado

```css
.composer { display:flex; align-items:flex-end; gap:8px; width:min(420px, 100% − 32px); border-radius:18px; padding:8px 8px 8px 10px;
  background: color-mix(in sRGB,#1e1e1b 82%, transparent);
  box-shadow: 0 0 0 1px rgba(240,240,234,.11), 0 18px 40px -16px rgb(0 0 0/55%); backdrop-filter: blur(18px) saturate(1.2);
  transition: box-shadow 200ms ease-out.., width 180ms smooth-out, translate 180ms smooth-out; }
.composer:focus-within, .composer:has(textarea:not(:placeholder-shown)) { width:min(680px, 100% − 32px);
  transition: … width 340ms cubic-bezier(.33,1.38,.46,1), translate 340ms same; }
.composer:focus-within { translate: 0 -2px; box-shadow: 0 0 0 1px rgba(232,232,224,.55), 0 24px 48px -18px rgb(0 0 0/60%); }
.target { height:28px; padding:0 10px 0 8px; border-radius:999px; background: rgba(240,240,234,.07); font: 560 12px; max-width:180px }  /* logo 13 + nombre */
textarea { font-size:13.5px; line-height:1.45; min-height:28px; max-height:160px; padding:4px 0; color:#f0f0ea } placeholder color #6e6e66
.send { width:30px; height:30px; border-radius:50%; background:#e8e8e0; color:#121211 }   /* ArrowUp 15; deshabilitado: text 12 % + #9a9a90; al habilitarse "send-pop" 320 ms scale .72→1 con ease-island */
```

Placeholder: "Escríbele a Claude Code…" (con destino) / "Toca una consola para escribirle" (sin destino).

### 5.8 Zoom y minimapa

Zoom (`BZOOM:145-...`): `top:12 right:12`; barra `flex, gap 2, pad 4, radius 12`, fondo `#1e1e1b` 82 %, ring text 10 %, blur 18; botones 30 px (radio 8, 11,5 px): Minus 14 · **"100%"** (min 46, `--rb-text`) · Plus 14 · sep · Maximize 14 (ver todas) · sep · 3 acomodos (fila/columna/grilla) · sep · Palette (fondo: Puntos/Cuadrícula/Liso). Quieta: opacidad .4. Minimapa (`BMAP`): `right:12 bottom:16`, caja **184 × 116**, radio 12, mismo vidrio; tarjetas como rectángulos radio 2 (color `var(--agent-accent)` por agente); se oculta si la ventana < 1100 px o está quieto (opacidad 0, vuelve al hover). `.float > * { transition: opacity 200ms ease-out }`.

---

## 6. Float legado y consola con pestañas (`ConsolePanel`)

### 6.1 Float (`FLOAT`)

Ventana-burbuja anclada a la pill (`data-side`), **radio 1.625 rem = 26 px** (`FLOAT:1226-1246`), `overflow:hidden`, fondo transparente (el fondo lo da la piel líquida `--skin` `#1a1a18`, que nace fundida con la pill por un cuello). Tamaños: lanzador **400 × 208** (`SETUP_DEFAULT_W`, `SETUP_H`), consola **680 × 520** (mín. alto 340), selector de carpeta 680 × 620; mín. de burbuja 336×176 (`contract.ts:34-35`); posición guardada, margen 12 px al área útil. La cara equivalente en la isla: 400×188 (lanzador) / 440×496 (consola) (`pillStage.ts:120-123`).
Entrada: `.af` opacidad 0→1 en `--float-close-dur` (100 ms) `smooth-out`; `.af-stage` (contenido) `opacity 0→1` 75 ms y `translateY(−8px) scale(.985)`→none 125 ms, `transition-delay 36 ms`, origen `var(--tail) 0`; cambio lanzador⇄consola anima `left/top/width/height` 200 ms `smooth-out` (`.is-mode-resizing`). Reduced-motion: sin transición.

### 6.2 Lanzador (`LAUNCH`, panel 400 × 208)

Estructura: `.launcher-view` (fondo `--skin` `#1a1a18`) → `header.drag-rail` (alto 2 rem = 32; a la izquierda, si hay consolas vivas, botón "● Consolas activas" 10 px/600 `--rb-muted`, punto 0.35 rem `--rb-ok`; a la derecha ✕ 36×36, X 13) → `.setup` (pad 12, gap 8): picker (`grid repeat(auto-fit, minmax(2.5rem,1fr))` gap 5,6; celda alto 44, radio 9,6; logo 22; hover text 6 %; activa fondo `accent 12 %` = `#333330` + `inset 0 0 0 1px accent 56 %`, logo `scale(1.08)`; no instalado opacidad .42) → `.launch-row` (gap 6,4; **carpeta** flex-1, alto 39,2, radio 9,92, fondo `#262622` 62 % ≈ `#21211e`, Folder 15 + ruta 11,2 px + ChevronRight 14; **stepper** grid `30,4px | texto | 30,4px`, pad 4, mismo fondo, "1 consola" 10,9 px/650; ✕ de "matar consolas" 39,2 si hay vivas) → **CTA** `.launch` (ancho 100 %, alto 40,8, `background: var(--agent-accent)` = `#e8e8e0`, texto `#121211` 11,5 px/700, "Abrir Claude Code" + ArrowRight 14, `margin-top 3,2`; hover mezcla 87 % accent + `#f0f0ea` = `#e9e9e1` y `translateY(−1px)`; active `scale(.96)`; radio sin declarar, §0.8). Contenedor `agents-launcher` (`@container ≤35rem`: pad 9,6; `≤28rem`: rail 28,8 px).
Textos (`ES page.agents.launcher/…`): "Abrir {name}" / "Instalar {name}", "{n} consola(s)", "Carpeta de inicio", "{name} no está en el PATH", "Cerrar y matar las consolas", "Mover ventana", "Consolas activas", "Volver a consolas".

### 6.3 Consola con rail (`CONS`, 680 × 520)

`<section class="console console-desk">` = flex fila: **rail** (izq.) + **col** (barra + cuerpo).

| Elemento | Medidas | Estilo | Cita |
|---|---|---|---|
| rail | ancho arrastrable `RAIL_MIN 60 / DEFAULT 128 / MAX 224` px (compacto <92: solo logos); pad `0.45rem` = 7,2; fondo `#171714` 88 % | sin borde | `CONS:311-313,5496-5510` |
| lista de fichas | `gap .22rem` 3,5 | columna, scroll oculto | `:5514` |
| ficha (`.rail-tab`) | alto **2.7 rem = 43,2**, ancho 100 %, grid `1.72rem (27,5) | 1fr`, gap 6,7, pad `5,6 6,4`, radio **9,6** (la 1ª: esquina sup.-izq. `26 − 7,2 = 18,8`) | logo 18; nombre `.7rem` = 11,2 px/680 `--rb-text`; estado `.625rem` = 10 px/540 `--rb-muted` | `:5521-5600` |
| ficha activa | fondo `color-mix(agent-accent #e85a52 13 %, skin)` = **`#352220`** | (ver §0.9) | `:5533` |
| punto de sesión | 0,32 rem = 5,1 px, arriba-derecha (top/right 5,4); TUI viva `#6faf88` + halo 2 px ok 18 %; chat: trabajando `--accent` late 1,2 s, espera `#d4a84b`, falla `#e85a52` | | `:5601-5628` |
| × de cierre | columna 1.2 rem = 19,2 a la derecha de la ficha; reposo opacidad .6, hover `#e85a52` sobre fondo rec 14 % | | `:5250-5285` |
| botón `+` | `tab-add` 12 px icono, pad 3,84; abre `.add-pop` (min-w 10.5 rem = 168, max-h 20 rem = 320, radio 8, pad 3,5, fondo `#1e1e1b` 96 %) | grupos "SE ABRE EN", "AGENTES" (0,625 rem/680 uppercase ls .05em), filas `0.7rem`/560, logo 14 + nombre + globo MessageSquare 12 ("Abrir X como chat") o chip "Instalar"; luego Consola local, SSH, "Comando…", guardados | `:3475-3682,5286-5460` |
| barra (`.bar`) | alto mín. **2.7 rem = 43,2**, pad `5,12 8,32`, grid `start | acciones` | izq.: `← Agentes` (back-btn, .65 rem/650), logo activo 18 con `session-dot` 6,7 px (`#6faf88` viva), nombre `.7rem`/700 `--rb-text`, chip MCP (Plug 13 + punto: rojo/ámbar/verde), chip carpeta (.65 rem, max 12 rem); der.: reconectar, atajos (Keyboard), uso (Activity), zoom `−  100%  +` (botones 23,2), pin, minimizar, maximizar, cerrar (23,2, hover rec) — botones **1.7 rem = 27,2**, radio 7,2, `--rb-muted` | `:5644-5745,3709-4035` |
| cuerpo (`.body`) | fondo `#121211`; cada `.term` absolute 100 %, fondo `#121211`; splits con costura `1px rgba(246,246,241,.10·.7)` y separador de 9 px que muestra 2 px al hover | | `:4927,4948,5750` |
| velo de arranque | centrado, fondo `#121211` 92 %, logo 28, "Arrancando {name}" 13,1 px/650, ayuda 10,6 px muted (máx 16 rem), spinner 16,8 px borde 1,5 `rgba(154,154,144,.32)` + top `--accent`, gira 0,7 s lineal | `:5059-5115` |
| modo denso (isla, 440 px) | rail pasa a barra **abajo** (fichas 2.4 rem = 38,4 en fila), barra 36 px | `:5821-5905` |

Menú/popover de atajos: 16,5 rem = 264 px, radio 10,4, fondo `#1e1e1b` 96 %, título 11,5 px/650. Cue de zoom: píldora `top 3,4 rem`, "110%" 12 px/650.

Tema del terminal (`terminalTheme.ts:10-58`, **oscuro**; el claro no aplica):

```ts
{ background:"#151715", foreground:"#e8e8e1", cursor:"#e36f52", cursorAccent:"#151715",
  selectionBackground:"rgba(218, 119, 86, 0.35)",
  black:"#22241f", red:"#e0675f", green:"#73b98d", yellow:"#d4ad58",
  blue:"#78a9d4", magenta:"#b18bd0", cyan:"#69b5bd", white:"#d9d9d2",
  brightBlack:"#777970", brightRed:"#f17b71", brightGreen:"#8ed0a4", brightYellow:"#e8c572",
  brightBlue:"#94c0e5", brightMagenta:"#c9a4e3", brightCyan:"#83cbd2", brightWhite:"#ffffff" }
```

Opciones xterm (`CONS:2754-2765`, `TERMV:130-140`): `fontFamily "Cascadia Mono, SFMono-Regular, Menlo, Consolas, monospace"`, `fontSize 12` en el float (10 si el pane <280 px de ancho, 11 si <420; + zoom −5…+12; `Ctrl+rueda`), 12.5 en la pizarra, `fontWeight 400 / bold 700`, `lineHeight 1.12`, `cursorBlink true`, `minimumContrastRatio 4.5`, `drawBoldTextInBrightColors true`, renderer WebGL (`terminalRenderer.ts`). Caret del cursor coral `#e36f52`.

---

## 7. Chat con interfaz (`AgentChatPanel` + conversación)

Ficha de chat = `.chat` (`CHAT:1090`): columna, `height:100%`, `font: 13px var(--rb-font)`, color `--rb-text`, `-webkit-font-smoothing:antialiased`. Dentro: `.scroll` (flex 1, `scrollbar-gutter: stable both-edges`) con `.column.thread` y abajo `.column.dock`. `.column`: **ancho máx. 760 px**, centrada, `padding 0 16px`. `.thread`: columna, **gap 12**, pad vertical 16, `line-height 1.55`.

### 7.1 Vacío (`CHAT:1187-1215`)
Centrado: `AgentLogo 30`, título **"¿En qué trabajamos?"** 17 px/600 (margen sup. 6), sub `12px --rb-muted`: "Claude Code · atic" (nombre · carpeta). Debajo, si hay historial: "Continuar una conversación" (11 px/600 faint) y filas `recent-item` (pad 7 8, radio 8, 12,5 px; preview con elipsis + hora 11 px faint), ancho máx. 440.

### 7.2 Mensaje del usuario (`CONV:64-130`)
Alineado a la **derecha**: burbuja `max-width min(85%, 560px)`, `border-radius 16px 16px 4px 16px`, `padding 8px 12px`, fondo `rgba(240,240,234,.08)`, texto en markdown 13 px. Adjuntos: miniaturas `max 180×120`, radio 10, `outline 1px rgba(255,255,255,.1)`. Origen ("portapapeles", "archivo", dictado…): 11 px `--rb-faint`.

### 7.3 Mensaje del agente (`CONV:32-38`, `MSG`)
**Sin burbuja**: texto corrido a todo el ancho de la columna (`.md`): `13px/1.62`, `gap .5rem` (8) entre bloques, `color --text`.
Markdown (`MSG:67-190`): `strong` peso por defecto; título nivel 1 `15 px/650`; `code` en línea: color **`#e8e8e0`** (`--coral`), `ui-monospace,"Cascadia Mono",Consolas`, `.9em`, sin fondo; `pre`: borde `1px --line`, radio 8, pad `8,8 11,2`, fondo `#262622`, 12 px/1.55, `max-height 16rem`; listas: marcador `--coral`, `padding-left 4,8`, gap 8; tablas: fondo `#262622`, radio 8, celdas `5,6 8,8`, cabecera con `accent 10 %`; hr `1px --line`. Mientras escribe (`is-live`): cursor bloque **0,45 em × 1 em**, radio 1, `#e8e8e0`, `blink 1s steps(2,start) infinite`.
"Pensando…": línea `.working` 12 px `--rb-muted` con **brillo** que barre el texto (`linear-gradient(100deg, muted 35 %, text 50 %, muted 65 %)`, `background-size:250 %`, `shimmer 1.6s linear infinite`, de `100 %` a `0 %`) + tiempo `12.3s` (décimas, `--rb-faint`, tabular).

### 7.4 Bloque de actividad (`ACT`) — plegado por defecto
Una línea: `[● 6px] Leyó 3 · editó 1 · ejecutó 2 [N con error] [ahora: título]  ⌄`, botón `width:fit-content`, `margin-left −8`, pad `4 8`, radio 8, 12 px `--rb-muted` (hover fondo text 6 %, color `--rb-text`); resumen peso 560; punto 6 px `ok 80 %`, rojo si hubo fallos; **en vivo** el punto es `--accent` y late (`pulse 1.2s`, 50 % opacidad .35) y aparece la acción actual en mono 11 px `--rb-faint` (`"pensando…"`, o el título de la última herramienta). Caret: `6×6`, borde `1.5px currentColor`, rota (`transition 160ms cubic-bezier(.2,0,0,1)`). Abierto: cuerpo con `border-left 1px --rb-border`, `margin-left 2`, `padding-left 12`, gap 4; pestañas por clase "Todo · Razonó · Leyó · Editó · Ejecutó · Otros" con contador (11,5 px, radio 7, activa fondo text 9 %). Pensamiento: 12 px itálica `--rb-muted`.
Al terminar el turno (`WORK`): línea "**Trabajó 42s**  US$ 0.31  ⌄ ————" (12 px, peso 560, costo 11 px faint, regla de 1 px `text 8 %` a la derecha); debajo `EditedFiles`: caja radio 12, fondo `text 3 %` + ring `text 8 %`, cabecera con cuadraditos 7 px (verde/rojo) "3 archivos cambiados" y `+18 −4` mono 11 px (verde `#6faf88` / rojo `#e85a52`), filas `nombre.ts` mono 11,5 px + carpeta `--rb-faint` a la derecha, "Ver 2 más".

### 7.5 Tarjeta de herramienta (`TOOL`) — la que aparece al abrir el bloque

```css
.tc { border:1px solid rgba(246,246,241,.10); border-radius:9px; background:#262622; overflow:hidden }
.tc.is-error { border-color: color-mix(in srgb, #e85a52 55%, rgba(246,246,241,.10)) }   /* se abre sola al fallar */
.tc-head { display:flex; align-items:center; gap:.5rem; padding:.42rem .6rem;           /* 8px; 6.7px 9.6px */
  font: .75rem ui-monospace,"Cascadia Mono",Consolas,monospace; color:#f0f0ea; text-align:left }
.tc-st { width:.4rem; height:.4rem; border-radius:999px; background:#6e6e66 }             /* 6.4px: pendiente gris */
.tc-st.is-run { background:#e8e8e0; animation: tc-pulse 1.4s ease-in-out infinite }       /* opacidad .3↔1 */
.tc-st.is-ok  { background:#6faf88 }   .tc-st.is-bad { background:#e85a52 }
.tc-kind { color:#6e6e66 }             /* AgentIcons 11px, trazo 1.8 */
.tc-name { color:#f0f0ea }             /* "Read" · "Edit" · "Bash" · "Grep" */
.tc-arg  { flex:1; color:#9a9a90; ellipsis por la izquierda (direction:rtl + <bdi>) }   /* ruta/comando: src/lib/pill/pillStage.ts */
.tc-num  { font-size:.6875rem; tabular }  .add{#6faf88}  .del{#e85a52}                  /* "+4 −1" solo en ediciones */
.tc-caret { color:#6e6e66; font-size:.625rem }  /* "⌄" cerrada / "⌃" abierta */
.tc-body { border-top:1px solid rgba(246,246,241,.10); padding:.45rem .6rem .55rem }
.tc-diff { max-height:15rem; border-radius:6px; background:#262622; font: .71875rem/1.5 mono; overflow:auto }
.dl { display:flex; gap:.5rem; padding:0 .5rem; white-space:pre }
.dl[data-sign="+"] { background: color-mix(in srgb,#6faf88 12%,transparent) }  .dl[data-sign="-"] { background: color-mix(in srgb,#e85a52 12%,transparent) }
.tc-cmd,.tc-json,.tc-out { max-height:12rem; border-radius:6px; padding:.4rem .5rem; background:#262622; color:#9a9a90; font: .71875rem/1.5 mono; white-space:pre-wrap }
.tc-cmd { color:#f0f0ea }   /* "$ cargo check" */     .tc-out.is-error { color:#e8a496 }
.tc-loc span { border-radius:4px; padding:.05rem .35rem; background:#262622; color:#6e6e66; font-size:.625rem }
```

Ejecutándose muestra "ejecutando…" (`.tc-wait`, mono 11 px faint). **Subagente** (`COLLAB`): mismo marco (radio 9), punto de estado 6,4 px, dos líneas: "Subagente · {tipo}" (10 px faint) y título (12 px `--text`), resumen mono 11 px con `border-top`.
Ejemplo realista (para el video, ver §8): `● Read src/lib/surfaces/overlay/pillStage.ts` (punto verde) → `● Edit src/lib/…/PillSurface.svelte +4 −1` → `● Bash cargo check --locked -p atic-desktop` (punto pulsando `#e8e8e0` hasta terminar).

### 7.6 Composer del chat (`CHAT:1364-1530`, `dock` pad-bottom 12, mismo eje 760 px)

```css
.composer { position:relative; display:flex; flex-direction:column; gap:6px; border-radius:16px; padding:10px 10px 8px 14px;
  background:#262622;                                                      /* --rb-surface-2 */
  box-shadow: 0 0 0 1px rgba(240,240,234,.09), 0 10px 28px -18px rgb(0 0 0/55%); transition: box-shadow 140ms ease; }
.composer:focus-within { box-shadow: 0 0 0 1px rgba(232,232,224,.45), 0 10px 28px -18px rgb(0 0 0/55%); }
textarea { font-size:13.5px; line-height:1.5; min-height:22px; max-height:200px; padding:2px 0 } placeholder #6e6e66  /* "Escríbele a Claude Code…" */
.tools { display:flex; align-items:center; gap:2px; margin-left:-6px }
.tool { width:28px; height:28px; border-radius:8px; color:#9a9a90 }            /* Paperclip 14 · Historial */
.model-group { display:flex; margin-left:2px; border-radius:9px; background: rgba(240,240,234,.05) }   /* [logo 14 + "Opus 5" ⌄] | 1×14 sep text 12 % | [medidor de esfuerzo ▁▃▅ "High"] */
/* selector de modo (solo Claude): ChatSelect "Manual" ⌄ (popover 240: Manual / Edits / Plan / Bypass con notas) */
.send { width:30px; height:30px; margin-left:auto; border-radius:8px; background:#e8e8e0; color:#121211 }       /* ArrowUp 15; disabled text 12 %; parar = Square 11, text 14 % */
```

Triggers de `ChatPopover`: alto 28, radio 8, pad `0 8`, 12 px `--rb-muted`, hover text 8 %. Selector agente·modelo: ancho 320 (pestañas de agentes con logo 15 y punto en el actual, lista de modelos con descripción, buscador si >N). Esfuerzo (`EffortSlider`): medidor de barras + etiqueta ("Default / Low / Medium / High / Extra High / Max"). Modos (`agentModels.ts:46-55`): "Manual" – "Pregunta cada acción", "Edits" – "Acepta ediciones de archivos", "Plan" – "Solo lectura y planificación", "Bypass" – "Sin prompts (solo entornos aislados)".
Error visible: caja radio 12, fondo `rec 10 %` sobre `#1e1e1b` (≈`#322421`) + ring rec 30 %, 12 px, máx 4 líneas.

---

## 8. Ejemplos de contenido realista (para el video)

Redactados para el video (no son datos del repo salvo que se indique). Nombres de archivos reales de Atic.

**Mensaje del usuario** (burbuja derecha): "Revisa por qué la pill vuelve al hogar convertida en barra ancha cuando se cierra el panel de textos." *(frase de `docs/demos/agentes.html:221-224`)*

**Respuesta del agente** (markdown, 13 px):
> El colapso mide la barra mientras todavía está estirada al ancho del panel. `collapse()` vuelve `barW` a `PILL.bar` antes de leer `target`, pero el `ResizeObserver` alcanza a disparar una vez más.
> - Medir solo cuando `surface === "none"`
> - Añadir un test de regresión en `pillPlan.test.ts`

*(primer párrafo: `agentes.html:225-231`; adaptado)*

**Bloque de actividad plegado**: `● Leyó 2 · editó 1 · ejecutó 1` → abierto: `Read pillStage.ts` (título = `file_path`), `Edit PillSurface.svelte` con `+4 −1` y diff `- barW = target;` / `+ barW = PILL.bar;`, `Bash cargo check --locked -p atic-desktop` (salida `Finished dev profile … in 4.2s`). Cierre: "Trabajó 42s", "3 archivos cambiados +4 −1".
Mostrar tarjeta `Read · pillStage.ts` = "157 líneas · windowFor, growsFirst, createStage" es de la maqueta (`agentes.html:232-239`), no de la app.

**Permiso**: "Claude Code quiere usar Bash" con `$ cargo test --locked -p atic-desktop pill_plan` y botones "Rechazar · Aprobar siempre · Aprobar".

**Pregunta** (`ChatQuestion`): chip "Alcance", "¿Aplico el arreglo también al modo flotante?", opciones "Solo acoplado" / "Ambos" (con descripciones), "O escribe tu respuesta…", "Saltar · Enviar respuestas".

**Aviso al terminar**: chip "Listo" o la última frase (recortada a ~18 caracteres + `…`), p. ej. "Arreglado el colapso de…".

TUI (no está en el repo): sugerencia de cómo dibujarlo en xterm — fondo `#151715`, texto `#e8e8e1`, prompt/acento `#e36f52`/`#da7756`; Claude Code imprime al arrancar su mascota (la ASCII que decodifica `AgentMark.svelte:1-20`, retícula 16×5 de "medios bloques") y una caja de bienvenida. No inventes más de lo necesario.

---

## 9. Textos exactos en español (Chile) — `ES`

**Herramienta** (`ES:39-42`): label "Agentes" · short "Terminales con agentes" · blurb "Tus agentes CLI en terminales con pestañas, splits y sesiones." · acción "Abrir consola".

**Pizarra / ventana** (`ES:1288-1391`): "Nueva consola" (`board.new`), "Nueva consola (Ctrl+N)", "Consolas abiertas", "Escríbele a {name}…", "Toca una consola para escribirle", "La pizarra está vacía", "Elige la carpeta y cuántas consolas, y toca el agente para abrirlas.", "Se termina su proceso y no se puede deshacer.", "Achicar la lista" / "Mostrar la lista", "Vista de la pizarra", "Acercar (Ctrl +)", "Alejar (Ctrl −)", "Tamaño real", "Ver todas (Ctrl 0)", "Acomodar una junto a otra / una debajo de otra / en grilla", "Maximizar (doble clic en la barra)", "Volver a su tamaño", "Mapa de la pizarra", "Sesiones guardadas", "Encargar lo seleccionado a otro agente", "Lo seleccionado en la consola va como encargo:", "Archivos de esta sesión", "Fondo de la pizarra": "Puntos / Cuadrícula / Liso"; `window.*`: "Terminal del sistema", "Terminal", "Carpeta de inicio", "Carpeta donde trabajan los agentes", "Arrancando…", "terminó · código {code}", "Cerrar {name}", "¿Cerrar {name}?", "Todavía está trabajando. Se corta lo que esté haciendo y no se puede deshacer.", estados: "Terminada", "Espera tu permiso", "Con error", "Trabajando…", "Respondió", "Lista", "Terminó". Carpeta: "Elegir carpeta", "Usar esta carpeta", "Subir", "Ubicaciones frecuentes", "Subcarpetas", "Favoritos".

**Lanzador** (`ES:1259-1280`): "Abrir agentes", "Mover ventana", "Agente", "Cantidad de consolas", "Menos consolas", "Más consolas", "Cerrar y matar las consolas", "Cierra las consolas y mata los procesos", "{name} no está en el PATH", "{name} no está instalado", "↑↓ elige · Enter abre", "Elegir agente…", "Nueva consola", "Cerrar las consolas", "¿Cerrar las consolas?", "Se cortan los procesos que estén corriendo, con lo que estén haciendo. No se puede deshacer."; genéricos `page.agents`: "Abrir {name}", "Instalar {name}", "Volver a consolas", "Consolas activas", "consola"/"consolas", "Instalar", "Reabrir", "Conectar", "Agentes", "Esconder. Las consolas siguen corriendo.", "«{label}» terminó su turno", "Arrancando {name}", "Arrancando consola", "El terminal espera a tener tamaño. El agente pinta cuando está listo.".

**Consola con rail** (`ES:1632-…`): "Consola", "Se abre en", "Agentes", "Carpeta de inicio", "Sesión activa", "Consolas abiertas", "{n} consolas", "Cerrar pestaña", "Nueva consola o agente (Ctrl+N)", "Consola local", "Consola SSH", "Comando…", "Guardados", "Arrastra para cambiar el ancho · Doble clic para contraer", "Alejar el texto / Acercar el texto / Restablecer el zoom del texto", "Volver al lanzador", "Sacar a la ventana", "Devolver a la pill", "Fijar ventana arriba" / "Desfijar ventana", "Sin consolas", estados de ficha "Activa" / "Preparando" / "Pausada", menú contextual "Copiar · Pegar · Sacar del grupo · Cerrar consola · Separar grupo · Cerrar grupo", confirmación "Cerrar «{label}» — Se cierra la consola y su proceso. No se puede deshacer.", MCP: "Activar MCP para comunicar agentes de distintos proveedores. Clic aquí." / "Este agente está conectado al hub de Atic y puede hablar con los demás. Clic para desconectarlo.", atajos: "Atajos de la consola", "Funcionan aunque el foco esté en el terminal.", "Dividir a la derecha", "Dividir hacia abajo", "Nueva consola o agente", "Cerrar pestaña", "Zoom del texto (también Ctrl+rueda)", "Restablecer zoom".

**Chat** (`ES:1392-1631`): "¿En qué trabajamos?", "Escríbele a {name}…", "Pensando…", "Enviar", "Detener", "Adjuntar archivos", "Agente y modelo", "Cargando modelos…", "Esfuerzo", "Volver al esfuerzo por defecto", "Por defecto", "Modo de permisos", "Sin coincidencias", "Continuar una conversación", "Plan", "Resumen del contexto", "{name} quiere usar {tool}", "Historial", "Buscar en tus conversaciones", "No hay conversaciones guardadas.", "Servidores MCP", "En uso", "Usar", "Buscar modelo", "Usa su modelo por defecto.", "Rápido", "Variante rápida del mismo modelo", "Esta sesión ya no está viva.", "Todavía no ha dicho nada.", "{name} · pedido por {parent}", "Parece un problema de credenciales de {name}. Prueba con otro modelo o vuelve a iniciar sesión en su CLI.", preguntas: "{name} te pregunta", "O escribe tu respuesta…", "Saltar", "Atrás", "Siguiente", "Enviar respuestas", "Anterior", "Siguiente", selección: "Citar", "Copiar", actividad: "leyó {n}", "editó {n}", "ejecutó {n}", "buscó {n}", "{n} subagentes", "{n} acciones", "pensó", "pensando…", "{n} con error", "1 archivo cambiado", "{n} archivos cambiados", "Ver {n} más", "Ver menos", pestañas "Todo/Razonó/Leyó/Editó/Ejecutó/Otros", trabajo "Trabajó {t}", "Detenido tras {t}", "Falló tras {t}", plan "Plan de {name}", "Rechazar", "Aprobar plan", estados de ficha "Terminada/Espera tu permiso/Con error/Trabajando…/Respondió/Lista".
Permisos (`ES:1281-1287`): "El agente espera tu permiso", "Abrir consola", "Rechazar", "Aprobar", "Aprobar siempre".

**Genéricos**: `chrome.close` "Cerrar", `chrome.minimize` "Minimizar", `chrome.maximize` "Maximizar", `chrome.restore` "Restaurar", `chrome.dismiss` "Descartar".

---

## 10. Animaciones (resumen para Remotion, 30 fps)

| Qué | Duración | Curva / detalle | Cita |
|---|---|---|---|
| Ventana de agentes al abrirse | (SO; primera vez ~1,8 s, 0 si precalentada) | mostrar ventana nativa | `agents_window.rs:36-68` |
| Tarjeta nace / se va | 320 ms `backOut` / 140 ms `cubicIn` | `translateY(6→0) scale(.94→1) opacity min(1,1.8t)` / `scale(.96→1)` | `CARD:83-105` |
| Ping de atención de la tarjeta | 1100 ms | anillo 2 px accent 85 % → 18 px 0 % | `CARD:260-283` |
| Punto de estado "trabajando" | 1200 ms bucle | opacidad 1 ↔ .35, `ease-in-out` | `ConsoleStateDot` |
| Punto de tarjeta de herramienta "corriendo" | 1400 ms bucle | opacidad .3 ↔ 1 | `TOOL:tc-pulse` |
| Composer pizarra: ensanchar 420→680 | 340 ms `cubic-bezier(.33,1.38,.46,1)` + sube 2 px; encoger 180 ms `smooth-out` | | `BCOMP:180-198` |
| Botón enviar habilitado | 320 ms `ease-island`, `scale .72→1` | | `BCOMP:272` |
| Chispa de envío | 520 ms `cubic-bezier(.22,1,.36,1)` | trayectoria: botón → (+50 %, −40 px) escala 1,1 → tarjeta escala .4, opacidad 0→1→0 | `BOARD:865-895` |
| Cámara de la pizarra al encuadrar | 250 ms `smooth-out` | | `BOARD:1883` |
| Chip de pill "listo" entra | 250 ms `smooth-out` | opacidad 0→1, `translateY(4→0)`, `blur(2→0)` | `PS:8919` |
| Logo del chip / cue "trabajando" | T = 1,8 s | opacidad `.55 + .45·(.5 − .5·cos)` | `PS:8776,7846` |
| Piel de la pill "respira" | T = 2,4 s | brillo 1,00 ↔ 1,08 | `Skin.svelte:148` |
| Tarjeta de permiso (`.float-emerge`) | 150 ms abrir / 100 ms cerrar | `scale .55→1`, `translateY(±18)`, `smooth-out`, origen en la pill | `APP:270-302` |
| Float legado: contenido | opacidad 75 ms, transform 125 ms, delay 36 ms; caja 100 ms | `translateY(−8) scale(.985)` | `FLOAT:1262-1290` |
| Float: cambio lanzador ⇄ consola | 200 ms | left/top/width/height `smooth-out` | `FLOAT:1247-1254` |
| "Pensando…" (chat) | 1600 ms bucle | shimmer lineal | `CHAT:1276` |
| Cursor del texto en vivo | 1000 ms `steps(2,start)` | | `CONV:blink` |
| Punto de bloque de actividad en vivo | 1200 ms | opacidad 1↔.35 | `ACT:pulse` |
| Toast | 200 ms | `fly y:8`; caja `max-w 400`, radio 8, `--elevated`, borde `--line`, `shadow-pop`, texto 12 px | `ToastStack.svelte:87-101` |
| Menús (`pop-in-down`) | 125 ms (contextual 75 ms) `smooth-out` | `translateY(−4) scale(.98)`→none | `CONS:pop-in-down` |

Preferencia del dueño (memoria de proyecto): al inventar movimientos nuevos favorece ~400-650 ms suaves y simétricos; pero **lo que ya existe en el código va con los tiempos de arriba**.

---

## 11. Bloque CSS/TS copiable

```css
:root {
  --bg:#121211; --surface:#1a1a18; --surface-2:#1e1e1b; --elevated:#262622;
  --text:#f0f0ea; --muted:#a8a89e; --faint:#8f8f86;
  --line:rgb(240 240 234 / 10%); --line-strong:rgb(240 240 234 / 18%);
  --accent:#e8e8e0; --on-accent:#121211; --skin:#1a1a18;
  --rec:#e85a52; --ok:#6faf88; --warn:#d4a84b; --info:#8fa9b8;
  /* legado (agentes) */
  --rb-bg0:#121211; --rb-surface:#1e1e1b; --rb-surface-2:#262622; --rb-surface-elevated:#2a2a26; --rb-sidebar:#171714;
  --rb-text:#f0f0ea; --rb-muted:#9a9a90; --rb-faint:#6e6e66;
  --rb-border:rgba(246,246,241,.10); --rb-record:#e85a52; --rb-ok:#6faf88; --rb-warn:#d4a84b;
  --font-sans:"Aptos","Avenir Next","Helvetica Neue","Segoe UI Variable",sans-serif;
  --font-mono:"Cascadia Mono","SFMono-Regular","Roboto Mono",monospace;
  --ease-smooth-out:cubic-bezier(.22,1,.36,1); --ease-island:cubic-bezier(.33,1.38,.46,1);
}
```

```ts
export const AGENT_UI = {
  window: { w: 1120, h: 760, minW: 680, minH: 480 },
  card: { defaultW: 760, defaultH: 480, minW: 420, minH: 260, bar: 32, radius: 12, cascade: 28, childW: 520, childH: 420 },
  list: { w: 232, wCollapsed: 52, radius: 16, insetLeft: 12, insetTop: 12, insetBottom: 16 },
  composer: { wIdle: 420, wOpen: 680, radius: 18, send: 30, bottom: 16 },
  minimap: { w: 184, h: 116 },
  float: { setup: [400, 208], console: [680, 520], radius: 26, rail: { min: 60, def: 128, max: 224 } },
  chip: { minH: 21.6, logo: 11, font: 10, maxW: 152 },
  cue: { btn: 26, logo: 18, msgW: 96 },
  pending: { w: 360, gap: 8, radius: 16 },
  chat: { colMax: 760, gutter: 16, gap: 12, composerRadius: 16, bubbleRadius: "16px 16px 4px 16px" },
} as const;
export const AGENT_COLORS = { claude: "#da7756", opencode: "#7fae86", codex: "#8fa9b8", cursor: "#a88fc4" } as const;
```

---

## 12. No encontrado / sin verificar

- Captura o medición real de la ventana de agentes (no reinicié la Atic del usuario ni abrí CDP; todo es lectura de código). Sobre todo sin verificar: barra de título nativa (claro/oscuro), radio real de `.launch`, alto exacto de la lista lateral (depende de las entradas), color de fondo de los tooltips (`tip`), y la altura real de la tarjeta de permiso (el código estima 88).
- El contenido interno del TUI de Claude Code / Codex / Cursor / OpenCode (lo pinta el CLI).
- Un disparador vivo del float/`ConsolePanel` con rail (ver §0.2).
- Sonidos/notificaciones del SO al terminar un turno (existen `beep.rs` y notificación del sistema según `Features/agentes.md`; no son visuales).
- Nombres de modelo visibles (p. ej. "Opus 5", "GPT-5 Codex") salen de `agentListModels` en tiempo de ejecución; los usados en `chatThread.test.ts:180-183` son datos de prueba.
