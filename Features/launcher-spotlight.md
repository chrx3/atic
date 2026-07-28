# Launcher tipo Spotlight

**Estado:** `parcial`

## Resumen

Barra de búsqueda global al estilo macOS (Cmd+Space) / Raycast / PowerToys
Run: abrir programas, acciones de Atic y rutas favoritas sin pasar por el
buscador de Windows. **No** es un índice Everything de todo el disco.

## Cómo se usa

1. Atajo global (por defecto `Ctrl+Space` / `Cmd+Space`; configurable en Ajustes).
2. Escribís; resultados al vuelo (prefix/contains).
3. Enter abre el ítem (acceso directo o acción interna). Esc o perder el foco cierra.

Fuentes actuales:

- Accesos del menú Inicio (`.lnk` en Start Menu usuario + ProgramData) en Windows
- Apps en `/Applications` y `~/Applications` en macOS
- Acciones internas de Atic (dictar, capturar, clipboard, fragmentos, agentes, ajustes)

## Código

- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — índice, búsqueda, abrir
- [`apps/desktop/src/routes/launcher/+page.svelte`](../apps/desktop/src/routes/launcher/+page.svelte) — UI overlay
- Atajo: `launcher_shortcut` en config + [`shortcuts.rs`](../apps/desktop/src-tauri/src/shortcuts.rs)

## Pendiente / siguiente

- [x] Atajo por defecto que no choque con el menú de ventana del SO
- [x] Indexar `.lnk` del menú Inicio + cache en RAM
- [x] Overlay Tauri + match simple
- [x] Acciones Atic como resultados de primera clase
- [ ] Preferencias (raíces extra, exclusiones, favoritos) en Ajustes
- [ ] Ranking por uso / fuzzy más fino
- [ ] Apps Store/UWP

## Relacionado

- [pill-shell.md](pill-shell.md)
- [ajustes-onboarding.md](ajustes-onboarding.md)
- [snippets.md](snippets.md)
- [agentes.md](agentes.md)
