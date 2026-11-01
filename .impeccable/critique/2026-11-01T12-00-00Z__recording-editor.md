---
target: recording-editor
total_score: 27
p0_count: 0
p1_count: 5
p2_count: 9
p3_count: 6
timestamp: 2026-11-01T12-00-00Z
slug: recording-editor
---
# Critique: Editor de grabaciones (`apps/atic/src/recording_editor/`)

Method: single-agent. Capturas n1, n4, n5, n6, p2 (interfaz actual con pestañas) y ed4, ed9, ed10, ed11 (**de antes de las pestañas**: su barra lateral es la vieja de una sola columna; de ellas solo valen la línea de tiempo, la tarjeta «Texto elegido» y el diálogo de exportar). Lo de «Lo elegido» sobre las pestañas sale del código (`style_panel.rs:1750`), no de una captura. Íconos faltantes (Subtítulos, −, ajustar) excluidos, como se pidió. No se compiló ni se corrió nada; los hallazgos de movimiento y foco salen de leer el código, y se marcan «verificar» donde depende de cómo se comporta GPUI.

## Design Health Score

| # | Heurística | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Cabezal, progreso y botón Exportar con estado son buenos; el autoguardado es mudo, el audio silencioso se ve como un hilo plano y el aviso mueve toda la ventana |
| 2 | Match System / Real World | 3 | Cortar, Zoom, Velocidad, Difuminar son lenguaje de editor; «queda 01:40» mientras vas en 0:13 en realidad es la duración de salida |
| 3 | User Control and Freedom | 3 | Deshacer/rehacer, restablecer con «¿Seguro?», cancelar exportación, Esc por capas; borrar y «Cortar silencios» no ofrecen «Deshacer» en el aviso |
| 4 | Consistency and Standards | 2 | Cinco patrones distintos de «elegido» (relleno, aro, borde+relleno, punto, relleno de pestaña), 7 tamaños de texto, botones fantasma vs. rellenos, una fila de imagen que se sale del margen |
| 5 | Error Prevention | 3 | Confirmación al restablecer, nombre inválido en rojo, hex inválido en rojo; faltan avisos antes de «Cortar silencios» (quita mucho de una vez) |
| 6 | Recognition Rather Than Recall | 2 | 7 pestañas solo con ícono, botones sin forma de botón, atajos solo en globitos y en una nota de la **última** pestaña |
| 7 | Flexibility and Efficiency | 3 | S/Z/T/V/B/A, ⌘Z/⌘E, flechas y doble clic en sliders, ←/→ en pestañas; chips, interruptores y botones no se alcanzan con Tab |
| 8 | Aesthetic and Minimalist Design | 3 | La vista previa y la línea de tiempo están limpias y bien proporcionadas; la barra lateral es densa, con notas largas en gris tenue y se corta abajo sin pista |
| 9 | Error Recovery | 3 | Exportación fallida con «Reintentar» y «Cambiar ajustes»; error de guardado persistente; la imagen inválida avisa. Aviso de éxito sin acción |
| 10 | Help and Documentation | 2 | Notas en contexto (bien), pero 11,5 px en `faint`; pistas vacías sin ayuda; cero guía de atajos a la vista |
| | **Total** | **27/40** | **Good (alto): sólido de base, falta el último 15 %** |

## Anti-Patterns Verdict

**LLM review**: No parece plantilla de IA. La paleta sale de `theme`, la jerarquía del layout (vista previa grande, transporte al centro, línea de tiempo ancha) es la de un editor de verdad, y no hay gradientes de adorno, glassmorphism por reflejo ni íconos en círculos de color. Lo reflexivo que sí aparece: títulos de sección en mayúsculas con tracking (`section()`, mod.rs:1460; es la convención de la casa), y la grilla de muestras de fondo, que se parece a la de cualquier editor de capturas (esperable: el usuario la pidió «como OpenScreen»).

**Deterministic scan**: no se corrió (el detector no lee `.rs`). Revisión manual del código: sin `border-left` de acento ni texto con gradiente; los contrastes sospechosos (`faint` a 11–11,5 px sobre `control`) están en el hallazgo 7; ningún componente consulta «Reducir movimiento» (hallazgo 18).

## Overall Impression

La base es buena y se nota que alguien midió: el slider tiene teclado, doble clic y un solo paso de deshacer por arrastre; la línea de tiempo tiene imán, zoom y selección con asas; el hover y el apretado ya se funden con `hover.rs` (120/220 ms). Lo que separa esto de «Screen Studio» no es el diseño estático sino que **el editor casi no se mueve cuando cambia de estado**: la pestaña, el chip, el interruptor, el diálogo y el aviso aparecen o saltan, y dos de esos saltos (aviso y «Lo elegido») además **reacomodan el layout y reescalan la vista previa**. Lo segundo más caro es el foco: el aro de la pestaña aparece al hacer clic y se lee como «seleccionada», mientras la mitad de los controles no se pueden enfocar. Con un helper de foco, un helper de «elegido que se desliza» y dos entradas (aviso y diálogo) se llega al nivel de producto final sin tocar el diseño visual.

## What's Working

1. **Vista previa limpia y honesta.** El cuadro lo compone el mismo `Composer` que exporta, centrado, con 18 px de aire, sin marcos ni badges encima salvo el de «Ver cómo queda». Es exactamente la referencia de Screen Studio: nada compite con el video.
2. **Slider bien resuelto en lo difícil.** Arrastre que sigue fuera de la pista, clic que salta, doble clic que restablece, ←/→/Inicio/Fin/⇧, un solo paso de deshacer por gesto (`style_release`, `KEY_SETTLE`), `Esc` devuelve el foco al editor. Pocos editores de escritorio lo hacen tan completo.
3. **Hover y presionado con propósito.** `hover_fx` da entrada rápida (120 ms) y salida lenta (220 ms), el ícono se encoge 1,5 px al apretar y el botón de reproducir baja su opacidad. Es retroalimentación rutinaria, rápida, interrumpible (`Track::aim` acorta el tramo si se da vuelta a medio camino). Eso es lo que pide la rúbrica.
4. **Línea de tiempo legible.** Una pista por tipo con ícono y nombre en el margen, color por tipo (violeta, verde, rosado, azul), selección por opacidad 0,30 → 0,55 + borde 0,7 → 1,0 + asas blancas, cabezal acento de 2 px con burbuja de tiempo en mono, imán con guía vertical y `Alt` para desactivarlo, clics como puntos sobre la regla.
5. **Estados de exportación completos.** Ajustes → avance con cancelar → listo (revelar, abrir, copiar) → fallo (reintentar/volver), y el botón de arriba refleja el estado con el diálogo oculto («Exportando 43 %», «Exportado»). Solo falta el movimiento (hallazgos 5 y 14).
6. **Notas en contexto en vez de documentación aparte**, y preajustes (Calmo/Normal/Dinámico) que se desmarcan solos cuando los números ya no coinciden (`preset_of`). Es el equilibrio «pocos botones, muchas opciones» que tiene OpenScreen.

## Priority Issues

### 1. [P1] El aviso entra al flujo y empuja toda la ventana (y reescala la vista previa)
**Why**: `notice_bar` se pinta como hijo de la columna raíz entre la barra de arriba y el cuerpo (`mod.rs:2064`, `.children(notice)`, con `mb(GAP/2)` y ~30 px de alto). Cada aviso (guardado, «Autofocus», errores) desplaza hacia abajo la vista previa, el transporte, la barra lateral y la línea de tiempo, y 4 s después todo vuelve a subir. El escenario cambia de tamaño → `fit()` recalcula → el `Composer` vuelve a armar el cuadro. En la rúbrica: la retroalimentación no puede cambiar el lugar de lo que el usuario mira. Y el aviso nace y muere de golpe.
**Fix**: sácalo del flujo y hazlo una pastilla flotante (`absolute`, `bottom(GAP + 8)`, centrado sobre el hueco entre transporte y línea de tiempo, o `top(TOP_H + 8)` sobre el escenario), `max_w(560)`, mismos colores que hoy (`pal.control` 0,95 de alfa para que se lea sobre el video; error: `pal.cut` 0,14 + borde `cut` 0,5) y sombra `0 6 20 black 0,30`. Movimiento: entrada 160 ms `ease_smooth_out`, opacidad 0 → 1 y `translateY` 8 px → 0; salida 120 ms, opacidad 1 → 0 y 0 → 4 px. El estado manda: guarda `Notice.at` (ya existe) y `Notice.gone_at: Option<Instant>`; el progreso de entrada es `segment(now - at, 0, 0.16)` y el de salida `segment(now - gone_at, 0, 0.12)`; el aviso se desmonta cuando sale terminó (regla 3: progreso desde el estado). Un aviso nuevo mientras sale otro **reutiliza** el elemento (mismo id) y empieza desde la opacidad actual (regla 4); no se apilan. Los de error se quedan hasta la X (ya es así) y entran con la misma curva, sin rebote. Agrega acción: texto a la derecha `Deshacer` (ver 12).
**Suggested command**: `$impeccable animate`

### 2. [P1] «Lo elegido» va encima de las pestañas y las empuja fuera de la vista
**Why**: `sidebar_panel` arma `selection` → `tabs` → `body` dentro del mismo scroll (`style_panel.rs:1750`). Con un texto elegido la tarjeta mide cerca de 540 px en la captura ed9 (campo, posición, tamaño, color, dos interruptores, «Quitar»); con un zoom, ~260 px. Al hacer clic en un bloque de la línea de tiempo, la fila de pestañas **salta hacia abajo y sale de la pantalla**, y al soltar la selección vuelve. Es el mismo problema de 1: el layout cambia por un gesto que el usuario no pidió sobre la barra. Además la tarjeta se ve igual que una sección más (mismo título 11 px muted), así que no se distingue «lo que elegí» de «los ajustes del proyecto».
**Fix**: (a) saca la fila de pestañas del scroll: `tabs` queda `flex_none` arriba (padding 14 px, `mb(10)`), y el scroll (`editor-sidebar`) empieza debajo. (b) Pasa `selection` al principio del scroll, como una **tarjeta** con identidad propia: `bg(pal.control.opacity(0.55))`, `rounded(pal.r_ctl + 2)`, `p(12)`, `mb(14)`, y una banda de color de la pista a la izquierda de 3 px **como barra de relleno de un hijo** (no `border-left`; usa `pal.zoom`, `pal.text_lane`, `pal.cut`, etc., con el mismo color del bloque en la línea de tiempo), título 12,5/600 `pal.text` («Texto elegido») y a la derecha una X de 20 px que deselecciona (`Esc` ya lo hace). (c) Al elegir un bloque, `scroll_to_top` (ver 8) para que lo elegido sea lo primero que se ve. (d) Si hay selección y la pestaña activa es la de ese tipo (Zoom → zoom, Texto → texto), no repitas la ayuda («Opciones del texto elegido arriba», `side_text_tab`); ya está arriba.
Movimiento: solo opacidad 0 → 1 de la tarjeta, 140 ms `ease_smooth_out`, **sin animar el alto** (regla 7: nada de `overflow_hidden` en algo que cambia de alto, y sin la U de la regla 6). El contenido de abajo se corre de un golpe una sola vez; aceptable porque lo provocó el clic en el bloque, no un aviso.
**Suggested command**: `$impeccable layout`

### 3. [P1] El foco visible no existe donde hace falta, y el aro de la pestaña se lee como «seleccionada»
**Why**: tres problemas en uno.
- **El aro aparece con el mouse.** `side_tab` usa `.focus(|s| s.border_color(accent))` (`style_panel.rs:1072`); un `div` con `track_focus` toma el foco al hacer clic, así que la pestaña recién clicada queda con borde acento **y** relleno `sel` (se ve en n4, n5 y n6; en n1, sin clics, no hay aro). Foco (dónde está el teclado) y selección (qué pestaña es la activa) se confunden, y el aro no aparece donde un usuario de teclado lo espera (seguir con Tab).
- **Casi nada se puede enfocar.** Solo pestañas y sliders tienen `track_focus`; chips (`chip`, mod.rs:1396), `switch_row`, `button`, `primary`, muestras de fondo, losas de cursor (`side_cursor_styles`) y filas de `side_rows` son solo de mouse. Con teclado se llega a un slider y a nada más.
- **El foco del slider es invisible:** `.focus(|s| s.bg(accent.opacity(0.12)))` sobre una pista de 22 px sobre `control` 0,55; en oscuro es casi indistinguible del hover (`hover.opacity(0.5)`).
**Fix**: un helper en `palette.rs`, `Ed::focus_ring(el, handle, radius)`: aro **2 px `pal.accent`**, desplazado 2 px (`.outline` o un hijo `absolute().inset(-3)` con `border_2`, `rounded(radius + 3)`), visible **solo si el foco vino del teclado**: guarda `kbd_nav: bool` en `RecordingEditor`, lo pones en `true` en `on_key` cuando la tecla es Tab, ←/→, Espacio o Intro con algo enfocado, y en `false` en el primer `on_mouse_down` de la ventana. Si el `gpui` fijado ya trae `.focus_visible(...)`, úsalo y borra el flag (verificar en `Cargo.lock`). Aplica el helper a: pestañas (quita el `border_color` actual), pista del slider (reemplaza el 12 %), chips, interruptores, botones, `primary`, muestras, losas y filas. Con `FocusHandle` por pieza en una lista (`tab_stop`), Tab recorre la barra en orden visual; ←/→ dentro de un grupo de chips mueven el foco al siguiente chip (`radiogroup`), y Espacio/Intro lo eligen. Sin animación del aro: aparece al instante (es foco, no decoración); si quieres, 80 ms de opacidad, nunca desplazamiento.
**Suggested command**: `$impeccable harden`

### 4. [P1] Lo que cambia de estado «salta»: pestañas, chips, interruptores, manilla y contenido
**Why**: el hover se funde, pero lo **elegido** no. Hoy:
- La pestaña activa cambia `bg` de golpe (`side_tab`, `on` decide `sel` o `hover`), y el panel de abajo se reemplaza entero (`side_panel`) con otra altura.
- En los chips (`chip`, mod.rs:1396; `side_pick`, `options`) `on` mueve el relleno `sel` de un chip a otro sin transición: el `hover_bg` interpola el *hover*, pero `rest` y `over` cambian instantáneamente cuando `on` cambia.
- El interruptor (`switch_row`) cambia color y posición de la bolita en un cuadro.
- La manilla del slider pasa de 14 a 16 px (`THUMB`/`THUMB_DRAG`) al arrastrar, sin transición.
- En `side_rows` el punto del radio aparece/desaparece de golpe.
Es lo que más separa esto de Screen Studio, y es el movimiento de **retroalimentación** más barato de ganar: el usuario ve que su clic cambió algo, y *qué* cambió (de dónde a dónde).
**Fix** (usa solo lo que ya existe: `glide.rs`, `hover.rs`, `anim.rs`):
1. **Pestañas**: `glide::indicator` dentro de la fila de pestañas (la misma pieza que ya usa Ajustes), `mark_on` en cada pestaña con clave `tab.index()`, el óvalo `sel` desliza de una a otra. `glide.rs` tiene `SECS = 0.5`, pensado para navegación grande; en una barra de 7 íconos de 38 px se ve lento: añade un parámetro a `Look` (`secs: Option<f32>`) y pasa **0,28** aquí. Quita el relleno propio de la pestaña activa (que lo dibuje solo el indicador) y deja solo el color del ícono (`h.mix(muted, text)` para inactiva; `text` para la activa), con `Tween::hover` de 120 ms para que el ícono cambie de color a la par.
2. **Chips y grupos** (`options`, `side_pick`): el mismo `glide::indicator` por grupo con clave por chip, **más corto: 0,18 s**, sin `SQUEEZE` (parámetro `squeeze: 0`) porque se usa seguido; con un clic repetido o rápido el borde de atrás del indicador sigue al estado (no reinicia) y nunca se queda atrás de la selección (interrumpible, regla del skill). Dentro de grupos de 2×2 (`Clic → Efecto`) el indicador viaja en diagonal; está bien, es continuidad espacial real.
3. **Interruptor**: bolita con `Tween::hover(0/1, 140 ms, ease_smooth_out)` por `id` (guarda el `Tween` en `StyleUi`, `HashMap<&'static str, Tween>`, y súmalo a la lista de «hay algo animándose» que ya pide cuadros): posición `lerp(2, 14, t)`, relleno `mix(control, accent, t)`, color de la bolita `mix(muted, on_accent, t)`. Sin rebote (`ease_back_out` es para momentos de énfasis, no para un ajuste).
4. **Manilla**: `Tween::hover(14↔18, 90 ms, ease_smooth_out)`, con aro `raised` de 2 px que crece a 3 px; **no** animes el *valor* durante un arrastre (gesto 1:1). Solo si el clic cae a más del 12 % de la pista (salto), anima el relleno y la manilla 90 ms `ease_smooth_out`; si sigue un arrastre, `snap`.
5. **Contenido de la pestaña**: nada de deslizar ni de cambiar la altura; **fundido corto de 120 ms** (opacidad 0 → 1 con `translateY` 4 → 0 px) del panel nuevo, sin fundido de salida (el viejo ya no se dibuja: un solo dueño, regla 1). Progreso desde `tab_changed_at: Instant` (regla 3). No animes alturas.
Reducido (ver 18): indicadores y pestañas hacen `snap`, el fundido queda en 80 ms sin `translateY`.
**Suggested command**: `$impeccable animate`

### 5. [P1] El diálogo de exportar aparece de golpe, y su tarjeta salta al cambiar de contenido
**Why**: `export_overlay` devuelve `None` cuando `open` es `false` (export_panel.rs:569) y un `div` con el 45 % de negro cuando es `true`: sin fundido del fondo, sin entrada de la tarjeta, sin salida. Peor, la tarjeta se **centra** (`items_center`, línea 638) y su alto depende del contenido: en la captura ed4 (MP4) su borde superior está en y=159 y en ed11 (GIF) en y=222, es decir, **~45 pt de salto** al tocar MP4 ↔ GIF (desaparecen Resolución, Calidad y Códec), y otra vez al pasar de ajustes a avance a listo (altura distinta). Cada salto mueve el título y los botones que el usuario tiene bajo el cursor. Es el caso de manual de la regla 6 (un contorno que cambia de tamaño sin dueño del movimiento).
**Fix**:
- **Anclaje**: cambia `items_center` por `items_start` y `pt(max(24, 12 % del alto de la ventana))`. El borde superior (título y primera fila) queda fijo; solo el borde de abajo crece o se achica. Es la corrección más barata y evita el 80 % del salto sin animar nada.
- **Entrada** (al `open_export`): fondo 0 → `SCRIM_ALPHA` en **140 ms** `ease_smooth_out`; tarjeta opacidad 0 → 1, escala 0,98 → 1 (origen arriba al centro) y `translateY` 6 → 0 px en **180 ms** `ease_smooth_out`. **Salida** (cierre, Esc, clic fuera): 120 ms, opacidad 1 → 0, escala 1 → 0,99, y el fondo en paralelo; la capa se desmonta al terminar. Hace falta un `export_ui.open_t: Tween` (`anim::Tween::hover`, sin `SLOW`: las duraciones son las finales) y que `export_overlay` siga devolviendo la capa mientras `open_t.is_running() || open_t.target() > 0`. Reabrir a mitad de la salida usa `Tween::set` y sigue desde donde estaba (interrumpible).
- **Tamaño de la tarjeta**: anima el alto con un `Tween` de **160 ms** `ease_smooth_out` (progreso desde el estado: `lerp(h_prev, h_new, t)`), pero **sin `overflow_hidden` en los renglones**, solo en la tarjeta (regla 7). Solo vale para MP4 ↔ GIF y ajustes → avance → listo; el contenido nuevo hace fundido de 120 ms (opacidad) y el viejo no se dibuja.
- No hay `Run::Done` con celebración: el ícono `circle-check` de 22 px (acento) puede entrar con `ease_back_out` 220 ms de escala 0,7 → 1 **una sola vez**; es el único momento «de énfasis» del diálogo y se lo ganó.
- Reducido: solo opacidad en 80 ms; sin escala ni desplazamiento.
**Suggested command**: `$impeccable animate`

### 6. [P2] Los botones fantasma no parecen botones, y todas las acciones pesan lo mismo
**Why**: `button()` (mod.rs:1335) no tiene fondo, borde ni forma en reposo: texto 12,5 + ícono 16 centrado en una columna de 268 px. En n6 «Ver cómo queda», «Acelerar silencios», «Cortar silencios», «Agregar velocidad» y «Agregar difuminado» son cinco filas de texto centrado con ícono; se leen como etiquetas hasta que pasas el mouse. «Cortar silencios» (destructivo: borra tramos) y «Agregar difuminado» (aditivo) tienen exactamente el mismo peso. Hoy solo hay dos niveles: ghost y acento lleno (`primary`, usado en «Autofocus»), sin intermedio. Screen Studio/OpenScreen usan botones con contorno o relleno suave dentro de paneles.
**Fix**: añade `Ed::secondary(el)`: `h(32)`, `w_full`, `px(12)`, `bg(pal.control.opacity(0.55))`, `border_1`, `border_color(pal.border.opacity(0.6))`, `rounded(pal.r_ctl)`, contenido alineado a la izquierda (`justify_start`, `gap(8)`) con ícono 15 px en `muted`, y con hover mezcla a `hover`. Úsalo para todas las acciones de panel (`editor-preview`, `editor-idle-*`, `editor-speed-add`, `editor-blur-add`, `editor-text-add`, `editor-image-pick`, «Zoom en el cabezal»). Mantén el botón ghost solo para barras (deshacer, carpeta, transporte). Las acciones **destructivas** (`Quitar`, `Cortar silencios`, `Restablecer`) con texto e ícono en `pal.cut` y, en hover, `bg(cut.opacity(0.12))`. Pares (`Agregar velocidad` / `Agregar difuminado`, `Acelerar` / `Cortar`) en una fila de 2 columnas de 130 px, no en dos filas centradas. Los estados ya existen: hover y presionado con `hover_fx`.
**Suggested command**: `$impeccable polish`

### 7. [P2] Escala tipográfica fragmentada y contraste bajo en el texto secundario
**Why**: en la barra y el diálogo conviven 11, 11,5, 12, 12,5, 13, 14 y 15 px; etiquetas de campo (`faint` 11,5), valores (`muted` 11,5), notas (`faint` 11,5), chips (12), botones (12,5), campos hex (12,5), campos de texto (13), y títulos de sección (11, mayúsculas, `muted`). La mitad de la diferencia son medios píxeles que nadie percibe como jerarquía, solo como irregularidad. Y el `faint` de 11 a 11,5 px sobre `control` 0,55 (que sobre el panel azulado de las capturas queda ~#3a3d44) lo da a ojo ~3:1 (el valor exacto sale de `theme::FAINT`; hay que medirlo con el validador del proyecto): «Margen», «Esquinas», «Tamaño», «Colores propios» y los párrafos de ayuda de n5/n6 («Con cuánto nacen los zooms nuevos…», «La estela que deja la cámara…») fallan AA y en la captura se leen con esfuerzo.
**Fix**: tres tamaños y tres colores, sin excepción: **11** (títulos de sección en mayúsculas y notas), **12** (etiquetas, valores, chips), **13** (botones, campos, filas). Elimina el 11,5 y el 12,5. Colores: etiqueta de campo en `muted` (no `faint`); valor en `text` mientras arrastra y `muted` si no (ya es así); notas en `muted` con alfa 0,85; `faint` solo para íconos y texto decorativo. Valida con el script de contraste que `muted`/`faint` contra `control` y `panel` den ≥ 4,5:1 en claro y oscuro **y** en los tres estilos (Atic, Material 3 Expressive, Liquid Glass; con vidrio, contra `raised` calculado). Notas de dos líneas se acortan a una cuando se pueda: «Con cuánto nacen los zooms nuevos y el zoom automático. Los que ya hiciste no cambian» es una sola idea («Se aplica a zooms nuevos») y cabe en 36 caracteres.
**Suggested command**: `$impeccable typeset`

### 8. [P2] La barra se corta abajo sin pista, y al cambiar de pestaña el scroll se queda donde estaba
**Why**: el panel (`overflow_hidden`, h_full) recorta el scroll en seco: en p2 «Sombra» queda a 10 px del borde con la manilla cortada; en n5 el título «CÁMARA» aparece pegado al borde inferior sin su contenido. No hay barra, degradado ni «más abajo». Peor: el scroll es un `div` con `.id("editor-sidebar")` fijo, y GPUI guarda el desplazamiento por id, así que al pasar de una pestaña larga (Fondo, Zoom) a otra, la nueva abre **desplazada** (verificar). Combinado con 2, hay estados donde ves el último tercio de un panel sin saberlo.
**Fix**: (a) cambia el id a `("editor-sidebar", tab.index())` para que cada pestaña arranque arriba y recuerde su propio scroll (o resetea con un `ScrollHandle`). (b) Degradado inferior de 20 px, `linear_gradient(pal.panel 0 → 1)`, visible solo cuando queda contenido debajo (usa el `ScrollHandle` para `max_offset` y `offset`); degradado superior igual cuando `offset < 0`. Con `pal.glass` no pintes degradado de color: usa una máscara (`opacity`) o un filete de 1 px `border.opacity(0.5)` y, si no es posible, solo el filete. (c) Barra de 4 px, `pal.muted.opacity(0.35)`, `rounded_full`, a 4 px del borde derecho, que se muestra 600 ms tras un scroll y mientras el mouse esté sobre la barra, opacidad 120 ms `ease_smooth_out` (retroalimentación sobre el scroll; no hace falta si ya hay degradado, elige uno de los dos y mantén el degradado). (d) Reduce lo que hay que scrollear: separa «Movimiento» en dos pestañas solo si pasa de ~560 px, no antes.
**Suggested command**: `$impeccable polish`

### 9. [P2] La imagen de fondo elegida se sale del margen y tapa su propio nombre
**Why**: en n1 la miniatura (fila «Whimsical Keyboard Explore…») empieza en x≈1493, **15 px a la izquierda** del borde de las muestras (x=1508), y es rectangular (~70×30 en la captura, ~50×22 pt) en vez de un cuadro de 30 px; el nombre del archivo arranca encima de su borde derecho. Causa: en `side_image` (style_panel.rs:1356) el contenedor es `size(30)` con `p(2)` y `border_2`, pero la `img` con `ObjectFit::Cover` y `size_full` **no se recorta** (no hay `overflow_hidden` en el contenedor) y se pinta a su proporción, desbordando por ambos lados. Los dos botones (cambiar y quitar) son solo ícono sin texto y de iguales tamaño y forma.
**Fix**: en `thumb` agrega `.overflow_hidden()`, y a la `img` `.size(px(22)).rounded(px(6)).flex_none()` (30 − 2·2 padding − 2·2 borde); `gap(8)` a la fila, nombre con `truncate` y `min_w_0` (ya está). Los botones de cambiar y quitar quedan igual, pero con el `secondary` de 6 solo si hay texto; con íconos a solas, `size(28)` y el ícono de «quitar» en `muted` (no `cut`: quitar la imagen no destruye nada, el fondo vuelve al anterior). Mientras la imagen se carga o falla, la miniatura muestra un cuadro `control` con el ícono `image` en `faint` (hoy: nada).
**Suggested command**: `$impeccable polish`

### 10. [P2] Bloques que se cortan a media letra, pistas vacías mudas y un mismo ámbar con tres significados
**Why**: (a) `block()` pone `whitespace_nowrap` + `overflow_hidden` (timeline.rs:422): los bloques cortos muestran «Difur» y «2» cortados a la mitad de una letra (n1, n4–n6). (b) Las pistas Velocidad, Difuminar y Texto están vacías en casi todas las capturas y no dicen nada: no hay «Z para agregar» ni indicio de qué va ahí; solo el ícono y el nombre en el margen. (c) `pal.click` y `pal.trim` son ambos `AMBER` (palette.rs:103–104), y la guía del imán (`timeline.rs:644`, `pal.click`) es el mismo color: puntos de clic, asas de recorte e imán son iguales, y los tres se ven en el mismo vistazo. (d) La burbuja del cabezal tapa la etiqueta de la regla debajo («0:» en n4). (e) «queda 01:40» a los 13,4 s parece «tiempo restante» y es la duración de salida (`out_length`).
**Fix**: (a) la etiqueta lleva `truncate()` y `min_w_0`; si el bloque mide menos de 28 px, sin etiqueta; con ≥ 28 px se corta con «…»; el nombre completo en el tooltip (`hover::tip_text`), que además da el rango («2× · 0:42–0:50»). (b) en cada pista vacía, texto de 11 px `faint` alineado a 10 px del borde izquierdo del área, «Z · acercar desde el cabezal» (con la tecla en un `kbd` de fondo `control`, `rounded(4)`, `px(4)`), visible solo mientras la pista está vacía y se desvanece (100 ms) al crear el primer bloque; el equivalente en Cortar (no hay pista de cortes) queda en el botón. (c) el imán pasa a `pal.text.opacity(0.9)` de 1 px, con una muesca de 5×5 pt arriba (en la regla), y el recorte conserva el ámbar; los clics siguen en ámbar pero a 4 px en vez de 5, porque ya hay otro uso. (d) cuando el cabezal está a menos de 24 px de una etiqueta de la regla, bájale la opacidad a esa etiqueta a 0,3. (e) cambia el rótulo a «Salida 01:40» (`pill.editor.remains`), y solo se muestra cuando difiere de la duración.
**Suggested command**: `$impeccable clarify`

### 11. [P2] Audio en silencio: se ve igual que «cargando» y no hay forma de saber por qué
**Why**: la memoria del proyecto lo dice: las grabaciones del usuario traen audio en −91 dB (permiso de micrófono sin dar). `Wave::Levels` con niveles ~0 pinta barras de 1,5 px (el `max(1.5)` de `timeline.rs:605`), que a escala reducida es una línea punteada plana, idéntica en espíritu a `Wave::Loading`. En n1 la pista Audio se ve así y el usuario no puede distinguir «sin audio», «cargando» o «audio mudo por permisos». Es información que ahorra media hora de buscar bugs en el editor.
**Fix**: si `levels.iter().cloned().fold(0, f32::max) < 0.01`, dibujar el mismo texto de `Wave::Silent` en `faint` («Audio en silencio · revisa el permiso del micrófono») centrado en la pista, ícono `circle-alert` de 12 px a la izquierda; con clic abre Ajustes de la pill → Permisos. `Wave::Loading` queda con su línea y gana un barrido sutil (ver Motion Audit, solo si el cargado pasa de 600 ms: gradiente de 40 px de ancho, `pal.faint` 0 → 0,25 → 0 a 1,2 s lineal, en bucle; con «Reducir movimiento», sin barrido).
**Suggested command**: `$impeccable harden`

### 12. [P2] Retroalimentación incompleta: autoguardado mudo, borrar sin «Deshacer» y exportación terminada con el diálogo oculto
**Why**: (a) `changed()` guarda el proyecto en cada cambio (mod.rs:562) y solo avisa si falla. El usuario nunca ve que su trabajo quedó guardado; en Screen Studio y en Figma hay un indicador discreto. (b) Borrar un bloque (`delete_selected`), «Cortar silencios», «Quitar zooms automáticos» y «Restablecer» no dicen qué pasó ni ofrecen deshacer a un clic; el usuario depende de ⌘Z, que la barra superior solo muestra como un ícono tenue (`icon_button` apagado a `faint` 0,6). (c) Si la exportación termina con el diálogo oculto, solo cambia el texto del botón de arriba a «Exportado»: sin aviso ni sonido ni pulso, y el botón es el mismo `primary` de siempre, con acento pleno.
**Fix**: (a) a la derecha del título, 11 px `faint`: «Guardado» con `check` de 11 px, que aparece 120 ms (opacidad) en cada guardado y se retira a los 1,6 s (también 120 ms); en error, el aviso actual (no duplicar). Es el momento de «reconocer», no de celebrar: sin color de acento. (b) `say(text, error, action: Option<(label, fn)>)` con el texto «Zoom eliminado» + botón `Deshacer` (11,5 → 12 px, `accent`, `bold`), que llama `undo()`; el aviso de borrar vive 5 s (hoy 4). Para «Cortar silencios», el texto incluye cuánto quitó («Quité 12 tramos · 0:38») y el botón. (c) cuando `Run::Done` llega con `export_ui.open == false`, `say("Exportado: nombre.mp4", false)` con acción «Mostrar» (revelar) y un pulso de 600 ms en el botón de arriba (anillo de acento 0 → 6 px y 0,4 → 0 de opacidad, `ease_smooth_out`, una sola vez). Si falla: aviso de error persistente con «Ver» que reabre el diálogo en `Run::Failed`.
**Suggested command**: `$impeccable harden`

### 13. [P2] Los atajos existen pero no se descubren
**Why**: S, Z, T, V, B, A, ⌘Z, ⌘E, +/− y Espacio solo están en los globitos (que tardan en salir) y en una nota de la **última** pestaña («Proyecto»; `side.shortcuts`, i18n:2647, 11,5 px `faint`). El usuario de Screen Studio espera ver la tecla junto al botón. Los botones «Cortar / Zoom / Texto / Velocidad / Difuminar» de abajo a la izquierda son la superficie natural.
**Fix**: con ancho (`wide`, ya existe `TOOLS_LABELS_MIN_W`), cada botón de herramienta lleva su tecla como `kbd` de 10 px en `faint`, fondo `control` 0,6, `rounded(4)`, `px(4)`, a la derecha de la etiqueta (`S`, `Z`, `T`, `V`, `B`). En los globitos (`hover::tip`), la tecla en un `kbd` aparte al final (en vez de «· S» pegado). Atajo `?` (o `⌘/`) abre una hoja de 340 px con la tabla completa, mismo estilo que el diálogo de exportar. Mueve la nota de atajos de «Proyecto» a un enlace «Atajos» en el pie de la barra, visible en toda pestaña. Y `⌘1…7` para ir a la pestaña correspondiente (con el glide de 4).
**Suggested command**: `$impeccable onboard`

### 14. [P2] El avance de la exportación va a saltos, y «cancelando» no se mueve
**Why**: `export_running_body` pinta `w(relative(p))` con el `p` que guarda un `AtomicU64` (export_panel.rs:1100). El hilo de exportación lo actualiza a su ritmo (por etapas, `overall(...)`) y la interfaz solo repinta cuando algo la despierta, así que la barra avanza en escalones; en GIF hay una etapa de video (0 → 0,5) y otra de GIF (0,5 → 1) que no se parecen, y entre etapas la barra se queda quieta sin que haya señal de vida. Al cancelar, el texto cambia a «Cancelando…» pero la barra sigue igual.
**Fix**: `shown_p` en `export_ui` que persigue a `p` con suavizado exponencial de ~150 ms (`shown += (p - shown) * (1 - exp(-dt/0.15))`), solo hacia delante, y la interfaz pide cuadros mientras `|p - shown| > 0.002` (súmalo al «hay animación» de `export_busy`). Mientras `p` no avanza más de 1,5 s, la barra muestra un brillo de 60 px que la recorre (`pal.on_accent` 0,18 → 0, 1,4 s lineal, en bucle) para decir «sigo vivo». Al llegar a 1, la barra se vuelve `pal.accent` sólido, 200 ms, y el diálogo pasa a «listo». Al cancelar, la barra se atenúa a `faint` en 140 ms y el brillo se apaga. El botón de arriba (`Exportando 43 %`) rellena su fondo de izquierda a derecha con el mismo `shown`: es progreso visible con el diálogo cerrado.
**Suggested command**: `$impeccable animate`

### 15. [P3] La línea de tiempo cambia de vista de golpe
**Why**: `set_view` asigna el nuevo `View` entero (timeline.rs:~700): los botones + y −, «ajustar» y el seguimiento del cabezal durante la reproducción (`View::follow`) mueven todo en un cuadro. La rueda es gesto 1:1 y está bien; los botones no lo son.
**Fix**: para botones y atajos (+, −, ajustar, ir al inicio/final), un `Tween` por `view.start` y `view.len` de **180 ms** `ease_smooth_out`, con el mismo `t` para ambos (la pista y la regla se deben mover juntas: una sola fuente de progreso, regla 6). La rueda, el arrastre de la barra de vista y el pinch hacen `snap`. El seguimiento del cabezal al reproducir pasa por 200 ms (`ease_smooth_out`) cuando salta de página, no cuadro a cuadro. Con 1,5× de `SLOW` en `Tween::new`, usa `Tween::hover` (duración exacta).
**Suggested command**: `$impeccable animate`

### 16. [P3] Bloques recién creados y seleccionados: un solo gesto de énfasis, no más
**Why**: al apretar Z, S, T, V o B aparece un bloque y se elige, pero en la línea de tiempo no hay indicio de *dónde* apareció (puede estar fuera de vista) y la selección cambia opacidad y borde de golpe. Con Autofocus (8–10 bloques a la vez) animarlos todos sería ruido.
**Fix**: (a) en la creación **manual**, un pulso: borde del bloque a 1,0 → 0,7 y relleno 0,55 → 0,30→ elegido (queda 0,55 por estar seleccionado), 200 ms `ease_smooth_out`, solo para el bloque nuevo; si queda fuera de la vista, el `view` se desplaza hasta mostrarlo con el tween de 15. (b) Selección: el cambio de opacidad del relleno y del borde pasa por `Tween::hover` de 100 ms por bloque (clave `(lane, i)`), no por el `bool` directo. (c) Autofocus, importaciones y deshacer/rehacer: sin animación (cambio en masa, no merece énfasis). (d) Borrar: sin animación; el aviso con «Deshacer» de 12 ya comunica.
**Suggested command**: `$impeccable animate`

### 17. [P3] Detalles del slider
**Why**: pistas de 4 px con manilla de 14, y un valor que cambia de ancho al arrastrar («8 %», «50 %», «1,5×», «Normal»): en proporcional los dígitos se mueven. El doble clic para restablecer solo está en el globito, que dice «Arrastra · doble clic restablece · ←/→ ajustan (⇧ más)» y se lee en una línea larga. Con el valor por defecto no hay marca en la pista.
**Fix**: valores en `font_family(pal.mono)` o con dígitos tabulares; pequeña muesca de 2×8 px a la altura del valor por defecto (`faint` 0,6) en la pista; cuando el valor difiere del defecto, un ícono `rotate-ccw` de 12 px a la derecha del valor en `faint` que, con hover, pasa a `text` y restablece con un clic (revela la acción de doble clic). Alto de la zona de agarre de 22 a 24 px (objetivo mínimo). El globito más corto: «Doble clic: restablecer».
**Suggested command**: `$impeccable polish`

### 18. [P3] «Reducir movimiento» no se consulta en ninguna parte del editor
**Why**: `grep` de `system_reduced_motion` en `recording_editor/` no da nada; `hover.rs` tampoco lo mira. `glide.rs` sí lo respeta (glide.rs:136), pero hoy el editor no usa glide, así que al agregar los hallazgos 1, 4, 5, 14 y 15 sin esto, se rompe la regla de la rúbrica.
**Fix**: un helper en `palette.rs` o `anim.rs`: `pub fn motion_ok() -> bool { !crate::code::style::system_reduced_motion() }` (cacheado un cuadro, que no consulte al sistema en cada pieza). Convención en todo el editor: sin movimiento → los `Tween` hacen `snap`, las traslaciones y escalas se quitan, los fundidos de opacidad quedan en ≤ 80 ms y los bucles (brillo, barrido) se apagan. El cambio de estado (qué pestaña es la activa, qué chip está elegido) sigue siendo visible por color y por el relleno.
**Suggested command**: `$impeccable harden`

### 19. [P3] Vista previa: sin eco al hacer clic y sin indicación del zoom activo
**Why**: hacer clic en el escenario alterna reproducir/pausar (`on_mouse_down` → `toggle_play`) sin ningún eco; el usuario no sabe si pausó o pausó el clic. Con un zoom activo en el cabezal (p2) no hay forma de saber «cuánto» ni «de dónde» se está acercando sin mirar la línea de tiempo. Sin cuadro todavía, el escenario es un fondo oscuro sin indicio de carga.
**Fix**: (a) al alternar con clic, un ícono play/pausa de 44 px sobre el centro, círculo `black 0,5`, que entra con escala 0,85 → 1 y opacidad 0 → 1 en 90 ms y sale a los 500 ms con opacidad en 160 ms; solo con clic en el escenario, no con Espacio (el botón de abajo ya cambia). (b) Chip de 11 px, `black 0,55` y texto blanco, en la esquina superior izquierda del escenario con «2,4×» mientras el cabezal está dentro de un zoom; fundido de 120 ms al entrar y salir, nunca reposiciona el video. (c) Sin cuadro, un `circle` de 18 px girando a 0,9 s lineal tras 400 ms de espera (si carga antes, no aparece); con «Reducir movimiento», solo el texto «Cargando…».
**Suggested command**: `$impeccable delight`

### 20. [P3] Las pestañas son solo ícono: el nombre vive en un globito que tapa el título
**Why**: siete íconos de 38 px (`TAB_H = 32`) en una fila de 296 pt; el nombre aparece solo en el globito, y el globito (n4–n6) cae justo encima del título de la sección de abajo («CURSOR», «ZOOM», «MOVIMIENTO»), tapándolo. Se intercalan íconos poco inequívocos: «layers» para Fondo, «activity» para Movimiento y «settings-2» para Proyecto, que mezcla historial, restablecer y atajos. El título de la primera sección, que repite el nombre de la pestaña, es 11 px muted en mayúsculas, el mismo que las otras secciones; no hay jerarquía de «dónde estoy».
**Fix**: el título de la **primera sección de cada pestaña** pasa a 13/600 `pal.text` y sin mayúsculas, como título de página («Cursor», «Zoom»); las demás secciones quedan 11 mayúsculas `muted`. Globito de pestaña debajo con `offset(8)` y 300 ms de espera (más largo que el normal: el título ya está visible y el globito es solo ayuda). Evalúa la pestaña activa con ícono + nombre (ancho animado con glide, 0,28 s): en 296 pt caben 6 íconos de 30 y una activa de ~92 pt; con Subtítulos y Proyecto a 7 queda justo, pero es el patrón de OpenScreen. Si no cabe, no lo hagas: el título de 13 px basta. Mueve «Atajos» fuera de Proyecto (13) y renombra la pestaña «Historial» o repártela entre el pie de la barra y la barra superior.
**Suggested command**: `$impeccable polish`

## Motion Audit: qué falta, qué merece existir y qué no

Base común: duraciones de `Tween::hover` (sin `SLOW`, son las finales), curva `ease_smooth_out` (0,22, 1, 0,36, 1) para casi todo; `ease_island` (sobrepaso) **no** va en herramientas de edición; `ease_back_out` solo para el ícono de «Exportado» (un único momento de énfasis). Todo se calcula desde un estado con `Instant` (regla 3) y es interrumpible por `Tween::set`.

| Transición | Hoy | ¿Merece existir? (propósito) | Duración y curva | Interrumpible / reducido |
|---|---|---|---|---|
| Hover y presionado de botones, chips | Funde 120/220 ms, ícono −1,5 px | Sí, retroalimentación (ya está) | 120/220, cúbica | Sí / se queda (solo color) |
| Pestaña activa (indicador) | Salta | **Sí**, continuidad: ver *qué* pestaña es | glide **280 ms** | Sí (resorte sigue al estado) / snap |
| Contenido al cambiar de pestaña | Reemplazo seco | **Sí, mínimo**: fundido | **120 ms** opacidad + 4 px | Sí / 80 ms sin desplazamiento |
| Chip elegido | Salta | **Sí**, retroalimentación inmediata | glide **180 ms** sin estrechamiento | Sí / snap |
| Interruptor | Salta | **Sí**, retroalimentación | **140 ms** `ease_smooth_out` | Sí / snap |
| Manilla del slider (14↔18) | Salta | Sí, atención al agarrar | **90 ms** | Sí / snap |
| Valor del slider durante el arrastre | 1:1 con el mouse | **No** animar (gesto directo) | — | — |
| Salto de la manilla por clic (>12 %) | Salta | Sí, solo ese caso | **90 ms** | `snap` si sigue un arrastre |
| Aviso (toast) | Aparece y desaparece en el flujo | **Sí**, retroalimentación + sin empujar | entrada **160 ms**, salida **120 ms**, 8 px | Sí, reutiliza el elemento / solo opacidad |
| Diálogo de exportar | Aparece/desaparece | **Sí**, continuidad con la ventana | fondo **140**, tarjeta **180** (escala 0,98), salida **120** | Sí, `Tween::set` / solo opacidad |
| Alto de la tarjeta del diálogo | Salta, y sube/baja | Sí, pero primero **anclar arriba** | **160 ms** | Sí / snap |
| Barra de avance de exportación | Escalones | Sí, honestidad de estado | suavizado exp. **150 ms** | n/a / se queda |
| Ícono «Exportado» | Aparece | Sí, un solo momento de énfasis | **220 ms** `ease_back_out` | n/a / sin escala |
| Vista de la línea de tiempo (botones) | Salta | Sí, continuidad espacial | **180 ms**, un solo `t` | Sí / snap |
| Vista de la línea de tiempo (rueda, arrastre) | 1:1 | **No** | — | — |
| Bloque nuevo (acción manual) | Aparece | Sí, atención (dónde cayó) | **200 ms** borde | Sí / se queda |
| Selección de bloque | Salta | Sí, pequeña | **100 ms** | Sí / snap |
| Creación en masa (Autofocus), deshacer/rehacer, borrar | Cambio seco | **No** (no se anima todo) | — | — |
| Guía del imán | Aparece/desaparece | Sí, solo fundido corto | **70 ms** opacidad | Sí / se queda |
| Cabezal y seek por clic | Salta al clic | **No** (manipulación directa) | — | — |
| Indicador «Guardado» | No existe | Sí, retroalimentación | **120** entrada, **120** salida a 1,6 s | Sí / se queda |
| Ícono de play/pausa sobre la vista previa | No existe | Sí, retroalimentación del clic | **90** in, **160** out | Sí / sin escala |
| Foco (aro) | Borde en pestañas, tinte 12 % en sliders | **Sí**, pero sin movimiento | 0–80 ms | n/a / se queda |
| Cambio de tema (claro/oscuro) | Instantáneo | No (el SO lo hace) | — | — |

Reglas del skill aplicadas: un solo dueño por elemento (el aviso, la tarjeta del diálogo), relevo cuando el contenido es idéntico (nada de fundido cruzado entre «ajustes» y «avance»; el nuevo entra con fundido, el viejo no se pinta), el progreso desde el estado (`Notice.at`, `open_t`, `tab_changed_at`) y revisar los cuadros intermedios (abrir y cerrar el diálogo en < 100 ms, cambiar de pestaña tres veces seguidas, aviso mientras otro sale).

## Comparación con Screen Studio y OpenScreen

- **Parecido**: panel lateral por pestañas con muchas opciones, vista previa limpia sin cromo, línea de tiempo por pistas, zoom como objeto editable con ícono de auto. En eso Atic ya está al nivel; la vista previa es incluso más fiel (mismo `Composer` que exporta).
- **Mejor que OpenScreen**: teclado en sliders, imán con `Alt`, un paso de deshacer por arrastre, preajustes que se desmarcan solos, estados de exportación completos, vidrio/Expressive/Atic según Apariencia.
- **Peor que Screen Studio**: (1) el cambio de estado no se mueve (hallazgos 4 y 5) y se nota rápido en la barra lateral, que es donde más se usa; (2) los botones de panel son texto flotante en vez de botones (6); (3) no hay indicador de guardado ni «Deshacer» en el aviso (12); (4) los atajos no están en la interfaz (13); (5) la tipografía de apoyo es chica y tenue (7).
- **Peor que OpenScreen**: las pestañas llevan solo ícono; OpenScreen etiqueta (20). No lo copiaría tal cual: con siete pestañas en 296 pt, el título de 13 px de cada panel hace el mismo trabajo.

## Minor Observations

- La muestra «sin marco» (cuadro con una cruz) es un cuadrado redondeado y las demás son círculos de 30 px: forma distinta para un estado distinto, pero lee como «muestra rota»; con borde `border` 1 px discontinuo y el texto «Sin marco» en el globito sería más claro.
- Las losas de estilo de cursor (`side_cursor_styles`) usan borde acento 1 px + `sel`, las muestras un aro de 2 px sin relleno, los chips solo relleno: tres variantes de «elegido». Unifica en **relleno `sel` + aro acento 1,5 px** para losas y muestras, y relleno solo para los chips (que están agrupados).
- Los dos botones (deshacer y rehacer) de arriba están apagados con `faint` 0,6, que sobre la barra oscura casi no se ven; el estado vacío («nada que deshacer») es correcto, pero apagado debería ser 0,45 de opacidad **del color `muted`**, para que el estado habilitado (`text`) se distinga claro.
- «Colores propios» y «Color 1/Color 2» son etiquetas de 11,5 `faint` del mismo peso que las del resto: es un grupo anidado dentro de «Fondo» que merece un filete de 1 px `border.opacity(0.35)` encima, no otra etiqueta igual.
- En los textos de ayuda larguísimos (n5: «Con cuánto nacen los zooms nuevos y el zoom automático. Los que ya hiciste no cambian») la nota queda **entre** el slider de Profundidad y la etiqueta «Velocidad · 0,5 s»; se lee como ayuda de Velocidad y es de Profundidad. Pégala (4 px) al control que explica y deja 12 px antes del siguiente.
- «Velocidad · 0,5 s» mete el valor en la etiqueta del grupo de chips, y en el slider de arriba el valor va a la derecha. Un solo patrón: etiqueta a la izquierda, valor a la derecha.
- La cabeza de la tarjeta «Texto elegido» (ed9) pone el campo de texto antes de Posición/Tamaño/Color; está bien. El ícono «Quitar» (trash) es un botón fantasma al nivel de «Negrita»: separa con un filete y píntalo en `cut`.
- El diálogo de exportar: la fila «Formato» con dos segmentos de 258 px y «Proporción» con losas de 80 px de alto dan un ritmo desparejo (44, 80, 44, 44…). Pon las proporciones en losas de 56 px y Formato en un segmentado de ancho natural (≈ 160 px) alineado a la izquierda.
- El resumen de resultados («Duración / Dimensiones / Peso estimado») se ve como una píldora muy redondeada (r_ctl en el estilo Expressive); con `r_ctl.min(14)`.
- «Cerrar» (con X) al lado de «Exportar» en el pie: en un diálogo que guarda sus ajustes con el proyecto, «Cerrar» es correcto; muestra `Esc` y `↵` como `kbd` pequeño al lado.
- Sin captura de la pestaña Subtítulos ni de los estados de carga de la transcripción (`captions.rs`, fases de audio/habla): no se pudo revisar; es la misma lista de estados (cargando, vacío, error) que se pidió en la rúbrica.

## Questions to Consider

- ¿Debería «Lo elegido» ser un **inspector aparte** (una segunda columna o una hoja bajo el escenario, como en Screen Studio) en vez de una tarjeta dentro de la barra de pestañas? Evita 2 de raíz, pero cuesta ancho de vista previa.
- ¿El «Guardado» es suficiente, o el usuario espera un archivo de proyecto explícito (⌘S, «Guardar como…»)? Hoy el modelo es «todo se guarda solo»; ¿está dicho en algún lugar a la vista?
- ¿Los siete paneles son la cantidad correcta? «Texto» y «Subtítulos» podrían ser una sola pestaña con dos secciones, y «Proyecto» (historial, restablecer, atajos) quizás no es una pestaña sino el pie de la barra.
- ¿La pill de dev debería permitir ver un `SLOW = 3` solo para el editor (como pide la skill en la sección 6) para revisar los cuadros intermedios de los hallazgos 4 y 5 antes de dejarlos? Propongo un `PILL_SLOW` por variable de entorno, solo en debug.
- ¿El foco por teclado debe llegar a la línea de tiempo (bloques como elementos enfocables con ←/→ para moverlos y `[`/`]` para estirarlos)? Hoy es solo mouse; para accesibilidad es el siguiente paso grande después del 3.
