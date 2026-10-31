//! Del montón de líneas que entrega Vision a un texto que se lea bien: en
//! orden de lectura (de arriba abajo y, en columnas, de izquierda a derecha),
//! con las líneas de un mismo párrafo unidas y las celdas de una fila de
//! tabla en la misma línea.
//!
//! Lógica pura (sin Vision) para poder probarla.

/// Un trozo leído (una observación de Vision), en píxeles con el origen
/// arriba a la izquierda.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Piece {
    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    fn mid_y(&self) -> f64 {
        self.y + self.h / 2.0
    }
}

/// Un hueco vertical entre columnas tiene que medir al menos esto (en alturas
/// de línea) para separarlas.
const GUTTER: f64 = 1.5;
/// Un hueco horizontal así de alto (en alturas de línea) separa bloques: más
/// que el interlineado normal.
const BAND_GAP: f64 = 0.9;
/// Entre dos bloques con un hueco así de alto va una línea en blanco.
const BLANK_GAP: f64 = 1.5;
/// Dentro de una fila, un hueco más ancho que esto (en alturas) va con
/// tabulación: son celdas, no palabras.
const CELL_GAP: f64 = 2.0;

/// El texto de la imagen, listo para copiar o buscar.
pub fn text(pieces: Vec<Piece>) -> String {
    let pieces: Vec<Piece> = pieces
        .into_iter()
        .map(|mut piece| {
            piece.text = piece.text.trim().to_string();
            piece
        })
        .filter(|piece| !piece.text.is_empty() && piece.w >= 0.0 && piece.h > 0.0)
        .collect();
    if pieces.is_empty() {
        return String::new();
    }
    let unit = median(pieces.iter().map(|piece| piece.h).collect());
    let mut regions = Vec::new();
    cut(pieces, unit, &mut regions);

    let mut out = String::new();
    let mut previous: Option<(f64, f64, f64)> = None;
    for region in regions {
        let (left, top, bottom) = bounds(&region);
        let rows = rows_of(region, unit);
        let body = paragraphs(&rows);
        if body.is_empty() {
            continue;
        }
        if let Some((prev_left, _, prev_bottom)) = previous {
            let below = top >= prev_bottom - unit * 0.25;
            let side_by_side = !below || left > prev_left + unit * GUTTER && top < prev_bottom;
            if side_by_side || top - prev_bottom >= unit * BLANK_GAP {
                out.push_str("\n\n");
            } else {
                out.push('\n');
            }
        }
        out.push_str(&body);
        previous = Some((left, top, bottom));
    }
    out
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values.get(values.len() / 2).copied().unwrap_or(1.0).max(1.0)
}

/// (izquierda, arriba, abajo) de un grupo.
fn bounds(pieces: &[Piece]) -> (f64, f64, f64) {
    let left = pieces.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let top = pieces.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let bottom = pieces.iter().map(Piece::bottom).fold(f64::NEG_INFINITY, f64::max);
    (left, top, bottom)
}

/// Corta en columnas y bloques (un XY-cut prudente) y deja los grupos en
/// orden de lectura en `out`.
fn cut(pieces: Vec<Piece>, unit: f64, out: &mut Vec<Vec<Piece>>) {
    if pieces.len() < 2 {
        out.extend((!pieces.is_empty()).then_some(pieces));
        return;
    }
    if let Some(at) = column_cut(&pieces, unit) {
        let (left, right): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(|p| p.right() <= at);
        cut(left, unit, out);
        cut(right, unit, out);
        return;
    }
    let bands = bands(pieces, unit);
    if bands.len() == 1 {
        out.extend(bands);
        return;
    }
    for band in bands {
        cut(band, unit, out);
    }
}

/// Dónde cortar en dos columnas: el hueco vertical más ancho que cruza todo
/// el grupo. No corta si a los dos lados hay celdas cortas alineadas por
/// fila (una tabla o un formulario se leen fila por fila).
fn column_cut(pieces: &[Piece], unit: f64) -> Option<f64> {
    let mut spans: Vec<(f64, f64)> = pieces.iter().map(|p| (p.x, p.right())).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut best: Option<(f64, f64)> = None;
    let mut reach = spans[0].1;
    for &(start, end) in &spans[1..] {
        let gap = start - reach;
        if gap >= unit * GUTTER && best.is_none_or(|(width, _)| gap > width) {
            best = Some((gap, reach + gap / 2.0));
        }
        reach = reach.max(end);
    }
    let (_, at) = best?;
    let (left, right): (Vec<&Piece>, Vec<&Piece>) = pieces.iter().partition(|p| p.right() <= at);
    let (fewer, other) = if left.len() <= right.len() { (&left, &right) } else { (&right, &left) };
    let aligned = fewer
        .iter()
        .filter(|p| other.iter().any(|q| (p.mid_y() - q.mid_y()).abs() < unit * 0.35))
        .count();
    let short = median(fewer.iter().map(|p| p.w).collect()) < unit * 8.0;
    let table = aligned * 10 >= fewer.len() * 6 && short;
    (!table).then_some(at)
}

/// Parte en bloques donde hay un hueco horizontal claro que cruza todo.
fn bands(mut pieces: Vec<Piece>, unit: f64) -> Vec<Vec<Piece>> {
    pieces.sort_by(|a, b| a.y.total_cmp(&b.y));
    let mut out: Vec<Vec<Piece>> = Vec::new();
    let mut reach = f64::NEG_INFINITY;
    for piece in pieces {
        let starts_band = piece.y - reach >= unit * BAND_GAP;
        reach = if starts_band { piece.bottom() } else { reach.max(piece.bottom()) };
        match out.last_mut() {
            Some(band) if !starts_band => band.push(piece),
            _ => out.push(vec![piece]),
        }
    }
    out
}

/// Una fila: los trozos a la misma altura, de izquierda a derecha.
#[derive(Debug)]
struct Row {
    text: String,
    x: f64,
    right: f64,
    y: f64,
    bottom: f64,
    h: f64,
}

fn rows_of(mut pieces: Vec<Piece>, unit: f64) -> Vec<Row> {
    pieces.sort_by(|a, b| a.mid_y().total_cmp(&b.mid_y()));
    let mut groups: Vec<Vec<Piece>> = Vec::new();
    for piece in pieces {
        let same_row = groups.last().is_some_and(|row| {
            let top = row.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let bottom = row.iter().map(Piece::bottom).fold(f64::NEG_INFINITY, f64::max);
            let mid = (top + bottom) / 2.0;
            (top..=bottom).contains(&piece.mid_y()) && (piece.y..=piece.bottom()).contains(&mid)
        });
        match groups.last_mut() {
            Some(row) if same_row => row.push(piece),
            _ => groups.push(vec![piece]),
        }
    }
    groups
        .into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut text = String::new();
            let mut reach: Option<f64> = None;
            for piece in &row {
                if let Some(reach) = reach {
                    text.push(if piece.x - reach >= unit * CELL_GAP { '\t' } else { ' ' });
                }
                text.push_str(&piece.text);
                reach = Some(reach.map_or(piece.right(), |r: f64| r.max(piece.right())));
            }
            Row {
                text,
                x: row.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
                right: row.iter().map(Piece::right).fold(f64::NEG_INFINITY, f64::max),
                y: row.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
                bottom: row.iter().map(Piece::bottom).fold(f64::NEG_INFINITY, f64::max),
                h: median(row.iter().map(|p| p.h).collect()),
            }
        })
        .collect()
}

/// Las filas de un bloque, con las de un mismo párrafo unidas.
fn paragraphs(rows: &[Row]) -> String {
    let Some(first) = rows.first() else {
        return String::new();
    };
    let left = rows.iter().map(|r| r.x).fold(f64::INFINITY, f64::min);
    let right = rows.iter().map(|r| r.right).fold(f64::NEG_INFINITY, f64::max);
    let width = right - left;
    let mut out = first.text.clone();
    for pair in rows.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if continues(a, b, right, width) {
            join_wrapped(&mut out, &b.text);
        } else {
            out.push('\n');
            out.push_str(&b.text);
        }
    }
    out
}

/// ¿`b` sigue el párrafo de `a`? Solo para prosa: `a` es larga y llega casi
/// al borde derecho (la línea se cortó por falta de espacio), están pegadas,
/// tienen el mismo tamaño de letra y `b` no tiene sangría ni viñeta.
fn continues(a: &Row, b: &Row, right: f64, width: f64) -> bool {
    let h = a.h.max(b.h);
    let gap = b.y - a.bottom;
    let similar = a.h.max(b.h) / a.h.min(b.h).max(1.0) <= 1.35;
    let long = a.right - a.x >= h * 12.0;
    let full = a.right >= right - (width * 0.2).max(h * 3.0);
    let indent = b.x - a.x;
    let aligned = indent <= h * 0.8 && -indent <= h * 4.0;
    let tabular = a.text.contains('\t') || b.text.contains('\t');
    gap < h * 0.7
        && similar
        && long
        && full
        && aligned
        && !tabular
        && !a.text.ends_with(['{', '}', ';', ':'])
        && !starts_list_item(&b.text)
}

/// Viñetas y enumeraciones: «• uno», «- dos», «3. tres», «b) cuatro».
fn starts_list_item(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if matches!(first, '•' | '◦' | '▪' | '·' | '‣' | '-' | '–' | '—' | '*' | '>') {
        return chars.next().is_none_or(char::is_whitespace);
    }
    let marker: String = text.chars().take_while(|c| c.is_alphanumeric()).collect();
    let rest = &text[marker.len()..];
    let short_marker = !marker.is_empty()
        && (marker.chars().all(|c| c.is_ascii_digit()) && marker.len() <= 3
            || marker.chars().count() == 1 && marker.chars().all(char::is_alphabetic));
    short_marker && (rest.starts_with(". ") || rest.starts_with(") "))
}

/// Une una línea cortada a la anterior: con espacio o, si la palabra quedó
/// partida con guion («continu-» + «ar»), sin el guion.
fn join_wrapped(out: &mut String, next: &str) {
    let mut tail = out.chars().rev();
    let hyphenated = tail.next() == Some('-')
        && tail.next().is_some_and(char::is_alphabetic)
        && next.chars().next().is_some_and(char::is_lowercase);
    if hyphenated {
        out.pop();
    } else {
        out.push(' ');
    }
    out.push_str(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una línea de texto: alto 20 px, unos 10 px por carácter.
    fn line(text: &str, x: f64, y: f64) -> Piece {
        Piece { text: text.into(), x, y, w: text.chars().count() as f64 * 10.0, h: 20.0 }
    }

    const LONG_A: &str = "Este es un párrafo bastante largo que Vision entrega en";
    const LONG_B: &str = "varias líneas porque no cabe en el ancho de la ventana y";
    const END: &str = "termina aquí.";

    #[test]
    fn une_las_lineas_de_un_parrafo() {
        let pieces = vec![line(END, 0.0, 56.0), line(LONG_A, 0.0, 0.0), line(LONG_B, 0.0, 28.0)];
        assert_eq!(text(pieces), format!("{LONG_A} {LONG_B} {END}"));
    }

    #[test]
    fn una_lista_corta_no_se_une() {
        let pieces = vec![line("Manzanas", 0.0, 0.0), line("Peras", 0.0, 28.0), line("Uvas", 0.0, 56.0)];
        assert_eq!(text(pieces), "Manzanas\nPeras\nUvas");
    }

    #[test]
    fn la_linea_corta_cierra_el_parrafo() {
        let pieces = vec![line(LONG_A, 0.0, 0.0), line(END, 0.0, 28.0), line(LONG_B, 0.0, 56.0)];
        assert_eq!(text(pieces), format!("{LONG_A} {END}\n{LONG_B}"));
    }

    #[test]
    fn las_vinetas_no_se_unen() {
        let pieces = vec![line(LONG_A, 0.0, 0.0), line("• otra cosa que también es larga pero va aparte", 0.0, 28.0)];
        assert_eq!(text(pieces), format!("{LONG_A}\n• otra cosa que también es larga pero va aparte"));
    }

    #[test]
    fn junta_la_palabra_partida_con_guion() {
        let pieces = vec![
            line("Una línea de prosa suficientemente larga para que se continu-", 0.0, 0.0),
            line("ara en la siguiente.", 0.0, 28.0),
        ];
        assert_eq!(text(pieces), "Una línea de prosa suficientemente larga para que se continuara en la siguiente.");
    }

    #[test]
    fn un_hueco_grande_separa_parrafos() {
        let pieces = vec![line(LONG_A, 0.0, 0.0), line(LONG_B, 0.0, 60.0)];
        assert_eq!(text(pieces), format!("{LONG_A}\n\n{LONG_B}"));
    }

    #[test]
    fn dos_columnas_se_leen_una_despues_de_la_otra() {
        // Renglones alineados en ambas columnas: igual se lee por columna.
        let pieces = vec![
            line("Columna izquierda, primera línea bastante larga", 0.0, 0.0),
            line("Columna derecha, primera línea bastante larga", 600.0, 0.0),
            line("Columna izquierda, segunda línea", 0.0, 28.0),
            line("Columna derecha, segunda línea", 600.0, 28.0),
        ];
        assert_eq!(
            text(pieces),
            "Columna izquierda, primera línea bastante larga Columna izquierda, segunda línea\n\n\
             Columna derecha, primera línea bastante larga Columna derecha, segunda línea"
        );
    }

    #[test]
    fn titulo_arriba_y_despues_las_columnas() {
        let pieces = vec![
            line("Un título que cruza toda la página de lado a lado sin parar nunca", 0.0, 0.0),
            line("Izquierda uno", 0.0, 60.0),
            line("Derecha uno", 600.0, 60.0),
            line("Izquierda dos", 0.0, 88.0),
            line("Derecha dos", 600.0, 100.0),
        ];
        let read = text(pieces);
        let order: Vec<usize> = ["título", "Izquierda uno", "Izquierda dos", "Derecha uno", "Derecha dos"]
            .iter()
            .map(|needle| read.find(needle).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]), "{read}");
    }

    #[test]
    fn una_tabla_se_lee_fila_por_fila() {
        let pieces = vec![
            line("Total", 0.0, 0.0),
            line("$1.234", 400.0, 2.0),
            line("IVA", 0.0, 28.0),
            line("$234", 400.0, 28.0),
        ];
        assert_eq!(text(pieces), "Total\t$1.234\nIVA\t$234");
    }

    #[test]
    fn palabras_cercanas_en_la_misma_fila_van_con_espacio() {
        let pieces = vec![line("Hola", 0.0, 0.0), line("mundo", 50.0, 1.0)];
        assert_eq!(text(pieces), "Hola mundo");
    }

    #[test]
    fn el_orden_de_entrada_no_importa() {
        let mut pieces = vec![line("uno", 0.0, 0.0), line("dos", 0.0, 28.0), line("tres", 0.0, 56.0)];
        pieces.reverse();
        assert_eq!(text(pieces), "uno\ndos\ntres");
    }

    #[test]
    fn enumeraciones() {
        assert!(starts_list_item("1. Primero"));
        assert!(starts_list_item("b) segundo"));
        assert!(starts_list_item("- guion"));
        assert!(!starts_list_item("-5 grados"));
        assert!(!starts_list_item("El 2. lugar"));
        assert!(!starts_list_item("Hola. Chao"));
    }
}
