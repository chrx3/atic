# Historial de portapapeles

**Estado:** `hecho`

## Resumen

Historial de texto e imágenes del clipboard, con pegado a la app que tenía
foco, inserción en agentes, cola “pegar después” y arrastre hacia otras
ventanas.

## Cómo se usa

- Abrir historial desde la pill.
- Clic para pegar (Ctrl+V al destino, o insert interno si agentes es el
  destino sin ventana externa).
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

- [`apps/desktop/src-tauri/src/clipboard_history.rs`](../apps/desktop/src-tauri/src/clipboard_history.rs)
- [`apps/desktop/src-tauri/src/paste_queue.rs`](../apps/desktop/src-tauri/src/paste_queue.rs)
- [`apps/desktop/src/lib/ClipboardHistoryList.svelte`](../apps/desktop/src/lib/ClipboardHistoryList.svelte)

## Pendiente / siguiente

- [ ] Revisar UX cuando agentes está abierto y el usuario quiere pegar afuera
      (prioridad externa ya aplicada en dictado; alinear clipboard si hace falta)
- [ ] Retención / límites de ítems si crece mucho el historial

## Relacionado

- [dictado.md](dictado.md)
- [capturas.md](capturas.md)
- [agentes.md](agentes.md)
