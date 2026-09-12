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
4. `Ctrl/Cmd+Enter` sobre una app **la cierra** (graceful: `WM_CLOSE`, la app
   decide; nunca se mata el proceso). La barra queda abierta para cerrar varias.

Fuentes actuales:

- Accesos del menú Inicio (`.lnk` en Start Menu usuario + ProgramData) en Windows
- Apps en `/Applications` y `~/Applications` en macOS
- Acciones internas de Atic (dictar, capturar, pizarra, color, clipboard,
  fragmentos, agentes, ajustes) + **calculadora** + **acciones de sistema**

### Calculadora inline

Si lo que escribís es una cuenta o una conversión, el resultado va **primero** y
Enter lo copia al portapapeles:

| Escribís | Resultado |
|---|---|
| `2+3*4` `(2+3)*4` `2^3^2` `10%3` `-4^2` | aritmética con `+ - * / % ^`, paréntesis y unarios |
| `10 km to mi` `1 in to cm` | longitud (mm, cm, m, km, in, ft, yd, mi) |
| `30 c to f` `100 f to c` | temperatura (c, f, k) |
| `2 gb to mb` | datos (kb, mb, gb, tb, decimal) |
| `1 h to min` | tiempo (s, min, h, d) |

Sin red y sin dependencias: lo que no se puede resolver con certeza **no se
muestra** (nada de números dudosos). Divisas y cripto quedan afuera a propósito.

### Acciones de sistema (Windows por ahora)

`Bloquear pantalla`, `Suspender`, `Silenciar o activar sonido` y `Vaciar
papelera`. La papelera conserva el diálogo de confirmación de Windows: Atic no
fuerza acciones destructivas. `Cerrar todas las apps` manda `WM_CLOSE` a las
ventanas visibles de apps de usuario (no al shell ni a Atic).

Ver [system-actions.md](system-actions.md).

## Código

- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — índice, búsqueda, abrir, cerrar; float vía `panel_float`
- [`apps/desktop/src-tauri/src/calc.rs`](../apps/desktop/src-tauri/src/calc.rs) — calculadora (aritmética + unidades), sin dependencias
- [`apps/desktop/src-tauri/src/system_actions.rs`](../apps/desktop/src-tauri/src/system_actions.rs) — bloqueo, suspensión, mute, papelera
- [`apps/desktop/src-tauri/src/launcher_recents.rs`](../apps/desktop/src-tauri/src/launcher_recents.rs) — apps corriendo/al frente + `WM_CLOSE` a sus ventanas
- [`apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte) — UI en el overlay
- Atajo: `launcher_shortcut` en config + [`shortcuts.rs`](../apps/desktop/src-tauri/src/shortcuts.rs)

## Pendiente / siguiente

- [ ] Overrides de slot en Ajustes (API: `setSlotOverrides` en toolSlots.ts)

- [x] Atajo por defecto que no choque con el menú de ventana del SO
- [x] Indexar `.lnk` del menú Inicio + cache en RAM
- [x] Match simple + acciones Atic
- [x] Float en overlay (nace centrado; path primario ya no usa
      la ventana Tauri `launcher`)
- [x] Apertura con nacimiento centrado: la barra aparece como gota de 40 px en el
      centro y **estira** al stadium (40 → 324 px, `--ease-liquid`, 200 ms) → favs
      de a uno; cierre = espejo (tuck → repliegue en el centro → dismiss);
      tokens `--launcher-bar-open-dur` / `--launcher-fav-stagger`
      (patrón documentado en [pill-liquid-emerge.md](pill-liquid-emerge.md))
- [x] Calculadora inline (aritmética + unidades) y acciones de sistema
- [x] Cerrar la app del resultado (`Ctrl/Cmd+Enter`) y cerrar todas las apps
- [ ] Divisas y cripto en vivo (necesita red: entra solo con opt-in explícito)
- [ ] Force quit / matar procesos (hoy es graceful por decisión de producto)
- [ ] Gestión de ventanas estilo Rectangle (mitades, cuartos, mover de monitor)
- [ ] Preferencias (raíces extra, exclusiones, favoritos) en Ajustes
- [ ] Ranking por uso / fuzzy más fino
- [ ] Apps Store/UWP
- [ ] Retirar ventana Tauri `launcher` (dormida; UI vive en overlay)

## Relacionado

- [pill-liquid-emerge.md](pill-liquid-emerge.md) — **referencia de implementación** del acto A (nacimiento centrado → peels) y del acto B (fused grow → separate) de los paneles
- [system-actions.md](system-actions.md) — las cuatro acciones del sistema y su política
- [pill-shell.md](pill-shell.md)
- [liquid.md](liquid.md)
- [ajustes-onboarding.md](ajustes-onboarding.md)
- [snippets.md](snippets.md)
- [agentes.md](agentes.md)
