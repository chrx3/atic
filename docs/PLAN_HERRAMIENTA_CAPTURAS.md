# Plan detallado: herramienta de capturas para Atic

> **Estado:** Fases 0–4 **implementadas**; Fase 5 = QA manual del usuario.
> Ver §25 para el registro de implementación y las desviaciones respecto a
> este plan (overlay opaco, overlay único, eventos `screenshot-*`).

## 1. Objetivo

Añadir a Atic una herramienta de capturas ligera para Windows 10/11,
optimizada para trabajar con Claude Code, terminales y chats de IA.

La primera versión permitirá capturar una ventana, una región o un monitor;
mostrará hasta cinco miniaturas laterales y permitirá arrastrar cada PNG fuera
de Atic.

## 2. Alcance confirmado

### Incluido en v1

- Exclusivamente Windows 10/11 x64.
- Captura mediante un atajo global configurable.
- Preselección de ventanas al pasar el mouse.
- Selección manual de una región.
- Captura del monitor completo.
- Compatibilidad con múltiples monitores y escalado DPI.
- Miniaturas flotantes en el costado derecho.
- Máximo de cinco capturas visibles.
- Arrastre del PNG hacia terminales, navegadores y otras aplicaciones.
- Copiar imagen o ruta del archivo.
- Eliminación automática de archivos después de 24 horas.
- Integración visual con el diseño actual de Atic.
- Funcionamiento completamente local.

### Fuera de v1

- OCR.
- Anotaciones, flechas y texto.
- Difuminado de información sensible.
- Captura con desplazamiento.
- Grabación de pantalla o GIF.
- Historial permanente.
- macOS.
- Windows Graphics Capture (se evalúa como upgrade en la segunda fase; ver §23).
- Modificación o reemplazo de configuraciones internas de Windows.

## 3. Correcciones respecto al plan anterior

Estas son las diferencias frente a la primera versión del plan. Se documentan
para que la revisión sea directa.

1. **Motor de captura: GDI en vez de Windows Graphics Capture.**
   El plan anterior proponía WGC «vía `windows-sys` crudo». Eso es inviable:
   WGC es una API **WinRT** que `windows-sys` no puede invocar de forma
   práctica (requeriría el crate `windows` completo con D3D11 e interop, la
   parte más compleja y arriesgada del proyecto). v1 usa **GDI**
   (`BitBlt` + `PrintWindow`) sobre `windows-sys`, que ya es dependencia y es
   el patrón que el proyecto usa hoy en `meeting_detection.rs`. Ver §11.

2. **Drag nativo con `tauri-plugin-drag` en vez de OLE a mano.**
   El plan anterior proponía escribir `DoDragDrop` / `IDataObject` /
   `IDropSource` / `CF_HDROP` manualmente. En su lugar se usa
   `tauri-plugin-drag` (crate `drag-rs` del ecosistema Tauri/CrabNebula), que
   entrega `CF_HDROP` nativo con muy poco código y elimina el mayor riesgo del
   proyecto. Ver §9.

3. **Ruta de capturas corregida.**
   El plan anterior indicaba `%LOCALAPPDATA%\com.ciat.atic\captures\`. La
   app real usa `directories::ProjectDirs::from("com","ciat","atic")`
   (`crates/core/src/paths.rs`), que en Windows resuelve a
   `%APPDATA%\ciat\atic\data\`. Las capturas van en un nuevo
   `AppDirs::captures_dir()` → `…\data\captures\`. Ver §13.

4. **Migración de `Config` explícita.**
   `crates/core/src/config.rs` usa un patrón de doble struct
   (`Config` + `ConfigFile` con campos `Option` + `From<ConfigFile>`). Todo
   campo nuevo debe añadirse en cinco lugares. Ver §15.

5. **Refactor de atajos con diseño concreto.**
   Se especifica cómo registrar cada atajo de forma independiente para que un
   conflicto de captura no desactive grabación, dictado ni pill, incluyendo el
   ajuste en `set_config`. Ver §15.

6. **Overlays y shelf como ventanas dinámicas.**
   La pill se declara estáticamente en `tauri.conf.json`. Los overlays por
   monitor y el shelf se crean en tiempo de ejecución con
   `WebviewWindowBuilder`, y requieren permisos en `capabilities/`. Ver §10 y §18.

7. **Flujo «congelar primero» (nuevo en esta revisión).**
   Los frames de todos los monitores se capturan a memoria al presionar el
   atajo, antes de mostrar los overlays. Región y monitor se recortan del
   frame congelado; la ventana se captura con `PrintWindow` (que renderiza
   solo la ventana objetivo). Resultado: el overlay no puede aparecer en el
   PNG y no hay que ocultarlo ni esperar al compositor (el enfoque anterior
   de «ocultar overlays antes de capturar» era una carrera contra DWM con
   parpadeo). Ver §11.

8. **Overlays y shelf reutilizables (nuevo en esta revisión).**
   Crear una ventana WebView2 tarda cientos de milisegundos; crearlas en cada
   sesión rompería el objetivo de 120 ms. Se crean una vez (perezosamente) y
   se reutilizan ocultas entre sesiones. Ver §16 y §22.

## 4. Decisiones técnicas confirmadas

- **Captura:** GDI (`BitBlt` para monitor/región, `PrintWindow` con
  `PW_RENDERFULLCONTENT` para ventana). Solo `windows-sys`.
- **Arrastre:** `tauri-plugin-drag`.
- **Codificación PNG:** crate `png` (ya presente en el árbol de dependencias
  vía `image`; se añade como dependencia directa de `crates/capture`).
- **Rutas:** `AppDirs::captures_dir()` en `crates/core`.
- **Enumeración de monitores:** `app.available_monitors()` de Tauri para
  geometría de overlays (posición, tamaño, escala); `MonitorFromPoint` +
  `GetMonitorInfoW` (rcWork) solo para ubicar el shelf respecto al área útil.
- **Flujo de sesión:** «congelar primero» — `BitBlt` de todos los monitores a
  memoria al presionar el atajo; overlays transparentes atenuados sobre la
  pantalla; región/monitor se recortan del frame congelado y la ventana usa
  `PrintWindow` al confirmar.
- **Ventanas de UI:** overlays y shelf se crean perezosamente la primera vez
  y se reutilizan ocultos después (objetivo de 120 ms).

## 5. Dependencias nuevas

Se añaden únicamente estas dependencias. Ninguna es el crate `windows` WinRT.

| Dependencia | Dónde | Motivo |
|---|---|---|
| `png` | `crates/capture/Cargo.toml` | Codificar BGRA → PNG. Ya compilado en el árbol vía `image`. |
| features de `windows-sys`: `Win32_Graphics_Gdi`, `Win32_Graphics_Dwm`, `Win32_UI_HiDpi` | `crates/capture` y `src-tauri` | `BitBlt`, `PrintWindow`, `DwmGetWindowAttribute`, monitores, DPI. |
| `tauri-plugin-drag` | `apps/desktop/src-tauri/Cargo.toml` | Arrastre nativo de archivo. |
| `@crabnebula/tauri-plugin-drag` | `apps/desktop/package.json` | API JS `startDrag` en el frontend. |

No se añade el crate `windows` (WinRT), ni `image` como dependencia directa,
ni ningún crate de captura de terceros (`scap`, `xcap`, `windows-capture`).

## 6. Flujo principal

```text
Atajo global
    ↓
Frames de todos los monitores congelados en memoria
    ↓
Overlay sobre los monitores
    ↓
Hover preselecciona una ventana
    ├─ Clic → captura la ventana
    ├─ Arrastrar → captura una región
    ├─ Espacio → captura el monitor
    └─ Esc → cancela
                ↓
         PNG temporal local
                ↓
     Miniatura lateral flotante
        ├─ Arrastrar archivo
        ├─ Copiar imagen
        ├─ Copiar ruta
        ├─ Abrir ubicación
        └─ Eliminar
```

La acción primaria será capturar y arrastrar sin interrumpir el flujo de
trabajo.

## 7. Experiencia de selección

Al activar la captura:

- Cada monitor recibe un overlay oscuro y sutil.
- La ventana bajo el cursor se ilumina con el acento verde actual.
- Se muestra una etiqueta compacta con dimensiones, por ejemplo `1280 × 720`.
- No se muestran diálogos ni barras de herramientas grandes.
- Una ayuda inferior indica:

```text
Clic: ventana · Arrastra: región · Espacio: pantalla · Esc: cancelar
```

Interacciones:

- Mover el mouse cambia la ventana preseleccionada.
- Un clic captura la ventana completa.
- Arrastrar más de 4 px cambia automáticamente a selección de región.
- `Espacio` selecciona el monitor completo.
- `Enter` confirma la selección actual.
- `Tab` recorre ventanas superpuestas bajo el cursor.
- `Esc` cancela sin generar archivos.

Detalles de la experiencia:

- **El overlay nunca aparece en el PNG por diseño**: los frames se congelan
  antes de mostrarlo (§11), así que no hay que ocultarlo ni hay parpadeo.
- Al confirmar una captura, un flash breve del área seleccionada confirma la
  acción antes de cerrar el overlay.
- La selección de región se limita en v1 al monitor donde comenzó el
  arrastre (simplifica DPI mixto).
- Con varios monitores, el foco de teclado sigue al mouse: el overlay bajo el
  cursor toma el foco al entrar, para que `Esc`/`Espacio`/`Tab`/`Enter`
  actúen donde el usuario está mirando.
- Como el overlay cubre el cursor, el hover **no** usa `WindowFromPoint`: se
  hace hit-testing contra la lista de candidatos calculada al inicio de la
  sesión (ver §12).

## 8. Miniaturas laterales (shelf)

Se creará una ventana Tauri independiente llamada `capture-shelf`, construida
dinámicamente con `WebviewWindowBuilder` (no se declara en `tauri.conf.json`).
Se crea oculta la primera vez que se necesita y se reutiliza entre sesiones.

Comportamiento:

- Solo se muestra al crear una captura nueva; al iniciar la app permanece
  oculta y su contenido se reconstruye escaneando la carpeta.
- Aparece en el monitor donde se hizo la captura.
- Se ubica a 16 px del borde derecho del **área útil** (`rcWork`, excluye la
  barra de tareas).
- No roba el foco de la aplicación activa (`focused(false)`).
- Permanece sobre otras ventanas (`always_on_top(true)`).
- No aparece en la barra de tareas (`skip_taskbar(true)`).
- Muestra hasta cinco capturas, con la más reciente arriba.
- Conserva la proporción de cada imagen.
- El hover revela acciones secundarias.
- Después de 20 segundos sin interacción se retrae.
- Al retraerse deja una pestaña pequeña con el número de capturas.
- Hover o clic sobre la pestaña vuelve a expandirla.
- El hover y un arrastre en curso pausan el temporizador.

Cada miniatura incluye:

- Imagen.
- Dimensiones.
- Hora de creación.
- Copiar imagen.
- Copiar ruta.
- Eliminar.
- Abrir carpeta desde un menú contextual.

La superficie usará los colores, radios y sombras existentes. No añadirá
bordes decorativos ni controles permanentes innecesarios.

## 9. Arrastre hacia otras aplicaciones

Cada captura se guarda inmediatamente como un archivo PNG real. El arrastre se
delega a **`tauri-plugin-drag`**, que en Windows entrega una referencia nativa
de archivo (`CF_HDROP`) al destino.

En el frontend, la miniatura inicia el arrastre con la API del plugin:

```ts
import { startDrag } from "@crabnebula/tauri-plugin-drag";

await startDrag({ item: [pngAbsolutePath], icon: pngAbsolutePath });
```

Resultado esperado:

- Windows Terminal y VS Code insertan la ruta entre comillas.
- Claude Code recibe una ruta local que el agente puede leer.
- Claude, ChatGPT y otros chats adjuntan el PNG.
- Explorador o escritorio copian el archivo.
- Editores gráficos abren o importan la imagen.

Complementos que ofrece Atic:

- Archivo mediante el arrastre del plugin (`CF_HDROP`).
- Imagen al portapapeles al pulsar **Copiar imagen** (vía `arboard`, que ya es
  dependencia del proyecto).
- Ruta absoluta al portapapeles al pulsar **Copiar ruta**.

El plugin reemplaza el trabajo de OLE a mano; la validación de la Fase 0 se
concentra en confirmar que el drag del plugin llega correctamente a los
destinos principales (ver §19).

## 10. Arquitectura propuesta

### Motor independiente

Crear un crate de workspace `crates/capture` (miembro nuevo en el
`Cargo.toml` raíz), sin dependencia de Tauri ni del frontend:

```text
crates/capture/
├── Cargo.toml        # deps: windows-sys (Gdi/Dwm/HiDpi), png, serde, chrono
└── src/
    ├── lib.rs
    ├── engine.rs     # orquestación de capturas
    ├── monitors.rs   # enumeración y geometría de monitores
    ├── windows.rs    # enumeración/filtrado de ventanas + bounds DWM
    ├── geometry.rs   # conversiones físico/lógico, recortes, intersección
    ├── encoding.rs   # BGRA (top-down) → PNG con el crate png
    └── retention.rs  # limpieza por antigüedad (patrón de crates/core)
```

Responsabilidades:

- Enumerar monitores.
- Enumerar y filtrar ventanas.
- Obtener dimensiones visuales reales (`DWMWA_EXTENDED_FRAME_BOUNDS`).
- Capturar un monitor o una ventana.
- Recortar regiones.
- Convertir frames BGRA a PNG.
- Gestionar nombres y rutas.
- Limpiar capturas antiguas.
- No depender de Tauri ni del frontend.

### Integración Tauri

Añadir a `apps/desktop/src-tauri/src/`:

```text
├── capture.rs          # comandos Tauri + coordinación de sesión
├── capture_overlay.rs  # crear/cerrar overlays por monitor (dinámicos)
└── capture_shelf.rs    # crear/actualizar el shelf (dinámico)
```

Responsabilidades:

- Coordinar una única sesión activa (patrón `Mutex<Option<…>>` como
  `AppState.active`).
- Crear overlays por monitor con `WebviewWindowBuilder` una sola vez y
  reutilizarlos ocultos; recrearlos si cambió la configuración de monitores
  (se comprueba al inicio de cada sesión).
- Guardar el `HWND` en primer plano al iniciar la sesión y restaurarle el
  foco al cerrarla.
- Mantener los frames congelados de la sesión en memoria y liberarlos al
  terminar (transitorio: ~8 MB por monitor 1080p, ~33 MB por 4K).
- Emitir eventos al frontend.
- Mantener el shelf sincronizado.
- Iniciar el arrastre (delegado al plugin en el frontend).
- Integrarse con atajos y estado global.

Notas de reutilización:

- El arrastre nativo lo aporta `tauri-plugin-drag`; no se crea `file_drag.rs`.
- La animación/posicionamiento del shelf puede seguir el patrón de
  `state::summon_pill_to_cursor` (hilo dedicado + `set_position`).
- La limpieza de 24 h sigue el patrón de `retention.rs`: canonicalizar la ruta
  y verificar que su `parent()` es la raíz de capturas antes de borrar
  (previene escapes de directorio).

### Frontend

Añadir a `apps/desktop/src/`. Como el proyecto es SPA
(`adapter-static`, `ssr = false`, `prerender = false`), las rutas nuevas
funcionan igual que `/pill` (enrutado en cliente):

```text
├── routes/
│   ├── capture-overlay/+page.svelte
│   └── capture-shelf/+page.svelte
└── lib/capture/
    ├── CaptureOverlay.svelte
    ├── CaptureSelection.svelte
    ├── CaptureShelf.svelte
    ├── CaptureThumbnail.svelte
    ├── capture-state.svelte.ts
    └── types.ts
```

## 11. APIs nativas de Windows (GDI)

Estrategia de captura (solo `windows-sys`, sin WinRT), con flujo
**«congelar primero»**:

1. **Al presionar el atajo** (antes de mostrar overlays) se captura cada
   monitor a memoria: `GetDC(NULL)` + `CreateCompatibleDC` +
   `CreateCompatibleBitmap` + `BitBlt` (`SRCCOPY | CAPTUREBLT`) con el rect
   físico del monitor. El escritorio virtual se referencia con
   `SM_XVIRTUALSCREEN` / `SM_YVIRTUALSCREEN` (soporta coordenadas negativas).
   Si `capture_include_cursor` está activo, el cursor se dibuja sobre el
   frame (`GetCursorInfo` + `DrawIconEx`) en ese instante.
2. **Monitor / región:** se recortan del frame congelado en memoria. El
   overlay (transparente y atenuado) no puede aparecer en el resultado y no
   hay que ocultarlo ni esperar al compositor.
3. **Ventana:** `PrintWindow(hwnd, hdc, PW_RENDERFULLCONTENT)` (bandera
   `0x2`) al confirmar, que renderiza **solo** la ventana objetivo — ni los
   overlays ni ventanas superpuestas la contaminan — y funciona con
   composición GPU (Chrome, terminales, apps modernas). El recorte usa
   `DWMWA_EXTENDED_FRAME_BOUNDS` para excluir la sombra invisible. Si el
   resultado sale negro/uniforme, se degrada automáticamente al recorte del
   rect de la ventana desde el frame congelado.
4. **Extracción de píxeles:** `GetDIBits` con `BITMAPINFOHEADER` de altura
   negativa (top-down) → buffer BGRA que `encoding.rs` convierte a PNG.

Tradeoff documentado: región y monitor reflejan el momento del atajo, no el
del clic. Si el contenido cambia durante la selección (p. ej. un video), el
PNG corresponde al instante en que el usuario presionó el atajo — el
comportamiento estándar de las herramientas de captura.

Detección de ventanas y monitores:

- `EnumWindows`: enumerar ventanas superiores (ya se usa en
  `meeting_detection.rs`).
- `DwmGetWindowAttribute` + `DWMWA_EXTENDED_FRAME_BOUNDS`: límites visuales
  reales (sin sombra).
- `DwmGetWindowAttribute` + `DWMWA_CLOAKED`: excluir ventanas ocultas del
  sistema.
- `GetWindowLongPtrW`: detectar estilos y ventanas auxiliares.
- `GetWindowTextW`, `GetWindowThreadProcessId`: título y proceso.
- `MonitorFromPoint` + `GetMonitorInfoW` (`rcWork`): monitor y área útil para
  ubicar el shelf.

Limitación honesta de GDI: contenido protegido por DRM o ciertos modos
fullscreen exclusivos pueden salir en negro. Mitigación: detectar el frame
negro/uniforme e informar, ofreciendo captura de región como alternativa (ya
contemplado en §22).

## 12. Detección de ventanas

Al abrir la captura se construye una lista ordenada por z-index:

```rust
WindowCandidate {
    hwnd,
    title,
    visual_bounds,   // DWMWA_EXTENDED_FRAME_BOUNDS, coords físicas
    process_id,
    z_index,
    monitor_id,
}
```

Se excluyen:

- Overlays y shelf de Atic (por `process_id == GetCurrentProcessId()`).
- Pill y ventana principal.
- Ventanas invisibles o minimizadas (`IsWindowVisible == 0`, `IsIconic != 0`).
- Elementos internos del shell.
- Tooltips y menús temporales.
- Ventanas completamente transparentes.
- Ventanas `cloaked`.
- Rectángulos inferiores a un tamaño mínimo.

El hover no dependerá de `WindowFromPoint`, porque el overlay está bajo el
cursor. Se hace hit-testing contra la lista previa respetando el orden visual;
`Tab` recorre los candidatos superpuestos en esa posición.

## 13. Modelo de datos

```rust
CaptureItem {
    id: String,
    path: PathBuf,
    created_at: DateTime<Utc>,
    width: u32,
    height: u32,
    source: CaptureSource,
    monitor_id: String,
}

enum CaptureSource {
    Window,
    Region,
    Monitor,
}
```

No se requiere una tabla SQLite en v1. El shelf mantiene los elementos en
memoria y reconstruye su estado escaneando la carpeta al iniciar.

Ruta (nueva en `crates/core/src/paths.rs`):

```rust
// AppDirs
pub fn captures_dir(&self) -> PathBuf {
    self.data_dir.join("captures")
}
// …y crearla en AppDirs::new() junto a recordings_dir()/models_dir().
```

En Windows resuelve a:

```text
%APPDATA%\ciat\atic\data\captures\
```

Formato de nombres:

```text
capture_2026-07-17_14-32-08_a1b2c3.png
```

Los PNG no contienen metadatos adicionales ni se envían a servicios externos.

## 14. Comandos y eventos

Comandos Tauri (registrar en `invoke_handler` de `lib.rs`):

```text
start_capture_session
cancel_capture_session
complete_window_capture
complete_region_capture
complete_monitor_capture
list_recent_captures
copy_capture_image
copy_capture_path
reveal_capture
delete_capture
```

> Nota: no se necesita `begin_capture_file_drag`. El arrastre se inicia desde
> el frontend con `startDrag` de `tauri-plugin-drag`.

Eventos:

```text
capture-session-started
capture-selection-changed
capture-created
capture-deleted
capture-session-ended
capture-error
capture-shelf-updated
```

Solo puede existir una sesión activa. Si se vuelve a presionar el atajo, la
sesión actual se cancela.

## 15. Configuración

### Campos nuevos en `Config`

`Config` vive en `crates/core/src/config.rs` y usa el patrón de migración con
`ConfigFile`. Cada campo nuevo debe añadirse en **cinco** lugares para no
romper configs existentes:

1. `struct Config` (campo + doc).
2. `impl Default for Config`.
3. `struct ConfigFile` (como `Option<…>`).
4. `impl Default for ConfigFile`.
5. `impl From<ConfigFile> for Config` (con fallback al default si es `None`).

Campos a añadir:

```rust
screenshot_shortcut: String,          // atajo de captura
capture_shelf_side: String,           // "right" | "left"
capture_shelf_timeout_seconds: u32,   // retracción del shelf
capture_retention_hours: u32,         // vencimiento de archivos
capture_include_cursor: bool,         // incluir cursor en el PNG
```

Valores iniciales:

```text
Atajo: Ctrl+Shift+4        (screenshot_shortcut)
Costado: right             (capture_shelf_side)
Retracción: 20             (capture_shelf_timeout_seconds)
Retención: 24              (capture_retention_hours)
Incluir cursor: false      (capture_include_cursor)
```

### Sección `Capturas` en Ajustes (`SettingsModal.svelte`)

- Atajo (reusar el componente existente `HotkeyCapture.svelte`).
- Intentar usar `Print Screen`.
- Posición derecha o izquierda.
- Tiempo antes de retraer.
- Duración de archivos temporales.
- Incluir cursor.
- Abrir carpeta.
- Limpiar capturas ahora.

`Print Screen` se puede intentar registrar, pero Atic no modificará el
registro de Windows ni deshabilitará Recortes. Si está ocupado, se informa el
conflicto y se conserva el atajo anterior.

### Refactor del registro de atajos (`shortcuts.rs`)

Estado actual (verificado): `register_shortcuts` desregistra todo con
`unregister_all()` y registra grabación/dictado/pill en cadena con `?`; un
fallo en cualquiera aborta el resto. Con un cuarto atajo (captura) el riesgo
crece.

Diseño objetivo:

- Mantener `unregister_all()` al inicio.
- Registrar **cada** atajo de forma independiente, capturando su error en un
  `Vec<String>` (o un struct de resultado por acción) en vez de `?`.
- Devolver qué atajos fallaron, para que la UI muestre el conflicto por acción
  sin desactivar los demás.
- La verificación de igualdad (hoy pares grabación/dictado/pill) debe incluir
  `screenshot_shortcut`.
- En `commands::set_config`, la condición que decide re-registrar (hoy compara
  los 3 atajos con `prev`) debe incluir `screenshot_shortcut`, y la firma de
  `register_shortcuts` pasa a recibir también el atajo de captura.

## 16. Rendimiento objetivo

- Actividad en reposo prácticamente nula.
- Overlay visible en menos de 120 ms desde el atajo (con ventanas ya
  creadas; la primera invocación puede tardar más mientras se crean y quedan
  reutilizables).
- Captura 1080p disponible en menos de 350 ms.
- Captura 4K disponible en menos de 700 ms.
- Miniatura visible en menos de 500 ms.
- Inicio del drag en menos de 100 ms.
- Codificación PNG fuera del hilo principal (hilo dedicado, patrón de
  `summon_pill_to_cursor` / `meeting_detection`).
- Máximo de cinco miniaturas decodificadas simultáneamente.

`BitBlt` y `PrintWindow` son operaciones rápidas; el costo dominante es la
codificación PNG, que va fuera del hilo principal. Los frames congelados
viven solo durante la sesión y se liberan al terminar. Para cumplir los
120 ms, overlays y shelf se crean una vez y se reutilizan ocultos: crear una
ventana WebView2 desde cero tarda cientos de milisegundos.

## 17. Accesibilidad

- Contraste suficiente en contorno y dimensiones.
- No depender exclusivamente del color para indicar selección.
- `Esc`, `Enter`, `Espacio` y `Tab` funcionales (el foco de teclado sigue al
  overlay bajo el cursor; el shelf **no** roba foco).
- Ayuda contextual visible.
- Restaurar el foco a la aplicación anterior al cerrar el overlay.
- Shelf sin activación ni robo de teclado.
- Animaciones de 150–200 ms.
- Alternativa inmediata o crossfade con movimiento reducido.
- Dimensiones legibles entre 100% y 200% de escalado.
- Errores concretos, por ejemplo:

```text
No fue posible capturar esta ventana.
Intenta seleccionar una región de la pantalla.
```

## 18. Permisos y capabilities

`apps/desktop/src-tauri/capabilities/default.json` hoy cubre solo
`["main", "pill"]`. Las ventanas nuevas necesitan permisos:

- Añadir `capture-overlay-*` y `capture-shelf` a un capability (el existente o
  uno nuevo `capture.json`), con al menos: `core:event:default`,
  `core:window:default`, `core:window:allow-set-position`,
  `core:window:allow-close`, `core:webview:default`.
- Añadir el permiso del plugin de drag (`drag:default` o el que exponga
  `tauri-plugin-drag`) al capability de las ventanas del shelf.
- Registrar el plugin en `lib.rs`: `.plugin(tauri_plugin_drag::init())`.

## 19. Fases de implementación

### Fase 0 — Spike técnico

Duración: 1 día.

- Capturar un monitor con `BitBlt` y una ventana con `PrintWindow` → generar un
  PNG válido.
- Integrar `tauri-plugin-drag` y arrastrar ese PNG a los destinos clave.
- Confirmar los requisitos de `startDrag` (formato del icono, gesto, hilo).
- Medir mostrar/ocultar una ventana WebView2 reutilizada (objetivo <120 ms).
- Validar Windows Terminal, VS Code, Claude Code, Chrome y Edge.

Criterio de continuación: no construir la UI completa hasta demostrar que el
PNG se arrastra correctamente a los destinos principales. Riesgo mucho menor
que antes porque el drag lo aporta un plugin probado.

### Fase 1 — Motor y almacenamiento

Duración: 3–4 días.

- Crear `crates/capture` (miembro del workspace) con deps `windows-sys`
  (Gdi/Dwm/HiDpi), `png`, `serde`, `chrono`.
- Añadir `AppDirs::captures_dir()` en `crates/core`.
- Enumerar monitores (Tauri) y ventanas (`EnumWindows` + DWM bounds).
- Implementar captura de monitor/ventana y recorte de región.
- Codificar BGRA → PNG.
- Implementar limpieza por antigüedad (patrón `retention.rs` con guard de
  canonicalización).
- Añadir pruebas de geometría, filtros y retención.

### Fase 2 — Overlay y selección inteligente

Duración: 4–6 días.

- Crear overlays por monitor con `WebviewWindowBuilder` (reutilizables).
- Manejar cambios de configuración de monitores entre sesiones.
- Preseleccionar ventanas por hit-testing (§12).
- Seleccionar regiones (arrastre > 4 px).
- Capturar el monitor (`Espacio`).
- Mostrar dimensiones y ayuda contextual.
- Controles de teclado (`Esc`/`Enter`/`Espacio`/`Tab`).
- Compatibilidad multi-DPI (coords físicas internas, conversión en el borde).
- Excluir las propias ventanas de Atic.

### Fase 3 — Shelf y drag-and-drop

Duración: 4–5 días.

- Crear ventana flotante `capture-shelf` (dinámica, sin foco, always-on-top).
- Stack de cinco miniaturas.
- Retracción a los 20 segundos (pausa en hover y durante un arrastre).
- Copiar imagen (`arboard`) y copiar ruta.
- Arrastre nativo con `startDrag` del plugin.
- Eliminar y abrir ubicación.
- Manejar archivos eliminados externamente.

### Fase 4 — Integración y ajustes

Duración: 2–3 días.

- Añadir `screenshot_shortcut` configurable.
- Refactorizar `shortcuts.rs` (registro independiente por acción; §15).
- Añadir sección `Capturas` en `SettingsModal.svelte`.
- Registrar plugin de drag + capabilities (§18).
- Integrar con tray y arranque.
- Persistir preferencias (migración `ConfigFile`).
- Manejar errores y estados.

### Fase 5 — QA y endurecimiento

Duración: 3–4 días.

- Windows 10 y 11.
- Uno, dos y tres monitores.
- Escalados 100%, 125%, 150% y 200%.
- Monitores con coordenadas negativas.
- Pantallas verticales.
- Taskbar en diferentes posiciones.
- Aplicaciones GPU (verificar `PW_RENDERFULLCONTENT`).
- Contenido DRM (verificar frame negro + fallback a región).
- Terminales y chats.
- Cancelaciones repetidas.
- Limpieza de archivos.
- Rendimiento y memoria.

Estimación total: **17–23 días laborables**, aproximadamente 3–5 semanas para
una versión lista para compartir. La elección de GDI + `tauri-plugin-drag`
recorta el riesgo (y algo de tiempo) del motor y del arrastre frente al plan
anterior.

## 20. Pruebas obligatorias

### Unitarias (en `crates/capture`)

- Conversión entre coordenadas físicas y lógicas.
- Intersección de ventanas.
- Orden z-index.
- Filtro de ventanas no seleccionables.
- Recorte fuera de límites.
- Nombres de archivo.
- Retención por antigüedad.
- Stack máximo de cinco capturas.

### Integración

- Capturar una ventana de prueba conocida y verificar dimensiones del PNG.
- Capturar regiones en coordenadas negativas.
- Copiar imagen y ruta.
- Recuperar el shelf después de reiniciar (escaneo de carpeta).
- Eliminar archivos inexistentes sin bloquear la UI.

### Manuales

- Arrastrar a Claude Code.
- Arrastrar a Windows Terminal.
- Arrastrar a VS Code.
- Adjuntar en Claude y ChatGPT.
- Arrastrar al escritorio.
- Capturar Chrome, aplicaciones Tauri y ventanas nativas.
- Capturar contenido en movimiento (video) y verificar que el resultado
  corresponde al momento del atajo.
- Confirmar que Atic no aparece en su propia captura.

## 21. Criterios de aceptación

La v1 se considera terminada cuando:

- El atajo abre la selección en menos de 120 ms (tras la primera invocación,
  que crea las ventanas reutilizables).
- Hover preselecciona correctamente ventanas comunes.
- Clic, región y monitor producen PNG válidos.
- El overlay nunca aparece en el resultado.
- La miniatura aparece sin cambiar el foco.
- El archivo puede arrastrarse a terminales y chats.
- Copiar imagen y copiar ruta funcionan.
- El shelf contiene como máximo cinco elementos.
- Se retrae después de 20 segundos.
- Los archivos se eliminan después de 24 horas.
- Funciona con dos monitores y DPI diferentes.
- Cancelar no genera archivos.
- Un error en el atajo de captura no rompe los demás atajos.
- No existe actividad significativa de CPU cuando está inactiva.

## 22. Riesgos y mitigaciones

### Drag externo desde WebView

Mitigación: delegar en `tauri-plugin-drag` (probado) y validarlo en la Fase 0
antes de construir la UI. Riesgo reducido frente al plan anterior (ya no se
implementa OLE a mano).

### Latencia de creación de ventanas WebView2

Crear una ventana WebView2 tarda cientos de milisegundos; hacerlo en cada
sesión rompería el objetivo de 120 ms.

Mitigación: crear overlays y shelf una vez (perezosamente) y reutilizarlos
ocultos; recrearlos solo si cambia la configuración de monitores. Se mide en
la Fase 0.

### Frame congelado desactualizado

Con «congelar primero», si la pantalla cambia durante la selección (p. ej. un
video), región y monitor reflejan el momento del atajo, no el del clic.

Mitigación: es el comportamiento estándar de las herramientas de captura y el
que el usuario espera; queda documentado (§11) y cubierto por una prueba
manual (§20).

### DPI y múltiples monitores

Mitigación: trabajar internamente con coordenadas físicas y convertir solo en
los límites del frontend, usando los `scale_factor` de `available_monitors()`.

### Ventanas protegidas y DRM / GDI en negro

Mitigación: detectar frame negro/uniforme, informar que el contenido no se
puede capturar y ofrecer captura de región como alternativa.

### `PrintWindow` incompleto en algunas apps

Mitigación: usar `PW_RENDERFULLCONTENT`; si el resultado sale negro o
uniforme, degradar automáticamente al recorte del rect de la ventana desde el
frame congelado.

### UAC y escritorio seguro

Mitigación: aceptar la restricción del sistema y cancelar de forma segura.

### `Print Screen` ocupado

Mitigación: ofrecer un atajo alternativo y no modificar Windows.

### HDR

GDI puede devolver colores lavados en monitores HDR.

Mitigación: limitación aceptada y documentada en v1; se verifica en QA y, si
el resultado es inaceptable, el upgrade a WGC (§23) la resuelve en la segunda
fase.

### Aplicaciones elevadas

Mitigación: degradar de forma segura cuando Windows limite la inspección de un
proceso no elevado.

## 23. Segunda fase futura

- **Windows Graphics Capture** como upgrade de calidad (ventanas ocluidas sin
  artefactos), si el enfoque GDI resulta insuficiente. Implicaría añadir el
  crate `windows` (WinRT) + D3D11.
- OCR local con Windows OCR.
- Anotaciones: flechas, rectángulos y texto.
- Difuminado y pixelado.
- Copiar texto detectado.
- Captura con desplazamiento.
- Historial buscable.
- Fijar capturas para conservarlas.
- Acciones IA sobre la imagen.
- Enviar la captura directamente a un resumen o conversación.

## 24. Dirección visual

La función seguirá el registro visual restringido de Atic:

- Tema claro u oscuro según el sistema.
- Acento reservado para selección y acciones importantes.
- Movimiento breve y funcional.
- Ninguna decoración innecesaria.
- Sin bordes decorativos.
- Shelf compacto que desaparece del camino cuando no se usa.

Referencias funcionales:

- Shelf y arrastre de CleanShot X.
- Preselección de ventanas de Recortes de Windows.
- Velocidad operativa de Shottr.

## 25. Registro de implementación (Fases 0–4)

Estado real de lo construido y las desviaciones respecto a este plan.

### Hecho

- **Fase 0** — Spike validado: captura GDI (`BitBlt`/`PrintWindow`) → PNG y
  arrastre nativo con `tauri-plugin-drag` a Terminal/VS Code/Claude Code.
- **Fase 1** — `crates/capture` (motor GDI, geometría, PNG, retención) con 22
  pruebas. `AppDirs::captures_dir()` y `overlay_frames_dir()`.
- **Fase 2** — Overlay de selección: ventana (clic), región (arrastre),
  monitor (Espacio), `Esc`/`Enter`. Flujo «congelar primero».
- **Fase 3** — Shelf flotante: stack de 5, arrastre, copiar imagen/ruta,
  abrir ubicación, eliminar.
- **Fase 4** — Atajo `Ctrl+Shift+4` configurable, refactor de `shortcuts.rs`
  (registro independiente por acción), sección «Capturas» en Ajustes,
  retención automática al iniciar.
- **Fase 5** — Robustez en código (coords negativas, DPI, exclude-self,
  fallback de frame negro, cierre seguro del overlay). El QA manual
  (multi-monitor, DPI mixto, HDR) queda para pruebas del usuario.

### Desviaciones respecto al plan

1. **Overlay opaco, no transparente.** Las ventanas WebView2 `transparent`
   hacen crashear a wry en `WM_SETFOCUS` (null deref del controlador de
   composición) al recibir un clic. El overlay y el shelf son **opacos**; el
   overlay muestra el frame congelado como fondo, lo que además es el diseño
   «congelar primero» correcto.
2. **Un solo overlay que abarca el escritorio virtual**, en vez de uno por
   monitor. Más simple y robusto; asume DPI uniforme entre monitores (válido
   en el caso común). Multi-DPI por monitor queda como mejora futura.
3. **Eventos `screenshot-*`** (no `capture-*`) para no colisionar con los
   eventos de audio existentes (`capture-error`, `capture-warn`).
4. **Disparador por tray** («Capturar pantalla») además del atajo global. El
   comando `capture_primary_monitor` (monitor directo) se conserva.
5. **Auto-retracción del shelf a los 20 s** (§8): pendiente. El shelf se
   oculta con su botón «×». `capture_shelf_side` y
   `capture_shelf_timeout_seconds` ya existen en config para cablearlo.
