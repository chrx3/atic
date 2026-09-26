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
- Apps de macOS: `/Applications`, `/System/Applications` y `~/Applications`,
  con recursión acotada (entra a `Utilities` y subcarpetas; nunca a un `.app`).
  Los agentes internos de `/System/Library/CoreServices` quedan afuera a propósito.
- Acciones internas de Atic (dictar, capturar, pizarra, color, clipboard,
  fragmentos, agentes, ajustes) + **calculadora** + **acciones de sistema**

### Recientes (sin escribir)

La barra idle no es muda: lista hasta 8 apps **abiertas ahora** y las que Atic
lanzó alguna vez (persistidas en `launcher-recents.json`). Windows enumera
ventanas visibles (con «En uso» / «Abierta hace…»); macOS usa
`NSWorkspace.runningApplications` (solo apps regulares: Finder, Atic y agentes
de fondo no cuentan). «Usada hace…» sale del registro de lanzamientos. Sin
nada abierto ni lanzado, el launcher queda solo barra + favoritos.

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
muestra** (nada de números dudosos). Las divisas van aparte y son opt-in.

### Divisas (opt-in)

Con **Ajustes → Launcher → Conversión de divisas** encendido, la calculadora
suma montos con tasas públicas y **siempre** muestra de dónde salen:

| Escribís | Resultado |
|---|---|
| `30.000 CLP a UF` (`30,000 CLP to UF`) | `0,73 UF · valor oficial del 26 sep` |
| `30 USD a CLP` | `28.971 CLP · valor oficial del 25 sep` |
| `100 libras a clp` | `126.840 CLP · tipo de cambio del 25 sep` |
| `uf a clp` (sin monto vale 1) | `41.024 CLP · valor oficial del 26 sep` |

- Monedas: CLP, UF, UTM, USD, EUR, ARS, BRL, PEN, COP, MXN, UYU, BOB, PYG,
  VES, CAD, GBP, CHF, JPY, CNY y AUD, con nombres en español e inglés
  (`dólar`, `dollar`, `peso chileno`, `libra`…) y sin tildes. Cripto queda
  afuera por ahora.
- Montos en los dos formatos: `30.000,50` (es-CL) y `30,000.50` (inglés); el
  caso ambiguo de un solo separador («1.234») sigue el idioma de la UI.
- La UF y la UTM son el **valor oficial** (mindicador.cl → Banco Central);
  los cruces que no son CLP, la tabla de mercado (exchangerate-api.com). Si
  una fuente falla, se conserva la última tabla y el subtítulo lo dice
  («datos antiguos» / «sin conexión»).
- Apagado por defecto: la calculadora no sale a la red sin permiso. Con el
  interruptor apagado, una query de dinero ofrece encenderlo (abre Ajustes).
  Lo escrito en el launcher no se envía a los proveedores: solo se descargan
  las tablas de tasas.

### Emojis (modo `:`)

Escribir `:` con la barra vacía (o elegir la acción «Emojis») cambia el float a
una grilla estilo Raycast: chip «Emojis» en la barra, recientes arriba,
categorías con salto directo y un botón de tono de piel (se recuerda).

- Busca por nombre y palabras clave en español **e** inglés, sin tildes
  («corazon rojo», «fire», «like»). Todas las palabras de la query tienen que calzar.
- Flechas navegan la grilla en 2D; **Enter pega** en la app que tenía el foco
  al abrir el launcher; `Ctrl/Cmd+Enter` solo copia. Sin destino externo, el
  texto va a la cola de pegado (mismo camino que los fragmentos).
- `Esc` o `Backspace` con la barra vacía vuelven al launcher.
- Catálogo local: `emojiData.json` (CLDR vía emojibase), generado con
  `node scripts/gen-emoji-data.mjs` y cargado con `import()` solo al entrar
  al modo. En Windows se ocultan las banderas de país (Segoe las dibuja como
  letras) y lo posterior a Emoji 15.0.

### Acciones de sistema

`Bloquear pantalla`, `Suspender`, `Silenciar o activar sonido` y `Vaciar
papelera` corren en Windows y macOS. La papelera conserva el diálogo de
confirmación del SO: Atic no fuerza acciones destructivas.
`Cerrar todas las apps` también corre en las dos plataformas: en Windows manda
`WM_CLOSE` a las ventanas visibles de apps de usuario (no al shell ni a Atic) y
en macOS pide Quit a las apps regulares vivas (`NSRunningApplication.terminate`).

Ver [system-actions.md](system-actions.md).

## Código

- [`apps/desktop/src-tauri/src/launcher.rs`](../apps/desktop/src-tauri/src/launcher.rs) — índice, búsqueda, abrir, cerrar; float vía `panel_float`
- [`apps/desktop/src-tauri/src/calc.rs`](../apps/desktop/src-tauri/src/calc.rs) — calculadora (aritmética + unidades + divisas), sin red
- [`apps/desktop/src-tauri/src/fx.rs`](../apps/desktop/src-tauri/src/fx.rs) — tasas de cambio: fuentes, caché en disco, frescura y opt-in
- [`apps/desktop/src-tauri/src/system_actions.rs`](../apps/desktop/src-tauri/src/system_actions.rs) — bloqueo, suspensión, mute, papelera
- [`apps/desktop/src-tauri/src/launcher_recents.rs`](../apps/desktop/src-tauri/src/launcher_recents.rs) — apps corriendo/al frente + cierre graceful (`WM_CLOSE` en Windows; `NSRunningApplication.terminate` en macOS)
- [`apps/desktop/src-tauri/src/launcher_icons.rs`](../apps/desktop/src-tauri/src/launcher_icons.rs) — iconos: shell en Windows, `NSWorkspace.iconForFile` → PNG en macOS
- [`apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte`](../apps/desktop/src/lib/surfaces/overlay/launcher/LauncherFloat.svelte) — UI en el overlay
- [`apps/desktop/src/lib/features/emoji/emoji.ts`](../apps/desktop/src/lib/features/emoji/emoji.ts) — catálogo, búsqueda, navegación de la grilla, recientes y tono; pega vía `launcher_paste_text`
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
- [x] Paridad macOS: apps de `/Applications` + `/System/Applications` (con
      `Utilities`), iconos reales (AppKit `iconForFile` → PNG), Recientes
      (`NSWorkspace.runningApplications`) y cierre graceful
      (`NSRunningApplication.terminate`)
- [x] Buscador de emojis (modo `:`, grilla, tonos, pegar en la app activa)
- [ ] Emojis sugeridos inline en la búsqueda normal
- [x] Divisas en vivo con opt-in (CLP, UF, UTM, USD, EUR y cruces; sin cripto)
- [ ] Cripto (BTC, ETH) en el conversor
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
