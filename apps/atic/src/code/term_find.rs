//! Buscar en el historial de la terminal: la lógica pura (qué líneas coinciden y
//! dónde), sin GPUI. La vista (`terminal.rs`) arma las líneas con
//! `console::row_text`, guarda las coincidencias y las pinta con `kit`.

/// Una coincidencia en el historial: la fila en la foto (0 = la primera línea
/// leída) y, en caracteres, dónde empieza y cuántos cubre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Found {
    pub row: usize,
    pub col: usize,
    pub len: usize,
}

/// Las filas cuyas líneas contienen `query` (sin distinguir mayúsculas ni
/// tildes en el caso, como la búsqueda del chat): una entrada por cada vez que
/// aparece, de arriba abajo. Una búsqueda vacía no encuentra nada.
pub fn find_in_lines(lines: &[String], query: &str) -> Vec<Found> {
    let query: Vec<char> = query.trim().to_lowercase().chars().collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.to_lowercase().chars().collect();
        if chars.len() < query.len() {
            continue;
        }
        let mut from = 0;
        while from + query.len() <= chars.len() {
            if chars[from..from + query.len()] == query[..] {
                out.push(Found { row, col: from, len: query.len() });
                from += query.len();
            } else {
                from += 1;
            }
        }
    }
    out
}

/// La coincidencia que sigue (o la anterior) desde `current`, dando la vuelta en
/// los extremos, como la búsqueda del chat.
pub fn step_from(current: usize, total: usize, forward: bool) -> usize {
    if total == 0 {
        0
    } else if forward {
        (current + 1) % total
    } else {
        (current + total - 1) % total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn busca_sin_distinguir_mayusculas_y_varias_por_linea() {
        let found = find_in_lines(&lines(&["Hola mundo", "hola HOLA", "nada"]), "hola");
        assert_eq!(found, vec![Found { row: 0, col: 0, len: 4 }, Found { row: 1, col: 0, len: 4 }, Found { row: 1, col: 5, len: 4 }]);
        assert_eq!(find_in_lines(&lines(&["Hola"]), "HOLA").len(), 1);
    }

    #[test]
    fn una_busqueda_vacia_no_encuentra_nada() {
        assert!(find_in_lines(&lines(&["hola"]), "").is_empty());
        assert!(find_in_lines(&lines(&["hola"]), "   ").is_empty());
        assert!(find_in_lines(&[], "hola").is_empty());
    }

    #[test]
    fn lo_que_no_esta_no_coincide() {
        assert!(find_in_lines(&lines(&["hola mundo"]), "chao").is_empty());
        // Más largo que la línea: nada.
        assert!(find_in_lines(&lines(&["ho"]), "hola").is_empty());
    }

    #[test]
    fn el_paso_da_la_vuelta_en_los_extremos() {
        assert_eq!(step_from(0, 3, true), 1);
        assert_eq!(step_from(2, 3, true), 0);
        assert_eq!(step_from(0, 3, false), 2);
        assert_eq!(step_from(1, 3, false), 0);
        assert_eq!(step_from(0, 0, true), 0);
    }
}
