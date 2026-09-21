# Acciones de sistema

**Estado:** `hecho`

## Resumen

Acciones del sistema operativo que se invocan desde el launcher como resultados
propios, y también desde la fila rápida del [panel de sistema](sistema.md):
bloquear la sesión, suspender, silenciar la salida de audio y vaciar la
papelera. Existen para no salir de Atic cuando el flujo es «una tecla y listo».

## Cómo se usa

- `Ctrl+Space` → escribir `bloquear`, `suspender`, `silenciar` o `vaciar` → Enter.
- También se pueden fijar como favoritos del launcher (funcionan como cualquier
  acción interna).
- En la pill, la cara **Sistema** tiene los mismos atajos en la fila de arriba.

| Acción | Windows | macOS |
|---|---|---|
| Bloquear pantalla | `LockWorkStation` (Win+L) | `SACLockScreenImmediate`, o `pmset displaysleepnow` |
| Suspender | `SetSuspendState` sin forzar | `pmset sleepnow` |
| Silenciar o activar sonido | tecla `VK_VOLUME_MUTE` | mute del dispositivo de salida (CoreAudio) |
| Vaciar papelera | `SHEmptyRecycleBinW` (conserva el diálogo del SO) | Finder vacía la papelera (también pregunta) |
| Cerrar todas las apps | `WM_CLOSE` a ventanas de usuario | `terminate` de apps regulares |

Política: **nunca forzar acciones destructivas** desde el launcher. El force-quit
de una app vive solo en el panel de sistema, detrás de un diálogo propio.

## Código

- [`apps/desktop/src-tauri/src/system_actions.rs`](../apps/desktop/src-tauri/src/system_actions.rs) — lock / sleep / mute / trash
- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — `builtin_actions` (ids `action:sys-*`) y `run_action`
- [`apps/desktop/src-tauri/src/launcher_recents.rs`](../apps/desktop/src-tauri/src/launcher_recents.rs) — ventanas visibles de apps de usuario (`close_user_windows`)
- [`apps/desktop/src-tauri/src/system_control/`](../apps/desktop/src-tauri/src/system_control/) — panel: snapshot, volumen, brillo y cierre por app

## Pendiente / siguiente

- [ ] Tema claro/oscuro y archivos ocultos (escritura en registro + `WM_SETTINGCHANGE`)
- [ ] Bluetooth (WinRT `Windows.Devices.Radios`)
- [ ] Reiniciar / apagar con confirmación propia
- [ ] Confirmación propia para acciones irreversibles, en vez de depender del SO

## Relacionado

- [sistema.md](sistema.md)
- [launcher-spotlight.md](launcher-spotlight.md)
- [pill-shell.md](pill-shell.md)
