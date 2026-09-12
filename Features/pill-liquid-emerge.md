# Emergencia de un float del overlay

**Estado:** `hecho` (acto A: launcher; acto B: clipboard, snippets) · patrón reutilizable

## Resumen

Dos actos conviven en el overlay y **no son el mismo gesto**:

- **Acto A — nacimiento centrado (launcher).** La barra no viaja: **nace en su
  centro** como una gota de 40 px (aparece con fade + `scale(0.82)`, el gesto de los favs)
  y **estira en el sitio** (40 → 324 px, `--ease-liquid`, 200 ms) hasta el stadium. Los hijos
  se desprenden de a uno. El cierre es el espejo: peels se repliegan, la barra vuelve a la
  gota en su centro y recién ahí dismiss + pill home.
- **Acto B — fused grow → separate (paneles: clipboard, snippets).** La
  superficie nace **pegada a la pill** como un solo blob, **crece** (ancho/alto,
  no scale) y después se **separa** hasta cortar el cuello. El cierre es el
  reverse: approach (cuello) → shrink a la semilla → dismiss.

Regla para elegir: si la superficie **debe leerse como algo que sale de la pill**
(panel de trabajo junto al notch), acto B. Si es una **barra centrada que toma el
foco** (Spotlight), acto A: mover formada una barra ya crecida se lee como “se
corrió una ventana”, no como nacimiento.

Implementación de referencia del acto A:
[`LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte);
del acto B: `ClipboardFloat.svelte` / `SnippetsFloat.svelte`.

**Quién lo usa:** launcher (acto A, stadium + peels); clipboard y snippets
(acto B: seed fused → grow w/h → separate). Candidato: agentes.

---

## 1. Intent / feel

El usuario debe leer **origen**, no “apareció un panel”.

| Momento | Qué se siente |
| --- | --- |
| Fused (acto B) | Pill + superficie = **un** blob. El goo une el hueco. |
| Grow (acto B) | La forma **se estira** hacia afuera (p. ej. a la derecha). Como dictado, no un pop de scale. |
| Separate (acto B) | El cuello se estira y **corta**. Quedan dos cuerpos. |
| Birth (acto A) | La gota aparece en el centro (fade + scale) y **estira** a la barra; el chrome entra recién cuando se asienta. |
| Recede (acto A) | La barra **vuelve a la gota** en su centro y se apaga; la pill recién ahí vuelve a casa. |
| Peels (opcional) | Hijos salen **uno tras otro** desde el borde, cada uno con un instante de fusión y luego idle separado. |

Orden fijo, acto A: open **slot (pill) → birth (gota → stadium) → favs → ready**;
close **tuck → recede → dismiss**.

Orden fijo, acto B: open **fuse grow → separate → (children) → ready**;
close **(tuck) → approach → shrink → dismiss**. No invertir dentro de cada
dirección (p. ej. separar antes de crecer, o hide sin reverse).

---

## 2. Liquid rules (REACH y gaps)

Fuente: [`constants.ts`](../apps/desktop/src/lib/liquid/constants.ts).

| Constante | Valor actual | Rol |
| --- | --- | --- |
| `BLEND` | `20` | Perilla de mezcla SDF (`smin`) |
| `REACH` | `sminReach(BLEND)` = **10 px** | Hueco máximo que el cuello todavía cruza |

Regla operativa:

- **gap ≤ REACH** → las formas se funden (un blob / cuello).
- **gap > REACH** → el cuello corta; siluetas independientes.

Gaps del acto B (paneles; el launcher ya no usa semilla fused):

| Gap | px | vs REACH | Por qué |
| --- | --- | --- | --- |
| `SEED_OVERLAP_PX` | **20** | solapa | Nacimiento y shrink: semilla *sobre* el borde de la pill (`placePanelFusedSeed` / paneles). |
| `FUSED_GAP_PX` | **2** | ≪ 10 | Solo **approach** (panel completo cerca para re-armar cuello). Nunca para la semilla. |
| `BIRTH_SEED_PX` (acto A) | **40** | no aplica (la gota no va pegada a la pill) | Gota del nacimiento centrada: cápsula de 40 de ancho al alto de la barra (con 40 de alto = disco de la pill). De ahí estira el stadium. |
| `LAUNCHER_PILL_GAP` (slot idle) | **16** | > 10 | Pill y barra en reposo **no** se pegan ([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts)). En el acto A no hay cuello: la pill se corre al slot y la barra nace centrada. |
| `FAVS_GAP_PX` / `DOT_GAP_PX` | **15** | > 10 | Idle: bolitas sueltas, no óvalo. Al salir, el transform arranca más cerca → un instante bajo REACH → cuello legible. |

Si portas el patrón: elige **un** fused gap (~2 px) y **un** resting gap
claramente > `REACH` (15–16 px está bien con `BLEND = 20`). No uses el gap de
reposo durante el grow.

Detalle del sistema (piel, color único, SDF vs SVG legacy): [liquid.md](liquid.md).

---

## 3. Acto B — phases checklist (paneles)

Tipo en los paneles:

```ts
type RevealPhase = "hidden" | "expand" | "separate" | "ready" | "approach" | "shrink";
```

| Fase | Qué hacer | Criterio de “listo” |
| --- | --- | --- |
| `hidden` | Float no mostrado / cerrado. | — |
| *(previo)* | Pill **vuela al slot** ([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts) + `PillSurface`). | Pill ya en sitio; recién ahí montas el float. |
| `expand` | `placeFusedToPill` (ancho inicial angosto) → animar **width** a tamaño final. Borde de origen **clavado**. | `afterTransition(el, "width", openDur)` |
| `separate` | Armar clase CSS de transición de `left`/`top` **un frame antes** de mover el ancla; luego `applyCenterPlace` (o resting gap). | `afterTransition(el, "left", separateDur)` |
| `ready` | Interacción normal; liquid idle / measured. | — |

**Reduced motion:** saltar a place final + todos los hijos visibles +
`revealPhase = "ready"`. Usar `prefersReducedMotion()` de
[`motion.ts`](../apps/desktop/src/lib/motion.ts).

**Epoch / cancel:** incrementar un `revealEpoch` al cancelar o reabrir; cada
`await` comprueba `epoch !== revealEpoch` y aborta. Evita grow fantasma tras Esc.

Checklist mínimo al abrir (acto B):

1. [ ] Pill en slot (no abrir float en el hogar de la pill si el diseño pide vuelo).
2. [ ] Place fused (`gap` ~2 px, ancho seed).
3. [ ] Fase `expand` + transition de width (borde izquierdo fijo si crece a la der.).
4. [ ] Fase `separate` + transition de left/top (gap idle > REACH).
5. [ ] (Opcional) peels secuenciales.
6. [ ] Hit-rects + publish de skin acordes a la fase.
7. [ ] Atajo reduced-motion.

---

## 3a. Acto A — launcher (nacimiento centrado)

Tipo en el launcher:

```ts
type RevealPhase = "hidden" | "birth" | "favs" | "ready" | "tuck" | "recede";
```

| Fase | Qué hacer | Criterio de “listo” |
| --- | --- | --- |
| `hidden` | Float no montado / cerrado. | — |
| *(previo)* | Pill **vuela al slot**; la barra se coloca **en su centro** como **gota** (`centerPlace` → `placeAtCenter(seed)`: cápsula `BIRTH_SEED_PX` centrada en el rect de reposo, mismo alto). | Gota montada y centrada; recién ahí el reveal. |
| `birth` | `animateEl` (fade + `scale(0.82)` → 1, `BIRTH_DROP_DUR_MS`) → hold `BIRTH_HOLD_MS` → `stretchToRest(openDur)`: **width + left** de la gota al stadium. Favs/recientes hidratan **en paralelo**. | `await` de la animación (`WAAPI finished`) |
| `favs` (opcional) | `favRevealCount = N` en un solo cambio; CSS escalona cada hijo con `--lf-i` y `--launcher-fav-stagger`. | Esperar el último hijo (`N × favStaggerDur`) |
| `ready` | Interacción normal; liquid idle / measured. | — |

Claves del acto A:

- **Nada de viaje:** el rect de reposo se calcula antes de nacer (`restRect`); la gota vive
  en su centro y **estira** hasta el stadium. Un re-place a mitad del morph es un salto:
  `applyCenterPlace` solo toca el rect en `hidden`/`ready`.
- **El estirón es layout real** (`width` + `left`, 40 → 324 px), no un scale: por eso se **ve
  crecer**. Lo anima `animateEl` con **WAAPI** (keyframes con medidas explícitas + `finished`),
  así no depende de un frame previo pintado ni del armado de una transición CSS.
- **Curva y duración:** `--ease-liquid` (arranque lento: *la gota se resiste antes de
  estirarse*) y `BIRTH_DUR_MS` = **200 ms** — más largo que el grow de panel porque el
  recorrido es 8× el ancho original.
- **La barra se pinta sola** mientras crece (el float recupera su fondo `--skin`): durante
  `birth`/`recede` la superficie **no publica silueta líquida** — no hay cuello con la pill y
  duplicaría el blob. El chrome (`.lf-head`) entra cuando la barra se asienta.
- **Reduced motion:** `ready` + `applyCenterPlace` (rect de reposo directo) + hijos visibles.

---

## 3b. Acto B — close (reverse)

El cierre es el **espejo** del open. No `bubble.hide()` de golpe: el float
vuelve a fundirse con la pill y se encoge a la semilla; recién ahí se dismiss-ea
y la pill puede volver a casa (si voló a un slot).

Tipo (paneles):

```ts
type RevealPhase =
  | "hidden" | "expand" | "separate" | "ready"
  | "tuck" | "approach" | "shrink";
```

| Fase close | Qué hacer | Criterio de “listo” |
| --- | --- | --- |
| *(cancel)* | `revealEpoch++` para abortar open en vuelo. No poner `hidden` mientras `shown` (re-dispararía open). | — |
| `tuck` (opc.) | Replegar peels (`favRevealCount = 0`) o fusionarlos rápido en la barra. | `afterTransition(dot, "transform", favStaggerDur)` o wait corto |
| `approach` | Mover float hacia la pill hasta gap fused (~2 px) **a tamaño lleno** — reverse de separate. Reusa `.is-separating`. | `afterTransition(el, "left"\|"top", separateDur)` |
| `shrink` | Encoger w (launcher) o w/h (panel) a la semilla fused, borde hacia la pill clavado — reverse de expand. Reusa `.is-expanding`. | `afterTransition(el, "width", openDur)` |
| *dismiss* | `bubble.hide()` + hide IPC; limpiar skin. Pill: `PillSurface` espera a que desaparezca el hit-rect del float y recién ahí `flyTo(home)`. | — |

Orden fijo: **(tuck) → approach → shrink → dismiss → (pill home)**.

**Reduced motion:** saltar morph; dismiss inmediato.

**CSS:** las mismas clases de open. `expanding = expand \| shrink`,
`separating = separate \| approach`. Durante motion → `publishFollowSkin`.

**Helpers:** [`floatReveal.ts`](../apps/desktop/src/lib/surfaces/overlay/floatReveal.ts)
(`separateAxisProp` para acto B; el acto A anima el rect con WAAPI, ver `animateEl`)

### Acto A — close (espejo en el centro)

El launcher **no vuelve a la pill**: se repliega donde nació.

| Fase close | Qué hacer | Criterio de “listo” |
| --- | --- | --- |
| *(cancel)* | `revealEpoch++` para abortar open en vuelo. No poner `hidden` mientras `shown`. | — |
| `tuck` (opc.) | `favRevealCount = 0`; CSS invierte el delay (`--lf-fav-last-index - --lf-i`). | `afterTransition(firstDot, "transform", favStaggerDur × N)` |
| `recede` | `shrinkToSeed(closeDur)`: WAAPI del rect (stadium → gota) + fade + `scale(0.82)` en una sola animación, **en el mismo centro**. | `await` de la animación |
| *dismiss* | `bubble.hide()` + hide IPC; el próximo open recalcula gota y reposo (`centerPlace`). Pill: espera el hit-rect gone y vuelve a casa. | — |

Orden fijo: **(tuck) → recede → dismiss → (pill home)**. El panel de resultados se
colapsa a stadium **antes** del repliegue: no se encoge un panel entero.

---

## 3c. Timings medidos (fuente de verdad)

Valores actuales en `:root` ([`app.css`](../apps/desktop/src/app.css)) +
fallbacks ([`motion.ts`](../apps/desktop/src/lib/motion.ts)). Ease común del
grow/separate: `--ease-smooth-out` (`cubic-bezier(0.22, 1, 0.36, 1)`).

### Tokens compartidos

| Token | ms | Uso |
| --- | ---: | --- |
| `--flight-dur` | **150** | Pill `flyTo` (slot / home / cursor) |
| `--launcher-bar-open-dur` | **100** (paneles) | Grow de panel (acto B). El estirón del launcher es JS: `BIRTH_DUR_MS` = **200** (WAAPI) |
| `--launcher-separate-dur` | **90** | Separate / approach (left/top) |
| `--launcher-fav-stagger` | **90** | Cada fav peel / tuck (delay CSS por dot) |
| `SEED_HOLD_MS` (const JS, paneles) | **60** | Hold en disco fused antes de estirar (acto B) |
| `BIRTH_DROP_DUR_MS` (const JS, launcher) | **120** | Aparición de la gota (fade + scale) |
| `BIRTH_HOLD_MS` (const JS, launcher) | **60** | Hold en la gota centrada antes de estirar (acto A) |
| `--lf-chrome-dur` (local) | **120** | Entrada del chrome (`.lf-head`) al asentarse la barra |
| `--float-open-dur` | **150** | `.float-emerge` open (agentes; = medium) |
| `--float-close-dur` | **150** | `.float-emerge` close / hide bubble (= morph-close) |
| `--morph-open-dur` | **150** | Morph rueda / gotas |
| `--morph-close-dur` | **150** | Morph close |
| `--panel-dur` | **125** | Arrive skin dictado (`p-skin-arrive`) |
| `--duration-quick` | **75** | Micro UI |

### Launcher (acto A: nacimiento centrado + peels)

**Open** (después del vuelo de pill si aplica):

| Etapa | ms | Acumulado típico* |
| --- | ---: | ---: |
| Pill → slot (`flight`) | 150 | 150 |
| Gota (fade + scale) | 120 | 270 |
| Hold en la gota | 60 | 330 |
| Estirón (gota → stadium, `--ease-liquid`) | 200 | 530 |
| Favs (CSS delay ×N) | 90×N | 530 + 90N |

\*Con 2 favs ≈ **0.7 s** total desde el atajo (incl. vuelo). El estirón se puede
acortar por lab (`openDur`) o bajando `BIRTH_DUR_MS`.

**Close** (espejo, sin viaje a la pill):

| Etapa | ms |
| --- | ---: |
| Tuck favs | 90×N (si había peels; CSS delay reverse) |
| Recede (stadium → gota → punto) | 120 (`closeDur`, WAAPI) |
| `bubble.hide()` → hit-rect gone | ~100 (`MOTION.floatClose`) |
| Pill → home | 150 |

### Clipboard / Snippets (panel fused grow)

Mismos tokens de grow/separate que el launcher (reusan `--launcher-*`).

**Open:** hold 60 + expand 100 + separate 90 = **250 ms** (+ flight 150 si la pill vuela).

**Close:** approach 90 + shrink 100 = **190 ms** (+ flight home 150).

### Agentes / auth (aún `.float-emerge`, no fused grow)

| Superficie | Open | Close |
| --- | ---: | ---: |
| `AgentsFloat` | 150 (`float-open`) | 150 (`float-close`) |
| Auth card (pill) | 250 (`--duration-very-slow` override) | 150 |

### Pill (otros)

| Acto | ms |
| --- | ---: |
| Vuelo slot / home / cursor | 150 |
| Morph open rueda | 150 |
| Morph close rueda | 150 |
| Morph quick (elegir tool) | 75 |

Si cambias un timing: toca el token en `app.css`, el fallback en `MOTION_FALLBACK`,
y esta tabla. Lab del launcher puede override solo `openDur` en DEV.

Checklist mínimo al cerrar:

1. [ ] Cancelar open (epoch); no `hide` sin reverse.
2. [ ] (Opcional) tuck peels.
3. [ ] Approach fused full + transition left/top.
4. [ ] Shrink a seed + transition width(/height).
5. [ ] Dismiss; pill home **después** de que el float suelte el hit-rect.
6. [ ] Reduced-motion shortcut.
7. [ ] Ignorar eco IPC de `hideLauncher` / `hide*Window` (flag `ignoreIpcDismiss`).

---

## 4. Geometry helpers

### Acto A — place centrado (nacimiento)

La barra se coloca **en su centro** antes de crecer: el **rect de reposo** (centro del
work area, `LAUNCHER_BAR_W` = 324 × `COMPACT_H` = 40) se calcula primero y queda en
`restRect`; lo que se monta es la **gota** (`BIRTH_SEED_PX` = 40 de ancho, mismo alto,
centrada ahí) y el estirón lo anima `animateEl` (WAAPI + `--ease-liquid`). El ancla para
elegir monitor es el nacimiento (`toolBirth`) o el centro de la pill; si no hay ninguno, el
centro del ancla que mandó Rust.

```text
rest  = resolveSlot("center", workAreas, { w, h }, pillCenter)
gota  = cápsula BIRTH_SEED_PX (mismo alto) centrada en rest
timeline: gota (fade+scale) → hold → estirón WAAPI → stadium
```

- `centerPlace` → **sync**: el reveal arranca en el primer frame, sin esperar IPC.
- `applyCenterPlace` → refresca work areas y re-coloca, pero **solo** en `hidden` o
  `ready`. A mitad del reveal un rect distinto es un salto: se descarta el re-place
  (las áreas ya quedaron frescas para el próximo open).

Referencia: `centerPlace` / `applyCenterPlace` en `LauncherFloat.svelte`.

### Acto B — place fused (grow-right, paneles)

Idea: la semilla es un **disco** (`PANEL_GROW_SEED` = 40 ≈ alto de la pill) que
**solapa** el borde de la pill (`SEED_OVERLAP_PX ≈ 20`). Si nace con gap positivo
al lado, se lee como elemento externo aunque el goo una.

```text
x = pill.x + pill.w - SEED_OVERLAP_PX
y = pill.y + (pill.h - h) / 2
w = PANEL_GROW_SEED   // disco; luego → ancho final del panel
```

Hold breve en disco (~60 ms) y chrome (icono/texto) oculto durante
`.is-expanding`. Al crecer solo `w` con `x` fijo, el blob se estira a la
derecha — misma lectura que el expand de dictado.

API: `bubble.place({ …, w, h, x, y, side, offset })` vía
[`bubble.svelte.ts`](../apps/desktop/src/lib/surfaces/overlay/bubble.svelte.ts).

Referencia: `placeFusedToPill` en `ClipboardFloat.svelte` / `SnippetsFloat.svelte`.

`FUSED_GAP_PX` (~2) se usa en **approach/close** para re-fundir el cuello, no
en el birth.

### Place after separate (idle, paneles)

Los paneles aterrizan centrados en el work area (`resolveSlot("center", …)`), con
`PANEL_RESTING_GAP_PX` implícito en el slot de la pill.

Los hijos **fuera** del ancho del float (CSS `position: absolute` a la derecha
del stadium). No desplazan el centro de la barra ni entran al flex del header.

Otros floats (clipboard / snippets) usan
[`placePanelResting`](../apps/desktop/src/lib/surfaces/overlay/floatPlace.ts)
(+ seed fused / grow) para el acto de apertura; agentes u otros aún pueden
quedarse en `placeBesidePill` + `.float-emerge` hasta portarlos.

---

## 5. Skin publishing

Helpers en
[`floatEmergeSkin.ts`](../apps/desktop/src/lib/surfaces/overlay/floatEmergeSkin.ts):

| Helper | Cuándo |
| --- | --- |
| `publishFollowSkin` | Geometría en movimiento continuo: **tuck** de peels, **expand/separate/approach/shrink** (acto B), **drag**. rAF sin idle-stop (con tope de seguridad). |
| `publishEmergeSkin` | Morph `.float-emerge` o un solo rect que se quieta; idle-stop + tope de frames. |
| `publishMeasuredSkin` / `publishCompactPills` | Varias formas medidas del DOM (barra + dots `.is-out`). Republica solo si cambia la clave; idle-stop. |

Reglas del launcher (copiar el criterio, no el id):

1. **Durante el morph** (acto B: expand/separate/approach/shrink; acto A: **tuck** de peels) → `publishFollowSkin("launcher", el, CORNER)`.
   En el acto A, `birth`/`recede` **no publican nada**: la barra se pinta sola mientras crece.
2. **Con favs ya revelados** (compacto) → medir head + `.lf-dot.is-out` y publicar varias shapes.
3. **Panel de resultados / búsqueda** → **no** remeshear SDF en cada tecla (`liquid.publish(id, [])` o chrome opaco). El thrash de height + remesh mataba la búsqueda.
4. **Drag** → `publishFollowSkin`; no depender de leer `bubble.anchor` cada frame dentro del effect (reiniciaría el tracker).

La piel va **aparte del contenido** (capa SDF filtrada vs UI). Mismo `--skin`.
Ver [liquid.md](liquid.md).

---

## 6. Motion tokens

Declarados en [`app.css`](../apps/desktop/src/app.css), mapeados en
[`motion.ts`](../apps/desktop/src/lib/motion.ts) (`MOTION` + `MOTION_FALLBACK`),
y re-inyectados en el float si hace falta override (lab / `style:`).

| Token CSS | Clave `MOTION` | Fallback / típico |
| --- | --- | --- |
| `--launcher-bar-open-dur` | `launcherBar` | **100 ms** |
| `--launcher-separate-dur` | `launcherSeparate` | **90 ms** |
| `--launcher-fav-stagger` | `launcherFavStagger` | **90 ms** |

Easing de referencia: `--ease-smooth-out` (`cubic-bezier(0.22, 1, 0.36, 1)`).

En JS: `ms(MOTION.launcherBar)` etc., y esperar con
`afterTransition(el, "width" | "left" | "transform", dur)`.

Al portar a otra tool: puedes reutilizar estos tokens o añadir
`--clipboard-bar-open-dur` etc. en el mismo estilo; no hardcodear ms sueltos en
CSS y JS sin alinearlos.

---

## 7. CSS contracts

Clases de fase — **acto A (launcher)**:

| Clase | Qué hace |
| --- | --- |
| `.is-revealing` | Morph en curso (birth/recede): `overflow: hidden` + chrome apagado; el rect, el fade y el scale los mueve `animateEl` (WAAPI). |
| *(JS, sin clase)* | El rect viaja por `bubble.place` + `animateEl`: gota (`seedRect`) → reposo (`stretchToRest`) → gota (`shrinkToSeed`, con `fill: forwards` hasta el dismiss). La curva sale de `--ease-liquid`. |
| `.is-favs-stagger` / `.is-tucking` | Escalona los peels (`--lf-i`; en tuck invierte con `--lf-fav-last-index`). |
| hijos `.is-out` | `transform` + `opacity` con `--launcher-fav-stagger`. |

Clases de fase — **acto B (paneles)**:

| Clase | Transiciones |
| --- | --- |
| `.is-expanding` | `width` (+ `height` si aplica) con `--launcher-bar-open-dur` |
| `.is-separating` | `left` / `top` con `--launcher-separate-dur` |

La exclusión del fondo opaco global vive en
[`OverlaySurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/OverlaySurface.svelte)
(`:not(.is-joined, .is-expanding, .is-separating, .is-settling)`). El acto A **no** se
excluye: mientras nace, la barra conserva su fondo `--skin` — eso es justamente lo que hace
visible el estirón. Si agregás una clase de morph a un panel, sumala ahí o se pinta como
caja opaca durante la animación.

Contratos importantes:

- El root usa `left` / `top` / `width` / `height` desde variables del bubble
  (`--x`, `--y`, `--w`, `--h`); el acto A mueve esas variables para nacer/replegarse.
- Acto A: el rect lo mueve `animateEl` (WAAPI) con medidas explícitas en los keyframes, así
  que no hace falta un frame previo pintado; `await` de `finished` es el criterio de “listo”.
- Acto B: **Armar** `.is-separating` **antes** de cambiar el ancla (tick + 1–2 frames);
  si no, `left` salta sin transición.
- Hijos: estado inicial cerca de la barra (`translateX` negativo + scale ~0.82,
  opacity 0); `.is-out` → `transform: none; opacity: 1`.
- **No** animar `height` en cada tecla de búsqueda (comentario en `.lf`: saltar
  a alto expandido para evitar thrash de layout + hit-rects).
- Compacto: overflow visible para que los peels vivan fuera del stadium.
- Hit-rect propio para peels (`surfaces.add("launcher-favs", favsEl)`) cuando
  `favRevealCount > 0`.

---

## 8. Recipe: portar el patrón

### 8a. Acto A — barra centrada que nace de la nada (launcher)

Objetivo: una barra o panel **centrado** que no debe leerse como ventana moviéndose.

1. **Slot de pill** — `DEFAULT_SLOTS` + `resolveSlot`: la pill se corre y deja libre el centro.
2. **Place centrado** — `centerPlace` (sync, con las work areas que ya hay) + refine
   idempotente `applyCenterPlace`. **No** colocar en el borde de la pill.
3. **Gota** — `placeAtCenter(seed)`: cápsula `BIRTH_SEED_PX` (mismo alto) centrada en el
   rect de reposo, guardado en `restRect`; `animateEl` la hace aparecer (fade + `scale(0.82)`).
4. **Estirón** — hold `BIRTH_HOLD_MS` → `stretchToRest(openDur)`: WAAPI de `width` + `left`
   (gota → reposo) con `--ease-liquid`; `await` de `finished`.
5. **Chrome** — el header en `opacity 0` durante el reveal, con la misma duración.
6. **Hijos** — un solo cambio de estado + delay CSS por hijo (`--lf-i`); en tuck, inverso.
7. **Cierre** — `favRevealCount = 0` → `shrinkToSeed(closeDur)` (WAAPI: rect + fade +
   scale, `fill: forwards`) → `bubble.hide()`. Reduced motion: `ready` + rect de reposo
   directo.

Pieza clave: la barra **se pinta a sí misma** durante el morph (el acto A no entra en la
exclusión de fondo opaco de `OverlaySurface.svelte`, y no publica silueta líquida mientras
nace o se repliega).

### 8b. Acto B — panel que sale de la pill (clipboard, snippets)

Objetivo: clipboard, snippets, agentes u otra tool con la lectura
“sale de la pill”.

1. **Slot de pill** — Entrada en `DEFAULT_SLOTS` / `resolveSlot` si la pill debe
   volar antes de abrir ([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts)).
2. **Bubble** — Misma instancia `Bubble` + `bubble.place` / `alive` / `shown`.
3. **Constantes** — `SEED_OVERLAP_PX` (~20), `PANEL_GROW_SEED` / `GROW_START_W` (= alto pill ~40),
   `FUSED_GAP_PX` (~2) **solo** para approach, resting gap > `REACH`.
4. **`placePanelFusedSeed` / `placeFusedToPill`** — Semilla **solapada** (gap negativo), no al lado.
5. **Grow** — `expandPanelFromSeed` (borde clavado). **No** `placePanelFusedFull` en el expand.
6. **`applyRestingPlace`** — Solo en `separate` / `ready`. Nunca en re-anchor durante birth.
7. **Fases** — open `hidden → expand → separate → (children) → ready` y close
   `(tuck) → approach → shrink → dismiss` + epoch + reduced-motion + **seed hold ~60 ms**.
8. **CSS** — Clases de fase; chrome (`head`/`body`/icono/input) **opacity 0** en expand/shrink.
9. **Liquid** — `publishFollowSkin` en expand/separate/approach/shrink.
10. **Hit-rects** — `surfaces.add` del root; peels fuera del bbox si aplica.
11. **Tokens** — `app.css` + `MOTION` / fallbacks.
12. **Cierre** — reverse completo; pill home tras hit-rect gone.

Referencia completa (acto B): `ClipboardFloat.svelte` / `SnippetsFloat.svelte`
(`runOpenReveal` / `runCloseReveal` + effects de skin). Acto A:
`LauncherFloat.svelte`. Producto launcher:
[launcher-spotlight.md](launcher-spotlight.md).

---

## 9. Anti-patterns

| Evitar | Por qué |
| --- | --- |
| Abrir con **`.float-emerge` scale + travel** como acto principal de un panel | Se lee “pop in” / “se corrió una ventana”, no “creció desde la pill”. Los paneles usan fused grow. El launcher (acto A) sí escala, pero **en el sitio y sin travel**: eso se lee como materializarse. |
| **Hide sin reverse** (`bubble.hide()` / Esc al instante) | Se lee “desapareció”; rompe la simetría con el open. Acto B: approach → shrink → dismiss. Acto A: tuck → recede → dismiss. |
| Meter favs/peels **dentro del flex del header** expandido | Desplazan el stadium, rompen el centro y el grow. Van absolute fuera del bar. |
| Esperar a **cargar hijos antes de colocar** la barra | La barra debe nacer fused al instante; los peels vienen después (`favs`). |
| Animar **height en cada tecla** de búsqueda | Thrash de layout, hit-rects y SDF. |
| Remeshear SDF en panel de resultados | Mismo thrash; chrome opaco basta. |
| Separar con gap **≤ REACH** en idle | Nunca corta el cuello; parecen pegados para siempre. |
| **Semilla con gap ≥ 0** (al lado de la pill) | Aunque `FUSED_GAP_PX = 2` una el goo, se ve un **segundo disco externo** desde el primer frame. Open y close deben **solapar** (`SEED_OVERLAP_PX`, gap negativo). |
| Grow con **`placePanelFusedFull`** desde la semilla | Reclava a gap+2 y salta el overlap → “apareció un panel”. Usa `expandPanelFromSeed` (borde clavado). |
| Animar **`width`/`left`** en el acto A | La barra centrada no viaja: nace y crece con `transform` en su sitio; mover la caja formada es justo lo que se quería evitar. |
| Chrome (título, lista, icono, input) **visible en expand/shrink** | Se lee control UI truncado, no blob. Ocultalo hasta separate / tras shrink. |
| Hold **0** en la semilla (acto B) | Si estiras en el primer frame, no se registra el nacimiento: el browser no tiene estado previo que interpolar. ~60 ms basta. En el acto A el hold es explícito (`BIRTH_HOLD_MS`) sobre una gota que ya se ve. |
| Re-anchor a **resting** mientras `revealPhase !== ready` | Un segundo IPC/workAreas hace snap a elemento separado a mitad del morph. |
| Fused gap **≥ REACH** al nacer | No hay blob único; el grow no se lee como un cuerpo. |
| Cambiar `left` en separate/approach **sin** clase de transition armada | Snap visible. |
| Volar la pill a **home** mientras el float aún hace reverse | El cuello se estira hacia un hogar lejano; esperar hit-rect gone. |
| Publicar solo el ancla lógico durante morph visual | Blob a tamaño lleno mientras el DOM anima otra cosa — por eso existen `publishFollowSkin` / emerge tracker. |

### Checklist de sensación — acto A (launcher)

1. Primer frame visible: el centro está **vacío** (el frame replegado es invisible pero ya se pintó).
2. La gota del líquido se materializa en el centro y **crece** hasta el stadium.
3. El chrome (icono + input) entra cuando la barra se asienta; recién ahí salen los peels.
4. Close: peels se repliegan (delay inverso) → la barra vuelve a la gota → dismiss.
5. Si ves la barra **viajando ya formada**, el acto está mal aplicado (eso es el acto B).

### Checklist de sensación — acto B (paneles)

1. Primer frame visible: **un** blob con la pill (disco solapado, sin chrome).
2. Luego el cuerpo **se estira** hacia afuera con cuello líquido (como el morph de la GIF de referencia).
3. Separate corta el cuello; recién ahí se ve el chrome completo.
4. Close: chrome se apaga → se acerca (cuello) → se encoge **solapado** en la pill → dismiss.
5. Si en open o close ves un rectángulo/stadium ya separado desde el inicio o el final, el patrón está mal aplicado.

---

## Código (mapa)

| Archivo | Rol |
| --- | --- |
| [`LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte) | Acto A: birth centrado, peels, liquid, close espejo |
| [`floatEmergeSkin.ts`](../apps/desktop/src/lib/surfaces/overlay/floatEmergeSkin.ts) | `publishFollowSkin` / emerge / measured |
| [`constants.ts`](../apps/desktop/src/lib/liquid/constants.ts) | `BLEND`, `REACH` |
| [`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts) | `LAUNCHER_BAR_W` (324), `LAUNCHER_PILL_GAP`, slots |
| [`motion.ts`](../apps/desktop/src/lib/motion.ts) + [`app.css`](../apps/desktop/src/app.css) | Tokens launcher **100 / 90 / 90 ms** |
| [`floatPlace.ts`](../apps/desktop/src/lib/surfaces/overlay/floatPlace.ts) | Place resting + seed fused / grow (paneles) |
| [`floatReveal.ts`](../apps/desktop/src/lib/surfaces/overlay/floatReveal.ts) | `separateAxisProp` (acto B) |
| [`ClipboardFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/clipboard/ClipboardFloat.svelte) / [`SnippetsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/snippets/SnippetsFloat.svelte) | Adopción panel: fused grow → separate + reverse close |
| [`PillSurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte) | Vuelo al slot; return-home tras reverse; expand de dictado |

## Relacionado

- [liquid.md](liquid.md) — reglas transversales del goo
- [launcher-spotlight.md](launcher-spotlight.md) — feature que implementa este patrón
- [pill-shell.md](pill-shell.md) — pill, slots, morph rueda
- [clipboard-historial.md](clipboard-historial.md) / [snippets.md](snippets.md) — adoptaron fused grow
- [agentes.md](agentes.md) — float candidato a portar
