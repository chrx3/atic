# Pill, tray, atajos y ventanas

**Estado:** `hecho`

## Resumen

Atic vive como barra flotante (pill), icono de bandeja y atajos globales.
La idea es no interrumpir: grabar, dictar o abrir herramientas sin una
ventana grande siempre al frente.

## Cómo se usa

- La pill flota sobre el escritorio; se puede mover y volver a su “hogar”.
- Clic / gestos abren grabación, dictado, capturas, clipboard, agentes, etc.
- Atajos globales configurables en Ajustes.
- Bandeja del sistema para mostrar/ocultar y salir.

## Código

- [`apps/desktop/src-tauri/src/floating.rs`](../apps/desktop/src-tauri/src/floating.rs) — geometría y morph de la pill
- [`apps/desktop/src-tauri/src/tray.rs`](../apps/desktop/src-tauri/src/tray.rs) — bandeja
- [`apps/desktop/src-tauri/src/shortcuts.rs`](../apps/desktop/src-tauri/src/shortcuts.rs) — atajos globales
- [`apps/desktop/src/routes/pill/+page.svelte`](../apps/desktop/src/routes/pill/+page.svelte) — UI de la pill

## Pendiente / siguiente

- [ ] Pulir geometría multi-monitor / DPI cuando aparezcan regresiones
- [ ] Unificar sensación de “abrir herramienta” con un futuro launcher

## Relacionado

- [dictado.md](dictado.md)
- [capturas.md](capturas.md)
- [launcher-spotlight.md](launcher-spotlight.md)
