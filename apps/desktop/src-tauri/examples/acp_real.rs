//! Prueba manual del adaptador ACP contra un agente instalado de verdad.
//!
//! No es un test: lanza un proceso externo, tarda, gasta tokens y depende de
//! qué tengas instalado. Vive como ejemplo para poder correrlo a mano cuando se
//! toca el adaptador, sin ensuciar `cargo test`.
//!
//! ```bash
//! cargo run -p atic-desktop --example acp_real -- opencode "lee README.md y di su primera linea"
//! ```
//!
//! Imprime cada `AgentDelta` tal como lo recibiría la interfaz.

use std::sync::mpsc;

use atic_desktop_lib::agents::model::{AgentDelta, ItemKind};
use atic_desktop_lib::agents::{acp, AgentBackend, StartOptions};

fn main() {
    let mut args = std::env::args().skip(1);
    let cual = args.next().unwrap_or_else(|| "opencode".into());
    let prompt = args
        .next()
        .unwrap_or_else(|| "Responde solo: hola".to_string());

    let backend: &dyn AgentBackend = match cual.as_str() {
        "cursor" => &acp::CURSOR,
        _ => &acp::OPENCODE,
    };

    if !backend.is_available() {
        println!(
            "{} no está instalado en esta máquina.",
            backend.display_name()
        );
        return;
    }
    println!("--- {} disponible", backend.display_name());

    let (tx, rx) = mpsc::channel();
    let mut session = backend
        .start(
            StartOptions {
                cwd: std::env::current_dir()
                    .ok()
                    .map(|p| p.display().to_string()),
                ..Default::default()
            },
            Box::new(move |d| {
                let _ = tx.send(d);
            }),
        )
        .expect("no arrancó");

    session.send(&prompt).expect("no se pudo mandar");

    // Hasta el fin de turno, o hasta que deje de llegar nada por un rato.
    let limite = std::time::Duration::from_secs(120);
    loop {
        match rx.recv_timeout(limite) {
            Ok(delta) => {
                describe(&delta);
                if matches!(
                    delta,
                    AgentDelta::TurnEnd { .. } | AgentDelta::Failed { .. }
                ) {
                    break;
                }
            }
            Err(_) => {
                println!("--- se quedó callado");
                break;
            }
        }
    }
    session.stop();
    println!("--- fin");
}

/// Una línea por delta, con lo justo para leerlo de un vistazo.
fn describe(d: &AgentDelta) {
    match d {
        AgentDelta::TurnStart { turn } => println!("turn.start  {turn}"),
        AgentDelta::ItemAdd { item, .. } => {
            let que = match &item.kind {
                ItemKind::Message { role, .. } => format!("message({role:?})"),
                ItemKind::Reasoning { .. } => "reasoning".into(),
                ItemKind::Tool {
                    title,
                    tool_kind,
                    status,
                    ..
                } => format!("tool[{tool_kind:?}/{status:?}] {title}"),
                ItemKind::Plan { entries } => format!("plan({} pasos)", entries.len()),
                ItemKind::Permission { tool, .. } => format!("PERMISO {tool}"),
                ItemKind::Notice { text } => format!("notice {text}"),
            };
            println!("item.add    #{} {que}", item.id);
        }
        AgentDelta::ItemChunk { item, text } => {
            println!(
                "item.chunk  #{item} {:?}",
                text.chars().take(40).collect::<String>()
            )
        }
        AgentDelta::ItemPatch { item, patch } => println!(
            "item.patch  #{item} {}",
            serde_json::to_string(patch).unwrap_or_default()
        ),
        AgentDelta::ThreadPatch { patch } => println!(
            "thread.patch {}",
            serde_json::to_string(patch).unwrap_or_default()
        ),
        AgentDelta::TurnEnd { status, .. } => println!("turn.end    {status:?}"),
        AgentDelta::Failed { message } => println!("FAILED      {message}"),
    }
}
