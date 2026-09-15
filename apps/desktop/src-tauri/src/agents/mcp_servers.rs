//! Traducción de los servidores MCP del modal a una forma que cada adaptador
//! pueda inyectar.
//!
//! # Por qué existe
//!
//! Hasta acá, los servidores que el usuario configura en Ajustes solo llegaban
//! a Claude Code: su JSON se mergeaba en `--mcp-config`. Codex los puede
//! recibir como `-c` al arrancar y los ACP en el `mcp_servers` del
//! `session/new` —otras formas—. En vez de que cada adaptador parsee el JSON
//! (y de que cada formato tenga su parser), acá se normaliza una sola vez.
//!
//! # Reglas
//!
//! - Solo servidores **stdio** (los que traen `command`). Un servidor por URL
//!   (SSE/HTTP) no tiene traducción pareja a todos los hosts, así que se
//!   saltea con un aviso; Claude lo sigue entendiendo por su JSON de siempre.
//! - `atic` se ignora en silencio: cada adaptador inyecta el suyo con el
//!   `--host` que le toca, y pisarlo sería peor. Es la misma regla del merge
//!   de Claude.
//! - El nombre tiene que ser una clave segura para `-c clave=valor` de Codex.
//!   Un nombre raro se saltea con aviso, no se inventa una versión escapada.
//!
//! Lo salteado no se tira en silencio: se devuelve en [`Traduccion::salteados`]
//! para que el arranque lo avise una vez.

use std::collections::BTreeMap;

use serde_json::Value;

/// El servidor del modal, tal cual vive en la config (`agent_mcp_servers`).
///
/// Vive acá y no en `hub` porque los dos lados lo leen: el merge de Claude y
/// esta traducción.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ModalServer {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub json: String,
    #[serde(default)]
    pub enabled: bool,
}

/// Un servidor MCP stdio, ya normalizado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServerDef {
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

/// El resultado de traducir las dos entradas (modal + `mcp_config` del request).
#[derive(Debug, Default)]
pub struct Traduccion {
    pub servidores: Vec<McpServerDef>,
    /// `(nombre, motivo)` de lo que no se pudo traducir, para avisar una vez.
    pub salteados: Vec<(String, String)>,
}

/// Traduce lo que el usuario tenga configurado a servidores inyectables.
///
/// El request pisa al modal con el mismo nombre, igual que en el merge de
/// Claude: lo que la UI manda para esta sesión es más nuevo que lo guardado.
pub fn traducir(agent_mcp_servers: &str, req_mcp_config: Option<&str>) -> Traduccion {
    // `BTreeMap`: la salida queda en orden estable (y en el mismo orden para
    // todos los hosts) sin depender del orden de inserción.
    let mut crudos: BTreeMap<String, Value> = BTreeMap::new();

    if let Ok(lista) = serde_json::from_str::<Vec<ModalServer>>(agent_mcp_servers) {
        for s in lista {
            if !s.enabled || s.name.trim().is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&s.json) {
                crudos.insert(s.name.trim().to_string(), v);
            }
        }
    }

    if let Some(raw) = req_mcp_config {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            let mapa = v
                .get("mcpServers")
                .and_then(Value::as_object)
                .or_else(|| v.as_object());
            if let Some(mapa) = mapa {
                for (nombre, valor) in mapa {
                    crudos.insert(nombre.clone(), valor.clone());
                }
            }
        }
    }

    let mut salida = Traduccion::default();
    for (nombre, valor) in crudos {
        // El de orquestación lo pone cada adaptador: si el usuario lo tuviera
        // guardado, el suyo no debe ganarle al de Atic.
        if nombre == "atic" {
            continue;
        }
        match servidor(&nombre, &valor) {
            Ok(s) => salida.servidores.push(s),
            Err(motivo) => salida.salteados.push((nombre, motivo)),
        }
    }
    salida
}

/// Un nombre que se pueda usar como clave de TOML (`-c mcp_servers.<nombre>…`).
fn nombre_seguro(nombre: &str) -> bool {
    !nombre.is_empty()
        && nombre
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Texto de un valor escalar: los JSON de verdad traen números y booleanos
/// donde el host espera texto (un puerto, un `true`), y rechazarlos sería
/// romper por prolijidad.
fn escalar(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn servidor(nombre: &str, valor: &Value) -> Result<McpServerDef, String> {
    if !nombre_seguro(nombre) {
        return Err(
            "el nombre trae caracteres que los hosts no aceptan; usa letras, números, «-» o «_»"
                .into(),
        );
    }
    let Some(obj) = valor.as_object() else {
        return Err("la entrada no es un objeto JSON".into());
    };
    let tipo = obj.get("type").and_then(Value::as_str);
    if obj.contains_key("url") || matches!(tipo, Some("sse" | "http" | "streamable-http")) {
        return Err("es un servidor por URL (SSE/HTTP) y no todos los hosts lo entienden".into());
    }
    let Some(command) = obj
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    else {
        return Err("le falta `command`".into());
    };

    let mut args = Vec::new();
    if let Some(lista) = obj.get("args") {
        let Some(lista) = lista.as_array() else {
            return Err("`args` no es una lista".into());
        };
        for a in lista {
            let Some(texto) = escalar(a) else {
                return Err("hay un `args` que no es texto".into());
            };
            args.push(texto);
        }
    }

    let mut env = Vec::new();
    if let Some(mapa) = obj.get("env") {
        let Some(mapa) = mapa.as_object() else {
            return Err("`env` no es un objeto".into());
        };
        for (clave, valor) in mapa {
            let Some(texto) = escalar(valor) else {
                return Err(format!("`env.{clave}` no es texto"));
            };
            env.push((clave.clone(), texto));
        }
        // Orden estable: el `Map` de `serde_json` itera distinto según las
        // features que unifiquen los otros crates del workspace, y los args no
        // tienen por qué cambiar de orden entre builds.
        env.sort();
    }

    Ok(McpServerDef {
        name: nombre.to_string(),
        command: command.to_string(),
        args,
        env,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El modal guarda `{ name, json, enabled }` y el `json` es la entrada de
    /// Claude con `command`/`args`/`env`.
    fn modal(items: &[(&str, &str, bool)]) -> String {
        serde_json::json!(items
            .iter()
            .map(|(name, json, enabled)| serde_json::json!({
                "name": name,
                "json": json,
                "enabled": enabled,
            }))
            .collect::<Vec<_>>())
        .to_string()
    }

    #[test]
    fn traduce_la_entrada_del_modal_con_args_y_env() {
        let guardada = modal(&[(
            "fs",
            r#"{"command":"npx","args":["-y","server-fs","/tmp"],"env":{"TOKEN":"abc","PORT":8080}}"#,
            true,
        )]);
        let t = traducir(&guardada, None);
        assert!(t.salteados.is_empty(), "{:?}", t.salteados);
        assert_eq!(t.servidores.len(), 1);
        let s = &t.servidores[0];
        assert_eq!(s.name, "fs");
        assert_eq!(s.command, "npx");
        assert_eq!(s.args, ["-y", "server-fs", "/tmp"]);
        assert_eq!(
            s.env,
            [
                ("PORT".to_string(), "8080".to_string()),
                ("TOKEN".to_string(), "abc".to_string())
            ]
        );
    }

    #[test]
    fn el_deshabilitado_no_entra() {
        let guardada = modal(&[("off", r#"{"command":"x"}"#, false)]);
        assert!(traducir(&guardada, None).servidores.is_empty());
    }

    #[test]
    fn el_servidor_de_atic_no_se_toca() {
        let guardada = modal(&[("atic", r#"{"command":"otro"}"#, true)]);
        let t = traducir(&guardada, None);
        assert!(t.servidores.is_empty());
        assert!(
            t.salteados.is_empty(),
            "no se avisa por lo que Atic inyecta"
        );
    }

    #[test]
    fn lo_que_no_tiene_traduccion_se_saltea_con_motivo() {
        let guardada = modal(&[
            (
                "remoto",
                r#"{"type":"sse","url":"https://ejemplo/mcp"}"#,
                true,
            ),
            ("sin-command", r#"{"args":["x"]}"#, true),
            ("raro!", r#"{"command":"x"}"#, true),
        ]);
        let t = traducir(&guardada, None);
        assert!(t.servidores.is_empty());
        let motivos: Vec<&str> = t.salteados.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(motivos, ["raro!", "remoto", "sin-command"]);
        assert!(t.salteados[0].1.contains("nombre"));
        assert!(t.salteados[1].1.contains("URL"));
    }

    #[test]
    fn el_request_pisa_al_modal_y_admite_el_mapa_pelado() {
        let guardada = modal(&[("fs", r#"{"command":"viejo"}"#, true)]);
        let req = r#"{"fs":{"command":"nuevo","args":[]},"otro":{"command":"y"}}"#;
        let t = traducir(&guardada, Some(req));
        assert_eq!(t.servidores.len(), 2);
        assert_eq!(t.servidores[0].name, "fs");
        assert_eq!(t.servidores[0].command, "nuevo", "el request gana");
        assert_eq!(t.servidores[1].name, "otro");
    }
}
