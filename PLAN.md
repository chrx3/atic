# Plan del proyecto — Atic

Asistente de reuniones/llamadas: detecta llamadas, graba el audio del PC (sistema + micrófono), transcribe, genera resúmenes con IA y permite enviarlos por correo. Multiplataforma (Windows y macOS primero, móvil después), liviano, bonito y no intrusivo.

## Decisiones ya tomadas

- **Enfoque híbrido con prioridad local**: Whisper permite transcribir sin que el audio salga del PC. Si el usuario selecciona Groq para dictado o transcripción en vivo, ese audio sí se envía a su API. Para resumir en la nube solo se envía el transcript; Ollama mantiene también esa etapa local.
- **Fase 1 solo escritorio** (Windows + macOS). La arquitectura queda preparada para móvil (Tauri 2 soporta iOS/Android), pero no se desarrolla aún.
- **Un solo proyecto/repositorio** con workspace de Cargo.

## Stack propuesto

| Capa | Elección | Por qué |
|---|---|---|
| Shell de app | **Tauri 2** | Binarios chicos (~10 MB), backend 100% Rust, un solo proyecto que compila a Windows/macOS/Linux y en el futuro iOS/Android. Permite ventanas transparentes/always-on-top para la "pill". |
| UI | **Svelte 5 + Tailwind CSS** | Liviano, rápido de desarrollar, fácil lograr una UI pulida. (React también sirve si lo prefieres.) |
| Captura de audio | `cpal` (WASAPI loopback en Windows) + bindings ScreenCaptureKit/Core Audio en macOS | Es la parte más "nativa" del proyecto; se aísla en su propio crate. |
| Transcripción local | `whisper-rs` (bindings de whisper.cpp) | Corre en CPU/GPU local, modelos descargables bajo demanda. |
| Resumen IA | Claude API (`claude-opus-4-8`) vía `reqwest`; opción local vía Ollama | Rust no tiene SDK oficial de Anthropic → HTTP directo contra `/v1/messages`. |
| Correo | `lettre` (SMTP) + fallback `mailto:` | Envío directo configurando SMTP, o abrir el cliente de correo del usuario. |
| Almacenamiento | SQLite (`rusqlite`) + archivos de audio en app-data; API keys en `keyring` | Simple, sin servidor, portable a móvil. |

## Estructura del workspace

```
atic/
├── Cargo.toml                # workspace
├── crates/
│   ├── core/                 # dominio: sesiones, grabaciones, resúmenes, storage (SQLite), config
│   ├── audio/                # captura: mic + loopback del sistema, mezcla, escritura WAV/FLAC, VAD
│   ├── transcribe/           # trait Transcriber + backends: whisper-rs (local), cloud (opcional)
│   ├── summarize/            # trait Summarizer + backends: Claude API, Ollama (local)
│   └── mailer/               # envío SMTP (lettre) y generación de borradores mailto:
└── apps/
    └── desktop/              # app Tauri 2: ventana principal + pill flotante + tray + shortcuts
        ├── src-tauri/        # comandos Tauri, plugins, empaquetado
        └── src/              # frontend Svelte + Tailwind
```

Los traits `Transcriber` y `Summarizer` son la clave de la escalabilidad: cambiar de local a nube (o agregar un backend nuevo) no toca el resto del código, y los crates `core/transcribe/summarize` se reutilizan tal cual en una futura app móvil.

## Captura de audio por plataforma

- **Windows**: WASAPI loopback (capturar lo que suena por los parlantes/audífonos) + micrófono, como **dos pistas separadas**. Mantenerlas separadas da diarización gratis: pista mic = "yo", pista sistema = "los demás".
- **macOS**: el micrófono es directo, pero el audio del sistema requiere ScreenCaptureKit (macOS 13+) o Core Audio taps (macOS 14.4+), con permisos TCC (micrófono + grabación de audio del sistema). Es el punto técnico más delicado del proyecto → fase propia.
- Formato: WAV durante la grabación, compresión a FLAC/Opus al finalizar.

## Detección de llamadas

No hace falta integrarse con Zoom/Teams/Meet: el loopback captura todo el audio del sistema igual. La "detección" es solo para **sugerir** iniciar grabación:

1. Detectar que el micrófono está en uso por otra app (Windows: sesiones de audio activas vía WASAPI; macOS: Core Audio).
2. Complementar con detección de procesos conocidos (Teams, Zoom, Slack, navegador).
3. Al detectar → la pill pulsa / notificación "¿Grabar esta llamada?". Nunca grabar automáticamente sin confirmación (tema legal, ver Riesgos).

## Transcripción (local primero)

- `trait Transcriber { async fn transcribe(&self, audio: &AudioSession) -> Transcript; }`
- **Backend por defecto**: `whisper-rs` con modelos GGUF descargados bajo demanda (base ~150 MB para empezar, small/medium opcionales para más calidad). La app se mantiene liviana: no se empaquetan modelos.
- **Backend nube (opcional, off por defecto)**: API de transcripción (p. ej. OpenAI Whisper API o Deepgram) para equipos sin potencia local.
- El transcript guarda hablante ("yo"/"otros" por pista), timestamps y texto.

## Resumen con IA (híbrido)

- `trait Summarizer { async fn summarize(&self, t: &Transcript, template: &Template) -> Summary; }`
- **Backend nube (por defecto para calidad)**: Claude API con `claude-opus-4-8`, HTTP directo con `reqwest` (streaming SSE para mostrar el resumen mientras se genera). Solo se envía **texto** (el transcript), nunca audio. API key del usuario guardada en `keyring`.
- **Backend local (privacidad total)**: Ollama si está instalado (detectarlo y ofrecerlo en config).
- Plantillas de resumen: minuta ejecutiva, acuerdos y tareas (con responsables), correo de seguimiento listo para enviar. Editables antes de enviar.

## Correo

- Fase inicial: **envío manual** — el usuario revisa/edita el resumen, escribe los destinatarios y envía.
- Backends: SMTP del usuario vía `lettre`, o abrir borrador `mailto:` en su cliente.
- Futuro: detección de participantes vía integración con calendario (Google/Outlook) para pre-llenar destinatarios.

## UX

- **Pill flotante**: ventana Tauri sin bordes, transparente, always-on-top, arrastrable. Colapsada ~56 px (punto de estado); al hover se expande: botón grabar/detener, timer, nivel de audio. Ocultable desde el tray.
- **Shortcut global** (`tauri-plugin-global-shortcut`): p. ej. `Ctrl+Shift+R` inicia/detiene grabación desde cualquier app.
- **Tray + autostart** (`tauri-plugin-autostart`): la app vive en la bandeja, no estorba.
- **Ventana principal**: biblioteca de grabaciones → detalle con audio, transcript y resumen editable → botón "Enviar por correo".
- **Onboarding**: asistente de permisos (crítico en macOS), selección de idioma, descarga del modelo Whisper, config opcional de API key/SMTP.
- Estados claros siempre: grabando (rojo pulsante), transcribiendo (progreso), resumen listo, error.

## Fases

| Fase | Entregable | Contenido | Estado |
|---|---|---|---|
| 0 | Esqueleto | Workspace Cargo + app Tauri 2 con ventana principal, tray y shortcut global. CI básico (fmt, clippy, test, build Win/mac). | Hecho |
| 1 | Grabadora (Windows) | Captura mic + loopback, guardar y reproducir grabaciones, pill flotante. **Primera versión usable.** | Hecho |
| 2 | Transcripción | whisper-rs + descarga de modelos, vista de transcript con hablantes. | Hecho |
| 3 | Resumen + correo | BYOK multi-proveedor (Claude, Ollama, OpenAI-compat), plantillas, editor, SMTP/mailto. **Producto completo en Windows.** | Hecho |
| 4 | macOS | Audio del sistema (ScreenCaptureKit/CA taps), permisos TCC, firma y notarización. | Andamiaje (Info.plist + stub; falta captura real — requiere Mac) |
| 5 | Detección de llamadas | Procesos + títulos de ventana (Meet) + mic-en-uso WASAPI; pill pulsante; beep; autostart; onboarding legal. | Hecho (Windows) |
| 6 (futuro) | Móvil companion | Tauri 2 iOS/Android: ver/reenviar resúmenes y grabar reuniones presenciales por micrófono. **No graba audio de llamadas del sistema** (limitación de OS). | Documentado (`docs/MOBILE.md`) |

## Riesgos y consideraciones

- **Legal**: grabar llamadas requiere consentimiento en la mayoría de jurisdicciones (en Chile, sí). Mitigación: nunca grabar sin acción explícita del usuario, y opción de aviso/beep configurable. Incluir nota en el onboarding.
- **macOS audio del sistema** es lo más complejo técnicamente; por eso va en fase propia y el MVP es Windows.
- **Móvil**: iOS/Android **no permiten** grabar el audio de llamadas del sistema; la app móvil solo podrá grabar por micrófono (reuniones presenciales) y servir de visor. Expectativa fijada desde ya.
- **Peso**: los modelos Whisper se descargan bajo demanda; la app en sí queda bajo ~20 MB.
- **Claude API**: modelo por defecto `claude-opus-4-8`; usar streaming para respuestas largas y manejar errores 429/5xx con reintentos. Nunca loggear la API key.
- **BYOK**: resumen vía Claude, Ollama o cualquier endpoint OpenAI-compatible (presets: OpenAI, OpenRouter, Groq, MiniMax, Custom). Login OAuth con Codex/ChatGPT queda como seguimiento futuro (no es BYOK puro).
- **Fase 6 (móvil)**: companion para ver/reenviar resúmenes y grabar audio ambiental por micrófono. iOS/Android no permiten capturar el audio de llamadas VoIP del sistema; no se promete paridad con escritorio.
