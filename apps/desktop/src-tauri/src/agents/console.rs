//! Consola embebida: PTY local (PowerShell/cmd) o SSH interactivo (`ssh -t`).
//!
//! I/O bidireccional vía eventos Tauri (`console-output` / `console-exit`).
//! N sesiones concurrentes, cada una con su propio PTY y su id: abrir otra
//! `local` ya no reemplaza a la anterior. El tope es defensivo, no de diseño
//! (cada sesión es un proceso vivo); quien las presenta decide cómo agruparlas.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use atic_core::{MutexExt, SshHost};

use crate::state::AppState;

use super::ssh::{self, AskpassGuard};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOutputPayload {
    pub session: String,
    /// Chunk UTF-8 (lossy); secuencias ANSI viajan como ASCII.
    pub data: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleExitPayload {
    pub session: String,
    pub code: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOpenOptions {
    /// `local` | `ssh`
    pub kind: String,
    pub host_id: Option<String>,
    pub cwd: Option<String>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
    /// Comando a ejecutar en la PTY local (`claude`, `opencode…`).
    /// Vacío/ausente = shell del sistema, como siempre.
    pub command: Option<String>,
    /// Vista que la abre. La reclama desde el arranque: sin esto, entre el
    /// `open` y el `attach` hay un hueco en el que el barrido la vería sola.
    pub view: Option<String>,
}

struct LiveConsole {
    writer: Mutex<Box<dyn Write + Send>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    stop: Arc<AtomicBool>,
    /// PID del proceso raíz del PTY (`cmd /K` o la shell). 0 = desconocido.
    pid: u32,
    _askpass: Option<AskpassGuard>,
}

/// Tope defensivo de sesiones vivas. No es una regla de producto: es que cada
/// una es un PTY con su proceso, y un bug de la vista no debería poder
/// spawnear shells sin freno.
const MAX_CONSOLES: usize = 12;

static CONSOLES: Mutex<Option<HashMap<String, LiveConsole>>> = Mutex::new(None);

/// Cola de traspaso entre ventanas (float ⇄ principal).
///
/// Mudar una consola viva de una webview a otra tiene una ventana donde
/// ninguna vista la reclama: la emisora ya soltó y la receptora todavía no
/// adoptó. Sin esto, un `console_gc` o un `console_close` rezagado en esa
/// ventana mata el PTY en pleno vuelo. Llevar timestamp acota la fuga si la
/// receptora nunca confirma: pasado el TTL, valen las reglas normales.
static TRANSFERS: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Cuánto dura la protección de una sesión en vuelo.
const TRANSFER_TTL: Duration = Duration::from_secs(90);

/// Qué vistas están mostrando cada sesión.
///
/// El GC no puede preguntarle a UNA vista qué sesiones existen. Con la isla y
/// el float (o la ventana dedicada) vivos a la vez, la lista de una es el
/// "huérfano" de la otra: `console_gc(keep)` mataba las PTY de la vecina en
/// cuanto caducaba la marca de traspaso. Acá cada vista declara lo que
/// muestra y el GC solo toca lo que no reclama nadie.
///
/// `orphan_since` es la otra mitad: entre desmontar una vista y montar la
/// siguiente hay un hueco de milisegundos que no es orfandad. Se cuenta desde
/// que la sesión se queda sin vistas, no desde que el GC la mira.
#[derive(Default)]
struct Claims {
    /// sesión → vista → último latido de esa vista.
    ///
    /// Lleva reloj porque una vista puede morir **sin desmontarse**: una
    /// recarga del overlay (HMR en desarrollo) se lleva el webview sin correr
    /// ningún `onDestroy`. Sin caducidad, ese reclamo fantasma mantendría viva
    /// para siempre una PTY que ya no mira nadie — justo lo que el barrido
    /// existe para evitar.
    by_session: HashMap<String, HashMap<String, Instant>>,
    orphan_since: HashMap<String, Instant>,
}

static CLAIMS: Mutex<Option<Claims>> = Mutex::new(None);

/// Cuánto aguanta viva una sesión que no muestra ninguna vista.
///
/// Cubre el desmontar/montar de un despegue, una recarga del overlay en
/// desarrollo y el arranque de la app antes de que la UI reclame lo suyo.
const ORPHAN_GRACE: Duration = Duration::from_secs(30);

/// Cada cuánto barre el hilo de limpieza.
const REAP_EVERY: Duration = Duration::from_secs(20);

/// Cuánto vale un reclamo sin latido. La vista late cada 10 s: cuatro
/// perdidos es una vista muerta, no una lenta.
const CLAIM_TTL: Duration = Duration::from_secs(45);

fn with_claims<T>(f: impl FnOnce(&mut Claims) -> T) -> T {
    let mut guard = CLAIMS.lock_or_recover();
    let claims = guard.get_or_insert_with(Claims::default);
    f(claims)
}

/// Una vista muestra la sesión (o confirma que la sigue mostrando).
fn claim(session: &str, view: &str) {
    with_claims(|c| {
        c.by_session
            .entry(session.to_string())
            .or_default()
            .insert(view.to_string(), Instant::now());
        c.orphan_since.remove(session);
    });
}

/// Una vista deja de mostrarla. Sin vistas, arranca el reloj de la gracia.
fn unclaim(session: &str, view: &str) {
    with_claims(|c| {
        let vacia = match c.by_session.get_mut(session) {
            Some(vistas) => {
                vistas.remove(view);
                vistas.is_empty()
            }
            None => true,
        };
        if vacia {
            c.by_session.remove(session);
            c.orphan_since
                .entry(session.to_string())
                .or_insert_with(Instant::now);
        }
    });
}

/// La sesión se fue (cerrada o muerta): fuera del registro.
fn forget_claims(session: &str) {
    with_claims(|c| {
        c.by_session.remove(session);
        c.orphan_since.remove(session);
    });
}

/// ¿La reclama alguna vista **viva**? Los reclamos vencidos se tiran acá.
fn claimed(session: &str) -> bool {
    with_claims(|c| {
        let vivas = match c.by_session.get_mut(session) {
            Some(vistas) => {
                vistas.retain(|_, visto| visto.elapsed() < CLAIM_TTL);
                !vistas.is_empty()
            }
            None => return false,
        };
        if !vivas {
            c.by_session.remove(session);
            c.orphan_since
                .entry(session.to_string())
                .or_insert_with(Instant::now);
        }
        vivas
    })
}

/// ¿Lleva sin ninguna vista más que la gracia?
///
/// La primera vez que se ve sin dueño se anota y se la deja pasar: una sesión
/// recién abierta (o que nadie reclamó todavía) no es basura.
fn orphan_ready(session: &str) -> bool {
    with_claims(|c| {
        if c.by_session.contains_key(session) {
            return false;
        }

        let desde = c
            .orphan_since
            .entry(session.to_string())
            .or_insert_with(Instant::now);
        desde.elapsed() >= ORPHAN_GRACE
    })
}

/// Cola de scrollback por sesión: los últimos bytes emitidos.
///
/// Repintar un xterm recién montado (mudar de ventana) sin esto deja el
/// terminal vacío aunque el PTY siga vivo. El tope iguala al buffer del
/// frontend (`OUTPUT_BUF_MAX`): misma ventana de historia en los dos lados.
const TAIL_MAX: usize = 256 * 1024;

static TAILS: Mutex<Option<HashMap<String, Arc<Mutex<VecDeque<u8>>>>>> = Mutex::new(None);

fn with_tails<T>(f: impl FnOnce(&mut HashMap<String, Arc<Mutex<VecDeque<u8>>>>) -> T) -> T {
    let mut guard = TAILS.lock_or_recover();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

/// Guarda los últimos bytes; lo viejo cae por delante.
fn tail_push(buf: &mut VecDeque<u8>, bytes: &[u8]) {
    buf.extend(bytes.iter().copied());
    let sobran = buf.len().saturating_sub(TAIL_MAX);
    if sobran > 0 {
        buf.drain(..sobran);
    }
}

fn transfer_guarded(id: &str) -> bool {
    let mut guard = TRANSFERS.lock_or_recover();
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(id) {
        Some(at) if at.elapsed() < TRANSFER_TTL => true,
        Some(_) => {
            map.remove(id);
            false
        }
        None => false,
    }
}

fn with_map<T>(f: impl FnOnce(&mut HashMap<String, LiveConsole>) -> T) -> T {
    let mut guard = CONSOLES.lock_or_recover();
    let map = guard.get_or_insert_with(HashMap::new);
    f(map)
}

fn pty_size(cols: Option<u16>, rows: Option<u16>) -> PtySize {
    PtySize {
        rows: rows.unwrap_or(24).max(2),
        cols: cols.unwrap_or(80).max(2),
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// La shell del usuario.
///
/// Una app abierta desde Finder no hereda `SHELL`; el fallback histórico a
/// `/bin/bash` abría una shell sin el perfil del usuario en macOS, donde el
/// default del sistema es zsh. `SHELL` solo se usa si el archivo existe: una
/// variable apuntando a una shell desinstalada rompía el arranque del PTY.
#[cfg(not(windows))]
fn user_shell() -> PathBuf {
    if let Some(shell) = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| shell.is_file())
    {
        return shell;
    }
    #[cfg(target_os = "macos")]
    {
        let zsh = PathBuf::from("/bin/zsh");
        if zsh.is_file() {
            return zsh;
        }
    }
    PathBuf::from("/bin/bash")
}

fn resolve_local_shell() -> CommandBuilder {
    #[cfg(windows)]
    {
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ps = PathBuf::from(&sysroot).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps.is_file() {
            let mut cmd = CommandBuilder::new(&ps);
            cmd.arg("-NoLogo");
            return cmd;
        }
        CommandBuilder::new(system_cmd_exe())
    }
    #[cfg(not(windows))]
    {
        CommandBuilder::new(user_shell())
    }
}

fn apply_cwd(cmd: &mut CommandBuilder, cwd: Option<&str>) {
    let Some(dir) = cwd.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    if Path::new(dir).is_dir() {
        cmd.cwd(dir);
    }
}

/// La consola embebida sí entiende ANSI/truecolor aunque Atic haya sido
/// lanzado desde un proceso que exporta `TERM=dumb` o `NO_COLOR=1`.
/// Limitar el override al hijo PTY: no alterar el entorno global de la app.
fn apply_terminal_color_env(cmd: &mut CommandBuilder) {
    cmd.env_remove("NO_COLOR");
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env("CLICOLOR", "1");
    cmd.env("CLICOLOR_FORCE", "1");
    cmd.env("FORCE_COLOR", "1");
}

/// ¿Variable de config de scripts npm/pnpm que el hijo no debe heredar?
///
/// Bajo `pnpm dev` el padre exporta `npm_config_*` y compañía; si la consola
/// las hereda, un `npm install -g` instala dentro del proyecto en vez del
/// prefix global. Case-insensitive: npm en Windows también lee `NPM_CONFIG_*`.
fn is_script_env_var(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("npm_config_")
        || lower.starts_with("npm_lifecycle_")
        || lower.starts_with("npm_package_")
        || lower == "npm_execpath"
        || lower == "pnpm_script_src_dir"
        || lower == "node_run_script_name"
}

/// Quita del hijo la config de scripts npm/pnpm heredada del proceso padre:
/// la consola debe comportarse como una terminal recién abierta. No toca PATH
/// ni el resto del entorno.
fn apply_clean_script_env(cmd: &mut CommandBuilder) {
    for (name, _) in std::env::vars_os() {
        if let Some(name) = name.to_str() {
            if is_script_env_var(name) {
                cmd.env_remove(name);
            }
        }
    }
}

/// El hijo ve el PATH fresco (proceso + registro en Windows, proceso +
/// carpetas de usuario en macOS): un CLI recién instalado resuelve en una
/// consola nueva sin reiniciar Atic.
///
/// Delante va la carpeta de los comandos Unix, que Atic trae consigo: es lo
/// que hace que `ls` funcione en una consola de Windows sin que el usuario
/// instale nada. Va primero a propósito —si el sistema ya tiene un `ls`, el
/// nuestro manda—, y si falla preparar los enlaces la consola abre igual: sin
/// `ls`, pero abre.
fn apply_fresh_path(cmd: &mut CommandBuilder, dir_datos: Option<&std::path::Path>) {
    let unix = dir_datos.and_then(|dir| match super::unix_tools::preparar(dir) {
        Ok(bin) => Some(bin),
        Err(e) => {
            tracing::warn!(error = %e, "sin comandos Unix en la consola");
            None
        }
    });
    // `join_paths` arma el separador del sistema. Armarlo a mano con `;` dejaba
    // el PATH roto en macOS (`a;b:c` es una sola ruta inexistente) y la consola
    // no resolvía ni un comando.
    let base = super::exe::merged_path_var().or_else(|| std::env::var_os("PATH"));
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(unix) = unix {
        dirs.push(unix);
    }
    if let Some(base) = base {
        dirs.extend(std::env::split_paths(&base));
    }
    if dirs.is_empty() {
        return;
    }
    match std::env::join_paths(dirs) {
        Ok(valor) => {
            cmd.env("PATH", valor);
        }
        Err(e) => tracing::warn!(error = %e, "no se pudo armar el PATH de la consola"),
    }
}

/// El `cmd.exe` de Windows, por ruta absoluta.
///
/// Por nombre pelado la PTY lo busca en el PATH probando `PATHEXT`, y un
/// paquete de npm que instale un bin llamado `cmd` deja un `cmd.cmd` en la
/// carpeta de npm: eso es lo que se lanzaba en vez de la shell, y el CLI de
/// Node moría con «too many arguments» antes de que el agente arrancara.
#[cfg(windows)]
fn system_cmd_exe() -> PathBuf {
    let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let cmd_exe = PathBuf::from(&sysroot).join(r"System32\cmd.exe");
    if cmd_exe.is_file() {
        return cmd_exe;
    }
    std::env::var_os("COMSPEC")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .unwrap_or(cmd_exe)
}

fn quote_cmd(s: &str) -> String {
    if s.bytes()
        .any(|b| b.is_ascii_whitespace() || matches!(b, b'"' | b'&' | b'^' | b'%'))
    {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

/// Comando de agente CLI (`claude`, `opencode`…) dentro de una shell que
/// sobrevive.
///
/// Si el PTY *es* el CLI, al salir —o si el shim de npm arranca y se despega—
/// la pestaña muere y parece que “se cerró”. `cmd /K` espera al TUI y, si el
/// proceso termina, deja el prompt.
fn build_local_command(command: &str) -> Result<CommandBuilder, String> {
    // Sintaxis de shell (pipe, redirección, comillas, encadenados…): partir la
    // línea por espacios y lanzar el primer token directo le pasa `|`, `>`
    // y compañía como argumentos al ejecutable. El instalador de Grok moría en
    // `curl … | bash` con `|` y `bash` interpretados como URLs. Esas líneas van
    // enteras a la shell del usuario, que es quien sabe interpretarlas.
    if has_shell_syntax(command) {
        return Ok(build_shell_line(command));
    }
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| "Comando de consola vacío.".to_string())?;
    let extra: Vec<&str> = parts.collect();
    let Some((exe, prefix)) = super::exe::launcher(program) else {
        // No está en el PATH: puede ser una función o alias del perfil del
        // usuario (p. ej. `dashboard`). La línea completa corre dentro de su
        // shell, que es quien la conoce.
        return Ok(build_shell_line(command));
    };

    #[cfg(windows)]
    {
        let invoked = if prefix.len() >= 2 && prefix[0].eq_ignore_ascii_case("/C") {
            prefix[1].clone()
        } else {
            exe.display().to_string()
        };
        let mut line = quote_cmd(&invoked);
        for arg in extra {
            line.push(' ');
            line.push_str(&quote_cmd(arg));
        }
        let mut cmd = CommandBuilder::new(system_cmd_exe());
        cmd.arg("/K");
        cmd.arg(line);
        Ok(cmd)
    }

    #[cfg(not(windows))]
    {
        let mut cmd = CommandBuilder::new(exe);
        cmd.args(&prefix);
        cmd.args(extra);
        Ok(cmd)
    }
}

/// ¿La línea necesita una shell para interpretarse?
///
/// Pipes, redirecciones, encadenados, variables, globs y comillas: nada de eso
/// lo entiende `Command`; solo el intérprete. Las líneas de instalación de los
/// agentes caen acá por el `|` (y las de Windows también por las comillas).
fn has_shell_syntax(line: &str) -> bool {
    line.contains([
        '|', '&', ';', '<', '>', '$', '`', '(', ')', '*', '?', '~', '"', '\'',
    ])
}

/// Línea arbitraria dentro de la shell del usuario, con el prompt vivo al
/// terminar (PowerShell carga el perfil, así los alias/funciones existen).
fn build_shell_line(line: &str) -> CommandBuilder {
    #[cfg(windows)]
    {
        let sysroot = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let ps = PathBuf::from(&sysroot).join(r"System32\WindowsPowerShell\v1.0\powershell.exe");
        if ps.is_file() {
            let mut cmd = CommandBuilder::new(&ps);
            cmd.arg("-NoLogo");
            cmd.arg("-NoExit");
            cmd.arg("-Command");
            cmd.arg(line);
            return cmd;
        }
        let mut cmd = CommandBuilder::new(system_cmd_exe());
        cmd.arg("/K");
        cmd.arg(line);
        cmd
    }
    #[cfg(not(windows))]
    {
        let mut cmd = CommandBuilder::new(user_shell());
        cmd.arg("-ic");
        cmd.arg(line);
        cmd
    }
}

fn build_ssh_builder(host: &SshHost) -> Result<(CommandBuilder, Option<AskpassGuard>), String> {
    let program = ssh::ensure_ssh_program()?;
    let args = ssh::ssh_interactive_args(host)?;
    let mut cmd = CommandBuilder::new(&program);
    cmd.args(&args);

    let (guard, script, pass) = ssh::prepare_askpass(host)?;
    if let (Some(script), Some(pass)) = (script, pass) {
        cmd.env("SSH_ASKPASS", &script);
        cmd.env("SSH_ASKPASS_REQUIRE", "force");
        cmd.env("ATIC_SSH_PASSPHRASE", &pass);
        if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
            cmd.env("DISPLAY", ":0");
        }
    }

    // Importante: no CREATE_NO_WINDOW — ConPTY necesita consola real.
    Ok((cmd, guard))
}

fn spawn_reader(
    app: AppHandle,
    session: String,
    mut reader: Box<dyn Read + Send>,
    stop: Arc<AtomicBool>,
    tail: Arc<Mutex<VecDeque<u8>>>,
) {
    thread::Builder::new()
        .name(format!("console-read-{session}"))
        .spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                        if let Ok(mut cola) = tail.lock() {
                            tail_push(&mut cola, data.as_bytes());
                        }
                        let _ = app.emit(
                            "console-output",
                            ConsoleOutputPayload {
                                session: session.clone(),
                                data,
                            },
                        );
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(8));
                        continue;
                    }
                    Err(_) => break,
                }
            }
        })
        .ok();
}

fn spawn_wait(
    app: AppHandle,
    session: String,
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    stop: Arc<AtomicBool>,
) {
    thread::Builder::new()
        .name(format!("console-wait-{session}"))
        .spawn(move || {
            let code = match child.wait() {
                Ok(status) => Some(status.exit_code()),
                Err(_) => None,
            };
            stop.store(true, Ordering::Relaxed);
            // Quitar del mapa si sigue siendo esta sesión.
            with_map(|map| {
                map.remove(&session);
            });
            with_tails(|tails| {
                tails.remove(&session);
            });
            forget_claims(&session);
            let _ = app.emit("console-exit", ConsoleExitPayload { session, code });
        })
        .ok();
}

fn close_session(id: &str) {
    forget_claims(id);
    let taken = with_map(|map| map.remove(id));
    with_tails(|tails| {
        tails.remove(id);
    });
    if let Some(mut live) = taken {
        live.stop.store(true, Ordering::Relaxed);
        let _ = live.killer.kill();
        // Dropear writer/master/askpass.
    }
}

/// Cierra todas las consolas (apagado de la app).
pub fn close_all() {
    let ids: Vec<String> = with_map(|map| map.keys().cloned().collect());
    for id in ids {
        close_session(&id);
    }
}

#[tauri::command]
pub fn console_open(
    app: AppHandle,
    state: State<'_, AppState>,
    options: ConsoleOpenOptions,
) -> Result<String, String> {
    let kind = match options.kind.as_str() {
        "local" | "ssh" => options.kind.clone(),
        other => {
            return Err(format!(
                "Tipo de consola desconocido: {other}. Usa local o ssh."
            ));
        }
    };

    // Antes se cerraba la sesión que compartiera `kind`: abrir una segunda
    // consola local mataba la primera. Ahora conviven; cerrar es explícito.
    let live_count = with_map(|map| map.len());
    if live_count >= MAX_CONSOLES {
        return Err(format!(
            "Ya hay {MAX_CONSOLES} consolas abiertas. Cierra alguna para abrir otra."
        ));
    }

    let size = pty_size(options.cols, options.rows);
    let (mut cmd, askpass) = match kind.as_str() {
        "local" => {
            let mut cmd = match options.command.as_deref().map(str::trim) {
                Some(c) if !c.is_empty() => build_local_command(c)?,
                _ => resolve_local_shell(),
            };
            apply_cwd(&mut cmd, options.cwd.as_deref());
            (cmd, None)
        }
        "ssh" => {
            let host_id = options
                .host_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "Falta host SSH para la consola remota.".to_string())?;
            let host = state
                .config
                .lock_or_recover()
                .ssh_hosts
                .iter()
                .find(|h| h.id == host_id)
                .cloned()
                .ok_or_else(|| format!("Host SSH no encontrado: {host_id}"))?;
            build_ssh_builder(&host)?
        }
        _ => unreachable!("kind ya validado"),
    };
    apply_terminal_color_env(&mut cmd);
    apply_clean_script_env(&mut cmd);
    apply_fresh_path(&mut cmd, Some(&state.dirs.data_dir()));

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(size)
        .map_err(|e| format!("No se pudo abrir PTY: {e}"))?;

    let child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("No se pudo spawnear la shell: {e}"))?;

    // Liberar slave explícitamente (buena práctica en Windows ConPTY).
    drop(pair.slave);

    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("No se pudo leer el PTY: {e}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|e| format!("No se pudo escribir al PTY: {e}"))?;

    let session = Uuid::new_v4().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let killer = child.clone_killer();
    let pid = child.process_id().unwrap_or(0);

    let tail = Arc::new(Mutex::new(VecDeque::new()));
    with_tails(|tails| {
        tails.insert(session.clone(), Arc::clone(&tail));
    });
    spawn_reader(
        app.clone(),
        session.clone(),
        reader,
        Arc::clone(&stop),
        tail,
    );
    spawn_wait(app, session.clone(), child, Arc::clone(&stop));

    with_map(|map| {
        map.insert(
            session.clone(),
            LiveConsole {
                writer: Mutex::new(writer),
                master: pair.master,
                killer,
                stop,
                pid,
                _askpass: askpass,
            },
        );
    });
    if let Some(view) = options
        .view
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        claim(&session, view);
    }
    spawn_reaper();

    Ok(session)
}

#[tauri::command]
pub fn console_write(session: String, data: String) -> Result<(), String> {
    with_map(|map| {
        let live = map
            .get(&session)
            .ok_or_else(|| "esa consola ya no existe".to_string())?;
        let mut w = live
            .writer
            .lock()
            .map_err(|_| "lock del writer de consola".to_string())?;
        w.write_all(data.as_bytes())
            .map_err(|e| format!("escritura PTY: {e}"))?;
        w.flush().map_err(|e| format!("flush PTY: {e}"))?;
        Ok(())
    })
}

#[tauri::command]
pub fn console_resize(session: String, cols: u16, rows: u16) -> Result<(), String> {
    with_map(|map| {
        let live = map
            .get(&session)
            .ok_or_else(|| "esa consola ya no existe".to_string())?;
        live.master
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(2),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("resize PTY: {e}"))
    })
}

#[tauri::command]
pub fn console_close(session: String) -> Result<(), String> {
    // En vuelo a otra ventana: la emisora suelta sin matar; la receptora ya
    // adoptó o está por adoptar. Se consume la marca para no blindar de más.
    let mut guard = TRANSFERS.lock_or_recover();
    if let Some(map) = guard.as_mut() {
        if map.remove(&session).is_some() {
            return Ok(());
        }
    }
    drop(guard);
    close_session(&session);
    Ok(())
}

/// Mata las PTY que no muestra ninguna vista.
///
/// Ya NO recibe la lista de la vista que llama: eso mataba las sesiones de las
/// otras vistas (ver `Claims`). Una sesión sobrevive mientras alguien la
/// reclame, mientras esté en vuelo a otra ventana, o mientras no haya agotado
/// la gracia sin dueño.
pub fn reap_orphans() -> u32 {
    let vivas: Vec<String> = with_map(|map| map.keys().cloned().collect());
    let stale: Vec<String> = vivas
        .into_iter()
        .filter(|id| !claimed(id) && !transfer_guarded(id) && orphan_ready(id))
        .collect();
    for id in &stale {
        close_session(id);
    }
    stale.len() as u32
}

/// Barrido a pedido de la UI. El de verdad lo hace el hilo de `spawn_reaper`.
#[tauri::command]
pub fn console_gc() -> Result<u32, String> {
    Ok(reap_orphans())
}

/// Hilo de limpieza, armado la primera vez que se abre una consola.
///
/// Sin esto, una sesión huérfana (ventana cerrada, overlay recargado) viviría
/// hasta que alguien abriera otra consola. Vive en este módulo y no en el
/// `setup` de la app para no pedirle nada a `lib.rs`.
fn spawn_reaper() {
    static REAPER: std::sync::Once = std::sync::Once::new();
    REAPER.call_once(|| {
        thread::spawn(|| loop {
            thread::sleep(REAP_EVERY);
            reap_orphans();
        });
    });
}

/// La vista empieza a mostrar esta sesión (y deja de ser huérfana).
#[tauri::command]
pub fn console_attach(session: String, view: String) {
    claim(&session, &view);
}

/// La vista deja de mostrarla. No la mata: solo suelta.
#[tauri::command]
pub fn console_detach(session: String, view: String) {
    unclaim(&session, &view);
}

/// «Sigo acá y sigo mostrando esto.»
///
/// Es lo que separa una vista viva de una que se fue sin avisar. Manda la
/// lista entera para que un reclamo perdido se recupere solo.
#[tauri::command]
pub fn console_heartbeat(view: String, sessions: Vec<String>) {
    for session in sessions {
        claim(&session, &view);
    }
}

/// Últimos bytes emitidos por la sesión, para repintar un terminal nuevo.
///
/// Al mudar una consola de ventana, el xterm nace vacío aunque el PTY siga
/// vivo: esto le devuelve su scrollback reciente. Puede cortar un escape ANSI
/// por la mitad al inicio —xterm lo tolera— y un multibyte partido sale como
/// `�`; el resto llega intacto.
#[tauri::command]
pub fn console_tail(session: String, max_bytes: Option<usize>) -> Result<String, String> {
    let tope = max_bytes.unwrap_or(TAIL_MAX).clamp(1, TAIL_MAX);
    with_tails(|tails| {
        let arc = tails
            .get(&session)
            .ok_or_else(|| "esa consola ya no existe".to_string())?;
        let mut cola = arc
            .lock()
            .map_err(|_| "lock de la cola de consola".to_string())?;
        let contiguo = cola.make_contiguous();
        let desde = contiguo.len().saturating_sub(tope);
        Ok(String::from_utf8_lossy(&contiguo[desde..]).into_owned())
    })
}

/// Marca sesiones en vuelo a otra ventana. Ver `TRANSFERS`.
#[tauri::command]
pub fn console_begin_transfer(sessions: Vec<String>) -> Result<(), String> {
    let now = Instant::now();
    let mut guard = TRANSFERS.lock_or_recover();
    let map = guard.get_or_insert_with(HashMap::new);
    for id in sessions {
        map.insert(id, now);
    }
    Ok(())
}

/// La receptora ya adoptó: se levanta la protección.
#[tauri::command]
pub fn console_end_transfer(sessions: Vec<String>) -> Result<(), String> {
    let mut guard = TRANSFERS.lock_or_recover();
    if let Some(map) = guard.as_mut() {
        for id in &sessions {
            map.remove(id);
        }
    }
    Ok(())
}

/// Entrega un JSON a otra ventana (`agents-transfer`, su ack, …).
///
/// El envío pasa por Rust y no por `emitTo` del frontend: el backend emite a
/// cualquier etiqueta sin pedir permiso extra de capabilities.
#[tauri::command]
pub fn console_transfer_deliver(
    app: AppHandle,
    target_window: String,
    event: String,
    payload: String,
) -> Result<(), String> {
    app.emit_to(target_window.as_str(), event.as_str(), payload)
        .map_err(|e| format!("no se pudo avisar a la otra ventana: {e}"))
}

/// CLI de agente que está corriendo *dentro* de la PTY (hijo de la shell).
///
/// Si abriste una consola local y después escribiste `codex`, la pestaña
/// sigue siendo «Local» hasta que esto lo ve. `None` = solo la shell, o
/// la sesión ya no existe.
#[tauri::command]
pub fn console_foreground_cli(session: String) -> Option<String> {
    let pid = with_map(|map| map.get(&session).map(|live| live.pid))?;
    if pid == 0 {
        return None;
    }
    foreground_agent_cli(pid)
}

#[cfg(windows)]
fn foreground_agent_cli(root: u32) -> Option<String> {
    let rows = process_snapshot();
    if rows.is_empty() {
        return None;
    }
    let mut best: Option<(u32, String)> = None;
    let mut stack = vec![(root, 0u32)];
    let mut seen = HashSet::new();
    while let Some((pid, depth)) = stack.pop() {
        if !seen.insert(pid) {
            continue;
        }
        if pid != root {
            let path = process_image_path(pid).or_else(|| {
                rows.iter()
                    .find(|(id, _, _)| *id == pid)
                    .map(|(_, _, exe)| exe.clone())
            });
            if let Some(path) = path {
                if let Some(cli) = agent_cli_from_path(&path) {
                    if best.as_ref().is_none_or(|(d, _)| depth >= *d) {
                        best = Some((depth, cli));
                    }
                }
            }
        }
        for (id, parent, _) in &rows {
            if *parent == pid {
                stack.push((*id, depth + 1));
            }
        }
    }
    best.map(|(_, cli)| cli)
}

#[cfg(not(windows))]
fn foreground_agent_cli(_root: u32) -> Option<String> {
    None
}

#[cfg(windows)]
fn process_snapshot() -> Vec<(u32, u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    let snap = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snap.is_null() || snap == INVALID_HANDLE_VALUE {
        return Vec::new();
    }
    let mut entry = unsafe { std::mem::zeroed::<PROCESSENTRY32W>() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut out = Vec::new();
    unsafe {
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                out.push((
                    entry.th32ProcessID,
                    entry.th32ParentProcessID,
                    wchar_to_string(&entry.szExeFile),
                ));
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
    }
    out
}

#[cfg(windows)]
fn wchar_to_string(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

#[cfg(windows)]
fn process_image_path(pid: u32) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Nombre de CLI conocido a partir de un exe o de su ruta (`codex.exe`,
/// `…/opencode-ai/bin/opencode`).
fn agent_cli_from_path(path: &str) -> Option<String> {
    let normalized = path.replace('\\', "/").to_ascii_lowercase();
    let stem = Path::new(&normalized)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    match stem {
        "claude" | "claude-code" => return Some("claude".into()),
        "codex" => return Some("codex".into()),
        "opencode" => return Some("opencode".into()),
        "cursor-agent" => return Some("cursor-agent".into()),
        "agy" | "antigravity" => return Some("agy".into()),
        "grok" => return Some("grok".into()),
        _ => {}
    }
    let file = Path::new(&normalized)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    for name in ["claude", "codex", "opencode", "cursor-agent", "agy", "grok"] {
        if file.contains(name) {
            return Some(name.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_push_recorta_por_delante() {
        let mut cola = VecDeque::new();
        tail_push(&mut cola, b"hola ");
        tail_push(&mut cola, b"mundo");
        assert_eq!(cola.iter().copied().collect::<Vec<_>>(), b"hola mundo");
        // Llenar por encima del tope: lo viejo cae, lo nuevo queda entero.
        let grande = vec![b'x'; TAIL_MAX + 100];
        tail_push(&mut cola, &grande);
        assert_eq!(cola.len(), TAIL_MAX);
        assert!(cola.iter().all(|b| *b == b'x'));
    }

    #[test]
    fn transfer_guarded_consume_y_caduca() {
        let id = format!("test-{}", Uuid::new_v4());
        assert!(!transfer_guarded(&id));
        console_begin_transfer(vec![id.clone()]).unwrap();
        assert!(transfer_guarded(&id));
        // `console_close` en vuelo no mata: consume la marca y sigue vivo.
        console_close(id.clone()).unwrap();
        assert!(!transfer_guarded(&id));
        // Sin marca, el gc lo vería como huérfano (acá solo se prueba la
        // marca: matar de verdad necesita un PTY y no se hace en tests).
        console_begin_transfer(vec![id.clone()]).unwrap();
        console_end_transfer(vec![id.clone()]).unwrap();
        assert!(!transfer_guarded(&id));
    }

    #[test]
    fn el_registro_de_vistas_manda_sobre_el_barrido() {
        let id = format!("test-{}", Uuid::new_v4());
        assert!(!claimed(&id));

        claim(&id, "isla");
        claim(&id, "float");
        assert!(claimed(&id));
        assert!(!orphan_ready(&id));

        // Soltar UNA vista no la deja huérfana: la otra la sigue mostrando.
        unclaim(&id, "isla");
        assert!(claimed(&id));

        // Sin vistas entra en la gracia, que es justo el hueco del despegue.
        unclaim(&id, "float");
        assert!(!claimed(&id));
        assert!(!orphan_ready(&id), "recién soltada no es huérfana");

        // Y volver a reclamarla apaga el reloj.
        claim(&id, "float");
        assert!(claimed(&id));
        forget_claims(&id);
        assert!(!claimed(&id));
    }

    #[test]
    fn un_reclamo_sin_latido_caduca() {
        let id = format!("test-{}", Uuid::new_v4());
        // Una vista que se fue sin desmontarse (recarga del overlay) deja su
        // reclamo con el reloj viejo: no puede blindar la PTY para siempre.
        with_claims(|c| {
            c.by_session.entry(id.clone()).or_default().insert(
                "fantasma".to_string(),
                Instant::now() - CLAIM_TTL - Duration::from_secs(1),
            );
        });
        assert!(!claimed(&id), "el reclamo vencido no cuenta");
        // Y al caer entra en la gracia como cualquier huérfana.
        assert!(!orphan_ready(&id));
        forget_claims(&id);
    }

    #[test]
    fn filters_npm_pnpm_script_vars() {
        // Las que dejaba pnpm dev y redirigian el prefix global de npm.
        assert!(is_script_env_var("npm_config_dir"));
        assert!(is_script_env_var("NPM_CONFIG_PREFIX"));
        assert!(is_script_env_var("npm_config__jsr-registry"));
        assert!(is_script_env_var("npm_lifecycle_event"));
        assert!(is_script_env_var("npm_package_json"));
        assert!(is_script_env_var("npm_execpath"));
        assert!(is_script_env_var("PNPM_SCRIPT_SRC_DIR"));
        assert!(is_script_env_var("NODE_RUN_SCRIPT_NAME"));
        // El resto del entorno queda intacto.
        assert!(!is_script_env_var("PATH"));
        assert!(!is_script_env_var("NODE_ENV"));
        assert!(!is_script_env_var("npmrc"));
        assert!(!is_script_env_var("NPM_TOKEN"));
    }

    #[test]
    fn agent_cli_from_exe_and_shim_path() {
        assert_eq!(agent_cli_from_path("codex.exe").as_deref(), Some("codex"));
        assert_eq!(
            agent_cli_from_path(r"C:\Users\x\.local\bin\claude.exe").as_deref(),
            Some("claude")
        );
        assert_eq!(
            agent_cli_from_path(r"C:\npm\opencode.cmd").as_deref(),
            Some("opencode")
        );
        assert_eq!(agent_cli_from_path("powershell.exe"), None);
        assert_eq!(agent_cli_from_path("cursor.exe"), None);
        assert_eq!(
            agent_cli_from_path("cursor-agent.exe").as_deref(),
            Some("cursor-agent")
        );
    }

    #[cfg(windows)]
    #[test]
    fn la_shell_de_los_agentes_es_el_cmd_de_windows() {
        let ruta = system_cmd_exe();
        assert!(ruta.is_absolute(), "{}", ruta.display());
        assert!(
            ruta.to_string_lossy().to_lowercase().ends_with(r"\cmd.exe"),
            "{}",
            ruta.display()
        );
    }

    /// La ruta elegida tiene que existir: spawnear un path inexistente rompe
    /// el PTY, y en macOS el fallback es zsh, no bash.
    #[cfg(not(windows))]
    #[test]
    fn la_shell_del_usuario_existe() {
        let shell = user_shell();
        assert!(shell.is_file(), "{}", shell.display());
        assert!(shell.is_absolute(), "{}", shell.display());
    }

    #[test]
    fn detecta_sintaxis_de_shell() {
        assert!(has_shell_syntax(
            "curl -fsSL https://x.ai/cli/install.sh | bash"
        ));
        assert!(has_shell_syntax(
            "irm 'https://cursor.com/install?win32=true' | iex"
        ));
        assert!(has_shell_syntax("npm i > log.txt"));
        assert!(!has_shell_syntax("npm install -g @openai/codex"));
        assert!(!has_shell_syntax("opencode"));
    }

    /// La regresión del instalador de Grok: `curl … | bash` lanzaba curl
    /// directo y `|` y `bash` terminaban siendo URLs.
    #[cfg(not(windows))]
    #[test]
    fn una_linea_con_pipe_va_a_la_shell() {
        let cmd = build_local_command("curl -fsSL https://x.ai/cli/install.sh | bash").unwrap();
        let argv = cmd.get_argv();
        assert_eq!(argv[0].as_os_str(), user_shell().as_os_str(), "{argv:?}");
        assert!(
            argv.iter().any(|a| a.to_string_lossy().contains("| bash")),
            "{argv:?}"
        );
    }

    /// Un comando simple sigue yendo directo: no hay shell de por medio.
    #[test]
    fn un_comando_simple_no_pasa_por_la_shell() {
        let Some((exe, _)) = crate::agents::exe::launcher("echo") else {
            return;
        };
        let cmd = build_local_command("echo hola").unwrap();
        assert_eq!(
            cmd.get_argv()[0].as_os_str(),
            exe.as_os_str(),
            "{:?}",
            cmd.get_argv()
        );
    }
}
