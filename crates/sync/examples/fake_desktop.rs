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
//!
//! Para probar un celular con varios PCs, cada uno con su nombre (y su clave):
//!
//! ```text
//! ATIC_FAKE_NAME="PC de la oficina" cargo run --example fake_desktop
//! ```

use std::{path::PathBuf, sync::Arc, time::Duration};

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent, PhoneRequest},
    ActivityKind, AgentActivity, AgentCard, AgentStatus, Decision, PermissionAsk,
};
use qrcode::{render::unicode::Dense1x2, QrCode};
use tokio::sync::mpsc;

/// El nombre del PC; con otro nombre es otro PC (otra clave, otros pareados).
fn name() -> String {
    std::env::var("ATIC_FAKE_NAME").unwrap_or_else(|_| "PC de prueba".into())
}

static SENT_WELCOME: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// El último celular que habló, para los comandos de la terminal.
static LAST_PHONE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Comandos por la entrada estándar para probar lo que el PC le pide al celular:
/// `responder <key> <texto>`, `descartar <key>`, `contestar`, `colgar`.
fn read_commands(host: Arc<Desktop>) {
    std::thread::spawn(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            let Some(device) = LAST_PHONE.lock().unwrap().clone() else {
                println!("← Ningún celular conectado todavía");
                continue;
            };
            let mut parts = line.trim().splitn(3, ' ');
            let ok = match (parts.next(), parts.next(), parts.next()) {
                (Some("responder"), Some(key), Some(text)) => host.notice_reply(&device, key.into(), text.into()),
                (Some("descartar"), Some(key), _) => host.notice_dismiss(&device, key.into()),
                (Some("contestar"), ..) => host.call_action(&device, atic_sync::CallAction::Answer),
                (Some("colgar"), ..) => host.call_action(&device, atic_sync::CallAction::HangUp),
                _ => {
                    println!("← No entiendo {line:?}");
                    continue;
                }
            };
            println!("← {line}: {}", if ok { "mandado" } else { "el celular no lo sabe hacer o no está" });
        }
    });
}

fn state_dir() -> PathBuf {
    let base = std::env::temp_dir().join("atic-fake-desktop");
    match std::env::var("ATIC_FAKE_NAME") {
        Ok(n) => base.join(n.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>()),
        Err(_) => base,
    }
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
        name: name(),
        trusted: trusted.clone(),
    })
    .await?;
    let desktop = Arc::new(desktop);
    // Todo lo nuevo prendido, para probar el celular entero.
    desktop.enable(atic_sync::feature::all(), Some(state_dir().join("entrada")));
    println!("{}: {}", name(), desktop.id());
    println!("Celulares pareados: {}", trusted.len());

    desktop.publish_clip_history(vec![atic_sync::ClipItem {
        // Un id y un texto por PC: con dos PCs se ve si el celular reparte lo de uno al otro.
        id: format!("fake-{}", state_dir().file_name().unwrap().to_string_lossy()),
        text: format!("Texto de ejemplo del historial de {}", name()),
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

    read_commands(desktop.clone());
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
                    println!("→ Historial del celular: {} ítems, {} borrados", items.len(), deleted.len());
                    for item in items.iter().take(5) {
                        println!("   · {}: {:?}", item.id, item.text);
                    }
                }
                DesktopEvent::ClipImage { id, data, .. } => println!("→ Imagen {id} del celular: {} bytes", data.len()),
                DesktopEvent::ClipDelete { ids, .. } => println!("→ Borrados en el celular: {ids:?}"),
                DesktopEvent::Paired { device_id, device_name } => {
                    println!("→ Pareado: {device_name} ({device_id})");
                    save_trusted(&device_id);
                }
                DesktopEvent::Connected { device_id, device_name, features } => {
                    println!("→ Conectado: {device_name} (sabe: {})", features.join(", "));
                    *LAST_PHONE.lock().unwrap() = Some(device_id.clone());
                    // Una vez por ejecución, un archivo para probar que el celular recibe.
                    if features.iter().any(|f| f == atic_sync::feature::FILES)
                        && !SENT_WELCOME.swap(true, std::sync::atomic::Ordering::Relaxed)
                    {
                        let host = host.clone();
                        tokio::spawn(async move {
                            let path = state_dir().join(format!("Hola desde {}.txt", name()));
                            std::fs::write(&path, format!("Archivo de prueba mandado por {}.\n", name())).unwrap();
                            match host.send_file(&device_id, &path, "text/plain".into(), Default::default(), None).await {
                                Ok(()) => println!("→ Archivo de prueba mandado al celular"),
                                Err(e) => println!("→ No se pudo mandar el archivo: {e:#}"),
                            }
                        });
                    }
                }
                DesktopEvent::Phone { device_id, request } => match request {
                    PhoneRequest::Notice(n) => println!(
                        "→ Aviso del celular [{}]{}: {} · {}: {}",
                        n.key,
                        if n.can_reply { " (se puede responder)" } else { "" },
                        n.app_name,
                        n.title,
                        n.text
                    ),
                    PhoneRequest::NoticeRemoved { key } => println!("→ Aviso quitado: {key}"),
                    PhoneRequest::Call(call) => println!("→ Llamada: {call:?}"),
                    PhoneRequest::Prompt { agent_id, text } => {
                        println!("→ Para {agent_id}: {text:?}");
                        host.agent_output(&device_id, agent_id, vec![format!("> {text}"), "Entendido, voy.".into()], false);
                    }
                    PhoneRequest::Watch { agent_id } => {
                        println!("→ Mirar: {agent_id:?}");
                        if let Some(agent) = agent_id {
                            let host = host.clone();
                            tokio::spawn(async move {
                                host.agent_output(&device_id, agent.clone(), vec![format!("$ cargo test ({agent})")], true);
                                for i in 1..=20 {
                                    tokio::time::sleep(Duration::from_millis(800)).await;
                                    if !host.agent_output(&device_id, agent.clone(), vec![format!("test caso_{i} ... ok")], false) {
                                        break;
                                    }
                                }
                            });
                        }
                    }
                    PhoneRequest::File { header, path } => {
                        println!("→ Archivo del celular: {} ({} bytes, {:?}) en {}", header.name, header.size, header.kind, path.display())
                    }
                },
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
