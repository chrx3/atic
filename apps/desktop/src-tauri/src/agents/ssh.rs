//! Transporte SSH para agentes remotos.
//!
//! El comando local es `ssh … "bash -lc 'cd … && exec claude …'"` (un solo
//! argumento remoto: en Windows OpenSSH, partir bash/-lc/script rompe el quoting).
//! El puente stdio no cambia: solo el programa que se spawnea.
//!
//! Seguridad:
//! - Preferir ssh-agent (`auth = "agent"`).
//! - Identity file = ruta; passphrase en keyring vía SSH_ASKPASS.
//! - known_hosts del usuario; sin `StrictHostKeyChecking=no`.
//! - BatchMode en tests para no colgar la UI.
//!
//! Destino:
//! - Alias de `~/.ssh/config`: `host = "contabo"`, user/port vacíos → `ssh contabo`.
//! - Explícito: user + host (+ port opcional) → `ssh [-p N] user@host`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use atic_core::secrets;
use atic_core::SshHost;
use serde::Serialize;

/// Destino remoto ya resuelto desde config (sin secretos en claro).
#[derive(Debug, Clone)]
pub struct RemoteTarget {
    pub host: SshHost,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTestResult {
    pub ok: bool,
    pub message: String,
    /// Epoch secs.
    pub checked_at: i64,
    /// Si `claude` (o el bin configurado) está en el PATH remoto.
    pub agent_available: Option<bool>,
}

/// Helper temporal para SSH_ASKPASS; se borra al dropear.
pub struct AskpassGuard {
    path: PathBuf,
}

impl Drop for AskpassGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Escapa un literal para `bash -lc '…'` (comillas simples).
pub fn bash_single_quote(s: &str) -> String {
    // 'foo'\''bar' → foo'bar dentro de comillas simples.
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// Destino que se pasa a `ssh` (alias o `user@host`).
pub fn ssh_destination(host: &SshHost) -> Result<String, String> {
    let hostname = host.host.trim();
    if hostname.is_empty() {
        return Err("El host SSH necesita un hostname, IP o alias de ~/.ssh/config.".into());
    }
    if hostname.contains('@') || hostname.contains(' ') {
        return Err(
            "El campo Host debe ser solo el hostname, IP o alias (p.ej. `contabo`). \
             No uses `user@host` acá: el usuario va en Usuario (o vacío con alias)."
                .into(),
        );
    }
    // `host:22` pegado por error (no confundir con IPv6, que también tiene `:`).
    if hostname
        .rsplit_once(':')
        .is_some_and(|(_, port)| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        && hostname.matches(':').count() == 1
    {
        return Err(
            "No pongas el puerto en Host (ej. `host:22`). Usá el campo Puerto, \
             o dejalo vacío si el Port viene de ~/.ssh/config."
                .into(),
        );
    }
    let user = host.user.trim();
    if user.is_empty() {
        Ok(hostname.to_string())
    } else {
        if user.contains('@') || user.contains(' ') {
            return Err(
                "Usuario inválido. Si usás un alias de ssh_config, dejá Usuario vacío."
                    .into(),
            );
        }
        Ok(format!("{user}@{hostname}"))
    }
}

/// Args para login shell interactivo: base sin BatchMode + `-t`, sin comando remoto.
///
/// Con PTY/ConPTY el cliente ya ve TTY; `-t` fuerza asignación remota. No usar
/// `CREATE_NO_WINDOW` en el spawn PTY (rompe ConPTY).
pub fn ssh_interactive_args(host: &SshHost) -> Result<Vec<String>, String> {
    let mut args = ssh_base_args(host, false)?;
    let dest = args
        .pop()
        .ok_or_else(|| "argumentos SSH incompletos".to_string())?;
    args.push("-t".into());
    args.push(dest);
    Ok(args)
}

/// Argumentos del cliente `ssh` hasta (sin incluir) el comando remoto.
pub fn ssh_base_args(host: &SshHost, batch_mode: bool) -> Result<Vec<String>, String> {
    let destination = ssh_destination(host)?;
    let mut args = vec![
        "-o".into(),
        "ConnectTimeout=15".into(),
    ];
    // port == 0 → no pasar `-p` (alias / default de OpenSSH o Port en ssh_config).
    if host.port > 0 {
        args.push("-p".into());
        args.push(host.port.to_string());
    }
    if batch_mode {
        args.push("-o".into());
        args.push("BatchMode=yes".into());
    }
    // Nunca StrictHostKeyChecking=no: usar known_hosts del usuario.
    match host.auth.as_str() {
        "key" => {
            let path = host
                .identity_file
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| {
                    "Auth por clave: falta la ruta al identity file.".to_string()
                })?;
            if !Path::new(path).is_file() {
                return Err(format!("No se encontró el identity file: {path}"));
            }
            args.push("-i".into());
            args.push(path.to_string());
            args.push("-o".into());
            args.push("IdentitiesOnly=yes".into());
        }
        "agent" | "password" => {}
        other => {
            return Err(format!(
                "Auth SSH desconocida: {other}. Usá agent o key."
            ));
        }
    }
    args.push(destination);
    Ok(args)
}

/// Ruta al cliente `ssh`, cacheada. Las apps GUI en Windows a menudo no heredan
/// el PATH del usuario; buscamos OpenSSH en ubicaciones conocidas.
pub fn resolve_ssh_program() -> PathBuf {
    static CACHED: OnceLock<PathBuf> = OnceLock::new();
    CACHED.get_or_init(find_ssh_program).clone()
}

fn find_ssh_program() -> PathBuf {
    if let Some(from_path) = find_ssh_on_path() {
        return from_path;
    }
    #[cfg(windows)]
    {
        if let Some(known) = windows_openssh_candidates()
            .into_iter()
            .find(|p| p.is_file())
        {
            return known;
        }
    }
    PathBuf::from(if cfg!(windows) { "ssh.exe" } else { "ssh" })
}

fn find_ssh_on_path() -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    let exe = if cfg!(windows) { "ssh.exe" } else { "ssh" };
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(windows)]
fn windows_openssh_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(sysroot) = std::env::var("SystemRoot") {
        out.push(PathBuf::from(sysroot).join(r"System32\OpenSSH\ssh.exe"));
    }
    out.push(PathBuf::from(r"C:\Windows\System32\OpenSSH\ssh.exe"));
    out
}

fn ssh_program_missing_message(program: &Path) -> String {
    let shown = program.display();
    #[cfg(windows)]
    {
        return format!(
            "No se encontró el cliente OpenSSH (`{shown}`). \
             Instalalo (Configuración → Aplicaciones opcionales → OpenSSH Client) \
             o asegurate de que exista `C:\\Windows\\System32\\OpenSSH\\ssh.exe`. \
             Las apps de escritorio a veces no ven el PATH de tu terminal."
        );
    }
    #[cfg(not(windows))]
    {
        format!(
            "No se encontró `ssh` (`{shown}`). Instalalo (openssh-client) o agregalo al PATH."
        )
    }
}

/// Resuelve el binario `ssh` o error claro si no está.
pub fn ensure_ssh_program() -> Result<PathBuf, String> {
    let program = resolve_ssh_program();
    if program.is_file() {
        return Ok(program);
    }
    // Nombre suelto (`ssh`): en Unix dejamos que el spawn busque en PATH.
    let bare = program.components().count() == 1;
    if bare {
        #[cfg(windows)]
        {
            // Ya probamos PATH + OpenSSH conocido; sin archivo → error claro.
            return Err(ssh_program_missing_message(&program));
        }
        #[cfg(not(windows))]
        {
            return Ok(program);
        }
    }
    Err(ssh_program_missing_message(&program))
}

fn write_askpass_script(passphrase: &str) -> Result<(AskpassGuard, String), String> {
    let dir = std::env::temp_dir();
    let name = format!("atic-ssh-askpass-{}.{}", uuid::Uuid::new_v4(), askpass_ext());
    let path = dir.join(&name);
    #[cfg(windows)]
    {
        let mut f = std::fs::File::create(&path)
            .map_err(|e| format!("no se pudo crear askpass: {e}"))?;
        // El passphrase viaja solo en el entorno del hijo, no en el script.
        writeln!(f, "@echo off").map_err(|e| e.to_string())?;
        writeln!(f, "@echo %ATIC_SSH_PASSPHRASE%").map_err(|e| e.to_string())?;
        let _ = passphrase; // usado vía env en apply_askpass
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o700)
            .open(&path)
            .map_err(|e| format!("no se pudo crear askpass: {e}"))?;
        writeln!(f, "#!/bin/sh").map_err(|e| e.to_string())?;
        writeln!(f, "printf '%s\\n' \"$ATIC_SSH_PASSPHRASE\"").map_err(|e| e.to_string())?;
        let _ = passphrase;
    }
    let path_str = path.to_string_lossy().to_string();
    Ok((AskpassGuard { path }, path_str))
}

fn askpass_ext() -> &'static str {
    if cfg!(windows) {
        "cmd"
    } else {
        "sh"
    }
}

/// Aplica SSH_ASKPASS si hay passphrase en el keyring. Devuelve guard + valor
/// para setear en el Command (no se loguea).
pub fn prepare_askpass(
    host: &SshHost,
) -> Result<(Option<AskpassGuard>, Option<String>, Option<String>), String> {
    if host.auth != "key" {
        return Ok((None, None, None));
    }
    let Some(pass) = secrets::get_ssh_host_secret(&host.id, "passphrase")
        .map_err(|e| e.to_string())?
        .filter(|s| !s.is_empty())
    else {
        return Ok((None, None, None));
    };
    let (guard, script) = write_askpass_script(&pass)?;
    Ok((Some(guard), Some(script), Some(pass)))
}

pub fn apply_askpass_env(cmd: &mut Command, script: &str, passphrase: &str) {
    cmd.env("SSH_ASKPASS", script);
    cmd.env("SSH_ASKPASS_REQUIRE", "force");
    cmd.env("ATIC_SSH_PASSPHRASE", passphrase);
    // OpenSSH en Unix exige DISPLAY o WAYLAND_DISPLAY para usar ASKPASS.
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        cmd.env("DISPLAY", ":0");
    }
}

/// Comando remoto: `cd` opcional + `exec` del CLI con args.
pub fn remote_shell_command(cwd: Option<&str>, program: &str, args: &[String]) -> String {
    let mut parts = Vec::with_capacity(2 + args.len());
    parts.push(bash_single_quote(program));
    for a in args {
        parts.push(bash_single_quote(a));
    }
    let exec = format!("exec {}", parts.join(" "));
    match cwd.map(str::trim).filter(|s| !s.is_empty() && *s != ".") {
        Some(dir) => format!("cd {} && {}", bash_single_quote(dir), exec),
        None => exec,
    }
}

/// Arma `Command` local = `ssh` con stdio piped, listo para spawn.
///
/// El `AskpassGuard` debe vivir mientras el proceso hijo exista (o al menos
/// hasta que autentique).
///
/// El remoto va en **un solo** argumento (`bash -lc '…'`). En Windows OpenSSH,
/// pasar `bash`, `-lc` y el script como argv separados hace que el cliente
/// arme `bash -lc echo hello` sin comillas: bash toma solo `echo` como `-c` y
/// el resto queda en `$0`, así el probe “falla” con exit 0 y stdout vacío.
pub fn build_ssh_command(
    host: &SshHost,
    remote_cmdline: &str,
    batch_mode: bool,
) -> Result<(Command, Option<AskpassGuard>), String> {
    let mut args = ssh_base_args(host, batch_mode)?;
    args.push(format!("bash -lc {}", bash_single_quote(remote_cmdline)));

    let program = ensure_ssh_program()?;
    let mut cmd = Command::new(&program);
    cmd.args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let (guard, script, pass) = prepare_askpass(host)?;
    if let (Some(script), Some(pass)) = (script, pass) {
        apply_askpass_env(&mut cmd, &script, &pass);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    Ok((cmd, guard))
}

fn map_ssh_error(stderr: &str, status_ok: bool, exit_code: Option<i32>) -> String {
    let s = stderr.trim();
    let lower = s.to_lowercase();
    if lower.contains("host key verification failed")
        || lower.contains("not in the list of known hosts")
    {
        return "Host key desconocida. Aceptá la fingerprint una vez en una terminal \
(`ssh user@host` o `ssh <alias>`) o con `ssh -o StrictHostKeyChecking=accept-new`, y reintentá."
            .into();
    }
    if lower.contains("permission denied")
        || lower.contains("authentication failed")
        || lower.contains("publickey")
    {
        return format!(
            "Autenticación SSH fallida. Revisá ssh-agent o el identity file.\n{s}"
        );
    }
    if lower.contains("timed out") || lower.contains("connection timed out") {
        return format!("Timeout al conectar por SSH.\n{s}");
    }
    if lower.contains("could not resolve") || lower.contains("name or service not known") {
        return format!("No se pudo resolver el hostname.\n{s}");
    }
    if s.is_empty() {
        if status_ok {
            "Conexión OK.".into()
        } else {
            let code = exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "?".into());
            let ssh = resolve_ssh_program();
            if ssh.is_absolute() && !ssh.is_file() {
                return ssh_program_missing_message(&ssh);
            }
            format!(
                "La conexión SSH falló (código de salida {code}, sin detalle en stderr). \
                 Si usás un Host alias de ~/.ssh/config, poné solo el alias en Host y dejá \
                 Usuario/Puerto vacíos. Probá el mismo destino en una terminal con `ssh`."
            )
        }
    } else {
        s.to_string()
    }
}

/// Prueba SSH con BatchMode + timeout, y opcionalmente `command -v` del agente.
pub fn test_host(host: &SshHost) -> SshTestResult {
    let checked_at = now_secs();
    let bin = host
        .remote_agent_bin
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("claude");

    let remote = format!(
        "echo ATIC_SSH_OK && command -v {} >/dev/null 2>&1 && echo ATIC_AGENT_OK || echo ATIC_AGENT_MISSING",
        // bin viene de config del usuario; igual lo acotamos a un token simple.
        shell_token(bin)
    );

    let (mut cmd, _guard) = match build_ssh_command(host, &remote, true) {
        Ok(v) => v,
        Err(message) => {
            return SshTestResult {
                ok: false,
                message,
                checked_at,
                agent_available: None,
            };
        }
    };
    // Test: no necesita stdin interactivo.
    cmd.stdin(Stdio::null());

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            let program = resolve_ssh_program();
            let not_found = e.kind() == std::io::ErrorKind::NotFound;
            let message = if not_found {
                ssh_program_missing_message(&program)
            } else {
                format!(
                    "No se pudo ejecutar `{}`: {e}.",
                    program.display()
                )
            };
            return SshTestResult {
                ok: false,
                message,
                checked_at,
                agent_available: None,
            };
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let ok = output.status.success() && stdout.contains("ATIC_SSH_OK");
    let agent_available = if ok {
        Some(stdout.contains("ATIC_AGENT_OK"))
    } else {
        None
    };

    let mut message = map_ssh_error(&stderr, ok, output.status.code());
    if ok {
        message = match agent_available {
            Some(true) => format!("Conexión OK. `{bin}` disponible en el remoto."),
            Some(false) => format!(
                "Conexión OK, pero no se encontró `{bin}` en el PATH remoto."
            ),
            None => "Conexión OK.".into(),
        };
    }

    SshTestResult {
        ok,
        message,
        checked_at,
        agent_available,
    }
}

/// Solo caracteres seguros para un nombre de binario en el probe.
fn shell_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_' || *c == '.' || *c == '/')
        .collect()
}

/// Espera corta usada en tests unitarios del escaping (no bloquea UI).
#[allow(dead_code)]
pub fn test_connect_timeout() -> Duration {
    Duration::from_secs(15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_interactive_has_t_no_batch() {
        let host = SshHost {
            user: "deploy".into(),
            host: "box.example".into(),
            port: 22,
            auth: "agent".into(),
            ..Default::default()
        };
        let args = ssh_interactive_args(&host).unwrap();
        assert!(args.iter().any(|a| a == "-t"));
        assert!(!args.iter().any(|a| a == "BatchMode=yes"));
        assert_eq!(args.last().unwrap(), "deploy@box.example");
        // Sin comando remoto: solo flags + destino.
        assert!(!args.iter().any(|a| a.starts_with("bash")));
    }

    #[test]
    fn bash_quote_simple() {
        assert_eq!(bash_single_quote("hello"), "'hello'");
    }

    #[test]
    fn bash_quote_with_quote() {
        assert_eq!(bash_single_quote("a'b"), "'a'\\''b'");
    }

    #[test]
    fn remote_cmd_with_cwd() {
        let args = vec!["-p".into()];
        let cmd = remote_shell_command(Some("/tmp/proj"), "claude", &args);
        assert!(cmd.starts_with("cd '/tmp/proj' && exec 'claude' '-p'"));
    }

    #[test]
    fn ssh_base_agent() {
        let host = SshHost {
            user: "deploy".into(),
            host: "box.example".into(),
            port: 2222,
            auth: "agent".into(),
            ..Default::default()
        };
        let args = ssh_base_args(&host, true).unwrap();
        assert!(args.contains(&"2222".into()));
        assert!(args.iter().any(|a| a == "BatchMode=yes"));
        assert_eq!(args.last().unwrap(), "deploy@box.example");
    }

    #[test]
    fn ssh_base_alias_omits_user_and_port() {
        let host = SshHost {
            user: "".into(),
            host: "contabo".into(),
            port: 0,
            auth: "agent".into(),
            ..Default::default()
        };
        let args = ssh_base_args(&host, true).unwrap();
        assert!(!args.iter().any(|a| a == "-p"));
        assert_eq!(args.last().unwrap(), "contabo");
        assert_eq!(ssh_destination(&host).unwrap(), "contabo");
    }

    #[test]
    fn ssh_destination_rejects_user_at_host_in_host_field() {
        let host = SshHost {
            user: "root".into(),
            host: "root@66.94.117.178".into(),
            port: 22,
            auth: "agent".into(),
            ..Default::default()
        };
        assert!(ssh_destination(&host).is_err());
    }

    #[test]
    fn ssh_destination_rejects_port_suffix_in_host_field() {
        let host = SshHost {
            user: "".into(),
            host: "66.94.117.178:22".into(),
            port: 0,
            auth: "agent".into(),
            ..Default::default()
        };
        assert!(ssh_destination(&host).is_err());
    }

    #[test]
    fn ssh_destination_allows_ipv6() {
        let host = SshHost {
            user: "root".into(),
            host: "2001:db8::1".into(),
            port: 22,
            auth: "agent".into(),
            ..Default::default()
        };
        assert_eq!(ssh_destination(&host).unwrap(), "root@2001:db8::1");
    }

    #[test]
    fn remote_command_is_single_ssh_arg() {
        // Windows OpenSSH: varios argv (`bash`, `-lc`, script) → `bash -lc echo`
        // sin comillas y el probe “falla” con exit 0. Un solo arg evita eso.
        let remote = format!("bash -lc {}", bash_single_quote("echo ATIC_SSH_OK"));
        assert_eq!(remote, "bash -lc 'echo ATIC_SSH_OK'");
        assert!(!remote.contains(" -- "));
    }
}
