//! Reglas del grafo de delegación: pura, sin I/O ni procesos.
//!
//! Por qué un módulo aparte: la profundidad, el ciclo y el reuso son lo que
//! impide que dos encargos mezclen contexto o que la cadena crezca sin tope.
//! Si viven junto al servidor HTTP se prueban con sockets; acá se prueban con
//! listas.

use serde_json::json;

use super::api::{HubError, Kind};

/// Se rechaza el spawn cuando `depth >= MAX_DEPTH`: el hijo sería el tercer salto.
pub const MAX_DEPTH: u8 = 2;
/// Tope del hub por espera, aunque el sidecar pida más.
pub const HUB_WAIT_MAX_S: u64 = 300;
/// Tope del texto que vuelve al padre; se conserva la cola.
pub const TEXT_CAP_BYTES: usize = 32 * 1024;

pub struct LiveSession {
    pub id: String,
    pub backend: String,
    pub host: String,
    pub cwd_key: String,
    pub root: Option<String>,
    pub running: bool,
}

pub struct SpawnCheck<'a> {
    pub backend: &'a str,
    pub host: &'a str,
    pub cwd_key: &'a str,
    pub depth: u8,
    pub root: Option<&'a str>,
    pub parent: Option<&'a str>,
}

/// Clave para comparar carpetas: canoniza si existe, minúsculas en Windows,
/// separadores `/`. Lo remoto no se canoniza (ver `atic_spawn`).
pub fn cwd_key(cwd: &str) -> String {
    let base = match std::fs::canonicalize(cwd) {
        Ok(p) => p.to_string_lossy().into_owned(),
        Err(_) => cwd.to_owned(),
    };
    let sin_verbatim = base
        .strip_prefix(r"\\?\")
        .or_else(|| base.strip_prefix("//?/"))
        .unwrap_or(&base);
    let con_barras = sin_verbatim.replace('\\', "/");
    #[cfg(windows)]
    {
        con_barras.to_lowercase()
    }
    #[cfg(not(windows))]
    {
        con_barras
    }
}

/// Revisa profundidad, reuso y ciclo, en ese orden: el mensaje más barato de
/// entender va primero.
pub fn check_spawn(
    req: SpawnCheck,
    live: &[LiveSession],
    chain_for_root: &[(String, String)],
) -> Result<(), HubError> {
    if req.depth >= MAX_DEPTH {
        return Err(HubError::con_datos(
            "depth_exceeded",
            "No se puede delegar más hondo: este agente ya es un hijo de un hijo. Resuelve tú la tarea o devuélvesela a quien te la pidió.".into(),
            json!({ "depth": req.depth, "max": MAX_DEPTH }),
        ));
    }
    if let Some(otra) = live
        .iter()
        .find(|s| s.backend == req.backend && s.host == req.host && s.cwd_key == req.cwd_key)
    {
        return Err(HubError::con_datos(
            "already_running",
            if req.parent == Some(otra.id.as_str()) {
                format!(
                    "Ya hay una sesión de {} viva en {}: {}. Es esta misma: sigue con atic_prompt en vez de abrir otra.",
                    req.backend, req.cwd_key, otra.id
                )
            } else {
                format!(
                    "Ya hay una sesión de {} viva en {}: {}. Sigue esa con atic_prompt en vez de abrir otra.",
                    req.backend, req.cwd_key, otra.id
                )
            },
            json!({ "session": otra.id }),
        ));
    }
    if chain_for_root
        .iter()
        .any(|(b, c)| b == req.backend && c == req.cwd_key)
    {
        return Err(HubError::con_datos(
            "cycle",
            "Ese agente ya está en la cadena de este encargo; devolvérselo sería un ciclo. Resuélvelo tú.".into(),
            json!({ "chain": chain_for_root.iter().map(|(b, c)| format!("{b}:{c}")).collect::<Vec<_>>() }),
        ));
    }
    Ok(())
}

/// Elige backend para `"auto"` solo por `kind`, jamás por el texto del recado.
/// `available` trae `(id, está_instalado)`; el orden de desempate es fijo.
pub fn route(kind: Option<Kind>, available: &[(&str, bool)]) -> Option<&'static str> {
    // Los últimos son los de menos rodaje: `auto` solo cae en ellos si no hay
    // nada más. Quien los quiera los pide por nombre.
    const DESEMPATE: &[&str] = &[
        "claude-code",
        "codex",
        "cursor",
        "opencode",
        "grok",
        "antigravity",
    ];
    let esta = |id: &str| available.iter().any(|(b, ok)| *b == id && *ok);
    // Devolver el elemento de `DESEMPATE` y no un literal: con un `match` de
    // identidad, sumar un backend a la lista y olvidarse del brazo lo enrutaba
    // en silencio al del `_`.
    let disponibles: Vec<&'static str> = DESEMPATE.iter().copied().filter(|id| esta(id)).collect();
    // Un solo disponible manda, sin importar el `kind`.
    if let [unico] = disponibles[..] {
        return Some(unico);
    }
    let preferido: Option<&'static str> = match kind {
        Some(Kind::Plan) => Some("claude-code"),
        Some(Kind::Patch) | Some(Kind::Review) => Some("codex"),
        Some(Kind::Apply) => Some("cursor"),
        None => None,
    };
    if let Some(p) = preferido {
        if esta(p) {
            return Some(p);
        }
    }
    disponibles.first().copied()
}

/// Recorta cualquier espera pedida al tope del hub.
pub fn clamp_wait(requested_s: u64) -> u64 {
    requested_s.min(HUB_WAIT_MAX_S)
}

/// Recorta por la cola con marca: lo último que dijo el agente es lo que más
/// le sirve al padre. Corta por borde de char para no romper UTF-8.
pub fn cap_text(text: &str) -> String {
    if text.len() <= TEXT_CAP_BYTES {
        return text.to_owned();
    }
    let marca = "[…recortado…]\n";
    let presupuesto = TEXT_CAP_BYTES.saturating_sub(marca.len());
    let mut corte = text.len().saturating_sub(presupuesto);
    while corte < text.len() && !text.is_char_boundary(corte) {
        corte += 1;
    }
    format!("{marca}{}", &text[corte..])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viva(id: &str, backend: &str, host: &str, cwd_key: &str) -> LiveSession {
        LiveSession {
            id: id.into(),
            backend: backend.into(),
            host: host.into(),
            cwd_key: cwd_key.into(),
            root: None,
            running: true,
        }
    }

    fn pedido<'a>(backend: &'a str, host: &'a str, cwd_key: &'a str, depth: u8) -> SpawnCheck<'a> {
        SpawnCheck {
            backend,
            host,
            cwd_key,
            depth,
            root: Some("r1"),
            parent: None,
        }
    }

    #[test]
    fn profundidad_dos_pasa_y_tres_se_rechaza() {
        assert!(
            check_spawn(pedido("codex", "local", "c:/repo", 1), &[], &[]).is_ok(),
            "el segundo salto pasa"
        );
        let err = check_spawn(pedido("codex", "local", "c:/repo", 2), &[], &[])
            .expect_err("el tercer salto se rechaza");
        assert_eq!(err.code, "depth_exceeded");
    }

    #[test]
    fn mismo_backend_host_cwd_vivo_se_rechaza_con_la_sesion() {
        let live = [viva("s1", "codex", "local", "c:/repo")];
        let err = check_spawn(pedido("codex", "local", "c:/repo", 0), &live, &[])
            .expect_err("el reuso se rechaza");
        assert_eq!(err.code, "already_running");
        assert_eq!(err.data.unwrap()["session"], "s1");
    }

    #[test]
    fn already_running_de_si_mismo_lo_dice() {
        let live = [viva("s1", "claude-code", "local", "c:/repo")];
        let mut req = pedido("claude-code", "local", "c:/repo", 0);
        req.parent = Some("s1");
        let err = check_spawn(req, &live, &[]).expect_err("el reuso se rechaza");
        assert!(
            err.message.contains("Es esta misma"),
            "el padre que se apunta a sí: {}",
            err.message
        );
    }

    #[test]
    fn mismo_cwd_en_otro_host_no_es_reuso() {
        let live = [viva("s1", "claude-code", "local", "c:/repo")];
        assert!(
            check_spawn(pedido("claude-code", "oficina", "c:/repo", 0), &live, &[]).is_ok(),
            "otro host es otra carpeta aunque el path coincida"
        );
    }

    #[cfg(windows)]
    #[test]
    fn cwd_con_barras_y_mayusculas_da_la_misma_clave() {
        assert_eq!(cwd_key("C:\\Repo\\Atic"), cwd_key("c:/repo/atic"));
        assert_eq!(cwd_key(r"\\?\C:\Repo"), cwd_key(r"C:\Repo"));
    }

    #[test]
    fn ciclo_en_el_mismo_root_se_rechaza() {
        let cadena = vec![("codex".to_owned(), "c:/repo".to_owned())];
        let err = check_spawn(pedido("codex", "local", "c:/repo", 0), &[], &cadena)
            .expect_err("el ciclo se rechaza");
        assert_eq!(err.code, "cycle");
    }

    #[test]
    fn mismo_backend_en_otro_root_no_es_ciclo() {
        // La cadena que se pasa ya es la de este `root`: si está vacía, no hay ciclo.
        assert!(
            check_spawn(pedido("codex", "local", "c:/repo", 0), &[], &[]).is_ok(),
            "otro encargo no es ciclo"
        );
    }

    fn disponible(ids: &[&str]) -> Vec<(&'static str, bool)> {
        ["claude-code", "codex", "cursor", "opencode"]
            .iter()
            .copied()
            .map(|b| (b, ids.contains(&b)))
            .collect()
    }

    #[test]
    fn kind_plan_prefiere_claude_y_cae_a_desempate() {
        assert_eq!(
            route(Some(Kind::Plan), &disponible(&["claude-code", "codex"])),
            Some("claude-code")
        );
        assert_eq!(
            route(Some(Kind::Plan), &disponible(&["codex", "cursor"])),
            Some("codex"),
            "sin Claude cae al desempate"
        );
    }

    #[test]
    fn kind_patch_y_review_prefieren_codex() {
        let todos = disponible(&["claude-code", "codex", "cursor", "opencode"]);
        assert_eq!(route(Some(Kind::Patch), &todos), Some("codex"));
        assert_eq!(route(Some(Kind::Review), &todos), Some("codex"));
    }

    #[test]
    fn kind_apply_prefiere_cursor() {
        let todos = disponible(&["claude-code", "codex", "cursor", "opencode"]);
        assert_eq!(route(Some(Kind::Apply), &todos), Some("cursor"));
    }

    #[test]
    fn sin_kind_toma_el_primero_disponible_del_desempate() {
        assert_eq!(
            route(None, &disponible(&["cursor", "opencode"])),
            Some("cursor")
        );
    }

    #[test]
    fn un_solo_disponible_ignora_kind() {
        assert_eq!(
            route(Some(Kind::Plan), &disponible(&["opencode"])),
            Some("opencode"),
            "uno solo manda aunque el kind pida otro"
        );
    }

    #[test]
    fn ninguno_disponible_no_rutea() {
        assert_eq!(route(Some(Kind::Plan), &disponible(&[])), None);
        assert_eq!(route(None, &disponible(&[])), None);
    }

    #[test]
    fn la_espera_se_recorta_a_300() {
        assert_eq!(clamp_wait(50), 50);
        assert_eq!(clamp_wait(300), 300);
        assert_eq!(clamp_wait(9999), 300);
    }

    #[test]
    fn el_texto_se_recorta_por_la_cola_con_marca() {
        let largo = "x".repeat(TEXT_CAP_BYTES + 100);
        let recortado = cap_text(&largo);
        assert!(recortado.starts_with("[…recortado…]\n"));
        assert!(recortado.len() <= TEXT_CAP_BYTES + "…".len());
        assert!(recortado.ends_with(&"x".repeat(100)));
    }
}
