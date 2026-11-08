//! Prototipo para probar el mouse y el teclado compartidos entre dos
//! computadores, sin compilar la pill.
//!
//! ```text
//! atic-kvm recibir                    # en el equipo sin mouse (el Mac): espera y muestra códigos
//! atic-kvm vincular                   # en el equipo con el mouse (el PC): elige el otro y escribe su código
//! atic-kvm compartir derecha [Mac]    # en el PC: el Mac está a la derecha; lleva el cursor a ese borde
//! atic-kvm equipos                    # los equipos vinculados
//! ```
//!
//! Ctrl+Alt+Esc trae el cursor de vuelta si algo se traba. `ATIC_KVM_DIR`
//! cambia la carpeta de datos (para dos instancias en el mismo equipo).

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use atic_kvm::link::{Node, Session, PAIR_ALPN};
use atic_kvm::proto::{read_frame, write_frame};
use atic_kvm::source::Grab;
use atic_kvm::target::Target;
use atic_kvm::{sys, Msg, Os, Side};
use iroh::EndpointId;

const USAGE: &str = "Uso:
  atic-kvm recibir                  Espera al otro equipo (y muestra el código para vincular)
  atic-kvm vincular                 Busca equipos en la red y se vincula con uno
  atic-kvm compartir <lado> [nombre]  Comparte este mouse y teclado; <lado>: izquierda, derecha, arriba o abajo
  atic-kvm equipos                  Los equipos vinculados";

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
    sys::prepare();
    let node = Node::start(&data_dir()?, computer_name()).await?;
    let result = match command {
        "recibir" | "serve" => receive(node.clone()).await,
        "vincular" | "pair" => pair(node.clone()).await,
        "compartir" | "share" => {
            let side = args.get(1).and_then(|s| Side::parse(s)).context(USAGE)?;
            share(node.clone(), side, args.get(2).cloned()).await
        }
        "equipos" | "peers" => {
            let peers = node.peers();
            if peers.is_empty() {
                println!("No hay equipos vinculados. Usa «atic-kvm vincular».");
            }
            for (id, p) in peers {
                println!("{}  ({:?})  {}", p.name, p.os, id.fmt_short());
            }
            Ok(())
        }
        _ => Err(anyhow::anyhow!("{USAGE}")),
    };
    node.endpoint.close().await;
    result
}

fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("ATIC_KVM_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(atic_core::AppDirs::new()?.data_dir().join("kvm"))
}

fn hello(node: &Node) -> Msg {
    Msg::Hello { name: node.name.clone(), os: Os::current(), screens: sys::screens().0 }
}

// ---------------------------------------------------------------- recibir

async fn receive(node: Arc<Node>) -> Result<()> {
    println!("«{}» espera conexiones ({}).", node.name, node.id().fmt_short());
    println!("En el otro equipo: «atic-kvm vincular» y después «atic-kvm compartir <lado>».");
    while let Some(incoming) = node.endpoint.accept().await {
        let node = node.clone();
        tokio::spawn(async move {
            let conn = match incoming.accept() {
                Ok(accepting) => match accepting.await {
                    Ok(conn) => conn,
                    Err(e) => return tracing::debug!("conexión fallida: {e:#}"),
                },
                Err(e) => return tracing::debug!("conexión rechazada: {e:#}"),
            };
            if conn.alpn() == PAIR_ALPN {
                let shown = |name: &str, code: &str| {
                    println!();
                    println!("  «{name}» quiere vincularse. Escribe este código allá:  {code}");
                    println!();
                };
                match node.serve_pair(conn, shown).await {
                    Ok(peer) => println!("Vinculado con «{}».", peer.name),
                    Err(e) => println!("No se vinculó: {e:#}"),
                }
                return;
            }
            match node.accept_session(conn, hello(&node)).await {
                Ok(session) => serve_session(session).await,
                Err(e) => println!("{e:#}"),
            }
        });
    }
    Ok(())
}

async fn serve_session(mut s: Session) {
    let Msg::Hello { name, os, .. } = s.hello.clone() else { return };
    println!("«{name}» conectado: su mouse y su teclado pueden pasar a este equipo.");
    let mut target = Target::new(os);
    loop {
        match read_frame::<_, Msg>(&mut s.recv).await {
            Ok(Some(msg)) => {
                if let Some(reply) = target.handle(msg) {
                    if write_frame(&mut s.send, &reply).await.is_err() {
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                tracing::debug!("la sesión terminó: {e:#}");
                break;
            }
        }
    }
    target.release_all();
    println!("«{name}» se desconectó.");
}

// ---------------------------------------------------------------- vincular

async fn pair(node: Arc<Node>) -> Result<()> {
    println!("Buscando equipos con «atic-kvm recibir» en la red…");
    let found = node.nearby(Duration::from_secs(4)).await;
    if found.is_empty() {
        bail!("No encontré ninguno. ¿Está corriendo «atic-kvm recibir» en el otro, en la misma red?");
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
    println!("Pidiendo el vínculo a «{name}»…");
    let ask = move || prompt(&format!("Escribe el código que aparece en «{name}»: "));
    let peer = node.pair(id, ask).await?;
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

// ---------------------------------------------------------------- compartir

fn pick_peer(node: &Node, query: Option<String>) -> Result<(EndpointId, String)> {
    let peers = node.peers();
    let matches: Vec<_> = match &query {
        Some(q) => peers.into_iter().filter(|(_, p)| p.name.to_lowercase().contains(&q.to_lowercase())).collect(),
        None => peers,
    };
    match matches.as_slice() {
        [] if query.is_some() => bail!("No hay un equipo vinculado con ese nombre («atic-kvm equipos»)."),
        [] => bail!("No hay equipos vinculados. Usa «atic-kvm vincular»."),
        [(id, p)] => Ok((*id, p.name.clone())),
        _ => bail!("Hay varios equipos vinculados: di cuál (atic-kvm compartir <lado> <nombre>)."),
    }
}

async fn share(node: Arc<Node>, side: Side, query: Option<String>) -> Result<()> {
    let (id, name) = pick_peer(&node, query)?;
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let grab = Arc::new(Grab::new(side, sys::screens(), tx));
    sys::start_capture(grab.clone())?;
    // Si se conecta o se mueve un monitor.
    {
        let grab = grab.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                grab.set_screens(sys::screens());
            }
        });
    }
    // Al cortar con Ctrl+C, el cursor vuelve antes de salir.
    {
        let grab = grab.clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            grab.set_connected(false);
            std::process::exit(0);
        });
    }
    let edge = match side {
        Side::Left => "izquierdo",
        Side::Right => "derecho",
        Side::Top => "de arriba",
        Side::Bottom => "de abajo",
    };
    loop {
        match node.connect(id, hello(&node)).await {
            Ok(session) => {
                // Lo que quedó de una sesión anterior no se manda.
                while rx.try_recv().is_ok() {}
                grab.set_connected(true);
                println!("Conectado con «{name}». Lleva el cursor al borde {edge} de la pantalla. Ctrl+Alt+Esc lo trae de vuelta.");
                let end = run_source(session, &mut rx, &grab).await;
                grab.set_connected(false);
                println!("Se cortó la conexión con «{name}»: {end:#}. Reintentando…");
            }
            Err(e) => println!("No se pudo conectar con «{name}»: {e:#}. Reintentando en 3 s…"),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
}

/// Manda lo que captura el gancho y atiende lo que devuelve el otro, hasta que se corte.
async fn run_source(s: Session, rx: &mut tokio::sync::mpsc::UnboundedReceiver<Msg>, grab: &Arc<Grab>) -> anyhow::Error {
    let Session { conn, mut send, mut recv, .. } = s;
    // Leer va en su propia tarea: cortar una lectura a medias perdería el mensaje.
    let mut reader = {
        let grab = grab.clone();
        tokio::spawn(async move {
            loop {
                match read_frame::<_, Msg>(&mut recv).await {
                    Ok(Some(Msg::Back { along })) => grab.back(along),
                    Ok(Some(_)) => {}
                    Ok(None) => return anyhow::anyhow!("el otro equipo cerró"),
                    Err(e) => return e,
                }
            }
        })
    };
    let end = loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { break anyhow::anyhow!("se detuvo la captura") };
                if let Err(e) = write_frame(&mut send, &msg).await {
                    break e;
                }
            }
            done = &mut reader => break done.unwrap_or_else(|e| anyhow::anyhow!("{e}")),
        }
    };
    reader.abort();
    conn.close(0u32.into(), b"");
    end
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
