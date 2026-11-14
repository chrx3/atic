---
name: m3-expressive-gpui
description: Cómo diseñar e implementar UI Material 3 Expressive en Atic (GPUI) y en gpui-m3 sin perder rendimiento. Úsala antes de diseñar o cambiar cualquier UI de Atic (Atic Code, Ajustes, Reuniones, paneles de la pill) o un componente de ../gpui-m3, y siempre que el cambio toque animaciones, movimiento, resortes, formas o morphing, color y contenedores, tipografía, indicadores de carga o progreso, estados vacíos, de carga o de error. También al revisar por qué una ventana se redibuja sin parar.
---

# Material 3 Expressive en GPUI (Atic + gpui-m3)

El único estilo de Atic es **Material 3 Expressive** (`appearance.rs:115`,
`ONLY_EXPRESSIVE = true`). Los estilos Atic/Formal y Liquid Glass ya no se usan:
no agregues ramas `!m3`, `flat_*` ni `Style::Glass` nuevas, y si tocas una
función que las tiene, no las amplíes.

Los componentes salen de `../gpui-m3` (repo aparte; ver `AGENTS.md`). Escribe en
español de Chile con tuteo en Atic; en gpui-m3 el código, la API y los doc
comments van en inglés, y el changelog en los dos idiomas.

Lee también:
- `.claude/skills/transiciones-fluidas/SKILL.md` antes de tocar una transición
  entre vistas (dueño único, relevo, progreso desde el estado, cuadros intermedios).
- `referencia-gpui-m3.md` (en esta carpeta): inventario de gpui-m3 con
  archivo:línea y qué usar para cada caso.

---

## 1. Antes de animar: elige el momento clave

Expressive no es "más animación": es usar **forma, color, tamaño, tipografía y
movimiento** para marcar qué importa y dar respuesta física a cada acción. La
expresividad va en los **momentos clave** (la acción principal, un cambio de
estado, algo que terminó), no en todo a la vez.

- **Una cosa importante por vista**, y a lo más **un héroe** (el elemento más
  grande, enfatizado o con forma especial).
- **Jerarquía de atención**, en este orden: tamaño y tipografía → forma → color
  del contenedor → movimiento. Si lo primero ya marca la jerarquía, no animes.
- **Prueba:** si quito la animación, ¿se entiende igual? Si no, el problema es
  de jerarquía, no de movimiento.
- A lo más **2–3 resortes espaciales visibles a la vez**; si hay más, escalona
  (`entrance(.., delay)`, `Menu::stagger`).

**Cuándo no animar:**
- Listas densas o que crecen con el streaming (mensajes, salida de
  herramientas, logs): que el contenido aparezca, sin resorte por fila.
- Valores que cambian con cada token o cada tick (contadores, tiempo, largo del
  texto): se actualizan sin resorte.
- Cambios de layout que el usuario no provocó (algo que se reacomoda solo).
- Datos que hay que leer al instante (un error, un resultado, un número).
- Con movimiento reducido: estado final directo (sección 5, regla 6).

---

## 2. Qué es M3 Expressive (2025)

### 2.1 Movimiento: física de resortes (motion physics)

M3 Expressive cambió las curvas con duración fija por **resortes** (rigidez +
amortiguamiento). Un resorte se puede interrumpir y redirigir sin saltos:
conserva la velocidad.

Hay dos tipos y tres velocidades:

| Tipo | Para qué | Rebote |
|---|---|---|
| **Spatial** (espacial) | Posición, tamaño, rotación, **forma** y radio | Sí, puede pasarse del objetivo |
| **Effects** (efectos) | Color, opacidad, desenfoque | No desde reposo (crítico); si cambia el objetivo a mitad, puede pasarse un poco |

| Velocidad | Cuándo |
|---|---|
| **Fast** | Componentes chicos: botones, switches, chips, íconos, el radio al presionar |
| **Default** | Partes de la pantalla: menús, tarjetas que se expanden, hojas, navegación |
| **Slow** | Pantalla completa o recorridos largos: cambiar de vista, un panel grande |

Valores en gpui-m3 (`src/motion.rs:25-49`), los oficiales de
`MotionScheme.expressive()` de Compose (masa 1): son la descomposición
spatial/effects × fast/default/slow de los tokens de M3 Expressive. **No los
"corrijas"** hacia el esquema *standard*.

| Constante | Rigidez | Amortiguamiento | Uso |
|---|---|---|---|
| `Spring::SPATIAL_FAST` | 800 | 0,6 (rebota ~9,5 %) | Radio al presionar, íconos que saltan, un punto que cambia de forma |
| `Spring::SPATIAL` | 380 | 0,8 | Alturas que se abren, entradas, desplazamientos |
| `Spring::SPATIAL_SLOW` | 200 | 0,8 | Paneles grandes, cambios de vista |
| `Spring::EFFECTS_FAST` | 3800 | 1,0 | Salidas, fundidos cortos (`presence`) |
| `Spring::EFFECTS` | 1600 | 1,0 | Colores de fondo y texto, opacidad |
| `Spring::EFFECTS_SLOW` | 800 | 1,0 | Fundidos largos |

**No existe** en gpui-m3 el esquema *standard* de Compose (`MotionScheme.standard()`:
espacial 1400/0,9, 700/0,9, 300/0,9; efectos iguales). En Atic no hace falta
(todo es Expressive); si se agrega, que sea un `Spring::STANDARD_*` aditivo.

Reglas de uso:
- Nunca uses un resorte espacial para color u opacidad (el rebote saca el color
  de rango o hace parpadear). Para eso, `EFFECTS*` (`animate_color` además
  limita los canales a [0, 1]).
- Las salidas son más rápidas que las entradas (`presence` usa `EFFECTS_FAST`).
- No inventes duraciones ni curvas nuevas: si de verdad necesitas una curva
  (algo guiado por gesto), usa `motion::cubic_bezier` o `anim.rs`, y anótalo.

### 2.2 Formas y morphing

- **La biblioteca de 35 formas** (Circle, Square, Slanted, Arch, Fan, Arrow,
  SemiCircle, Oval, Pill, Triangle, Diamond, ClamShell, Pentagon, Gem, Sunny,
  VerySunny, Cookie4/6/7/9/12, Ghostish, Clover4/8, Burst, SoftBurst, Boom,
  SoftBoom, Flower, Puffy, PuffyDiamond, PixelCircle, PixelTriangle, Bun,
  Heart). gpui-m3 las tiene todas en `ShapeName` (`src/shapes.rs:14`, 38 con
  Blob, Blob2 y Pebble). Todas tienen los mismos 120 puntos, así que cualquiera
  se transforma en cualquiera (`Morph`, `src/shapes.rs:575`).
- **Cambio de forma por estado**: al presionar, un botón redondo se aplasta
  hacia un radio menor (`Button`/`IconButton` ya lo hacen con
  `SPATIAL_FAST`); un botón de alternar elegido pasa de redondo a cuadrado (o
  al revés); en un grupo conectado el elegido se vuelve píldora; el indicador
  de carga va cambiando de forma mientras gira.
- **Contraste de formas**: una forma especial (Cookie, SoftBurst, Clover) en
  un elemento entre muchos rectángulos redondeados marca el foco: el avatar,
  la marca de "hecho", el encabezado de un permiso. Úsalas con medida: una o
  dos por vista.
- **Escala de esquinas** (`theme::corner`, `src/theme.rs:48`): NONE 0, XS 4,
  S 8, M 12, L 16, LI 20, XL 28, XLI 32, XXL 48, FULL. Expressive agregó los
  "increased" (LI, XLI) y XXL. No escribas radios sueltos (`px(26.)`): usa la
  escala o un token de `code/style.rs`.

### 2.3 Tipografía enfática

M3 Expressive agrega **estilos enfatizados** (más peso) a cada nivel de la
escala, para titulares, la acción o el dato importante, y el momento "héroe".
En gpui-m3: `theme::type_scale` (`src/theme.rs:606`): `DISPLAY_*`, `HEADLINE_*`,
`TITLE_*`, `BODY_*`, `LABEL_*`, cada uno con `weight` y `emphasized`, y
`TypeScaleExt::type_style(..)` / `type_emphasized(..)` (`src/theme.rs:644`).
Importa con `use gpui_m3::theme::type_scale::{self, TypeScaleExt};`.

- Usa la escala, no `text_size(px(12.5)).font_weight(SEMIBOLD)` sueltos. Hoy
  Atic casi no la usa: al tocar una vista, pásala a la escala.
- Énfasis = jerarquía: un título de sección enfatizado, el cuerpo normal, lo
  secundario en `on_surface_variant` (no en un gris más claro que no se lee).

### 2.4 Color y contenedores para la jerarquía

- Los roles de `Scheme` (`src/theme.rs:66`): `primary`, `secondary`,
  `tertiary` y sus `*_container`/`on_*_container`; las superficies
  `surface_container_lowest … highest`; `error_container`; `inverse_*`.
- **Contenedor = importancia y rol**: lo principal en `primary` (lleno) o
  `primary_container`; lo seleccionado en `secondary_container`; un acento o lo
  que pide atención (un permiso, algo nuevo) en `tertiary_container`; lo
  fallido en `error_container` (no solo un ícono rojo). El contenedor también
  agrupa: lo que comparte rol comparte contenedor.
- Las superficies suben de nivel con `surface_container_*`, no con sombras ni
  con `opacity(0.14)` sobre un color.
- Siempre el par: fondo `*_container` con texto `on_*_container`.
- En Atic, los tokens de las ventanas salen de `code/style.rs` y
  `crate::theme` (`expressive()`, `panel_bg`); en componentes de gpui-m3,
  `Theme::of(cx)`.

### 2.5 Componentes nuevos de Expressive

| Componente | En gpui-m3 | Para qué |
|---|---|---|
| **Button groups** (estándar y conectados) | `ButtonGroup` | Opciones relacionadas; el presionado se ensancha, el elegido se vuelve píldora |
| **Split button** | `SplitButton` (`chip.rs:405`) | Acción principal + menú de variantes ("Permitir ▾") |
| **FAB menu** | `FabMenu` | Un FAB que se abre en varias acciones escalonadas |
| **Toolbars** (flotante y anclada, *vibrant*) | `Toolbar` | Acciones contextuales flotando sobre el contenido (un reproductor, una selección) |
| **Loading indicator** | `LoadingIndicator` (`contained`) | Esperas cortas (< ~5 s) sin progreso medible; formas que cambian |
| **Progreso ondulado** (lineal y circular) | `WavyProgress`, `WavyCircularProgress` | Progreso medible que debe notarse: descargas, transcripción, grabación |
| **Carousel** | `Carousel` (5 layouts) | Contenido para recorrer (onboarding, medios) |
| **Botones XS–XL, anchos de icon button** | `M3ButtonSize`, `IconButtonWidth` | Tamaño como jerarquía: la acción clave más grande |
| **App bars flexibles** | `TopAppBar` (`collapse`) | Títulos grandes que se pliegan al hacer scroll |
| **Otros** | `SearchBar/SearchView`, `BottomSheet/SideSheet`, `RichTooltip`, `Slider` Expressive | |

Para progreso de fondo (que no debe robar atención), `LinearProgress`: no es
continuo, solo anima el relleno con resorte cuando cambia el valor.

Falta en gpui-m3 (a la 0.2.0, noviembre 2026; revisa con `grep` antes): estado vacío,
indicador de página, marca de estado que se transforme (pendiente → corriendo
→ hecho), texto con brillo (*shimmer*), visor JSON, `Toolbar` compacta o con
contenido propio, `Fab` con color/ocupado. Si lo necesitas, propónlo en
gpui-m3 (sección 6) en vez de hacerlo a mano en Atic.

---

## 3. Escritorio (macOS/Windows)

M3 se pensó para pantallas táctiles; Atic es de escritorio. Adapta:

- **Densidad:** en vistas densas (Atic Code, Ajustes) baja un nivel la
  tipografía respecto de la guía móvil. `DISPLAY_*` solo en una vista héroe
  (bienvenida, estado vacío grande).
- **Mouse:** el hover (la capa de estado) es la primera señal de que algo se
  puede presionar; el morph al presionar es un refuerzo, no la única pista.
- **Teclado:** foco visible con `interaction::focus_ring` (`interaction.rs:258`)
  y orden de Tab lógico. El movimiento nunca mueve ni esconde el elemento con
  foco.
- **Convenciones de la plataforma por encima de M3:** atajos, menús
  contextuales, orden de los botones en diálogos, scroll.
- **Patrones táctiles:** carousel y gestos solo para contenido que se recorre,
  y siempre con controles visibles (flechas, botones) para mouse y teclado.
- **Accesibilidad mínima:**
  - Contraste 4,5:1 para texto y 3:1 para íconos y bordes de controles. Usa
    los pares `*_container`/`on_*_container`; cuidado con `on_surface_variant`
    sobre superficies altas y con `opacity(0.4)` sobre texto ya tenue. Revisa
    en claro y oscuro.
  - Áreas de clic: ~28 px como mínimo, ~36 px cómodo.

---

## 4. Cómo se hace en gpui-m3 y en Atic

### 4.1 Las piezas de movimiento (`gpui-m3/src/motion.rs`)

| Pieza | Línea | Qué hace | Cuándo |
|---|---|---|---|
| `animate`, `animate_f32`, `animate_px`, `animate_color` | 387, 541, 553, 565 | Sigue un objetivo con resorte; pide cuadros **solo mientras se mueve** | Un valor que depende de un estado (abierto, elegido, presionado) |
| `animate_from` | 399 | Igual, pero el primer cuadro parte de `from` | Entrada con resorte de un valor |
| `animate_on_activate` | 473 | Al pasar `active` de false a true, salta a `from` y vuelve | Un ícono que "salta" al elegirse |
| `entrance(id, spring, delay)` | 515 | Progreso 0 → 1 desde que apareció | Listas y menús escalonados |
| `entrance_since(at, spring, delay)` | — | Igual, desde un instante tuyo: no se repite al volver a dibujar el elemento | Partes que llegan a una lista virtual (`Enter::since` en Atic) |
| `presence(id, show)` | 708 | Mantiene algo dibujado mientras sale | Banners, menús, ofertas que se van |
| `expand(id, open, measured, spring)` | 776 | Alto de 0 a natural con resorte | Tarjetas y secciones que se abren |
| `replay`, `play_on_rise` | 629, 663 | Efecto de una vez al cambiar una generación o un borde | Lanzar el botón enviar, respirar una vez |
| `continuous_at(fps)` | 271 | Reloj **continuo**: bajo 60 fps, temporizador + `cx.notify` a la vista actual; ≥ 60, `request_animation_frame`; ≤ 4 fps con la ventana atrás; nada con movimiento reducido | Solo indicadores que giran |
| `ambient_clock` | 243 | Reloj que se congela con la ventana atrás | Formas que respiran |
| `Shape::ambient(false)` | — | La forma se queda en su pose y no pide cuadros; con `true` sigue sin salto | Formas decorativas en reposo (el inicio de Atic Code) |
| `request_animation_frame(window)` | — | `window.request_animation_frame()` contado en `take_frame_requests` | Animaciones propias hechas a mano |
| `pulse(period)` | 617 | Opacidad 1 → 0,45 → 1 a 30 fps | Texto "capturando…" (con cuidado: es continuo) |
| `tick_delay(fps)` | 173 | Espera hasta el próximo tick de la grilla compartida | Redibujos con ritmo (streaming, relojes) |
| `MotionSettings { reduced, time_scale }` | 93 | Global: `reduced` apaga todo; `time_scale` > 1 lo hace lento | Accesibilidad y revisión |

(Todas reciben además `window, cx` al final.) `animate*`, las entradas y los
ripples piden el siguiente cuadro mientras siguen activos, sin límite propio de
fps: la frecuencia depende del ciclo de cuadros de GPUI.

Muchos componentes visuales son `RenderOnce` y guardan su estado de animación
por `id` (los de texto, `TextField`, `TextArea`, `CodeEditor`, son entidades con
`Render`): **cada id debe ser único en la vista**. Con `frame-stats` (Atic lo
activa en `apps/atic/Cargo.toml:98`) un id duplicado que se pelea el objetivo
avisa en el log ("retargeted twice within a frame"). Ese estado vive mientras el
elemento se dibuje en cuadros seguidos (`motion.rs:453`): **un id estable no
conserva la animación si el elemento se desmonta** y vuelve.

### 4.2 Patrones listos

- **Cambio de forma por estado** (seleccionado, hecho, en foco):
  `Shape::new(A).breathe_on_hover(id, B).hovered(estado)` hace el morph A → B
  con `SPATIAL` una sola vez al cambiar `estado` (pese al nombre, sirve para
  cualquier booleano). `MorphDot` para un punto cuadrado/círculo/rombo.
- **Radio que responde al presionar**: ya viene en `Button`, `IconButton`,
  `Chip`, `ButtonGroup`. Si haces una fila a mano (`src/interaction.rs:47`,
  `:52`, `:117`):

  ```rust
  let state = interaction::use_interaction(&id, window, cx);
  let pressed = state.read(cx).pressed;
  let target_radius: f32 = if pressed { 8. } else { 16. };
  let radius = motion::animate_f32("row-radius", Spring::SPATIAL_FAST, target_radius, window, cx);
  interaction::interactive(div().id(id.clone()), &state, enabled, ripple)
      .rounded(px(radius))
      .child(interaction::state_layer(&id, &state, radii, content, window, cx)) // primer hijo
      .child(..)
  ```

  Para un `Pixels`, `animate_px`. El id del resorte, derivado del de la fila
  (`motion::key(&id, "radius")`) si hay varias.
- **Entradas en Atic Code**: `code/enter.rs` (`Enter`) sobre `entrance`/`replay`.
- **Menús**: `Menu` ya escalona (`stagger`), `pop_in`/`pop_in_toward`
  (`surfaces.rs:48`, `:53`) para popovers propios. `Badge::pop(id)` para un
  contador que salta (solo `Badge`, no `Chip`).
- **Cambio de tema**: `gpui_m3::animate_theme(dark, seed, cx)` /
  `gpui_m3::theme_reveal_at(scheme, dark, origin, window, cx)` (el módulo
  `theme_motion` es privado). `animate_theme` usa un temporizador de 16 ms y
  `refresh_windows()`: es una transición global deliberada, no un patrón local
  barato.

### 4.3 En Atic

- **Toda vista raíz de ventana** que dibuje componentes de gpui-m3 llama a
  `crate::theme::scope_m3(cx)` (`apps/atic/src/theme.rs:294`) al principio de
  `render` y agrega `.children(m3_scope)` al final. Sin eso toma el esquema de
  otra ventana (blanco sobre blanco en claro).
- Tokens: `code/style.rs` (`t()`, `tokens`), `crate::theme::{expressive,
  panel_bg, font}`.
- Movimiento reducido: `crate::code::style::system_reduced_motion()` se lleva a
  gpui-m3 en `code/mod.rs:956` (`set_reduced_motion`), **solo al abrir Atic Code
  y cuando su ventana vuelve a activarse** (`code/mod.rs:1165`, `:1169`). Otras
  ventanas leen `MotionSettings::get(cx).reduced`, que puede estar
  desactualizado si Atic Code no se abrió; no llames a
  `system_reduced_motion()` en cada cuadro (en Mac es una llamada objc).
- Movimiento propio heredado (`anim.rs` `Tween`, `glide.rs`, `hover.rs`,
  `meetings/motion.rs`): no lo extiendas para UI nueva; usa los resortes de
  gpui-m3. `Tween` sigue siendo correcto para lo guiado por gesto de la pill
  (ver transiciones-fluidas).

---

## 5. Reglas de rendimiento (OBLIGATORIAS)

Cada una salió de un caso real en que Atic Code se comía un núcleo.

1. **Un aviso invalida la vista y todos sus ancestros** (`mark_view_dirty`,
   `vendor/gpui/src/window.rs:1309`). Lo que ahorra trabajo son las **vistas
   hermanas cacheadas**: Atic Code se dibuja por secciones
   (`apps/atic/src/code/section.rs`: barra, chat, panel derecho, cada una con
   `AnyView::cached`) y una sección reutiliza su layout y pintado si no cambiaron
   sus bounds, máscara, estilo de texto, las entidades que leyó, ni hubo un
   refresco global (`vendor/gpui/src/view.rs:209`). Un `cx.notify()` a
   `CodeView` rehace las tres (todas la leen); avisa solo a la sección
   (`sections.chat.update(cx, |_, cx| cx.notify())`, como `redraw_on_tick` en
   `code/mod.rs:1790`). La raíz se rehace igual: mantén liviano su render.
2. **Lo continuo va en una capa hermana.** `continuous_at` avisa a
   `window.current_view()`, la vista que lo dibuja. Los ticks de `SpinnerLayer`
   invalidan la capa y sus ancestros; las secciones hermanas pueden reutilizar
   layout y pintado mediante `.cached`, siempre que sus dependencias y claves de
   caché no hayan cambiado. Por eso en Atic Code la fila deja un hueco con
   `spinners::slot(size)` (`code/spinners.rs:41`, `pub(super)`) y `SpinnerLayer`
   (`:81`) dibuja ahí el `LoadingIndicator`. **Solo aloja el `LoadingIndicator`
   estándar**: otro continuo (ondas, formas, texto que late) necesita su propia
   capa especializada. **Nunca** pongas `LoadingIndicator`, `WavyProgress`,
   `WavyCircularProgress`, `Shape::breathe/spin/drift` ni `pulse` directo dentro
   de una sección o de una vista raíz. `WavyProgress` anima la onda **siempre**,
   aunque el valor sea fijo.
   No basta con crear una entidad hoja dentro de una sección: sus avisos también
   invalidan esa sección. En ventanas sin secciones (Ajustes, Reuniones), pon la
   animación en una capa hermana o separa explícitamente el contenido costoso en
   vistas cacheadas.
3. **Nada continuo a ritmo alto.** Los resortes de `animate*` corren mientras se
   mueven y se detienen al asentarse: eso está bien; lo prohibido es lo continuo
   a ritmo alto. El indicador va a 15 fps (4 con la ventana atrás); las formas
   ambientales a 30 (congeladas atrás); el streaming a `STREAM_FPS` = 15
   (`code/mod.rs:156`). `window.request_animation_frame()` a mano pide un cuadro
   por cada cuadro de GPUI, sin tope: solo para algo que dura una fracción de
   segundo y se corta solo (mejor `animate*`).
   - `continuous_at` comparte **un** temporizador pendiente por entidad: varios
     ritmos en una misma vista no tienen temporizadores independientes.
   - Evita `refresh_windows()` para animaciones locales: impide reutilizar las
     cachés.
   - `window.refresh()` dentro de un `canvas` (prepaint o paint) **no hace nada**:
     mientras se dibuja, GPUI lo ignora. Para pedir otro cuadro desde ahí, usa
     `gpui_m3::motion::request_animation_frame(window)`.
   - Lo decorativo continuo (formas que respiran) se detiene en reposo:
     `Shape::ambient(false)` pasado un rato o sin el cursor encima.
4. **Un resorte no se reinicia con cada token.** Caso real: el «dim» de
   `AgentPresence` (`code/view.rs:4493`) animaba la opacidad con resorte; `dim`
   cambiaba con cada texto que llegaba y pedía cuadros sin parar mientras el
   agente escribía. Ahora cambia sin resorte. Ata los resortes a estados que
   cambian pocas veces (abierto, elegido, terminado), nunca a `working()`, al
   largo del texto ni a nada que cambie con el streaming. Tampoco animes al
   terminar cada herramienta si pueden venir muchas seguidas
   (`code/tools.rs:360`).
5. **Un evento del dominio lleva su propio reloj.** `entrance`, `animate_from`,
   `replay` y `play_on_rise` guardan su instante en el estado del elemento: es
   correcto para el montaje o una interacción local. Si la animación representa
   un evento (`finished_at`, `since`) que debe seguir donde iba al volver a
   mostrarse, guarda el instante en el estado del evento y deriva la animación
   de ahí: abrir la conversación a mitad del ✓ lo muestra en su punto. No uses
   `or_insert_with(Instant::now)` en el render como reloj de un evento.
6. **Respeta el movimiento reducido.** Los helpers de gpui-m3 ya lo hacen con
   `MotionSettings::reduced`; si calculas algo a mano (`Spring::step(t)`,
   `request_animation_frame`), pregunta por `MotionSettings::get(cx).reduced` y
   muestra el estado final.
7. **Los ids de animación son estables.** Si cambian, el resorte o la entrada
   arranca de nuevo; si se repiten, dos elementos se pelean el objetivo y la
   ventana no para de dibujar.

Antes de entregar algo animado, contesta: ¿qué vista se redibuja (y qué
ancestros)?, ¿a cuántos fps?, ¿cuándo se detiene?, ¿qué pasa con la ventana
atrás y con movimiento reducido?

---

## 6. Cuándo va en gpui-m3 y cuándo en Atic

- Si es un componente o patrón de M3 (estado vacío, marca de estado, toolbar
  compacta), va en gpui-m3 con API en inglés, estilo builder
  (`Foo::new(id, ..).bar(..)`), en el gallery (`examples/gallery`) y en
  `changelog/changelog.json` (es + en). Ver `changelog/README.md`.
- **Compatibilidad para 0.2.x**: agregar métodos, tipos o constantes es seguro.
  No es seguro: cambiar firmas, quitar o renombrar, cambiar valores por defecto
  visibles, ni agregar variantes a enums públicos sin `#[non_exhaustive]`
  (`Tone`, `ButtonVariant`, `ShapeName`… rompen a quien hace `match`). Eso va a
  0.3.
- Si cambias gpui-m3, sube los dos repos (ver `AGENTS.md`).

---

## 7. Cómo revisar

1. **Claro y oscuro.** Opciones no elegidas, switches apagados y texto
   secundario tienen que leerse (sección 3, accesibilidad).
2. **Cuadros intermedios** (transiciones-fluidas, sección 6): lento, de ida y
   vuelta, interrumpiendo, entrando a mitad. Para ver un resorte lento: el
   gallery de gpui-m3 tiene **Slow ×5**; en Atic, `MotionSettings.time_scale`
   (hay que compilar y dejarlo como estaba).
3. **Movimiento reducido** del sistema: todo queda en su estado final, sin
   saltos raros.
4. **Teclado:** recorre la vista con Tab; el foco se ve y no salta con las
   animaciones.
5. **Costo**, con la ventana quieta y con un agente escribiendo:
   - `top -pid $(pgrep -f "Atic Pill.app/Contents/MacOS/atic-pill")` (Mac;
     cuidado, puede estar la instalada en `/Applications/Atic.app`). En reposo
     tiene que bajar a ~0 %.
   - El log (`~/Library/Application Support/com.ciat.atic/logs/pill.<fecha>.log`,
     en Windows `%APPDATA%\ciat\atic\data\logs\`): el aviso **«Atic Code se
     redibuja sin parar»** (`code/mod.rs:2323`) trae `cuadros_por_s`,
     `hilo_ocupado` y `pedidos=`, la lista `archivo:línea ×n` de quién pidió
     cuadros (`motion::take_frame_requests`). Ahí sale el culpable.
   - `PILL_FPS=1` (`meter.rs`) muestra cuadros por segundo y el peor cuadro.
6. **Capturas sin tocar la pill**: busca el id de la ventana (CGWindowList) y
   `screencapture -x -o -l <id> archivo.png`. Para Atic Code aislado:
   `CODE_ALONE=1`, `CODE_DEMO=1`.

No digas que algo "se siente vivo" o "no cuesta" sin haberlo visto y medido.
Compila una sola vez y solo si nadie más está compilando (`AGENTS.md`).
