//! Prototipo para probar el mouse y el teclado compartidos entre dos
//! computadores, sin compilar la pill.
//!
//! ```text
//! atic-kvm recibir                    # espera; si el otro comparte, comparte de vuelta solo
//! atic-kvm vincular [id]              # elige el otro de la red (o por su id) y escribe su código
//! atic-kvm compartir derecha [Mac]    # el Mac está a la derecha; lleva el cursor a ese borde
//! atic-kvm equipos                    # los equipos vinculados
//! atic-kvm olvidar <nombre>           # deja de aceptar a un equipo
//! ```
//!
//! Los dos sentidos funcionan a la vez, y el portapapeles se comparte.
//! Ctrl+Alt+Esc trae el cursor de vuelta si algo se traba. `ATIC_KVM_DIR`
//! cambia la carpeta de datos (para dos instancias en el mismo equipo).

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use atic_kvm::files::Purpose;
use atic_kvm::{EndpointId, Event, Service, Share, Side};
use tokio::sync::mpsc;

const USAGE: &str = "Uso:
  atic-kvm recibir                    Espera al otro equipo (y comparte de vuelta si el otro comparte)
  atic-kvm vincular [id]              Busca equipos en la red (o usa el id que muestra «recibir») y se vincula
  atic-kvm compartir <lado> [nombre]  Comparte este mouse y teclado; <lado>: izquierda, derecha, arriba o abajo
  atic-kvm enviar <archivo>... [--a nombre]  Manda archivos o carpetas a las Descargas del otro
  atic-kvm equipos                    Los equipos vinculados
  atic-kvm olvidar <nombre>           Deja de aceptar a un equipo";

/// Lo que «recibir» acepta pedidos de vínculo desde que arranca.
const PAIR_WINDOW: Duration = Duration::from_secs(10 * 60);

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "atic_kvm=info,error".into()))
        .with_target(false)
        .init();
    if let Err(e) = run().await {
        eprintln!("Error: {e:#}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("recibir");
    if matches!(command, "-h" | "--help" | "ayuda") {
        println!("{USAGE}");
        return Ok(());
    }
    let (events, rx) = mpsc::unbounded_channel();
    let service = Arc::new(Service::start(&data_dir()?, computer_name(), events).await?);
    match command {
        "recibir" | "serve" => {
            // Quien lo corre a mano está vinculando: se aceptan pedidos por un rato.
            service.open_pairing(PAIR_WINDOW);
            println!("«{}» espera conexiones (y acepta vínculos por {} minutos).", service.name(), PAIR_WINDOW.as_secs() / 60);
            println!("En el otro equipo: «atic-kvm vincular» y después «atic-kvm compartir <lado>».");
            println!("Si no lo encuentra en la red: atic-kvm vincular {}", service.id());
            run_forever(service, rx, true).await
        }
        "vincular" | "pair" => pair(&service, args.get(1).cloned()).await,
        "compartir" | "share" => {
            let side = args.get(1).and_then(|s| Side::parse(s)).context(USAGE)?;
            let (peer, name) = pick_peer(&service, args.get(2).cloned())?;
            service.share(Some(Share { peer, side }))?;
            println!("Compartiendo con «{name}», que está {}. Ctrl+Alt+Esc trae el cursor de vuelta.", side_name(side));
            run_forever(service, rx, false).await
        }
        "enviar" | "send" => {
            let mut paths: Vec<PathBuf> = Vec::new();
            let mut to = None;
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                if arg == "--a" || arg == "--to" {
                    to = rest.next().cloned();
                } else {
                    paths.push(PathBuf::from(arg));
                }
            }
            if paths.is_empty() {
                bail!("{USAGE}");
            }
            let (peer, name) = pick_peer(&service, to)?;
            println!("Conectando con «{name}»…");
            run_send(service, rx, peer, paths).await
        }
        "equipos" | "peers" => {
            let peers = service.peers();
            if peers.is_empty() {
                println!("No hay equipos vinculados. Usa «atic-kvm vincular».");
            }
            for (id, p) in peers {
                println!("{}  ({:?})  {}", p.name, p.os, id.fmt_short());
            }
            Ok(())
        }
        "olvidar" | "forget" => {
            let (peer, name) = pick_peer(&service, Some(args.get(1).cloned().context(USAGE)?))?;
            service.forget(&peer)?;
            println!("Olvidado «{name}».");
            Ok(())
        }
        _ => bail!("{USAGE}"),
    }
}

/// Muestra lo que pasa hasta Ctrl+C. `share_back`: si el otro comparte con
/// este equipo, compartir de vuelta por el lado que dice.
async fn run_forever(service: Arc<Service>, mut rx: mpsc::UnboundedReceiver<Event>, share_back: bool) -> Result<()> {
    {
        let service = service.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            // El cursor vuelve antes de salir.
            service.stop();
            tokio::time::sleep(Duration::from_millis(200)).await;
            std::process::exit(0);
        });
    }
    let name_of = |id: &EndpointId| service.peers().into_iter().find(|(p, _)| p == id).map(|(_, p)| p.name).unwrap_or_else(|| id.fmt_short().to_string());
    while let Some(event) = rx.recv().await {
        match event {
            Event::PairCode { name, code } => {
                println!();
                println!("  «{name}» quiere vincularse. Escribe este código allá:  {code}");
                println!();
            }
            Event::Paired { name, .. } => println!("Vinculado con «{name}»."),
            Event::PairFailed { reason } => println!("No se vinculó: {reason}"),
            Event::Connected { name, .. } => println!("«{name}» conectado."),
            Event::Disconnected { name, .. } => println!("«{name}» se desconectó."),
            Event::PeerLayout { id, side } => {
                // Si ya comparte con ese mismo equipo, sigue el lado nuevo (el otro se reacomodó).
                let ours = service.sharing().is_none_or(|s| s.peer == id && s.side != side);
                if share_back && ours {
                    match service.share(Some(Share { peer: id, side })) {
                        Ok(()) => println!("También compartes este mouse y teclado con «{}», que está {}.", name_of(&id), side_name(side)),
                        Err(e) => println!("Para usar este mouse y teclado en «{}» falta un permiso: {e:#}", name_of(&id)),
                    }
                }
            }
            Event::Away { name, .. } => println!("→ el cursor está en «{name}»"),
            Event::Home => println!("← el cursor volvió"),
            Event::Controlled { name, on, .. } => {
                println!("{}", if on { format!("→ «{name}» está usando este equipo") } else { format!("← «{name}» dejó de usar este equipo") })
            }
            Event::Clipboard { name, image, .. } => println!("{} copiada en «{name}»", if image { "Imagen" } else { "Texto" }),
            Event::FilesSending { name, count, bytes, .. } => println!("→ mandando {count} archivo(s) ({}) a «{name}»", size(bytes)),
            Event::FilesReceived { name, paths, purpose, .. } => {
                let donde = if purpose == Purpose::Clipboard { "listos para pegar" } else { "en Descargas" };
                println!("← de «{name}», {donde}:");
                for p in paths {
                    println!("   {}", p.display());
                }
            }
            Event::FilesSent { name, count, .. } => println!("→ {count} archivo(s) llegaron a «{name}»"),
            Event::FilesFailed { reason } => println!("No se pudieron pasar los archivos: {reason}"),
            Event::Error(e) => println!("{e}"),
        }
    }
    Ok(())
}

/// Para `enviar`: abre la sesión, manda y sale cuando terminó.
async fn run_send(service: Arc<Service>, mut rx: mpsc::UnboundedReceiver<Event>, peer: EndpointId, paths: Vec<PathBuf>) -> Result<()> {
    service.connect(peer).await?;
    service.send_files(peer, paths);
    while let Some(event) = rx.recv().await {
        match event {
            Event::FilesSending { count, bytes, .. } => println!("Mandando {count} archivo(s), {}…", size(bytes)),
            Event::FilesSent { name, .. } => {
                println!("Listo: quedaron en las Descargas de «{name}».");
                break;
            }
            Event::FilesFailed { reason } => bail!("{reason}"),
            _ => {}
        }
    }
    service.stop();
    Ok(())
}

fn size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b.div_ceil(1024)),
    }
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Left => "a la izquierda",
        Side::Right => "a la derecha",
        Side::Top => "arriba",
        Side::Bottom => "abajo",
    }
}

fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_KVM_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(atic_core::AppDirs::new()?.data_dir().join("kvm"))
}

async fn pair(service: &Service, id: Option<String>) -> Result<()> {
    let (id, name) = match id {
        // Con el id no hace falta la red local: iroh lo encuentra por internet (o por relay).
        Some(id) => (id.trim().parse::<EndpointId>().context("ese id no es válido")?, "el otro equipo".to_string()),
        None => {
            println!("Buscando equipos con «atic-kvm recibir» en la red…");
            let found = service.nearby(Duration::from_secs(4)).await;
            if found.is_empty() {
                bail!(
                    "No encontré ninguno. ¿Está corriendo «atic-kvm recibir» en el otro, en la misma red? Si el firewall bloquea la búsqueda, usa «atic-kvm vincular <id>» con el id que muestra el otro."
                );
            }
            for (i, (id, name)) in found.iter().enumerate() {
                println!("  {}. {name}  ({})", i + 1, id.fmt_short());
            }
            let pick = if found.len() == 1 {
                0
            } else {
                let line = prompt("¿Con cuál? (número): ")?;
                line.trim().parse::<usize>().ok().filter(|n| (1..=found.len()).contains(n)).context("número inválido")? - 1
            };
            let (id, name) = found[pick].clone();
            (id, format!("«{name}»"))
        }
    };
    println!("Pidiendo el vínculo a {name}…");
    let question = format!("Escribe el código que aparece en {name}: ");
    let code = async move { tokio::task::spawn_blocking(move || prompt(&question)).await? };
    let peer = service.pair(id, code).await?;
    println!("Vinculado con «{}». Ahora: atic-kvm compartir <lado>", peer.name);
    Ok(())
}

fn prompt(text: &str) -> Result<String> {
    print!("{text}");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    Ok(line)
}

fn pick_peer(service: &Service, query: Option<String>) -> Result<(EndpointId, String)> {
    let peers = service.peers();
    let matches: Vec<_> = match &query {
        Some(q) => peers.into_iter().filter(|(_, p)| p.name.to_lowercase().contains(&q.to_lowercase())).collect(),
        None => peers,
    };
    match matches.as_slice() {
        [] if query.is_some() => bail!("No hay un equipo vinculado con ese nombre («atic-kvm equipos»)."),
        [] => bail!("No hay equipos vinculados. Usa «atic-kvm vincular»."),
        [(id, p)] => Ok((*id, p.name.clone())),
        _ => bail!("Hay varios equipos vinculados: di cuál por su nombre."),
    }
}

/// El nombre del equipo: `COMPUTERNAME` en Windows; en macOS, el de Compartir.
fn computer_name() -> String {
    if let Some(name) = std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()) {
        return name;
    }
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("/usr/sbin/scutil").args(["--get", "ComputerName"]).output() {
        let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if out.status.success() && !name.is_empty() {
            return name;
        }
    }
    "Computador".into()
}
