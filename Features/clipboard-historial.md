# Historial de portapapeles

**Estado:** `hecho`

## Resumen

Historial de texto e imágenes del clipboard, con pegado a la app que tenía
foco, inserción en agentes, cola “pegar después” y arrastre hacia otras
ventanas.

## Cómo se usa

- Abrir historial desde la pill (rueda / atajo): sale un **float independiente**
  en el overlay (`.float-emerge`), fundido a la pill por liquid. La barra de la
  pill **no crece**.
- Clic para pegar (Ctrl+V al destino, o insert interno si agentes es el
  destino sin ventana externa). Pegar cierra el float.
- Cola de pegado cuando no hay destino externo listo.
- Dictado y capturas se integran con el mismo sistema de foco/destino.

## Qué no guarda

El historial vive en `data/clipboard/history.json`, en texto plano y en el
disco del usuario. Dos cosas lo acotan:

- **Se puede apagar** desde Ajustes (`clipboard_history`, encendido por
  defecto). Apagado, el vigilante deja de mirar el portapapeles en la vuelta
  siguiente, sin reiniciar la app.
- **Respeta los marcadores de contenido efímero de Windows**
  (`ExcludeClipboardContentFromMonitorProcessing` y
  `CanIncludeInClipboardHistory`). Los ponen los gestores de contraseñas
  justamente para que su copia no sobreviva al pegado; `arboard` no los mira,
  así que la comprobación es propia (`clipboard_is_sensitive`).

## Código

- [`apps/desktop/src-tauri/src/clipboard_history.rs`](../apps/desktop/src-tauri/src/clipboard_history.rs) — store + `show_clipboard_window`
- [`apps/desktop/src-tauri/src/panel_float.rs`](../apps/desktop/src-tauri/src/panel_float.rs) — ancla genérica a la pill
- [`apps/desktop/src/lib/surfaces/overlay/clipboard/ClipboardFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/clipboard/ClipboardFloat.svelte)
- [`apps/desktop/src-tauri/src/paste_queue.rs`](../apps/desktop/src-tauri/src/paste_queue.rs)
- [`apps/desktop/src/lib/ClipboardHistoryList.svelte`](../apps/desktop/src/lib/ClipboardHistoryList.svelte)

## Relacionado

- [liquid.md](liquid.md)
- [pill-shell.md](pill-shell.md)
- [agentes.md](agentes.md)
