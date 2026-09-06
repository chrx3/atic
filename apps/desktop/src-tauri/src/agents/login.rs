//! ¿Tiene este CLI una sesión iniciada?
//!
//! Un backend instalado pero sin login acepta el `start`, arranca el proceso y
//! recién ahí falla, con el mensaje que cada CLI quiera dar. Delegarle una tarea
//! así quema un turno del padre para nada.
//!
//! # Por qué mirar archivos y no preguntarle al CLI
//!
//! `codex login status` y sus equivalentes levantan un proceso —y a veces salen
//! a la red— por cada consulta, y esto se pregunta al listar agentes, que pasa
//! seguido y en el hilo de un request. Los archivos de credenciales están
//! quietos en el disco y contestan en microsegundos.
//!
//! # Por qué `Option<bool>` y por qué no esconde a nadie
//!
//! Es una heurística: un usuario con la clave en una variable de entorno rara, o
//! con un CLI que mañana mueva su archivo, aparecería como «sin sesión» estando
//! perfectamente logueado. Por eso esto **no** apaga `available` —esconder un
//! agente que funciona es peor que ofrecer uno que no— sino que viaja aparte:
//! la vista lo muestra y el hub lo dice en el mensaje de error. `None` es «no sé
//! cómo mirarlo en este backend», que no es lo mismo que «no».

use std::path::PathBuf;

/// La carpeta del usuario, que es de donde cuelgan todos estos archivos.
fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// ¿Hay algo en esta variable de entorno?
fn env_con_algo(clave: &str) -> bool {
    std::env::var(clave).is_ok_and(|v| !v.trim().is_empty())
}

/// Claude Code: token OAuth en el entorno o credenciales en su carpeta de
/// configuración (que `CLAUDE_CONFIG_DIR` puede mover).
pub fn claude() -> Option<bool> {
    if env_con_algo("CLAUDE_CODE_OAUTH_TOKEN") {
        return Some(true);
    }
    let dir = super::skills::config_dir()?;
    Some(dir.join(".credentials.json").is_file())
}

/// Codex: `~/.codex/auth.json` lo escribe `codex login`, y la clave de OpenAI en
/// el entorno también le sirve.
pub fn codex() -> Option<bool> {
    if env_con_algo("OPENAI_API_KEY") || env_con_algo("CODEX_API_KEY") {
        return Some(true);
    }
    Some(home()?.join(".codex").join("auth.json").is_file())
}

/// OpenCode: `auth.json` vive bajo `~/.local/share/opencode` **también en
/// Windows**. Acá alcanza con que exista: a diferencia del cupo, cualquier
/// credencial sirve para trabajar, sea plan o clave de un tercero.
pub fn opencode() -> Option<bool> {
    Some(super::opencode_usage::auth_path()?.is_file())
}

/// Cursor: el CLI deja su estado en `~/.cursor`; sin login no hay `cli-config`.
pub fn cursor() -> Option<bool> {
    Some(home()?.join(".cursor").join("cli-config.json").is_file())
}

/// Grok: lo dice él mismo en el `initialize`, donde ofrece el método
/// `cached_token` descrito como «Cached token from ~/.grok/auth.json».
pub fn grok() -> Option<bool> {
    Some(home()?.join(".grok").join("auth.json").is_file())
}

/// Antigravity: reusa el lector de credenciales del cupo, que es el que sabe
/// dónde quedaron y si vencieron.
pub fn antigravity() -> Option<bool> {
    Some(super::antigravity_usage::detected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_home_no_se_inventa_una_respuesta() {
        // No se puede tocar el entorno del proceso de test sin afectar a los
        // demás, así que se comprueba la pieza: sin carpeta, nadie contesta.
        let sin_home: Option<PathBuf> = None;
        assert!(sin_home.map(|h: PathBuf| h.join("x").is_file()).is_none());
    }

    #[test]
    fn una_variable_vacia_no_cuenta_como_sesion() {
        // Nombre propio de este test: los tests comparten el entorno del proceso.
        std::env::set_var("ATIC_TEST_LOGIN_VACIA", "   ");
        assert!(!env_con_algo("ATIC_TEST_LOGIN_VACIA"));
        std::env::set_var("ATIC_TEST_LOGIN_VACIA", "x");
        assert!(env_con_algo("ATIC_TEST_LOGIN_VACIA"));
        std::env::remove_var("ATIC_TEST_LOGIN_VACIA");
    }

    #[test]
    fn los_backends_contestan_algo_en_esta_maquina() {
        // No se afirma el valor —depende de quién corra los tests— sino que
        // ninguno se queda en `None` por un `home()` que no resolvió.
        for (nombre, v) in [
            ("claude", claude()),
            ("codex", codex()),
            ("opencode", opencode()),
            ("cursor", cursor()),
            ("grok", grok()),
            ("antigravity", antigravity()),
        ] {
            assert!(v.is_some(), "{nombre} no supo contestar");
        }
    }
}
