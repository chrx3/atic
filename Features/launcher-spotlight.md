# Launcher tipo Spotlight

**Estado:** `hecho` (float desde la pill; ventana Tauri dormida)

## Resumen

Barra de búsqueda global al estilo macOS (Cmd+Space) / Raycast / PowerToys
Run: abrir programas, acciones de Atic y rutas favoritas sin pasar por el
buscador de Windows. **No** es un índice Everything de todo el disco.

## Cómo se usa

1. Atajo global (por defecto `Ctrl+Space` / `Cmd+Space`; configurable en Ajustes).
2. Escribes; resultados al vuelo (prefix/contains).
3. Enter abre el ítem (acceso directo o acción interna). Esc o perder el foco cierra.

Fuentes actuales:

- Accesos del menú Inicio (`.lnk` en Start Menu usuario + ProgramData) en Windows
- Apps en `/Applications` y `~/Applications` en macOS
- Acciones internas de Atic (dictar, capturar, clipboard, fragmentos, agentes, ajustes)

## Código

- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — índice, búsqueda, abrir; float vía `panel_float`
- [`apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte) — UI en el overlay
- Atajo: `launcher_shortcut` en config + [`shortcuts.rs`](../apps/desktop/src-tauri/src/shortcuts.rs)

## Pendiente / siguiente

- [ ] Overrides de slot en Ajustes (API: `setSlotOverrides` en toolSlots.ts)

- [x] Atajo por defecto que no choque con el menú de ventana del SO
- [x] Indexar `.lnk` del menú Inicio + cache en RAM
- [x] Match simple + acciones Atic
- [x] Float en overlay (crece desde la pill; path primario ya no usa
      la ventana Tauri `launcher`)
- [x] Apertura tipo dictado→Spotlight: barra crece a la derecha → se
      separa → favs de a uno; cierre = reverse (tuck → fuse → shrink);
      tokens `--launcher-bar-open-dur` /
      `--launcher-separate-dur` / `--launcher-fav-stagger`
      (patrón documentado en [pill-liquid-emerge.md](pill-liquid-emerge.md))
- [ ] Preferencias (raíces extra, exclusiones, favoritos) en Ajustes
- [ ] Ranking por uso / fuzzy más fino
- [ ] Apps Store/UWP
- [ ] Retirar ventana Tauri `launcher` (dormida; UI vive en overlay)

## Relacionado

- [pill-liquid-emerge.md](pill-liquid-emerge.md) — **referencia de implementación** del acto fused grow → separate → peels
- [pill-shell.md](pill-shell.md)
- [liquid.md](liquid.md)
- [ajustes-onboarding.md](ajustes-onboarding.md)
- [snippets.md](snippets.md)
- [agentes.md](agentes.md)
