fn main() {
    tauri_build::build();

    // `tauri-build` embebe el manifiesto de Windows (Common-Controls v6) solo
    // en los binarios (`rustc-link-arg-bins`); no hay forma de dárselo también
    // al exe de tests sin duplicar el recurso y romper el enlace del bin. Sin
    // manifiesto, el exe de tests carga `comctl32` v5 de System32, y como
    // `rfd` y `muda` importan `TaskDialogIndirect` —que solo existe en la v6—
    // el proceso muere al arrancar con 0xC0000139 antes de correr un solo test.
    //
    // Con carga diferida, `comctl32` se resuelve en la primera llamada: en los
    // tests nunca llega, y en la app ocurre con el manifiesto ya activo, igual
    // que antes.
    #[cfg(windows)]
    {
        println!("cargo:rustc-link-arg=/DELAYLOAD:comctl32.dll");
        println!("cargo:rustc-link-arg=delayimp.lib");
    }
}
