//! Spike de captura: valida que el motor produce PNGs con contenido.
//!
//! Congela cada monitor y captura la ventana en primer plano, escribiendo los
//! PNG a la carpeta temporal. En macOS pide permiso de grabación de pantalla.
//!
//! Uso:
//!   cargo run --example spike -p atic-capture

#[cfg(any(windows, target_os = "macos"))]
fn main() {
    use atic_capture::{engine, monitors, windows as win};

    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    #[cfg(target_os = "macos")]
    {
        let access = core_graphics::access::ScreenCaptureAccess;
        println!(
            "permiso grabación de pantalla: preflight={} request={}",
            access.preflight(),
            access.request()
        );
    }

    let out_dir = std::env::temp_dir();
    println!("Escribiendo capturas en: {}", out_dir.display());

    let mons = monitors::enumerate();
    println!("\nMonitores detectados: {}", mons.len());
    println!("Escritorio virtual: {:?}", monitors::virtual_screen());
    for m in &mons {
        println!(
            "  {} bounds={:?} work={:?} primary={}",
            m.id, m.bounds, m.work_area, m.is_primary
        );
    }

    println!("\n== Captura de monitores (congelar primero) ==");
    for monitor in &mons {
        match engine::capture_rect(monitor.bounds, false) {
            Ok(frame) => {
                let path = out_dir.join(format!(
                    "spike_{}x{}_{}.png",
                    frame.width(),
                    frame.height(),
                    frame.bounds.x
                ));
                report(&frame, &path);
            }
            Err(error) => println!("  {} error: {error}", monitor.id),
        }
    }

    let candidates = win::enumerate_candidates(0, &mons);
    println!("\n== Ventanas candidatas: {} ==", candidates.len());
    for candidate in candidates.iter().take(8) {
        println!(
            "  id={} pid={} {} {:?}",
            candidate.hwnd, candidate.process_id, candidate.title, candidate.visual_bounds
        );
    }

    println!("\n== Captura de la ventana en primer plano ==");
    let hwnd = win::foreground_window();
    if hwnd == 0 {
        println!("  (sin ventana en primer plano)");
    } else {
        match engine::capture_window(hwnd) {
            Ok(frame) => {
                let path = out_dir.join(format!("spike_window_{hwnd:x}.png"));
                report(&frame, &path);
            }
            Err(error) => println!("  error al capturar la ventana: {error}"),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn report(frame: &atic_capture::Frame, path: &std::path::Path) {
    let non_black = frame.bgra.iter().any(|&b| b > 8);
    match frame.to_png() {
        Ok(png) => {
            if let Err(error) = std::fs::write(path, &png) {
                println!("  {}: error al escribir: {error}", path.display());
                return;
            }
            println!(
                "  {} — {}x{} — {} bytes PNG — {}",
                path.display(),
                frame.width(),
                frame.height(),
                png.len(),
                if non_black {
                    "con contenido"
                } else {
                    "TODO NEGRO (posible DRM/GPU)"
                }
            );
        }
        Err(error) => println!("  {}: error al codificar: {error}", path.display()),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn main() {
    eprintln!("El spike de capturas solo funciona en Windows y macOS.");
}
