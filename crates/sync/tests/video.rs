//! La pantalla del celular en el PC, de punta a punta: escritorio y celular de verdad,
//! pareados y hablando por iroh, con la conexión de video aparte (`atic/video/1`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent},
    feature, generate_secret_key,
    phone::{LinkStatus, Phone, PhoneConfig, PhoneEvent, Target},
    video::{flags, VideoControl, VideoDirection, VideoHello, VideoSource},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

async fn next<T: std::fmt::Debug>(rx: &mut UnboundedReceiver<T>, what: &str, pick: impl Fn(&T) -> bool) -> T {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = rx.recv().await.expect("el canal se cerró");
            if pick(&event) {
                return event;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no llegó: {what}"))
}

/// Un PC con la pantalla prendida y un celular pareado y conectado. Devuelve el id del PC.
async fn paired(screen: bool) -> (Desktop, Phone, String) {
    let features = if screen { vec![feature::SCREEN.to_string()] } else { vec![] };
    let (desktop, mut desk) = Desktop::start(DesktopConfig { secret_key: generate_secret_key(), name: "PC".into(), trusted: vec![], features, inbox: None })
        .await
        .unwrap();
    let ticket = desktop.pairing_ticket().await;
    let phone = Phone::start(PhoneConfig { secret_key: generate_secret_key(), device_name: "Celular".into(), inbox: None }).await.unwrap();
    let (tx, mut events) = unbounded_channel();
    let desktop_id = phone.connect(Target::from_ticket(&ticket).unwrap(), tx);
    let DesktopEvent::PairRequested { device_id, .. } = next(&mut desk, "pareo", |e| matches!(e, DesktopEvent::PairRequested { .. })).await else {
        unreachable!()
    };
    assert!(desktop.approve_pairing(&device_id, true));
    next(&mut events, "conectado", |e| matches!(e, PhoneEvent::Status(LinkStatus::Connected { .. }))).await;
    (desktop, phone, desktop_id)
}

fn hello() -> VideoHello {
    VideoHello { source: VideoSource::Screen, direction: VideoDirection::Send, codec: "h264".into(), width: 720, height: 1600, rotation: 0 }
}

#[tokio::test(flavor = "multi_thread")]
async fn la_pantalla_del_celular_llega_al_pc() {
    let (desktop, phone, desktop_id) = paired(true).await;
    // El PC muestra lo que llegue: los cuadros pasan a este canal.
    let (shown_tx, mut shown) = unbounded_channel();
    let links = Arc::new(Mutex::new(Vec::new()));
    let keep = links.clone();
    desktop.on_video(move |mut link| {
        assert_eq!(link.hello.width, 720);
        let shown_tx = shown_tx.clone();
        keep.lock().unwrap().push(link.control.clone());
        tokio::spawn(async move {
            while let Some(frame) = link.frames.recv().await {
                let _ = shown_tx.send(frame);
            }
        });
        Ok(())
    });
    let (controls_tx, mut controls) = unbounded_channel();
    let sender = phone
        .start_video(&desktop_id, hello(), move |c| { let _ = controls_tx.send(c); }, |_| {})
        .expect("el PC muestra video");
    // Lo primero que pide el PC es un keyframe.
    next(&mut controls, "pedido de keyframe", |c| *c == VideoControl::RequestKeyframe).await;
    assert!(sender.send(flags::CONFIG | flags::KEY, 0, vec![0, 0, 0, 1, 0x67, 0x42]));
    let first = next(&mut shown, "el keyframe", |_| true).await;
    assert_eq!(first.data, vec![0, 0, 0, 1, 0x67, 0x42]);
    assert!(sender.send(0, 33_333, vec![0, 0, 0, 1, 0x41, 1]));
    let second = next(&mut shown, "el cuadro siguiente", |_| true).await;
    assert_eq!(second.pts_us, 33_333);
    // El PC pide un bitrate: llega al codificador.
    links.lock().unwrap()[0].send(VideoControl::SetBitrate { bps: 2_000_000 }).unwrap();
    next(&mut controls, "bitrate", |c| *c == VideoControl::SetBitrate { bps: 2_000_000 }).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn con_la_pantalla_apagada_el_pc_no_la_acepta() {
    let (desktop, phone, desktop_id) = paired(false).await;
    desktop.on_video(|_| Ok(()));
    // El PC no anuncia la función: el celular ni lo intenta.
    assert!(phone.start_video(&desktop_id, hello(), |_| {}, |_| {}).is_none());
}
