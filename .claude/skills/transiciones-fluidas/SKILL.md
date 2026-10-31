---
name: transiciones-fluidas
description: Cómo diseñar, implementar y revisar transiciones fluidas en la pill GPUI (elemento compartido, container transform, animación guiada por gesto, coreografía por etapas, carruseles). Úsala antes de tocar cualquier animación, morph o cambio entre vistas de la pill — vistazo → panel, tira, notch, letra que cuelga, Ahora suena, paneles que crecen, contenido que entra o sale — y al revisar un parpadeo, salto, recorte o "se ve doble".
---

# Transiciones fluidas en la pill

Esta skill junta lo que aprendimos al rehacer la transición de **Ahora suena**:
la letra que cuelga del notch → el vistazo al pasar por la carátula → el panel
completo al hacer scroll. Explica cómo se llama esto, qué reglas sigue, qué
piezas del código ya existen y cómo revisar que de verdad se vea fluido.

Todo el trabajo va en `apps/atic` (ver `AGENTS.md`). Escribe en
español de Chile, con tuteo.

---

## 1. Cómo se llama esto

No es un solo truco: son varias ideas conocidas que trabajan juntas.

| Nombre | Qué es | Dónde lo usamos |
|---|---|---|
| **Transición de elemento compartido** (*shared element transition*; en SwiftUI `matchedGeometryEffect`, en Flutter *Hero*) | Un mismo elemento viaja de una vista a otra cambiando posición y tamaño, **sin duplicarse** | La letra: una sola que baja de la franja al vistazo y crece hasta la del panel |
| **Container transform** (Material Design) | Un contenedor chico se estira y se convierte en uno grande; su contenido se reacomoda adentro | El vistazo de Ahora suena que se transforma en el panel |
| **Animación guiada por gesto** (*gesture-driven*, *scrubbable*, *interactive transition*) | El gesto no dispara la animación: **la controla**. Lento va lento; si retrocedes, retrocede | El scroll sobre la carátula (`scroll_pull` 0 → 1 vistazo, 1 → 2 panel) |
| **Interrumpible y redirigible** | Puedes cambiar de idea a mitad de camino y sigue desde donde estaba, sin saltos | `Tween::set` parte del valor actual, no del inicio |
| **Escalonado** (*staggering*) | Las partes entran una tras otra, no todas juntas | `Motion::enter(i)` en los vistazos: título, controles, letra |
| **Coreografía** (*choreography*, Material) | Decidir qué se mueve, en qué orden y **quién manda** en cada tramo | `media_handoff`: hasta el aterrizaje dibuja el vistazo; después, el panel |
| **Continuidad espacial** | Nada aparece de la nada: viene de algún lugar y vuelve a él | La letra sube de vuelta a la franja al cerrar el vistazo |
| **Carrusel sincronizado** (letra de Apple Music) | Al cambiar de verso, todo sube un renglón; el siguiente crece hasta el lugar del actual | `hang::verse_carousel` |

### ¿Es estilo Apple?

Sí. La referencia es la charla **"Designing Fluid Interfaces"** de la WWDC 2018.
Sus ideas, en corto:

- **Responder al instante** y seguir al dedo 1:1 mientras dura el gesto.
- **Se puede interrumpir y redirigir** en cualquier momento.
- **Continuidad espacial:** las cosas salen de algún lugar y vuelven a él.
- **Al soltar, se respeta el impulso:** termina hacia donde iba el gesto
  (`settle_scroll` usa la dirección del último movimiento).
- **Nada se duplica ni se reinicia** mientras el usuario mira.

Ejemplos de Apple: la Dynamic Island que se convierte en la Live Activity,
abrir una app desde su ícono, la letra sincronizada de Apple Music y el
reproductor que pasa de la barra mini a pantalla completa.

---

## 2. Las reglas

Son ocho. Cada una sale de un error real que tuvimos en Ahora suena (sección 5).

### Regla 1 — Un solo dueño por elemento en cada momento

Un elemento que existe en dos vistas (la letra, el título, los controles) lo
dibuja **una sola** de ellas en cada cuadro. Nunca dos copias superpuestas
haciendo un fundido cruzado, salvo que en ese momento sean idénticas (regla 2).

- Decide explícitamente quién manda en cada tramo y escríbelo en el doc
  comment (ver `media_handoff` y `hang_rides_media_peek`).
- Si una vista le "presta" el elemento a otra, la dueña lo esconde con opacidad
  (sigue en el árbol, así sus animaciones no se reinician) y la otra lo dibuja.

### Regla 2 — El relevo solo cuando las dos copias son idénticas

Si de verdad hay que pasar el elemento de una vista a otra (porque cada una lo
arma distinto), el relevo ocurre cuando **coinciden en todo**: posición, tamaño
de letra, alto de renglón, margen, ancho, color y **contenido**.

- Mientras viaja, interpola las métricas hacia las del destino (`grow` en
  `verse_lines`: 14,5 → 17 px, renglón 18 → 21, margen 18 → 24).
- El contenido también cuenta: el panel mostraba el verso anterior y el vistazo
  no; al cruzarse, ese renglón aparecía de golpe. Los tres lugares tienen que
  mostrar **lo mismo**.

### Regla 3 — El progreso sale del estado, no de cuándo se dibujó

Lo que se anima tiene que poder calcularse desde un valor de estado (el gesto,
cuánto lleva sonando el verso, cuánto creció el notch). Así:

- Abrir una vista a mitad de camino la muestra **en el punto correcto**, sin
  reproducir una animación que ya pasó.
- Varias vistas que muestran lo mismo quedan **sincronizadas** entre sí.

Ejemplo: el carrusel de la letra usa `Verses::since` (segundos desde que empezó
el verso). Si el verso empezó hace rato, aparece quieto; si recién cambió,
anima. `with_animation` de GPUI se usa solo para pedir cuadros, y su `t` se
combina con lo que ya pasó: `start + (1 - start) * t`.

### Regla 4 — No reiniciar entradas de lo que sigue a la vista

Una animación de entrada (`shown_at`, `Motion::enter`, ids de `with_animation`)
**solo** arranca de cero si el elemento venía oculto de verdad. Si algo lo manda
a cerrar y otro lo retoma en el mismo instante (abrir el notch durante el
scroll), tiene que seguir donde estaba.

- Pregunta por el valor (`amount.value(now) > 0.01`), no solo por el objetivo
  (`amount.target() == 0.0`).
- Cuidado con los ids de `with_animation`: si cambian, la animación vuelve a
  empezar. Ponles una clave que solo cambie cuando de verdad cambia el
  contenido (el índice del verso), nunca algo que cambie cada cuadro.

### Regla 5 — Lo que deja de servir se va primero

En una transición escalonada, el orden de salida importa tanto como el de
entrada:

- Lo que pierde sentido apenas empieza la transición sale **en el primer
  tramo**. Ejemplo: "Open Now playing" sobra en cuanto el vistazo empieza a
  volverse panel, así que se apaga en el primer 30 % de `grow`.
- Lo que comparten las dos vistas (título, controles, letra) se queda hasta el
  relevo.
- Lo que solo existe en el destino (volumen, salida de audio, fuentes) entra al
  final, cuando ya hay espacio.

Usa `segment(t, inicio, duración)` para darle a cada parte su propio tramo
dentro del mismo progreso.

### Regla 6 — Las formas crecen o se achican en una sola dirección

El alto o el ancho total de la pill no puede bajar y volver a subir en una
transición que solo abre (o solo cierra).

- Si dos piezas cambian a la vez (la letra que cuelga cede lo que baja el
  vistazo), sus alturas tienen que sumarse de forma pareja. Con dos `Tween`
  de duraciones distintas (300 y 240 ms) la suma hace una "U".
- Cuando dos cosas se reparten un espacio, derívalas del **mismo** progreso,
  o haz que una se calcule a partir de la otra.
- Los anchos de lo que cuelga del tab deberían coincidir (hoy todo usa
  `PANEL_W` = 440), así el contorno no salta al pasar de uno a otro.

### Regla 7 — Recorta los contenedores, no los renglones

- Un elemento que crece (alto animado) **no** lleva `overflow_hidden`: al
  principio de la animación es más chico que su contenido y lo corta. El verso
  de dos líneas se cortaba mientras subía.
- El recorte va en el bloque que contiene toda la animación.
- Cuidado con `flex()` alrededor de texto: en una fila flex, GPUI no parte el
  texto en renglones y se sale por los lados. Para centrar un solo carácter
  (la "♪") sí sirve; para un verso, usa `text_center()` sin flex.

### Regla 8 — Revisa los cuadros intermedios, no solo el principio y el final

Casi todos los errores de Ahora suena estaban **a mitad de camino**: dos
letras corridas, controles que desaparecen, un renglón cortado, un pie que
tarda en irse. Ver el estado inicial y el final no basta (sección 6).

---

## 3. Cómo trabajar una transición nueva

Sigue estos pasos en orden. No empieces a codificar sin el paso 3.

1. **Lista los elementos de cada vista.** Para Ahora suena: título, artista,
   barra de tiempo, controles, letra, pie, volumen, salida de audio, fondo de
   carátula.
2. **Clasifica cada uno:**
   - *Compartido* (está en origen y destino): necesita un dueño por tramo y un
     relevo con métricas idénticas.
   - *Solo origen:* sale, ¿en qué tramo?
   - *Solo destino:* entra, ¿en qué tramo?
3. **Escribe la tabla de la coreografía** con el progreso (0 → 1) en columnas:

   | Elemento | 0 – 0,3 | 0,3 – 0,6 | 0,6 – 1 | Dueño |
   |---|---|---|---|---|
   | Pie "Open …" | se apaga | — | — | vistazo |
   | Título y controles | se corren hacia el panel | ← | relevo al aterrizar | vistazo → panel |
   | Letra | crece y se corre | ← | relevo con métricas iguales | vistazo → panel |
   | Volumen, salida | — | — | entra | panel |

4. **Elige la fuente del progreso** (regla 3): el gesto, un `Tween`, el tiempo
   de la pista, cuánto creció el notch. Si hay gesto, el gesto manda y el
   `Tween` solo termina el recorrido al soltar.
5. **Implementa con las piezas que ya existen** (sección 4). No inventes otra
   curva ni otro reloj si ya hay uno.
6. **Deja escrito el contrato** en doc comments: quién dibuja qué en cada tramo
   y por qué. Los comentarios de `media_peek_morph`, `media_handoff` y
   `hang_rides_media_peek` son el modelo.
7. **Compila una vez** (`scripts/pill-dev-mac.sh` o `scripts/pill-dev.ps1`) y
   pide revisarla con la lista de la sección 6. Antes de compilar, mira que no
   haya otra compilación corriendo (`pgrep -f "pill-dev|cargo build"`): puede
   haber otro agente trabajando en el mismo árbol.

---

## 4. Las piezas del código

### Tiempo y curvas — `src/anim.rs`

| Pieza | Para qué |
|---|---|
| `Tween` | Un valor que va de `from` a `to` con duración y curva. `set` parte del valor **actual** (interrumpible); `snap` salta sin animar; `target()` dice hacia dónde va; `is_running` sirve para pedir cuadros. `Tween::new` aplica `SLOW`; `Tween::hover`, no |
| `segment(t, inicio, duración)` | Saca de un progreso 0 → 1 el tramo de una parte. Es la base de la coreografía |
| `lerp(a, b, t)` | Interpolar métricas (tamaños, posiciones, alturas) |
| `ease_island` | Abrir y cerrar la isla, con un leve sobrepaso |
| `ease_smooth_out` | Lo que entra y frena |
| `ease_back_out` | Rebote corto |
| `cubic_bezier` | Una curva CSS a medida, si de verdad hace falta |

En GPUI también están `gpui::ease_in_out` y `gpui::ease_out_quint()`.

### Entradas escalonadas — `src/peeks.rs`

- `Motion { t }` con `enter(i)`: cada parte del vistazo entra en su tramo,
  separadas por 0,055.
- `shown_at`: el reloj de esa entrada. Respeta la regla 4.

### La transformación vistazo → panel — `src/peeks.rs` y `src/main.rs`

- `media_peek_follow(amount)`: el vistazo sigue al scroll, sin animar.
- `media_peek_settle(open)`: al soltar, termina el recorrido o se recoge.
- `apply_scroll_pull` / `settle_scroll` / `ease_scroll` (en `main.rs`): el
  gesto de 0 a 2, suavizado al ritmo de la pantalla, y el cierre hacia donde
  iba el último movimiento.
- `media_peek_morph()`: si el vistazo se está transformando en el panel, y
  cuánto (`fade`, `grow`).
- `media_handoff()`: qué ya le pasó el vistazo al panel (`landed`, `lyric`).
- `MediaPanel::morph_targets(timeline)`: dónde quedan, en el panel, el título,
  los controles y el verso. El vistazo calcula su desplazamiento hacia ahí
  (`shift`).
- `MediaPanel::set_handoff`: el panel esconde lo que todavía dibuja el vistazo.

### La letra — `src/hang.rs`

- `verses(lines, position) -> Verses`: el verso que suena, sus vecinos y
  `since` (cuánto lleva).
- `verse_carousel(id, &verses, VerseLook)`: el carrusel compartido por la letra
  que cuelga, el vistazo y el panel. Cada lugar le pasa su `VerseLook`.
- `verse_lines(side, id, grow)`: la versión de la pill; `grow` la lleva a las
  métricas del panel.
- `hang_rides_media_peek(now)` (en `peeks.rs`): mientras baja el vistazo, la
  letra que cuelga viaja hasta el lugar de los versos del vistazo, y él no
  dibuja la suya.

### Otras piezas de movimiento

- `src/glide.rs` — un indicador que se desliza entre opciones (la selección del
  menú de Ajustes, los segmented).
- `src/glass_pieces.rs` — `Morph`: un vidrio de Liquid Glass que nace de un
  botón y se estira hasta su panel.
- `src/hover.rs` — `Hover`/`HoverFx`: hover y press con su propio `Tween`.
- `crate::code::style::system_reduced_motion()` — si el sistema pide reducir el
  movimiento.

### Pedir cuadros

Mientras algo anima, la pill tiene que seguir dibujando. Si usas un `Tween`
nuevo, súmalo a la función que dice si hay algo animándose (por ejemplo,
`tool_peek_animating` o la lista de `is_running` de `main.rs`). Con
`with_animation`, GPUI pide los cuadros solo.

---

## 5. El caso de Ahora suena: errores y arreglos

Cada error es un ejemplo de qué buscar. Aplica lo mismo en otras transiciones.

| Lo que se veía | Causa | Arreglo | Regla |
|---|---|---|---|
| Dos letras: una arriba (la que cuelga) y otra abajo (la del vistazo), que se intercambiaban | Cada vista dibujaba su propia letra y se cruzaban con un fundido | La letra que cuelga **viaja** hasta el lugar de la del vistazo; el vistazo esconde la suya (`hang_rides_media_peek`) | 1 |
| La forma se achicaba y volvía a crecer | La letra cedía alto con un `Tween` de 300 ms y el vistazo crecía con otro de 240 ms | Una sola letra que se mueve; las alturas se derivan del mismo progreso | 6 |
| Letra doble y corrida al llegar al panel | El vistazo usaba 14,5 px y margen 18; el panel, 17 px y margen 24 | `grow` interpola las métricas del vistazo hasta las del panel | 2 |
| Un renglón aparecía de golpe al final | El panel mostraba el verso anterior; el vistazo y la letra que cuelga, no | Los tres muestran lo mismo: el verso actual y el siguiente | 2 |
| "Open Now playing" se iba muy tarde | Se apagaba al ritmo del contenido del panel | Sale en el primer 30 % de `grow` | 5 |
| La letra cambiaba "a saltos" | El verso nuevo aparecía en su lugar con una entrada corta | Carrusel: todo sube un renglón y el siguiente crece hasta el lugar del actual | 3 |
| La "♪" pegada a la izquierda | `text_center` no centraba un carácter solo | Solo la "♪" va en una fila flex centrada | 7 |
| La letra que cuelga cortada por los lados | La fila flex del arreglo anterior no partía los versos en renglones | El texto normal vuelve a `text_center` sin flex | 7 |
| Un verso de dos líneas cortado mientras subía | El renglón que crece tenía `overflow_hidden` y empezaba con 18 px | Sin recorte en los renglones; solo en el bloque | 7 |
| Controles y letra que desaparecían a mitad del scroll | Abrir el notch mandaba a cerrar el vistazo y el scroll lo retomaba como si fuera nuevo, reiniciando `shown_at` | Solo se reinicia si el vistazo no estaba a la vista | 4 |

---

## 6. Cómo revisar una transición

Haz cada prueba mirando la pantalla, no el código. Si no puedes verla, dile al
usuario exactamente qué mirar en cada paso.

1. **Lento, de ida.** Recorre la transición muy despacio: con el scroll, o
   subiendo por un rato `SLOW` en `anim.rs` (hoy 1,5; hay que recompilar y
   después dejarlo como estaba). ¿Hay algún cuadro con dos copias, un hueco, un
   salto o algo cortado?
2. **De ida y vuelta.** Avanza hasta la mitad y retrocede, varias veces.
   ¿Retoma desde donde estaba? ¿Se reinicia algo?
3. **Interrumpir.** Cambia de idea en el peor momento: suelta a mitad de camino,
   mueve el cursor afuera durante la animación, abre otra herramienta.
4. **Entrar a mitad.** Abre la vista cuando el estado ya está avanzado (un
   verso que empezó hace rato, una pista a la mitad). ¿Aparece en su lugar o
   reproduce una animación que ya pasó?
5. **Contenido extremo.** Un verso largo (dos líneas), uno vacío ("♪"), un
   título largo, sin carátula, sin letra, en pausa.
6. **Todas las variantes:** acoplada arriba y en un costado, fundida con el
   notch, tema claro y oscuro, los tres estilos de ventana (Atic, Material 3
   Expressive, Liquid Glass) y "Reducir movimiento" del sistema.
7. **Fíjate en los bordes:** ¿el contorno de la pill solo crece (o solo se
   achica)? ¿Algo se sale de la forma o queda debajo del notch?

Si encuentras un error, ubícalo en la tabla de la sección 5 o agrégale una fila.

---

## 7. Errores típicos

- Animar dos copias del mismo elemento con un fundido cruzado "para que se vea
  suave". Se ve doble.
- Crear un `Tween` nuevo para algo que debería salir del mismo progreso que su
  vecino.
- Reiniciar `shown_at` o cambiar el id de una animación sin necesidad.
- Poner `overflow_hidden` en algo cuyo alto se anima.
- Envolver texto que puede partirse en una fila flex.
- Probar solo el estado inicial y el final.
- Cambiar el diseño visual (bordes, contraste) sin que el usuario lo vea antes:
  propón y muéstralo con `tweaks.json` (`src/tweak.rs`) para ajustarlo en vivo.
- Compilar encima de otra compilación (otro agente puede estar trabajando en el
  mismo árbol).

---

## Relacionado

- `docs/DISENO_PILL.md` — lineamientos de diseño de la pill (sección
  "Transiciones fluidas").
- `AGENTS.md` — cómo compilar, probar y validar la pill.
- WWDC 2018, "Designing Fluid Interfaces" (Apple).
- Material Design, "Motion" → *container transform* y *choreography*.
