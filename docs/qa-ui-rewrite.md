# QA manual de la reescritura de UI

Lo que no se puede probar con `pnpm verify`.

No propongo Playwright: un E2E sobre Tauri + WebView2 + cinco ventanas +
click-through cuesta más de lo que devuelve. Esto es la alternativa honesta —
una lista corta, versionada, que se corre a mano.

**Cómo correrlo:** `pnpm tauri dev` desde `apps/desktop`. La UI nueva es `/`.
La anterior sigue en `/legacy` y se llega con **Ctrl+Alt+M**, que es la vía de
escape si algo de acá falla.

Marcá lo que falle con la fecha y qué pasó. Un paso que falla no invalida el
resto: seguí y anotá.

---

## 1. Ventana principal

| # | Paso | Qué tiene que pasar |
|---|---|---|
| 1.1 | Abrir la app | Sale el hub con seis herramientas, ninguna marcada «en obra» |
| 1.2 | `Ctrl+K` | Se abre la búsqueda con el cursor puesto; Esc la cierra |
| 1.3 | Esc en una herramienta | Vuelve al hub. **No** vuelve si el foco está en un campo ni si hay un diálogo abierto |
| 1.4 | Cambiar el tema en Ajustes → General | Cambian la ventana principal **y** la pill, sin recargar |

## 2. Reuniones

| # | Paso | Qué tiene que pasar |
|---|---|---|
| 2.1 | Grabar → hablar → Parar | El cronómetro corre, los medidores se mueven, aparece la grabación en la lista |
| 2.2 | Con la transcripción en vivo activada, grabar | Sale el bloque «En vivo» y se ancla solo al último renglón; lo parcial se ve atenuado |
| 2.3 | Importar | Abre el diálogo de archivos, importa, y **selecciona el primero importado** |
| 2.4 | Importar mientras se graba | El botón está deshabilitado |
| 2.5 | Transcribir | Barra de progreso en la fila y en el detalle; al terminar sale el texto agrupado por hablante |
| 2.6 | «Ver y corregir» → buscar | Filtra los fragmentos; el contador dice «N de M» |
| 2.7 | Filtrar por hablante | Yo / Otros / Todos |
| 2.8 | Clic en un fragmento | Suena desde ese momento y la fila queda resaltada mientras avanza |
| 2.9 | «Editar» → cambiar un hablante a «Yo» | El campo de nombre se deshabilita |
| 2.10 | Editar → Guardar | Se relee de Rust; los fragmentos vacíos desaparecen |
| 2.11 | Editar → Descartar | Vuelve todo como estaba. **Ojo:** en modo edición el modal no se cierra con Esc a propósito |
| 2.12 | Exportar a Word / PDF / Markdown | Abre el diálogo, exporta, y la ruta queda escrita abajo |
| 2.13 | «Resumir» → Generar | El texto llega token a token y se va ordenando en secciones |
| 2.14 | Cerrar el modal a media generación y reabrirlo | **El borrador sigue ahí.** Es lo que motivó bajarlo al store |
| 2.15 | Revisar → Editar → Guardar | El indicador pasa de «Cambios sin guardar» a «Guardado» |
| 2.16 | Cerrar con cambios sin guardar | Pregunta antes de descartar |
| 2.17 | Sin clave de proveedor, Generar | Sale el aviso con botón «Ajustes», no un error suelto |
| 2.18 | Enviar → sin destinatarios | Avisa en vez de mandar |
| 2.19 | Borrar una grabación | Pide confirmación y dice que se van audio, transcripción y resumen |

## 3. Primer uso

Se prueba con `onboarding_done: false` en la configuración.

| # | Paso | Qué tiene que pasar |
|---|---|---|
| 3.1 | Abrir la app | Sale el modal y **no se puede cerrar** — ni con Esc ni clicando el telón |
| 3.2 | Cambiar una preferencia y cerrar la app a mitad | Al reabrir, la preferencia quedó guardada y el onboarding vuelve a salir |
| 3.3 | «Más tarde» en el paso de modelos | Pasa al tutorial igual |
| 3.4 | «Descargar y seguir» | Barra de progreso; si falla, se queda en el paso con el error a la vista |
| 3.5 | Último paso | Los tres atajos se leen con las teclas reales de la configuración |
| 3.6 | «Empezar» | Se cierra y no vuelve a salir |

## 4. Ventanas chicas

| # | Paso | Qué tiene que pasar |
|---|---|---|
| 4.1 | Atajo del lanzador | Aparece con el cursor puesto y el campo vacío |
| 4.2 | Escribir | Los resultados llegan; las acciones de Atic tienen el cuadrito verde y las apps el neutro |
| 4.3 | Flechas y Enter | Navega y abre |
| 4.4 | Clic fuera del lanzador | Se cierra solo al perder el foco |
| 4.5 | Reabrirlo | Vuelve vacío, no con lo anterior |
| 4.6 | Capturar | Aparece el estante abajo con la miniatura |
| 4.7 | Clic en la miniatura | Abre la captura |
| 4.8 | Arrastrar la miniatura a una carpeta o a un chat | Sale el archivo. Es arrastre nativo, no HTML5 |
| 4.9 | Dejar el puntero encima seis segundos | **No** se cierra mientras el puntero está encima |
| 4.10 | «Texto» | Extrae el OCR y lo dice en la tarjeta antes de cerrarse |
| 4.11 | Overlay de captura: pasar el puntero | Se resalta la ventana de abajo con las medidas encima |
| 4.12 | Arrastrar una región | El recuadro sigue al puntero y las medidas también |
| 4.13 | Espacio | Captura el monitor donde está el cursor |
| 4.14 | Esc | Cierra **siempre**, incluso si la foto congelada no llegó a cargar |
| 4.15 | Clic derecho | Cancela |

## 5. Overlay

> Todavía sin reescribir (fase 7). Estos pasos valen como red: son lo que **no
> se puede romper** al portarlo, y hay que correrlos otra vez después del
> restyle de la fase 8.

| # | Paso | Qué tiene que pasar |
|---|---|---|
| 5.1 | Atajo de la rueda, mantener | Aparece en el cursor; la rueda del mouse elige; soltar activa |
| 5.2 | Abrir el panel del portapapeles con la pill arriba | Abre hacia abajo |
| 5.3 | Lo mismo con la pill pegada al borde inferior | Abre hacia **arriba** y la barra queda abajo |
| 5.4 | Arrastrar la pill contra los cuatro bordes | Se detiene a 8 px del área útil, sin salirse |
| 5.5 | Arrastrarla entre monitores | Cambia de pantalla por el **centro**, no por la esquina |
| 5.6 | Abrir la consola de agentes | Sale del costado de la pill con el cuello dibujado |
| 5.7 | Arrastrar la consola lejos | El cuello se estira y se corta; no queda un hilo colgando |
| 5.8 | Estirarla por las tres agarraderas | No baja de 420 × 340 |
| 5.9 | Cambiar de tema con la consola abierta | Cambian las dos, sin recargar |
| 5.10 | Entrar y salir de un campo de texto en el overlay | Acepta teclas dentro; al salir, la app de abajo recupera el foco |
| 5.11 | Parar una grabación desde la pill sin mover el mouse antes | El botón responde. Fue un bug real: `ARMED` solo se recalculaba al mover el puntero |
| 5.12 | Cola de pegado | Se encola y se pega en orden |

---

## Lo que ya está verificado y no hace falta repetir

- Las 20 primitivas × 3 paletas en `/dev/kitchen-sink`, revisado en WebView2.
- El campo de distancia dentro de WebView2: 88 fps animando, 2,3 ms de cómputo
  (`docs/qa-liquid-spike.md`).
- Las constantes gemelas con Rust: hay un test que lee el `.rs`
  (`src/lib/surfaces/overlay/contract.test.ts`).
