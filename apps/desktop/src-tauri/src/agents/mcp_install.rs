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

use std::path::{Path, PathBuf};
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
///
/// OpenCode acepta `opencode.json` y `opencode.jsonc`, y honra
/// `XDG_CONFIG_HOME`: devolver una sola ruta fija es lo que dejó al MCP
/// funcionando con el enchufe en rojo (el usuario tenía `.jsonc`). El orden
/// es el de preferencia del CLI; el archivo a editar es el primero que
/// exista, y si no existe ninguno, el canónico.
fn config_candidates(cli: &str) -> Option<Vec<PathBuf>> {
    let home = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?);
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    Some(match cli {
        "cursor-agent" | "cursor" => vec![home.join(".cursor").join("mcp.json")],
        "opencode" => vec![
            base.join("opencode").join("opencode.jsonc"),
            base.join("opencode").join("opencode.json"),
        ],
        _ => return None,
    })
}

/// El archivo a editar: el primero que ya exista; si no hay ninguno, el
/// canónico de la lista (para OpenCode, `.jsonc`, el formato que su propio
/// CLI escribe).
fn config_json(cli: &str) -> Option<PathBuf> {
    let candidatos = config_candidates(cli)?;
    Some(
        candidatos
            .iter()
            .find(|ruta| ruta.exists())
            .cloned()
            .unwrap_or_else(|| candidatos[0].clone()),
    )
}

/// OpenCode v2 mudó el registro de MCP: trae `opencode mcp add` y la entrada
/// vive bajo `mcp.servers`. v1 exigía editar `{"mcp": {…}}` a mano.
fn opencode_v2() -> bool {
    version_major("opencode").is_some_and(|major| major >= 2)
}

/// El número mayor de `<cli> --version`, o `None` si no se puede correr.
fn version_major(cli: &str) -> Option<u32> {
    let (program, prefix) = super::exe::launcher(cli)?;
    let mut cmd = Command::new(program);
    cmd.args(prefix).arg("--version").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let salida = cmd.output().ok()?;
    parse_major(&format!(
        "{}{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    ))
}

/// `"2.0.2"`, `"opencode v2.0.2"` o `"1.18.30"` → 2, 2, 1.
///
/// Se prefiere el primer token con forma de versión (`2.0.2`): un año o un
/// número suelto en un banner de ayuda no la tienen. Sin ninguno, cae a un
/// entero corto (`"opencode v2"` → 2), nunca a un año de cuatro cifras.
fn parse_major(texto: &str) -> Option<u32> {
    let con_punto = texto.split_whitespace().find_map(|token| {
        let token = token.trim_start_matches('v');
        let (mayor, resto) = token.split_once('.')?;
        if !resto.starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        mayor.parse().ok()
    });
    con_punto.or_else(|| {
        texto.split_whitespace().find_map(|token| {
            let mayor: u32 = token.trim_start_matches('v').parse().ok()?;
            (mayor < 100).then_some(mayor)
        })
    })
}

/// Los dos formatos de OpenCode: v1 `mcp.atic`, v2 `mcp.servers.atic`.
fn entrada_mcp_presente(cfg: &Value) -> bool {
    cfg.get("mcp").and_then(|m| m.get(NOMBRE)).is_some()
        || cfg
            .get("mcp")
            .and_then(|m| m.get("servers"))
            .and_then(|s| s.get(NOMBRE))
            .is_some()
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
///
/// Acepta JSONC para leer: comentarios y coma final no llegan a serde.
/// ESCRIBIR sigue con `serde_json` puro — reescribir un `.jsonc` pierde sus
/// comentarios, y por eso el toggle de OpenCode v2 pasa por el CLI (que sí los
/// preserva), no por acá.
fn leer_json(ruta: &Path) -> Result<Value, String> {
    match std::fs::read_to_string(ruta) {
        Ok(texto) if texto.trim().is_empty() => Ok(json!({})),
        Ok(texto) => {
            // El BOM de UTF-8 (Notepad, algunos editores de Windows) no es
            // JSON y hacía fallar la lectura de un archivo por lo demás sano.
            let texto = texto.trim_start_matches('\u{feff}');
            let limpio = jsonc_a_json(texto);
            serde_json::from_str(&limpio)
                .map_err(|e| format!("{} no es JSON válido: {e}", ruta.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("no se pudo leer {}: {e}", ruta.display())),
    }
}

/// JSONC → JSON en un solo barrido: recorta `// …` de línea y `/* … */` de
/// bloque sin tocar lo que está dentro de un string, y suelta la coma antes
/// de `}` o `]` (legal en JSONC, no en serde). Los delimitadores son ASCII,
/// así que el resto del texto pasa tal cual, multibyte incluido.
fn jsonc_a_json(texto: &str) -> String {
    let mut out = String::with_capacity(texto.len());
    // Comas y espacios a la espera de saber si la coma sobra.
    let mut pendiente = String::new();
    let mut chars = texto.chars().peekable();
    let mut en_string = false;
    let mut escapado = false;
    while let Some(c) = chars.next() {
        if en_string {
            // El string va entero, con las comas que lleve dentro.
            out.push_str(&pendiente);
            pendiente.clear();
            out.push(c);
            if escapado {
                escapado = false;
            } else if c == '\\' {
                escapado = true;
            } else if c == '"' {
                en_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                out.push_str(&pendiente);
                pendiente.clear();
                en_string = true;
                out.push(c);
            }
            '/' => match chars.peek() {
                Some('/') => {
                    chars.next();
                    out.push_str(&pendiente);
                    pendiente.clear();
                    for n in chars.by_ref() {
                        if n == '\n' {
                            out.push('\n');
                            break;
                        }
                    }
                }
                Some('*') => {
                    chars.next();
                    out.push_str(&pendiente);
                    pendiente.clear();
                    let mut anterior = '\0';
                    for n in chars.by_ref() {
                        if anterior == '*' && n == '/' {
                            break;
                        }
                        anterior = n;
                    }
                    out.push(' ');
                }
                _ => {
                    out.push_str(&pendiente);
                    pendiente.clear();
                    out.push(c);
                }
            },
            ',' => pendiente.push(','),
            '}' | ']' => {
                if pendiente.contains(',') {
                    pendiente.retain(|ch| ch != ',');
                }
                out.push_str(&pendiente);
                pendiente.clear();
                out.push(c);
            }
            ' ' | '\t' | '\n' | '\r' => pendiente.push(c),
            _ => {
                out.push_str(&pendiente);
                pendiente.clear();
                out.push(c);
            }
        }
    }
    out.push_str(&pendiente);
    out
}

fn escribir_json(ruta: &Path, valor: &Value) -> Result<(), String> {
    let texto = serde_json::to_string_pretty(valor)
        .map_err(|e| format!("no se pudo serializar la config: {e}"))?;
    // Atómica: son archivos de otros CLIs —el `mcp.json` de Cursor puede
    // llevar credenciales de otros servidores— y un corte a mitad de la
    // escritura los dejaría truncados, que es el modo de fallo que ya evita
    // `config.json` con este mismo helper.
    atic_core::fs_atomic::write_atomic_str(ruta, &texto)
        .map_err(|e| format!("no se pudo escribir {}: {e}", ruta.display()))
}

/// ¿La salida de `mcp list` lista a `atic` como servidor?
///
/// Se compara el primer token con nombre de cada línea, no un `contains`: un
/// servidor llamado `atic-extra`, una ruta que mencione el binario o un
/// mensaje de error con la palabra daban el enchufe por conectado.
fn nombre_en_lista(texto: &str) -> bool {
    texto.lines().any(|linea| {
        linea
            .split_whitespace()
            .map(|token| {
                token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
            })
            .find(|token| !token.is_empty())
            .is_some_and(|primero| primero == NOMBRE)
    })
}

/// ¿Tiene este CLI el servidor `atic` registrado?
pub fn instalado(cli: &str) -> Result<bool, String> {
    if host_de(cli).is_none() {
        return Err(format!("«{cli}» no es un agente que Atic sepa conectar."));
    }
    // OpenCode v2 trae `mcp list`: el CLI sabe dónde vive su config —json,
    // jsonc, XDG o lo que venga— y la lee mejor que nosotros. Si el usuario la
    // mudó o la renombró a `.jsonc`, el punto del enchufe no puede mentir.
    if cli == "opencode" && opencode_v2() {
        let (_, texto) = mcp_cmd(cli, &["list".to_string()])?;
        return Ok(nombre_en_lista(&texto));
    }
    if let Some(candidatos) = config_candidates(cli) {
        for ruta in candidatos {
            if !ruta.exists() {
                continue;
            }
            let cfg = leer_json(&ruta)?;
            if cli == "opencode" {
                if entrada_mcp_presente(&cfg) {
                    return Ok(true);
                }
            } else {
                let (clave, _) = entrada_json(cli, "", "");
                if cfg.get(&clave).and_then(|m| m.get(NOMBRE)).is_some() {
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    }
    // Los que se administran solos: se les pregunta a ellos.
    let (_, texto) = mcp_cmd(cli, &["list".to_string()])?;
    Ok(nombre_en_lista(&texto))
}

/// Registra el servidor. Repetirlo no duplica nada: todos los `mcp add` que se
/// usan acá son «add or update», y el JSON se reescribe por clave.
pub fn instalar(cli: &str) -> Result<(), String> {
    let host = host_de(cli).ok_or_else(|| format!("«{cli}» no se puede conectar."))?;
    let ruta = sidecar()?;
    // v2 sabe registrarse solo y escribe su formato (`mcp.servers`); v1 no
    // tiene un `add` no interactivo y sigue por el JSON.
    if cli == "opencode" && opencode_v2() {
        let args = vec![
            "add".to_string(),
            "--global".to_string(),
            NOMBRE.to_string(),
            "--".to_string(),
            ruta.clone(),
            "--host".to_string(),
            host.to_string(),
        ];
        let (ok, texto) = mcp_cmd(cli, &args)?;
        return if ok {
            Ok(())
        } else {
            Err(recorte(&texto, cli))
        };
    }
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
    if cli == "opencode" {
        // v2 no tiene `mcp remove`: se limpia de TODOS los archivos que el CLI
        // pueda leer (.jsonc y .json, XDG incluido). La reescritura va por
        // serde: si el archivo era jsonc, sus comentarios no sobreviven — el
        // propio CLI edita el jsonc preservándolos, pero no sabe quitar.
        if let Some(candidatos) = config_candidates(cli) {
            for archivo in candidatos {
                if !archivo.exists() {
                    continue;
                }
                let mut cfg = leer_json(&archivo)?;
                if !entrada_mcp_presente(&cfg) {
                    continue;
                }
                if let Some(mapa) = cfg.get_mut("mcp").and_then(|m| m.as_object_mut()) {
                    mapa.remove(NOMBRE);
                    if let Some(servidores) =
                        mapa.get_mut("servers").and_then(|s| s.as_object_mut())
                    {
                        servidores.remove(NOMBRE);
                    }
                }
                escribir_json(&archivo, &cfg)?;
            }
            // Los archivos que el CLI leería ya no lo tienen: no hay nada que
            // limpiar. Si viviera en una config de proyecto, `instalado` —que
            // le pregunta al CLI— lo sigue viendo, y ese estado es el que
            // devuelve el toggle.
            return Ok(());
        }
    } else if let Some(archivo) = config_json(cli) {
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
    fn el_jsonc_se_lee_con_comentarios_y_coma_final() {
        let dir = std::env::temp_dir().join(format!("atic-jsonc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archivo = dir.join("opencode.jsonc");
        std::fs::write(
            &archivo,
            r#"{
  // Comentario de línea con "comillas" dentro
  /* bloque
     multilínea */
  "mcp": {
    "servers": {
      "atic": { "type": "local", "command": ["/x",], },
    },
  },
  "url": "https://x.y/a/b",
}"#,
        )
        .unwrap();

        let cfg = leer_json(&archivo).unwrap();
        assert_eq!(cfg["mcp"]["servers"]["atic"]["type"], "local");
        assert_eq!(cfg["url"], "https://x.y/a/b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn jsonc_no_toca_los_strings_que_llevan_comas() {
        let texto = r#"{"a":"x, y}","b":1,}"#;
        let limpio = jsonc_a_json(texto);
        let cfg: Value = serde_json::from_str(&limpio).unwrap();
        assert_eq!(cfg["a"], "x, y}");
    }

    #[test]
    fn jsonc_sobrevive_los_casos_bordes() {
        // El `//` dentro de un string no abre comentario.
        assert_eq!(jsonc_a_json(r#"{"u":"https://x"}"#), r#"{"u":"https://x"}"#);
        // Escapes: `\"` no cierra el string.
        assert_eq!(
            jsonc_a_json(r#"{"a":"cito \"// eso"}"#),
            r#"{"a":"cito \"// eso"}"#
        );
        // Barra sola (ruta de Windows) no abre comentario de bloque.
        assert_eq!(jsonc_a_json(r#"{"p":"C:\\x"}"#), r#"{"p":"C:\\x"}"#);
        // Sin nada que recortar, sale idéntico.
        let limpio = r#"{"mcp":{"servers":{"atic":{"enabled":true}}}}"#;
        assert_eq!(jsonc_a_json(limpio), limpio);
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

    #[test]
    fn parse_major_lee_los_formatos_de_version() {
        assert_eq!(parse_major("2.0.2"), Some(2));
        assert_eq!(parse_major("opencode v2.0.2"), Some(2));
        assert_eq!(parse_major("1.18.30"), Some(1));
        assert_eq!(
            parse_major("OpenCode command line interface\n1.18.30"),
            Some(1)
        );
        assert_eq!(parse_major("opencode v2"), Some(2));
        assert_eq!(parse_major("sin números"), None);
        // Un año en el banner no es una versión: la detección de v2 no puede
        // depender de la primera cifra que aparezca.
        assert_eq!(parse_major("© 2026 OpenCode"), None);
    }

    #[test]
    fn el_nombre_del_servidor_se_lee_del_primer_token() {
        assert!(nombre_en_lista(
            "atic: /x/atic-mcp --host claude-code - ✓ Connected"
        ));
        assert!(nombre_en_lista(
            "Checking MCP server health…\natic  /x/atic-mcp --host codex"
        ));
        assert!(nombre_en_lista("• atic: conectado"));
        // Mencionar «atic» no es listarlo: un servidor con otro nombre, una
        // ruta o un error no pueden dar el enchufe por conectado.
        assert!(!nombre_en_lista("atic-extra: /x/atic-mcp"));
        assert!(!nombre_en_lista("Error: no se encontró el binario atic"));
        assert!(!nombre_en_lista("Checking MCP server health…\n"));
        assert!(!nombre_en_lista(""));
    }

    #[test]
    fn un_bom_no_rompe_la_lectura() {
        let dir = std::env::temp_dir().join(format!("atic-bom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archivo = dir.join("opencode.jsonc");
        std::fs::write(&archivo, "\u{feff}{ \"mcp\": {} }").unwrap();
        assert_eq!(leer_json(&archivo).unwrap(), json!({ "mcp": {} }));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn escribir_json_no_deja_temporales() {
        let dir = std::env::temp_dir().join(format!("atic-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let archivo = dir.join("mcp.json");
        escribir_json(&archivo, &json!({ "mcpServers": {} })).unwrap();
        assert_eq!(leer_json(&archivo).unwrap(), json!({ "mcpServers": {} }));
        let sobrantes: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(sobrantes.is_empty(), "quedó basura: {sobrantes:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opencode_acepta_los_dos_formatos_de_entrada_mcp() {
        let v1 = json!({ "mcp": { "atic": { "type": "local" } } });
        let v2 = json!({ "mcp": { "servers": { "atic": { "type": "local" } } } });
        assert!(entrada_mcp_presente(&v1));
        assert!(entrada_mcp_presente(&v2));
        assert!(!entrada_mcp_presente(
            &json!({ "mcp": { "servers": { "otro": {} } } })
        ));
        assert!(!entrada_mcp_presente(&json!({})));
    }
}
