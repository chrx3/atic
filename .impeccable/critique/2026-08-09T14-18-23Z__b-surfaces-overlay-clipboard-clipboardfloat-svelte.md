---
target: clipboard float / drag-to-input
total_score: 19
max_score: 40
na_heuristics: 
p0_count: 2
p1_count: 2
timestamp: 2026-08-09T14-18-23Z
slug: b-surfaces-overlay-clipboard-clipboardfloat-svelte
---
Method: dual-agent (A: 18030ed8-7720-4bd6-a9a8-c58e97edaf25 · B: 8772a9da-edbc-454e-9242-b0fdc6431693)

#### Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 2 | Drag fallido / sin destino = silencio; float no cablea `onError` |
| 2 | Match System / Real World | 2 | Promete drag-to-app; texto no usa OLE/`clipboard_drag_path` |
| 3 | User Control and Freedom | 3 | Esc/pin/close OK; delete sin undo; gestos sin salida de error |
| 4 | Consistency and Standards | 2 | Texto=HTML5 vs imagen=OLE; pin float ≠ estrella Fav |
| 5 | Error Prevention | 1 | Mismo hit pegar/arrastrar; fallback mudo sobre apps del SO |
| 6 | Recognition Rather Than Recall | 2 | Solo `title` + grab; hay que recordar cuándo funciona el drag |
| 7 | Flexibility and Efficiency | 2 | Clic-pegar rápido; drag (job power) incompleto |
| 8 | Aesthetic and Minimalist Design | 3 | Float Operate limpio, familia líquida |
| 9 | Error Recovery | 1 | Sin toast/`onError` en el float; drag muerto sin recuperación |
| 10 | Help and Documentation | 1 | Sin copy in-UI del flujo “arrastra a otra app” |
| **Total** | | **19/40** | **Poor** |

#### Design Specificity Verdict

**LLM assessment:** El chrome es Atic (emerge líquido, pin, overlay). La lista es un historial genérico. El claim diferenciador —arrastrar texto/imagen a un input real— no está diseñado como primaria visible; el split HTML5/OLE y el click-through del overlay hoy rompen ese job más de lo que lo habilitan.

**Deterministic scan:** `detect.mjs` exit 0, `[]` — 0 hallazgos en `ClipboardFloat.svelte` y `ClipboardHistoryList.svelte`. El detector no ve el fallo de interacción cross-window (está fuera de reglas visuales). Smoke check del CLI OK (Inter → `overused-font`).

**Visual overlays:** No disponibles. Browser visualization omitida: float es overlay Tauri; Vite en `[::1]:1420` sirve el shell, no el float real.

#### Overall Impression

El float se ve y se abre bien en la familia Atic, pero el job #1 del producto (soltar historial en un input ajeno) falla en texto: parece soportado (`grab`, tooltip) y muere en silencio. Imágenes por OLE van por el camino correcto; el eslabón débil es texto + feedback.

#### What's Working

1. Shell Operate de la familia líquida (emerge/close, pin/X, compact).
2. Conciencia de click-through al arrastrar hacia el SO (`armOverlayForItemDrag`).
3. Imágenes ya usan `clipboardDragPath` + `tauri-plugin-drag` (OLE).

#### Priority Issues

**[P0] Texto no usa OLE / `clipboard_drag_path`**
- Why: Rust ya materializa `.atic-drag-*.txt`; agents lee esas rutas; la lista solo OLE-a imágenes. Texto HTML5 no cruza bien a Notepad/chat/inputs nativos.
- Fix: Unificar texto e imagen en `startDrag` vía `clipboardDragPath`.
- Suggested command: `/impeccable harden`

**[P0] Click vs drag: fallo silencioso del path de texto**
- Why: Tras umbral se cancela el paste; si HTML5 no toma el gesto, `insertTextAtPoint` no ve el SO → nada.
- Fix: OLE siempre al cruzar umbral; feedback si no hay drop; o handle de drag separado del clic.
- Suggested command: `/impeccable harden`

**[P1] `armOverlayForItemDrag` incompleto / asimétrico**
- Why: Solo con agents/snippets vivos; imágenes no lo llaman; contrato in-overlay vs OS no unificado.
- Fix: Política explícita OLE-para-OS + armar overlay solo si hay droppables internos.
- Suggested command: `/impeccable shape`

**[P1] Cero feedback de fallo en el float**
- Why: `ClipboardFloat` no pasa `onError`; errores de drag/paste se pierden.
- Fix: Toast/inline + estado “arrastrando…”.
- Suggested command: `/impeccable clarify`

**[P2] Affordance de drag casi invisible**
- Why: Solo cursor grab + title; la fila parece botón de pegar.
- Fix: Handle de drag o hint en header.
- Suggested command: `/impeccable onboard`

#### Persona Red Flags

**Alex (Power User):** Espera soltar en VS Code/Slack; texto no hace OLE; gesto “casi” funciona. Refuerza que el producto “es pegar”, no “soltar en el input”.

**Riley (Stress Tester):** Solo clipboard → texto a Notepad → vacío. Agents+clipboard: path `.atic-drag-*` que agents ya lee nunca se emite desde la lista. Features marca arrastre como `hecho`.

**Dev multitasker (agents+clipboard):** Click-pegar al composer sí inserta; drag al composer es el eslabón débil justo en el flujo combinado.

#### Minor Observations

- `aria-selected="false"` fijo en filas.
- Float sin `onError` (main `ClipboardTool` sí lo tiene).
- Features/clipboard-historial.md afirma arrastre a otras ventanas; texto no cierra el claim.
- Auto-close post-paste es correcto Operate; pin multi-drop poco enseñado.

#### Questions to Consider

1. Si el job #1 es soltar en un input ajeno, ¿por qué el control primario sigue siendo clic = pegar?
2. ¿Por qué existe `clipboard_drag_path` + `readClipboardDragText` si la lista nunca emite `.atic-drag-*.txt` al arrastrar texto?
3. ¿Qué cuenta como éxito visible: el caret cambió, el archivo se soltó, o solo que el gesto “salió” del float?
