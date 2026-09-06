//! Registrar el servidor `atic` en la config de un CLI del usuario.
//!
//! # Por qué hace falta
//!
//! Las sesiones que abre el hub reciben el MCP inyectado al arrancarlas. Las
//! consolas que abre el usuario son su CLI de siempre, con su config de
//! siempre: Atic solo les da un terminal. Sin el servidor en su archivo, esa
//! sesión no ve las herramientas y el agente acaba llamando al otro por shell,
//! que es justo lo que la orquestación viene a evitar.
//!
//! # Por qué se le pide al CLI y no se le edita el archivo
//!
//! Casi todos traen `mcp add` / `mcp remove`, y usarlo es más seguro que
//! escribirles el JSON o el TOML: respeta su formato, sus comentarios y lo que
//! ya tuvieran dentro —el de Cursor lleva credenciales de otros servidores—.
//! Solo se edita a mano donde no hay otra: Cursor no publica CLI de MCP y el
//! `mcp add` de OpenCode es un asistente interactivo.
//!
//! # Codex y su corte de 60 s
//!
//! `codex mcp add` no sabe escribir `tool_timeout_sec`, así que en vez de
//! pelearse con su TOML se le registra con el presupuesto corto: el sidecar
//! devuelve el traspaso a los 50 s, por debajo del corte, y el padre conserva
//! el `session` para seguir con `atic_wait`. Quien quiera esperas largas tiene
//! el snippet de Ajustes, que sí lleva los dos números.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use serde_json::{json, Value};

/// El nombre con el que se registra en todas partes.
const NOMBRE: &str = "atic";

/// Qué `--host` le toca a cada CLI. De él salen el presupuesto de espera del
/// sidecar y la clave de `already_running`.
fn host_de(cli: &str) -> Option<&'static str> {
    Some(match cli {
        "claude" => "claude-code",
        "codex" => "codex",
        "opencode" => "opencode",
        "cursor-agent" | "cursor" => "cursor",
        "agy" => "antigravity",
        "grok" => "grok",
        _ => return None,
    })
}

/// Dónde vive el archivo de un CLI que no sabe registrarse solo.
fn config_json(cli: &str) -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let home = PathBuf::from(home);
    Some(match cli {
        "cursor-agent" | "cursor" => home.join(".cursor").join("mcp.json"),
        "opencode" => home.join(".config").join("opencode").join("opencode.json"),
        _ => return None,
    })
}

/// La ruta del sidecar, o el motivo por el que no se puede ofrecer.
fn sidecar() -> Result<String, String> {
    super::hub::mcp_path()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| {
            "No se encontró atic-mcp. En dev, ejecuta `pnpm mcp:build` primero.".to_string()
        })
}

/// Corre `<cli> mcp …` y devuelve su salida.
fn mcp_cmd(cli: &str, args: &[String]) -> Result<(bool, String), String> {
    let (program, prefijo) =
        super::exe::launcher(cli).ok_or_else(|| format!("no se encontró «{cli}» en el PATH."))?;
    let mut cmd = Command::new(program);
    cmd.args(prefijo).arg("mcp").args(args).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let salida = cmd
        .output()
        .map_err(|e| format!("no se pudo correr «{cli} mcp»: {e}"))?;
    let texto = format!(
        "{}{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    );
    Ok((salida.status.success(), texto))
}

/// Los argumentos de `mcp add` de cada CLI, que no se ponen de acuerdo.
fn args_add(cli: &str, ruta: &str, host: &str) -> Option<Vec<String>> {
    let s = |x: &str| x.to_string();
    Some(match cli {
        // `--` separa la config del CLI de la del servidor.
        "claude" => vec![
            s("add"),
            s("--scope"),
            s("user"),
            s(NOMBRE),
            s("--"),
            s(ruta),
            s("--host"),
            s(host),
        ],
        "codex" => vec![s("add"), s(NOMBRE), s("--"), s(ruta), s("--host"), s(host)],
        // Grok se queda los flags del servidor si no van tras `--`.
        "grok" => vec![
            s("add"),
            s("--scope"),
            s("user"),
            s(NOMBRE),
            s(ruta),
            s("--"),
            s("--host"),
            s(host),
        ],
        "agy" => vec![s("add"), s(NOMBRE), s(ruta), s("--host"), s(host)],
        _ => return None,
    })
}

fn args_remove(cli: &str) -> Option<Vec<String>> {
    let s = |x: &str| x.to_string();
    Some(match cli {
        "claude" => vec![s("remove"), s(NOMBRE), s("-s"), s("user")],
        "codex" | "agy" => vec![s("remove"), s(NOMBRE)],
        "grok" => vec![s("remove"), s("--scope"), s("user"), s(NOMBRE)],
        _ => return None,
    })
}

/// El servidor tal como lo espera cada archivo.
fn entrada_json(cli: &str, ruta: &str, host: &str) -> (String, Value) {
    match cli {
        // OpenCode: `{"mcp": {"atic": {"type": "local", "command": [...]}}}`
        "opencode" => (
            "mcp".to_string(),
            json!({ "type": "local", "command": [ruta, "--host", host], "enabled": true }),
        ),
        // Cursor y el resto de los JSON: `{"mcpServers": {"atic": {…}}}`
        _ => (
            "mcpServers".to_string(),
            json!({ "command": ruta, "args": ["--host", host] }),
        ),
    }
}

/// Lee el archivo de config, o un objeto vacío si no existe todavía.
fn leer_json(ruta: &PathBuf) -> Result<Value, String> {
    match std::fs::read_to_string(ruta) {
        Ok(texto) if texto.trim().is_empty() => Ok(json!({})),
        Ok(texto) => serde_json::from_str(&texto)
            .map_err(|e| format!("{} no es JSON válido: {e}", ruta.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("no se pudo leer {}: {e}", ruta.display())),
    }
}

fn escribir_json(ruta: &PathBuf, valor: &Value) -> Result<(), String> {
    if let Some(padre) = ruta.parent() {
        std::fs::create_dir_all(padre)
            .map_err(|e| format!("no se pudo crear {}: {e}", padre.display()))?;
    }
    let texto = serde_json::to_string_pretty(valor)
        .map_err(|e| format!("no se pudo serializar la config: {e}"))?;
    std::fs::write(ruta, texto).map_err(|e| format!("no se pudo escribir {}: {e}", ruta.display()))
}

/// ¿Tiene este CLI el servidor `atic` registrado?
pub fn instalado(cli: &str) -> Result<bool, String> {
    if host_de(cli).is_none() {
        return Err(format!("«{cli}» no es un agente que Atic sepa conectar."));
    }
    if let Some(ruta) = config_json(cli) {
        let (clave, _) = entrada_json(cli, "", "");
        return Ok(leer_json(&ruta)?
            .get(&clave)
            .and_then(|m| m.get(NOMBRE))
            .is_some());
    }
    // Los que se administran solos: se les pregunta a ellos.
    let (_, texto) = mcp_cmd(cli, &["list".to_string()])?;
    Ok(texto.lines().any(|l| l.contains(NOMBRE)))
}

/// Registra el servidor. Repetirlo no duplica nada: todos los `mcp add` que se
/// usan acá son «add or update», y el JSON se reescribe por clave.
pub fn instalar(cli: &str) -> Result<(), String> {
    let host = host_de(cli).ok_or_else(|| format!("«{cli}» no se puede conectar."))?;
    let ruta = sidecar()?;
    if let Some(archivo) = config_json(cli) {
        let mut cfg = leer_json(&archivo)?;
        let (clave, entrada) = entrada_json(cli, &ruta, host);
        let mapa = cfg
            .as_object_mut()
            .ok_or_else(|| format!("{} no tiene un objeto en la raíz.", archivo.display()))?;
        mapa.entry(clave)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .ok_or_else(|| format!("{} tiene los servidores en otra forma.", archivo.display()))?
            .insert(NOMBRE.to_string(), entrada);
        return escribir_json(&archivo, &cfg);
    }
    let args = args_add(cli, &ruta, host)
        .ok_or_else(|| format!("no sé cómo registrarle el MCP a «{cli}»."))?;
    let (ok, texto) = mcp_cmd(cli, &args)?;
    if ok {
        Ok(())
    } else {
        Err(recorte(&texto, cli))
    }
}

/// Lo quita. Un CLI que no lo tiene no es un error: el resultado es el pedido.
pub fn quitar(cli: &str) -> Result<(), String> {
    if host_de(cli).is_none() {
        return Err(format!("«{cli}» no se puede desconectar."));
    }
    if let Some(archivo) = config_json(cli) {
        let mut cfg = leer_json(&archivo)?;
        let (clave, _) = entrada_json(cli, "", "");
        if let Some(mapa) = cfg.get_mut(&clave).and_then(|m| m.as_object_mut()) {
            mapa.remove(NOMBRE);
        }
        return escribir_json(&archivo, &cfg);
    }
    let args = args_remove(cli).ok_or_else(|| format!("no sé cómo quitárselo a «{cli}»."))?;
    let (ok, texto) = mcp_cmd(cli, &args)?;
    if ok || texto.to_lowercase().contains("not found") {
        Ok(())
    } else {
        Err(recorte(&texto, cli))
    }
}

/// El mensaje del CLI, sin volcarle media pantalla a la interfaz.
fn recorte(texto: &str, cli: &str) -> String {
    let limpio = texto.trim();
    if limpio.is_empty() {
        return format!("«{cli}» falló sin decir por qué.");
    }
    let corto: String = limpio.lines().take(3).collect::<Vec<_>>().join(" ");
    if corto.chars().count() > 300 {
        format!("{}…", corto.chars().take(300).collect::<String>())
    } else {
        corto
    }
}

/// ¿Está el servidor `atic` en la config de este CLI?
#[tauri::command]
pub async fn agent_mcp_status(cli: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || instalado(&cli))
        .await
        .map_err(|e| format!("consulta cancelada: {e}"))?
}

/// Conecta o desconecta este CLI del hub, y devuelve cómo quedó.
#[tauri::command]
pub async fn agent_mcp_toggle(cli: String, on: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if on {
            instalar(&cli)?;
        } else {
            quitar(&cli)?;
        }
        instalado(&cli)
    })
    .await
    .map_err(|e| format!("operación cancelada: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_cli_conocido_tiene_su_host() {
        for (cli, host) in [
            ("claude", "claude-code"),
            ("codex", "codex"),
            ("opencode", "opencode"),
            ("cursor-agent", "cursor"),
            ("agy", "antigravity"),
            ("grok", "grok"),
        ] {
            assert_eq!(host_de(cli), Some(host), "{cli}");
        }
        assert_eq!(host_de("bash"), None);
    }

    #[test]
    fn solo_editan_archivo_los_que_no_saben_registrarse() {
        // Cursor no publica CLI de MCP y el `add` de OpenCode es interactivo.
        assert!(config_json("cursor-agent").is_some());
        assert!(config_json("opencode").is_some());
        for cli in ["claude", "codex", "grok", "agy"] {
            assert!(config_json(cli).is_none(), "{cli} se administra solo");
            assert!(args_add(cli, "/x", "h").is_some(), "{cli} sabe add");
            assert!(args_remove(cli).is_some(), "{cli} sabe remove");
        }
    }

    #[test]
    fn los_flags_del_servidor_van_tras_el_separador() {
        // Sin `--`, Claude y Grok se quedan el `--host` para ellos.
        for cli in ["claude", "grok"] {
            let args = args_add(cli, "/x/atic-mcp", "h").unwrap();
            let sep = args.iter().position(|a| a == "--").expect("lleva --");
            let host = args.iter().position(|a| a == "--host").expect("lleva host");
            assert!(sep < host, "{cli}: {args:?}");
        }
    }

    #[test]
    fn el_json_de_opencode_tiene_su_propia_forma() {
        let (clave, valor) = entrada_json("opencode", "/x", "opencode");
        assert_eq!(clave, "mcp");
        assert_eq!(valor["type"], "local");
        assert_eq!(valor["command"][0], "/x");

        let (clave, valor) = entrada_json("cursor-agent", "/x", "cursor");
        assert_eq!(clave, "mcpServers");
        assert_eq!(valor["command"], "/x");
        assert_eq!(valor["args"][1], "cursor");
    }

    #[test]
    fn registrar_no_pisa_lo_que_ya_habia() {
        let dir = std::env::temp_dir().join(format!("atic-mcpinst-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archivo = dir.join("mcp.json");
        // Un servidor ajeno, con credenciales dentro: no se puede perder.
        std::fs::write(
            &archivo,
            r#"{"mcpServers":{"otro":{"command":"npx","env":{"TOKEN":"secreto"}}}}"#,
        )
        .unwrap();

        let mut cfg = leer_json(&archivo).unwrap();
        let (clave, entrada) = entrada_json("cursor-agent", "/x/atic-mcp", "cursor");
        cfg.as_object_mut()
            .unwrap()
            .entry(clave)
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap()
            .insert(NOMBRE.to_string(), entrada);
        escribir_json(&archivo, &cfg).unwrap();

        let leido = leer_json(&archivo).unwrap();
        assert_eq!(leido["mcpServers"]["otro"]["env"]["TOKEN"], "secreto");
        assert_eq!(leido["mcpServers"]["atic"]["command"], "/x/atic-mcp");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_archivo_que_no_existe_es_un_objeto_vacio() {
        let ruta = std::env::temp_dir().join("atic-no-existe-jamas.json");
        assert_eq!(leer_json(&ruta).unwrap(), json!({}));
    }
}
