//! Dos (y tres) celulares de verdad, en el mismo proceso, hablando por iroh.

use std::time::Duration;

use atic_sync::{
    generate_secret_key,
    peer::{BuzzStep, PeerEvent, Peers},
    phone::{Phone, PhoneConfig},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

struct Cel {
    // `peers` antes que `phone`: se suelta primero.
    peers: Peers,
    _phone: Phone,
    events: UnboundedReceiver<PeerEvent>,
}

async fn cel(name: &str) -> Cel {
    let phone = Phone::start(PhoneConfig { secret_key: generate_secret_key(), device_name: name.into(), inbox: None }).await.unwrap();
    let (tx, events) = unbounded_channel();
    let peers = Peers::start(phone.endpoint(), name.into(), Vec::new(), tx);
    Cel { peers, _phone: phone, events }
}

async fn next(rx: &mut UnboundedReceiver<PeerEvent>, what: &str) -> PeerEvent {
    tokio::time::timeout(Duration::from_secs(30), rx.recv())
        .await
        .unwrap_or_else(|_| panic!("no llegó: {what}"))
        .expect("el canal se cerró")
}

/// Ninguno más en un rato corto.
async fn nothing(rx: &mut UnboundedReceiver<PeerEvent>) {
    if let Ok(Some(e)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        panic!("llegó algo de más: {e:?}");
    }
}

/// `a` muestra el código y `b` lo escanea. Devuelve los ids (de `a`, de `b`).
async fn vincular(a: &mut Cel, b: &mut Cel) -> (String, String) {
    let ticket = a.peers.ticket().await;
    let en_b = b.peers.pair(&ticket).await.expect("se vincula");
    let PeerEvent::Paired { peer: en_a } = next(&mut a.events, "vínculo en a").await else { panic!("esperaba Paired") };
    assert!(matches!(next(&mut b.events, "vínculo en b").await, PeerEvent::Paired { .. }));
    assert_eq!(en_b.name, "Celular A");
    assert_eq!(en_a.name, "Celular B");
    (en_b.id(), en_a.id())
}

#[tokio::test(flavor = "multi_thread")]
async fn se_vinculan_y_se_escriben_en_los_dos_sentidos() {
    let mut a = cel("Celular A").await;
    let mut b = cel("Celular B").await;
    let (id_a, id_b) = vincular(&mut a, &mut b).await;

    b.peers.send_text(&id_a, "m1".into(), "hola desde b".into(), 1).await.expect("llega a a");
    assert_eq!(
        next(&mut a.events, "texto en a").await,
        PeerEvent::Text { peer_id: id_b.clone(), id: "m1".into(), text: "hola desde b".into(), sent_at_ms: 1 }
    );

    // Al revés: `a` llama a `b` con la dirección que `b` mandó al vincularse.
    a.peers.send_text(&id_b, "m2".into(), "hola desde a".into(), 2).await.expect("llega a b");
    assert!(matches!(next(&mut b.events, "texto en b").await, PeerEvent::Text { id, .. } if id == "m2"));

    let steps = vec![BuzzStep { on_ms: 120, off_ms: 80, amplitude: 255 }, BuzzStep { on_ms: 300, off_ms: 0, amplitude: 120 }];
    a.peers.send_buzz(&id_b, "z1".into(), steps.clone(), 3).await.expect("zumbido llega");
    assert_eq!(
        next(&mut b.events, "zumbido en b").await,
        PeerEvent::Buzz { peer_id: id_a.clone(), id: "z1".into(), steps, sent_at_ms: 3 }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn un_reintento_no_llega_dos_veces() {
    let mut a = cel("Celular A").await;
    let mut b = cel("Celular B").await;
    let (id_a, _) = vincular(&mut a, &mut b).await;

    b.peers.send_text(&id_a, "m1".into(), "una vez".into(), 1).await.unwrap();
    next(&mut a.events, "el primero").await;
    // Como si el Ack se hubiera perdido: se manda igual y se confirma, pero no se repite.
    b.peers.send_text(&id_a, "m1".into(), "una vez".into(), 1).await.expect("el reintento se confirma");
    nothing(&mut a.events).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn un_codigo_sirve_una_sola_vez_y_un_desconocido_no_entra() {
    let mut a = cel("Celular A").await;
    let b = cel("Celular B").await;
    let c = cel("Celular C").await;
    let ticket = a.peers.ticket().await;
    b.peers.pair(&ticket).await.unwrap();
    next(&mut a.events, "vínculo con b").await;

    let err = c.peers.pair(&ticket).await.expect_err("el código ya se usó");
    assert!(err.to_string().contains("venció o ya se usó"), "{err:#}");
    nothing(&mut a.events).await;

    // Aunque supiera el id de `a`, `c` no está vinculado: ni siquiera lo conoce para mandarle.
    let id_a = b.peers.known()[0].id();
    assert!(c.peers.send_text(&id_a, "x".into(), "hola".into(), 1).await.is_err());
    drop(b);
}

#[tokio::test(flavor = "multi_thread")]
async fn desvincular_avisa_al_otro() {
    let mut a = cel("Celular A").await;
    let mut b = cel("Celular B").await;
    let (id_a, id_b) = vincular(&mut a, &mut b).await;

    b.peers.unpair(&id_a).await;
    assert_eq!(next(&mut a.events, "desvínculo en a").await, PeerEvent::Unpaired { peer_id: id_b.clone() });
    assert!(a.peers.known().is_empty());
    // Ninguno de los dos puede seguir mandando.
    let err = a.peers.send_text(&id_b, "m".into(), "¿sigues?".into(), 1).await.expect_err("ya no está vinculado");
    assert!(err.to_string().contains("no está vinculado"), "{err:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn no_se_puede_vincular_consigo_mismo() {
    let a = cel("Celular A").await;
    let ticket = a.peers.ticket().await;
    assert!(a.peers.pair(&ticket).await.is_err());
}
