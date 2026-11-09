# Historial de versiones

English: [CHANGELOG.en.md](CHANGELOG.en.md)

Lo que notó quien usa Atic en cada versión, de la más nueva a la más antigua. Es el mismo texto que la app muestra en «Novedades» al actualizar (`apps/atic/assets/changelog.json`); este archivo se genera desde ahí con `node scripts/gen-changelog.mjs`. Las descargas están en [Releases](https://github.com/chrx3/atic/releases).


## 0.6.0 — 2026-11-08

- **Nuevo:** El panel Celular del notch vincula con un QR más grande, da los permisos y muestra lo que enviaste y recibiste
- **Nuevo:** Desde la pantalla del celular en el PC puedes pedirle permiso para controlarlo
- **Nuevo:** La cara del notch trae acciones rápidas: novedades, actualizar, idioma, tema y color de la pill
- **Nuevo:** El inicio de Atic Code muestra un carrusel con tus últimas conversaciones y sugerencias
- **Mejorado:** Atic Code usa Material 3 Expressive y pliega lo que hace el agente en cada turno en una tarjeta
- **Corregido:** Atic ya no se redibuja sin parar, Alt+X no deja una «x» suelta, cada chat conserva su modelo y ya puedes escribir en «Otra»

## 0.5.10 — 2026-11-05

- **Corregido:** Atic Code ya no muestra «El agente se detuvo» cuando una conversación queda quieta un minuto
- **Corregido:** Una conversación nueva aparece al tiro en la barra lateral

## 0.5.9 — 2026-11-05

- **Nuevo:** Desde Agentes puedes conversar sin proyecto, para una pregunta rápida o algo general
- **Nuevo:** En Ahora suena, el nombre de la app te lleva a su ventana
- **Mejorado:** Cada conversación de Atic Code muestra si está trabajando, si te pregunta algo o si ya está lista
- **Mejorado:** Los agentes de Atic Code siguen trabajando aunque cierres la ventana o reinicies Atic
- **Mejorado:** Cuando hay una actualización, el aviso queda a la vista y la cara de Atic late en verde
- **Mejorado:** Con M3 Expressive, Atic Code, Ajustes y Reuniones tienen las mismas esquinas redondeadas

## 0.5.8 — 2026-11-04

- **Nuevo:** En Atic Code, mientras Claude responde puedes dirigir tu mensaje dentro de la respuesta en curso o dejarlo en cola (Alt+Enter alterna)
- **Nuevo:** En Atic Code, un minimapa que se arrastra salta entre tus mensajes, «Citar» pone el texto seleccionado en tu mensaje y puedes guardar borradores desde el menú de acciones
- **Nuevo:** En Atic Code, pregunta al margen en un panel aparte, envía el mismo mensaje a varios modelos, fija, pospone o marca como hechas tus conversaciones (las que trabajan se agrupan solas; la barra también se ve como una lista sin carpetas), adjunta archivos a tus respuestas y diagnostica un problema de Atic desde la paleta
- **Nuevo:** En el lanzador, Ctrl+Mayús+C copia la ruta de la carpeta o el proyecto elegido
- **Mejorado:** «Copiar texto» vuelve a leer los huecos de cada línea, así recupera palabras y cifras que el OCR se saltaba
- **Mejorado:** «Copiar texto» reconoce las filas de la interfaz (barras, bordes de tarjetas) además de las columnas, y no mezcla lo de arriba con lo de abajo
- **Mejorado:** En Atic Code, las herramientas seguidas se pliegan en una tarjeta con su resumen, y el plan se puede copiar o guardar como .md
- **Corregido:** En Capturas puedes elegir las ventanas de Atic, y una ventana tapada por otra sale entera, sin la de enfrente (Windows)
- **Corregido:** La búsqueda de Procesos encuentra cualquier app, no solo las que más CPU usan
- **Corregido:** El cuentagotas vuelve a mostrar el puntero en macOS (solo se esconde mientras grabas la pantalla)

## 0.5.7 — 2026-11-02

- **Mejorado:** Las letras de «Ahora suena» también se buscan en NetEase si LRCLIB no tiene el tema o está caído
- **Corregido:** En Windows, la pill ya no se cierra sola al alternar la pill en las capturas ni al usar «Ahora suena»

## 0.5.6 — 2026-11-01

- **Nuevo:** En Windows, Editar en tus grabaciones: autofocus, fondos, textos, cortes, zonas difuminadas y «Cursor suave», y exportas en H.264, HEVC o GIF
- **Nuevo:** Capturas tiene una barra para cambiar entre Captura y Grabar, con micrófono, sonido y pill a un clic
- **Nuevo:** En Mac, Editar en tus grabaciones: autofocus en tus clics y donde escribes, 18 fondos de imagen (o la tuya), textos y cortes
- **Nuevo:** En Mac, exporta tus grabaciones en vertical, cuadrado o 4:5, como GIF o en HEVC, con subtítulos automáticos que se generan en tu equipo, sin internet
- **Nuevo:** En Mac, el editor suma un Asistente: le pides los cambios a Claude, Codex, OpenCode o Cursor, y eliges el modelo
- **Nuevo:** Subtítulos en vivo en Reuniones con el modelo local: sin internet ni llave, y el audio no sale de tu computador
- **Mejorado:** En Mac, las grabaciones pesan cerca de la mitad (HEVC), pausar no corta el video y hay «Cursor suave»
- **Mejorado:** El editor de grabaciones se ve igual que la exportación y su cámara sigue al mouse sin pegarlo al borde; ajustas cursor, clic y desenfoque, y suma deshacer, velocidad y zonas difuminadas
- **Mejorado:** Copiar texto separa por columnas: en una interfaz con barra lateral y paneles, la selección no se pasa a la columna de al lado
- **Corregido:** En los avisos que no se pueden responder, «Descartar» queda al final de la fila y el texto se ve más completo

## 0.5.5 — 2026-10-31

- **Nuevo:** Sistema → Rendimiento: CPU, GPU, memoria y red en vivo, con temperaturas, discos y batería
- **Nuevo:** Liberar espacio: encuentra compilaciones y cachés que se pueden borrar y las manda a la Papelera
- **Nuevo:** Sistema → Pantallas: resolución, frecuencia, pantalla principal, duplicar y modo oscuro
- **Nuevo:** Novedades: al actualizar, Atic te cuenta qué cambió
- **Nuevo:** El PC maneja el celular mientras ve su pantalla
- **Nuevo:** Copiar texto respeta columnas y párrafos, y copia las tablas como tabla
- **Corregido:** Reuniones no pierde trabajo al borrar, retranscribir o resumir, y recupera grabaciones cortadas
- **Corregido:** La X para borrar páginas del Flip ya no desaparece bajo el mouse
- **Corregido:** Captura, Flip, Pizarra y cuentagotas ya no se quedan pegados ni dejan el equipo tomado

## 0.5.4 — 2026-10-29

- **Nuevo:** Chat y zumbidos entre dos Atic vinculados
- **Nuevo:** Video de pantalla compartida con bitrate adaptable a la red
- **Mejorado:** La pantalla del celular se decodifica por hardware en macOS
- **Mejorado:** Compartir la pantalla del PC usa hardware y se recupera solo en una red mala
- **Corregido:** La vista previa de Atic Code funciona en Windows

## 0.5.3 — 2026-10-28

- **Nuevo:** Ve la pantalla del celular en el PC y la del PC en el celular, con toque absoluto
- **Nuevo:** Vista previa web en Atic Code
- **Nuevo:** Comparte enlaces y fotos entre el celular y el PC
- **Mejorado:** En Atic Code la selección de texto pasa de un mensaje al siguiente
- **Corregido:** Atic Code deja seleccionar y copiar texto, y sus consolas se desplazan
- **Corregido:** Las letras con traducción muestran solo el original en el verso

## 0.5.2 — 2026-10-26

- **Nuevo:** Atic Code te guía para instalar Node y Claude Code
- **Corregido:** La rueda ya no cierra la herramienta que abre

## 0.5.1 — 2026-10-26

- **Nuevo:** Pomodoro en la pill
- **Nuevo:** Chat con Atic Code en el panel de Agentes
- **Nuevo:** Tareas en Textos, también desde los resúmenes de Reuniones
- **Nuevo:** Paneles lado a lado y carrusel de vistazos
- **Corregido:** La lupa va sin lag y la mira y el cuentagotas funcionan en todas las pantallas
- **Corregido:** Los clics pasan a otras apps tras la captura

## 0.5.0 — 2026-10-24

- **Nuevo:** Bienvenida nueva por pasos, con permisos y un tour que se puede repetir desde Ajustes
- **Nuevo:** El celular y el PC se encuentran en la misma red, con archivos, avisos, llamadas y control táctil
- **Nuevo:** Dictado y Reuniones con motor de voz local Parakeet v3 y prompts propios
- **Nuevo:** Toda la app en inglés, además de español
- **Mejorado:** Transiciones fluidas, letra en carrusel y selecciones que se deslizan como líquido
- **Mejorado:** Ajustes y Reuniones en Material 3 Expressive y Liquid Glass en macOS 26

## 0.4.50 — 2026-10-19

- **Nuevo:** El vistazo de Ahora suena se transforma en el panel
- **Mejorado:** Windows va más fluido y con menos memoria
- **Mejorado:** La bandeja de capturas usa menos memoria y deja dormir a la pill
- **Corregido:** El Portapapeles ya no relee las imágenes ni las deja en memoria

## 0.4.49 — 2026-10-17

- **Nuevo:** Atic de macOS ahora corre con la pill nueva, con notch físico y voz de macOS
- **Nuevo:** Reuniones deja elegir el micrófono y usa otro si el elegido no suena
- **Nuevo:** Ahora suena se despliega desde la carátula con el scroll
- **Nuevo:** La Pizarra agrupa sus herramientas en islas, como el dock de Flip
- **Nuevo:** Atic Code cierra por tiempo las conversaciones inactivas
- **Corregido:** En macOS el arrastre del Portapapeles ya no congela la pill

## 0.4.48 — 2026-10-15

- **Nuevo:** Atic Code suma agentes ACP
- **Mejorado:** Configurar Reuniones es más fácil

## 0.4.47 — 2026-10-13

- **Nuevo:** La Pizarra suma texto, flechas, pixelar, lupa, sellos y exportar
- **Nuevo:** Envía lo dibujado en la Pizarra a Flip o a un agente
- **Nuevo:** Flip importa reuniones con su resumen y las tareas como casillas
- **Nuevo:** Ajustes rápidos al quedarte sobre la cara del notch
- **Mejorado:** Atic Code incluye su agente en el instalador y pule Liquid Glass
- **Corregido:** AltGr ya no dispara los atajos con Alt

## 0.4.46 — 2026-10-10

- **Nuevo:** Atic Code: tu espacio de trabajo con agentes en la pill, con carpetas, paneles y cambios
- **Nuevo:** Atic corre ahora como la pill nativa, con Ajustes, bandeja y bienvenida propias
- **Nuevo:** Vincula la app del celular: música, preguntas y permisos de agentes desde el teléfono
- **Nuevo:** Reuniones, Portapapeles y notch lateral renovados
- **Nuevo:** Dictado con Whisper local opcional
- **Nuevo:** Tema claro para Atic Code, Ajustes y Reuniones

## 0.4.44 — 2026-10-02

- **Nuevo:** La isla se abre como un solo objeto que se estira
- **Nuevo:** Oculta la letra y el texto de los agentes en reposo
- **Nuevo:** Flip permite dibujar encima, textos sin fondo y checklist con saltos
- **Corregido:** La rueda flotante ya no se abre y cierra sola
- **Corregido:** El notch se acopla arriba en monitores de distinto alto
- **Corregido:** Los subagentes ya no se superponen en la Pizarra

## 0.4.43 — 2026-09-28

- **Corregido:** En macOS la pill funciona con un Space por pantalla
- **Corregido:** En macOS el soltar en la Pizarra cae bajo el cursor y acepta texto del historial
- **Corregido:** En macOS el dictado va a la consola de Atic y no a la app externa

## 0.4.42 — 2026-09-27

- **Nuevo:** Consola de administrador y Flip más completo
- **Corregido:** Arreglos de ventanas

## 0.4.41 — 2026-09-26

- **Nuevo:** Pizarra con archivos y espacios que se guardan solos
- **Nuevo:** El lanzador convierte divisas en la calculadora
- **Nuevo:** Consolas con WebGL y selección con zoom
- **Mejorado:** La pill gasta menos con animaciones más ligeras y menos lecturas del sistema
- **Corregido:** El dictado devuelve el texto a la consola donde empezaste
- **Corregido:** Las respuestas del chat tienen mejor formato y los hilos largos pesan menos

## 0.4.40 — 2026-09-25

- **Corregido:** En macOS el OCR ya no espera minutos a que Vision compile

## 0.4.39 — 2026-09-24

- **Nuevo:** Compatibilidad con macOS
- **Nuevo:** Pizarra de consolas con sesiones que se retoman y actividad en vivo
- **Nuevo:** Vistazos, personalizar la pill, elegir su hogar y re-acople
- **Mejorado:** Windows enlaza más rápido y arreglos de volumen, clics e íconos
- **Corregido:** El audio muestra una fila por app y el volumen afecta todas sus sesiones
- **Corregido:** El arrastre del portapapeles funciona con terminales web e imágenes grandes

## 0.4.38 — 2026-09-21

- **Corregido:** El historial ya no se sale de su caja y los logos de agentes caben

## 0.4.37 — 2026-09-21

- **Nuevo:** Cuotas por agente y arrastre desde el historial
- **Nuevo:** Recursos del sistema reales, avisos con umbral, café y volumen con la rueda
- **Nuevo:** Ventana dedicada de consolas
- **Mejorado:** Las cinco herramientas de la ventana se rediseñan con un estilo unificado
- **Mejorado:** Más contraste en los textos secundarios de todas las paletas
- **Corregido:** Servidores MCP más robustos

## 0.4.36 — 2026-09-13

- **Nuevo:** Pantalla de permisos en macOS
- **Corregido:** Se arregla el audio en macOS y una captura que se colgaba

## 0.4.35 — 2026-09-13

- **Nuevo:** Atic llega a macOS: captura, Pizarra, estante, OCR y audio del sistema
- **Nuevo:** Lanzador de macOS con apps del sistema, íconos y Recientes
- **Nuevo:** La pill puede cruzar al monitor vecino
- **Mejorado:** Los atajos se capturan por tecla física y avisan si están repetidos
- **Corregido:** El estante ya no activa Atic al arrastrarlo

## 0.4.34 — 2026-09-12

- **Nuevo:** El lanzador crece en el sitio, calcula y cierra aplicaciones

## 0.4.33 — 2026-09-12

- **Mejorado:** Atic usa menos recursos en reposo al liberar la captura precargada

## 0.4.32 — 2026-09-11

- **Mejorado:** Mejoras internas y de estabilidad

## 0.4.31 — 2026-09-11

- **Corregido:** El botón de confirmar de la consola se puede pulsar en el overlay

## 0.4.30 — 2026-09-10

- **Corregido:** El chip de actualización recibe el clic con la pill acoplada
- **Corregido:** La captura ya no espera una vista previa que nunca llega

## 0.4.29 — 2026-09-10

- **Nuevo:** Orquesta agentes entre sí por MCP y conéctalos desde la interfaz
- **Nuevo:** Panel de uso de los agentes
- **Nuevo:** El reverso de la ventana es un tablero de notas que puedes exportar con fotos y texto
- **Nuevo:** Registra los atajos globales desde Ajustes
- **Mejorado:** Bandeja de capturas y vista previa del portapapeles rehechas

## 0.4.28 — 2026-09-05

- **Nuevo:** Cuentagotas para tomar colores de la pantalla
- **Nuevo:** El historial del portapapeles muestra los colores copiados

## 0.4.27 — 2026-09-03

- **Mejorado:** Mejoras en la pill, el overlay y el anclaje de las herramientas

## 0.4.26 — 2026-09-01

- **Nuevo:** Elige el tamaño del overlay en Ajustes
- **Corregido:** Se corrige el zoom del overlay

## 0.4.25 — 2026-09-01

- **Mejorado:** Pulido general de agentes, pill y overlay

## 0.4.24 — 2026-08-30

- **Nuevo:** La pill muestra los cupos de uso de los agentes
- **Mejorado:** Chip de agentes y hogar de la pill mejorados

## 0.4.23 — 2026-08-28

- **Mejorado:** Ventana principal, pill y temas rehechos

## 0.4.22 — 2026-08-27

- **Nuevo:** Vista previa del portapapeles, arrastre de texto y rueda que se puede mover

## 0.4.21 — 2026-08-26

- **Nuevo:** Consolas embebidas con tantas pestañas como quieras, en vez de dos fijas
- **Nuevo:** Lanzador de agentes compacto que crece desde la pill, con los íconos de cada marca
- **Nuevo:** Elige la carpeta de inicio de cada consola de agentes
- **Mejorado:** Mejores splits, paneles y atajos en las consolas de agentes

## 0.4.20 — 2026-08-21

- **Mejorado:** La interfaz usa español de Chile

## 0.4.19 — 2026-08-20

- **Corregido:** Buscar actualizaciones ya no deja la ventana principal sin responder

## 0.4.18 — 2026-08-20

- **Corregido:** La rueda de herramientas vuelve a moverse con la rueda del mouse

## 0.4.17 — 2026-08-20

- **Corregido:** La ventana principal volvía a responder a los clics tras mover la pill

## 0.4.16 — 2026-08-20

- **Corregido:** La rueda del mouse funciona en la ventana principal fuera de la pill

## 0.4.15 — 2026-08-20

- **Corregido:** Adjuntar un archivo ya no corta el giro del selector

## 0.4.14 — 2026-08-20

- **Corregido:** El overlay ya no tapa la ventana con pantallas de distinto DPI

## 0.4.13 — 2026-08-20

- **Nuevo:** Atic sigue el idioma del sistema
- **Corregido:** El overlay se coloca bien al despertar el equipo

## 0.4.12 — 2026-08-20

- **Corregido:** Puedes descargar la actualización aunque la app siga buscando

## 0.4.11 — 2026-08-20

- **Nuevo:** Elige el idioma de la interfaz
- **Mejorado:** Los resúmenes y transcripciones muestran errores más claros

## 0.4.10 — 2026-08-19

- **Mejorado:** El aviso de actualización y la sección Acerca de muestran mejor el estado de la descarga

## 0.4.9 — 2026-08-19

- **Mejorado:** Mejoras internas y de estabilidad

## 0.4.8 — 2026-08-19

- **Mejorado:** Mejoras internas y de estabilidad

## 0.4.7 — 2026-08-19

- **Corregido:** El tutorial se cierra si se abrió por accidente
- **Corregido:** Se cubre el segundo monitor al arrancar con Windows

## 0.4.6 — 2026-08-19

- **Nuevo:** Reuniones transcribe con Groq y suma una pestaña de Información
- **Nuevo:** Actualizaciones automáticas de la app

## 0.4.5 — 2026-08-18

- **Nuevo:** Dibuja sobre la pantalla congelada o sobre una captura recién tomada
- **Nuevo:** La pill muestra el estado de los agentes que corren en su terminal
- **Mejorado:** El lanzador aparece como un círculo

## 0.4.4 — 2026-08-17

- **Corregido:** El atajo de captura ya no cierra el portapapeles ni la pill
- **Corregido:** Las herramientas flotantes ya no se recortan en los bordes

## 0.4.3 — 2026-08-17

- **Corregido:** La selección de captura ya no se desfasa con varias pantallas de distinto DPI

## 0.4.2 — 2026-08-17

- **Corregido:** El overlay de captura cubre todo el escritorio en vez de recortarse a una pantalla

## 0.4.1 — 2026-08-16

- **Mejorado:** La pill puede llegar hasta el borde superior de la pantalla
- **Mejorado:** El overlay sigue el tema claro
- **Corregido:** Al invocar desde el borde se restaura el tamaño flotante

## 0.4.0 — 2026-08-16

- **Nuevo:** La pill se acopla a los bordes de la pantalla como una isla de herramientas
- **Mejorado:** El efecto líquido es más viscoso y la rueda ya no rebota
- **Corregido:** Arreglos en la captura y el portapapeles

## 0.3.2 — 2026-08-09

- **Nuevo:** Arrastra un elemento del portapapeles directo al compositor de los agentes
- **Nuevo:** Conexiones SSH y mejoras del lanzador
- **Mejorado:** Arrastrar las herramientas flotantes es más fluido y el portapapeles carga más rápido

## 0.3.1 — 2026-08-09

- **Mejorado:** Mejor experiencia con los agentes y más pulido en el overlay
- **Corregido:** Los clics ya no se pierden en las herramientas flotantes del overlay

## 0.3.0 — 2026-08-06

- **Nuevo:** Interfaz renovada con una pill nueva dibujada con efecto líquido
- **Nuevo:** Búsqueda global y ajustes reorganizados en secciones
- **Nuevo:** Agentes de IA en la pill, empezando por Claude Code, con permisos y comandos de barra
- **Nuevo:** Varias notas en Textos y sonidos elegibles por acción
- **Nuevo:** Una bienvenida con un último paso que te enseña a usar la app
- **Nuevo:** Instalador de Windows propio

## 0.2.0 — 2026-07-19

- **Nuevo:** Historial del portapapeles en la pill, que vuela hasta ti al invocarlo
