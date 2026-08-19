# Transcripción, resumen y correo

**Estado:** `hecho`

## Resumen

Transcribe grabaciones (y audio importado) con Whisper local o Groq (BYOK),
opcionalmente en vivo. Genera resúmenes con proveedores BYOK y permite enviar
o abrir borrador por correo.

## Cómo se usa

- Tras grabar: transcribir en la app principal (Local o Groq).
- Local no sale del PC. Groq trocea el audio (~10 min) y pide marcas de tiempo.
- Live transcription si está activada en ajustes, con Whisper local o Groq.
- Si se selecciona Groq, el audio se envía a su API. Los proveedores de resumen
  reciben texto, no audio.
- Resumen con Claude, Ollama, OpenAI-compat (OpenRouter, Groq, etc.).
- Envío SMTP o `mailto:` según config.

## Código

- [`crates/transcribe/`](../crates/transcribe/) — Whisper, modelos, modos
- [`crates/summarize/`](../crates/summarize/) — BYOK
- [`crates/mailer/`](../crates/mailer/) — SMTP / mailto
- [`apps/desktop/src-tauri/src/transcription.rs`](../apps/desktop/src-tauri/src/transcription.rs)
- [`apps/desktop/src-tauri/src/summarization.rs`](../apps/desktop/src-tauri/src/summarization.rs)
- [`apps/desktop/src-tauri/src/live.rs`](../apps/desktop/src-tauri/src/live.rs)
- [`apps/desktop/src-tauri/src/mail.rs`](../apps/desktop/src-tauri/src/mail.rs)
- [`apps/desktop/src-tauri/src/import.rs`](../apps/desktop/src-tauri/src/import.rs)

## Pendiente / siguiente

- [ ] Mejoras de UX de cola / progreso si el feedback de usuarios lo pide
- [ ] GPU por plataforma (Metal/CUDA/Vulkan) documentada en macOS/Windows

## Relacionado

- [grabacion-reuniones.md](grabacion-reuniones.md)
- [dictado.md](dictado.md)
