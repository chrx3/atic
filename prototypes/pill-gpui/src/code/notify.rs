//! Avisos del sistema cuando Atic Code no se está mirando, como `notify.ts` de
//! la referencia: Claude terminó, tuvo un problema o pide un permiso. Se muestran con
//! el ícono de la pill en la bandeja de Windows (`tray_icon::notify`); un clic
//! en el aviso abre Atic Code. Sin la pill (`CODE_ALONE=1`) no hay ícono y no
//! se avisa.
//!
//! «Mirando» es la ventana al frente con esa conversación a la vista. Si
//! estás en otra conversación, también se avisa (como en la referencia).

use serde_json::Value;

use super::chat::{Chat, Item};
use super::CodeView;

/// Cuánto de la última respuesta va en el aviso (`notifyBody` de la referencia).
const SNIPPET: usize = 140;

/// Lo primero de una respuesta, en una línea y sin marcas de markdown.
fn snippet(text: &str) -> String {
    let plain: String = text.chars().filter(|c| !"#*`_>".contains(*c)).collect();
    let one_line = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line.chars().take(SNIPPET).collect()
}

/// El cuerpo de «Claude terminó»: el título de la conversación y, debajo,
/// el comienzo de lo último que dijo.
fn done_body(title: &str, last_text: Option<&str>) -> String {
    match last_text.map(snippet).filter(|s| !s.is_empty()) {
        Some(snippet) => format!("{title}\n{snippet}"),
        None => title.to_string(),
    }
}

/// El título del aviso con el proyecto delante, si hay.
fn headline(project: Option<&str>, title: &str) -> String {
    match project.filter(|p| !p.is_empty()) {
        Some(project) => format!("{project} · {title}"),
        None => title.to_string(),
    }
}

/// Qué avisar de un evento de la conversación `chat`, si algo: el título, el
/// cuerpo y si es un problema. `interrupted` es si el usuario detuvo el turno
/// (un turno detenido no se avisa).
fn alert_for(chat: &Chat, event: &str, data: &Value, interrupted: bool) -> Option<(&'static str, String, bool)> {
    match event {
        "permission" => {
            let what = ["title", "displayName", "toolName"].iter().find_map(|name| data.get(*name).and_then(Value::as_str).filter(|s| !s.is_empty())).unwrap_or_default();
            Some(("Claude necesita tu permiso", format!("{}\n{what}", chat.title), false))
        }
        "result" if !interrupted => {
            let last = chat.items.iter().rev().find_map(|item| match item {
                Item::Text(text) => Some(text.as_str()),
                _ => None,
            });
            let failed = data.get("isError").and_then(Value::as_bool).unwrap_or(false);
            let title = if failed { "Claude tuvo un problema" } else { "Claude terminó" };
            Some((title, done_body(&chat.title, last), failed))
        }
        _ => None,
    }
}

impl CodeView {
    /// Avisa al sistema de lo que pasó en la conversación `key`, salvo que se
    /// esté mirando.
    pub(super) fn system_alert(&self, key: &str, event: &str, data: &Value, interrupted: bool) {
        if self.window_active && self.active.as_deref() == Some(key) {
            return;
        }
        let Some(chat) = self.chats.iter().find(|c| c.key == key) else {
            return;
        };
        let Some((title, body, error)) = alert_for(chat, event, data, interrupted) else {
            return;
        };
        let project = self.workspaces.get(chat.workspace).map(|w| w.name.as_str());
        send(&headline(project, title), &body, error);
    }
}

#[cfg(windows)]
fn send(title: &str, body: &str, error: bool) {
    crate::platform::tray_icon::notify(title, body, error);
}

#[cfg(not(windows))]
fn send(_: &str, _: &str, _: bool) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chat_with(text: &str) -> Chat {
        let mut chat = Chat::new("c".into(), 1, std::path::PathBuf::from("."));
        chat.title = "Arreglar el login".into();
        chat.items.push(Item::user("hola"));
        chat.items.push(Item::Text(text.into()));
        chat
    }

    #[test]
    fn el_cuerpo_lleva_el_titulo_y_el_comienzo_de_la_respuesta() {
        let (title, body, error) =
            alert_for(&chat_with("## Listo\n\nArreglé **el login** con `auth.rs`.\n> y los tests"), "result", &json!({ "isError": false }), false).unwrap();
        assert_eq!((title, error), ("Claude terminó", false));
        assert_eq!(body, "Arreglar el login\nListo Arreglé el login con auth.rs. y los tests");
        let long = "palabra ".repeat(50);
        let (_, body, _) = alert_for(&chat_with(&long), "result", &json!({}), false).unwrap();
        assert_eq!(body.lines().nth(1).unwrap().chars().count(), SNIPPET);
        // Sin texto, solo el título.
        let mut empty = chat_with("");
        empty.items.pop();
        assert_eq!(alert_for(&empty, "result", &json!({}), false).unwrap().1, "Arreglar el login");
    }

    #[test]
    fn fallo_permiso_y_detenido() {
        let chat = chat_with("x");
        let (title, _, error) = alert_for(&chat, "result", &json!({ "isError": true }), false).unwrap();
        assert_eq!((title, error), ("Claude tuvo un problema", true));
        // Lo que el usuario detuvo no se avisa.
        assert!(alert_for(&chat, "result", &json!({}), true).is_none());
        let (title, body, _) = alert_for(&chat, "permission", &json!({ "toolName": "Bash", "displayName": "Ejecutar comando" }), false).unwrap();
        assert_eq!(title, "Claude necesita tu permiso");
        assert_eq!(body, "Arreglar el login\nEjecutar comando");
        assert!(alert_for(&chat, "assistant", &json!({}), false).is_none());
    }

    #[test]
    fn el_titulo_lleva_el_proyecto() {
        assert_eq!(headline(Some("atic"), "Claude terminó"), "atic · Claude terminó");
        assert_eq!(headline(None, "Claude terminó"), "Claude terminó");
        assert_eq!(headline(Some(""), "Claude terminó"), "Claude terminó");
    }
}
