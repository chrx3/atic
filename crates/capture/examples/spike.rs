//! Spike de la Fase 0: valida que la captura GDI produce PNGs correctos.
//!
//! Congela cada monitor y captura la ventana en primer plano, escribiendo los
//! PNG a la carpeta temporal e informando dimensiones y si el frame trae
//! contenido (no todo negro). No cubre el arrastre nativo (requiere prueba
//! manual con `tauri-plugin-drag`).
//!
//! Uso:
//!   cargo run --example spike -p atic-capture

#[cfg(windows)]
fn main() {
    use atic_capture::{engine, monitors, windows as win};

    // El spike es un binario suelto (no Tauri), así que debe declararse
    // consciente de DPI para que BitBlt capture píxeles físicos reales.
    unsafe {
        use windows_sys::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
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
    for frame in engine::freeze_monitors(&mons, false) {
        let path = out_dir.join(format!(
            "spike_{}x{}_{}.png",
            frame.width(),
            frame.height(),
            frame.bounds.x
        ));
        report(&frame, &path);
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

#[cfg(windows)]
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

#[cfg(not(windows))]
fn main() {
    eprintln!("El spike de capturas solo funciona en Windows.");
}
