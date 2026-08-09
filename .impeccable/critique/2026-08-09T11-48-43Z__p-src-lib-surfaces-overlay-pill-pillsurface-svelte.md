---
target: animacion apertura y cierre barra de busqueda / pill
total_score: 27
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 2
timestamp: 2026-08-09T11-48-43Z
slug: p-src-lib-surfaces-overlay-pill-pillsurface-svelte
---
Method: dual-agent (A: 523a50b1-a405-4c5b-a3dd-78ee35755d20 · B: 00b5c246-b900-4c8e-87c8-7aae82158ee4)

## Design Health Score

| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Durante `flyTo` previo al morph solo se ve la pill viajar; el anillo aparece después |
| 2 | Match System / Real World | 3 | Summon + radial familiar; overshoot `--morph-ease` se siente más juguete que utilidad |
| 3 | User Control and Freedom | 3 | Esc/clic fuera/`collapseEpoch` bien; no se puede abortar a mitad de `flyTo` |
| 4 | Consistency and Standards | 2 | Rueda (overshoot 175) vs floats (smooth-out 200/175, scale 0.55); launcher no hereda morph |
| 5 | Error Prevention | 3 | Sin preselect; `openingWheel` evita tirones; soltar fuera no arma |
| 6 | Recognition Rather Than Recall | 2 | Caption oculto en compacto; morph enseña forma, no labels |
| 7 | Flexibility and Efficiency | 3 | PTT + teclado + `wheelQuick` 75ms; open serial resta latencia |
| 8 | Aesthetic and Minimalist Design | 3 | Centro fijo + gotas fuertes; `.p-root` salta width/height sin tween |
| 9 | Error Recovery | 3 | Epoch cancela bien; sync por `wait()` en vez de `afterTransition` |
| 10 | Help and Documentation | 2 | Tooltips en reposo; durante open/close casi cero coaching |
| **Total** | | **27/40** | **Acceptable (cercano a Good)** |

## Design Specificity Verdict

**LLM assessment:** Autorizado para Atic, no genérico. Morph líquido disco→anillo con marca fija, pivote `center`, handoff `wheelChromeActive`, tokens `ms(MOTION.*)`, cierre `wheelQuick` 75ms. Lo intercambiable: vuelo `.is-flying` 175ms y dialecto `.float-emerge` de los floats hermanos. La deuda en `Features/pill-shell.md` confirma que el producto quiere más continuidad de la que hoy entrega.

**Deterministic scan:** `detect.mjs --json` sobre `PillSurface.svelte` y `apps/desktop/src/lib/surfaces/overlay/pill` → exit 0, `[]`, 0 findings. El detector no captura seams de motion; un scan limpio no contradice los issues de craft.

**Visual overlays:** No disponibles. Injection omitida: overlay Tauri, sin Playwright/browser MCP, sin mutación DOM fiable pese a Vite en `:1420`.

## Overall Impression

La firma motion (gotas + centro fijo + cierre rápido al elegir tool) es lo más “Atic” del overlay. El mayor freno es que open se siente como dos sistemas en serie (vuelo → snap de caja → morph), y el handoff a launcher/floats cambia de idioma.

## What's Working

1. Contrato geométrico testeable (`pivotFor`, `wheelChromeActive`, `morphsInPlace`) — evita el colapso que “salta” el pivote.
2. Una fuente de verdad de tiempos (`app.css` ↔ `motion.ts` / `ms()`) con `prefersReducedMotion` → 0.
3. Interruptibilidad cierre→reapertura (`collapseEpoch`, `cancelPendingCollapse`) y no-preselect — pensamiento Operate serio.

## Priority Issues

### [P1] La caja no morflea; solo el anillo
- **Why:** `.p-root` aplica width/height al instante; el morph vive en `ParticleWheel`. Se lee pop + bloom, no un material.
- **Fix:** Transicionar tamaño del root (o scale del contenedor) al mismo `--morph-*-dur` / `--morph-ease`.
- **Suggested command:** `/impeccable animate`

### [P1] Open en serie no interrumpible (`flyTo` + morph)
- **Why:** ~350ms de compromiso; Esc no aplica igual durante vuelo; `openingWheel` bloquea reentrada.
- **Fix:** Solapar vuelo y morph; cancel durante vuelo; skip-flight si distancia < umbral.
- **Suggested command:** `/impeccable animate` + `/impeccable optimize`

### [P2] Dos dialectos de motion vs floats / launcher
- **Why:** Overshoot rueda vs smooth-out floats; path launcher = closeMorph → flyToSlot → float-emerge.
- **Fix:** Unificar familia overlay o morph continuo disco→barra launcher.
- **Suggested command:** `/impeccable animate` o `/impeccable polish`

### [P2] `playCloseMorph` sincroniza con `wait()`, no con la transición real
- **Why:** Desfase posible con overshoot/WebView; recorte al encoger.
- **Fix:** Usar `afterTransition` sobre `.pw-blob` / `.pw-nodes`.
- **Suggested command:** `/impeccable harden`

### [P3] `leavingWheel` muerto; foco post-open ausente
- **Why:** Branch de reconcile nunca corre; Sam no aterriza en el toolbar al abrir.
- **Fix:** Eliminar o cablear `leavingWheel`; focus al core/toolbar tras `wheelShown`.
- **Suggested command:** `/impeccable audit` + `/impeccable distill`

## Persona Red Flags

**Alex (Power User):** Open serial 175+175 compite con atajo radial; cierre in-situ reescribe hogar espacial; solo `wheelQuick` 75ms se siente a su velocidad.

**Sam (Accessibility):** Reduced motion bien; sin move-focus al abrir; caption compacto off; overshoot/blur puede marear con motion on.

**Riley (Stress Tester):** Epoch ayuda en spam, pero `wait` fijo + resize puede dejar un frame inconsistente; durante `flyTo`, dismiss/Esc no simétrico; locks (`opening`, `slotBusy`) pueden dejar la pill quieta un beat.

## Minor Observations

- `.p-root.is-quick` acorta close/fade — coherente con `wheelQuick`.
- Auth card usa `.float-emerge` con `--duration-very-slow` (tercer micro-dialecto).
- Docs Features aún apuntan paths viejos (`lib/` vs `surfaces/overlay/pill/`).
- Compact oculta caption: tradeoff consciente, coste de wayfinding.

## Questions to Consider

1. Si el morph es la firma, ¿por qué el hit-box salta y solo las gotas mienten que hubo morph?
2. ¿El vuelo al cursor es significado o impuesto de 175ms antes del peak?
3. Al cerrar, ¿quedarse donde el gesto dejó la pill o deshacer el summon?
4. ¿El launcher merece el mismo material que las gotas, o dos idiomas para siempre?
5. ¿Close debería ser más corto/ease-out (sin bounce) porque Operate odia el overshoot al salir?

## Cognitive Load

3 fallos (chunking, one-thing-at-a-time, minimal choices) → carga moderada.
