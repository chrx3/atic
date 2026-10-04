//! Un escritorio de mentira para probar el celular sin tocar la app de Atic.
//!
//! ```text
//! cargo run --example fake_desktop
//! ```
//!
//! Muestra el QR de pareo en la terminal (y el texto, para pegarlo), y publica
//! un guion de agentes: uno trabaja, pide permiso y espera tu respuesta desde
//! el celular. La clave y los celulares pareados se guardan en el directorio
//! temporal, así que reiniciar no obliga a volver a parear.

use std::{path::PathBuf, sync::Arc, time::Duration};

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent},
    ActivityKind, AgentActivity, AgentCard, AgentStatus, Decision, PermissionAsk,
};
use qrcode::{render::unicode::Dense1x2, QrCode};
use tokio::sync::mpsc;

fn state_dir() -> PathBuf {
    std::env::temp_dir().join("atic-fake-desktop")
}

fn load_key() -> [u8; 32] {
    let path = state_dir().join("key");
    if let Ok(bytes) = std::fs::read(&path) {
        if let Ok(key) = bytes.try_into() {
            return key;
        }
    }
    let key = atic_sync::generate_secret_key();
    std::fs::create_dir_all(state_dir()).unwrap();
    std::fs::write(&path, key).unwrap();
    key
}

fn load_trusted() -> Vec<String> {
    std::fs::read_to_string(state_dir().join("trusted"))
        .map(|s| s.lines().map(str::to_string).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn save_trusted(id: &str) {
    let mut ids = load_trusted();
    if !ids.iter().any(|i| i == id) {
        ids.push(id.to_string());
    }
    std::fs::write(state_dir().join("trusted"), ids.join("\n")).unwrap();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into()))
        .init();

    let trusted = load_trusted();
    let (desktop, mut events) = Desktop::start(DesktopConfig {
        secret_key: load_key(),
        name: "PC de prueba".into(),
        trusted: trusted.clone(),
    })
    .await?;
    let desktop = Arc::new(desktop);
    println!("PC de prueba: {}", desktop.id());
    println!("Celulares pareados: {}", trusted.len());

    desktop.publish_clip_history(vec![atic_sync::ClipItem {
        id: "fake-1".into(),
        text: "Texto de ejemplo del historial del PC".into(),
        source_name: String::new(),
        copied_at_ms: 0,
        pinned: true,
        kind: Default::default(),
        thumb: None,
        width: 0,
        height: 0,
    }]);
    let ticket = desktop.pairing_ticket().await;
    let qr = QrCode::new(ticket.as_bytes())?;
    println!("\n{}", qr.render::<Dense1x2>().quiet_zone(true).build());
    println!("Ticket (vale 5 minutos):\n{ticket}\n");
    std::fs::write(state_dir().join("ticket"), &ticket)?;

    let (decisions_tx, decisions) = mpsc::unbounded_channel();
    let host = desktop.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                // Atic pregunta antes de aceptar; este ejemplo acepta solo para no frenar las pruebas.
                DesktopEvent::PairRequested { device_id, device_name } => {
                    println!("→ Pedido de pareo de {device_name}: aceptado automáticamente (solo en el ejemplo)");
                    host.approve_pairing(&device_id, true);
                }
                DesktopEvent::Clip { item, .. } => {
                    println!("→ Copiado desde {}: {:?}", item.source_name, item.text);
                    // Para probar el otro sentido: se lo devuelve en mayúsculas.
                    host.send_clip(item.text.to_uppercase(), item.copied_at_ms);
                }
                DesktopEvent::Command { command, .. } => println!("→ Comando: {command:?}"),
                DesktopEvent::FetchImage { device_id, id } => {
                    host.send_image(&device_id, id, "image/png".into(), None);
                }
                DesktopEvent::Image { data, .. } => println!("→ Imagen del celular: {} bytes", data.len()),
                DesktopEvent::PhoneMedia { media, .. } => println!("→ Suena en el celular: {media:?}"),
                DesktopEvent::ClipSync { items, deleted, .. } => {
                    println!("→ Historial del celular: {} ítems, {} borrados", items.len(), deleted.len())
                }
                DesktopEvent::ClipImage { id, data, .. } => println!("→ Imagen {id} del celular: {} bytes", data.len()),
                DesktopEvent::ClipDelete { ids, .. } => println!("→ Borrados en el celular: {ids:?}"),
                DesktopEvent::Paired { device_id, device_name } => {
                    println!("→ Pareado: {device_name} ({device_id})");
                    save_trusted(&device_id);
                }
                DesktopEvent::Connected { device_name, .. } => println!("→ Conectado: {device_name}"),
                DesktopEvent::Disconnected { .. } => println!("→ Desconectado"),
                DesktopEvent::Decide { agent_id, permission_id, decision, .. } => {
                    println!("→ Decisión: {decision:?} para {agent_id}/{permission_id}");
                    let _ = decisions_tx.send((permission_id, decision));
                }
                DesktopEvent::Answer { agent_id, permission_id, answers, .. } => {
                    println!("→ Respuestas para {agent_id}/{permission_id}: {answers:?}");
                    let _ = decisions_tx.send((permission_id, Decision::Allow));
                }
            }
        }
    });

    script(&desktop, decisions).await;
    Ok(())
}

fn agent(id: &str, backend: &str, name: &str, project: &str) -> AgentCard {
    AgentCard {
        id: id.into(),
        backend_id: backend.into(),
        backend_name: name.into(),
        project: project.into(),
        status: AgentStatus::Idle,
        activity: None,
        preview: None,
        permission: None,
    }
}

fn doing(kind: ActivityKind, detail: Option<&str>) -> Option<AgentActivity> {
    Some(AgentActivity { kind, detail: detail.map(str::to_string) })
}

/// El mismo guion que el `DemoFeed` del celular, pero desde el PC.
async fn script(desktop: &Desktop, mut decisions: mpsc::UnboundedReceiver<(String, Decision)>) {
    let sleep = |ms| tokio::time::sleep(Duration::from_millis(ms));
    let mut round = 0;
    loop {
        round += 1;
        let mut claude = agent("pc-claude", "claude-code", "Claude Code", "atic");
        let mut codex = agent("pc-codex", "codex", "Codex", "atic-android");

        claude.status = AgentStatus::Working;
        claude.activity = doing(ActivityKind::Thinking, None);
        desktop.publish(vec![claude.clone(), codex.clone()]);
        sleep(3_000).await;

        claude.activity = doing(ActivityKind::Editing, Some("desktop.rs"));
        codex.status = AgentStatus::Working;
        codex.activity = doing(ActivityKind::Reading, Some("IslandHub.kt"));
        desktop.publish(vec![claude.clone(), codex.clone()]);
        sleep(4_000).await;

        let permission_id = format!("pc-perm-{round}");
        claude.status = AgentStatus::Waiting;
        claude.activity = None;
        claude.preview = Some("Quiero correr las pruebas de sync antes de seguir.".into());
        claude.permission = Some(PermissionAsk {
            id: permission_id.clone(),
            title: "Ejecutar comando".into(),
            detail: Some("cargo test -p atic-sync".into()),
            can_allow_always: true,
            questions: Vec::new(),
        });
        desktop.publish(vec![claude.clone(), codex.clone()]);
        println!("Esperando la respuesta del celular…");

        let decision = loop {
            match decisions.recv().await {
                Some((id, decision)) if id == permission_id => break decision,
                Some(_) => continue,
                None => return,
            }
        };

        claude.permission = None;
        if decision == Decision::Deny {
            claude.status = AgentStatus::Ready;
            claude.preview = Some("Ok, no corro las pruebas.".into());
        } else {
            claude.status = AgentStatus::Working;
            claude.activity = doing(ActivityKind::Running, Some("cargo test"));
            desktop.publish(vec![claude.clone(), codex.clone()]);
            sleep(3_000).await;
            claude.status = AgentStatus::Ready;
            claude.activity = None;
            claude.preview = Some("Listo: 9 pruebas pasaron.".into());
        }
        desktop.publish(vec![claude.clone(), codex.clone()]);
        sleep(3_000).await;

        codex.status = AgentStatus::Ready;
        codex.activity = None;
        codex.preview = Some("Revisé el hub, sin cambios.".into());
        desktop.publish(vec![claude.clone(), codex.clone()]);
        sleep(8_000).await;

        claude.status = AgentStatus::Idle;
        codex.status = AgentStatus::Idle;
        desktop.publish(vec![claude, codex]);
        sleep(5_000).await;
    }
}
