//! Idioma del sistema operativo para la UI y para Whisper.

/// Tag BCP-47 del SO (`en-US`, `es-CL`), si el entorno lo informa.
pub fn os_locale_tag() -> Option<String> {
    sys_locale::get_locale()
}

fn lang_code(tag: &str) -> String {
    tag.split(['-', '_'])
        .next()
        .unwrap_or(tag)
        .to_ascii_lowercase()
}

/// UI: `en` o `es`. Cualquier otro idioma del SO cae a español.
pub fn ui_language_from_tag(tag: Option<&str>) -> String {
    match tag.map(lang_code).as_deref() {
        Some("en") => "en".into(),
        _ => "es".into(),
    }
}

/// Whisper: código ISO o `auto` si no hay equivalente claro.
pub fn whisper_language_from_tag(tag: Option<&str>) -> String {
    match tag.map(lang_code).as_deref() {
        Some("en") => "en".into(),
        Some("pt") => "pt".into(),
        Some("fr") => "fr".into(),
        Some("es") => "es".into(),
        Some(_) => "auto".into(),
        None => "es".into(),
    }
}

/// `system` sigue al SO; `es`/`en` quedan fijos.
pub fn resolve_ui_language(stored: &str) -> String {
    match stored {
        "en" | "es" => stored.to_string(),
        _ => ui_language_from_tag(os_locale_tag().as_deref()),
    }
}

/// `None` = Whisper autodetecta el audio.
pub fn resolve_whisper_language(stored: &str) -> Option<String> {
    let code = match stored {
        "auto" => return None,
        "es" | "en" | "pt" | "fr" => stored.to_string(),
        _ => whisper_language_from_tag(os_locale_tag().as_deref()),
    };
    if code == "auto" {
        None
    } else {
        Some(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_maps_english_and_spanish_tags() {
        assert_eq!(ui_language_from_tag(Some("en-US")), "en");
        assert_eq!(ui_language_from_tag(Some("en")), "en");
        assert_eq!(ui_language_from_tag(Some("es-CL")), "es");
        assert_eq!(ui_language_from_tag(Some("pt-BR")), "es");
        assert_eq!(ui_language_from_tag(None), "es");
    }

    #[test]
    fn whisper_maps_common_tags() {
        assert_eq!(whisper_language_from_tag(Some("en-GB")), "en");
        assert_eq!(whisper_language_from_tag(Some("pt-BR")), "pt");
        assert_eq!(whisper_language_from_tag(Some("fr-FR")), "fr");
        assert_eq!(whisper_language_from_tag(Some("es-MX")), "es");
        assert_eq!(whisper_language_from_tag(Some("ja-JP")), "auto");
        assert_eq!(whisper_language_from_tag(None), "es");
    }

    #[test]
    fn stored_system_uses_tag_helpers() {
        assert_eq!(resolve_ui_language("en"), "en");
        assert_eq!(resolve_ui_language("es"), "es");
        assert_eq!(
            resolve_ui_language("system"),
            ui_language_from_tag(os_locale_tag().as_deref())
        );
        assert_eq!(resolve_whisper_language("auto"), None);
        assert_eq!(resolve_whisper_language("pt").as_deref(), Some("pt"));
        assert_eq!(
            resolve_whisper_language("system"),
            resolve_whisper_language(&whisper_language_from_tag(os_locale_tag().as_deref()))
        );
    }
}
