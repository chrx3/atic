# Pill, tray, atajos y ventanas

**Estado:** `hecho`

## Resumen

Atic vive como barra flotante (pill), icono de bandeja y atajos globales.
La idea es no interrumpir: grabar, dictar o abrir herramientas sin una
ventana grande siempre al frente.

## Cómo se usa

- La pill flota sobre el escritorio; se puede mover y volver a su “hogar”.
- Puede **solapar la barra de tareas** y pegarse al borde de la pantalla
  (`bounds` + `MARGIN = 0`) — base para un futuro modo tipo Dynamic Island.
- Clic / gestos abren grabación, dictado, capturas, clipboard, agentes, etc.
- Atajos globales configurables en Ajustes.
- Bandeja del sistema para mostrar/ocultar y salir.

## Código

- [`apps/desktop/src-tauri/src/floating.rs`](../apps/desktop/src-tauri/src/floating.rs) — geometría y morph de la pill
- [`apps/desktop/src-tauri/src/tray.rs`](../apps/desktop/src-tauri/src/tray.rs) — bandeja
- [`apps/desktop/src-tauri/src/shortcuts.rs`](../apps/desktop/src-tauri/src/shortcuts.rs) — atajos globales
- [`apps/desktop/src/lib/PillSurface.svelte`](../apps/desktop/src/lib/PillSurface.svelte) — UI de la pill dentro del overlay
- [`apps/desktop/src/routes/overlay/+page.svelte`](../apps/desktop/src/routes/overlay/+page.svelte) — composición compartida de superficies flotantes

## Pendiente / siguiente

- [ ] Pulir geometría multi-monitor / DPI cuando aparezcan regresiones
- [x] Open/close rueda: morph = ParticleWheel (gotas); hit-box instantánea con
      pivot center (no tween width — derivaba el centro); skip-flight cercano;
      Esc cancela el vuelo; cierre espera `afterTransition` de nodos y vuelve al hogar
- [x] Morph pill → launcher (pill al slot → barra crece a la der. →
      separa → favs secuenciales + liquid; receta en
      [pill-liquid-emerge.md](pill-liquid-emerge.md))
- [ ] Overrides de slot por tool en Ajustes (`setSlotOverrides` ya existe)

## Relacionado

- [pill-liquid-emerge.md](pill-liquid-emerge.md)
- [dictado.md](dictado.md)
- [capturas.md](capturas.md)
- [launcher-spotlight.md](launcher-spotlight.md)
- [liquid.md](liquid.md)
- Slots: [`apps/desktop/src/lib/surfaces/overlay/toolSlots.ts`](../apps/desktop/src/lib/surfaces/overlay/toolSlots.ts)
