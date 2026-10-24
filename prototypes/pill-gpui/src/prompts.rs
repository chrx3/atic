//! Los prompts de cada persona y los de Atic.
//!
//! Hay dos clases: los del dictado (el `prompt` que Groq Whisper usa para el
//! vocabulario y el idioma) y los del resumen (las instrucciones que recibe
//! el modelo en vez de las de una plantilla). Los de Atic no se guardan: se
//! arman cada vez, en el idioma que toca (`default_dictation`, las plantillas
//! de `atic_summarize`). Los propios van en `prompts.json`, en la carpeta de
//! datos de Atic, con el que está elegido para el dictado.
//!
//! Editar el de Atic no lo cambia: al volver a abrirlo aparece el de fábrica.
//! Para quedarse con una versión hay que guardarla como propia.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Para qué es un prompt.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Dictation,
    Summary,
}

/// Un prompt que guardó la persona.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub text: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    prompts: Vec<Saved>,
    /// El propio elegido para el dictado; `None`: el de Atic.
    #[serde(default)]
    dictation: Option<String>,
}

/// El vocabulario que Whisper suele escribir mal sin ayuda.
const VOCABULARY: &str = "Atic, GPUI, notch, pill, Claude, Claude Code, Codex, Groq, Tauri, Svelte, Rust, commit, push, deploy, prompt";

/// El prompt de Atic para el dictado, en el idioma en que se dicta.
pub fn default_dictation(english: bool) -> String {
    if english {
        format!("Dictation in English, with punctuation and capital letters. Vocabulary: {VOCABULARY}.")
    } else {
        format!("Dictado en español de Chile, con tildes y puntuación. Vocabulario: {VOCABULARY}.")
    }
}

/// Si el dictado va en inglés: el idioma elegido para dictar, o el de la UI
/// si se deja al sistema o a la detección.
pub fn dictation_english(language: &str) -> bool {
    match language {
        "en" => true,
        "es" => false,
        _ => crate::i18n::english(),
    }
}

/// El texto que se manda a Groq al dictar: el propio elegido o el de Atic.
pub fn dictation_text(english: bool) -> String {
    active_dictation().map(|p| p.text).unwrap_or_else(|| default_dictation(english))
}

fn path() -> Option<PathBuf> {
    crate::paths::data_dir().map(|dir| dir.join("prompts.json"))
}

fn load() -> Store {
    path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn store(store: &Store) {
    let Some(path) = path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match serde_json::to_string_pretty(store) {
        Ok(text) => {
            if let Err(error) = std::fs::write(&path, text) {
                tracing::warn!(%error, "no se pudieron guardar los prompts");
            }
        }
        Err(error) => tracing::warn!(%error, "no se pudieron guardar los prompts"),
    }
}

/// Los propios de una clase, en el orden en que se guardaron.
pub fn saved(kind: Kind) -> Vec<Saved> {
    load().prompts.into_iter().filter(|p| p.kind == kind).collect()
}

pub fn get(id: &str) -> Option<Saved> {
    load().prompts.into_iter().find(|p| p.id == id)
}

/// Guarda uno nuevo y lo devuelve. Sin nombre, «Mi prompt N».
pub fn add(kind: Kind, name: &str, text: &str) -> Saved {
    let mut all = load();
    let count = all.prompts.iter().filter(|p| p.kind == kind).count() + 1;
    let name = name.trim();
    let prompt = Saved {
        id: format!("{}-{}", match kind { Kind::Dictation => "dict", Kind::Summary => "sum" }, now_millis()),
        kind,
        name: if name.is_empty() { crate::i18n::tf("pill.prompts.untitled", &[("n", &count)]) } else { name.to_string() },
        text: text.to_string(),
    };
    all.prompts.push(prompt.clone());
    store(&all);
    prompt
}

/// Cambia el nombre y el texto de uno propio.
pub fn update(id: &str, name: &str, text: &str) {
    let mut all = load();
    if let Some(prompt) = all.prompts.iter_mut().find(|p| p.id == id) {
        if !name.trim().is_empty() {
            prompt.name = name.trim().to_string();
        }
        prompt.text = text.to_string();
        store(&all);
    }
}

/// Borra uno propio; si era el del dictado, vuelve el de Atic.
pub fn remove(id: &str) {
    let mut all = load();
    all.prompts.retain(|p| p.id != id);
    if all.dictation.as_deref() == Some(id) {
        all.dictation = None;
    }
    store(&all);
}

/// El propio elegido para el dictado, si sigue existiendo.
pub fn active_dictation() -> Option<Saved> {
    let all = load();
    let id = all.dictation?;
    all.prompts.into_iter().find(|p| p.id == id && p.kind == Kind::Dictation)
}

/// Elige el prompt del dictado; `None`: el de Atic.
pub fn set_active_dictation(id: Option<&str>) {
    let mut all = load();
    all.dictation = id.map(str::to_string);
    store(&all);
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_de_atic_va_en_el_idioma_del_dictado() {
        assert!(default_dictation(false).starts_with("Dictado en español"));
        assert!(default_dictation(true).starts_with("Dictation in English"));
        assert!(default_dictation(true).contains("Claude Code"));
        assert!(dictation_english("en"));
        assert!(!dictation_english("es"));
    }

    #[test]
    fn el_archivo_se_lee_aunque_falten_campos() {
        let store: Store = serde_json::from_str("{}").unwrap();
        assert!(store.prompts.is_empty() && store.dictation.is_none());
        let store: Store = serde_json::from_str(r#"{"prompts":[{"id":"sum-1","kind":"summary","name":"Mío","text":"Hazlo corto"}]}"#).unwrap();
        assert_eq!(store.prompts[0].kind, Kind::Summary);
    }
}
