# Acciones de sistema

**Estado:** `parcial` (Windows; macOS pendiente)

## Resumen

Acciones del sistema operativo que se invocan desde el launcher como resultados
propios: bloquear la sesión, suspender, silenciar la salida de audio y vaciar la
papelera. Existen para no salir de Atic cuando el flujo es «una tecla y listo».
Son Windows-first porque cada una depende de una API del SO.

## Cómo se usa

- `Ctrl+Space` → escribir `bloquear`, `suspender`, `silenciar` o `vaciar` → Enter.
- También se pueden fijar como favoritos del launcher (funcionan como cualquier
  acción interna).

| Acción | Qué hace | Nota |
|---|---|---|
| Bloquear pantalla | `LockWorkStation` (equivale a Win+L) | reversible con la contraseña |
| Suspender | `SetSuspendState` sin forzar | no hiberna |
| Silenciar o activar sonido | `SendInput` con `VK_VOLUME_MUTE` | la misma tecla del teclado |
| Vaciar papelera | `SHEmptyRecycleBinW` | **conserva el diálogo de Windows**: es irreversible |
| Cerrar todas las apps | `WM_CLOSE` a las ventanas visibles de apps de usuario | pide guardar lo que corresponda; no toca el shell ni Atic |

Política: **nunca forzar acciones destructivas**. Por eso la papelera pregunta
(no se le pasa `SHERB_NOCONFIRMATION`) y no existe force quit: cerrar una app es
mandarle `WM_CLOSE`, como el aspa de su ventana, y la app decide.

## Código

- [`apps/desktop/src-tauri/src/system_actions.rs`](../apps/desktop/src-tauri/src/system_actions.rs) — las cuatro acciones (Windows + stubs por plataforma)
- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — `builtin_actions` (ids `action:sys-*`) y `run_action`
- [`apps/desktop/src-tauri/src/launcher_recents.rs`](../apps/desktop/src-tauri/src/launcher_recents.rs) — ventanas visibles de apps de usuario (`close_user_windows`)

## Pendiente / siguiente

- [ ] macOS: bloquear/suspender y mute tienen equivalentes propios
- [ ] Tema claro/oscuro y archivos ocultos (escritura en registro + `WM_SETTINGCHANGE`)
- [ ] Bluetooth (WinRT `Windows.Devices.Radios`)
- [ ] Reiniciar / apagar con confirmación propia
- [ ] Confirmación propia para acciones irreversibles, en vez de depender del SO

## Relacionado

- [launcher-spotlight.md](launcher-spotlight.md)
- [pill-shell.md](pill-shell.md)
