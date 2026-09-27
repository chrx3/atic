# Cuentagotas de color

**Estado:** `hecho`

## Resumen

Elige un color de la pantalla, píxel a píxel, con una lupa en vivo que sigue al
cursor y no congela el escritorio. También se puede armar desde la rosa
cromática o escribiendo un HEX. El código se copia al portapapeles en HEX, RGB
o HSL.

## Cómo se usa

- Atajo por defecto `Ctrl/Cmd+Shift+C`, o desde la rueda / el launcher.
- **Clic o Enter copia** el color bajo el cursor; `R` abre la rosa cromática
  (mismo gesto para volver). `Esc` sale.
- En la rosa: anillo de matiz, área de saturación/brillo, sliders, entrada HEX
  y los colores recientes.
- La lupa lee el píxel global del cursor, así que coincide con lo que hay bajo
  el puntero con DPI ≠ 100% y en varios monitores.

## Código

- [`apps/desktop/src-tauri/src/color_picker.rs`](../apps/desktop/src-tauri/src/color_picker.rs) — lupa, lectura de píxel y rosa (Windows y macOS)
- [`apps/desktop/src/lib/surfaces/color/ColorLoupeSurface.svelte`](../apps/desktop/src/lib/surfaces/color/ColorLoupeSurface.svelte) — UI del HUD
- [`apps/desktop/src/lib/features/color/colorMath.ts`](../apps/desktop/src/lib/features/color/colorMath.ts) — conversiones HEX/RGB/HSV/HSL, con tests

## Pendiente / siguiente

- [ ] Más formatos de copia (Tailwind, variables CSS) si se piden
- [ ] Historial más largo que los recientes actuales

## Relacionado

- [capturas.md](capturas.md)
- [launcher-spotlight.md](launcher-spotlight.md)
- [pill-shell.md](pill-shell.md)
