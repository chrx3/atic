//! El ícono de Atic en el exe (el que muestran el Explorador, la barra de
//! tareas y el acceso del menú Inicio) y la versión de la app.

fn main() {
    println!("cargo:rerun-if-changed=assets/app/icon.ico");
    // La versión de la app es la de este `Cargo.toml`: la que sube el script
    // de release, la que publica `latest.json` y la de las Novedades.
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    println!("cargo:rustc-env=ATIC_VERSION={version}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = tauri_winres::WindowsResource::new();
    res.set_icon("assets/app/icon.ico");
    // Lo que muestran el Administrador de tareas y el Explorador.
    res.set("FileDescription", "Atic");
    res.set("ProductName", "Atic");
    res.set("CompanyName", "chrx3");
    res.set("FileVersion", &version);
    res.set("ProductVersion", &version);
    if let Err(error) = res.compile() {
        // Sin ícono el exe funciona igual: que no frene la compilación.
        println!("cargo:warning=atic sin ícono: {error}");
    }
}
