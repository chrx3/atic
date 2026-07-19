// Evita abrir una consola extra en Windows en release. NO ELIMINAR.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    atic_desktop_lib::run()
}
