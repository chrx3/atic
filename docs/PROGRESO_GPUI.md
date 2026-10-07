# Progreso: GPUI sin Tauri en Windows

Avance de `docs/PLAN_MIGRACION_GPUI.md` en la rama `gpui-standalone`
(worktree `%LOCALAPPDATA%\atic-wt`). Se actualiza al cerrar cada paso.

Validación:

- Workspace: `CARGO_TARGET_DIR=$LOCALAPPDATA/atic-ct cargo check -p <crate>`.
  El worktree necesita los sidecars en `apps/desktop/src-tauri/binaries/`
  (no se versionan): copiarlos del checkout principal.
- Pill: `cd prototypes/pill-gpui && CARGO_TARGET_DIR=$LOCALAPPDATA/atic-gpui cargo check`
  (no `cargo build`: el `pill-gpui.exe` de esa carpeta está corriendo).

## Fase 0: lista

| Paso | Estado | Commit |
| --- | --- | --- |
| Datos en `AppDirs` con migración desde `%LOCALAPPDATA%\atic-gpui` | listo | `64e2fbd` |
| Log a archivo con rotación y pánicos | listo | `a394c24` |
| Quitar los `.expect()` de `main()` | listo | `a394c24` |
| Instancia única | listo | `d335785` |
| Mecanismo de traducción es/en | listo | `d81f8f6` |
| Capa de plataforma | listo (falta `capture.rs`) | `83a7e3f` |

Detalles:

- **Datos:** `src/paths.rs`. Lo de la pill va en `%APPDATA%\ciat\atic\data\pill\`
  y las capturas en `data\captures\` (las mismas de Atic). La migración copia
  una sola vez lo que falte (marca `.migrated-from-atic-gpui`) y no borra el
  original.
- **Log:** `diagnostics.rs` pasó de Tauri a `atic-core` (feature
  `diagnostics`). Tauri escribe `atic.<fecha>.log` y la pill
  `pill.<fecha>.log` en `data\logs\`. Los avisos de GPUI entran al log; ya no
  hace falta `PILL_DEBUG` para verlos (sigue sirviendo para las trazas
  `debug()`).
- **Instancia única:** mutex `Local\atic-pill-gpui`; la segunda pill registra
  «ya hay una pill corriendo» y sale. Todavía no le avisa a la primera.
- **Traducción:** `src/i18n.rs`, `t("clave")` y `tf("clave", &[("var", &valor)])`.
  `assets/i18n/atic.{es,en}.json` se generan con
  `node prototypes/pill-gpui/scripts/i18n-export.mjs` (1.678 claves) y
  `pill.{es,en}.json` son a mano. Solo 2 textos de la pill pasaron a claves;
  el resto es la Fase 3.
- **Plataforma:** `src/platform/` con el contrato documentado y lo de Windows
  en `platform/win32/` (overlay, vidrio, pegar, atajos, arrastre, OCR,
  privacidad, apps abiertas).

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

1. Compilar la pill del worktree y abrirla (cerrando antes la que corre):
   que aparezca donde estaba (usa `home.txt` migrado) y con tus colores,
   herramientas y apariencia.
2. Revisar `%APPDATA%\ciat\atic\data\pill\` y que `data\logs\pill.*.log`
   exista y tenga «log iniciado».
3. Abrir una segunda pill: no debe aparecer otra.
4. Cambiar `ui_language` a `en` en Ajustes: en el portapapeles, «yesterday» en
   vez de «ayer» (al reiniciar la pill).
5. La app de Tauri del worktree sigue escribiendo `atic.<fecha>.log`.

## Trabas y decisiones

- La pill vieja que sigue corriendo escribe en `%LOCALAPPDATA%\atic-gpui`.
  Lo que cambies ahí después de la primera vez que arranque la nueva no se
  copia (la migración corre una sola vez).
- El exe de la pill todavía es de consola (sin `windows_subsystem`); se
  resuelve junto con el instalador.
