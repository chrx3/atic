//! Escritorio y celular de verdad, en el mismo proceso, hablando por iroh.

use std::time::Duration;

use atic_sync::{
    desktop::{Desktop, DesktopConfig, DesktopEvent},
    generate_secret_key,
    phone::{LinkStatus, Phone, PhoneConfig, PhoneEvent, Target},
    AgentCard, AgentStatus, Decision, PermissionAsk,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

fn card(id: &str, ask: Option<&str>) -> AgentCard {
    AgentCard {
        id: id.into(),
        backend_id: "claude-code".into(),
        backend_name: "Claude Code".into(),
        project: "atic".into(),
        status: if ask.is_some() { AgentStatus::Waiting } else { AgentStatus::Working },
        activity: None,
        preview: None,
        permission: ask.map(|p| PermissionAsk {
            id: p.into(),
            title: "Ejecutar comando".into(),
            detail: Some("cargo test".into()),
            can_allow_always: true,
        }),
    }
}

async fn next<T: std::fmt::Debug>(rx: &mut UnboundedReceiver<T>, what: &str, pick: impl Fn(&T) -> bool) -> T {
    let found = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let event = rx.recv().await.expect("el canal se cerró");
            if pick(&event) {
                return event;
            }
        }
    })
    .await;
    found.unwrap_or_else(|_| panic!("no llegó: {what}"))
}

async fn desktop(trusted: Vec<String>, key: [u8; 32]) -> (Desktop, UnboundedReceiver<DesktopEvent>) {
    Desktop::start(DesktopConfig { secret_key: key, name: "PC de prueba".into(), trusted })
        .await
        .unwrap()
}

/// Acepta en el PC el próximo pedido de pareo.
async fn approve(desktop: &Desktop, events: &mut UnboundedReceiver<DesktopEvent>) -> String {
    let DesktopEvent::PairRequested { device_id, .. } =
        next(events, "pedido de pareo", |e| matches!(e, DesktopEvent::PairRequested { .. })).await
    else {
        unreachable!()
    };
    assert!(desktop.approve_pairing(&device_id, true));
    device_id
}

/// Un celular pareado y conectado, con sus eventos.
async fn paired_pair() -> (Desktop, UnboundedReceiver<DesktopEvent>, Phone, UnboundedReceiver<PhoneEvent>, String) {
    let (desktop, mut desk_events) = desktop(vec![], generate_secret_key()).await;
    let ticket = desktop.pairing_ticket().await;
    let phone = phone(generate_secret_key()).await;
    let (tx, mut events) = unbounded_channel();
    phone.connect(Target::from_ticket(&ticket).unwrap(), tx);
    let device_id = approve(&desktop, &mut desk_events).await;
    next(&mut events, "conectado", |e| matches!(e, PhoneEvent::Status(LinkStatus::Connected { .. }))).await;
    next(&mut desk_events, "conectado en el PC", |e| matches!(e, DesktopEvent::Connected { .. })).await;
    (desktop, desk_events, phone, events, device_id)
}

async fn phone(key: [u8; 32]) -> Phone {
    Phone::start(PhoneConfig { secret_key: key, device_name: "Celular".into() }).await.unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn parea_recibe_agentes_y_contesta_un_permiso() {
    let (desktop, mut desk_events) = desktop(vec![], generate_secret_key()).await;
    desktop.publish(vec![card("a", Some("p1"))]);
    let ticket = desktop.pairing_ticket().await;

    let phone = phone(generate_secret_key()).await;
    let (tx, mut events) = unbounded_channel();
    phone.connect(Target::from_ticket(&ticket).unwrap(), tx);

    next(&mut events, "espera aprobación", |e| matches!(e, PhoneEvent::Status(LinkStatus::AwaitingApproval))).await;
    approve(&desktop, &mut desk_events).await;
    let PhoneEvent::Paired(paired) = next(&mut events, "pareo", |e| matches!(e, PhoneEvent::Paired(_))).await else {
        unreachable!()
    };
    assert_eq!(paired.name, "PC de prueba");
    next(&mut events, "conectado", |e| matches!(e, PhoneEvent::Status(LinkStatus::Connected { .. }))).await;
    let PhoneEvent::Agents(agents) = next(&mut events, "agentes", |e| matches!(e, PhoneEvent::Agents(_))).await else {
        unreachable!()
    };
    assert_eq!(agents, vec![card("a", Some("p1"))]);
    next(&mut desk_events, "pareo en el PC", |e| matches!(e, DesktopEvent::Paired { .. })).await;

    assert!(phone.decide("a".into(), "p1".into(), Decision::Allow));
    let decided = next(&mut desk_events, "decisión", |e| matches!(e, DesktopEvent::Decide { .. })).await;
    let DesktopEvent::Decide { agent_id, permission_id, decision, .. } = decided else { unreachable!() };
    assert_eq!((agent_id.as_str(), permission_id.as_str(), decision), ("a", "p1", Decision::Allow));

    desktop.publish(vec![card("a", None)]);
    let PhoneEvent::Agents(agents) = next(&mut events, "foto nueva", |e| matches!(e, PhoneEvent::Agents(_))).await else {
        unreachable!()
    };
    assert_eq!(agents[0].permission, None);

    // El ticket ya se gastó: otro celular con el mismo QR queda afuera.
    let intruder = self::phone(generate_secret_key()).await;
    let (tx, mut intruder_events) = unbounded_channel();
    intruder.connect(Target::from_ticket(&ticket).unwrap(), tx);
    next(&mut intruder_events, "rechazo", |e| matches!(e, PhoneEvent::Rejected(_))).await;

    phone.shutdown().await;
    intruder.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn un_celular_pareado_vuelve_a_entrar_y_uno_desconocido_no() {
    let desk_key = generate_secret_key();
    let phone_key = generate_secret_key();

    // Primer arranque: parear y quedarse con lo que cada lado guardaría.
    let (first, mut first_events) = desktop(vec![], desk_key).await;
    let ticket = first.pairing_ticket().await;
    let p = phone(phone_key).await;
    let (tx, mut events) = unbounded_channel();
    p.connect(Target::from_ticket(&ticket).unwrap(), tx);
    approve(&first, &mut first_events).await;
    let PhoneEvent::Paired(saved) = next(&mut events, "pareo", |e| matches!(e, PhoneEvent::Paired(_))).await else {
        unreachable!()
    };
    let DesktopEvent::Paired { device_id, .. } =
        next(&mut first_events, "pareo en el PC", |e| matches!(e, DesktopEvent::Paired { .. })).await
    else {
        unreachable!()
    };
    p.shutdown().await;
    first.shutdown().await;

    // Segundo arranque del PC con la misma clave y la lista guardada.
    let (second, _events) = desktop(vec![device_id], desk_key).await;
    let _ = second.pairing_ticket().await; // solo para esperar a que esté en línea
    let p = phone(phone_key).await;
    let (tx, mut events) = unbounded_channel();
    p.connect(Target::Known(atic_sync::phone::PairedDesktop::from_json(&saved.to_json()).unwrap()), tx);
    next(&mut events, "reconexión", |e| matches!(e, PhoneEvent::Status(LinkStatus::Connected { .. }))).await;

    let stranger = phone(generate_secret_key()).await;
    let (tx, mut stranger_events) = unbounded_channel();
    stranger.connect(Target::Known(saved), tx);
    next(&mut stranger_events, "rechazo", |e| matches!(e, PhoneEvent::Rejected(_))).await;

    p.shutdown().await;
    stranger.shutdown().await;
    second.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn si_el_pc_no_acepta_el_celular_queda_afuera() {
    let (desktop, mut desk_events) = desktop(vec![], generate_secret_key()).await;
    let ticket = desktop.pairing_ticket().await;
    let p = phone(generate_secret_key()).await;
    let (tx, mut events) = unbounded_channel();
    p.connect(Target::from_ticket(&ticket).unwrap(), tx);
    let DesktopEvent::PairRequested { device_id, .. } =
        next(&mut desk_events, "pedido", |e| matches!(e, DesktopEvent::PairRequested { .. })).await
    else {
        unreachable!()
    };
    assert!(desktop.approve_pairing(&device_id, false));
    next(&mut events, "rechazo", |e| matches!(e, PhoneEvent::Rejected(_))).await;
    // El token se gastó en el intento: no queda ventana abierta para otro.
    assert!(!desktop.approve_pairing(&device_id, true));
    p.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn revocar_corta_la_sesion_abierta() {
    let (desktop, _desk_events, phone, mut events, device_id) = paired_pair().await;
    desktop.revoke(&device_id).unwrap();
    // Se corta y, al reintentar, el PC ya no lo reconoce.
    next(&mut events, "rechazo tras revocar", |e| matches!(e, PhoneEvent::Rejected(_))).await;
    assert!(!phone.decide("a".into(), "p".into(), Decision::Allow));
    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn el_portapapeles_viaja_en_los_dos_sentidos() {
    let (desktop, mut desk_events, phone, mut events, _) = paired_pair().await;

    assert!(desktop.send_clip("copiado en el PC".into(), 1));
    let PhoneEvent::Clip(item) = next(&mut events, "clip del PC", |e| matches!(e, PhoneEvent::Clip(_))).await else {
        unreachable!()
    };
    assert_eq!(item.text, "copiado en el PC");
    assert_eq!(item.source_name, "PC de prueba");

    assert!(phone.send_clip("copiado en el celular".into(), 2));
    let DesktopEvent::Clip { item, .. } =
        next(&mut desk_events, "clip del celular", |e| matches!(e, DesktopEvent::Clip { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(item.text, "copiado en el celular");
    assert_eq!(item.source_name, "Celular");

    // Lo que mandó el celular no le vuelve como eco.
    let echo = tokio::time::timeout(Duration::from_millis(500), async {
        loop {
            if let Some(PhoneEvent::Clip(_)) = events.recv().await {
                return;
            }
        }
    })
    .await;
    assert!(echo.is_err(), "el celular recibió su propio texto de vuelta");

    // Demasiado largo: ni sale.
    assert!(!desktop.send_clip("x".repeat(atic_sync::MAX_CLIP_BYTES + 1), 3));

    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn el_estado_del_pc_llega_y_los_comandos_vuelven() {
    use atic_sync::{MediaState, PcCommand, PcState, RecordingState};
    let (desktop, mut desk_events, phone, mut events, _) = paired_pair().await;

    let state = PcState {
        media: Some(MediaState {
            title: "Tema".into(),
            artist: "Artista".into(),
            app: "Spotify".into(),
            playing: true,
            can_toggle: true,
            can_next: true,
            can_prev: false,
        }),
        recording: Some(RecordingState { started_at_ms: 42 }),
    };
    desktop.publish_pc(state.clone());
    let got = next(&mut events, "estado del PC", |e| matches!(e, PhoneEvent::Pc(p) if p.media.is_some())).await;
    assert_eq!(got, PhoneEvent::Pc(state));

    assert!(phone.command(PcCommand::StopRecording));
    let DesktopEvent::Command { command, .. } =
        next(&mut desk_events, "comando", |e| matches!(e, DesktopEvent::Command { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(command, PcCommand::StopRecording);

    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn el_historial_llega_entero_y_sin_textos_gigantes() {
    use atic_sync::ClipItem;
    let (desktop, _desk_events, phone, mut events, _) = paired_pair().await;
    let item = |id: &str, text: String| ClipItem {
        id: id.into(),
        text,
        source_name: "PC".into(),
        copied_at_ms: 1,
        pinned: id == "a",
        kind: Default::default(),
        thumb: None,
        width: 0,
        height: 0,
    };
    desktop.publish_clip_history(vec![
        item("a", "fijado".into()),
        item("b", "x".repeat(atic_sync::MAX_HISTORY_ITEM_BYTES + 1)),
        item("c", "reciente".into()),
    ]);
    let PhoneEvent::ClipHistory(items) =
        next(&mut events, "historial", |e| matches!(e, PhoneEvent::ClipHistory(i) if !i.is_empty())).await
    else {
        unreachable!()
    };
    let ids: Vec<_> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "c"]);
    assert!(items[0].pinned);
    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn las_imagenes_van_como_miniatura_y_se_piden_enteras() {
    use atic_sync::{ClipItem, ClipKind};
    let (desktop, mut desk_events, phone, mut events, device_id) = paired_pair().await;
    desktop.publish_clip_history(vec![ClipItem {
        id: "img".into(),
        text: String::new(),
        source_name: String::new(),
        copied_at_ms: 1,
        pinned: false,
        kind: ClipKind::Image,
        thumb: Some(vec![0xFF, 0xD8, 0xFF]),
        width: 1920,
        height: 1080,
    }]);
    let PhoneEvent::ClipHistory(items) =
        next(&mut events, "historial", |e| matches!(e, PhoneEvent::ClipHistory(i) if !i.is_empty())).await
    else {
        unreachable!()
    };
    assert_eq!(items[0].thumb.as_deref(), Some(&[0xFF, 0xD8, 0xFF][..]));
    assert_eq!((items[0].width, items[0].height), (1920, 1080));

    assert!(phone.fetch_image("img".into()));
    let DesktopEvent::FetchImage { id, .. } =
        next(&mut desk_events, "pedido de imagen", |e| matches!(e, DesktopEvent::FetchImage { .. })).await
    else {
        unreachable!()
    };
    assert!(desktop.send_image(&device_id, id, "image/png".into(), Some(vec![1, 2, 3])));
    let got = next(&mut events, "imagen", |e| matches!(e, PhoneEvent::Image { .. })).await;
    assert_eq!(got, PhoneEvent::Image { id: "img".into(), mime: "image/png".into(), data: Some(vec![1, 2, 3]) });

    // Y al revés: una imagen del celular al PC.
    assert!(phone.send_image("image/jpeg".into(), vec![9, 9]));
    let DesktopEvent::Image { mime, data, .. } =
        next(&mut desk_events, "imagen del celular", |e| matches!(e, DesktopEvent::Image { .. })).await
    else {
        unreachable!()
    };
    assert_eq!((mime.as_str(), data), ("image/jpeg", vec![9, 9]));

    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn la_musica_del_celular_llega_al_pc_y_se_controla_desde_ahi() {
    use atic_sync::{MediaState, PcCommand};
    let (desktop, mut desk_events, phone, mut events, device_id) = paired_pair().await;
    let song = MediaState {
        title: "Perreala".into(),
        artist: "Nobdy".into(),
        app: "YouTube Music".into(),
        playing: true,
        can_toggle: true,
        can_next: true,
        can_prev: true,
    };
    assert!(phone.publish_media(Some(song.clone()), Some(vec![0xFF, 0xD8])));
    let DesktopEvent::PhoneMedia { media, art, .. } =
        next(&mut desk_events, "música del celular", |e| matches!(e, DesktopEvent::PhoneMedia { .. })).await
    else {
        unreachable!()
    };
    assert_eq!((media, art), (Some(song), Some(vec![0xFF, 0xD8])));

    assert!(desktop.phone_media_command(&device_id, PcCommand::MediaNext));
    let got = next(&mut events, "comando de medios", |e| matches!(e, PhoneEvent::MediaCommand(_))).await;
    assert_eq!(got, PhoneEvent::MediaCommand(PcCommand::MediaNext));

    phone.shutdown().await;
    desktop.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn el_celular_suma_su_historial_y_los_borrados_viajan_en_los_dos_sentidos() {
    use atic_sync::{ClipItem, ClipKind};
    let (desktop, mut desk_events, phone, mut events, device_id) = paired_pair().await;
    let item = |id: &str, kind: ClipKind| ClipItem {
        id: id.into(),
        text: if kind == ClipKind::Text { "del otro PC".into() } else { String::new() },
        source_name: String::new(),
        copied_at_ms: 1,
        pinned: false,
        kind,
        thumb: None,
        width: 0,
        height: 0,
    };

    assert!(phone.sync_clips(vec![item("t1", ClipKind::Text), item("i1", ClipKind::Image)], vec!["viejo".into()]));
    let DesktopEvent::ClipSync { items, deleted, .. } =
        next(&mut desk_events, "historial del celular", |e| matches!(e, DesktopEvent::ClipSync { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(items.len(), 2);
    assert_eq!(deleted, vec!["viejo".to_string()]);

    // El PC pide la imagen que le falta y el celular se la manda.
    assert!(desktop.request_clip_images(&device_id, vec!["i1".into()]));
    let got = next(&mut events, "pedido", |e| matches!(e, PhoneEvent::ClipNeed(_))).await;
    assert_eq!(got, PhoneEvent::ClipNeed(vec!["i1".into()]));
    assert!(phone.send_clip_image("i1".into(), "image/png".into(), vec![7]));
    let DesktopEvent::ClipImage { id, data, .. } =
        next(&mut desk_events, "imagen", |e| matches!(e, DesktopEvent::ClipImage { .. })).await
    else {
        unreachable!()
    };
    assert_eq!((id.as_str(), data), ("i1", vec![7]));

    // Borrar en el PC llega al celular, y al revés.
    assert!(desktop.clips_deleted(vec!["t1".into()]));
    let got = next(&mut events, "borrado en el PC", |e| matches!(e, PhoneEvent::ClipDeleted(_))).await;
    assert_eq!(got, PhoneEvent::ClipDeleted(vec!["t1".into()]));
    assert!(phone.delete_clips(vec!["i1".into()]));
    let DesktopEvent::ClipDelete { ids, .. } =
        next(&mut desk_events, "borrado en el celular", |e| matches!(e, DesktopEvent::ClipDelete { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(ids, vec!["i1".to_string()]);

    phone.shutdown().await;
    desktop.shutdown().await;
}

/// Regresión: una imagen grande llegando mientras el PC publica cosas seguido.
/// La lectura no puede cancelarse a medias (se perdían bytes y el siguiente
/// largo salía de adentro de la imagen: «mensaje de N bytes supera el tope»).
#[tokio::test(flavor = "multi_thread")]
async fn un_mensaje_grande_no_se_corta_aunque_lleguen_otros_eventos() {
    use atic_sync::{PcState, RecordingState};
    let (desktop, mut desk_events, phone, _events, _) = paired_pair().await;
    let desktop = std::sync::Arc::new(desktop);
    let noisy = desktop.clone();
    let noise = tokio::spawn(async move {
        for i in 0..3_000i64 {
            noisy.publish_pc(PcState { media: None, recording: Some(RecordingState { started_at_ms: i }) });
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    });
    let big = vec![42u8; 4 << 20];
    assert!(phone.send_clip_image("grande".into(), "image/png".into(), big.clone()));
    let DesktopEvent::ClipImage { data, .. } =
        next(&mut desk_events, "imagen grande entera", |e| matches!(e, DesktopEvent::ClipImage { .. })).await
    else {
        unreachable!()
    };
    assert_eq!(data.len(), big.len());
    noise.abort();
    phone.shutdown().await;
}
