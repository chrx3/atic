//! Lo que cada agente de Atic Code necesita en la máquina, en orden, y cómo instalarlo con
//! un botón: Node (con npm), el CLI del agente y su sesión.
//!
//! Cada paso corre en la terminal integrada, a la vista, como un script propio
//! (`powershell -ExecutionPolicy Bypass -File` en Windows, `/bin/sh` en macOS) que al
//! terminar deja su código de salida en un archivo. Así se sabe cuándo terminó: se vuelve
//! a revisar la máquina y, con «Instalar lo que falta», se sigue con el próximo paso.
//!
//! Por qué así y no con el `npm` del PATH (lo que falló con un usuario nuevo): tras instalar
//! Node con Atic abierta, el PATH de Atic y de sus terminales seguía siendo el de antes, y
//! `npm` no aparecía. Ahora cada script arma su PATH desde el registro (Windows) o desde las
//! carpetas conocidas (macOS), y `npm` se toma junto al `node` encontrado (`npm.cmd`: el
//! `npm.ps1` lo bloquea la política de scripts de PowerShell).
//!
//! Sin winget (Windows) ni Homebrew (macOS), Node se instala en la carpeta del usuario
//! desde nodejs.org: sin permisos de administrador y con `npm -g` que escribe ahí mismo.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, ClickEvent, Context, FontWeight, SharedString, Window};

use super::style::t;
use super::{agent_details, agents_tab, sidecar, subscriptions, CodeView};
use crate::i18n::{t as tr, tf};

/// Cada cuánto se mira si el paso terminó, y hasta cuándo se espera.
const WATCH: Duration = Duration::from_millis(1500);
const WATCH_FOR: Duration = Duration::from_secs(30 * 60);

const NODE_DOWNLOAD: &str = "https://nodejs.org/en/download";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    /// Node.js con npm: corre el agente de Atic Code y, con npm, instala algunos CLI.
    Node,
    /// El CLI del agente (no la app de escritorio).
    Cli,
    /// La sesión en el CLI.
    SignIn,
}

/// Lo que se encontró para un agente.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Report {
    pub node: Option<PathBuf>,
    pub npm: Option<PathBuf>,
    /// El CLI está instalado.
    pub cli: bool,
    /// Está la app de escritorio, pero no el CLI.
    pub desktop: bool,
    /// `None`: no se sabe (el CLI no tiene cómo preguntarlo, o no se preguntó).
    pub signed_in: Option<bool>,
}

impl Report {
    /// npm hace falta solo para instalar un CLI que va por npm y aún no está.
    fn npm_missing(&self, cli: &str) -> bool {
        self.node.is_some() && self.npm.is_none() && !self.cli && needs_npm(cli)
    }

    pub fn done(&self, step: Step, cli: &str) -> bool {
        match step {
            Step::Node => self.node.is_some() && !self.npm_missing(cli),
            Step::Cli => self.cli,
            Step::SignIn => self.signed_in == Some(true),
        }
    }
}

/// Un paso corriendo en la terminal.
pub(super) struct Run {
    pub agent: &'static str,
    pub step: Step,
    marker: PathBuf,
    /// «Instalar lo que falta»: al terminar bien, sigue con el próximo.
    all: bool,
    /// El CLI se instaló en esta pasada: se pide la sesión aunque no se sepa si falta.
    installed_cli: bool,
    started: Instant,
}

/// Un paso que terminó sin quedar listo.
#[derive(Clone)]
pub(super) struct Failure {
    pub agent: &'static str,
    pub step: Step,
    pub message: String,
    /// Ofrecer la descarga de Node en el navegador.
    pub node_download: bool,
}

// --- Qué necesita cada agente ---------------------------------------------------------

fn catalog(cli: &str) -> Option<&'static crate::agents::Agent> {
    crate::agents::AGENTS.iter().find(|agent| agent.cli == cli)
}

pub(super) fn agent_name(cli: &str) -> &'static str {
    catalog(cli).map(|agent| agent.name).unwrap_or("Claude Code")
}

fn install_line(cli: &str) -> Option<&'static str> {
    catalog(cli).map(|agent| agent.install)
}

/// Su instalación va por `npm` (Codex; OpenCode en Windows).
fn needs_npm(cli: &str) -> bool {
    install_line(cli).is_some_and(|line| line.starts_with("npm "))
}

/// Cómo se inicia sesión. Antigravity no tiene comando: pide entrar al abrirlo.
fn login_line(cli: &str) -> Option<&'static str> {
    agent_details::auth_commands(cli).map(|(login, _)| login).filter(|_| cli != "agy")
}

/// El nombre del CLI, para no confundirlo con la app de escritorio.
fn cli_name(cli: &str) -> &'static str {
    match cli {
        "claude" => "Claude Code",
        "codex" => "Codex CLI",
        "opencode" => "OpenCode CLI",
        "cursor-agent" => "Cursor CLI (cursor-agent)",
        "agy" => "Antigravity CLI (agy)",
        "grok" => "Grok CLI",
        _ => "CLI",
    }
}

/// La app de escritorio que suele confundirse con el CLI.
fn app_name(cli: &str) -> Option<&'static str> {
    Some(match cli {
        "claude" => "Claude",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        "cursor-agent" => "Cursor",
        "agy" => "Antigravity",
        _ => return None,
    })
}

/// Si está la app de escritorio del agente (que no trae el CLI que usa Atic Code).
fn desktop_app(cli: &str) -> bool {
    app_name(cli).is_some_and(|app| desktop_app_installed(cli, app))
}

#[cfg(target_os = "macos")]
fn desktop_app_installed(_cli: &str, app: &str) -> bool {
    let bundle = format!("{app}.app");
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Path::new("/Applications").join(&bundle).exists() || home.is_some_and(|home| home.join("Applications").join(&bundle).exists())
}

#[cfg(windows)]
fn desktop_app_installed(cli: &str, _app: &str) -> bool {
    let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) else {
        return false;
    };
    // Instaladores clásicos en `%LOCALAPPDATA%` y, para las de la Microsoft Store, la
    // carpeta del paquete (`<Nombre>_<editor>`).
    let (dirs, packages): (&[&str], &[&str]) = match cli {
        "claude" => (&["AnthropicClaude", r"Programs\Claude"], &["claude_", "anthropicpbc.claude"]),
        "codex" => (&[r"Programs\OpenAI\Codex", r"Programs\Codex"], &["openai.codex"]),
        "opencode" => (&[r"Programs\OpenCode", r"Programs\opencode-desktop"], &[]),
        "cursor-agent" => (&[r"Programs\cursor"], &[]),
        "agy" => (&[r"Programs\Antigravity"], &[]),
        _ => (&[], &[]),
    };
    if dirs.iter().any(|dir| local.join(dir).is_dir()) {
        return true;
    }
    let Ok(entries) = std::fs::read_dir(local.join("Packages")) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        packages.iter().any(|prefix| name.starts_with(prefix))
    })
}

#[cfg(not(any(windows, target_os = "macos")))]
fn desktop_app_installed(_cli: &str, _app: &str) -> bool {
    false
}

/// Los pasos de un agente, en orden.
pub(super) fn steps(cli: &str) -> Vec<Step> {
    let mut steps = vec![Step::Node, Step::Cli];
    if login_line(cli).is_some() {
        steps.push(Step::SignIn);
    }
    steps
}

/// El próximo paso que falta. La sesión se pide si se sabe que falta o si el CLI recién se
/// instaló (un CLI nuevo nunca tiene sesión).
pub(super) fn next_step(report: &Report, cli: &str, installed_cli: bool) -> Option<Step> {
    if !report.done(Step::Node, cli) {
        return Some(Step::Node);
    }
    if !report.cli {
        return Some(Step::Cli);
    }
    if login_line(cli).is_none() {
        return None;
    }
    match report.signed_in {
        Some(false) => Some(Step::SignIn),
        None if installed_cli => Some(Step::SignIn),
        _ => None,
    }
}

/// Revisa la máquina para un agente. Lanza procesos (con `login`, el CLI para ver la
/// sesión): va en otro hilo.
pub(super) fn inspect(cli: &str, login: bool) -> Report {
    // Lo instalado con Atic abierta (Windows lo anota en el registro) queda a la vista.
    atic_agents::exe::refresh_process_path();
    inspect_fresh(cli, login)
}

/// `inspect` sin refrescar el PATH: para revisar varios agentes a la vez, que lo
/// refresque una vez quien los lanza.
pub(super) fn inspect_fresh(cli: &str, login: bool) -> Report {
    let node = sidecar::node_path();
    let npm = sidecar::npm_path(node.as_deref());
    let found = if cli == "claude" { sidecar::claude_path().is_some() } else { crate::agents::on_path(cli) };
    let signed_in = if login && found { signed_in(cli) } else { None };
    Report { node, npm, cli: found, desktop: !found && desktop_app(cli), signed_in }
}

fn signed_in(cli: &str) -> Option<bool> {
    match cli {
        "claude" => sidecar::claude_path().map(|path| super::setup::signed_in(&path)),
        "codex" => agent_details::run_cli("codex", &["login", "status"], 15).as_deref().and_then(agent_details::parse_codex_status).map(|account| account.signed_in),
        "opencode" => Some(!subscriptions::opencode().is_empty()),
        _ => None,
    }
}

// --- Los scripts ----------------------------------------------------------------------

/// Entre comillas simples de PowerShell.
fn ps_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Entre comillas simples de sh.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Lo que corre un paso, sin el envoltorio.
fn body(step: Step, cli: &str, report: &Report) -> Option<String> {
    match step {
        Step::Node => Some(node_body(report.node.is_some())),
        Step::Cli => install_body(cli, report.npm.as_deref()),
        Step::SignIn => login_line(cli).map(|login| agents_tab::with_resolved_cli(cli, login)),
    }
}

/// Instala Node (o lo repara si está sin npm).
fn node_body(repair: bool) -> String {
    if cfg!(windows) {
        let force = if repair { " --force" } else { "" };
        format!(
            "if (Get-Command winget -ErrorAction SilentlyContinue) {{\r\n  \
               Write-Host 'Instalando Node.js con winget...'\r\n  \
               winget install -e --id OpenJS.NodeJS.LTS --accept-source-agreements --accept-package-agreements{force}\r\n  \
               Update-AticPath\r\n\
             }}\r\n\
             if (-not (Get-Command npm.cmd -ErrorAction SilentlyContinue)) {{\r\n  \
               Install-AticNode\r\n  \
               Update-AticPath\r\n\
             }}\r\n\
             $global:LASTEXITCODE = 0\r\n\
             if (-not (Get-Command npm.cmd -ErrorAction SilentlyContinue)) {{ throw 'npm sigue sin aparecer' }}"
        )
    } else {
        let brew = if repair { "" } else { "if command -v brew >/dev/null 2>&1; then\n  echo 'Instalando Node.js con Homebrew...'\n  brew install node\nelse\n" };
        let end = if repair { "" } else { "fi\n" };
        format!(
            "{brew}  echo 'Instalando Node.js en ~/.local/node...'\n  \
             arch=$(uname -m); [ \"$arch\" = x86_64 ] && arch=x64\n  \
             base=https://nodejs.org/dist/latest-v24.x\n  \
             file=$(curl -fsSL \"$base/SHASUMS256.txt\" | awk '{{print $2}}' | grep \"darwin-$arch.tar.gz$\" | head -n 1)\n  \
             [ -n \"$file\" ] || {{ echo 'No se encontró la descarga de Node.'; exit 1; }}\n  \
             rm -rf \"$HOME/.local/node\" && mkdir -p \"$HOME/.local/node\" && curl -fSL \"$base/$file\" | tar -xz --strip-components=1 -C \"$HOME/.local/node\" || exit 1\n\
             {end}command -v npm >/dev/null 2>&1"
        )
    }
}

/// Instala el CLI. Lo que va por npm usa el `npm` junto a Node (no el del PATH); en macOS,
/// si la carpeta global de npm no deja escribir (el Node del `.pkg` oficial), va a
/// `~/.local`, donde Atic también busca.
fn install_body(cli: &str, npm: Option<&Path>) -> Option<String> {
    let line = install_line(cli)?;
    let Some(rest) = line.strip_prefix("npm ") else {
        return Some(line.to_string());
    };
    Some(if cfg!(windows) {
        let npm = npm.map(|p| p.display().to_string()).unwrap_or_else(|| "npm.cmd".into());
        format!("& {} {rest}", ps_quote(&npm))
    } else {
        let npm = npm.map(|p| sh_quote(&p.display().to_string())).unwrap_or_else(|| "npm".into());
        format!("{npm} {rest} || {npm} {rest} --prefix \"$HOME/.local\"")
    })
}

/// El script completo: PATH fresco, el paso y el código de salida en `marker`.
fn wrap(body: &str, marker: &Path) -> String {
    if cfg!(windows) {
        // Con BOM: PowerShell 5.1 lee como ANSI un archivo sin él, y la ruta del usuario
        // puede llevar tildes.
        format!(
            "\u{feff}[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor 3072\r\n\
             $ProgressPreference = 'SilentlyContinue'\r\n\
             function Update-AticPath {{\r\n  \
               $dirs = @([Environment]::GetEnvironmentVariable('Path', 'Machine'), [Environment]::GetEnvironmentVariable('Path', 'User'), $env:Path, \"$env:ProgramFiles\\nodejs\", \"$env:LOCALAPPDATA\\Programs\\atic-node\", \"$env:APPDATA\\npm\")\r\n  \
               $env:Path = ($dirs | Where-Object {{ $_ }}) -join ';'\r\n\
             }}\r\n\
             function Install-AticNode {{\r\n  \
               Write-Host 'Instalando Node.js en tu carpeta de usuario...'\r\n  \
               $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {{ 'arm64' }} else {{ 'x64' }}\r\n  \
               $v = (Invoke-RestMethod https://nodejs.org/dist/index.json | Where-Object {{ $_.lts }} | Select-Object -First 1).version\r\n  \
               $zip = Join-Path $env:TEMP \"node-$v.zip\"\r\n  \
               Invoke-WebRequest \"https://nodejs.org/dist/$v/node-$v-win-$arch.zip\" -OutFile $zip -UseBasicParsing\r\n  \
               $tmp = Join-Path $env:TEMP \"atic-node-$v\"\r\n  \
               Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue\r\n  \
               Expand-Archive $zip $tmp -Force\r\n  \
               $dest = Join-Path $env:LOCALAPPDATA 'Programs\\atic-node'\r\n  \
               Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue\r\n  \
               New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null\r\n  \
               Move-Item (Join-Path $tmp \"node-$v-win-$arch\") $dest\r\n  \
               $user = \"$([Environment]::GetEnvironmentVariable('Path', 'User'))\"\r\n  \
               foreach ($dir in @($dest, (Join-Path $env:APPDATA 'npm'))) {{\r\n    \
                 if (($user -split ';') -notcontains $dir) {{ $user = (($user.TrimEnd(';') + ';' + $dir).TrimStart(';')) }}\r\n  \
               }}\r\n  \
               [Environment]::SetEnvironmentVariable('Path', $user, 'User')\r\n\
             }}\r\n\
             Update-AticPath\r\n\
             $code = 0\r\n\
             $global:LASTEXITCODE = 0\r\n\
             try {{\r\n\
             {body}\r\n  \
               if (-not $?) {{ $code = 1 }}\r\n  \
               if ($LASTEXITCODE) {{ $code = $LASTEXITCODE }}\r\n\
             }} catch {{\r\n  \
               Write-Host $_ -ForegroundColor Red\r\n  \
               $code = 1\r\n\
             }}\r\n\
             Set-Content -LiteralPath {marker} -Value $code\r\n\
             exit $code\r\n",
            marker = ps_quote(&marker.display().to_string()),
        )
    } else {
        let path = atic_agents::exe::merged_path_var().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        format!(
            "#!/bin/sh\n\
             export PATH={path}\":$HOME/.local/node/bin:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH\"\n\
             (\n{body}\n)\n\
             code=$?\n\
             printf '%s\\n' \"$code\" > {marker}\n\
             exit $code\n",
            path = sh_quote(&path),
            marker = sh_quote(&marker.display().to_string()),
        )
    }
}

/// La línea que se escribe en la terminal para correr el script.
fn launch_line(script: &Path) -> String {
    if cfg!(windows) {
        format!("powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}", ps_quote(&script.display().to_string()))
    } else {
        format!("/bin/sh {}", sh_quote(&script.display().to_string()))
    }
}

fn step_key(step: Step) -> &'static str {
    match step {
        Step::Node => "node",
        Step::Cli => "cli",
        Step::SignIn => "signin",
    }
}

/// Deja el script de un paso y devuelve (línea para la terminal, archivo del resultado).
fn prepare(step: Step, cli: &str, report: &Report) -> Result<(String, PathBuf), String> {
    let body = body(step, cli, report).ok_or_else(|| tr("pill.code.agentSetup.noCommand").to_string())?;
    let dir = std::env::temp_dir().join("atic-setup");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let name = format!("{cli}-{}", step_key(step));
    let marker = dir.join(format!("{name}.done"));
    let _ = std::fs::remove_file(&marker);
    let script = dir.join(format!("{name}.{}", if cfg!(windows) { "ps1" } else { "sh" }));
    std::fs::write(&script, wrap(&body, &marker)).map_err(|e| e.to_string())?;
    Ok((launch_line(&script), marker))
}

/// El código que dejó el script, si ya terminó.
fn read_marker(marker: &Path) -> Option<i32> {
    let text = std::fs::read_to_string(marker).ok()?;
    Some(text.trim_start_matches('\u{feff}').trim().parse().unwrap_or(1))
}

/// Qué decir cuando un paso terminó sin quedar listo.
fn failure(step: Step, cli: &'static str, report: &Report, code: Option<i32>) -> Failure {
    let (message, node_download) = match step {
        Step::Node if report.node.is_some() => (tr("pill.code.agentSetup.failNpm").to_string(), true),
        Step::Node if code == Some(0) => (tr("pill.code.agentSetup.failNodeRestart").to_string(), false),
        Step::Node if cfg!(windows) => (tr("pill.code.agentSetup.failNodeWin").to_string(), true),
        Step::Node => (tr("pill.code.agentSetup.failNodeMac").to_string(), true),
        Step::Cli => (tf("pill.code.agentSetup.failCli", &[("name", &cli_name(cli))]), false),
        Step::SignIn => (tr("pill.code.agentSetup.failSignIn").to_string(), false),
    };
    Failure { agent: cli, step, message, node_download }
}

// --- La vista -------------------------------------------------------------------------

impl CodeView {
    /// Guarda lo revisado. Una revisión sin la sesión no borra la que ya se sabía.
    pub(super) fn store_setup_report(&mut self, cli: &'static str, mut report: Report) {
        if report.signed_in.is_none() && report.cli {
            report.signed_in = self.setup_reports.get(cli).and_then(|old| old.signed_in);
        }
        self.setup_reports.insert(cli, report);
    }

    /// Corre un paso (`only`) o, sin él, lo que falte en orden («Instalar lo que falta»).
    pub(super) fn start_agent_setup(&mut self, cli: &'static str, only: Option<Step>, window: &mut Window, cx: &mut Context<Self>) {
        if self.setup_run.is_some() {
            return;
        }
        let report = self.setup_reports.get(cli).cloned().unwrap_or_default();
        let Some(step) = only.or_else(|| next_step(&report, cli, false)) else {
            self.refresh_agent_setup(cli, cx);
            return;
        };
        self.launch_setup_step(cli, step, only.is_none(), false, &report, window, cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn launch_setup_step(&mut self, cli: &'static str, step: Step, all: bool, installed_cli: bool, report: &Report, window: &mut Window, cx: &mut Context<Self>) {
        self.setup_failed = None;
        let (line, marker) = match prepare(step, cli, report) {
            Ok(prepared) => prepared,
            Err(error) => {
                self.setup_failed = Some(Failure { agent: cli, step, message: tf("pill.code.agentSetup.failStart", &[("error", &error)]), node_download: false });
                cx.notify();
                return;
            }
        };
        let title = tf("pill.code.agentSetup.runTitle", &[("step", &step_title(step, cli))]);
        // La terminal tiene que verse: ahí se aceptan permisos o se entra en el navegador.
        self.settings_open = false;
        let home = crate::paths::home().map(std::ffi::OsString::from).map(PathBuf::from);
        self.run_in_terminal(line, &title, home, window, cx);
        self.setup_run = Some(Run { agent: cli, step, marker, all, installed_cli, started: Instant::now() });
        cx.notify();
        self.watch_setup_run(window, cx);
    }

    /// Espera a que el paso deje su resultado, revisa y sigue (o dice qué pasó).
    fn watch_setup_run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(WATCH).await;
            let Ok(Some((cli, marker, started))) = this.update(cx, |view, _| view.setup_run.as_ref().map(|run| (run.agent, run.marker.clone(), run.started))) else {
                return;
            };
            let code = read_marker(&marker);
            let timed_out = started.elapsed() > WATCH_FOR;
            if code.is_none() && !timed_out {
                continue;
            }
            let _ = std::fs::remove_file(&marker);
            let report = cx.background_spawn(async move { inspect(cli, true) }).await;
            let _ = this.update_in(cx, |view, window, cx| view.finish_setup_step(report, code, window, cx));
            return;
        })
        .detach();
    }

    fn finish_setup_step(&mut self, report: Report, code: Option<i32>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.setup_run.take() else {
            return;
        };
        let cli = run.agent;
        // Sin forma de ver la sesión, vale lo que dijo el comando.
        let ok = match run.step {
            Step::SignIn if report.signed_in.is_none() => code == Some(0),
            step => report.done(step, cli),
        };
        let installed_cli = run.installed_cli || (run.step == Step::Cli && ok);
        let mut report = report;
        if run.step == Step::SignIn && ok {
            report.signed_in = Some(true);
        }
        self.store_setup_report(cli, report.clone());
        // El agente de Atic Code (que sobrevive a la pill) ve lo recién instalado.
        self.sidecar.refresh_path();
        self.after_setup_change(cli, &report, cx);
        if !ok {
            let failed = failure(run.step, cli, &report, code);
            // Node quedó instalado pero ni releyendo el PATH se ve: reiniciar Atic lo arregla.
            if run.step == Step::Node && report.node.is_none() && code == Some(0) {
                crate::restart::ask(tr("pill.code.agentSetup.restartWhy"), crate::restart::Resume::CodeAgentSetup { agent: cli.to_string() }, cx);
            }
            self.setup_failed = Some(failed);
            cx.notify();
            return;
        }
        if run.all {
            if let Some(step) = next_step(&report, cli, installed_cli) {
                // Un paso que no avanzó no se repite en bucle.
                if step != run.step {
                    self.launch_setup_step(cli, step, true, installed_cli, &report, window, cx);
                    return;
                }
            }
        }
        cx.notify();
    }

    /// Lo que cambia en el resto de Atic Code cuando se instala algo.
    fn after_setup_change(&mut self, cli: &'static str, report: &Report, cx: &mut Context<Self>) {
        if report.cli && cli != "claude" && super::acp::agent(cli).is_some() && !self.acp_installed.contains(&cli) {
            self.acp_installed.push(cli);
        }
        // La versión en la pestaña Agentes y la tarjeta del inicio.
        self.load_agent_versions(cx);
        self.load_setup(false, cx);
    }

    /// Vuelve a revisar un agente, con su sesión.
    pub(super) fn refresh_agent_setup(&mut self, cli: &'static str, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let report = cx.background_spawn(async move { inspect(cli, true) }).await;
            let _ = this.update(cx, |view, cx| {
                view.store_setup_report(cli, report);
                cx.notify();
            });
        })
        .detach();
    }

    /// Los pasos de un agente, con su estado y sus botones, y «Instalar lo que falta».
    /// `None` si aún no se revisó.
    pub(super) fn agent_setup_steps(&self, cli: &'static str, cx: &mut Context<Self>) -> Option<AnyElement> {
        let report = self.setup_reports.get(cli)?;
        let t = t();
        let busy = self.setup_run.is_some();
        let running = self.setup_run.as_ref().filter(|run| run.agent == cli).map(|run| run.step);
        let failed = self.setup_failed.as_ref().filter(|failed| failed.agent == cli);
        let id_base = super::agents_tab::rows().iter().position(|row| row.cli == cli).unwrap_or(0) * 10;
        let mut list = div().flex().flex_col().gap(px(6.));
        let mut missing = false;
        for (index, step) in steps(cli).into_iter().enumerate() {
            let done = report.done(step, cli);
            let is_running = running == Some(step);
            let error = failed.filter(|failed| failed.step == step);
            // La sesión que no se puede revisar no cuenta como pendiente.
            let unknown = step == Step::SignIn && report.signed_in.is_none();
            let blocked = match step {
                Step::Node => false,
                Step::Cli => !report.done(Step::Node, cli),
                Step::SignIn => !report.cli,
            };
            if !done && !unknown {
                missing = true;
            }
            let mark = if is_running {
                div().size(px(22.)).flex_none().flex().items_center().justify_center().child(gpui_m3::LoadingIndicator::new().size(px(22.))).into_any_element()
            } else if done {
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(t.accent)
                    .child(gpui_m3::Icon::new("check").size(px(14.)).color(t.on_accent))
                    .into_any_element()
            } else if error.is_some() {
                div().size(px(22.)).flex_none().flex().items_center().justify_center().child(gpui_m3::Icon::new("alert").size(px(18.)).color(t.bad)).into_any_element()
            } else {
                div()
                    .size(px(22.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_2()
                    .border_color(t.muted.opacity(0.5))
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.muted)
                    .child((index + 1).to_string())
                    .into_any_element()
            };
            let desc: SharedString = if is_running {
                if step == Step::SignIn { tr("pill.code.agentSetup.signingIn").into() } else { tr("pill.code.agentSetup.running").into() }
            } else if let Some(error) = error {
                error.message.clone().into()
            } else {
                step_desc(step, cli, report).into()
            };
            let mut text = div()
                .flex_1()
                .min_w(px(0.))
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().text_size(px(13.)).font_weight(FontWeight::SEMIBOLD).text_color(t.text).child(step_title(step, cli)))
                .child(div().text_size(px(12.)).text_color(if error.is_some() { t.bad } else { t.muted }).child(desc));
            if error.is_some_and(|error| error.node_download) {
                text = text.child(
                    div().pt(px(4.)).child(
                        gpui_m3::Button::new(("setup-node-site", id_base + index), tr("pill.code.agentSetup.openNodeSite"))
                            .text()
                            .size(gpui_m3::ButtonSize::Small)
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                if let Err(error) = crate::launcher::shell_open(NODE_DOWNLOAD) {
                                    view.show_toast(error, cx);
                                }
                            })),
                    ),
                );
            }
            let mut row = div()
                .id(("agent-setup-step", id_base + index))
                .flex()
                .items_center()
                .gap(px(12.))
                .px(px(14.))
                .py(px(10.))
                .rounded(px(t.r_ctl.min(14.)))
                .bg(t.control)
                .child(mark)
                .child(text);
            if !done && !is_running {
                let label = match step {
                    Step::Node if report.node.is_some() => tr("pill.code.agentSetup.repair"),
                    Step::SignIn => tr("pill.code.setup.signIn"),
                    _ if error.is_some() => tr("pill.code.agentSetup.retry"),
                    _ => tr("pill.code.setup.install"),
                };
                row = row.child(
                    div().flex_none().child(
                        gpui_m3::Button::new(("agent-setup-run", id_base + index), label)
                            .tonal()
                            .size(gpui_m3::ButtonSize::Small)
                            .disabled(blocked || busy)
                            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.start_agent_setup(cli, Some(step), window, cx))),
                    ),
                );
            }
            list = list.child(row);
        }
        if missing {
            list = list.child(
                div().pt(px(4.)).flex().justify_end().child(
                    gpui_m3::Button::new(("agent-setup-all", id_base), tr("pill.code.agentSetup.installAll"))
                        .filled()
                        .size(gpui_m3::ButtonSize::Small)
                        .icon("import")
                        .disabled(busy)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.start_agent_setup(cli, None, window, cx))),
                ),
            );
        }
        Some(list.into_any_element())
    }
}

fn step_title(step: Step, cli: &str) -> String {
    match step {
        Step::Node => tr("pill.code.agentSetup.nodeTitle").to_string(),
        Step::Cli => cli_name(cli).to_string(),
        Step::SignIn => tf("pill.code.agentSetup.signInTitle", &[("name", &agent_name(cli))]),
    }
}

fn step_desc(step: Step, cli: &str, report: &Report) -> String {
    match step {
        Step::Node if report.npm_missing(cli) => tf("pill.code.agentSetup.nodeNoNpm", &[("cli", &cli_name(cli))]),
        Step::Node if needs_npm(cli) => tf("pill.code.agentSetup.nodeDescNpm", &[("cli", &cli_name(cli))]),
        Step::Node => tr("pill.code.agentSetup.nodeDesc").to_string(),
        Step::Cli if report.desktop => {
            tf("pill.code.agentSetup.desktopOnly", &[("app", &app_name(cli).unwrap_or_default()), ("cli", &cli_name(cli))])
        }
        Step::Cli => tr("pill.code.agentSetup.cliDesc").to_string(),
        Step::SignIn if report.signed_in.is_none() => tr("pill.code.agentSetup.signInMaybe").to_string(),
        Step::SignIn => tr("pill.code.agentSetup.signInDesc").to_string(),
    }
}

/// La línea de estado de un agente sin instalar en la pestaña Agentes: dice si lo que hay
/// es la app de escritorio.
pub(super) fn missing_status(cli: &str, report: Option<&Report>) -> Option<String> {
    report.filter(|report| !report.cli && report.desktop)?;
    Some(tf("pill.code.agentSetup.desktopShort", &[("app", &app_name(cli).unwrap_or_default())]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(node: bool, npm: bool, cli: bool, signed_in: Option<bool>) -> Report {
        Report {
            node: node.then(|| PathBuf::from("/x/node")),
            npm: npm.then(|| PathBuf::from("/x/npm")),
            cli,
            desktop: false,
            signed_in,
        }
    }

    #[test]
    fn los_pasos_van_en_orden() {
        // Sin Node, Node primero aunque falte todo.
        assert_eq!(next_step(&report(false, false, false, None), "codex", false), Some(Step::Node));
        // Node sin npm: para Codex (npm) hay que repararlo; Claude no lo necesita.
        assert_eq!(next_step(&report(true, false, false, None), "codex", false), Some(Step::Node));
        assert_eq!(next_step(&report(true, false, false, None), "claude", false), Some(Step::Cli));
        // Con el CLI ya instalado, npm no importa.
        assert_eq!(next_step(&report(true, false, true, Some(true)), "codex", false), None);
        // La sesión: si se sabe que falta, o si el CLI recién se instaló.
        assert_eq!(next_step(&report(true, true, true, Some(false)), "codex", false), Some(Step::SignIn));
        assert_eq!(next_step(&report(true, true, true, None), "grok", false), None);
        assert_eq!(next_step(&report(true, true, true, None), "grok", true), Some(Step::SignIn));
        // Antigravity no tiene comando de sesión.
        assert_eq!(next_step(&report(true, true, true, None), "agy", true), None);
        assert!(!steps("agy").contains(&Step::SignIn));
    }

    #[test]
    fn npm_va_por_la_ruta_junto_a_node() {
        let npm = if cfg!(windows) { PathBuf::from(r"C:\Program Files\nodejs\npm.cmd") } else { PathBuf::from("/Users/x/.local/node/bin/npm") };
        let line = install_body("codex", Some(&npm)).unwrap();
        assert!(line.contains(&npm.display().to_string()), "{line}");
        assert!(line.contains("install -g @openai/codex"), "{line}");
        // Sin npm conocido, nunca el `npm` pelado en Windows (caería en npm.ps1).
        let bare = install_body("codex", None).unwrap();
        if cfg!(windows) {
            assert!(bare.contains("'npm.cmd'"), "{bare}");
        }
        // Lo que no va por npm queda igual.
        assert_eq!(install_body("claude", Some(&npm)).as_deref(), install_line("claude"));
    }

    #[test]
    fn el_script_deja_su_resultado() {
        let marker = PathBuf::from("/tmp/atic setup/codex-cli.done");
        let script = wrap("echo hola", &marker);
        assert!(script.contains("echo hola"));
        assert!(script.contains("atic setup/codex-cli.done") || script.contains(r"atic setup\codex-cli.done"));
    }

    #[test]
    fn el_resultado_se_lee_con_o_sin_bom() {
        let dir = std::env::temp_dir().join(format!("atic-setup-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("x.done");
        assert_eq!(read_marker(&marker), None);
        std::fs::write(&marker, "\u{feff}0\r\n").unwrap();
        assert_eq!(read_marker(&marker), Some(0));
        std::fs::write(&marker, "1603\n").unwrap();
        assert_eq!(read_marker(&marker), Some(1603));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn las_comillas_no_se_rompen() {
        assert_eq!(ps_quote("C:\\Users\\O'Brien"), "'C:\\Users\\O''Brien'");
        assert_eq!(sh_quote("/Users/o'brien"), "'/Users/o'\\''brien'");
    }
}
