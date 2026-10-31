//! La tecla principal de los atajos dentro de las ventanas: Ctrl en Windows,
//! Comando en macOS (Cmd+C, Cmd+K…). Los de la terminal no pasan por aquí:
//! ahí Ctrl es del shell.

/// `ctrl-k` → `cmd-k` en macOS; igual en Windows. Sirve también para
/// secuencias (`ctrl-k ctrl-s`).
pub fn primary(key: &str) -> String {
    if !cfg!(target_os = "macos") {
        return key.to_string();
    }
    key.split(' ')
        .map(|step| match step.strip_prefix("ctrl-") {
            Some(rest) if !rest.is_empty() => format!("cmd-{rest}"),
            _ => step.to_string(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn la_principal_es_comando_en_mac() {
        let expected = if cfg!(target_os = "macos") { "cmd-shift-z" } else { "ctrl-shift-z" };
        assert_eq!(super::primary("ctrl-shift-z"), expected);
        assert_eq!(super::primary("escape"), "escape");
        assert_eq!(super::primary("alt-x"), "alt-x");
    }
}

/// Un texto de ayuda con atajos («Ctrl+Z deshacer») como se lee en macOS
/// («⌘Z deshacer»). Cada texto distinto se arma una vez y queda para siempre.
pub fn hint(text: &'static str) -> &'static str {
    if !cfg!(target_os = "macos") || !(text.contains("Ctrl+") || text.contains("Alt+")) {
        return text;
    }
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: Mutex<Option<HashMap<&'static str, &'static str>>> = Mutex::new(None);
    let Ok(mut cache) = CACHE.lock() else {
        return text;
    };
    cache.get_or_insert_with(HashMap::new).entry(text).or_insert_with(|| Box::leak(mac_text(text).into_boxed_str()))
}

/// Como `hint`, para un texto armado en el momento.
pub fn hint_owned(text: String) -> String {
    if cfg!(target_os = "macos") {
        mac_text(&text)
    } else {
        text
    }
}

fn mac_text(text: &str) -> String {
    text.replace("Ctrl+", "⌘").replace("Alt+", "⌥").replace("Mayús+", "⇧").replace("Shift+", "⇧")
}

/// La tecla principal apretada: Ctrl en Windows, Comando en macOS (donde
/// Ctrl queda libre para el shell y los agentes).
pub fn primary_held(m: &gpui::Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        m.platform
    } else {
        m.control
    }
}
