# Progreso: GPUI sin Tauri en Windows

Avance de `docs/PLAN_MIGRACION_GPUI.md` en la rama `gpui-standalone`
(worktree `%LOCALAPPDATA%\atic-wt`). Se actualiza al cerrar cada paso.

Validación:

- Workspace: `CARGO_TARGET_DIR=$LOCALAPPDATA/atic-ct cargo check --locked -p <crate>`.
- Pill: `cd prototypes/pill-gpui && CARGO_TARGET_DIR=$LOCALAPPDATA/atic-gpui cargo check`
  (no `cargo build`: el `pill-gpui.exe` de esa carpeta está corriendo).

## Fase 0

| Paso | Estado |
| --- | --- |
| Datos en `AppDirs` con migración desde `%LOCALAPPDATA%\atic-gpui` | pendiente |
| Log a archivo con rotación y pánicos | pendiente |
| Quitar los `.expect()` de `main()` | pendiente |
| Instancia única | pendiente |
| Mecanismo de traducción es/en | pendiente |
| Capa de plataforma | pendiente |

## Fase 1

| Paso | Estado |
| --- | --- |
| Portapapeles y `paste_queue` a un crate | pendiente |
| Servicios de fondo (retention, fx, mouse_bindings, reuniones, phone_sync) | pendiente |
| Hub MCP, hooks y watchers de agentes | pendiente |
| Ícono de bandeja e inicio automático | pendiente |
| Tauri en modo «solo ventana principal» | pendiente |
| Instalador | pendiente |

## Para probar a mano

(nada todavía)

## Trabas y decisiones

(nada todavía)
