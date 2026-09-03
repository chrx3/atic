//! Ajustes de WebView2: la app no es un navegador.

use tauri::{Manager, WebviewWindow};

/// Desactiva atajos y chrome de Chromium (Ctrl+P, Ctrl+F, zoom, menú Inspect).
#[cfg(windows)]
pub fn disable_browser_accelerator_keys(window: &WebviewWindow) {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    use tauri::Emitter;
    use webview2_com::AcceleratorKeyPressedEventHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2AcceleratorKeyPressedEventArgs2, ICoreWebView2Settings3,
        COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN,
    };
    use windows_core::Interface;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT,
    };

    // `SetAreBrowserAcceleratorKeysEnabled(false)` solo entra en vigor tras la
    // próxima navegación. La pill ya está cargada cuando Tauri nos entrega el
    // WebView, así que además filtramos cada acelerador en el controlador. Eso
    // evita que WebView2 se quede con Ctrl+D/Ctrl+N y deja que el evento siga
    // hacia el DOM (SetHandled(true) NO serviría: también cortaría el DOM).
    static FILTERS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

    let label = window.label().to_string();
    let label_for_cb = label.clone();
    let window_for_keys = window.clone();
    if let Err(err) = window.with_webview(move |webview| {
        unsafe {
            let controller = webview.controller();
            let controller_id = Interface::as_raw(&controller) as usize;
            let Ok(core) = controller.CoreWebView2() else {
                tracing::warn!(label = %label_for_cb, "WebView2: sin CoreWebView2");
                return;
            };
            let Ok(settings) = core.Settings() else {
                tracing::warn!(label = %label_for_cb, "WebView2: sin Settings");
                return;
            };
            if let Err(err) = settings.SetAreDefaultContextMenusEnabled(false) {
                tracing::warn!(label = %label_for_cb, %err, "no se pudo desactivar el menú contextual");
            }
            if let Err(err) = settings.SetIsZoomControlEnabled(false) {
                tracing::warn!(label = %label_for_cb, %err, "no se pudo desactivar el zoom");
            }
            if let Err(err) = settings.SetAreDevToolsEnabled(cfg!(debug_assertions)) {
                tracing::warn!(label = %label_for_cb, %err, "no se pudo ajustar DevTools");
            }
            let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() else {
                tracing::warn!(label = %label_for_cb, "WebView2: Settings3 no disponible");
                return;
            };
            if let Err(err) = settings3.SetAreBrowserAcceleratorKeysEnabled(false) {
                tracing::warn!(label = %label_for_cb, %err, "no se pudo desactivar atajos del navegador");
            }

            let filters = FILTERS.get_or_init(|| Mutex::new(HashMap::new()));
            let already_installed = filters
                .lock()
                .map(|installed| installed.get(&label_for_cb) == Some(&controller_id))
                .unwrap_or(false);
            if already_installed {
                return;
            }

            let label_for_key = label_for_cb.clone();
            let handler = AcceleratorKeyPressedEventHandler::create(Box::new(
                move |_controller, args| {
                    let Some(args) = args else {
                        return Ok(());
                    };
                    let mut virtual_key = 0u32;
                    let _ = args.VirtualKey(&mut virtual_key);
                    let Ok(args2) = args.cast::<ICoreWebView2AcceleratorKeyPressedEventArgs2>()
                    else {
                        return Ok(());
                    };

                    let mut browser_enabled = windows_core::BOOL::default();
                    let _ = args2.IsBrowserAcceleratorKeyEnabled(&mut browser_enabled);
                    args2.SetIsBrowserAcceleratorKeyEnabled(false)?;

                    // Este runtime confirma `browser_enabled=false`, pero aun
                    // así no entrega Ctrl+D/Ctrl+N al DOM. Enviar los acordes
                    // del workspace como acciones nativas evita depender de
                    // esa propagación rota.
                    let mut event_kind = Default::default();
                    let _ = args.KeyEventKind(&mut event_kind);
                    let ctrl_down = GetKeyState(i32::from(VK_CONTROL)) < 0;
                    let shift_down = GetKeyState(i32::from(VK_SHIFT)) < 0;
                    let alt_down = GetKeyState(i32::from(VK_MENU)) < 0;
                    // WebView2 se come Ctrl+D/N/W aunque el acelerador del
                    // browser esté apagado. Reinyectarlos como layout de la
                    // consola: partir, nueva pestaña, cerrar. Ctrl+Shift+D
                    // parte hacia abajo y no choca con el split a la derecha.
                    let action = if label_for_key == "overlay"
                        && event_kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN
                        && ctrl_down
                        && !alt_down
                    {
                        match virtual_key {
                            0x44 if shift_down => Some("split-down"),
                            0x44 => Some("split-right"),
                            0x4e if !shift_down => Some("new-console"),
                            0x57 if !shift_down => Some("close-console"),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(action) = action {
                        match window_for_keys.emit("agents-workspace-shortcut", action) {
                            Ok(()) => {
                                args.SetHandled(true)?;
                                tracing::info!(
                                    target: "keyboard",
                                    label = %label_for_key,
                                    %action,
                                    "atajo de agentes enviado directamente al frontend"
                                );
                            }
                            Err(err) => tracing::warn!(
                                target: "keyboard",
                                label = %label_for_key,
                                %action,
                                %err,
                                "no se pudo enviar el atajo de agentes"
                            ),
                        }
                    }

                    // Diagnóstico deliberadamente acotado a los atajos del
                    // workspace para no ensuciar el log con cada pulsación.
                    if matches!(virtual_key, 0x44 | 0x4e | 0x57) {
                        tracing::info!(
                            target: "keyboard",
                            label = %label_for_key,
                            key = virtual_key,
                            browser_enabled = browser_enabled.as_bool(),
                            "WebView2 dejó pasar el acelerador hacia el DOM"
                        );
                    }
                    Ok(())
                },
            ));
            let mut token = 0i64;
            match controller.add_AcceleratorKeyPressed(&handler, &mut token) {
                Ok(()) => {
                    if let Ok(mut installed) = filters.lock() {
                        installed.insert(label_for_cb.clone(), controller_id);
                    }
                    tracing::info!(
                        target: "keyboard",
                        label = %label_for_cb,
                        token,
                        "filtro nativo de aceleradores instalado"
                    );
                }
                Err(err) => tracing::warn!(
                    target: "keyboard",
                    label = %label_for_cb,
                    %err,
                    "no se pudo instalar el filtro nativo de aceleradores"
                ),
            }
        }
    }) {
        tracing::warn!(%label, %err, "with_webview falló al desactivar atajos");
    }
}

#[cfg(not(windows))]
pub fn disable_browser_accelerator_keys(_window: &WebviewWindow) {}

/// Píxeles DIP que WebView2 espera en `SetBounds` para un cliente físico.
pub(crate) fn controller_dip_size(physical: i32, scale: f64) -> i32 {
    ((physical as f64) / scale.max(0.01))
        .round()
        .clamp(1.0, 16384.0) as i32
}

/// Cliente físico del HWND. `GetClientRect` a veces ya viene en DIP.
pub(crate) fn physical_client_px(
    client_w: i32,
    client_h: i32,
    outer_w: i32,
    outer_h: i32,
    scale: f64,
) -> (i32, i32) {
    if client_w <= 0 || client_h <= 0 {
        return (outer_w.max(1), outer_h.max(1));
    }
    if scale > 1.01 {
        let scaled_w = (f64::from(client_w) * scale).round() as i32;
        if (scaled_w - outer_w).abs() <= 32 {
            return (outer_w.max(1), outer_h.max(1));
        }
        return (client_w, client_h);
    }
    (client_w, client_h)
}

/// Rasterización de un HWND que cubre el escritorio virtual.
///
/// `user_scale` es el tamaño de Ajustes (1.0 = 1 CSS px por px físico).
/// Si el HWND ya está en DIP, se ignora para no hacer doble zoom.
pub(crate) fn overlay_raster_scale(
    hwnd_dpi: u32,
    tauri_scale: f64,
    max_monitor_scale: f64,
    window_w: i32,
    virtual_physical_w: u32,
    user_scale: f64,
) -> f64 {
    let from_hwnd = f64::from(hwnd_dpi.max(96)) / 96.0;
    let user = atic_core::config::sanitize_overlay_scale(user_scale);
    // HWND ya en DIP (proceso unaware): rasterizar al máximo haría doble zoom.
    if virtual_physical_w > 0 && max_monitor_scale > 1.01 {
        let dip_w = (f64::from(virtual_physical_w) / max_monitor_scale).round() as i32;
        if (window_w - dip_w).abs() <= 32 {
            return from_hwnd.max(tauri_scale).max(1.0);
        }
    }
    user
}

/// Ajusta el bounds del controlador WebView2 al cliente de la ventana.
///
/// wry crea un HWND hijo `WRY_WEBVIEW`. El modo por defecto de WebView2 es
/// `USE_RAW_PIXELS`: `SetBounds` y el hijo van en píxeles físicos, y
/// `RasterizationScale` define cuántos CSS px caben ahí. Si `SetBounds` recibe
/// DIP, el bitmap queda chico, Windows lo estira y `innerWidth` se divide otra
/// vez por la escala (zoom + blur).
///
/// Devuelve la escala de rasterización aplicada, para que el overlay no
/// siga usando `scale_factor=1` en el mapeo CSS.
#[cfg(windows)]
pub fn sync_controller_bounds(window: &WebviewWindow) -> Option<f64> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Controller3, COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS,
    };
    use windows::Win32::Foundation::RECT;
    use windows_core::Interface;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    let Ok(hwnd) = window.hwnd() else {
        return None;
    };
    let hwnd_sys = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    let mut client = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let mut outer = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: HWND de Tauri vivo; estas APIs solo leen el rectángulo.
    let ok_client = unsafe { GetClientRect(hwnd_sys, &mut client) };
    let ok_outer = unsafe { GetWindowRect(hwnd_sys, &mut outer) };
    if ok_client == 0 || client.right <= 0 || client.bottom <= 0 {
        return None;
    }
    let client_w = client.right - client.left;
    let client_h = client.bottom - client.top;
    let outer_w = if ok_outer == 0 {
        client_w
    } else {
        outer.right - outer.left
    };
    let outer_h = if ok_outer == 0 {
        client_h
    } else {
        outer.bottom - outer.top
    };

    let hwnd_dpi = unsafe { GetDpiForWindow(hwnd_sys) };
    let tauri_scale = window.scale_factor().unwrap_or(1.0);
    let spanning = window.label() == "overlay" || window.label() == "capture-overlay";
    let (max_mon, vs_w) = if spanning {
        let max_mon = atic_capture::monitors::enumerate()
            .iter()
            .map(|m| m.scale)
            .fold(1.0_f64, f64::max);
        (max_mon, atic_capture::monitors::virtual_screen().width)
    } else {
        (1.0, 0)
    };
    let user_scale = window
        .app_handle()
        .try_state::<crate::state::AppState>()
        .map(|s| {
            use atic_core::MutexExt;
            atic_core::config::sanitize_overlay_scale(s.config.lock_or_recover().overlay_scale)
        })
        .unwrap_or(1.0);
    let scale = overlay_raster_scale(hwnd_dpi, tauri_scale, max_mon, outer_w, vs_w, user_scale);
    let (phys_w, phys_h) = physical_client_px(client_w, client_h, outer_w, outer_h, scale);
    let dip_w = controller_dip_size(phys_w, scale);
    let dip_h = controller_dip_size(phys_h, scale);

    // SAFETY: el overlay vive; FindWindowEx / SetWindowPos solo tocan su hijo.
    unsafe {
        let mut class: Vec<u16> = "WRY_WEBVIEW".encode_utf16().collect();
        class.push(0);
        let child = FindWindowExW(
            hwnd_sys,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        );
        if child.is_null() {
            tracing::warn!(
                target: "overlay",
                label = %window.label(),
                phys_w,
                phys_h,
                "sin HWND WRY_WEBVIEW: el webview todavía no nació"
            );
        } else {
            SetWindowPos(
                child,
                std::ptr::null_mut(),
                0,
                0,
                phys_w,
                phys_h,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    let bounds = RECT {
        left: 0,
        top: 0,
        right: phys_w,
        bottom: phys_h,
    };
    let label = window.label().to_string();
    if let Err(err) = window.with_webview(move |webview| unsafe {
        let controller = webview.controller();
        if let Err(err) = controller.SetZoomFactor(1.0) {
            tracing::warn!(label = %label, %err, "WebView2: no se pudo fijar ZoomFactor");
        }
        if let Ok(c3) = controller.cast::<ICoreWebView2Controller3>() {
            if let Err(err) = c3.SetShouldDetectMonitorScaleChanges(false) {
                tracing::warn!(
                    label = %label,
                    %err,
                    "WebView2: no se pudo fijar ShouldDetectMonitorScaleChanges"
                );
            }
            if let Err(err) = c3.SetBoundsMode(COREWEBVIEW2_BOUNDS_MODE_USE_RAW_PIXELS) {
                tracing::warn!(label = %label, %err, "WebView2: no se pudo fijar BoundsMode");
            }
            if let Err(err) = c3.SetRasterizationScale(scale) {
                tracing::warn!(label = %label, %err, "WebView2: no se pudo fijar RasterizationScale");
            }
        }
        if let Err(err) = controller.SetBounds(bounds) {
            tracing::warn!(label = %label, %err, "WebView2: no se pudo sincronizar Bounds");
        }
    }) {
        tracing::warn!(label = %window.label(), %err, "with_webview falló al sincronizar Bounds");
    }
    tracing::debug!(
        target: "overlay",
        label = %window.label(),
        phys_w,
        phys_h,
        dip_w,
        dip_h,
        scale,
        hwnd_dpi,
        tauri_scale,
        "WebView2 bounds sincronizados"
    );
    Some(scale)
}

#[cfg(not(windows))]
pub fn sync_controller_bounds(_window: &WebviewWindow) -> Option<f64> {
    None
}

/// Pide a WebView2 un PNG de lo que está mostrando (pill, launcher, goo).
///
/// `BitBlt` / `PrintWindow` no ven esta ventana layered. Chromium sí puede
/// rasterizarla. El PNG suele traer alpha de verdad; si no, el freeze hace
/// knockout del negro vacío.
#[cfg(windows)]
pub fn capture_preview_png(window: &WebviewWindow) -> Result<Vec<u8>, String> {
    use std::time::Duration;
    use webview2_com::CapturePreviewCompletedHandler;
    use webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG;
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{
        ISequentialStream, IStream, STATFLAG, STATSTG, STREAM_SEEK_SET,
    };

    fn read_istream(stream: &IStream) -> Result<Vec<u8>, String> {
        unsafe {
            stream
                .Seek(0, STREAM_SEEK_SET, None)
                .map_err(|err| err.to_string())?;
            let mut stat = STATSTG::default();
            stream
                .Stat(&mut stat, STATFLAG(1))
                .map_err(|err| err.to_string())?;
            let size = stat.cbSize as usize;
            if size == 0 {
                return Err("CapturePreview devolvió un stream vacío".into());
            }
            let mut buf = vec![0u8; size];
            let mut read = 0u32;
            ISequentialStream::Read(
                stream,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                size as u32,
                Some(&mut read),
            )
            .ok()
            .map_err(|err| err.to_string())?;
            let mut total = read as usize;
            while total < size {
                let mut chunk = 0u32;
                ISequentialStream::Read(
                    stream,
                    buf[total..].as_mut_ptr() as *mut std::ffi::c_void,
                    (size - total) as u32,
                    Some(&mut chunk),
                )
                .ok()
                .map_err(|err| err.to_string())?;
                if chunk == 0 {
                    break;
                }
                total += chunk as usize;
            }
            buf.truncate(total);
            Ok(buf)
        }
    }

    let (tx, rx) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| unsafe {
            let send = |msg: Result<Vec<u8>, String>| {
                let _ = tx.send(msg);
            };
            let Ok(core) = webview.controller().CoreWebView2() else {
                send(Err("WebView2: sin CoreWebView2".into()));
                return;
            };
            let Ok(stream) = CreateStreamOnHGlobal(HGLOBAL::default(), true) else {
                send(Err("no se pudo crear el IStream de CapturePreview".into()));
                return;
            };
            let stream_done = stream.clone();
            let tx_done = tx.clone();
            let handler = CapturePreviewCompletedHandler::create(Box::new(move |result| {
                match result {
                    Ok(()) => match read_istream(&stream_done) {
                        Ok(bytes) => {
                            let _ = tx_done.send(Ok(bytes));
                        }
                        Err(err) => {
                            let _ = tx_done.send(Err(err));
                        }
                    },
                    Err(err) => {
                        let _ = tx_done.send(Err(err.to_string()));
                    }
                }
                Ok(())
            }));
            if let Err(err) = core.CapturePreview(
                COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                &stream,
                &handler,
            ) {
                send(Err(err.to_string()));
            }
        })
        .map_err(|err| err.to_string())?;

    rx.recv_timeout(Duration::from_millis(800))
        .map_err(|_| "CapturePreview tardó demasiado".to_string())?
}

#[cfg(not(windows))]
pub fn capture_preview_png(_window: &WebviewWindow) -> Result<Vec<u8>, String> {
    Err("CapturePreview solo existe en Windows".into())
}

/// Todas las ventanas, incluida `main`. Llamar después de crear el overlay.
pub fn apply_to_all_windows(app: &tauri::AppHandle) {
    for (_, window) in app.webview_windows() {
        disable_browser_accelerator_keys(&window);
    }
}

#[cfg(test)]
mod tests {
    use super::{controller_dip_size, overlay_raster_scale, physical_client_px};

    #[test]
    fn dip_from_physical_125() {
        assert_eq!(controller_dip_size(1920, 1.25), 1536);
    }

    #[test]
    fn dip_from_physical_150() {
        assert_eq!(controller_dip_size(3840, 1.5), 2560);
    }

    #[test]
    fn dip_at_100_is_identity() {
        assert_eq!(controller_dip_size(1920, 1.0), 1920);
    }

    #[test]
    fn client_physical_when_rects_match() {
        assert_eq!(
            physical_client_px(3840, 1080, 3840, 1080, 1.5),
            (3840, 1080)
        );
    }

    #[test]
    fn client_dip_uses_outer_physical() {
        assert_eq!(physical_client_px(2560, 720, 3840, 1080, 1.5), (3840, 1080));
    }

    #[test]
    fn mixed_dpi_default_is_compact() {
        assert!((overlay_raster_scale(96, 1.0, 1.5, 3840, 3840, 1.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn user_scale_is_the_raster() {
        assert!((overlay_raster_scale(96, 1.0, 1.5, 3840, 3840, 1.25) - 1.25).abs() < 0.001);
    }

    #[test]
    fn unaware_hwnd_does_not_double_zoom() {
        // Outer ya en DIP (2560) con escritorio físico 3840 @ 150%.
        assert!((overlay_raster_scale(96, 1.0, 1.5, 2560, 3840, 1.25) - 1.0).abs() < 0.001);
    }

    #[test]
    fn single_monitor_still_honors_user_scale() {
        assert!((overlay_raster_scale(120, 1.25, 1.25, 1920, 1920, 1.0) - 1.0).abs() < 0.001);
    }
}
