//! Un celular de mentira para probar el chat entre celulares sin un segundo Android.
//!
//!   cargo run -p atic-sync --example fake_peer -- 'aticp1:…'   # vincularse
//!   cargo run -p atic-sync --example fake_peer -- --again       # volver, ya vinculado
//!
//! Se vincula con el código que muestra el celular de verdad, imprime lo que llega y manda
//! cada línea que se escribe. `/zumbido` manda un zumbido de prueba. Guarda su clave y el
//! vínculo en `$TMPDIR/atic-fake-peer.json`, para probar lo que llega al volver.

use std::time::{SystemTime, UNIX_EPOCH};

use atic_sync::{
    generate_secret_key,
    peer::{BuzzStep, PairedPeer, PeerEvent, Peers},
    phone::{Phone, PhoneConfig},
};
use tokio::io::{AsyncBufReadExt, BufReader};

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let arg = std::env::args().nth(1).expect("falta el código aticp1:… o --again");
    let state_path = std::env::temp_dir().join("atic-fake-peer.json");
    let saved: Option<(Vec<u8>, String)> = std::fs::read(&state_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let again = arg == "--again";
    let key: [u8; 32] = match (&saved, again) {
        (Some((k, _)), true) => k.as_slice().try_into()?,
        _ => generate_secret_key(),
    };
    let known = match (&saved, again) {
        (Some((_, json)), true) => vec![PairedPeer::from_json(json)?],
        _ => Vec::new(),
    };
    let phone = Phone::start(PhoneConfig { secret_key: key, device_name: "Mac de prueba".into(), inbox: None }).await?;
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    let peers = Peers::start(phone.endpoint(), "Mac de prueba".into(), known.clone(), tx);

    let other = match known.into_iter().next() {
        Some(p) => p,
        None => peers.pair(&arg).await?,
    };
    std::fs::write(&state_path, serde_json::to_vec(&(key.to_vec(), other.to_json()))?)?;
    let id = other.id();
    println!("{} con «{}»", if again { "DE VUELTA" } else { "VINCULADO" }, other.name);

    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            match event {
                PeerEvent::Text { text, .. } => println!("LLEGÓ: {text}"),
                PeerEvent::Buzz { steps, .. } => println!("LLEGÓ ZUMBIDO: {} golpes", steps.len()),
                PeerEvent::Unpaired { .. } => println!("DESVINCULADO por el otro"),
                PeerEvent::Paired { .. } => {}
            }
        }
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut n = 0;
    while let Some(line) = lines.next_line().await? {
        n += 1;
        let msg_id = format!("mac-{n}-{}", now_ms());
        let result = if line.trim() == "/zumbido" {
            let steps = vec![BuzzStep { on_ms: 150, off_ms: 100, amplitude: 255 }, BuzzStep { on_ms: 150, off_ms: 100, amplitude: 255 }, BuzzStep { on_ms: 400, off_ms: 0, amplitude: 180 }];
            peers.send_buzz(&id, msg_id, steps, now_ms()).await
        } else {
            peers.send_text(&id, msg_id, line, now_ms()).await
        };
        match result {
            Ok(()) => println!("ENTREGADO"),
            Err(e) => println!("NO LLEGÓ: {e:#}"),
        }
    }
    Ok(())
}
