//! Hub de orquestación: directorio de agentes + espera a fin de turno.
//!
//! Fase 0: tipos y reglas puras (`api`, `graph`, `wait`). Fase 1: servidor
//! HTTP + cableado a `bridge` (`server`, estado y merge de `mcp_config` acá).

pub mod api;
pub mod graph;
pub mod server;
pub mod wait;

use std::path::PathBuf;
use std::sync::Mutex;

use atic_core::MutexExt;

/// Estado vivo del hub: lo que `atic-mcp` necesita para encontrar a Atic.
#[derive(Debug, Clone)]
pub struct HubState {
    pub port: u16,
    pub token: String,
    pub mcp_path: Option<PathBuf>,
    pub hub_json: PathBuf,
}

static HUB: Mutex<Option<HubState>> = Mutex::new(None);

/// Guarda el estado al arrancar; lo lee `mcp_server_entry` y `hub_status`.
pub fn set_running(state: HubState) {
    *HUB.lock_or_recover() = Some(state);
}

/// Lo borra al parar.
pub fn set_stopped() {
    *HUB.lock_or_recover() = None;
}

/// ¿Acepta el hub pedidos ahora?
pub fn is_running() -> bool {
    HUB.lock_or_recover().is_some()
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

/// Dónde está `atic-mcp`: junto al ejecutable instalado, o en `target/` en dev.
pub fn mcp_path() -> Option<PathBuf> {
    // Instalado: al lado de `Atic.exe`.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let junto = dir.join(exe_name("atic-mcp"));
            if junto.is_file() {
                return Some(junto);
            }
        }
    }
    // Dev: el más nuevo entre `target/debug` y `target/release`.
    if cfg!(debug_assertions) {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut mejor: Option<(PathBuf, std::time::SystemTime)> = None;
        for perfil in ["debug", "release"] {
            let p = manifest
                .join("..")
                .join("..")
                .join("..")
                .join("target")
                .join(perfil)
                .join(exe_name("atic-mcp"));
            if let Ok(meta) = std::fs::metadata(&p) {
                let tiempo = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                let es_mejor = mejor.as_ref().is_none_or(|(_, t)| tiempo > *t);
                if es_mejor {
                    mejor = Some((p, tiempo));
                }
            }
        }
        if let Some((p, _)) = mejor {
            return Some(p);
        }
    }
    None
}

/// El servidor `atic` en forma neutral, para que cada adaptador lo traduzca a
/// lo suyo: JSON en Claude, `-c mcp_servers.atic.*` en Codex, `mcp_servers` del
/// `session/new` en ACP. Sin esto, un hijo que no es Claude puede recibir
/// trabajo pero no repartirlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AticMcp {
    pub command: std::path::PathBuf,
    pub args: Vec<String>,
}

/// El servidor `atic` para un hijo del backend dado: solo si el hub corre y hay
/// binario que anunciarle. El `--host` es el id del backend porque de él salen
/// el presupuesto de espera del sidecar y la clave de `already_running`.
pub fn atic_mcp(host: &str) -> Option<AticMcp> {
    if !is_running() {
        return None;
    }
    let command = HUB
        .lock_or_recover()
        .as_ref()
        .and_then(|s| s.mcp_path.clone())
        .or_else(mcp_path)?;
    Some(AticMcp {
        command,
        args: vec!["--host".to_string(), host.to_string()],
    })
}

/// Entrada `atic` para el `--mcp-config` del hijo Claude.
pub fn mcp_server_entry() -> Option<serde_json::Value> {
    let atic = atic_mcp("claude-code")?;
    Some(serde_json::json!({
        "type": "stdio",
        "command": atic.command.to_string_lossy(),
        "args": atic.args,
    }))
}

/// Estado para Ajustes → Agentes → «Desde otras apps».
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubStatus {
    pub running: bool,
    pub port: Option<u16>,
    pub mcp_path: Option<String>,
}

/// ¿Corre el hub y dónde está el sidecar?
#[tauri::command]
pub fn hub_status() -> HubStatus {
    match HUB.lock_or_recover().clone() {
        Some(estado) => HubStatus {
            running: true,
            port: Some(estado.port),
            mcp_path: estado.mcp_path.map(|p| p.to_string_lossy().into_owned()),
        },
        None => HubStatus {
            running: false,
            port: None,
            mcp_path: mcp_path().map(|p| p.to_string_lossy().into_owned()),
        },
    }
}

/// Snippet para pegar en cada host. `{MCP}` es la ruta absoluta real: un
/// binario «en el PATH» falla en Windows igual que `opencode`.
#[tauri::command]
pub fn hub_snippet(host: String) -> Result<String, String> {
    let mcp = HUB
        .lock_or_recover()
        .as_ref()
        .and_then(|s| s.mcp_path.clone())
        .or_else(mcp_path)
        .ok_or_else(|| {
            "No se encontró atic-mcp. En dev, ejecuta `pnpm mcp:build` primero.".to_string()
        })?;
    formatear_snippet(&host, &mcp)
}

/// Arma el texto a pegar. JSON/TOML escapan `\`; la línea de `claude mcp add` no.
pub fn formatear_snippet(host: &str, mcp: &std::path::Path) -> Result<String, String> {
    let cruda = mcp.to_string_lossy();
    let json = cruda.replace('\\', "\\\\").replace('"', "\\\"");
    match host {
        "claude-code" => Ok(format!(
            "claude mcp add --scope user atic -- \"{cruda}\" --host claude-code"
        )),
        "cursor" => Ok(format!(
            "{{ \"mcpServers\": {{ \"atic\": {{ \"command\": \"{json}\", \"args\": [\"--host\", \"cursor\"] }} }} }}"
        )),
        "codex" => Ok(format!(
            "[mcp_servers.atic]\ncommand = \"{json}\"\nargs = [\"--host\", \"codex\", \"--wait\", \"300\"]\ntool_timeout_sec = 330"
        )),
        "opencode" => Ok(format!(
            "{{ \"mcp\": {{ \"atic\": {{ \"type\": \"local\", \"command\": [\"{json}\", \"--host\", \"opencode\"], \"enabled\": true }} }} }}"
        )),
        // Los dos últimos tienen su propio `mcp add`, así que van como comando
        // y sin escapar. En Grok los args del servidor van tras `--` o se los
        // queda él.
        "grok" => Ok(format!(
            "grok mcp add --scope user atic \"{cruda}\" -- --host grok"
        )),
        "antigravity" => Ok(format!("agy mcp add atic \"{cruda}\" --host antigravity")),
        _ => Err(format!("Host desconocido: {host}.")),
    }
}

/// Junta los MCP del modal (`agent_mcp_servers`: array JSON de
/// `{ name, json, enabled }`), lo que la UI mande en `req.mcp_config` y el
/// servidor `atic` de orquestación, en un `{"mcpServers": {…}}` para Claude.
///
/// Con conflicto de nombre gana el usuario, salvo `atic`, que es nuestro.
/// Devuelve `None` si no hay ningún servidor que anunciar.
pub fn merge_mcp_config(
    agent_mcp_servers: &str,
    req_mcp_config: Option<&str>,
    atic: Option<serde_json::Value>,
) -> Option<String> {
    let mut servidores = serde_json::Map::new();
    // Los del modal que estén `enabled`.
    if let Ok(lista) = serde_json::from_str::<Vec<ModalServer>>(agent_mcp_servers) {
        for s in lista {
            if !s.enabled || s.name.trim().is_empty() || s.name == "atic" {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s.json) {
                servidores.insert(s.name, v);
            }
        }
    }
    // Lo que la UI mande (`{"mcpServers": {…}}` o el mapa pelado).
    if let Some(raw) = req_mcp_config {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            let mapa = v
                .get("mcpServers")
                .and_then(|m| m.as_object())
                .or_else(|| v.as_object());
            if let Some(mapa) = mapa {
                for (k, v) in mapa {
                    if k != "atic" {
                        servidores.insert(k.clone(), v.clone());
                    }
                }
            }
        }
    }
    // El nuestro, al final y sin discusión.
    if let Some(atic) = atic {
        servidores.insert("atic".to_string(), atic);
    }
    if servidores.is_empty() {
        return None;
    }
    Some(serde_json::json!({ "mcpServers": servidores }).to_string())
}

#[derive(serde::Deserialize)]
struct ModalServer {
    #[serde(default)]
    name: String,
    #[serde(default)]
    json: String,
    #[serde(default)]
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_merge_suma_atic_y_respeta_lo_del_usuario() {
        let modal = r#"[{"name":"fs","json":"{\"command\":\"npx\"}","enabled":true},{"name":"off","json":"{\"command\":\"x\"}","enabled":false},{"name":"atic","json":"{\"command\":\"malo\"}","enabled":true}]"#;
        let req = r#"{"mcpServers":{"extra":{"command":"y"}}}"#;
        let atic = serde_json::json!({"command":"atic-mcp"});
        let fuera = merge_mcp_config(modal, Some(req), Some(atic)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&fuera).unwrap();
        let mapa = v["mcpServers"].as_object().unwrap();
        assert!(mapa.contains_key("fs"), "el del modal va");
        assert!(mapa.contains_key("extra"), "el del req va");
        assert!(!mapa.contains_key("off"), "el deshabilitado no va");
        assert_eq!(mapa["atic"]["command"], "atic-mcp", "atic es el nuestro");
    }

    #[test]
    fn sin_servidores_no_hay_config() {
        assert_eq!(merge_mcp_config("", None, None), None);
        assert_eq!(merge_mcp_config("[]", None, None), None);
    }

    #[test]
    fn snippet_claude_no_escapa_barras() {
        let mcp = std::path::Path::new(r"C:\atic\atic-mcp.exe");
        let claude = formatear_snippet("claude-code", mcp).unwrap();
        assert!(
            claude.contains(r"C:\atic\atic-mcp.exe"),
            "la línea de claude lleva la ruta cruda: {claude}"
        );
        assert!(
            !claude.contains(r"C:\\atic"),
            "claude mcp add no es JSON: {claude}"
        );
        let cursor = formatear_snippet("cursor", mcp).unwrap();
        assert!(
            cursor.contains(r"C:\\atic\\atic-mcp.exe"),
            "el JSON sí escapa: {cursor}"
        );
    }

    #[test]
    fn los_snippets_de_comando_no_escapan_y_llevan_su_host() {
        let mcp = std::path::Path::new(r"C:\atic\atic-mcp.exe");
        for (host, marca) in [("grok", "grok mcp add"), ("antigravity", "agy mcp add")] {
            let s = formatear_snippet(host, mcp).unwrap();
            assert!(s.starts_with(marca), "{s}");
            assert!(s.contains(r"C:\atic\atic-mcp.exe"), "sin escapar: {s}");
            assert!(s.contains(&format!("--host {host}")), "{s}");
        }
        // Grok se queda los flags del servidor si no van tras `--`.
        assert!(formatear_snippet("grok", mcp)
            .unwrap()
            .contains("-- --host"));
    }

    #[test]
    fn un_host_que_no_conocemos_se_rechaza() {
        let mcp = std::path::Path::new("/opt/atic-mcp");
        assert!(formatear_snippet("gemini", mcp).is_err());
    }
}
