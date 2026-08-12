# Plan de remediación — code review (fases 3–6)

> **Estado: cerrado.** F1–F10 están aplicados y verificados contra el código
> actual. Varios se resolvieron por refactor y no por el parche literal que
> propone este documento: F3/F9 vive hoy en `playback.playSpeaker`, F4/F10 en
> `rollbackGeneration` y los `try/catch` de `SummaryModal`, y F5 desapareció al
> reescribir la detección (`meeting_detection.rs` ya no escanea procesos: mira
> títulos de ventana). Queda como registro de por qué el código es así.
>
> El §0 de entorno quedó **desactualizado**: la receta buena, con el paso que
> falta para LLVM 19+, está en el README y en `PLAN_AGENTES.md`.

Plan autocontenido para aplicar con otro agente. Corrige 10 hallazgos de un
code review + deja el gate de CI (`clippy -D warnings`, `cargo fmt --check`) en
verde. Ordenado por severidad. Cada ítem indica archivo, ubicación, problema y
el arreglo concreto.

---

## 0. Entorno de build (LEER PRIMERO — imprescindible)

El crate `atic-transcribe` usa `whisper-rs`, que compila whisper.cpp y necesita
**CMake** y **LLVM/libclang**. Sin esto, `cargo build/clippy/test` de cualquier
crate que dependa de transcribe FALLA. En este equipo ya están instalados; hay
que exponerlos por entorno en cada invocación de cargo (PowerShell):

```powershell
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
$env:Path = "$env:USERPROFILE\AppData\Local\atic-tools\cmake-3.31.6-windows-x86_64\bin;$env:USERPROFILE\.cargo\bin;$env:Path"
$cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe"
```

`cargo` NO está en el PATH de Git Bash; usar la ruta completa o PowerShell con el
`$env:Path` de arriba. Comandos de verificación (los que corre el CI):

```powershell
& $cargo fmt --all --check
& $cargo clippy --workspace --all-targets -- -D warnings
& $cargo test --workspace
```

Frontend (desde `apps/desktop/`): `pnpm check` (svelte-check) y `pnpm build`.

> Nota: puede haber otro agente editando en paralelo. Antes de empezar, confirmar
> con `git status` que el árbol está quieto; re-verificar cada hallazgo contra el
> estado actual del archivo antes de tocarlo (las líneas pueden haberse movido).

---

## Correctness

### F1 — Grabación imposible de transcribir (severidad alta)
- **Archivo:** `crates/core/src/config.rs`, `effective_transcribe_tracks` (~L270).
- **Problema:** `effective_record_tracks` fuerza `"system"` siempre que
  `speakers_mode` esté activo, pero `effective_transcribe_tracks` solo lo fuerza
  cuando `transcribe_tracks == "both"`. Con `speakers_mode=true` +
  `transcribe_tracks="mic"` se graba solo `system.wav` pero la transcripción pide
  la pista `mic` (inexistente) → `transcribe_recording` devuelve “No hay pistas
  para transcribir” y la grabación nunca se puede transcribir.
- **Arreglo:** espejar la lógica de grabación — si `speakers_mode`, forzar
  `"system"` siempre (garantiza transcribe ⊆ record):

```rust
pub fn effective_transcribe_tracks(&self) -> &str {
    if self.speakers_mode {
        return "system";
    }
    match self.transcribe_tracks.as_str() {
        "mic" | "system" => self.transcribe_tracks.as_str(),
        _ => "both",
    }
}
```
- **Extra defensivo (opcional):** en `transcription.rs::transcribe_recording`, en
  vez de error “No hay pistas”, transcribir las pistas que SÍ existen en disco.

### F2 — `set_config` persiste un atajo inválido antes de validarlo (alta)
- **Archivo:** `apps/desktop/src-tauri/src/commands.rs`, `set_config` (~L63-82).
- **Problema:** escribe `config` a memoria (L70) y disco (L72-74) y recién después
  llama `register_recording_shortcut` (L78-79). Si el atajo no parsea, el comando
  reporta error pero el valor inválido ya quedó en `config.json`; en el próximo
  arranque el registro vuelve a fallar y el hotkey queda muerto sin recuperación.
- **Arreglo:** validar/parsear el atajo ANTES de persistir. Si cambia el atajo,
  registrarlo primero; solo si tiene éxito, guardar. Reordenar así:

```rust
// 1) Si el atajo cambió, validar+registrar PRIMERO.
if shortcut != prev_shortcut {
    crate::shortcuts::register_recording_shortcut(&app, &shortcut)?; // aborta si es inválido
}
// 2) Recién ahora persistir memoria + disco.
{ *state.config.lock().unwrap() = config.clone(); }
config.save(&state.dirs.config_path()).map_err(|e| e.to_string())?;
state::set_pill_visible(&app, show_pill);
sync_autostart(&app, want_autostart);
Ok(())
```
  (Verificar que `register_recording_shortcut` realmente devuelva `Err` en un
  atajo no parseable; si hoy loguea y sigue, hacer que retorne error.)

### F3 — La transcripción solo reproduce una pista (media-alta)
- **Archivo:** `apps/desktop/src/lib/TranscriptModal.svelte`, `seek()` (~L20-27).
- **Problema:** `if (!audioEl.src)` fija la pista (mic si existe) una sola vez y
  nunca cambia. Al hacer clic en un segmento “Los demás” (sistema) se reproduce
  la pista del micrófono (silencio si el usuario no habló) en ese timestamp. No
  se puede escuchar a la otra persona.
- **Arreglo:** elegir la pista según el hablante del segmento y recargar `src`
  cuando cambia. Requiere pasar el `speaker` a `seek`. Ver también F9 (esperar
  metadata antes del seek); combinar ambos:

```svelte
<script lang="ts">
  let currentTrack: "mic" | "system" | null = null;

  async function seek(ms: number, speaker: "me" | "others") {
    // "me" → mic; "others" → system. Fallback a la pista que exista.
    let track: "mic" | "system" = speaker === "me" ? "mic" : "system";
    if (track === "mic" && !recording.mic_path) track = "system";
    if (track === "system" && !recording.system_path) track = "mic";

    if (currentTrack !== track) {
      audioEl.src = await trackSrc(recording.id, track);
      currentTrack = track;
      await new Promise<void>((resolve) => {
        audioEl.addEventListener("loadedmetadata", () => resolve(), { once: true });
      });
    }
    audioEl.currentTime = ms / 1000;
    await audioEl.play();
  }
</script>
```
  En el template, pasar el hablante: `onclick={() => seek(seg.start_ms, seg.speaker)}`.

### F4 — “Regenerar” borra el resumen guardado si el backend falla (media)
- **Archivo:** `apps/desktop/src/lib/SummaryModal.svelte`, `generate()` (~L59-76).
- **Problema:** pone `body = ""` (L62) antes del `await`. Si `summarizeRecording`
  rechaza (p. ej. sin API key), `body` queda vacío mientras `summary` aún tiene el
  texto viejo → el textarea se ve en blanco y Guardar aparece deshabilitado.
- **Arreglo:** en el `catch`, restaurar el cuerpo desde el resumen existente:

```ts
} catch (e) {
  const msg = String(e);
  onToast(msg);
  generating = false;
  body = summary?.body ?? "";      // restaurar lo que había
  streamRaw = "";
  if (msg.toLowerCase().includes("api key") || msg.toLowerCase().includes("ollama")) {
    onNeedSetup?.();
  }
}
```

### F5 — Falsos positivos de “llamada detectada” (media, on por defecto)
- **Archivo:** `apps/desktop/src-tauri/src/call_detect.rs`, `scan_call_signals` (~L167).
- **Problema:** `found.extend(scan_process_list(sys, MEETING_PROCESSES))` agrega
  procesos (zoom.exe, goto.exe, webex.exe, ciscocollabhost.exe…) de forma
  incondicional, a diferencia de `RESIDENT_PROCESSES` que exigen mic en uso.
  Zoom minimizado en bandeja dispara “¿Quieres grabarla?” (y fuerza la pill) cada
  ~10 min sin reunión, con `call_detection=true` por defecto.
- **Arreglo:** exigir señal de actividad también para procesos de reunión. Como
  los títulos de ventana fuertes ya se agregaron en el paso 1 (que sí exigen
  “activo”), gatear el paso 2 tras el mic:

```rust
// 2) Procesos de reunión: solo si el micrófono está en uso.
if mic {
    found.extend(scan_process_list(sys, MEETING_PROCESSES));
}
```
  (En no-Windows `mic=false`, por lo que la detección por proceso no dispara; es
  aceptable — macOS ya usa su propia señal de mic. Revisar que no se pierda la
  detección legítima de Zoom-en-llamada, que sí tiene el mic activo.)

### F7 — Parseo SSE sensible al espacio de `data:` (media-baja)
- **Archivos:** `crates/summarize/src/openai_compat.rs` (~L119) y
  `crates/summarize/src/claude.rs` (~L97).
- **Problema:** `line.strip_prefix("data: ")` exige el espacio. La spec SSE
  permite `data:` sin espacio; un endpoint OpenAI-compatible self-hosted que emita
  `data:{...}` produce cero deltas y falla con “el modelo no devolvió texto”.
  (Claude siempre manda “data: ” con espacio, pero unificar es correcto.)
- **Arreglo (en ambos archivos):**

```rust
let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
    continue;
};
```

### F8 — Error del keyring se traga y se reporta como “falta API key” (media-baja)
- **Archivo:** `apps/desktop/src-tauri/src/summarization.rs`,
  `summarizer_config_from_app` (~L96-97).
- **Problema:** `get_secret(kind).ok().flatten()` descarta cualquier error real del
  llavero (bloqueado, secret-service/D-Bus caído). `api_key` queda `None` y la UI
  dice “agrega tu API key” aunque SÍ esté guardada.
- **Arreglo:** distinguir `None` (no hay) de `Err` (falla del keyring):

```rust
let api_key = match SecretKind::for_summary_provider(&cfg.summary_backend) {
    Some(kind) => atic_core::secrets::get_secret(kind).map_err(|e| e.to_string())?,
    None => None,
};
```

### F9 — Primer clic en un segmento arranca en 0:00 (baja)
- **Archivo:** `apps/desktop/src/lib/TranscriptModal.svelte`, `seek()` (~L25).
- **Problema:** `audioEl.currentTime = ms/1000` justo tras asignar `src`, antes de
  cargar metadata; el navegador ignora el seek en el primer clic.
- **Arreglo:** ya cubierto por el rewrite de F3 (espera `loadedmetadata` antes de
  fijar `currentTime`). Si F3 no se aplica, al menos esperar el evento
  `loadedmetadata` la primera vez.

### F10 — Modal de resumen se cuelga en “Cargando…” ante error transitorio (baja-media)
- **Archivo:** `apps/desktop/src/lib/SummaryModal.svelte`, `onMount` (~L144-150).
- **Problema:** el IIFE `async` hace `templates = await listSummaryTemplates()` y
  luego `await reload()`. Si `listSummaryTemplates()` rechaza, `reload()` nunca
  corre, `loading` queda `true` para siempre y hay un rechazo no manejado.
- **Arreglo:** envolver en try/catch que limpie `loading` y avise:

```ts
(async () => {
  try {
    templates = await listSummaryTemplates();
    if (templates[0]) selectedTemplate = templates[0].id;
    await reload();
  } catch (e) {
    onToast(String(e));
    loading = false;
  }
})();
```

---

## F6 — Gate de CI en rojo (aplicar al final, tras F1–F10)

### fmt (18 archivos sin formatear)
- **Arreglo:** `cargo fmt --all` (con el entorno del §0). Verificar con
  `cargo fmt --all --check` → exit 0.

### clippy `-D warnings` (3 errores)
1. `apps/desktop/src-tauri/src/call_detect_win.rs:50` — `collapsible_if`:
   colapsar el `if` externo con el interno usando `&&`.
2. `apps/desktop/src-tauri/src/call_detect_win.rs:64` — `collapsible_if`: ídem.
   (Ejemplo del patrón: `if a { if b { … } }` → `if a && b { … }`.)
3. `crates/transcribe/src/whisper.rs:228` — `assertions_on_constants`: hay un
   `assert!(...)` con valor constante en un test. Reemplazar por una aserción
   real sobre `is_silence_marker` (p. ej. `assert!(is_silence_marker("[silence]"))`
   y `assert!(!is_silence_marker("hola"))`) o quitar la aserción trivial.
- **Verificar:** `cargo clippy --workspace --all-targets -- -D warnings` → sin
  warnings/errores. (Puede aparecer alguno nuevo tras editar; resolverlo.)

---

## Menores / opcionales (de los agentes; no bloquean)

- `apps/desktop/src-tauri/src/call_detect_win.rs` (~L141,156): `CoInitializeEx`
  sin `CoUninitialize` (desbalance de refcount en el hilo de polling) e ignora su
  HRESULT; además una sesión de captura cuyo `GetProcessId` falla se cuenta como
  “de otra app” (posible falso mic-en-uso). Considerar: no contar sesiones con PID
  ilegible, y valorar `CoUninitialize` o inicializar COM una sola vez por hilo.
- `apps/desktop/src/routes/+page.svelte` `showToast` (~L147): cada llamada agenda
  un `setTimeout` sin cancelar el anterior; toasts rápidos se borran antes. Guardar
  el handle y `clearTimeout` antes de reprogramar.
- Modales (`SettingsModal`, `SummaryModal`, `TranscriptModal`, `ConfirmModal`):
  sin cerrar con `Escape` ni focus-trap. Agregar `keydown Escape` → `onClose` y
  foco inicial. (UX.)
- `SettingsModal.svelte` / `+page.svelte` init IIFEs sin try/catch (mismo patrón
  que F10): si `getConfig()`/`secretsStatus()`/`listSummaryProviders()` rechazan,
  el modal queda en “Cargando…”. Envolver en try/catch.

---

## Checklist de cierre

- [x] F1–F5, F7–F10 aplicados y revisados.
- [x] `cargo fmt --all` aplicado (F6).
- [x] `cargo clippy --workspace --all-targets -- -D warnings` → verde.
- [x] `cargo test --workspace` → verde.
- [x] `pnpm check` (svelte-check) sin errores; `pnpm build` OK.
- [ ] Prueba manual: grabar → transcribir → en el modal, clic en un segmento
      “Los demás” reproduce la pista del sistema desde el timestamp correcto;
      “Regenerar” sin API key no borra el resumen guardado; con Zoom minimizado
      (sin llamada) NO aparece la sugerencia de grabar.
