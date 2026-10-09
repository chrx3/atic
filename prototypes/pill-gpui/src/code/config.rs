//! La configuración de Claude de cada espacio de trabajo: el modelo, cómo pide
//! permisos, el esfuerzo y si muestra el razonamiento. Vale para las
//! conversaciones nuevas y se aplica en vivo a las abiertas. Se guarda en
//! `code-claude.json`, aparte de los espacios.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Los modos de permisos de Claude Code, con su nombre en la interfaz.
pub const MODES: [(&str, &str); 4] = [
    ("default", "Preguntar"),
    ("acceptEdits", "Editar automáticamente"),
    ("plan", "Plan"),
    ("bypassPermissions", "Omitir permisos"),
];

/// Los esfuerzos, de menos a más (los de la referencia). Sin elegir, decide Claude Code.
pub const EFFORTS: [(&str, &str); 5] = [("low", "Bajo"), ("medium", "Medio"), ("high", "Alto"), ("xhigh", "Muy alto"), ("max", "Máximo")];

pub fn effort_label(id: &str) -> &'static str {
    EFFORTS.iter().find(|(e, _)| *e == id).map(|(_, label)| *label).unwrap_or("Predeterminado")
}

/// Lo que explica cada modo en su menú.
pub fn mode_hint(id: &str) -> &'static str {
    match id {
        "acceptEdits" => "Edita archivos sin preguntar; pregunta lo demás",
        "plan" => "Investiga y propone un plan antes de tocar nada",
        "bypassPermissions" => "No pregunta nada: úsalo con cuidado",
        _ => "Pregunta antes de editar o ejecutar",
    }
}

/// Los modelos mientras el sidecar no diga cuáles hay: los alias de Claude Code.
pub const MODELS: [(&str, &str); 4] = [("", "Predeterminado"), ("opus", "Opus"), ("sonnet", "Sonnet"), ("haiku", "Haiku")];

/// Los modelos a la vista y los demás (índices en `list`): de cada familia
/// («Opus 4.1», «Opus 4.5»…) solo el más nuevo, más el elegido y los que no
/// tienen versión (el predeterminado). Como `splitModels` de la referencia.
pub fn split_models(list: &[(String, String)], current: &str) -> (Vec<usize>, Vec<usize>) {
    let parse = |name: &str| -> Option<(String, f64)> {
        let mut parts = name.split_whitespace();
        let family = parts.next()?;
        let version = parts.next()?.parse().ok()?;
        parts.next().is_none().then(|| (family.to_string(), version))
    };
    let mut newest: HashMap<String, f64> = HashMap::new();
    for (_, name) in list {
        if let Some((family, version)) = parse(name) {
            let best = newest.entry(family).or_insert(version);
            *best = best.max(version);
        }
    }
    let (mut main, mut more) = (Vec::new(), Vec::new());
    for (index, (id, name)) in list.iter().enumerate() {
        let shown = match parse(name) {
            None => true,
            Some((family, version)) => newest.get(&family) == Some(&version) || id == current,
        };
        if shown { main.push(index) } else { more.push(index) }
    }
    (main, more)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    /// Vacío: el de Claude Code.
    pub model: String,
    pub permission_mode: String,
    pub effort: String,
    pub thinking: bool,
    pub ultracode: bool,
    pub fast_mode: bool,
    /// Cambiar de modelo cuando Claude Code marca un mensaje.
    pub switch_model_on_flag: bool,
    /// «default» o el nombre de un estilo de salida.
    pub output_style: String,
    pub sandbox: bool,
    /// Claude in Chrome: se fija al abrir la sesión.
    pub chrome: bool,
    pub remote_control: bool,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            permission_mode: "default".into(),
            effort: String::new(),
            thinking: false,
            ultracode: false,
            fast_mode: false,
            switch_model_on_flag: true,
            output_style: "default".into(),
            sandbox: false,
            chrome: false,
            remote_control: false,
        }
    }
}

impl ClaudeConfig {
    pub fn mode_label(&self) -> &'static str {
        MODES.iter().find(|(id, _)| *id == self.permission_mode).map(|(_, label)| *label).unwrap_or("Preguntar")
    }

    /// Lo que se suma a `start` para abrir una sesión con esta configuración.
    pub fn start_params(&self, params: &mut Value) {
        if !self.model.is_empty() {
            params["model"] = json!(self.model);
        }
        if !self.effort.is_empty() {
            params["effort"] = json!(self.effort);
        }
        params["permissionMode"] = json!(self.permission_mode);
        params["thinking"] = json!(self.thinking);
        params["chrome"] = json!(self.chrome);
        params["remoteControl"] = json!(self.remote_control);
        params["flags"] = self.flags(None);
    }

    /// Los ajustes de Claude Code que van por `applyFlags`, como en la referencia: todos
    /// (al abrir una sesión) o solo los que cambiaron respecto de `before`.
    pub fn flags(&self, before: Option<&ClaudeConfig>) -> Value {
        let mut flags = serde_json::Map::new();
        let changed = |pick: fn(&ClaudeConfig) -> Value| before.is_none_or(|b| pick(b) != pick(self));
        if before.is_some() && changed(|c| json!(c.effort)) && !self.effort.is_empty() {
            flags.insert("effortLevel".into(), json!(self.effort));
        }
        if changed(|c| json!(c.thinking)) {
            flags.insert("alwaysThinkingEnabled".into(), json!(self.thinking));
        }
        if changed(|c| json!(c.ultracode)) {
            flags.insert("ultracode".into(), json!(self.ultracode));
        }
        if changed(|c| json!(c.fast_mode)) {
            flags.insert("fastMode".into(), json!(self.fast_mode));
        }
        if changed(|c| json!(c.switch_model_on_flag)) {
            flags.insert("switchModelsOnFlag".into(), json!(self.switch_model_on_flag));
        }
        if changed(|c| json!(c.output_style)) {
            flags.insert("outputStyle".into(), json!(self.output_style));
        }
        if changed(|c| json!(c.sandbox)) && (before.is_some() || self.sandbox) {
            flags.insert("sandbox".into(), json!({ "enabled": self.sandbox }));
        }
        Value::Object(flags)
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Configs {
    by_workspace: HashMap<u64, ClaudeConfig>,
    /// El estilo y el modo de la ventana: de la app, no de cada proyecto.
    #[serde(default)]
    pub style: super::style::Style,
    #[serde(default)]
    pub mode: super::style::Mode,
    /// La línea de estado bajo la caja de texto (modelo y último turno).
    #[serde(default)]
    pub status_line: bool,
}

impl Configs {
    pub fn load() -> Self {
        crate::paths::file("code-claude.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = crate::paths::file("code-claude.json") else {
            return;
        };
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(error) = std::fs::write(&path, text) {
                    tracing::warn!(%error, "atic code: no se guardó la configuración de Claude");
                }
            }
            Err(error) => tracing::warn!(%error, "atic code: configuración sin serializar"),
        }
    }

    pub fn get(&self, workspace: u64) -> ClaudeConfig {
        self.by_workspace.get(&workspace).cloned().unwrap_or_default()
    }

    pub fn set(&mut self, workspace: u64, config: ClaudeConfig) {
        self.by_workspace.insert(workspace, config);
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_predeterminado_no_fuerza_modelo_ni_esfuerzo() {
        let mut params = json!({ "key": "c1" });
        ClaudeConfig::default().start_params(&mut params);
        assert_eq!(params["permissionMode"], "default");
        assert!(params.get("model").is_none() && params.get("effort").is_none());

        let config = ClaudeConfig { model: "opus".into(), effort: "high".into(), permission_mode: "plan".into(), thinking: true, ..Default::default() };
        let mut params = json!({});
        config.start_params(&mut params);
        assert_eq!((&params["model"], &params["effort"], &params["thinking"]), (&json!("opus"), &json!("high"), &json!(true)));
        assert_eq!(config.mode_label(), "Plan");
    }

    #[test]
    fn en_vivo_solo_van_los_ajustes_que_cambiaron() {
        let before = ClaudeConfig::default();
        let after = ClaudeConfig { effort: "max".into(), sandbox: true, ..before.clone() };
        assert_eq!(after.flags(Some(&before)), json!({ "effortLevel": "max", "sandbox": { "enabled": true } }));
        // Al abrir, todos menos el sandbox apagado y el esfuerzo (que va aparte).
        let all = before.flags(None);
        assert!(all.get("sandbox").is_none() && all.get("effortLevel").is_none());
        assert_eq!(all["outputStyle"], "default");
    }

    #[test]
    fn de_cada_familia_se_ve_el_modelo_mas_nuevo() {
        let list: Vec<(String, String)> = [("", "Predeterminado"), ("opus-41", "Opus 4.1"), ("opus-45", "Opus 4.5"), ("sonnet-45", "Sonnet 4.5"), ("sonnet-4", "Sonnet 4")]
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        assert_eq!(split_models(&list, ""), (vec![0, 2, 3], vec![1, 4]));
        // El elegido se ve aunque sea viejo.
        assert_eq!(split_models(&list, "opus-41"), (vec![0, 1, 2, 3], vec![4]));
    }
}
