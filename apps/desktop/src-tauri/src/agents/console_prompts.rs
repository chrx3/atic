//! Lo que un agente en una consola de Atic espera del usuario: un permiso o
//! una pregunta con opciones. Se contestan desde el celular escribiendo en la
//! consola las mismas teclas que usaría uno.
//!
//! El registro de lo pendiente y qué teclas contestan viven en
//! `atic_agents::prompts` (los comparte la pill GPUI). Acá queda lo de esta
//! app: en qué consola corre cada sesión y cómo se le escribe, y el servidor
//! de OpenCode (ver [`super::console_opencode`]).
//!
//! Kimi: Atic no encuentra su consola por el proceso, así que cada consola
//! lleva una marca propia (ver [`register_console_token`]).

use std::collections::HashMap;
use std::sync::Mutex;

use atic_agents::prompts::{self, Decision, Prompts, Reply};
use atic_core::MutexExt;
use serde_json::Value;

use super::PermissionDecision;

pub(crate) use atic_agents::prompts::{Cli, Prompt};

static PENDING: Mutex<Option<Prompts>> = Mutex::new(None);

/// Marca de consola → id de la consola, y sesión → marca. Para los CLI cuya
/// consola no se encuentra por el proceso (Kimi).
static TOKENS: Mutex<Option<(HashMap<String, String>, HashMap<String, String>)>> = Mutex::new(None);

fn with_prompts<R>(f: impl FnOnce(&mut Prompts) -> R) -> R {
    f(PENDING.lock_or_recover().get_or_insert_with(Default::default))
}

/// La consola recién lanzada con esta marca en su entorno.
pub(crate) fn register_console_token(token: &str, console: &str) {
    let mut guard = TOKENS.lock_or_recover();
    guard.get_or_insert_with(Default::default).0.insert(token.to_string(), console.to_string());
}

/// Un hook de la sesión llegó desde la consola con esta marca.
pub(crate) fn link_session(session: &str, token: &str) {
    let mut guard = TOKENS.lock_or_recover();
    guard.get_or_insert_with(Default::default).1.insert(session.to_string(), token.to_string());
}

fn console_by_token(session: &str) -> Option<String> {
    let guard = TOKENS.lock_or_recover();
    let (consoles, sessions) = guard.as_ref()?;
    consoles.get(sessions.get(session)?).cloned()
}

/// Lo que dice un hook sobre lo que espera su sesión.
pub(crate) fn observe(v: &Value, cli: Cli) {
    with_prompts(|prompts| prompts.observe(v, cli));
}

/// Lo pendiente que lista el servidor de un OpenCode. Lo que ese puerto ya no
/// lista, se contestó.
pub(crate) fn sync_opencode(port: u16, live: Vec<(String, String, Prompt)>) {
    with_prompts(|prompts| prompts.sync_opencode(port, live));
}

/// ¿Se puede contestar desde acá? Por HTTP (OpenCode) siempre; con teclas,
/// solo si la sesión corre en una consola de Atic.
pub(crate) fn answerable(session: &str) -> bool {
    match with_prompts(|prompts| prompts.port(session)) {
        Some(Some(_)) => true,
        Some(None) => console_of(session).is_ok(),
        None => false,
    }
}

/// Lo que espera una sesión: su id y qué es.
pub(crate) fn pending(session: &str) -> Option<(String, Prompt)> {
    with_prompts(|prompts| prompts.pending(session))
}

/// Contesta una pregunta tecleando en la consola donde corre la sesión.
pub(crate) fn answer(session: &str, id: &str, answers: &[String]) -> Result<(), String> {
    let pending = with_prompts(|prompts| prompts.take(session, id))?;
    let reply = prompts::answer_reply(&pending, answers)?;
    send(session, id, pending.port, reply)
}

/// Contesta un permiso, o descarta una pregunta (rechazar es Esc en los dos).
pub(crate) fn decide(session: &str, id: &str, decision: PermissionDecision) -> Result<(), String> {
    let pending = with_prompts(|prompts| prompts.take(session, id))?;
    let decision = match decision {
        PermissionDecision::Allow => Decision::Allow,
        PermissionDecision::AllowAlways => Decision::AllowAlways,
        PermissionDecision::Deny => Decision::Deny,
    };
    let reply = prompts::decide_reply(&pending, decision)?;
    send(session, id, pending.port, reply)
}

fn send(session: &str, id: &str, port: Option<u16>, reply: Reply) -> Result<(), String> {
    match (reply, port) {
        (Reply::Keys(steps), _) => {
            let console = console_of(session)?;
            prompts::type_steps(steps, move |keys| super::console::write_input(&console, keys));
            Ok(())
        }
        (Reply::OpenCodeAnswer(lists), Some(port)) => super::console_opencode::reply_question(port, id, lists),
        (Reply::OpenCodeReject, Some(port)) => super::console_opencode::reject_question(port, id),
        (Reply::OpenCodePermission(reply), Some(port)) => {
            super::console_opencode::reply_permission(port, id, reply)
        }
        (_, None) => Err("respuesta de OpenCode sin puerto".into()),
    }
}

fn console_of(session: &str) -> Result<String, String> {
    console_by_token(session)
        .or_else(|| super::console::console_for_presence(session.to_string()))
        .ok_or_else(|| "la sesión no corre en una consola de Atic".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_marca_de_la_consola_lleva_a_su_sesion() {
        register_console_token("tok-1", "consola-9");
        link_session("kimi-s1", "tok-1");
        assert_eq!(console_of("kimi-s1").as_deref(), Ok("consola-9"));
    }
}
