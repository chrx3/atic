# Lineamientos de diseño de la pill

Cómo se ve y cómo se comporta cada superficie de la pill, para que una
herramienta nueva no tenga que inventar nada y una vieja no se desvíe.

Esto **no** repite el sistema líquido —cómo se funden las siluetas está en
[`Features/liquid.md`](../Features/liquid.md)— ni el catálogo de producto, que
está en [`Features/README.md`](../Features/README.md). Acá está el contrato
visual y de interacción.

Los valores citados salen del código y son la fuente de verdad:
[`pillStage.ts`](../apps/desktop/src/lib/surfaces/overlay/pillStage.ts) para la
geometría, [`app.css`](../apps/desktop/src/app.css) y
[`motion.ts`](../apps/desktop/src/lib/motion.ts) para el movimiento. Si un
número de acá no coincide con el código, el código gana y este documento está
desactualizado.

---

## La regla

> **El estado es la forma. La herramienta solo llena el hueco.**

La pill decide su tamaño, su silueta y su coreografía a partir de en qué estado
está — no a partir de qué herramienta esté activa. Una herramienta aporta
icono, etiqueta y acción; nunca su propio tamaño de botón, su propia duración
ni su propia forma. Cuando una se sale de eso, se nota enseguida: el aviso de
agente llegó a pintarse como una capa en `inset: 0` sobre la pestaña entera,
con un logo de 9 px adentro, y para que se viera había que apagar la marca de
Atic.

> **Una silueta por modo. El estado nunca la cambia.**

Acoplada es una pestaña; flotando, una cápsula; abierta, una tira. Lo que pasa
adentro —grabar, dictar, un aviso— **alarga** la forma a lo largo de su eje,
nunca le cuelga un bulto ni la sustituye por otra. Colgar era lo que hacía que
la pestaña cerrada, la tira abierta y la cápsula flotante no se leyeran como la
misma cosa.

La única excepción es la rueda, que es su propio escenario cuadrado: ahí la
gota de actividad no rompe ninguna continuidad porque no hay ninguna que
romper.

---

## Los tres ejes de estado

El estado de la pill **no** es un enum grande. Son tres ejes ortogonales que se
combinan, más el acople al borde. Están marcados como tal en
[`PillSurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte)
(`─── Eje 1 ───`, etc.) y su tipo vive en
[`pillPlan.ts`](../apps/desktop/src/lib/surfaces/overlay/pill/pillPlan.ts).

| Eje | Valores | Quién manda |
|---|---|---|
| **1. Actividad** | `idle` · `recording` · `dictating` | grabación y dictado, no la UI |
| **2. Superficie** | `none` · `wheel` · `edge` | el usuario: clic, `Alt+Z`, arrastre al borde |
| **3. Cola de pegado** | vacía · con ítems | el clipboard cuando no hay destino |
| **Acople** | `Dock { edge, expanded }` | contra qué lado se aplana y si el puntero la abrió |

`Dock` va aparte de `Surface` a propósito. Multiplicar los estados
(`edge-left`, `edge-left-open`, …) haría cuatro veces el mismo trabajo: el
borde no cambia lo que la pill *es*, solo contra qué lado se aplana y hacia
dónde crece.

**Al agregar un estado nuevo, preguntá primero de qué eje es.** Si no es de
ninguno, casi siempre es un aviso (ver más abajo) y no un estado.

### Qué se ve en cada combinación

| Estado | Forma | Qué muestra |
|---|---|---|
| `none` + idle | disco de `bar` (52) | la marca de Atic, viva |
| `none` + recording | cápsula | punto rojo, cronómetro, ondas, stop |
| `none` + dictating | cápsula | icono de dictado + ondas, sin texto |
| `none` + cola | cápsula | marca, contador, pegar, descartar |
| `wheel` | cuadrado de `wheel` (252) | núcleo + gajos de herramienta |
| `edge` cerrada | pestaña `islandThick` × `islandLong` | la marca; los avisos al lado si hay |
| `edge` abierta | tira `islandTool` de largo variable | la marca, un botón por herramienta, el update |

**La marca está en las tres.** Es lo que hace que se lean como la misma pill
desplegándose, y no como tres controles distintos. En la tira abierta es la
primera celda y su clic abre la rueda, igual que el cuerpo de la pestaña.

**La actividad no cuelga: la dice la cara de la marca.** Grabando, el círculo
de la 'a' se llena con el cuadrado rojo latiendo; dictando, con tres barras.
La cabeza y el asta no se mueven, así que la marca se sigue leyendo.

**Y la marca es el control de lo que muestra.** Si la cara dice qué está
corriendo, apretarla lo para; en reposo abre la rueda. Un segundo botón rojo al
lado decía dos veces lo mismo y se comía el ancho que la cápsula necesita para
el contador. Vale para las tres formas: pestaña, tira y cápsula. Por eso la
actividad tampoco cuenta en `edgeCueMarks`: su control ya estaba ahí.

`liveHang` solo devuelve algo en `wheel`, y `islandLiveSlots()` devuelve 0 — la
función quedó para no reescribir los tests de geometría.

---

## Geometría

Un solo archivo decide los tamaños:
[`pillStage.ts`](../apps/desktop/src/lib/surfaces/overlay/pillStage.ts). Nadie
más escribe píxeles de layout de la pill.

| Token | px | Qué es |
|---|---|---|
| `pad` | 4 | respiro entre el contenido y el borde de la ventana |
| `bar` | 52 | alto de la barra compacta y diámetro del disco en reposo |
| `wheel` | 252 | lado del escenario de la rueda (232 del disco + 10 de aire por lado) |
| `panelW` / `panelH` | 312 / 332 | panel de historial y fragmentos |
| `islandThick` | 40 | lo que la pestaña asoma hacia adentro |
| `islandCueThick` | 42 | ídem, con aviso: cabe un icono legible |
| `islandLong` | 56 | dintel de la pestaña a lo largo del borde |
| `islandMark` | 32 | la marca de Atic dentro de la pestaña |
| `islandCueBtn` | 26 | botón de aviso al lado de la marca |
| `islandCueMark` | 14 | logo de agente / icono de update dentro de ese botón |
| `islandTool` | 44 | botón de herramienta en la tira abierta |
| `islandGap` | 2 | hueco entre botones de la tira |
| `recDrop` / `recDropGap` | 36 / 8 | gota de actividad y su cuello — **solo en la rueda** |
| `wheelLiveHang` | 28 | alto extra de la rueda cuando hay gota viva |

### Reglas que estos números tienen que respetar

No son preferencias: romperlas produce bugs concretos que ya pasaron.

1. **`islandCueThick` < `islandTool`.** Si la pestaña cerrada fuera más gorda
   que la tira abierta, el hover para abrir encogería un eje, el cursor
   quedaría fuera y el ciclo abrir/cerrar se realimentaría a 60 Hz.
2. **Abrir la isla nunca encoge la caja en ningún eje.** Mismo bucle, por el
   otro lado: la tira abierta puede quedar más corta que la pestaña cuando el
   usuario esconde herramientas desde Ajustes, así que `contentFor` le pone
   piso con `Math.max` contra el largo cerrado. Hay un test que barre
   `toolCount` de 1 a 9 contra `cueCount` de 0 a 3.
3. **Acoplada, la caja crece solo a lo largo del borde.** Nunca hacia adentro:
   hacia adentro tapa pantalla y, peor, cambia la silueta con el estado. Hay un
   test que exige que grabando y dictando den exactamente la misma caja que en
   reposo, en los cuatro bordes y en los dos estados de apertura.
4. **El área de clic es la caja, no lo dibujado.** El puntero recibe el
   rectángulo del elemento aunque el líquido pinte otra cosa. Publicar la barra
   entera estando acoplada dibujaba el disco sobre una zona viva del tamaño de
   la pestaña: se veía sin cambiar y no respondía.
5. **Entre dos gotas circulares no puede haber hueco muerto.** Un clic que cae
   entre iconos llega al cuerpo de la isla, y el cuerpo abre la rueda. Por eso
   `islandGap` es 2 y los botones llevan un `::before` que cubre el filete.
6. **Restar `--goo-grow` a lo que se dibuje con tamaño exacto.** El endurecido
   del filtro engorda la silueta 1.68 px por lado. Ver
   [`liquid.md`](../Features/liquid.md).

---

## Movimiento

Las duraciones son tokens de [`app.css`](../apps/desktop/src/app.css), leídos
desde JS con `ms(MOTION.x)` de
[`motion.ts`](../apps/desktop/src/lib/motion.ts). **Ningún componente escribe
un número de milisegundos suelto**: si lo hace, deja de responder al tema y a
`prefers-reduced-motion`.

| Token | ms | Para qué |
|---|---|---|
| `--duration-micro` | 40 | acuse de recibo de un press |
| `--morph-quick-dur` | 60 | cierre acelerado: la rueda ya cumplió |
| `--duration-quick` | 75 | hover de un botón |
| `--morph-fade-dur` | 80 | entra o sale un hijo de la barra |
| `--launcher-separate-dur` | 90 | la barra se despega del launcher |
| `--morph-close-dur` | 100 | cerrar rueda, float, launcher |
| `--morph-open-dur` | 110 | abrir la rueda |
| `--panel-dur` | 110 | panel, y la gota que llega a la barra |
| `--flight-dur` | 110 | vuelo de la pill al cursor |
| `--duration-fast` | 125 | cambio de estado visible |
| `--duration-medium` | 150 | apertura de float |
| `--island-open-dur` | 190 | abrir y cerrar la tira acoplada |
| `--duration-slow` | 200 | lo que el usuario tiene que alcanzar a leer |
| `--morph-stagger` | 16 | entre gotas de la rueda |
| `--island-stagger` | 26 | entre botones de la tira |

Curvas: `--ease-liquid` `cubic-bezier(0.5, 0, 0.2, 1)` para lo que se deforma,
`--ease-smooth-out` `cubic-bezier(0.22, 1, 0.36, 1)` para lo que entra y frena.

### Reglas de movimiento

1. **Todo lo que abre, cierra.** Una transición `in:` sin `out:` desmonta en
   seco y el campo líquido corta. Por eso existe `opacityFade` en `motion.ts`
   (`transition:`, no `in:`) y no el keyframe `p-in` que solo corría de ida.
   Si un estado se monta con animación, tiene que salir con la suya.
2. **Solo opacidad para entrar y salir de la barra.** Un `transform` acá pelea
   con el `scale` de hover y press de los botones, y la animación gana sobre la
   transición.
3. **El outro no puede sumar ancho.** Dos estados de la barra vivos a la vez
   medirían viejo + nuevo y la cápsula saltaría. Se resuelve apilándolos en la
   misma celda de grid (`.p-bar-slot`), no acortando la salida.
4. **Cambiar de contenido es cerrar y abrir, no reemplazar.** Pasar de un
   anillo de la rueda a otro desmontando el viejo se lee como apertura sin
   cierre. Las gotas vuelven al núcleo y salen las nuevas (`swapWheelPage`).
5. **`prefers-reduced-motion` apaga la coreografía, no el estado.** Con reduce
   activo la isla salta entre cerrada y abierta sin recorrido, pero no se fuerza
   el estado abierto: las reglas de `.is-open` siguen mandando.

---

## Avisos: agente, update, cola

Un aviso **no** es un estado: es algo que aparece al lado de lo que ya está.
Los tres siguen el mismo contrato.

1. **Nunca reemplazan a la marca.** La marca de Atic es la puerta a la rueda;
   taparla deja al usuario sin acceso. En la pestaña acoplada conviven: la
   pestaña se alarga a lo largo del borde y `islandCueLong` mide marca + un
   botón por aviso.
2. **Crecen a lo largo del borde, no hacia adentro.** Hacia adentro tapan
   pantalla del usuario; a lo largo del canto hay lugar. Ninguno cuelga.
3. **La actividad no es un aviso: es la marca.** No suma un chip a la fila ni
   cuenta en `edgeCueMarks`. La cara dice cuál es y el clic la para.
4. **Son un clic solo si llevan a algún lado.** Si se puede accionar, es un
   `<button>` con `aria-label` y tooltip: la cola muestra *pegar* y *descartar*,
   el update baja e instala, el chip de agentes enfoca la consola. Si no lleva
   a ningún lado —un agente sin terminal que enfocar ni atar— es texto con
   `role="status"` y sin cursor de mano. Un botón que se come el gesto sin
   decir nada es peor que un aviso pasivo.
5. **Lo que promete el globo es lo que hace el clic simple.** No una acción
   escondida detrás de Ctrl. El chip de agente sin terminal decía «Clic vincula
   la última ventana» y el clic intentaba *enfocar* una ventana inexistente:
   se perdía el gesto.
6. **Ningún texto de aviso estira la pill sin techo.** El preview de un agente
   llega con el largo que venga: va con `max-width` y elipsis, y el texto
   entero en el globo.
7. **Con la isla abierta se desmontan de la pestaña.** Si quedaran, taparían
   los iconos. El update reaparece como última celda de la tira.
8. **Abrir espera si el aviso está en la pestaña.** El icono vive en la pestaña
   cerrada: abrir en el mismo cuadro del hover lo desmonta y el clic cae en una
   herramienta. `UPDATE_ISLAND_OPEN_DELAY_MS` (180) da tiempo a apretarlo.

---

## Iconos y tamaños de contenido

| Dónde | Tamaño | Trazo |
|---|---|---|
| Marca en el disco en reposo | 32 | 1.5, `alive` |
| Marca en la pestaña acoplada | `islandMark` (32) | 1.6, `alive` |
| Herramienta en la tira abierta | 22 | 1.6 |
| Logo de agente en el aviso | `islandCueMark` (14) | — |
| Icono en botón de chrome denso | 13–16 | 1.5–2 |

**El área de clic no es el icono.** Un icono de 13 px vive en un botón de
`bar` (52) en la barra, o de `islandCueBtn` (26) en la pestaña. Cuando el botón
tiene que ser chico por diseño, se le cuelga un `::after` transparente para
llevar el hit a ≥ 40 px — sin inflar el icono ni solaparse con el vecino.

---

## Contrato de una herramienta nueva

Lo que sí define la herramienta, y nada más:

- Su entrada en [`tools.ts`](../apps/desktop/src/lib/core/tools.ts): `id`,
  `label`, `short`, `blurb`, `actionLabel`, y `comingSoon` mientras no esté.
- Su icono, registrado en `TOOL_ICONS`
  ([`icons.ts`](../apps/desktop/src/lib/icons.ts)) y pintado con
  [`ToolIcon`](../apps/desktop/src/lib/ToolIcon.svelte) — nunca un `<svg>`
  suelto ni un import directo de Lucide.
- Su superficie propia (float, panel, ventana), si la necesita.
- Sus textos, en `es.ts` **y** `en.ts`, nunca hardcodeados.

Lo que hereda y no debe redefinir: el tamaño del botón, el gap, las duraciones,
las curvas, la silueta, el comportamiento del hover, el orden de slots
([`toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts)) y la
decisión de si su superficie se funde o no
([`liquid.md`](../Features/liquid.md)).

### Checklist antes de dar por cerrada una herramienta

- [ ] ¿Se ve igual en la rueda, en la tira acoplada y en la barra?
- [ ] ¿Tiene estado de carga, vacío y error, y los tres caben en la forma?
- [ ] ¿Entra y sale con transición, o desmonta en seco?
- [ ] ¿El hit del botón llega a 40 px?
- [ ] ¿Los textos están en los dos idiomas?
- [ ] ¿Respeta `prefers-reduced-motion`?
- [ ] ¿Abrir su superficie encoge la caja en algún eje?
- [ ] ¿Sus tamaños salen de `PILL` y sus tiempos de `MOTION`?

---

## Dónde vive cada cosa

| Archivo | Rol |
|---|---|
| [`pillStage.ts`](../apps/desktop/src/lib/surfaces/overlay/pillStage.ts) | `PILL`: las medidas |
| [`pillPlan.ts`](../apps/desktop/src/lib/surfaces/overlay/pill/pillPlan.ts) | estado derivado puro: qué tamaño va ahora |
| [`pillCssStage.ts`](../apps/desktop/src/lib/surfaces/overlay/pillCssStage.ts) | ejecutor: escribe `left`/`top` |
| [`PillSurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte) | la pill: geometría, atajos, siluetas |
| [`motion.ts`](../apps/desktop/src/lib/motion.ts) | tokens de movimiento y helpers |
| [`app.css`](../apps/desktop/src/app.css) | los tokens en sí, y el tema |
| [`tools.ts`](../apps/desktop/src/lib/core/tools.ts) | catálogo de herramientas |

`pillPlan.ts` es puro y está testeado
([`pillPlan.test.ts`](../apps/desktop/src/lib/surfaces/overlay/pill/pillPlan.test.ts)):
**las reglas de geometría se prueban ahí, no a ojo en la ventana.** Un cambio
de layout que no se puede expresar como test de `pillPlan` suele ser una señal
de que la decisión está en el lugar equivocado.

---

## Relacionado

- [`Features/liquid.md`](../Features/liquid.md) — cómo se funden las siluetas
- [`Features/pill-shell.md`](../Features/pill-shell.md) — la pill como shell
- [`Features/pill-liquid-emerge.md`](../Features/pill-liquid-emerge.md) — patrón fused grow → separate
