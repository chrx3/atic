//! La pantalla del celular en el PC, de punta a punta: escritorio y celular de verdad,
//! pareados y hablando por iroh, con la conexión de video aparte (`atic/video/1`).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent},
    feature, generate_secret_key,
    phone::{LinkStatus, Phone, PhoneConfig, PhoneEvent, Target},
    video::{flags, ScreenSource, VideoControl, VideoDirection, VideoFrame, VideoHello, VideoSource},
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
    let features = if screen { vec![feature::SCREEN.to_string(), feature::SCREEN_OUT.to_string()] } else { vec![] };
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

#[tokio::test(flavor = "multi_thread")]
async fn la_pantalla_del_pc_llega_al_celular() {
    let (desktop, phone, desktop_id) = paired(true).await;
    // El PC comparte: lo que mande por `frames_tx` le llega al celular.
    let (frames_tx, frames) = tokio::sync::mpsc::channel(4);
    let frames = std::sync::Mutex::new(Some(frames));
    let (asked_tx, mut asked) = unbounded_channel();
    let widths = Arc::new(Mutex::new(Vec::new()));
    let seen = widths.clone();
    desktop.on_screen_request(move |request| {
        seen.lock().unwrap().push(request.max_width);
        let (control, mut rx) = unbounded_channel();
        let asked_tx = asked_tx.clone();
        tokio::spawn(async move {
            while let Some(c) = rx.recv().await {
                let _ = asked_tx.send(c);
            }
        });
        Ok(ScreenSource { frames: frames.lock().unwrap().take().expect("un solo pedido"), control })
    });
    let (got_tx, mut got) = unbounded_channel::<VideoFrame>();
    let view = phone
        .watch_screen(&desktop_id, 1600, move |frame| { let _ = got_tx.send(frame); }, |_| {})
        .expect("el PC comparte su pantalla");
    // Al aceptar, el PC se pide un keyframe para el celular.
    next(&mut asked, "keyframe", |c| *c == VideoControl::RequestKeyframe).await;
    assert_eq!(*widths.lock().unwrap(), vec![1600]);
    frames_tx.send(VideoFrame { flags: flags::FORMAT, pts_us: 0, data: br#"{"width":1600,"height":1000,"rotation":0}"#.to_vec() }).await.unwrap();
    frames_tx.send(VideoFrame { flags: flags::KEY | flags::CONFIG, pts_us: 0, data: vec![0, 0, 0, 1, 0x67] }).await.unwrap();
    let format = next(&mut got, "formato", |_| true).await;
    assert_eq!(format.format().map(|f| (f.width, f.height)), Some((1600, 1000)));
    assert_eq!(next(&mut got, "keyframe", |_| true).await.data, vec![0, 0, 0, 1, 0x67]);
    // Lo que pide el celular llega a quien comparte.
    view.set_bitrate(3_000_000);
    next(&mut asked, "bitrate", |c| *c == VideoControl::SetBitrate { bps: 3_000_000 }).await;
    // Soltar la vista: el PC deja de compartir.
    drop(view);
    next(&mut asked, "parar", |c| *c == VideoControl::Stop).await;
}
