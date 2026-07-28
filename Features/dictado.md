# Dictado

**Estado:** `parcial`

## Resumen

Dictado por atajo (toggle o push-to-talk): graba el mic, transcribe (Whisper
local o Groq) y pega el texto en el campo donde estabas escribiendo — o en el
compositor de agentes si no hay destino externo.

## Cómo se usa

- Atajo por defecto tipo `Ctrl+Shift+D` (configurable).
- Empieza a hablar; al soltar / toggle otra vez se transcribe y pega.
- Con la burbuja de agentes abierta, el pegado prioriza la ventana externa
  (Chrome, Word, etc.) si había foco ahí; si no, inserta en agentes.
- Backend: local o Groq según ajustes.

## Código

- [`apps/desktop/src-tauri/src/dictation.rs`](../apps/desktop/src-tauri/src/dictation.rs)
- [`apps/desktop/src-tauri/src/paste_queue.rs`](../apps/desktop/src-tauri/src/paste_queue.rs) — destino de pegado / cola
- [`apps/desktop/src-tauri/src/clipboard_history.rs`](../apps/desktop/src-tauri/src/clipboard_history.rs) — foco y HWND destino
- [`crates/transcribe/`](../crates/transcribe/) — dictado y Groq

## Pendiente / siguiente

- [ ] UI de confirmación si el pegado falla y queda en cola
- [ ] Seguir afinando foco en Electron/WebView2 sin mover el caret
- [ ] Sonidos / feedback más claros en error de mic

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [agentes.md](agentes.md)
- [pill-shell.md](pill-shell.md)
