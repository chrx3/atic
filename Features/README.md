# Features — catálogo vivo de Atic

Inventario de capacidades de producto: qué hay, qué está a medias y qué
queremos. Cada ficha se actualiza cuando la feature cambia.

En el instalador actual la herramienta de **agentes** está oculta en la UI;
el código y su ficha siguen en el repo.

Los planes técnicos largos siguen en [`docs/`](../docs/). Acá no se duplica
implementación: se resume, se apunta al código y al plan si existe.

---

## Qué es Atic hoy

Una caja de herramientas de escritorio **local-first**: el audio se graba en el
propio PC y Whisper local permite transcribir sin subirlo. Si el usuario elige
Groq para dictado o transcripción en vivo, el audio correspondiente sí se envía
a Groq; los proveedores de resumen reciben únicamente el texto transcrito.

No tiene una ventana grande que haya que ir a buscar. Vive en tres lugares:

- **La pill** — barra flotante siempre encima, movible, que se expande según lo
  que esté pasando y despliega una rueda con las siete herramientas.
- **La bandeja del sistema** — mostrar/ocultar y salir.
- **Nueve atajos globales** — cubren los flujos principales y permiten abrir el
  resto de herramientas mediante la rueda o el launcher, sin buscar la ventana
  principal.

Encima de eso hay una ventana principal (biblioteca de grabaciones, transcript,
resumen, ajustes) para el trabajo que no cabe en la pill.

---

## Las siete herramientas

Las que ve el usuario en la rueda, en el orden de [`tools.ts`](../apps/desktop/src/lib/tools.ts).

### 1. Reuniones — grabar y resumir

El flujo completo del producto original: graba, transcribe local y resume.

- Graba **micrófono + audio del sistema** en pistas separadas. Mantenerlas
  separadas da diarización gratis: pista mic = "yo", pista sistema = "los demás".
- **Detección de llamadas** que solo *sugiere* grabar — Meet en navegador,
  procesos conocidos, mic en uso por otra app. Nunca graba solo.
- **Modo parlantes** para cuando no hay auriculares.
- Transcripción con **Whisper local** (modelos descargados bajo demanda, no van
  en el instalador) o **Groq** (misma API key que el dictado; el audio sale del
  PC), con opción de transcripción **en vivo**.
- También transcribe **audio importado**, no solo lo que grabó.
- Resumen **BYOK** con plantillas editables: Claude, Ollama, o cualquier
  endpoint OpenAI-compatible (OpenAI, OpenRouter, Groq, MiniMax, Custom).
- Envío por **SMTP** o abrir borrador **`mailto:`**.

→ [grabacion-reuniones.md](grabacion-reuniones.md) · [transcripcion-resumen.md](transcripcion-resumen.md)

### 2. Dictado — voz a texto en cualquier app

- Atajo global, en modo **toggle o push-to-talk**.
- Graba el mic, transcribe (Whisper local o Groq) y **pega el texto donde
  estabas escribiendo** — Chrome, Word, la terminal, lo que sea.
- Al elegir Groq, el audio del dictado se envía a su API para transcribirlo.
- Si no hay destino externo con foco, inserta en el compositor de agentes.
  Con la burbuja de agentes abierta, la ventana externa tiene prioridad.

→ [dictado.md](dictado.md)

### 3. Clipboard — historial local

- Historial de **texto e imágenes**, en el disco del usuario.
- Clic para pegar a la app que tenía el foco; arrastre hacia otras ventanas.
- **Cola "pegar después"** cuando no hay destino externo listo.
- Se apaga desde Ajustes, y **nunca archiva lo que un gestor de contraseñas
  marcó como efímero** (respeta los marcadores de Windows que `arboard` ignora).

→ [clipboard-historial.md](clipboard-historial.md)

### 4. Textos — los que escribes siempre

- Textos guardados **a mano y a propósito** — esa es la diferencia con
  Clipboard, que se llena solo con lo que copias.
- Más un **bloc para notas sueltas** (varias notas, no un único bloque).
- Pegado al destino con el mismo sistema de foco que dictado y clipboard.

→ [snippets.md](snippets.md)

### 5. Agentes — consola de IA con interfaz

- **Claude Code, Codex, OpenCode y Cursor** conversando dentro de Atic, con sus
  herramientas y sus permisos reales.
- **Atic no autentica nada**: se cuelga del CLI que el usuario ya instaló y
  logueó. No hay que meter otra API key.
- Modelo, esfuerzo, Fast (Cursor) y modo de permisos **se recuerdan por
  backend** al reabrir.
- El **escudo de permisos dice el riesgo por su forma**, no solo por su texto.
- Adjuntar capturas y archivos al compositor; dictar dentro de él.
- Las conversaciones **se guardan** en `atic.db3` y se pueden releer.
- La burbuja cuelga de la pill y **se funde con ella** cuando están cerca.

→ [agentes.md](agentes.md) · [liquid.md](liquid.md)

### 6. Capturas — recortes de pantalla

- **Región, ventana o monitor**, con overlay para delimitar.
- Van al portapapeles y a un **shelf flotante**.
- **Dibujar encima**: flechas, círculos, trazo libre y resaltador sobre la
  captura recién tomada, y de ahí al portapapeles con Enter. Guardar deja una
  captura nueva; el original no se toca.
- **Pizarra**: `Ctrl+Shift+X` congela la pantalla y deja marcarla ahí donde
  está, con las mismas herramientas. Esc la saca.
- Se pueden adjuntar directo al compositor de agentes.

→ [capturas.md](capturas.md)

### 7. Pizarra — marcar la pantalla

- `Ctrl/Cmd+Shift+X` **congela la pantalla** y la marcas ahí donde está, con
  las mismas herramientas del editor de capturas.
- Sobre la congelada y no sobre la viva: con el escritorio en movimiento, lo de
  abajo se corre y la marca deja de señalar lo que señalaba.
- Enter copia lo marcado; `Ctrl+Enter` lo guarda como captura. Esc la saca.

→ [capturas.md](capturas.md)

### Y además: el launcher

No está en la rueda pero es una herramienta más. Barra de búsqueda global tipo
Spotlight / Raycast: abre programas y acciones de Atic sin pasar por el buscador
de Windows. Indexa los `.lnk` del menú Inicio (Windows) y `/Applications`
(macOS), más las acciones internas como resultados de primera clase.

**No** es un índice de todo el disco tipo Everything.

→ [launcher-spotlight.md](launcher-spotlight.md)

---

## Atajos globales por defecto

Todos configurables en Ajustes. Valores de [`config.rs`](../crates/core/src/config.rs).

| Atajo | Qué hace |
|---|---|
| `Ctrl/Cmd+Shift+R` | Iniciar / detener grabación |
| `Ctrl/Cmd+Shift+D` | Dictado |
| `Ctrl/Cmd+Shift+V` | Historial de portapapeles |
| `Ctrl/Cmd+Shift+S` | Textos |
| `Ctrl/Cmd+Shift+4` | Captura de pantalla |
| `Ctrl/Cmd+Shift+X` | Dibujar sobre la pantalla |
| `Ctrl/Cmd+Shift+P` | Traer la pill al cursor |
| `Ctrl/Cmd+Space` | Launcher |
| `Alt+Z` | Rueda radial de la pill |

`Alt+Space` fue el default de la rueda hasta 0.2.0 y se cambió: es el menú de
ventana del SO, y registrarlo global lo mataba en todo Windows. La config vieja
se migra sola.

---

## Sonido

Cinco acciones suenan —grabación (inicio/fin), dictado (inicio/listo), captura—
y cada una tiene **voz elegible desde Ajustes** entre ocho timbres: grave,
pulso, cristal, madera, campana, cuerda, aire, digital. O `ninguno`, y esa
acción no suena.

El **gesto melódico lo define la acción** y no se toca (sube al iniciar, baja al
parar): es lo que hace que el sonido signifique algo sin mirar la pantalla. La
voz solo cambia color, registro y duración.

---

## Ajustes y primer uso

- Onboarding de primer arranque con **nota de consentimiento** para grabar.
- Dispositivos de audio, modelos de Whisper, backend de dictado, proveedores de
  resumen (BYOK), SMTP, sonidos, tema, atajos.
- **Autostart** opcional.
- **Buscar actualizaciones** desde Ajustes, más un chequeo discreto al arrancar.
- Las API keys y la contraseña SMTP van al **llavero del sistema**, nunca a
  `config.json`.

→ [ajustes-onboarding.md](ajustes-onboarding.md)

---

## Qué funciona en cada plataforma

| | Windows | macOS |
|---|---|---|
| Grabar micrófono | sí | sí |
| **Audio del sistema** | sí (WASAPI loopback) | **no** — fase 4 |
| Transcripción y resumen | sí | sí |
| Dictado y pegado | sí | sí |
| Capturas de pantalla | sí | no |
| Clipboard, textos, agentes | sí | sí |
| Launcher | sí (menú Inicio) | sí (`/Applications`) |

Windows es la plataforma completa. En macOS, grabar una reunión captura **solo
tu voz** — es la limitación conocida más grande del producto hoy.

---

## Lo que todavía no hace

Consolidado de los pendientes de cada ficha que el usuario llegaría a notar.

| Falta | Dónde |
|---|---|
| Audio del sistema en macOS (ScreenCaptureKit + permisos TCC) | [macos-audio-sistema.md](macos-audio-sistema.md) |
| UI para `cursor/ask_question` y `cursor/create_plan` — hoy se auto-responden | [agentes.md](agentes.md) |
| Paridad de UX entre backends de agentes (costos, modos, errores) | [agentes.md](agentes.md) |
| Aviso cuando el pegado del dictado falla y queda en cola | [dictado.md](dictado.md) |
| Feedback más claro cuando falla el micrófono | [dictado.md](dictado.md) |
| Retención temporal / configuración del límite fijo de 100 ítems del clipboard | [clipboard-historial.md](clipboard-historial.md) |
| Preferencias del launcher (raíces extra, exclusiones, favoritos) | [launcher-spotlight.md](launcher-spotlight.md) |
| Ranking por uso y fuzzy más fino en el launcher | [launcher-spotlight.md](launcher-spotlight.md) |
| Expansión de textos por trigger tipado | [snippets.md](snippets.md) |
| Capturas en macOS | [capturas.md](capturas.md) |
| Companion móvil | [companion-movil.md](companion-movil.md) |

---

## Dónde viven los datos

Todo local, bajo `%APPDATA%\ciat\atic\data\` (Windows) o
`~/Library/Application Support/ciat/atic/data/` (macOS): grabaciones,
transcripciones, resúmenes, capturas, historial de clipboard, logs de 7 días y
el índice SQLite. Los secretos van aparte, al llavero del sistema.

Detalle completo en el [README del repo](../README.md#privacidad).

---

## Índice de fichas

| Estado | Feature | Archivo |
|--------|---------|---------|
| hecho | Pill, tray, atajos y ventanas | [pill-shell.md](pill-shell.md) |
| parcial | Grabación de reuniones | [grabacion-reuniones.md](grabacion-reuniones.md) |
| hecho | Transcripción, resumen y correo | [transcripcion-resumen.md](transcripcion-resumen.md) |
| parcial | Dictado | [dictado.md](dictado.md) |
| hecho | Capturas de pantalla | [capturas.md](capturas.md) |
| hecho | Historial de portapapeles | [clipboard-historial.md](clipboard-historial.md) |
| hecho | Snippets | [snippets.md](snippets.md) |
| parcial | Agentes multi-proveedor | [agentes.md](agentes.md) |
| en curso | Sistema líquido (transversal) | [liquid.md](liquid.md) |
| hecho | Emergencia fused grow → separate (patrón) | [pill-liquid-emerge.md](pill-liquid-emerge.md) |
| hecho | Ajustes y onboarding | [ajustes-onboarding.md](ajustes-onboarding.md) |
| parcial | Audio del sistema en macOS | [macos-audio-sistema.md](macos-audio-sistema.md) |
| hecho | Launcher tipo Spotlight | [launcher-spotlight.md](launcher-spotlight.md) |
| idea | Companion móvil | [companion-movil.md](companion-movil.md) |
| idea | Hosts SSH para agentes remotos | [ssh-remote-hosts.md](ssh-remote-hosts.md) |

## Cómo mantener esto

1. Feature nueva → copia [`_template.md`](_template.md) a `<slug>.md` y
   rellena.
2. Cambió el comportamiento → edita la ficha, esta tabla y —si el usuario lo
   nota— la sección de arriba que corresponda.
3. Estados: `hecho` | `parcial` | `en curso` | `idea`.
