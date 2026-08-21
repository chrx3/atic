# Demos de fusión líquida

La idea es una sola: los elementos no aparecen ni se solapan, **se desprenden**
unos de otros y **vuelven a fundirse** al acercarse, con un cuello que se estira
y se corta. Una perilla —la viscosidad— va de juntas nítidas a gotas gomosas.

Se abren con doble clic: no hay build, ni servidor, ni dependencias.

| Archivo | Qué muestra |
| --- | --- |
| `index.html` | Índice. Empieza aquí. |
| `fusion.html` | El laboratorio: tres piezas que se arrastran y la cuenta del alcance en vivo. |
| `pill.html` | La barra que crece absorbiendo lo que llega, y el panel que se derrama de ella. |
| `rueda.html` | Las seis herramientas saliendo del núcleo como gotas estiradas. |
| `agentes.html` | La burbuja descolgándose de la pill, y el porqué de las dos gotas del cuello. |

`liquid.css` y `liquid.js` son compartidos. `liquid.js` es un script clásico y
no un módulo a propósito: los módulos ES no cargan por `file://`.

En esta carpeta también está `liquid-glass.html`, que es de **otro tema** —
compara el launcher de hoy contra lo que produciría `windowEffects`— y no tiene
que ver con la fusión.

## Cómo funciona

No es simulación de fluidos. Es el truco de *metaballs* con filtros SVG, el
mismo que la app usa mediante [`GooFilter.svelte`](../../apps/desktop/src/lib/GooFilter.svelte)
en las superficies de la pill y agentes:

1. `feGaussianBlur` difumina el grupo entero. Cada forma queda con un halo de
   alfa que se desvanece hacia afuera.
2. `feColorMatrix` endurece ese alfa: `a' = 18·a − 7`, recortado a `[0,1]`.
   Todo lo que quede por encima del umbral `7/18` vuelve a ser opaco.
3. `feComposite atop` devuelve el gráfico nítido encima, para que el endurecido
   no coma los bordes de las formas originales.

Entre dos formas cercanas los dos halos **se suman**. Donde la suma pasa el
umbral, el hueco se rellena: eso es el cuello. Al alejarse la suma cae, el
cuello se afina y se corta.

### El alcance

Para un borde recto, el alfa difuminado a distancia `d` por fuera del borde vale
`Φ(−d/σ)`. En el medio de un hueco `g` cada borde aporta `Φ(−g/2σ)`, así que el
cuello existe mientras `2·Φ(−g/2σ) > 7/18`. Despejando:

```
hueco máximo ≈ 1.72 · σ
```

Con la σ = 5 de la app, el cuello se corta a **8.6 px**. `Liquid.reach()` lo
calcula por bisección y las demos lo muestran en vivo contra el hueco real. Es
el caso de bordes rectos; entre dos formas redondas el alcance es algo menor,
porque la curvatura le quita área al halo.

## Reglas que la app debe conservar

**1. La piel tiene que separarse del contenido.** El filtro difumina todo lo
que tenga adentro: el texto y los iconos se convertirían en manchas. La
estructura que usan las demos —y que la implementación actual ya aplica— es

```
.stack
  .skin   siluetas, filtradas, sin contenido    →  se funden
  .ink    contenido, intacto, sin fondo propio  →  se lee
```

En la app, [`PillSurface.svelte`](../../apps/desktop/src/lib/PillSurface.svelte)
y [`AgentsSurface.svelte`](../../apps/desktop/src/lib/AgentsSurface.svelte)
mantienen esta separación. El precio sigue siendo que la silueta y el
contenido duplican geometría y deben permanecer alineados.

**2. Todo lo que se funde va del mismo color.** El cuello lo pinta el
difuminado, que promedia lo que tenga cerca; dos tonos distintos dejan una
franja sucia justo en la unión.

**3. La sombra va después del goo, en la misma cadena de `filter`.** Así cae
sobre la silueta ya fundida. Una `box-shadow` en cada forma entra al filtro
como alfa parcial, el endurecido la lee y la usa para alargar el cuello: la
fusión termina ocurriendo más lejos de lo que se ve, sin motivo aparente.

**4. `filter` aísla el backdrop.** Un elemento con `filter` crea bloque
contenedor y corta el `backdrop-filter` de sus descendientes. Fundirse y
refractar no caben en la misma capa; si alguna vez se quieren las dos cosas,
la salida es enmascarar una superficie con la silueta ya fundida.

## Lo que apareció mirando el código

El cuello de la burbuja de agentes son dos gotas (`22×18` y `14×14`, con la
punta solapada 6 px) y en el código no está escrito por qué. La cuenta lo
explica: la separación real entre la pill y el globo es `gap: 10` px
(`agents/bridge.rs`), y con σ = 5 el filtro solo cruza **8.6**. Sin las gotas
intermedias el cuello no se forma y la burbuja queda suelta —
`agentes.html` lo deja apagar para verlo.

Hay una alternativa: con **σ ≥ 5.8** el alcance supera los 10 px y el cuello
sale solo, sin gotas.

La versión anterior de la burbuja escribía `mode="matrix"` en
`feColorMatrix`; el atributo correcto es `type`. El componente compartido
actual ya usa `type="matrix"`.

## Atajos

En la URL:

- `#oscuro` — arranca en tema oscuro
- `#quieto` — congela las animaciones

El fondo se invierte con el tema, y eso es una decisión de la demo, no una
propuesta: acá lo que hay que mirar es la silueta, y una piel oscura sobre un
fondo oscuro no se lee. En la app el fondo es el escritorio del usuario y no se
elige.
