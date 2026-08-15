---
target: la pill
total_score: 22
p0_count: 0
p1_count: 4
timestamp: 2026-08-13T20-12-28Z
slug: p-src-lib-surfaces-overlay-pill-pillsurface-svelte
---
# Critique: pill (PillSurface)

Target: `apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte`
Method: dual-agent (A: 60575da2-9295-4ed0-8962-683e9cf8b12f · B: a9994a91-34a6-4b10-8af9-5e75652ad8ce)

## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Timer, waveform y chips de grabación/dictado; el stop desaparece al abrir la rueda |
| 2 | Match System / Real World | 2 | Hold-to-select no es Raycast; mezcla Clipboard/Textos/Apps; disco «a» sin semántica de herramienta |
| 3 | User Control and Freedom | 2 | Esc y clic-fuera cierran; clic en esquina recorta la rueda; cola/grabación bloquean abrir por clic |
| 4 | Consistency and Standards | 2 | Tres selectores (rueda compacta, hub ParticleWheel, ToolRail); launcher en la rueda vs PRODUCT.md |
| 5 | Error Prevention | 2 | Sin preselección al abrir (bien); núcleo = Abrir Atic; release-on-hover fácil de fallar |
| 6 | Recognition Rather Than Recall | 1 | Icon-only; caption apagado a propósito; hay que recordar el atajo radial |
| 7 | Flexibility and Efficiency | 3 | Atajos globales, radial press/release, rueda del mouse, skip-flight, pin de floats |
| 8 | Aesthetic and Minimalist Design | 3 | Idle excelente; rueda de seis blobs + overshoot bounce |
| 9 | Error Recovery | 2 | Chip Error de grabación sin acción; IPC a console.warn; auth sin Esc |
| 10 | Help and Documentation | 2 | UsageGuide enseña 3 atajos, no el clic; title nativo tardío |
| **Total** | | **22/40** | **Acceptable** |

## Anti-Patterns Verdict

**LLM assessment:** El disco idle no parece IA: es Dynamic Island / tray con oficio. La rueda compacta sí: seis gotas icon-only, caption con `display: none`, nombre solo en `title` nativo. Eso no es Spotlight; es un radial inventado. Overshoot `--morph-ease: cubic-bezier(0.34, 1.25, 0.64, 1)` es bounce (ban de motion). El goo no es glassmorphism decorativo.

**Deterministic scan (CLI):** exit 2, 2 warnings, ambos en `docs/demos/pill.html` (`flat-type-hierarchy`, `em-dash-overuse`). Cero hallazgos en `PillSurface.svelte`, `AgentAuthCard.svelte`, `OverlaySurface.svelte`, `ParticleWheel.svelte`. Los dos hits del demo son falsos positivos para esta superficie de producción.

**Visual overlays:** Inyección de `detect.js` en `http://localhost:1420/overlay` (tab [Human]) **sí corrió**. Consola `[impeccable]`: `clipped-overflow-container` en `div.p-root` (48×48, overflow hidden recorta un hijo posicionado); `bounce-easing` `cubic-bezier(0.34, 1.25, 0.64, 1)` en `body`; `layout-transition` `transition: width` en `body`. El grupo decía 2; se loguearon 3 reglas. Overlay de detector visible (banner amarillo + outline en la pill colapsada). Preview web, no la ventana Tauri always-on-top.

**Acuerdo A+B:** recorte de la rueda (LLM: geometría en esquina; detector: clipped-overflow en `.p-root`) y bounce (LLM: overshoot; detector: bounce-easing). El detector CLI no vio esos problemas porque viven en CSS computado / DOM vivo, no en el markup estático de la pill.

## Overall Impression

La pill idle es la mejor pieza del producto: una marca, arrastre, no interrumpe. El acto que importa —abrir herramientas— se rompe en reconocimiento (iconos sin nombre) y en geometría (clic en el hogar del borde recorta o desplaza la rueda). El atajo radial al cursor es el camino que sí encaja con Atic; el clic no.

## What's Working

1. **Tres ejes ortogonales** (`activity` × `surface` × `queue`): grabar no debería morir al abrir historial. El modelo de estado es correcto; el chrome de la rueda todavía no lo honra.
2. **Radial sin preselección** (`wheelTool = null`, soltar vacío = close) + `FLIGHT_SKIP_PX = 48` + `prefers-reduced-motion` cableado.
3. **Disco idle:** una marca, drag con cursor de Rust, hogar persistido. Cumple «no interrumpir».

## Priority Issues

### [P1] Rueda compacta icon-only
**What:** `.pw.is-compact .pw-caption { display: none }` — el caption existe y se apaga porque «no entra» en 232px. Queda `title` nativo.
**Why it matters:** Seis destinos equivalentes sin nombre. Jordan no sabe qué toca; Alex espera labels como Raycast. El ToolRail de la ventana principal sí pone «Reuniones» + «Grabar».
**Fix:** Mostrar el label en el núcleo al hover/focus (`activeTool?.label`). Si hace falta espacio, subir `PILL.wheel` y una pastilla bajo el anillo (el propio comentario en `ParticleWheel.svelte` lo propone).
**Suggested command:** `$impeccable clarify` (labels) + `$impeccable layout` (espacio del núcleo)

### [P1] Clic en el disco del rincón recorta (o desplaza) la rueda
**What:** Clic ⇒ skip-flight ⇒ resize pivot center. En Chrome el root pasó a 252×252 en `(-102, -102)` y solo se vieron 3 gotas. Detector: `.p-root` 48×48 `overflow: hidden` recorta hijos posicionados. En Tauri, `clampTo` metería el cuadrado 252px y el núcleo dejaría de coincidir con el disco.
**Why it matters:** El camino «clic donde vive» pelea con el hogar pegado al borde (`MARGIN = 0`). El atajo radial vuela al cursor y lo esquiva; el clic no.
**Fix:** El clic debe ser el mismo acto que el radial: `flyTo(cursor)` (o inset interior del work area) **antes** de revelar gotas. No morflear in-situ si el cuadrado no cabe alrededor del hogar.
**Suggested command:** `$impeccable adapt`

### [P1] Stop de grabación y rueda no son ortogonales en UI
**What:** `endDrag` solo abre rueda si `activity === "idle" && !hasQueue`. Con rueda abierta, `.p-stack` está `inert` + `aria-hidden`: el recDot «Detener grabación» desaparece.
**Why it matters:** En una reunión, abrir herramientas esconde el stop. Viola el modelo de tres ejes y el principio «no interrumpir» / control del usuario.
**Fix:** Stop persistente fuera del stack. Quitar el bloqueo de clic por activity/queue: la marca sigue visible; que también abra la rueda.
**Suggested command:** `$impeccable harden`

### [P1] Launcher en la rueda (6º gajo = Apps)
**What:** `TOOLS` incluye `launcher`. Con `AGENTS_ENABLED = false` el 6º gajo es Apps. PRODUCT.md: seis tools + launcher **fuera**. UsageGuide enseña Spotlight por atajo. El núcleo ya es «Abrir Atic».
**Why it matters:** Duplica el rol de salir al SO; 7 destinos en el anillo; primer nivel mezcla grabar/dictar con Spotlight.
**Fix:** Sacar `launcher` de `TOOLS` / ParticleWheel. Dejar `Ctrl+Space` + tray. Núcleo = cerrar / idle, no main.
**Suggested command:** `$impeccable distill`

### [P2] Dos modelos mentales + núcleo peligroso
**What:** Onboarding = mantener atajo + scroll + soltar. Disco = clic. Centro = `closeWheel` + `showMainWindow()`.
**Why it matters:** Un miss hacia el núcleo abre la ventana grande y rompe el escritorio (principio 1).
**Fix:** Unificar clic = radial al cursor. Núcleo = cerrar. Main por doble clic, bandeja o atajo.
**Suggested command:** `$impeccable onboard`

## Cognitive Load

Fallos: 7 de 8 (carga alta). Single focus, chunking, grouping, hierarchy en rueda, one-thing, minimal choices, working memory, progressive disclosure.

Puntos >4 opciones: rueda abierta (6 tools + núcleo = 7); radial hover/scroll (6 + soltar vacío); bandeja (5).

## Emotional Journey

Arranque: círculo «a» sin rol de botón — valle para Jordan. Pico: vuelo 110ms al cursor con gotas (Alex + atajo). Valle: clic en esquina recorta gotas. Alta exigencia: waveform tranquiliza; abrir rueda apaga el stop. Peak-end: elegir tool cierra en 60ms (nítido); clic en núcleo abre main (amargo).

## Persona Red Flags

**Alex:** Atajos y radial bien. Clic en casa peor que el atajo. Summon reescribe `home` (no es «traer», es «mudar»). Centro → ventana principal es un trap. Launcher fused-grow ~0.9s con favs.

**Jordan:** Disco no es botón (`role` null, `tabindex` -1). Toolbar y 6 botones ya en el árbol a11y con rueda cerrada. Sin labels. UsageGuide no enseña el clic. Bandeja abre main, no la pill.

**Sam:** Idle no es control. Foco va a `.pw-nodes` toolbar, no a un gajo. Overlay no-activable: teclado in-rueda depende de que Rust dé foco. `svelte-ignore a11y_no_static_element_interactions` en `.p-root`.

**Riley:** Geometría esquina + pivot center. Multi-monitor/DPI pendiente. Clipboard desde rueda en Chrome: cierra y cero float/error. Auth usa `.float-emerge`, no fused grow.

**Reunión:** Dictado listening = barra es stop (bien). Grabación: abrir rueda esconde el stop. Chip «BT» opaco.

**Dev CLI:** `AGENTS_ENABLED = false`; AgentAuthCard es riesgo aplazado (4 acciones, sin Esc, `allowAlways` junto a aprobar).

## Minor Observations

- Hover débiles en `.p-rec`, `.p-dict-wave`, `.p-queue-btn`, `.p-agent`.
- Chips uppercase 0.5625rem, max-width 3.5rem, ellipsis.
- Cola: un ítem a la vez; no hay forma de ver el 2º sin pegar/descartar el 1º.
- Docs dicen flight 150ms; `app.css` tiene `--flight-dur: 110ms`, morph open 110, close 100, quick 60.
- PRODUCT.md vs pill-liquid-emerge: morph launcher «incompleto» vs «hecho».
- `/pill` 404; glob listaba `routes/pill/+page.svelte` pero el FS no lo ve.

## Flow Map

```
home persistido --drag--> nuevo home
clic (solo idle ∧ ¬queue) | atajo radial Press
        \                    /
         v                  v
    skip-flight si dist<48   flyTo(cursor)
                     \      /
                      WHEEL  (6 gajos + núcleo)
         hover/scroll/flechas -> wheelTool
         Release/Enter/clic | Esc / release vacío
                      |
         meetings / captures / dictation / launcher / clipboard / snippets
         núcleo: closeWheel + showMainWindow()
                      |
         floats close reverse -> wait hit-rect -> flyTo(home)
RECORDING / DICTATING ejes paralelos (estado sí, chrome no)
QUEUE bloquea clic→rueda
AUTH solo si agentes on
```

## Questions to Consider

- Si el clic hiciera exactamente lo que el atajo (aparecer en el cursor), ¿haría falta el morph in-situ?
- El caption del núcleo ya nombra la tool: ¿por qué se apaga en el único sitio donde hace falta?
- ¿El launcher es hermano Spotlight o el 6º gajo?
- ¿El núcleo es marca, cierre, home de la app, o launcher?
- Cuando alguien graba una reunión y abre la rueda, ¿dónde vive el stop?
