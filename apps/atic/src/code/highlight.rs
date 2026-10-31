//! El resaltado del visor de solo lectura: los colores de cada línea salen de
//! `SyntaxLines` de gpui-m3 (el analizador guarda su estado cada 16 líneas, así que
//! una línea cualquiera cuesta a lo más re-analizar esas 16) y el visor sigue
//! dibujando con `uniform_list`, solo las filas a la vista.

use std::cell::RefCell;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gpui::HighlightStyle;
use gpui_m3::{SyntaxLines, SyntaxPalette};

/// El resaltador de un archivo; `None` dentro si no hay gramática para su extensión.
pub type Syntax = Rc<RefCell<Option<SyntaxLines>>>;

/// El resaltador de las líneas de ahora. Se guarda el último: el visor se dibuja a cada
/// cuadro, y cuando recarga el archivo cambia el `Arc` de las líneas y se rehace.
pub fn doc_syntax(lines: &Arc<Vec<String>>, path: &Path) -> Syntax {
    thread_local! {
        static LAST: RefCell<Option<(Arc<Vec<String>>, Syntax)>> = const { RefCell::new(None) };
    }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((of, syntax)) = last.as_ref() {
            if Arc::ptr_eq(of, lines) {
                return syntax.clone();
            }
        }
        let syntax: Syntax = Rc::new(RefCell::new(SyntaxLines::for_path(&path.to_string_lossy())));
        *last = Some((lines.clone(), syntax.clone()));
        syntax
    })
}

/// Los colores de la línea `index` (rangos en bytes dentro de ella).
pub fn line_styles(syntax: &mut SyntaxLines, lines: &[String], index: usize, palette: &SyntaxPalette) -> Vec<(Range<usize>, HighlightStyle)> {
    syntax
        .tokens(index, |ix| lines.get(ix).map(String::as_str).unwrap_or(""))
        .into_iter()
        .map(|(range, kind)| (range, palette.style(kind)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_m3::Scheme;

    #[test]
    fn cada_linea_se_pinta_con_el_estado_de_las_anteriores() {
        let palette = SyntaxPalette::from_scheme(&Scheme::baseline_dark(), true);
        let lines: Vec<String> = ["fn main() {", "    /* un comentario", "       que sigue */", "    let x = 1;"].map(String::from).to_vec();
        let mut syntax = SyntaxLines::for_path("main.rs").expect("hay gramática de Rust");
        // `fn` es una palabra clave.
        let first = line_styles(&mut syntax, &lines, 0, &palette);
        assert!(first.iter().any(|(r, s)| *r == (0..2) && s.color == Some(palette.keyword)));
        // La segunda línea del comentario sigue siendo comentario (no se analiza suelta).
        let inside = line_styles(&mut syntax, &lines, 2, &palette);
        assert!(inside.iter().any(|(r, s)| r.start <= 7 && r.end >= 18 && s.color == Some(palette.comment)), "{inside:?}");
        // Y la siguiente vuelve a ser código, también si se pide fuera de orden.
        let after = line_styles(&mut syntax, &lines, 3, &palette);
        assert!(after.iter().any(|(_, s)| s.color == Some(palette.keyword)));
        assert!(line_styles(&mut syntax, &lines, 99, &palette).is_empty());
    }

    #[test]
    fn sin_gramatica_no_hay_resaltador() {
        let lines = Arc::new(vec!["x".to_string()]);
        assert!(doc_syntax(&lines, Path::new("notas.zzz")).borrow().is_none());
        let rust = Arc::new(vec!["fn a() {}".to_string()]);
        assert!(doc_syntax(&rust, Path::new("src/a.rs")).borrow().is_some());
        // El mismo `Arc` devuelve el mismo resaltador (no se rehace en cada cuadro).
        assert!(Rc::ptr_eq(&doc_syntax(&rust, Path::new("src/a.rs")), &doc_syntax(&rust, Path::new("src/a.rs"))));
    }
}
