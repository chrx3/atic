//! El modo emoji del lanzador (`:`), como en Atic
//! (`apps/desktop/src/lib/features/emoji/emoji.ts`): el catálogo CLDR de Atic
//! dentro del binario, la búsqueda en español e inglés con el mismo puntaje,
//! la grilla por secciones (recientes y una por categoría), los tonos de piel
//! y el movimiento con flechas entre secciones.

use std::sync::OnceLock;

use gpui::SharedString;

pub struct Emoji {
    pub ch: SharedString,
    pub name: SharedString,
    pub group: u8,
    /// Cinco variantes de tono (claro → oscuro), si el emoji las tiene.
    pub skins: Option<[SharedString; 5]>,
    /// Nombres (es, en) y palabras clave ya normalizados.
    names: [String; 2],
    words: Vec<String>,
}

impl Emoji {
    /// El emoji con el tono elegido (0 = amarillo, 1..5 claro → oscuro).
    pub fn with_skin(&self, tone: u8) -> SharedString {
        match (&self.skins, tone) {
            (Some(skins), 1..=5) => skins[tone as usize - 1].clone(),
            _ => self.ch.clone(),
        }
    }
}

/// Las categorías en el orden de la grilla, con su ícono y nombre. La 2
/// (componentes: tonos sueltos) no se muestra.
pub const GROUPS: [(u8, &str, &str); 9] = [
    (0, "😀", "Caras y emociones"),
    (1, "👋", "Personas y cuerpo"),
    (3, "🐶", "Animales y naturaleza"),
    (4, "🍎", "Comida y bebida"),
    (5, "✈️", "Viajes y lugares"),
    (6, "⚽", "Actividades"),
    (7, "💡", "Objetos"),
    (8, "❤️", "Símbolos"),
    (9, "🏁", "Banderas"),
];

/// Los tonos para elegir, sobre la mano levantada.
pub const TONES: [&str; 6] = ["✋", "✋🏻", "✋🏼", "✋🏽", "✋🏾", "✋🏿"];

/// La última versión de Emoji que dibuja Segoe UI Emoji (Windows 11): los
/// más nuevos salían como cuadritos.
const NEWEST: f64 = 15.0;
pub const RECENTS_MAX: usize = 16;

pub fn fold(s: &str) -> String {
    crate::clipboard::fold(s)
}

/// Banderas de país = dos indicadores regionales: Windows las dibuja como
/// dos letras («CL»), así que no van.
fn country_flag(ch: &str) -> bool {
    ch.chars().next().is_some_and(|c| ('\u{1F1E6}'..='\u{1F1FF}').contains(&c))
}

/// `emojiData.json` de Atic (CLDR vía emojibase), dentro del binario. Se arma
/// la primera vez que se entra al modo.
pub fn all() -> &'static Vec<Emoji> {
    static EMOJIS: OnceLock<Vec<Emoji>> = OnceLock::new();
    EMOJIS.get_or_init(|| {
        let raw = include_str!("../../../apps/desktop/src/lib/features/emoji/emojiData.json");
        parse(raw)
    })
}

fn parse(raw: &str) -> Vec<Emoji> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    let Some(rows) = value["rows"].as_array() else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let row = row.as_array()?;
            let ch = row.first()?.as_str()?;
            let group = row.get(1)?.as_u64()? as u8;
            let es = row.get(2)?.as_str().unwrap_or_default();
            let en = row.get(3)?.as_str().unwrap_or_default();
            let words = row.get(4)?.as_str().unwrap_or_default();
            let version = row.get(5).and_then(|v| v.as_f64()).unwrap_or(0.0);
            if version > NEWEST || country_flag(ch) || group == 2 {
                return None;
            }
            let skins = row.get(6).and_then(|s| s.as_array()).and_then(|s| {
                let s: Vec<SharedString> =
                    s.iter().filter_map(|v| v.as_str()).map(|v| SharedString::from(v.to_string())).collect();
                <[SharedString; 5]>::try_from(s).ok()
            });
            Some(Emoji {
                ch: ch.to_string().into(),
                name: es.to_string().into(),
                group,
                skins,
                names: [fold(es), fold(en)],
                words: fold(words).split_whitespace().map(str::to_string).collect(),
            })
        })
        .collect()
}

/// El puntaje de una palabra de la búsqueda, como en Atic: nombre exacto,
/// nombre que empieza igual, una palabra del nombre que empieza igual, y
/// recién después las palabras clave.
fn token_score(token: &str, emoji: &Emoji) -> u32 {
    let mut best = 0;
    for name in &emoji.names {
        if name == token {
            return 100;
        }
        if name.starts_with(token) {
            best = best.max(80);
        } else if name.split([' ', ':', ',', '-']).any(|w| w.starts_with(token)) {
            best = best.max(60);
        } else if token.chars().count() >= 3 && name.contains(token) {
            best = best.max(20);
        }
    }
    if best < 60 {
        if emoji.words.iter().any(|w| w == token) {
            best = best.max(50);
        } else if emoji.words.iter().any(|w| w.starts_with(token)) {
            best = best.max(40);
        }
    }
    best
}

/// Índices de los que calzan: cada palabra tiene que calzar; a igual
/// puntaje, el orden del catálogo (que ya agrupa lo parecido).
pub fn search(list: &[Emoji], query: &str, limit: usize) -> Vec<usize> {
    let q = fold(query);
    let tokens: Vec<&str> = q.split_whitespace().collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, usize)> = list
        .iter()
        .enumerate()
        .filter_map(|(i, emoji)| {
            let mut total = 0;
            for t in &tokens {
                let s = token_score(t, emoji);
                if s == 0 {
                    return None;
                }
                total += s;
            }
            Some((total, i))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

/// Una sección de la grilla: su título (ninguno en los resultados) y sus
/// emojis, como índices del catálogo.
pub struct Section {
    pub title: Option<&'static str>,
    pub group: Option<u8>,
    pub items: Vec<usize>,
}

/// Sin búsqueda: los recientes (si hay) y una sección por categoría. Con
/// búsqueda: una sola, sin título.
pub fn sections(query: &str, recents: &[String]) -> Vec<Section> {
    let list = all();
    if !query.trim().is_empty() {
        return vec![Section { title: None, group: None, items: search(list, query, 160) }];
    }
    let mut out = Vec::new();
    let recent: Vec<usize> =
        recents.iter().filter_map(|ch| list.iter().position(|e| e.ch.as_ref() == ch.as_str())).collect();
    if !recent.is_empty() {
        out.push(Section { title: Some("Recientes"), group: None, items: recent });
    }
    for (group, _, title) in GROUPS {
        let items: Vec<usize> = (0..list.len()).filter(|&i| list[i].group == group).collect();
        if !items.is_empty() {
            out.push(Section { title: Some(title), group: Some(group), items });
        }
    }
    out
}

/// Una fila de la grilla: un título o unas celdas (posiciones planas en el
/// recorrido de todas las secciones).
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    Title(&'static str),
    Cells { start: usize, len: usize },
}

/// Las filas para la lista: el título de cada sección y sus emojis en
/// filas de `cols`.
pub fn rows(sections: &[Section], cols: usize) -> Vec<Row> {
    let mut out = Vec::new();
    let mut start = 0;
    for section in sections {
        if section.items.is_empty() {
            continue;
        }
        if let Some(title) = section.title {
            out.push(Row::Title(title));
        }
        for chunk in 0..section.items.len().div_ceil(cols) {
            let len = (section.items.len() - chunk * cols).min(cols);
            out.push(Row::Cells { start: start + chunk * cols, len });
        }
        start += section.items.len();
    }
    out
}

pub enum Key {
    Left,
    Right,
    Up,
    Down,
}

/// Mueve la selección en una grilla partida en secciones, cada una con su
/// última fila incompleta (`moveInGrid` de Atic). Arriba/abajo conservan la
/// columna y saltan de sección; si la fila destino es más corta, cae en su
/// último emoji.
pub fn move_in_grid(sizes: &[usize], index: usize, key: Key, cols: usize) -> usize {
    let total: usize = sizes.iter().sum();
    if total == 0 {
        return 0;
    }
    match key {
        Key::Left => return index.saturating_sub(1),
        Key::Right => return (index + 1).min(total - 1),
        _ => {}
    }
    let mut starts = Vec::with_capacity(sizes.len());
    let mut acc = 0;
    for &size in sizes {
        starts.push(acc);
        acc += size;
    }
    let Some(mut section) = (0..sizes.len()).find(|&i| index >= starts[i] && index < starts[i] + sizes[i]) else {
        return index.min(total - 1);
    };
    let local = index - starts[section];
    let (col, row) = (local % cols, local / cols);
    let rows = |size: usize| size.div_ceil(cols);
    let at = |s: usize, r: usize| starts[s] + (r * cols + col).min(sizes[s] - 1);
    match key {
        Key::Down => {
            if row + 1 < rows(sizes[section]) {
                return at(section, row + 1);
            }
            loop {
                section += 1;
                if section >= sizes.len() {
                    return index;
                }
                if sizes[section] > 0 {
                    return at(section, 0);
                }
            }
        }
        _ => {
            if row > 0 {
                return at(section, row - 1);
            }
            loop {
                if section == 0 {
                    return index;
                }
                section -= 1;
                if sizes[section] > 0 {
                    return at(section, rows(sizes[section]) - 1);
                }
            }
        }
    }
}

/// El reciente nuevo va primero; sin repetir y hasta `RECENTS_MAX`.
pub fn push_recent(recents: &mut Vec<String>, ch: &str) {
    recents.retain(|c| c != ch);
    recents.insert(0, ch.to_string());
    recents.truncate(RECENTS_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_catalogo_omite_banderas_de_pais_y_componentes() {
        let list = all();
        assert!(list.len() > 1500);
        assert!(!list.iter().any(|e| country_flag(&e.ch)));
        assert!(list.iter().any(|e| e.ch.as_ref() == "🏁"));
        assert!(!list.iter().any(|e| e.group == 2));
    }

    #[test]
    fn busca_como_atic() {
        let list = all();
        let first = |q: &str| list[search(list, q, 10)[0]].ch.to_string();
        // El nombre exacto gana a lo que solo lo contiene.
        assert_eq!(first("fuego"), "🔥");
        // El catálogo trae 👍 con el selector de variante (U+FE0F).
        assert_eq!(first("thumbs up").trim_end_matches('\u{fe0f}'), "👍");
        // Sin tildes también.
        assert!(!search(list, "corazon", 5).is_empty());
        assert!(search(list, "zzzz", 5).is_empty());
    }

    #[test]
    fn los_tonos_cambian_el_emoji() {
        let list = all();
        let wave = list.iter().find(|e| e.ch.as_ref() == "👋").unwrap();
        assert_eq!(wave.with_skin(0).as_ref(), "👋");
        assert_eq!(wave.with_skin(3).as_ref(), "👋🏽");
        let fire = list.iter().find(|e| e.ch.as_ref() == "🔥").unwrap();
        assert_eq!(fire.with_skin(3).as_ref(), "🔥");
    }

    #[test]
    fn filas_con_titulos() {
        let sections = vec![
            Section { title: Some("A"), group: None, items: vec![1, 2, 3] },
            Section { title: Some("B"), group: None, items: vec![4] },
        ];
        assert_eq!(
            rows(&sections, 2),
            vec![
                Row::Title("A"),
                Row::Cells { start: 0, len: 2 },
                Row::Cells { start: 2, len: 1 },
                Row::Title("B"),
                Row::Cells { start: 3, len: 1 },
            ]
        );
    }

    #[test]
    fn flechas_entre_secciones() {
        // Secciones de 10 y 3, 8 columnas.
        let sizes = [10, 3];
        assert_eq!(move_in_grid(&sizes, 1, Key::Down, 8), 9);
        // Desde la última fila de la primera, baja a la segunda (misma columna o la última).
        assert_eq!(move_in_grid(&sizes, 9, Key::Down, 8), 11);
        assert_eq!(move_in_grid(&sizes, 12, Key::Up, 8), 9);
        assert_eq!(move_in_grid(&sizes, 0, Key::Up, 8), 0);
        assert_eq!(move_in_grid(&sizes, 12, Key::Right, 8), 12);
    }

    #[test]
    fn recientes_sin_repetir() {
        let mut r = vec!["a".to_string(), "b".to_string()];
        push_recent(&mut r, "b");
        assert_eq!(r, vec!["b", "a"]);
    }
}
