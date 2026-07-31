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

Chrome 1504×732, dpr 1.25, σ = 5, hueco 10, escena completa (pill 176×40 + globo 580×520).

| Escenario | fps | cuadro p95 | peor cuadro |
| --- | --- | --- | --- |
| Reposo | 176 | 5.8 ms | 11.1 ms |
| Animando, **con** filtro | 80 | 16.9 ms | 22.4 ms |
| Animando, **sin** filtro | 136 | 11.2 ms | 16.6 ms |

Región del filtro: **1.32–1.45 Mpx** (la caja mide ~580×570 y la región es `-50%/200%`, o
sea 4× el área). Fusión y filetes: correctos.

> Nota: `liquid.md` estima esa región en "cerca de 3 Mpx". El número medido es la mitad.
> También difiere el grosor del cuello: el documento dice "de 22 px a 8" y el código
> (`AgentsSurface.svelte`) usa `NECK_THICK = 26` → `NECK_THIN = 10`. Manda el código.

## Resultado en WebView2

_Pendiente de correr._

| Prueba | Resultado | Notas |
| --- | --- | --- |
| 1. Fusión y filetes | | |
| 2. Alcance a σ=5 / 3 / 8 | | |
| 3. Engorde vs contorno | | |
| 4. fps y p95 (arrastre / animando / sin filtro) | | |
| 5. Ventana transparente | | |

**Si algo falla**, el plan B ya está decidido: la piel pasa a `<svg>` con paths analíticos o
a `<canvas>`. Las primitivas `Skin`/`Blob` reciben **solo números** justamente para que ese
cambio sea de renderizador y no de arquitectura.

Cuando esté corrido, el resultado se vuelca al pendiente de
[`Features/liquid.md`](../Features/liquid.md).
