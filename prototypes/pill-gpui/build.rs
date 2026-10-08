//! El ícono de Atic en el exe de la pill (el que muestran el Explorador, la
//! barra de tareas y el acceso del menú Inicio). Mismo crate que usa Tauri
//! para el suyo.

fn main() {
    println!("cargo:rerun-if-changed=../../apps/desktop/src-tauri/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = tauri_winres::WindowsResource::new();
    res.set_icon("../../apps/desktop/src-tauri/icons/icon.ico");
    if let Err(error) = res.compile() {
        // Sin ícono el exe funciona igual: que no frene la compilación.
        println!("cargo:warning=pill-gpui sin ícono: {error}");
    }
}
