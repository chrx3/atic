//! Ajustes de WebView2: la app no es un navegador.

use tauri::{Manager, WebviewWindow};

/// Desactiva atajos y chrome de Chromium (Ctrl+P, Ctrl+F, zoom, menú Inspect).
#[cfg(windows)]
pub fn disable_browser_accelerator_keys(window: &WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
    use windows_core::Interface;

    let label = window.label().to_string();
    let label_for_cb = label.clone();
    if let Err(err) = window.with_webview(move |webview| {
        unsafe {
            let Ok(core) = webview.controller().CoreWebView2() else {
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
        }
    }) {
        tracing::warn!(%label, %err, "with_webview falló al desactivar atajos");
    }
}

#[cfg(not(windows))]
pub fn disable_browser_accelerator_keys(_window: &WebviewWindow) {}

/// Ajusta el bounds del controlador WebView2 al cliente de la ventana.
///
/// wry crea un HWND hijo `WRY_WEBVIEW`. `SetBounds` solo no basta: hay que
/// redimensionar ese hijo. Si queda chico, el CSS no cubre el overlay, la pill
/// no llega al mouse y los hit-rects no coinciden con el cursor.
#[cfg(windows)]
pub fn sync_controller_bounds(window: &WebviewWindow) {
    use windows::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    let mut rc = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // SAFETY: HWND de Tauri vivo; GetClientRect solo escribe el RECT.
    let ok =
        unsafe { windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd.0 as _, &mut rc) };
    if ok == 0 || rc.right <= 0 || rc.bottom <= 0 {
        return;
    }
    let width = rc.right - rc.left;
    let height = rc.bottom - rc.top;

    // SAFETY: el overlay vive; FindWindowEx / SetWindowPos solo tocan su hijo.
    unsafe {
        let mut class: Vec<u16> = "WRY_WEBVIEW".encode_utf16().collect();
        class.push(0);
        let child = FindWindowExW(
            hwnd.0 as _,
            std::ptr::null_mut(),
            class.as_ptr(),
            std::ptr::null(),
        );
        if child.is_null() {
            tracing::warn!(
                target: "overlay",
                label = %window.label(),
                width,
                height,
                "sin HWND WRY_WEBVIEW: el webview todavía no nació"
            );
        } else {
            SetWindowPos(
                child,
                std::ptr::null_mut(),
                0,
                0,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }

    let bounds = RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let label = window.label().to_string();
    if let Err(err) = window.with_webview(move |webview| unsafe {
        if let Err(err) = webview.controller().SetBounds(bounds) {
            tracing::warn!(label = %label, %err, "WebView2: no se pudo sincronizar Bounds");
        }
    }) {
        tracing::warn!(label = %window.label(), %err, "with_webview falló al sincronizar Bounds");
    }
}

#[cfg(not(windows))]
pub fn sync_controller_bounds(_window: &WebviewWindow) {}

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
