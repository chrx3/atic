//! Nombres de archivo de capturas: `Atic_<fecha>_<hora>.png`.

use std::path::Path;

use chrono::NaiveDateTime;

/// Formatea el nombre de archivo a partir de una fecha/hora local.
/// `disambiguator` añade un sufijo (`_2`, `_3`, …) si hay colisión en el mismo segundo.
pub fn format_capture_filename(when: &NaiveDateTime, disambiguator: Option<u32>) -> String {
    let stamp = when.format("%Y-%m-%d_%H-%M-%S");
    match disambiguator {
        None | Some(0) | Some(1) => format!("Atic_{stamp}.png"),
        Some(n) => format!("Atic_{stamp}_{n}.png"),
    }
}

/// Nombre único dentro de `dir` (evita sobrescribir si se capturan varias
/// veces en el mismo segundo).
pub fn unique_capture_filename(dir: &Path) -> String {
    let when = chrono::Local::now().naive_local();
    let mut n = 1u32;
    loop {
        let name = format_capture_filename(&when, if n <= 1 { None } else { Some(n) });
        if !dir.join(&name).exists() {
            return name;
        }
        n = n.saturating_add(1);
    }
}

/// Etiqueta corta para la UI del shelf (hora local de la captura).
pub fn shelf_label(when: &NaiveDateTime) -> String {
    when.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn formats_expected_pattern() {
        let when = NaiveDate::from_ymd_opt(2026, 7, 17)
            .unwrap()
            .and_hms_opt(14, 32, 8)
            .unwrap();
        assert_eq!(
            format_capture_filename(&when, None),
            "Atic_2026-07-17_14-32-08.png"
        );
        assert_eq!(
            format_capture_filename(&when, Some(2)),
            "Atic_2026-07-17_14-32-08_2.png"
        );
    }

    #[test]
    fn shelf_label_is_hh_mm() {
        let when = NaiveDate::from_ymd_opt(2026, 7, 17)
            .unwrap()
            .and_hms_opt(14, 32, 8)
            .unwrap();
        assert_eq!(shelf_label(&when), "14:32");
    }
}
