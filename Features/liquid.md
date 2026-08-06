# Sistema líquido

**Estado:** `en curso`

## Resumen

Las superficies de Atic no aparecen ni se solapan: **se desprenden** unas de
otras y **vuelven a fundirse** al acercarse, con un cuello que se estira y se
corta. No es un efecto decorativo — es cómo se dice de dónde salió cada cosa.

## La regla

> **Dos formas se funden cuando una sale de la otra.**

Eso alcanza para decidir qué se toca y qué no, sin caso por caso:

| Superficie | ¿Se funde? | Con qué |
| --- | --- | --- |
| Float clipboard / textos | sí | hermanos de la pill en el overlay (salen de ella) |
| Rueda de herramientas (compacta) | sí | las seis gotas salen del núcleo |
| Float de agentes | sí | cuelga de la pill, por el cuello |
| Barra al crecer (grabando, cola, aviso) | sí (pendiente) | lo que llega se incorpora al disco |
| Rail / picker de la ventana principal | sí | las gotas salen del arco, entre sí |
| Cards del picker (derecha de la rueda) | sí* | mismo material; flotan aparte (hueco > REACH), no se pegan al arco |
| Modal de detalle de herramienta | **no** | aparece encima; no sale del rail |
| Launcher | **no** | aparece en el cursor, no sale de nada |
| Shelf de capturas | **no** | idem |

Si algo no sale de nada, fundirlo es ruido.

## Cómo funciona

Metaballs con filtros SVG: difuminar el grupo y volver a endurecer el alfa con
`a' = 18·a − 7`. Entre dos formas cercanas los halos **se suman**, pasan el
umbral `7/18`, y el hueco se rellena. `feComposite atop` devuelve el gráfico
nítido encima para que el endurecido no coma los bordes.

**El alcance** —el hueco máximo que el cuello todavía cruza— sale de
`2·Φ(−hueco/2σ) > 7/18`, o sea **1.72·σ**. Con σ = 5 son **8.6 px**.

De ahí sale, por ejemplo, que el cuello de la burbuja haya que dibujarlo: la
separación entre la pill y el globo es `gap: 10` (`agents/bridge.rs`) y el
filtro solo cruza 8.6. Lo que el filtro aporta no es el puente sino los filetes
cóncavos donde el cuello nace y muere.

## Tres reglas que mandan sobre el markup

1. **La piel va aparte del contenido.** El filtro difumina todo lo que tenga
   adentro: el texto y los iconos se volverían manchas. La estructura es una
   capa de siluetas (filtrada, sin contenido) y otra de contenido (intacta,
   sin fondo propio), con la misma geometría. El precio es que la geometría
   queda duplicada y tiene que coincidir.
2. **Todo lo que se funde va del mismo color** (`--skin`). El cuello lo pinta
   el difuminado, que promedia lo que tenga cerca; dos tonos dejan una franja
   sucia justo en la unión.
3. **La sombra va después del goo, en la misma cadena de `filter`**, para que
   caiga sobre la silueta ya fundida. Una `box-shadow` por forma entra al
   filtro como alfa parcial, el endurecido la lee y la usa para alargar el
   cuello: la fusión termina ocurriendo más lejos de lo que se ve.

También: el endurecido **engorda** la silueta `--goo-grow` por lado (0.28·σ =
1.4 px). Quien dibuje una forma de tamaño exacto tiene que restarle el doble
antes, o el disco de 40 sale de 43. En JS está `preFilter()`.

## Código

- [`apps/desktop/src/lib/liquid/Skin.svelte`](../apps/desktop/src/lib/liquid/Skin.svelte) — piel SDF (producción)
- [`apps/desktop/src/lib/GooFilter.svelte`](../apps/desktop/src/lib/GooFilter.svelte) — filtro SVG legacy (`ParticleWheel` compact)
- [`apps/desktop/src/app.css`](../apps/desktop/src/app.css) — `--skin` y `--goo-grow`
- [`apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte`](../apps/desktop/src/lib/surfaces/overlay/pill/PillSurface.svelte) — publica siluetas al grupo del overlay
- [`apps/desktop/src/lib/ParticleWheel.svelte`](../apps/desktop/src/lib/ParticleWheel.svelte) — `.pw-skin`, núcleo + seis gotas (solo `compact`, goo SVG)
- [`apps/desktop/src/lib/surfaces/main/ToolRail.svelte`](../apps/desktop/src/lib/surfaces/main/ToolRail.svelte) — picker de la ventana principal: arco + cards (SDF local)
- [`apps/desktop/src/lib/surfaces/overlay/agents/AgentsFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/agents/AgentsFloat.svelte) — junta pill ↔ float de agentes vía el grupo del overlay

## La junta pill ↔ consola

Era el límite duro del sistema: la pill y la burbuja eran ventanas distintas, y
un filtro SVG solo alcanza lo que está en el mismo documento. El cuello se
dibujaba dentro de la burbuja y estiraba hacia la pill — un puente pintado, no
una fusión.

Se cerró mudando las dos a **una sola ventana overlay**. Hoy `.bub-skin` lleva,
en una capa filtrada, tres siluetas: la de la pill, dos gotas de cuello y la del
globo.

Dos consecuencias que hay que tener presentes al tocarlo:

- **La silueta de la pill se dibuja dos veces** — en su propia capa (que tiene
  su filtro, para fundir su barra con su panel) y otra vez acá. Son del mismo
  color y del mismo tamaño, así que la copia no se ve: lo único que aporta es
  darle al cuello algo con qué fundirse. La pill publica su rectángulo como
  `pill-skin` en `overlaySurfaces`.
- **El cuello se DIBUJA, no se rellena solo.** Es una cápsula que entra 9 px en
  la pill y 7 px en el globo, así que cruza cualquier hueco; lo que cambia al
  separarse es el grosor, de 22 px a 8. Los filetes cóncavos de las dos puntas
  los pone el filtro. Se corta a los 140 px, que es donde deja de leerse como
  una cosa estirándose y pasa a ser un hilo entre dos.
- **El grosor tiene un piso de 6 px.** El endurecido borra lo que no llegue al
  umbral: una barra de grosor `g` difuminada con σ = 5 queda con alfa
  `2·Φ(g/10) − 1` en su eje, y hace falta pasar 7/18. Por debajo de 6 el cuello
  se cortaría solo a mitad del estirado.
- **Arrastrar el globo no lo despega.** Lo hacía cuando el cuello era un dibujo
  fijo: movido de sitio apuntaba a donde la pill no estaba. Ahora sale de los
  dos rectángulos en vivo, así que estira siguiendo al globo.

Y el color: todas las superficies del grupo usan el mismo `--skin` del tema
(`app.css`), para que el cuello no deje una franja sucia al promediar tonos.

## Límites que no se pueden cerrar

**La rueda compacta no tiene pie de texto.** No entra: el centro mide 58 y
"HERRAMIENTAS" pide unos 90, y lo que sobra cae sobre la ventana transparente.
Debajo del anillo tampoco — las gotas llegan a 98 del centro y `.p-wheel` mide
232. Recuperarlo obliga a agrandar `PILL.wheel` y colgarle una pastilla fundida
abajo. El nombre sigue en el `title` de cada gajo.

**La región del filtro es cara.** `GooFilter` declara `-50% / 200%`, o sea que
la capa de la junta rasteriza una región del triple del área de la caja que
envuelve pill y globo — cerca de 3 Mpx. Solo se paga cuando la geometría
cambia, no por cuadro, pero es el primer sitio donde mirar si el overlay se
siente pesado.

## Pendiente / siguiente

- [x] Unificar cuello y cuerpo de la burbuja en una sola silueta filtrada. Se resolvió con el overlay: cuello, cuerpo y pill comparten capa. El `border: 1px` de `.bub-body` se fue — dentro del goo cruzaba la base del cuello
- [ ] Verificar dentro de WebView2: hasta ahora solo se miró en Chrome con Vite

## Exploración

`docs/demos/` tiene cuatro demos sueltas de donde salió todo esto, con perilla
de viscosidad y lectura en vivo del hueco contra el alcance. Son una
implementación **aparte** de la de la app (`docs/demos/liquid.js`): sirven para
probar ideas, no se mantienen sincronizadas.

## Relacionado

- [pill-shell.md](pill-shell.md)
- [agentes.md](agentes.md)
