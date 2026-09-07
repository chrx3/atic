//! Comandos Unix para las consolas de Atic.
//!
//! # Por qué existe
//!
//! Una consola de Atic en Windows deja al usuario en `cmd`, donde `ls` no
//! existe. Pedirle que instale Git Bash o que se acostumbre a `dir` es
//! trasladarle un problema nuestro: la consola la abrimos nosotros, así que
//! las herramientas las ponemos nosotros.
//!
//! # Por qué uutils y no busybox
//!
//! busybox-w32 es más pequeño y trae más comandos (`grep`, `sed`, `awk`), pero
//! es GPL-2.0: distribuirlo obliga a ofrecer su código. uutils es **MIT**, está
//! escrito en Rust como el resto del proyecto y entra como una dependencia más
//! —sin binarios vendorizados en el repo, sin descargas en el build, sin sumas
//! de verificación que mantener—.
//!
//! Lo que se pierde: `grep`, `sed`, `awk` y `find` no son coreutils y no están
//! acá. Si hacen falta, `ripgrep` (MIT) es el candidato para el primero.
//!
//! # Cómo se invoca
//!
//! Es un binario multiuso: mira su `argv[0]` y despacha. Atic crea, junto al
//! ejecutable, un enlace duro por comando (`ls.exe`, `cat.exe`, …) y pone esa
//! carpeta en el PATH de la consola. Llamado por su propio nombre, lista lo
//! que sabe hacer.
//!
//! # Crédito
//!
//! Los comandos son de uutils/coreutils, bajo licencia MIT. El texto de la
//! licencia viaja junto al binario y en Ajustes.

use std::ffi::OsString;

/// Qué comando corre cada nombre. Sumar uno es una línea acá y otra en el
/// `Cargo.toml`.
macro_rules! comandos {
    ($($nombre:literal => $krate:ident),* $(,)?) => {
        /// Los nombres que Atic enlaza junto al binario.
        pub const NOMBRES: &[&str] = &[$($nombre),*];

        fn correr(nombre: &str, args: impl Iterator<Item = OsString>) -> Option<i32> {
            match nombre {
                $($nombre => {
                    preparar_idioma($nombre);
                    Some($krate::uumain(args))
                })*
                _ => None,
            }
        }
    };
}

comandos! {
    "basename" => uu_basename,
    "cat" => uu_cat,
    "cp" => uu_cp,
    "cut" => uu_cut,
    "dirname" => uu_dirname,
    "du" => uu_du,
    "env" => uu_env,
    "head" => uu_head,
    "ls" => uu_ls,
    "mkdir" => uu_mkdir,
    "mv" => uu_mv,
    "nl" => uu_nl,
    "realpath" => uu_realpath,
    "rm" => uu_rm,
    "rmdir" => uu_rmdir,
    "seq" => uu_seq,
    "sort" => uu_sort,
    "tac" => uu_tac,
    "tail" => uu_tail,
    "tee" => uu_tee,
    "touch" => uu_touch,
    "tr" => uu_tr,
    "uniq" => uu_uniq,
    "wc" => uu_wc,
}

/// Carga los textos del comando antes de correrlo.
///
/// Desde 0.11 uutils traduce con Fluent y pide sus mensajes por clave. Sin
/// esta llamada el comando imprime **la clave**: `ls -l` encabezaba con
/// `ls-total` en vez de `total`. Si falla no se aborta: listar el directorio
/// con los textos raros es mejor que no listarlo.
fn preparar_idioma(nombre: &str) {
    if let Err(e) = uucore::locale::setup_localization(nombre) {
        eprintln!("atic-unix: sin textos para «{nombre}»: {e}");
    }
}

/// El nombre con el que se invocó, sin ruta ni extensión y en minúsculas.
///
/// En Windows el enlace se llama `ls.exe` y el usuario escribe `LS` o `ls`
/// indistintamente: `cmd` no distingue mayúsculas y el comando tampoco debería.
fn invocado_como(argv0: Option<OsString>) -> Option<String> {
    let raw = argv0?;
    let path = std::path::PathBuf::from(raw);
    let stem = path.file_stem()?.to_str()?.to_lowercase();
    Some(stem)
}

fn main() {
    let mut args = std::env::args_os();
    let argv0 = args.next();

    // Lo normal: invocado por el enlace, `ls.exe` corre `ls`.
    if let Some(nombre) = invocado_como(argv0) {
        if let Some(code) = correr(&nombre, std::iter::once(OsString::from(&nombre)).chain(args)) {
            std::process::exit(code);
        }
    }

    // Invocado por su propio nombre: el comando viene como primer argumento.
    let mut resto = std::env::args_os().skip(1);
    let Some(primero) = resto.next() else {
        ayuda();
        return;
    };
    let nombre = primero.to_string_lossy().to_lowercase();
    if let Some(code) = correr(&nombre, std::iter::once(primero).chain(resto)) {
        std::process::exit(code);
    }
    eprintln!("atic-unix: no conozco «{nombre}»");
    ayuda();
    std::process::exit(1);
}

fn ayuda() {
    println!("Comandos Unix de uutils/coreutils (MIT) para las consolas de Atic.");
    println!("Uso: atic-unix <comando> [args] — o llámalo por el nombre del comando.");
    println!();
    println!("{}", NOMBRES.join(" "));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_nombre_sale_de_la_ruta_sin_extension_ni_mayusculas() {
        assert_eq!(
            invocado_como(Some(OsString::from(r"C:\atic\bin\LS.exe"))).as_deref(),
            Some("ls")
        );
        assert_eq!(
            invocado_como(Some(OsString::from("/usr/local/bin/cat"))).as_deref(),
            Some("cat")
        );
        assert_eq!(invocado_como(None), None);
    }

    #[test]
    fn la_lista_no_tiene_repetidos_ni_vacios() {
        let mut vistos = std::collections::HashSet::new();
        for n in NOMBRES {
            assert!(!n.is_empty());
            assert!(vistos.insert(*n), "«{n}» está dos veces");
        }
        assert!(NOMBRES.contains(&"ls"), "el que motivó todo esto");
    }
}
