---
target: flipboard
total_score: 25
p0_count: 0
p1_count: 4
p2_count: 1
timestamp: 2026-09-08T15-52-37Z
slug: apps-desktop-src-lib-surfaces-windowflip
---
# Critique: Window Flip / FlipBoard

Method: dual-agent (A: ses_f7e50bcacffeucGzP2yjyvoCS2 · B: ses_f7e50a3beffejEhGe4m58olRAw)

## Design Health Score

| # | Heurística | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Tinta actual invisible con el cajón cerrado; no se ve con qué color se dibuja |
| 2 | Match System / Real World | 3 | Metáfora papel/lápiz coherente; "Drawer" no anticipa que tiene colores + clipboard |
| 3 | User Control and Freedom | 2 | Sin undo/redo; Delete destruye un bloque para siempre |
| 4 | Consistency and Standards | 3 | Toolbar con `rb-btn`/`aria-pressed` bien; swatches 20px y asa 14px fuera del sistema |
| 5 | Error Prevention | 2 | Delete de un bloque con texto = un keystroke de la nada, sin confirmar ni deshacer |
| 6 | Recognition Rather Than Recall | 2 | Pan es affordance muerto (cursor `grab`, nunca dispara); ctrl+wheel oculto; color requiere abrir el cajón |
| 7 | Flexibility and Efficiency | 2 | Sin atajos de herramienta, sin nudge de flechas, sin Enter para item nuevo, sin undo |
| 8 | Aesthetic and Minimalist Design | 3 | Limpio; la franja de agarre de 16px permanente agrega chrome a cada objeto |
| 9 | Error Recovery | 2 | Fallos de import de clipboard/imagen silenciosos; nada "pasó" y no hay aviso |
| 10 | Help and Documentation | 3 | Placeholder y footer enseñan bien; el estado vacío enseña la acción equivocada |
| **Total** | | **25/40** | **Acceptable** |

## Anti-Patterns Verdict

**LLM review**: No es plantilla. El giro 3D está autorizado (timings asimétricos, easing partido en 90°, `perspective-origin` centrado en la tarjeta). Detalles reflexivos menores: títulos de sección del cajón en uppercase con tracking (convención house) y el par hairline + sombra de 40px en `.papel`.

**Deterministic scan**: 0 findings. Los rules de contrast/radius/border-shadow del detector solo corren sobre `.html`, no `.svelte`; es un artifact de cobertura, no un "clean". El paso manual encontró el señal real: hit targets < 24px (.asa 18×18, .lapiz 20×20, .agarre 16px, .mas ~13px), move/resize solo pointer (tabindex=-1, .asa sin click), sin focus ring en los dos inputs de texto (outline:none + .atic-root no presente en esta ruta), --rb-faint a 2.8-3.3:1 (bajo AA), y sin pairing border+shadow ni radius off-scale.

## Overall Impression

El flip es lo mejor de Atic: autorado, con estado, no decorativo. La parte floja es la vida editorial del reverso: sin deshacer, pan que no funciona como promete, y un primer flip que confunde. La oportunidad única: después de mostrar un momento espectacular, la suite de notas debería desaparecer en la tarea y hoy pide memoria y contraataca errores.

## What's Working

1. **La coreografía del flip** — easing partido en 90°, perspectiva anclada a la tarjeta, cámara a medio ancho para no crecer en ventanas maximizadas. El movimiento comunica estado y nunca decora.
2. **Escape progresivo por capas** — FlipBoard consume Esc para deseleccionar/salir de herramienta (`stopImmediatePropagation`) antes del handler de cierre; Esc una vez = volver a select, Esc dos = volver al frente. Nunca atrapa en edición.
3. **Flujo de clipboard real** — drag con MIME propio que suelta en el cursor, inserción con clic, paste que respeta textarea/input. Insertar luego selecciona (`poner`), el bloque queda accionable de inmediato.
4. **Autoguardado honesto** — persist debounced + snapshot al cerrar + aviso "Saved" sin desplazar el footer + silencio al fallar guardar en vez de mentir. La decisión correcta como producto.

## Priority Issues

### 1. [P1] No hay undo/reset y el Delete es destructivo
**Why**: `quitar` y Delete/Backspace destruyen para siempre; `cerrarTrazo` commitea strokes sin historial. Un usuario que hace una mancha al lápiz no tiene escapatoria. El pico-emocional negativo está en el primer error.
**Fix**: stack de undo acotado (~50 ops: stroke, crear, mover, resize, borrar, checklist) + Ctrl+Z / Ctrl+Shift+Z; mínimo, confirmar Delete de bloques con contenido.
**Suggested command**: `$impeccable harden`

### 2. [P1] El pan es un affordance muerto que se convierte en trampa
**Why**: `.vista` muestra `cursor: grab` y `empezarPan` exige `target === currentTarget`, pero `.papel` ocupa todo menos 10px de gutter; a zoom > 1, el papel llena la vista y no se puede arrastrar. Quedas varado en el zoom; el único escape es % que además resetea el pan.
**Fix**: permitir pan cuando el drag empieza sobre papel/tinta (no objeto), o pan con espacio/middle-drag + botón "Ajustar" (fit).
**Suggested command**: `$impeccable polish`

### 3. [P1] El estado vacío enseña una acción falsa
**Why**: "Clic para escribir..." con herramienta select activa: el primer clic no hace nada (solo limpia selección). Primer flip = confusión, parece roto.
**Fix**: reescribir para nombrar las herramientas ("Elige Texto o Lápiz..."), hacer el estado interactivo (afordancia "Añadir texto"), o abrir el cajón la primera vez.
**Suggested command**: `$impeccable onboard`

### 4. [P1] No hay camino de teclado para operar objetos
**Why**: `.objeto` sin focus, `.agarre` tabindex=-1, `.asa` es `<button>` pero solo tiene `onpointerdown` (Enter/Space no resize), focus ring de `.atic-root` no existe en esta ruta, checkboxes con aria-label genérico repetido, aria de swatches = hex crudo. Hit targets: `.asa` 18×18, `.lapiz` 20×20, `.agarre` 16px, `.mas` ~13px. Texto con `--rb-faint` a 2.8-3.3:1.
**Why**: El usuario con teclado quita la pill del camino pero no puede ni mover un bloque; los controles finos son imprecisos para todos.
**Fix**: tab stop por objeto (Enter selecciona, flechas mueven, Delete quita), foco visible en inputs, targets ≥24px, accent en --rb-faint a ≥4.5:1, nombres accesibles significativos.
**Suggested command**: `$impeccable harden`

### 5. [P2] El color de tinta es invisible y elegir swatch arma el lápiz
**Why**: con el cajón cerrado nada muestra el color; clicar un swatch además conmuta a draw (efecto colateral silencioso); swatches de 20px.
**Fix**: chip de color en el botón Lápiz (o toolbar), separar "elegir color" de "activar lápiz", targets ≥24px.
**Suggested command**: `$impeccable polish`

## Persona Red Flags

**Alex (power user)**: sin atajos de letra (V/P/T/L); texto/check auto-vuelven a select tras UN bloque mientras draw queda activo — inconsistente; no hay Ctrl+Z, ni mover con flechas; para cambiar de color cada vez debe abrir el cajón.

**Sam (accesibilidad/teclado)**: objetos inalcanzables por teclado; `.asa` solo pointer; inputs sin focus ring en esta ruta (`outline: none`, ring global no aplica); checkbox con aria-label genérica repetida en todo el listado; swatch con aria-label "#e5483f"; `role="application"` sobre `.vista` sin navegación de teclado a cambio; texto faint falla 4.5:1; en compacta las labels de herramientas desaparecen.

**Riley (stress)**: arrastrar bloques no tiene clamp — se cortan en el borde (overflow hidden) pero siguen en los datos; insertar desde cajón/paste usa coordenadas lógicas del papel para que bajo zoom/pan caiga fuera de vista, sin feedback; import de imagen falla en silencio; la lista del checklist crece pero el ul scrollea dentro de 120px.

## Minor Observations

- `.objeto` no tiene hover state; solo selected recibe outline.
- El agarre de 16px es visible permanente en cada objeto; debería aparecer en hover/selección.
- `soltarSeleccion` está exportada y nunca se llama (dead code).
- `.kicker` con `text-transform: lowercase` — "back of chrome.exe" suena a typo.
- `.papel`: hairline 0 1px 0 + sombra 0 18px 40px — el pairing banneado (aunque ambos shadows).
- Flip 400/320ms por encima del registro 150-250ms — justificado por escala; re-check conciente.
- `.estado` del hint desaparece en compacta: se pierde el único recordatorio escrito de atajos.
- Falta control "Ajustar" (fit); el % funciona de reset con título "Zoom 100%".

## Questions to Consider

- ¿Si el bloc fuera una hoja fija del tamaño de la ventana, el flip 3D sigue siendo la mejor revelación — o la metáfora de tarjeta hoy paga complejidad de pan/zoom que un slide-over no tendría?
- ¿Y si las herramientas fueran keyboard-first (V/P/T/L, Q = texto one-shot) y la toolbar fuera solo leyenda? El flujo completo de Alex baja de ~9 clics a 3 y la inconsistencia de auto-revert desaparece.
- ¿Y si "color" fuera un choice por-stroke desde las últimas 3 tintas (estilo resaltador) en vez de un cajón que abres y cierras — el cajón necesita existir para colores o solo para clipboard?
- ¿Y si el papel creciera con el contenido (casi infinito) en vez de ser del viewport — zoom, pan y el bug del bloque cortado se vuelven no-problemas?
- Si el estado vacío abriera el cajón en el primer flip en vez de mostrar texto, ¿la primera sesión se enseña sola?
