# Emergencia desde la pill (fused grow → separate)

**Estado:** `hecho` (referencia: launcher; adoptado: clipboard, snippets) · patrón reutilizable

## Resumen

Cómo nace una superficie del overlay **desde la pill** con líquido legible:
primero un solo blob que **crece** (ancho, no scale), después se **separa**
hasta cortar el cuello, y opcionalmente hijos (favoritos, chips, etc.) se
**desprenden de a uno**. El **cierre es el reverse**: peels se repliegan, el
float se acerca (cuello), se encoge a la semilla y recién ahí dismiss + pill
home.

Implementación de referencia:
[`LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte).

Analogía de producto: expandir la barra al dictar (crecer width con borde
clavado) + Spotlight (barra stadium + peels laterales). No es el morph
`.float-emerge` (scale + travel) que siguen usando agentes (y auth) hoy.

**Quién lo usa:** launcher (referencia stadium + peels); clipboard y snippets
(paneles: seed fused → grow w/h → separate). Candidato: agentes.

---

## 1. Intent / feel

El usuario debe leer **origen**, no “apareció un panel”.

| Momento | Qué se siente |
| --- | --- |
| Fused | Pill + superficie = **un** blob. El goo une el hueco. |
| Grow | La forma **se estira** hacia afuera (p. ej. a la derecha). Como dictado, no un pop de scale. |
| Separate | El cuello se estira y **corta**. Quedan dos cuerpos. |
| Peels (opcional) | Hijos salen **uno tras otro** desde el borde, cada uno con un instante de fusión y luego idle separado. |

Orden fijo open: **fuse grow → separate → (children) → ready**.
Close: **(tuck) → approach → shrink → dismiss**. No invertir dentro de cada
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

Gaps del launcher (referencia):

| Gap | px | vs REACH | Por qué |
| --- | --- | --- | --- |
| `SEED_OVERLAP_PX` | **20** | solapa | Nacimiento y shrink: semilla *sobre* el borde de la pill (`placePanelFusedSeed` / launcher). |
| `FUSED_GAP_PX` | **2** | ≪ 10 | Solo **approach** (panel completo cerca para re-armar cuello). Nunca para la semilla. |
| `LAUNCHER_PILL_GAP` (slot idle) | **16** | > 10 | Pill y barra en reposo **no** se pegan ([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts)). |
| `FAVS_GAP_PX` / `DOT_GAP_PX` | **15** | > 10 | Idle: bolitas sueltas, no óvalo. Al salir, el transform arranca más cerca → un instante bajo REACH → cuello legible. |

Si portás el patrón: elegí **un** fused gap (~2 px) y **un** resting gap
claramente > `REACH` (15–16 px está bien con `BLEND = 20`). No uses el gap de
reposo durante el grow.

Detalle del sistema (piel, color único, SDF vs SVG legacy): [liquid.md](liquid.md).

---

## 3. Phases checklist

Tipo en el launcher:

```ts
type RevealPhase = "hidden" | "expand" | "separate" | "favs" | "ready";
```

| Fase | Qué hacer | Criterio de “listo” |
| --- | --- | --- |
| `hidden` | Float no mostrado / cerrado. | — |
| *(previo)* | Pill **vuela al slot** ([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts) + `PillSurface`). | Pill ya en sitio; recién ahí montás el float. |
| `expand` | `placeFusedToPill` (ancho inicial angosto) → animar **width** a tamaño final. Borde de origen **clavado**. | `afterTransition(el, "width", openDur)` |
| `separate` | Armar clase CSS de transición de `left`/`top` **un frame antes** de mover el ancla; luego `applyCenterPlace` (o resting gap). | `afterTransition(el, "left", separateDur)` |
| `favs` (opcional) | `favRevealCount = 1…N` secuencial; cada hijo `.is-out`. | Por hijo: `afterTransition(dot, "transform", favStaggerDur)` |
| `ready` | Interacción normal; liquid idle / measured. | — |

**Reduced motion:** saltar a place final + todos los hijos visibles +
`revealPhase = "ready"`. Usar `prefersReducedMotion()` de
[`motion.ts`](../apps/desktop/src/lib/motion.ts).

**Epoch / cancel:** incrementar un `revealEpoch` al cancelar o reabrir; cada
`await` comprueba `epoch !== revealEpoch` y aborta. Evita grow fantasma tras Esc.

Checklist mínimo al abrir:

1. [ ] Pill en slot (no abrir float en el hogar de la pill si el diseño pide vuelo).
2. [ ] Place fused (`gap` ~2 px, ancho seed).
3. [ ] Fase `expand` + transition de width (borde izquierdo fijo si crece a la der.).
4. [ ] Fase `separate` + transition de left/top (gap idle > REACH).
5. [ ] (Opcional) peels secuenciales.
6. [ ] Hit-rects + publish de skin acordes a la fase.
7. [ ] Atajo reduced-motion.

---

## 3b. Close (reverse)

El cierre es el **espejo** del open. No `bubble.hide()` de golpe: el float
vuelve a fundirse con la pill y se encoge a la semilla; recién ahí se dismiss-ea
y la pill puede volver a casa (si voló a un slot).

Tipo (launcher; paneles omiten `tuck` / `favs`):

```ts
type RevealPhase =
  | "hidden" | "expand" | "separate" | "favs" | "ready"
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

---

## 3c. Timings medidos (fuente de verdad)

Valores actuales en `:root` ([`app.css`](../apps/desktop/src/app.css)) +
fallbacks ([`motion.ts`](../apps/desktop/src/lib/motion.ts)). Ease común del
grow/separate: `--ease-smooth-out` (`cubic-bezier(0.22, 1, 0.36, 1)`).

### Tokens compartidos

| Token | ms | Uso |
| --- | ---: | --- |
| `--flight-dur` | **150** | Pill `flyTo` (slot / home / cursor) |
| `--launcher-bar-open-dur` | **150** | Grow / shrink (width o w+h) |
| `--launcher-separate-dur` | **150** | Separate / approach (left/top) |
| `--launcher-fav-stagger` | **150** | Cada fav peel / tuck |
| `SEED_HOLD_MS` (const JS) | **150** | Hold en disco fused antes de estirar |
| `--float-open-dur` | **150** | `.float-emerge` open (agentes; = medium) |
| `--float-close-dur` | **150** | `.float-emerge` close / hide bubble (= morph-close) |
| `--morph-open-dur` | **150** | Morph rueda / gotas |
| `--morph-close-dur` | **150** | Morph close |
| `--panel-dur` | **125** | Arrive skin dictado (`p-skin-arrive`) |
| `--duration-quick` | **75** | Micro UI |

### Launcher (fused grow + peels)

**Open** (después del vuelo de pill si aplica):

| Etapa | ms | Acumulado típico* |
| --- | ---: | ---: |
| Pill → slot (`flight`) | 150 | 150 |
| Seed hold | 150 | 300 |
| Expand (barra) | 150 | 450 |
| Separate | 150 | 600 |
| Favs (×N) | 150×N | 600 + 150N |

\*Con 2 favs ≈ **0.9 s** total desde el atajo (incl. vuelo).

**Close:**

| Etapa | ms |
| --- | ---: |
| Tuck favs | 150 (si había peels) |
| Approach | 150 |
| Shrink | 150 |
| Pill → home | 150 |

### Clipboard / Snippets (panel fused grow)

Mismos tokens de grow/separate que el launcher (reusan `--launcher-*`).

**Open:** hold 150 + expand 150 + separate 150 = **450 ms** (+ flight 150 si la pill vuela).

**Close:** approach 150 + shrink 150 = **300 ms** (+ flight home 150).

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

Si cambiás un timing: tocá el token en `app.css`, el fallback en `MOTION_FALLBACK`,
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

### Place fused (grow-right)

Idea: la semilla es un **disco** (`GROW_START_W = 40` = alto de la pill) que
**solapa** el borde derecho de la pill (`SEED_OVERLAP_PX ≈ 20`). Si nace con
gap positivo al lado, se lee como elemento externo aunque el goo una.

```text
x = pill.x + pill.w - SEED_OVERLAP_PX
y = pill.y + (pill.h - h) / 2
w = GROW_START_W   // disco; luego → LAUNCHER_BAR_W (292)
```

Hold breve en disco (~150 ms) y chrome (icono/texto) oculto durante
`.is-expanding`. Al crecer solo `w` con `x` fijo, el blob se estira a la
derecha — misma lectura que el expand de dictado.

API: `bubble.place({ …, w, h, x, y, side, offset })` vía
[`bubble.svelte.ts`](../apps/desktop/src/lib/surfaces/overlay/bubble.svelte.ts).

Referencia: `placeFusedToPill` en `LauncherFloat.svelte`.

`FUSED_GAP_PX` (~2) se usa en **approach/close** para re-fundir el cuello, no
en el birth.

### Place after separate (idle)

Centrar la barra en el work area (`resolveSlot("center", …)`), con
`LAUNCHER_PILL_GAP` implícito en el slot de la pill
(`center-left-of-launcher`: pill a la izquierda de una barra de
`LAUNCHER_BAR_W`).

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
| `publishFollowSkin` | Geometría en movimiento continuo: **expand**, **separate**, **approach**, **shrink**, **drag**. rAF sin idle-stop (con tope de seguridad). |
| `publishEmergeSkin` | Morph `.float-emerge` o un solo rect que se quieta; idle-stop + tope de frames. |
| `publishMeasuredSkin` / `publishCompactPills` | Varias formas medidas del DOM (barra + dots `.is-out`). Republica solo si cambia la clave; idle-stop. |

Reglas del launcher (copiar el criterio, no el id):

1. **Durante expand/separate/approach/shrink** → `publishFollowSkin("launcher", el, CORNER)`.
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
| `--launcher-bar-open-dur` | `launcherBar` | **150 ms** |
| `--launcher-separate-dur` | `launcherSeparate` | **150 ms** |
| `--launcher-fav-stagger` | `launcherFavStagger` | **150 ms** |

Easing de referencia: `--ease-smooth-out` (`cubic-bezier(0.22, 1, 0.36, 1)`).

En JS: `ms(MOTION.launcherBar)` etc., y esperar con
`afterTransition(el, "width" | "left" | "transform", dur)`.

Al portar a otra tool: podés reutilizar estos tokens o añadir
`--clipboard-bar-open-dur` etc. en el mismo estilo; no hardcodear ms sueltos en
CSS y JS sin alinearlos.

---

## 7. CSS contracts

Clases de fase (launcher):

| Clase | Transiciones |
| --- | --- |
| `.is-expanding` | `width` (+ `height` si aplica) con `--launcher-bar-open-dur` |
| `.is-separating` | `left` / `top` con `--launcher-separate-dur` |
| hijos `.is-out` | `transform` + `opacity` con `--launcher-fav-stagger` |

Contratos importantes:

- El root usa `left` / `top` / `width` / `height` desde variables del bubble
  (`--x`, `--y`, `--w`, `--h`), no solo `transform` de emerge.
- **Armar** `.is-separating` **antes** de cambiar el ancla (tick + 1–2 frames);
  si no, `left` salta sin transición.
- Hijos: estado inicial cerca de la barra (`translateX` negativo + scale ~0.82,
  opacity 0); `.is-out` → `transform: none; opacity: 1`.
- **No** animar `height` en cada tecla de búsqueda (comentario en `.lf`: saltar
  a alto expandido para evitar thrash de layout + hit-rects).
- Compacto: overflow visible para que los peels vivan fuera del stadium.
- Hit-rect propio para peels (`surfaces.add("launcher-favs", favsEl)`) cuando
  `favRevealCount > 0`.

---

## 8. Recipe: portar a otro float

Objetivo: clipboard, snippets, agentes u otra tool con la misma lectura
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
   `(tuck) → approach → shrink → dismiss` + epoch + reduced-motion + **seed hold ~150 ms**.
8. **CSS** — Clases de fase; chrome (`head`/`body`/icono/input) **opacity 0** en expand/shrink.
9. **Liquid** — `publishFollowSkin` en expand/separate/approach/shrink.
10. **Hit-rects** — `surfaces.add` del root; peels fuera del bbox si aplica.
11. **Tokens** — `app.css` + `MOTION` / fallbacks.
12. **Cierre** — reverse completo; pill home tras hit-rect gone.

Referencia completa: `LauncherFloat.svelte` (comentario de cabecera +
`runOpenReveal` / `runCloseReveal` + effects de skin). Producto launcher:
[launcher-spotlight.md](launcher-spotlight.md).

---

## 9. Anti-patterns

| Evitar | Por qué |
| --- | --- |
| Abrir con **`.float-emerge` scale snap** como acto principal | Se lee “pop in”, no “creció desde la pill”. Ese morph sigue en agentes/auth; clipboard/snippets ya usan fused grow. |
| **Hide sin reverse** (`bubble.hide()` / Esc al instante) | Se lee “desapareció”; rompe la simetría con el open. Siempre approach → shrink → dismiss. |
| Meter favs/peels **dentro del flex del header** expandido | Desplazan el stadium, rompen el centro y el grow. Van absolute fuera del bar. |
| Esperar a **cargar hijos antes de colocar** la barra | La barra debe nacer fused al instante; los peels vienen después (`favs`). |
| Animar **height en cada tecla** de búsqueda | Thrash de layout, hit-rects y SDF. |
| Remeshear SDF en panel de resultados | Mismo thrash; chrome opaco basta. |
| Separar con gap **≤ REACH** en idle | Nunca corta el cuello; parecen pegados para siempre. |
| **Semilla con gap ≥ 0** (al lado de la pill) | Aunque `FUSED_GAP_PX = 2` una el goo, se ve un **segundo disco externo** desde el primer frame. Open y close deben **solapar** (`SEED_OVERLAP_PX`, gap negativo). |
| Grow con **`placePanelFusedFull`** desde la semilla | Reclava a gap+2 y salta el overlap → “apareció un panel”. Usá `expandPanelFromSeed` (borde clavado). |
| Chrome (título, lista, icono, input) **visible en expand/shrink** | Se lee control UI truncado, no blob. Ocultalo hasta separate / tras shrink. |
| Hold **0** en la semilla | Si estirás en el primer frame, no se registra el nacimiento. ~150 ms basta. |
| Re-anchor a **resting** mientras `revealPhase !== ready` | Un segundo IPC/workAreas hace snap a elemento separado a mitad del morph. |
| Fused gap **≥ REACH** al nacer | No hay blob único; el grow no se lee como un cuerpo. |
| Cambiar `left` en separate/approach **sin** clase de transition armada | Snap visible. |
| Volar la pill a **home** mientras el float aún hace reverse | El cuello se estira hacia un hogar lejano; esperar hit-rect gone. |
| Publicar solo el ancla lógico durante morph visual | Blob a tamaño lleno mientras el DOM anima otra cosa — por eso existen `publishFollowSkin` / emerge tracker. |

### Checklist de sensación (open y close)

1. Primer frame visible: **un** blob con la pill (disco solapado, sin chrome).
2. Luego el cuerpo **se estira** hacia afuera con cuello líquido (como el morph de la GIF de referencia).
3. Separate corta el cuello; recién ahí se ve el chrome completo.
4. Close: chrome se apaga → se acerca (cuello) → se encoge **solapado** en la pill → dismiss.
5. Si en open o close ves un rectángulo/stadium ya separado desde el inicio o el final, el patrón está mal aplicado.

---

## Código (mapa)

| Archivo | Rol |
| --- | --- |
| [`LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte) | Referencia: phases, place fused/center, favs, liquid |
| [`floatEmergeSkin.ts`](../apps/desktop/src/lib/surfaces/overlay/floatEmergeSkin.ts) | `publishFollowSkin` / emerge / measured |
| [`constants.ts`](../apps/desktop/src/lib/liquid/constants.ts) | `BLEND`, `REACH` |
| [`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts) | `LAUNCHER_BAR_W`, `LAUNCHER_PILL_GAP`, slots |
| [`motion.ts`](../apps/desktop/src/lib/motion.ts) + [`app.css`](../apps/desktop/src/app.css) | Tokens unificados a **150 ms** |
| [`floatPlace.ts`](../apps/desktop/src/lib/surfaces/overlay/floatPlace.ts) | Place resting + seed fused / grow (paneles) |
| [`floatReveal.ts`](../apps/desktop/src/lib/surfaces/overlay/floatReveal.ts) | `waitFrames`, `separateAxisProp` (open/close) |
| [`ClipboardFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/clipboard/ClipboardFloat.svelte) / [`SnippetsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/snippets/SnippetsFloat.svelte) | Adopción panel: fused grow → separate + reverse close |
| [`PillSurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte) | Vuelo al slot; return-home tras reverse; expand de dictado |

## Relacionado

- [liquid.md](liquid.md) — reglas transversales del goo
- [launcher-spotlight.md](launcher-spotlight.md) — feature que implementa este patrón
- [pill-shell.md](pill-shell.md) — pill, slots, morph rueda
- [clipboard-historial.md](clipboard-historial.md) / [snippets.md](snippets.md) — adoptaron fused grow
- [agentes.md](agentes.md) — float candidato a portar
