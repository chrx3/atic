//! Lo que está sonando en el equipo, y sus controles.
//!
//! En Windows sale de los controles de medios del sistema (SMTC): la misma
//! fuente que usa el panel de volumen de Windows, así que cubre Spotify, el
//! navegador, el reproductor del sistema y cualquier app que se registre ahí,
//! sin hablar con ninguna en particular.
//!
//! En macOS no hay API pública equivalente: se lee MediaRemote (privada) a
//! través de `osascript`, y los controles van como teclas multimedia. Ver
//! `imp` de macOS.

use serde::Serialize;

/// Lo que suena ahora. `None` en el comando = no hay nada (o está detenido).
#[derive(Debug, Clone, Serialize)]
pub struct MediaNow {
    pub title: String,
    pub artist: String,
    /// De qué app viene, ya legible («Spotify», «Msedge»).
    pub app: String,
    pub playing: bool,
    pub can_toggle: bool,
    pub can_next: bool,
    pub can_prev: bool,
    /// Si la app deja mover la posición (Spotify sí; en el navegador, según el sitio).
    pub can_seek: bool,
    /// Carátula como data URL. Falta si la app no la publica, o si quien pregunta
    /// ya la tiene (`known` == `thumb_key`): pesa cientos de KB y se sondea
    /// cada segundo y medio.
    pub thumbnail: Option<String>,
    /// Identifica el tema: cambia cuando cambia la carátula.
    pub thumb_key: String,
    pub position_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    /// Cuándo midió la app `position_ms` (ms Unix). Entre muestras la posición
    /// se extrapola en el frontend: las apps la publican cada varios segundos.
    pub updated_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy)]
enum MediaAction {
    Toggle,
    Next,
    Prev,
}

impl MediaAction {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "toggle" => Ok(Self::Toggle),
            "next" => Ok(Self::Next),
            "prev" => Ok(Self::Prev),
            other => Err(format!("acción de medios desconocida: {other}")),
        }
    }
}

/// Nombre legible a partir del id que da el SO.
///
/// Win32 da el ejecutable (`Spotify.exe`) y las apps empaquetadas su AUMID
/// (`Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic`). Ninguno de los
/// dos se lee bien tal cual. Los navegadores dan un hash (`F0DC299D809B9700`)
/// que no dice nada: ahí, vacío.
fn app_label(id: &str) -> String {
    if id.len() >= 12 && id.chars().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    let tail = id.rsplit('!').next().unwrap_or(id);
    let base = tail.split('_').next().unwrap_or(tail);
    let base = base
        .strip_suffix(".exe")
        .or_else(|| base.strip_suffix(".EXE"))
        .unwrap_or(base);
    let base = base.rsplit('.').next().unwrap_or(base);
    let mut chars = base.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// El volumen que mueve el reproductor.
#[derive(Debug, Clone, Serialize)]
pub struct MediaVolume {
    pub level: f32,
    /// `true` = el de la app que suena; `false` = el del equipo, porque la app
    /// no se pudo identificar (los navegadores se registran con un id opaco).
    pub app: bool,
}

#[tauri::command]
pub async fn media_now(known: Option<String>) -> Result<Option<MediaNow>, String> {
    // WinRT bloquea hasta que el SO responde: fuera del hilo principal.
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        let mut now = imp::now_known(known.as_deref())?;
        #[cfg(not(target_os = "macos"))]
        let mut now = imp::now()?;
        if let Some(now) = now.as_mut() {
            if known.as_deref() == Some(now.thumb_key.as_str()) {
                now.thumbnail = None;
            }
        }
        Ok(now)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn media_volume() -> Result<MediaVolume, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let exe = imp::source_exe();
        crate::system_control::media_volume(exe.as_deref())
            .map(|(level, app)| MediaVolume { level, app })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn media_set_volume(volume: f32) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let exe = imp::source_exe();
        crate::system_control::set_media_volume(exe.as_deref(), volume)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lleva el tema a `position_ms`, contado desde el inicio.
#[tauri::command]
pub async fn media_seek(position_ms: u64) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || imp::seek(position_ms))
        .await
        .map_err(|e| e.to_string())?
}

/// Trae al frente la ventana de la app que suena. `false` si no dio con ella.
#[tauri::command]
pub async fn media_focus() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(imp::focus)
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn media_control(action: String) -> Result<bool, String> {
    let action = MediaAction::parse(&action)?;
    tauri::async_runtime::spawn_blocking(move || imp::control(action))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(windows)]
mod imp {
    use std::sync::Mutex;

    use base64::Engine;
    use windows::core::Interface;
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::{DataReader, IInputStream, IRandomAccessStreamReference};
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    use super::{app_label, MediaAction, MediaNow};

    /// Una carátula de más de esto no es una carátula.
    const MAX_THUMB_BYTES: u64 = 2 * 1024 * 1024;
    /// De 1601 (época de Windows) a 1970, en ms.
    const WINDOWS_EPOCH_MS: i64 = 11_644_473_600_000;

    /// La última carátula leída, por tema: se sondea cada segundo y medio y
    /// releer la imagen cada vez sería lo único caro de todo el sondeo.
    static THUMB: Mutex<Option<(String, Option<String>)>> = Mutex::new(None);

    fn err(e: windows::core::Error) -> String {
        e.message().to_string()
    }

    fn session() -> Result<Option<Session>, String> {
        // Los hilos de `spawn_blocking` no tienen COM: sin esto WinRT falla.
        // Repetirlo en un hilo ya inicializado devuelve S_FALSE y no hace nada.
        // SAFETY: sin precondiciones; el resultado se descarta a propósito.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let manager = Manager::RequestAsync().map_err(err)?.get().map_err(err)?;
        // Sin sesión el SO devuelve null, que acá llega como error.
        Ok(manager.GetCurrentSession().ok())
    }

    fn read_thumbnail(reference: &IRandomAccessStreamReference) -> Option<String> {
        let stream = reference.OpenReadAsync().ok()?.get().ok()?;
        let size = stream.Size().ok()?;
        if size == 0 || size > MAX_THUMB_BYTES {
            return None;
        }
        let input: IInputStream = stream.cast().ok()?;
        let reader = DataReader::CreateDataReader(&input).ok()?;
        let loaded = reader.LoadAsync(size as u32).ok()?.get().ok()?;
        let mut bytes = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        let mime = stream
            .ContentType()
            .map(|t| t.to_string())
            .ok()
            .filter(|t| t.starts_with("image/"))
            .unwrap_or_else(|| "image/png".into());
        let data = base64::engine::general_purpose::STANDARD.encode(bytes);
        Some(format!("data:{mime};base64,{data}"))
    }

    pub fn now() -> Result<Option<MediaNow>, String> {
        let Some(session) = session()? else {
            return Ok(None);
        };
        let info = session.GetPlaybackInfo().map_err(err)?;
        let status = info.PlaybackStatus().map_err(err)?;
        // Detenido o cerrado no es «algo sonando»: el reproductor se esconde.
        if status != Status::Playing && status != Status::Paused {
            return Ok(None);
        }
        let controls = info.Controls().map_err(err)?;
        let props = session
            .TryGetMediaPropertiesAsync()
            .map_err(err)?
            .get()
            .map_err(err)?;
        let title = props.Title().map(|s| s.to_string()).unwrap_or_default();
        let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        let app = session
            .SourceAppUserModelId()
            .map(|s| app_label(&s.to_string()))
            .unwrap_or_default();

        let key = format!("{app}\u{1}{title}\u{1}{artist}");
        let thumb_key = key.clone();
        let thumbnail = {
            let cached = THUMB.lock().ok().and_then(|g| g.clone());
            match cached {
                Some((k, thumb)) if k == key => thumb,
                _ => {
                    let thumb = props.Thumbnail().ok().and_then(|r| read_thumbnail(&r));
                    if let Ok(mut guard) = THUMB.lock() {
                        *guard = Some((key, thumb.clone()));
                    }
                    thumb
                }
            }
        };

        let (position_ms, duration_ms, updated_ms) = match session.GetTimelineProperties() {
            Ok(tl) => {
                let ticks = |t: windows::Foundation::TimeSpan| t.Duration / 10_000;
                let start = tl.StartTime().map(ticks).unwrap_or(0);
                let end = tl.EndTime().map(ticks).unwrap_or(0);
                let pos = tl.Position().map(ticks).unwrap_or(0);
                let updated = tl
                    .LastUpdatedTime()
                    .map(|d| d.UniversalTime / 10_000 - WINDOWS_EPOCH_MS)
                    .ok()
                    .filter(|ms| *ms > 0);
                let duration = end - start;
                if duration > 0 {
                    (
                        Some((pos - start).clamp(0, duration) as u64),
                        Some(duration as u64),
                        updated,
                    )
                } else {
                    (None, None, None)
                }
            }
            Err(_) => (None, None, None),
        };

        Ok(Some(MediaNow {
            title,
            artist,
            app,
            playing: status == Status::Playing,
            can_toggle: controls.IsPlayPauseToggleEnabled().unwrap_or(false),
            can_next: controls.IsNextEnabled().unwrap_or(false),
            can_prev: controls.IsPreviousEnabled().unwrap_or(false),
            can_seek: position_ms.is_some()
                && controls.IsPlaybackPositionEnabled().unwrap_or(false),
            thumbnail,
            thumb_key,
            position_ms,
            duration_ms,
            updated_ms,
        }))
    }

    /// El ejecutable de la app que suena, si es una app Win32 (`spotify.exe`).
    /// Las empaquetadas y los navegadores dan un AUMID o un hash: `None`.
    pub fn source_exe() -> Option<String> {
        let id = session().ok()??.SourceAppUserModelId().ok()?.to_string();
        id.to_ascii_lowercase().ends_with(".exe").then_some(id)
    }

    pub fn control(action: MediaAction) -> Result<bool, String> {
        let Some(session) = session()? else {
            return Ok(false);
        };
        let op = match action {
            MediaAction::Toggle => session.TryTogglePlayPauseAsync(),
            MediaAction::Next => session.TrySkipNextAsync(),
            MediaAction::Prev => session.TrySkipPreviousAsync(),
        };
        op.map_err(err)?.get().map_err(err)
    }

    pub fn seek(position_ms: u64) -> Result<bool, String> {
        let Some(session) = session()? else {
            return Ok(false);
        };
        // La línea de tiempo puede no empezar en cero: la posición pedida es
        // relativa al inicio, igual que la que informa `now`.
        let start = session
            .GetTimelineProperties()
            .and_then(|tl| tl.StartTime())
            .map(|t| t.Duration)
            .unwrap_or(0);
        let ticks = start.saturating_add((position_ms as i64).saturating_mul(10_000));
        session
            .TryChangePlaybackPositionAsync(ticks)
            .map_err(err)?
            .get()
            .map_err(err)
    }

    /// El SO no dice qué ventana es la del reproductor, así que se busca por
    /// partes: el ejecutable (Win32), la activación del paquete (Store) y, para
    /// los navegadores, que solo dan un hash, una ventana cuyo título lleve el
    /// del tema (la pestaña activa lo pone ahí).
    pub fn focus() -> Result<bool, String> {
        let Some(session) = session()? else {
            return Ok(false);
        };
        let id = session
            .SourceAppUserModelId()
            .map(|s| s.to_string())
            .unwrap_or_default();
        if id.to_ascii_lowercase().ends_with(".exe") {
            if let Some(hwnd) = window::of_exe(&id) {
                window::bring(hwnd);
                return Ok(true);
            }
        } else if !app_label(&id).is_empty() && crate::launcher::aumid_valido(&id) {
            // Activar un paquete que ya corre trae su ventana, no abre otra.
            return Ok(crate::launcher::launch_apps_folder(&id).is_ok());
        }
        let title = session
            .TryGetMediaPropertiesAsync()
            .and_then(|op| op.get())
            .and_then(|p| p.Title())
            .map(|s| s.to_string())
            .unwrap_or_default();
        match window::titled(&title) {
            Some(hwnd) => {
                window::bring(hwnd);
                Ok(true)
            }
            None => Ok(false),
        }
    }

    mod window {
        use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
        use windows_sys::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            IsIconic, IsWindowVisible, ShowWindow, GW_OWNER, SW_RESTORE,
        };

        /// Ventanas de primer nivel visibles, con título y ajenas, en orden Z.
        fn candidates() -> Vec<(HWND, u32, String)> {
            unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> i32 {
                let out = &mut *(lparam as *mut Vec<(HWND, u32, String)>);
                if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
                    return 1;
                }
                let len = GetWindowTextLengthW(hwnd);
                if len <= 0 {
                    return 1;
                }
                let mut buf = vec![0u16; len as usize + 1];
                let got = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                if got > 0 && pid != 0 && pid != std::process::id() {
                    out.push((hwnd, pid, String::from_utf16_lossy(&buf[..got as usize])));
                }
                1
            }
            let mut out: Vec<(HWND, u32, String)> = Vec::new();
            // SAFETY: `visit` solo usa `lparam` como el Vec vivo de esta pila.
            unsafe {
                let _ = EnumWindows(Some(visit), &mut out as *mut _ as LPARAM);
            }
            out
        }

        fn exe_name(pid: u32) -> Option<String> {
            // SAFETY: el handle se cierra antes de salir y el buffer es local.
            unsafe {
                let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
                if handle.is_null() {
                    return None;
                }
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
                let _ = CloseHandle(handle);
                if ok == 0 {
                    return None;
                }
                let path = String::from_utf16_lossy(&buf[..len as usize]);
                path.rsplit('\\').next().map(str::to_ascii_lowercase)
            }
        }

        pub fn of_exe(exe: &str) -> Option<HWND> {
            let exe = exe.to_ascii_lowercase();
            candidates()
                .into_iter()
                .find(|(_, pid, _)| exe_name(*pid).as_deref() == Some(exe.as_str()))
                .map(|(hwnd, _, _)| hwnd)
        }

        pub fn titled(title: &str) -> Option<HWND> {
            let title = title.trim();
            // Con títulos muy cortos cualquier ventana calza.
            if title.chars().count() < 3 {
                return None;
            }
            candidates()
                .into_iter()
                .find(|(_, _, text)| text.contains(title))
                .map(|(hwnd, _, _)| hwnd)
        }

        pub fn bring(hwnd: HWND) {
            // SAFETY: `hwnd` salió de `EnumWindows` hace un instante.
            unsafe {
                if IsIconic(hwnd) != 0 {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
            }
            crate::clipboard_history::force_foreground(hwnd);
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;
    use std::sync::Mutex;

    use atic_core::MutexExt;
    use serde::Deserialize;

    use super::{MediaAction, MediaNow};

    /// Lee «lo que suena» de MediaRemote desde `osascript` (JXA).
    ///
    /// Desde macOS 15.4 MediaRemote no le contesta a apps de terceros, pero sí
    /// a binarios de Apple como `osascript`: por eso se pregunta a través de
    /// él y no desde el proceso de Atic. Ve lo mismo que el Centro de control:
    /// Spotify, Música y el navegador (YouTube Music) sin permisos extra.
    ///
    /// `argv[0]` es la clave del tema que el front ya tiene: si coincide, la
    /// carátula no se vuelve a codificar (pesa cientos de KB por sondeo).
    const SCRIPT: &str = r#"
function run(argv) {
  ObjC.import('Foundation');
  ObjC.import('AppKit');
  $.NSBundle.bundleWithPath('/System/Library/PrivateFrameworks/MediaRemote.framework/').load;
  const R = $.NSClassFromString('MRNowPlayingRequest');
  if (!R) return '';
  const item = R.localNowPlayingItem;
  if (!item || item.isNil()) return '';
  const info = item.nowPlayingInfo;
  if (!info || info.isNil()) return '';
  const get = (k) => {
    const v = info.objectForKey('kMRMediaRemoteNowPlayingInfo' + k);
    return v && !v.isNil() ? v : null;
  };
  const txt = (k) => { const v = get(k); return v ? ObjC.unwrap(v) : ''; };
  const num = (k) => { const v = get(k); return v ? Number(ObjC.unwrap(v)) : null; };
  const title = txt('Title');
  if (!title) return '';
  const path = R.localNowPlayingPlayerPath;
  let bundle = '';
  if (path && !path.isNil() && !path.client.isNil()) bundle = ObjC.unwrap(path.client.bundleIdentifier) || '';
  let app = '';
  if (bundle) {
    const url = $.NSWorkspace.sharedWorkspace.URLForApplicationWithBundleIdentifier(bundle);
    if (url && !url.isNil()) app = ObjC.unwrap($.NSFileManager.defaultManager.displayNameAtPath(url.path)).replace(/\.app$/, '');
  }
  const stamp = get('Timestamp');
  const key = bundle + ':' + (txt('ContentItemIdentifier') || title + '|' + txt('Artist'));
  let art = '';
  let mime = '';
  if (key !== argv[0]) {
    const data = get('ArtworkData');
    if (data) {
      art = ObjC.unwrap(data.base64EncodedStringWithOptions(0));
      mime = txt('ArtworkMIMEType') || 'image/jpeg';
    }
  }
  return JSON.stringify({
    playing: !!R.localIsPlaying,
    title: title,
    artist: txt('Artist'),
    app: app,
    bundle: bundle,
    duration: num('Duration'),
    elapsed: num('ElapsedTime'),
    stamp: stamp ? stamp.timeIntervalSince1970 * 1000 : null,
    key: key,
    art: art,
    mime: mime,
  });
}
"#;

    #[derive(Debug, Deserialize)]
    struct Raw {
        playing: bool,
        title: String,
        artist: String,
        app: String,
        bundle: String,
        duration: Option<f64>,
        elapsed: Option<f64>,
        stamp: Option<f64>,
        key: String,
        art: String,
        mime: String,
    }

    /// App que suena, para traerla al frente.
    static SOURCE: Mutex<Option<String>> = Mutex::new(None);

    fn parse(json: &str) -> Option<(MediaNow, String)> {
        let raw: Raw = serde_json::from_str(json).ok()?;
        let duration_ms = raw
            .duration
            .filter(|d| *d > 0.0)
            .map(|d| (d * 1000.0) as u64);
        let position_ms = raw.elapsed.map(|e| (e.max(0.0) * 1000.0) as u64);
        let thumbnail =
            (!raw.art.is_empty()).then(|| format!("data:{};base64,{}", raw.mime, raw.art));
        let now = MediaNow {
            title: raw.title,
            artist: raw.artist,
            app: raw.app,
            playing: raw.playing,
            can_toggle: true,
            can_next: true,
            can_prev: true,
            // Mover la posición pide un comando de MediaRemote, y esos sí
            // quedaron cerrados para terceros (no hay tecla que lo haga).
            can_seek: false,
            thumbnail,
            thumb_key: raw.key,
            position_ms,
            duration_ms,
            updated_ms: raw
                .stamp
                .map(|ms| ms as i64)
                .or_else(|| Some(chrono::Utc::now().timestamp_millis())),
        };
        Some((now, raw.bundle))
    }

    /// `known`: la clave de carátula que el front ya tiene.
    pub fn now_known(known: Option<&str>) -> Result<Option<MediaNow>, String> {
        let out = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", SCRIPT, known.unwrap_or("")])
            .output()
            .map_err(|e| format!("osascript: {e}"))?;
        let text = String::from_utf8_lossy(&out.stdout);
        let parsed = parse(text.trim());
        *SOURCE.lock_or_recover() = parsed.as_ref().map(|(_, bundle)| bundle.clone());
        Ok(parsed.map(|(now, _)| now))
    }

    /// Tecla multimedia (`NX_KEYTYPE_*`), igual que las del teclado: el sistema
    /// se la entrega a quien está sonando, sea app o pestaña del navegador.
    fn media_key(key: i64) -> bool {
        use objc2::runtime::AnyObject;

        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventPost(tap: u32, event: *mut std::ffi::c_void);
        }
        // SAFETY: NSEvent autoreleased dentro del pool; `CGEvent` devuelve un
        // CGEventRef +0 que vive mientras vive el NSEvent.
        objc2::rc::autoreleasepool(|_| unsafe {
            for (flags, state) in [(0xa00usize, 0xa_i64), (0xb00, 0xb)] {
                let event: *mut AnyObject = objc2::msg_send![
                    objc2::class!(NSEvent),
                    otherEventWithType: 14usize,
                    location: objc2_foundation::NSPoint::new(0.0, 0.0),
                    modifierFlags: flags,
                    timestamp: 0.0f64,
                    windowNumber: 0isize,
                    context: std::ptr::null_mut::<AnyObject>(),
                    subtype: 8i16,
                    data1: (key << 16) | (state << 8),
                    data2: -1isize
                ];
                if event.is_null() {
                    return false;
                }
                let cg: *mut std::ffi::c_void = objc2::msg_send![event, CGEvent];
                if cg.is_null() {
                    return false;
                }
                CGEventPost(0, cg);
            }
            true
        })
    }

    pub fn control(action: MediaAction) -> Result<bool, String> {
        // NX_KEYTYPE_PLAY / NEXT / PREVIOUS.
        Ok(media_key(match action {
            MediaAction::Toggle => 16,
            MediaAction::Next => 17,
            MediaAction::Prev => 18,
        }))
    }

    pub fn seek(_position_ms: u64) -> Result<bool, String> {
        Ok(false)
    }

    pub fn focus() -> Result<bool, String> {
        use objc2::runtime::AnyObject;
        use objc2_foundation::{NSArray, NSString};

        let Some(bundle) = SOURCE.lock_or_recover().clone().filter(|b| !b.is_empty()) else {
            return Ok(false);
        };
        objc2::rc::autoreleasepool(|_| unsafe {
            let id = NSString::from_str(&bundle);
            let apps: *mut AnyObject = objc2::msg_send![
                objc2::class!(NSRunningApplication),
                runningApplicationsWithBundleIdentifier: &*id
            ];
            if apps.is_null() {
                return Ok(false);
            }
            let apps: &NSArray<AnyObject> = &*apps.cast();
            let Some(app) = apps.iter().next() else {
                return Ok(false);
            };
            // NSApplicationActivateAllWindows.
            let ok: bool = objc2::msg_send![&*app, activateWithOptions: 1usize];
            Ok(ok)
        })
    }

    pub fn source_exe() -> Option<String> {
        None
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lee_lo_que_suena_en_el_navegador() {
            let (now, bundle) = parse(
                r#"{"playing":true,"title":"Aunque No Sea Conmigo","artist":"Macha","app":"Zen","bundle":"app.zen-browser.zen","duration":175.3,"elapsed":12.5,"stamp":1790300709000.5,"key":"app.zen-browser.zen:abc","art":"","mime":""}"#,
            )
            .unwrap();
            assert!(now.playing);
            assert_eq!(now.app, "Zen");
            assert_eq!(bundle, "app.zen-browser.zen");
            assert_eq!(now.duration_ms, Some(175_300));
            assert_eq!(now.position_ms, Some(12_500));
            assert_eq!(now.updated_ms, Some(1_790_300_709_000));
            assert!(now.thumbnail.is_none());
            assert!(!now.can_seek);
        }

        #[test]
        fn nada_sonando_es_none() {
            assert!(parse("").is_none());
        }

        #[test]
        fn la_caratula_viaja_como_data_url() {
            let (now, _) = parse(
                r#"{"playing":false,"title":"T","artist":"A","app":"Música","bundle":"com.apple.Music","duration":null,"elapsed":null,"stamp":null,"key":"k","art":"QUJD","mime":"image/png"}"#,
            )
            .unwrap();
            assert_eq!(now.thumbnail.as_deref(), Some("data:image/png;base64,QUJD"));
            assert!(now.updated_ms.is_some());
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use super::{MediaAction, MediaNow};

    pub fn now() -> Result<Option<MediaNow>, String> {
        Ok(None)
    }

    pub fn control(_action: MediaAction) -> Result<bool, String> {
        Ok(false)
    }

    pub fn seek(_position_ms: u64) -> Result<bool, String> {
        Ok(false)
    }

    pub fn focus() -> Result<bool, String> {
        Ok(false)
    }

    pub fn source_exe() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::app_label;

    #[test]
    fn nombres_legibles_de_app() {
        assert_eq!(app_label("Spotify.exe"), "Spotify");
        assert_eq!(app_label("msedge"), "Msedge");
        assert_eq!(
            app_label("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"),
            "ZuneMusic"
        );
        assert_eq!(app_label(""), "");
        assert_eq!(app_label("F0DC299D809B9700"), "");
    }
}
