//! El ícono de Atic en el exe de la pill (el que muestran el Explorador, la
//! barra de tareas y el acceso del menú Inicio). Mismo crate que usa Tauri
//! para el suyo.

fn main() {
    println!("cargo:rerun-if-changed=../../apps/desktop/src-tauri/icons/icon.ico");
    // La versión de la app: la de `tauri.conf.json`, la misma que sube el
    // script de release y que publica `latest.json`.
    println!("cargo:rerun-if-changed=../../apps/desktop/src-tauri/tauri.conf.json");
    let conf = std::fs::read_to_string("../../apps/desktop/src-tauri/tauri.conf.json").unwrap_or_default();
    let version = conf
        .lines()
        .find_map(|line| line.trim().strip_prefix("\"version\":"))
        .map(|rest| rest.trim().trim_end_matches(',').trim_matches('"').to_string())
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=ATIC_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = tauri_winres::WindowsResource::new();
    res.set_icon("../../apps/desktop/src-tauri/icons/icon.ico");
    // Lo que muestran el Administrador de tareas y el Explorador.
    res.set("FileDescription", "Atic");
    res.set("ProductName", "Atic");
    res.set("CompanyName", "chrx3");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    if let Err(error) = res.compile() {
        // Sin ícono el exe funciona igual: que no frene la compilación.
        println!("cargo:warning=pill-gpui sin ícono: {error}");
    }
}
