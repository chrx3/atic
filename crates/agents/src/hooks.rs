//! Hooks de los CLI de agentes: un archivo en temp, no un segundo proceso.
//!
//! El CLI manda JSON por stdin a cada hook. El hook anexa la línea a un
//! archivo y Atic la consume por offset ([`Drain`]). No se escribe la
//! configuración ajena: Claude recibe `--settings <archivo>`, Codex un perfil
//! propio (`-p`), Kimi una copia de su config con los hooks.
//!
//! Dónde se anota depende de quién lanzó la consola ([`Sink`]):
//! - **La app de Tauri** usa un archivo compartido por CLI y averigua la
//!   consola por el proceso.
//! - **La pill GPUI** marca cada consola con [`CONSOLE_TOKEN_VAR`] en su
//!   entorno y sus hooks escriben en `atic-pill-<marca>.jsonl`: la marca dice
//!   de qué consola es cada aviso, sin adivinar.

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

/// La variable que lleva la marca de cada consola.
pub const CONSOLE_TOKEN_VAR: &str = "ATIC_CONSOLE_TOKEN";
/// Kimi en las consolas de Tauri: un archivo por consola.
pub const KIMI_PREFIX: &str = "atic-kimi-";
/// Las consolas de la pill: un archivo por consola, para cualquier CLI.
pub const PILL_PREFIX: &str = "atic-pill-";

/// Dónde anotan los hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sink {
    /// Un archivo por CLI (`atic-agent-ping.jsonl`, `atic-codex-ping.jsonl`).
    Shared,
    /// `atic-pill-<marca>.jsonl`, con la marca del entorno de la consola.
    PerConsole,
}

pub fn ping_path() -> PathBuf {
    std::env::temp_dir().join("atic-agent-ping.jsonl")
}

/// Dónde anotan los hooks de Codex. Archivo aparte: sus ids de sesión son de
/// Codex y la presencia tiene que saber de qué CLI es cada uno.
pub fn codex_ping_path() -> PathBuf {
    std::env::temp_dir().join("atic-codex-ping.jsonl")
}

/// Línea de `settings.json` (hooks) de Claude. Con [`Sink::Shared`] es el
/// fragmento que se ofrece para pegar.
pub fn hook_settings(sink: Sink) -> String {
    let command = hook_command(sink);
    serde_json::to_string_pretty(&serde_json::json!({
        "hooks": {
            "PermissionRequest": [{
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "Notification": [{
                "matcher": "permission_prompt|idle_prompt|agent_needs_input",
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "PostToolUse": [{
                // Las que pueden pedir permiso: así se sabe que ya se contestó.
                // Leer y buscar quedan fuera; son las más seguidas y no preguntan.
                "matcher": "AskUserQuestion|Bash|PowerShell|Edit|MultiEdit|Write|NotebookEdit|WebFetch|WebSearch|mcp__.*",
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "UserPromptSubmit": [{
                "hooks": [{ "type": "command", "command": command.clone() }]
            }],
            "Stop": [{
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    }))
    .unwrap_or_else(|_| "{}".into())
}

/// El comando que Claude corre por cada hook.
///
/// Claude Code corre los hooks con `sh`, también en Windows (Git Bash). Un
/// comando de PowerShell no sobrevivía: bash expandía su `$t` a nada antes de
/// pasárselo y el ping nunca llegaba. Anexa la línea JSON que el CLI manda por
/// stdin, tal cual. Con [`Sink::PerConsole`] el `sh` de adentro expande la
/// marca.
fn hook_command(sink: Sink) -> String {
    match sink {
        Sink::Shared => sh_append(&ping_path()),
        Sink::PerConsole => sh_append(&per_console_target()),
    }
}

/// `atic-pill-$ATIC_CONSOLE_TOKEN.jsonl` en temp: el `sh` del hook expande la
/// marca.
fn per_console_target() -> PathBuf {
    std::env::temp_dir().join(format!("{PILL_PREFIX}${CONSOLE_TOKEN_VAR}.jsonl"))
}

/// `sh -c` que anexa stdin, en una línea, a `target`.
fn sh_append(target: &Path) -> String {
    // Git Bash entiende `C:/…`; las barras invertidas se las comería.
    let target = target.to_string_lossy().replace('\\', "/");
    // La ruta va entre comillas dobles dentro del `sh -c`; una comilla simple
    // en el camino se escapa cerrando y reabriendo el literal.
    let target = target.replace('\'', r"'\''");
    format!("sh -c 'printf \"%s\\n\" \"$(cat)\" >> \"{target}\"'")
}

/// El comando de los hooks de Codex. En Windows Codex los corre con
/// PowerShell (no con `sh` como Claude), y su `[Console]::In` lee en la página
/// de códigos de la consola: sin fijar UTF-8 los acentos llegaban rotos.
fn codex_hook_command(sink: Sink) -> String {
    #[cfg(windows)]
    {
        let target = match sink {
            Sink::Shared => format!("'{}'", codex_ping_path().to_string_lossy().replace('\'', "''")),
            Sink::PerConsole => format!(
                "('{}' + $env:{CONSOLE_TOKEN_VAR} + '.jsonl')",
                std::env::temp_dir().join(PILL_PREFIX).to_string_lossy().replace('\'', "''"),
            ),
        };
        format!(
            "[Console]::InputEncoding=[Text.UTF8Encoding]::new($false); [Console]::In.ReadToEnd() | Add-Content -LiteralPath {target} -Encoding utf8"
        )
    }
    #[cfg(not(windows))]
    {
        match sink {
            Sink::Shared => sh_append(&codex_ping_path()),
            Sink::PerConsole => sh_append(&per_console_target()),
        }
    }
}

/// Perfil de Codex con los hooks de Atic (`codex -p <perfil>`): una capa sobre
/// la configuración del usuario, que no se toca. Solo pesa en las consolas
/// que lo usan. Devuelve el nombre del perfil. `codex_home` es `CODEX_HOME`
/// o `~/.codex`.
pub fn codex_profile(sink: Sink, codex_home: &Path) -> Option<&'static str> {
    if !codex_home.is_dir() {
        return None;
    }
    let name = match sink {
        Sink::Shared => "atic",
        Sink::PerConsole => "atic-pill",
    };
    // Una cadena JSON es una cadena básica de TOML válida.
    let command = serde_json::to_string(&codex_hook_command(sink)).ok()?;
    let hook = |matcher: Option<&str>| {
        let matcher = matcher.map(|m| format!("matcher = \"{m}\", ")).unwrap_or_default();
        format!("[{{ {matcher}hooks = [{{ type = \"command\", command = {command} }}] }}]")
    };
    // Sin `PostToolUse` para todas las herramientas: cada hook es un PowerShell
    // que arranca, y Codex corre muchos comandos. Un permiso contestado en el
    // PC se borra con el `Stop` del turno.
    let body = format!(
        "# Lo escribe Atic para sus consolas (`codex -p {name}`). Se regenera solo.\n\
         [hooks]\n\
         PermissionRequest = {}\n\
         PreToolUse = {}\n\
         PostToolUse = {}\n\
         UserPromptSubmit = {}\n\
         Stop = {}\n",
        hook(None),
        hook(Some("request_user_input")),
        hook(Some("request_user_input")),
        hook(None),
        hook(None),
    );
    std::fs::write(codex_home.join(format!("{name}.config.toml")), body).ok()?;
    Some(name)
}

/// `CODEX_HOME`, o `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(|home| PathBuf::from(home).join(".codex"))
    })
}

/// Config de Kimi para sus consolas: la del usuario, copiada tal cual, más los
/// hooks de Atic. Kimi no mezcla configs (`--config-file` reemplaza la suya),
/// por eso se copia en cada lanzamiento. Va en `~/.kimi`, junto a la original.
/// `None` si el usuario ya tiene hooks propios escritos de otra forma: mejor
/// sin aviso que pisarle la config.
pub fn kimi_config() -> Option<PathBuf> {
    let dir = PathBuf::from(std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?).join(".kimi");
    let base = std::fs::read_to_string(dir.join("config.toml")).ok()?;
    let config = with_kimi_hooks(&base, &kimi_hook_command())?;
    let path = dir.join("atic-config.toml");
    std::fs::write(&path, config).ok()?;
    Some(path)
}

/// Kimi corre los hooks con `cmd`. `findstr "^"` copia stdin tal cual (UTF-8
/// incluido) y termina la línea; cada consola anota en su propio archivo.
fn kimi_hook_command() -> String {
    let dir = std::env::temp_dir();
    format!("findstr \"^\" >> \"{}\\{KIMI_PREFIX}%{CONSOLE_TOKEN_VAR}%.jsonl\"", dir.display())
}

fn with_kimi_hooks(base: &str, command: &str) -> Option<String> {
    // Literal de TOML: el comando lleva comillas dobles pero no simples.
    if command.contains('\'') {
        return None;
    }
    let hook = |event: &str, matcher: &str| {
        let matcher = if matcher.is_empty() { String::new() } else { format!(", matcher = \"{matcher}\"") };
        format!("{{ event = \"{event}\"{matcher}, command = '{command}' }}")
    };
    let hooks = format!(
        "hooks = [{}, {}, {}, {}]",
        hook("PreToolUse", "AskUserQuestion"),
        hook("PostToolUse", "AskUserQuestion"),
        hook("UserPromptSubmit", ""),
        hook("Stop", ""),
    );
    if base.lines().any(|l| l.trim() == "hooks = []") {
        let lines: Vec<String> =
            base.lines().map(|l| if l.trim() == "hooks = []" { hooks.clone() } else { l.to_string() }).collect();
        return Some(lines.join("\n") + "\n");
    }
    if base.lines().any(|l| l.trim_start().starts_with("hooks") || l.trim_start().starts_with("[[hooks")) {
        return None;
    }
    // Las claves de arriba van antes de cualquier tabla.
    Some(format!("{hooks}\n{base}"))
}

/// Los hooks de Claude en un archivo, para lanzar `claude --settings
/// <archivo>` sin tocar el `settings.json` del usuario.
pub fn console_settings_path(sink: Sink) -> Option<PathBuf> {
    let name = match sink {
        Sink::Shared => "atic-claude-hooks.json",
        Sink::PerConsole => "atic-pill-claude-hooks.json",
    };
    let path = std::env::temp_dir().join(name);
    std::fs::write(&path, hook_settings(sink)).ok()?;
    Some(path)
}

/// Una línea con sintaxis de shell no se reescribe: no se sabe dónde meter
/// las opciones sin romperla.
pub fn has_shell_syntax(line: &str) -> bool {
    line.contains([
        '|', '&', ';', '<', '>', '$', '`', '(', ')', '*', '?', '~', '"', '\'',
    ])
}

fn program_is(command: &str, name: &str) -> bool {
    let first = command.split_whitespace().next().unwrap_or("");
    first.eq_ignore_ascii_case(name) || first.eq_ignore_ascii_case(&format!("{name}.exe"))
}

/// `claude …` con `--settings <archivo>` (los hooks). Cualquier otra línea,
/// tal cual.
pub fn with_claude_hooks(command: &str, sink: Sink) -> String {
    if !program_is(command, "claude") || has_shell_syntax(command) || command.contains("--settings") {
        return command.to_string();
    }
    match console_settings_path(sink) {
        // Sin espacios en la ruta: la línea se parte por espacios.
        Some(path) if !path.to_string_lossy().contains(' ') => {
            format!("{command} --settings {}", path.display())
        }
        _ => command.to_string(),
    }
}

/// `codex …` con el perfil de Atic. Los hooks que no vienen del config del
/// usuario piden confianza en `/hooks`; el flag la da solo a esta corrida.
pub fn with_codex_hooks(command: &str, sink: Sink) -> String {
    let mut parts = command.splitn(2, char::is_whitespace);
    let first = parts.next().unwrap_or("");
    let rest = parts.next().unwrap_or("").trim();
    let has_profile = rest.split_whitespace().any(|a| a == "-p" || a.starts_with("--profile"));
    if !program_is(command, "codex") || has_shell_syntax(command) || has_profile {
        return command.to_string();
    }
    let Some(profile) = codex_home().and_then(|home| codex_profile(sink, &home)) else {
        return command.to_string();
    };
    // Las opciones globales van antes de un subcomando (`codex resume <id>`).
    let flags = format!("-p {profile} --dangerously-bypass-hook-trust");
    if rest.is_empty() {
        format!("{first} {flags}")
    } else {
        format!("{first} {flags} {rest}")
    }
}

/// Qué dice un hook sobre el estado de su sesión.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookStatus {
    /// Espera al usuario: un permiso o una pregunta.
    Waiting,
    /// Terminó el turno.
    Ready,
    /// Volvió a trabajar.
    Working,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookPing {
    pub session_id: String,
    pub cwd: String,
    pub status: HookStatus,
    pub preview: Option<String>,
}

pub fn classify_hook(v: &Value) -> Option<HookPing> {
    let session_id = v
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let cwd = v.get("cwd").and_then(Value::as_str).unwrap_or("").to_string();
    let event = v.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
    let ntype = v.get("notification_type").and_then(Value::as_str).unwrap_or("");
    let status = match event {
        "PermissionRequest" => HookStatus::Waiting,
        "Notification" if matches!(ntype, "permission_prompt" | "idle_prompt" | "agent_needs_input") => {
            HookStatus::Waiting
        }
        "Stop" => HookStatus::Ready,
        // Contestó la pregunta o mandó otro mensaje: vuelve a trabajar.
        "PostToolUse" | "UserPromptSubmit" => HookStatus::Working,
        _ => return None,
    };
    let preview = v
        .get("last_assistant_message")
        .and_then(Value::as_str)
        .or_else(|| v.get("message").and_then(Value::as_str))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().take(120).collect());
    Some(HookPing { session_id, cwd, status, preview })
}

/// Lee lo que anotaron los hooks desde la última vez, archivo por archivo.
#[derive(Default)]
pub struct Drain {
    offsets: HashMap<PathBuf, u64>,
}

impl Drain {
    /// Las líneas nuevas de `path`.
    pub fn file(&mut self, path: &Path, mut f: impl FnMut(&Value)) {
        let offset = self.offsets.entry(path.to_path_buf()).or_insert(0);
        drain_file(path, offset, &mut f);
    }

    /// Las líneas nuevas de cada `<prefix><marca>.jsonl` de `dir`, con su
    /// marca.
    pub fn prefixed(&mut self, dir: &Path, prefix: &str, mut f: impl FnMut(&str, &Value)) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(token) = name.strip_prefix(prefix).and_then(|t| t.strip_suffix(".jsonl")) else {
                continue;
            };
            let offset = self.offsets.entry(entry.path()).or_insert(0);
            drain_file(&entry.path(), offset, &mut |v| f(token, v));
        }
    }
}

fn drain_file(path: &Path, offset: &mut u64, f: &mut dyn FnMut(&Value)) {
    let Ok(mut file) = OpenOptions::new().read(true).open(path) else {
        return;
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return;
    };
    if len < *offset {
        *offset = 0;
    }
    let start = (*offset).min(len);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return;
    }
    let reader = BufReader::new(file);
    let mut consumed = start;
    for line in reader.lines() {
        let Ok(line) = line else {
            break;
        };
        consumed += line.len() as u64 + 1;
        // PowerShell abre el archivo con BOM.
        if let Ok(v) = serde_json::from_str::<Value>(line.trim_start_matches('\u{feff}')) {
            f(&v);
        }
    }
    *offset = consumed.min(len);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn permission_request_es_waiting() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "cwd": "/x",
            "hook_event_name": "PermissionRequest"
        }))
        .unwrap();
        assert_eq!(ping.status, HookStatus::Waiting);
        assert_eq!(ping.session_id, "s1");
    }

    #[test]
    fn notification_permiso_es_waiting() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "message": "Claude needs your permission to use Bash"
        }))
        .unwrap();
        assert_eq!(ping.status, HookStatus::Waiting);
        assert_eq!(ping.preview.as_deref(), Some("Claude needs your permission to use Bash"));
    }

    #[test]
    fn stop_es_ready() {
        let ping = classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Stop",
            "last_assistant_message": "listo"
        }))
        .unwrap();
        assert_eq!(ping.status, HookStatus::Ready);
        assert_eq!(ping.preview.as_deref(), Some("listo"));
    }

    #[test]
    fn notification_irrelevante_se_ignora() {
        assert!(classify_hook(&json!({
            "session_id": "s1",
            "hook_event_name": "Notification",
            "notification_type": "auth_success"
        }))
        .is_none());
    }

    #[test]
    fn la_config_de_kimi_suma_los_hooks_sin_pisar_la_del_usuario() {
        let base = "default_yolo = false\nhooks = []\ntheme = \"dark\"\n\n[loop_control]\nmax = 1\n";
        let out = with_kimi_hooks(base, "findstr \"^\" >> \"C:\\t\\x.jsonl\"").unwrap();
        assert!(out.contains("event = \"PreToolUse\", matcher = \"AskUserQuestion\""), "{out}");
        assert!(out.contains("theme = \"dark\"") && out.contains("[loop_control]"), "{out}");
        assert!(!out.contains("hooks = []"), "{out}");
        // Sin `hooks`, van arriba de las tablas.
        let out = with_kimi_hooks("[loop_control]\nmax = 1\n", "c").unwrap();
        assert!(out.starts_with("hooks = ["), "{out}");
        // Hooks propios escritos de otra forma: no se toca.
        assert!(with_kimi_hooks("[[hooks]]\nevent = \"Stop\"\ncommand = \"x\"\n", "c").is_none());
    }

    #[test]
    fn el_hook_de_codex_es_powershell_en_utf8() {
        let command = codex_hook_command(Sink::Shared);
        #[cfg(windows)]
        {
            assert!(command.contains("UTF8Encoding"), "{command}");
            assert!(command.contains("Add-Content"), "{command}");
        }
        assert!(command.contains("atic-codex-ping.jsonl"), "{command}");
    }

    #[test]
    fn el_hook_de_codex_de_la_pill_usa_la_marca() {
        let command = codex_hook_command(Sink::PerConsole);
        assert!(command.contains(PILL_PREFIX), "{command}");
        assert!(command.contains(CONSOLE_TOKEN_VAR), "{command}");
    }

    #[test]
    fn snippet_es_json_con_hooks() {
        let v: Value = serde_json::from_str(&hook_settings(Sink::Shared)).unwrap();
        assert!(v.get("hooks").and_then(|h| h.get("Stop")).is_some());
        assert!(v.pointer("/hooks/PermissionRequest").and_then(Value::as_array).is_some());
    }

    #[test]
    fn el_comando_del_hook_usa_el_shell_de_su_so() {
        let command = hook_command(Sink::Shared);
        assert!(command.starts_with("sh -c "), "{command}");
        let ruta = ping_path().to_string_lossy().replace('\\', "/");
        assert!(command.contains(&ruta), "{command}");
        assert!(!command.contains("powershell"), "{command}");
    }

    #[test]
    fn el_hook_de_claude_de_la_pill_escribe_en_el_archivo_de_su_consola() {
        let command = hook_command(Sink::PerConsole);
        assert!(command.starts_with("sh -c "), "{command}");
        assert!(command.contains(&format!("{PILL_PREFIX}${CONSOLE_TOKEN_VAR}.jsonl")), "{command}");
    }

    #[test]
    fn solo_se_reescriben_los_comandos_simples_de_su_cli() {
        assert_eq!(with_claude_hooks("codex", Sink::PerConsole), "codex");
        assert_eq!(with_claude_hooks("claude | tee x", Sink::PerConsole), "claude | tee x");
        assert_eq!(with_claude_hooks("claude --settings x", Sink::PerConsole), "claude --settings x");
        assert_eq!(with_codex_hooks("codex -p otro", Sink::PerConsole), "codex -p otro");
        assert_eq!(with_codex_hooks("claude", Sink::PerConsole), "claude");
    }

    #[test]
    fn drain_lee_solo_lo_nuevo_y_da_la_marca() {
        let dir = std::env::temp_dir().join(format!("atic-agents-drain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{PILL_PREFIX}tok1.jsonl"));
        std::fs::write(&path, "\u{feff}{\"a\":1}\nno es json\n").unwrap();
        let mut drain = Drain::default();
        let mut seen = Vec::new();
        drain.prefixed(&dir, PILL_PREFIX, |token, v| seen.push((token.to_string(), v.clone())));
        assert_eq!(seen, vec![("tok1".to_string(), json!({"a": 1}))]);
        std::fs::write(&path, "\u{feff}{\"a\":1}\nno es json\n{\"b\":2}\n").unwrap();
        seen.clear();
        drain.prefixed(&dir, PILL_PREFIX, |token, v| seen.push((token.to_string(), v.clone())));
        assert_eq!(seen, vec![("tok1".to_string(), json!({"b": 2}))]);
        let _ = std::fs::remove_dir_all(dir);
    }
}
