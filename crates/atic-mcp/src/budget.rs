//! Presupuesto de espera por host: el `tools/call` lo corta el host, no el hub.
//!
//! Codex y Cursor cortan a los ~60 s sin avisar; si el sidecar esperara los
//! 300 s del hub, el padre perdería el `session` del hijo que Atic sí levantó.
//! Por eso cada host tiene su tope y `atic_wait` reengancha el turno.

/// Tope del hub: ninguna espera lo pasa.
pub const MAX_ESPERA_S: u64 = 300;

/// Cuánto espera el sidecar antes de devolver el traspaso (`session` + parcial).
pub fn wait_budget(host: Option<&str>, wait_flag: Option<u64>, client_name: Option<&str>) -> u64 {
    if let Some(s) = wait_flag {
        return s.clamp(1, MAX_ESPERA_S);
    }
    match host {
        Some("claude-code") => MAX_ESPERA_S,
        Some("codex") | Some("cursor") | Some("opencode") | Some("grok") | Some("antigravity") => {
            50
        }
        _ => {
            // Sin `--host`, el nombre del cliente del `initialize` legacy.
            if client_name.is_some_and(|n| n.to_lowercase().contains("claude")) {
                MAX_ESPERA_S
            } else {
                50
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_por_flag_tiene_300() {
        assert_eq!(wait_budget(Some("claude-code"), None, None), 300);
    }

    #[test]
    fn cursor_tiene_50() {
        assert_eq!(wait_budget(Some("cursor"), None, None), 50);
    }

    #[test]
    fn sin_pistas_es_50() {
        assert_eq!(wait_budget(None, None, None), 50);
    }

    #[test]
    fn client_info_con_claude_da_300() {
        assert_eq!(wait_budget(None, None, Some("claude-code")), 300);
        assert_eq!(wait_budget(None, None, Some("cursor-agent")), 50);
    }

    #[test]
    fn wait_explicito_manda_y_se_recorta_a_300() {
        assert_eq!(wait_budget(Some("cursor"), Some(300), None), 300);
        assert_eq!(wait_budget(Some("cursor"), Some(9999), None), 300);
        assert_eq!(wait_budget(Some("claude-code"), Some(20), None), 20);
    }
}
