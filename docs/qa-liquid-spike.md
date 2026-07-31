# Spike: el sistema líquido dentro de WebView2

`Features/liquid.md` deja un pendiente explícito: *"Verificar dentro de WebView2: hasta
ahora solo se miró en Chrome con Vite"*. Todo el sistema líquido —y con él la dirección
visual de la app— descansa sobre un filtro SVG que nunca se probó en el motor donde la app
realmente corre.

Este documento es el protocolo para cerrar ese pendiente. Se corre una vez; el resultado
decide si la capa `liquid/` de la reescritura se construye sobre filtros CSS o sobre un
renderizador propio.

## El banco de pruebas

`apps/desktop/src/lib/dev/LiquidLab.svelte`, solo en dev. A diferencia de `docs/demos/`,
que son una implementación aparte, usa el **`GooFilter` de producción** y **la misma
geometría de cuello que `AgentsSurface`** (cápsula de 26→10 px, penetración 9/7, piso de 6,
corte a 140, `preFilter()` en las formas exactas). Lo que se ve es lo que hace la app.

## Dos renderizadores sobre la misma geometría

El botón de arriba del panel cambia entre los dos. **Esa comparación es el verdadero
objeto del spike**, más que el sí/no de WebView2.

**`filtro goo`** — el de producción. Difumina el alfa del grupo y lo vuelve a endurecer con
`a' = 18a − 7`. Tres consecuencias: depende del motor (de ahí este documento), engorda la
silueta `0.28σ` por lado y hay que compensarlo con `preFilter()`, y cruza como mucho `1.72σ`
por su cuenta — por eso el cuello de la burbuja hay que dibujarlo con cinco constantes
(penetración 9/7, grosor 26→10, piso 6, corte 140).

**`sdf`** — `apps/desktop/src/lib/liquid/{sdf,contour}.ts`. Cada forma es un campo de
distancia con signo; el grupo es la unión suave (`smin`) de esos campos, y el contorno se
traza con marching squares a un `<path>`. El filete cóncavo deja de ser un artefacto del
desenfoque y pasa a ser la geometría real de la unión. No depende del motor, no engorda, y
el alcance lo fija `k` en vez de estar atado a la viscosidad.

Su límite es otro: **marching squares no ve nada más fino que su celda**. El cuello de esta
escena baja a 6 px, así que `cell` tiene que ser 3 o menos, y el costo va con el cuadrado.
Para que eso sea viable, `contour.ts` descarta en bloque lo que está lejos del contorno
(`BLOCK`): en la escena de la pill eso baja las evaluaciones del campo de 39k a 11k por
cuadro, con un contorno idéntico —mismo número de puntos, mismo dibujo—. El descarte es
exacto, no una aproximación: como el gradiente del campo nunca supera 1, `|d| > lado·1.71`
en el centro de un bloque garantiza que no hay cruce ni ahí ni en las celdas que lo tocan.

## Cómo se abre

**Dentro del overlay real (WebView2) — es la medición que importa:**

1. `pnpm dev` en la raíz (levanta `tauri dev`).
2. Con la **ventana principal enfocada**, `Ctrl+Alt+L`.

El overlay no recibe teclado —nace `focusable(false)`— y Rust construye su URL, así que no
se le puede pedir el lab por query. El atajo vive en la ventana principal y viaja por
`localStorage`, que las dos ventanas comparten por ser del mismo origen, igual que ya hace
el tema. Mientras el lab está abierto, el overlay deja de ser click-through en toda la
pantalla; `cerrar lab` (o `Ctrl+Alt+L` otra vez) lo devuelve a la normalidad.

**En Chrome, como referencia:** con el dev server levantado, abrir
`http://localhost:1420/dev/liquid-lab`.

## Qué mirar

### 1. ¿Se funde? — la pregunta principal

Con los valores por defecto (σ = 5, hueco 10, `dibujar cuello` encendido), la pill y el
globo tienen que leerse como **una sola cosa**: un cuello que sale del globo y entra en la
pill, con un **filete cóncavo** en cada una de las dos uniones.

- ✅ **Pasa** si los filetes están. Son lo que aporta el filtro; el rectángulo del cuello lo
  dibuja el código.
- ❌ **Falla** si el cuello se ve como una barra pegada con esquinas rectas o si aparece un
  escalón de color en la unión.

### 2. Alcance — ¿el umbral es el mismo?

Apagar **`dibujar cuello`**. Quedan dos formas sueltas y el único puente posible es el que
pone el filtro. Con los botones de `hueco` (−1 / +1) buscar el punto exacto donde el puente
se corta, y comparar con el `alcance 1.72·σ` del panel.

- ✅ **Pasa** si se corta a ~8.6 px con σ = 5 (±1 px).
- ❌ **Falla** si se corta bastante antes o después. El sospechoso es
  `color-interpolation-filters="sRGB"`: si WebView2 lo ignora y opera en linearRGB, el
  endurecido `a' = 18a − 7` cae en otro sitio y **todos** los números del sistema
  (`GOO_GROW`, el piso de 6 px del cuello, las dos gotas de la burbuja) quedan mal
  calibrados.

Conviene repetirlo con σ = 3 y σ = 8 para ver si la relación se mantiene lineal.

### 3. Engorde — ¿`preFilter()` compensa lo correcto?

El **contorno punteado naranja** marca la geometría exacta pedida (pill de 176×40, globo de
580×520). La silueta filtrada tiene que morir **justo encima** del contorno.

- ✅ **Pasa** si el contorno queda pegado al borde de la silueta.
- ❌ **Falla** si la silueta asoma por fuera (el motor engorda más que `GOO_GROW = 1.4`) o
  si se mete por dentro (engorda menos). Cualquiera de los dos desalinea la piel del
  contenido en toda la app, porque la capa de tinta se dibuja con las medidas exactas.

### 4. Costo — ¿aguanta el arrastre?

Arrastrar el globo por la pantalla y mirar `fps` y `cuadro p95`. Después encender
**`animar (peor caso)`**, que mueve la geometría en **cada** cuadro: es más de lo que la app
pide nunca, porque en producción el filtro solo se repinta cuando la geometría cambia.

El interruptor **`filtro goo`** da el A/B contra exactamente el mismo trabajo sin filtrar,
que es lo que aísla el costo del filtro del costo de todo lo demás.

- ✅ **Pasa** con ≥ 50 fps y p95 < 20 ms animando.
- ⚠️ **Atención** si el arrastre va bien pero animar se cae: es aceptable (la app no anima
  la geometría por cuadro), pero conviene anotarlo.
- ❌ **Falla** si arrastrar ya se siente pesado.

### 5. La ventana transparente

Es lo que Chrome no puede reproducir: el overlay es transparente, `always_on_top` y
click-through. Mirar que el `drop-shadow` caiga sobre el escritorio sin recuadro opaco, que
no haya borde ni halo en la caja del elemento filtrado, y que la silueta no parpadee al
arrastrar sobre ventanas de otras apps.

## Referencia de Chrome (medida)

Chrome 1504×732, dpr 1.25, escena completa (pill 176×40 + globo 580×520, caja ~580×570).
`goo` con σ = 5; `sdf` con k = 26, `cell` = 3, suavizado 2. "Animando" mueve la geometría en
cada cuadro, que es el peor caso y más de lo que la app pide nunca.

| Escenario | fps | p95 | peor cuadro |
| --- | --- | --- | --- |
| Reposo (cualquiera) | ~176 | 5.8 ms | 11 ms |
| Animando, **goo** | 80 | 16.9 ms | 22.4 ms |
| Animando, **sdf** (banda estrecha) | **96** | **16.7 ms** | 22.1 ms |
| Animando, sdf sin banda estrecha | 70 | 22.2 ms | 28 ms |
| Animando, sin ninguno de los dos | 136 | 11.2 ms | 16.6 ms |

Fusión y filetes: correctos en los dos.

- **goo:** región del filtro **1.32–1.45 Mpx** (`-50%/200%` = 4× el área de la caja).
- **sdf:** **11.1k evaluaciones** de 39k vértices de grilla, ~5.8 ms de cálculo por cuadro,
  3.084 puntos de contorno.

El titular es que en Chrome el SDF ya **iguala o supera** al filtro, pese a correr en el
hilo principal en vez del compositor. Sin el descarte por bloques no lo haría: costaba
11 ms por cuadro y quedaba por debajo.

> Nota: `liquid.md` estima la región del filtro en "cerca de 3 Mpx". El número medido es la
> mitad. También difiere el grosor del cuello: el documento dice "de 22 px a 8" y el código
> (`AgentsSurface.svelte`) usa `NECK_THICK = 26` → `NECK_THIN = 10`. Manda el código.

## Resultado en WebView2

_Pendiente de correr._ Cada prueba se corre **con los dos renderizadores**.

| Prueba | goo | sdf | Notas |
| --- | --- | --- | --- |
| 1. Fusión y filetes | | | |
| 2. Alcance (σ=5/3/8 · k=10/26/60) | | | |
| 3. Engorde vs contorno | | | |
| 4. fps y p95 (arrastre / animando) | | | |
| 5. Ventana transparente | | | |

**El SDF ya es el plan B implementado.** Si el filtro falla en WebView2, no hay que rediseñar
nada: se cambia el renderizador y la arquitectura queda igual, que es exactamente para lo que
`Skin`/`Blob` reciben solo números.

Y si el filtro pasa, la decisión igual no es obvia — el SDF además borra `preFilter()`,
`GOO_GROW` y las cinco constantes del cuello dibujado. Esa es la comparación que hay que
hacer con los dos delante.

Cuando esté corrido, el resultado se vuelca al pendiente de
[`Features/liquid.md`](../Features/liquid.md).
