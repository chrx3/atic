//! El video v2 por una red mala simulada: dos endpoints de iroh en este equipo unidos por
//! un proxy UDP que limita el ancho de banda (con cola, como un router), agrega demora y
//! pierde paquetes. Del lado que manda, un codificador de mentira que hace caso al
//! bitrate y a los keyframes que le piden; del que recibe, la demora de cada cuadro.
//!
//! A mano (tarda ~30 s): `cargo test -p atic-sync red_mala -- --ignored --nocapture`.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::*;

/// Cómo está la red en un momento: ida (de quien manda a quien mira) limitada.
#[derive(Clone, Copy)]
struct Net {
    kbps: u64,
    /// Demora de cada sentido (el RTT es el doble).
    delay_ms: u64,
    /// Paquetes perdidos de cada mil, en la ida.
    loss_permil: u64,
}

/// El proxy: lo que manda `A` pasa por un enlace de `kbps` con una cola de como mucho
/// `BUFFER`; la vuelta solo tiene demora.
struct Proxy {
    net: Mutex<Net>,
}

const BUFFER: Duration = Duration::from_millis(300);

impl Proxy {
    /// `to_b` es IPv6 y `A` habla solo IPv4: así no hay otro camino que el proxy (iroh
    /// se pasa las direcciones de cada uno y abriría uno directo).
    async fn start(to_b: SocketAddr, net: Net) -> (Arc<Self>, SocketAddr) {
        let side_a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let side_b = Arc::new(UdpSocket::bind("[::1]:0").await.unwrap());
        let addr = side_a.local_addr().unwrap();
        let proxy = Arc::new(Self { net: Mutex::new(net) });
        // Cada sentido entrega en orden: lo que sale antes, llega antes.
        let deliver = |socket: Arc<UdpSocket>| {
            let (tx, mut rx) = mpsc::unbounded_channel::<(Instant, Vec<u8>, SocketAddr)>();
            tokio::spawn(async move {
                while let Some((at, packet, to)) = rx.recv().await {
                    tokio::time::sleep_until(at.into()).await;
                    let _ = socket.send_to(&packet, to).await;
                }
            });
            tx
        };
        let forward = deliver(side_b.clone());
        let back = deliver(side_a.clone());
        let from_a: Arc<Mutex<Option<SocketAddr>>> = Arc::default();
        // La vuelta: solo demora.
        let (me, last_a) = (proxy.clone(), from_a.clone());
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            while let Ok((len, _)) = side_b.recv_from(&mut buf).await {
                let delay = me.net.lock().unwrap().delay_ms;
                if let Some(a) = *last_a.lock().unwrap() {
                    let _ = back.send((Instant::now() + Duration::from_millis(delay), buf[..len].to_vec(), a));
                }
            }
        });
        // La ida: pérdida, enlace de `kbps` con su cola y demora.
        let me = proxy.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            // Cuándo queda libre el enlace de ida.
            let mut free_at = Instant::now();
            let mut seed = 0x2545_f491_4f6c_dd1du64;
            while let Ok((len, from)) = side_a.recv_from(&mut buf).await {
                *from_a.lock().unwrap() = Some(from);
                let net = *me.net.lock().unwrap();
                let now = Instant::now();
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                if seed % 1000 < net.loss_permil {
                    continue;
                }
                let start = free_at.max(now);
                // Cola llena: el router lo bota.
                if start - now > BUFFER {
                    continue;
                }
                free_at = start + Duration::from_secs_f64(len as f64 * 8. / (net.kbps as f64 * 1000.));
                let _ = forward.send((free_at + Duration::from_millis(net.delay_ms), buf[..len].to_vec(), to_b));
            }
        });
        (proxy, addr)
    }

    fn set(&self, net: Net) {
        *self.net.lock().unwrap() = net;
    }
}

async fn endpoint(bind: &str, alpns: Vec<Vec<u8>>) -> Endpoint {
    Endpoint::builder(presets::Minimal).clear_ip_transports().bind_addr(bind).unwrap().alpns(alpns).bind().await.unwrap()
}

/// Lo que ve quien mira en un rato: cuadros, cuántos fueron keyframes y la demora de cada
/// uno (de que se "capturó" a que llegó).
#[derive(Default, Debug)]
struct Seen {
    frames: usize,
    keys: usize,
    dropped: usize,
    delays: Vec<Duration>,
}

impl Seen {
    fn p(&self, q: f64) -> Duration {
        let mut d = self.delays.clone();
        d.sort();
        d.get(((d.len() as f64 * q) as usize).min(d.len().saturating_sub(1))).copied().unwrap_or_default()
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn red_mala() {
    let _ = tracing_subscriber::fmt().with_env_filter("atic_sync=info").with_test_writer().try_init();
    const FPS: u64 = 30;
    const MAX: u32 = 4_000_000;
    let good = Net { kbps: 8_000, delay_ms: 10, loss_permil: 0 };

    let b = endpoint("[::1]:0", vec![VIDEO_ALPN_2.to_vec()]).await;
    let a = endpoint("127.0.0.1:0", vec![]).await;
    let (proxy, via) = Proxy::start(b.bound_sockets()[0], good).await;

    // Quien mira: lee como el PC y anota cada cuadro en el rato que corre.
    let started = Instant::now();
    let seen: Arc<Mutex<Seen>> = Arc::default();
    let sink = seen.clone();
    let b2 = b.clone();
    tokio::spawn(async move {
        let conn = b2.accept().await.unwrap().accept().unwrap().await.unwrap();
        let (_send, recv) = conn.accept_bi().await.unwrap();
        let mut incoming = spawn_reader(Wire::V2, conn.clone(), recv, "red mala");
        while let Some(Ok(got)) = incoming.recv().await {
            let mut seen = sink.lock().unwrap();
            match got {
                Incoming::Frame(frame) => {
                    seen.frames += 1;
                    seen.keys += frame.is_key() as usize;
                    seen.delays.push(started.elapsed().saturating_sub(Duration::from_micros(frame.pts_us)));
                }
                Incoming::Dropped => seen.dropped += 1,
            }
        }
    });

    // Quien manda: el transporte de verdad, solo por el proxy.
    let conn = connect(&a, EndpointAddr::new(b.id()).with_ip_addr(via)).await.unwrap();
    let (mut send, _recv) = conn.open_bi().await.unwrap();
    // El otro lado recién ve el bi-stream cuando llega algo.
    send.write_all(b"hola").await.unwrap();
    let target = Arc::new(AtomicU32::new(MAX));
    let want_key = Arc::new(AtomicBool::new(true));
    let (frames_tx, mut frames) = mpsc::channel(QUEUE);
    let (t, k) = (target.clone(), want_key.clone());
    let on_control: Arc<ControlHandler> = Arc::new(move |c| match c {
        VideoControl::RequestKeyframe => k.store(true, Ordering::Relaxed),
        VideoControl::SetBitrate { bps } => t.store(bps, Ordering::Relaxed),
        _ => {}
    });
    let sender = Arc::new(VideoSender { frames: frames_tx, need_key: AtomicBool::new(true), on_control: on_control.clone() });
    let control = on_control.clone();
    let pump_conn = conn.clone();
    let pumping = tokio::spawn(async move { pump(Wire::V2, &pump_conn, &mut send, &mut frames, &*control, MAX, "red mala").await });

    // El codificador de mentira: cada cuadro mide lo que da el bitrate; un keyframe, 4×, y
    // uno por pedido (como MediaCodec en el celular).
    let (t, k, s) = (target.clone(), want_key.clone(), sender.clone());
    let encoder = tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_micros(1_000_000 / FPS));
        loop {
            tick.tick().await;
            let size = (t.load(Ordering::Relaxed) as u64 / 8 / FPS) as usize;
            let key = k.swap(false, Ordering::Relaxed);
            let (flags, size) = if key { (flags::KEY, size * 4) } else { (0, size) };
            let pts = started.elapsed().as_micros() as u64;
            s.send(flags, pts, vec![0u8; size]);
        }
    });

    // Cada tramo: cómo está la red y cuánto dura. Se informa cada segundo.
    let phases = [
        ("buena", good, 4),
        ("cae a 1 Mb/s, RTT 100 ms", Net { kbps: 1_000, delay_ms: 50, loss_permil: 0 }, 10),
        ("vuelve a 8 Mb/s con 2 % de pérdida", Net { kbps: 8_000, delay_ms: 20, loss_permil: 20 }, 14),
    ];
    let mut report = Vec::new();
    for (name, net, secs) in phases {
        proxy.set(net);
        println!("--- {name}");
        let mut per_phase = Vec::new();
        for second in 0..secs {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let window = std::mem::take(&mut *seen.lock().unwrap());
            println!(
                "{:>2}s  objetivo {:>4} kb/s  {:>2} cuadros ({} key, {} abandonados)  demora p50 {:>4} ms  p95 {:>4} ms",
                second + 1,
                target.load(Ordering::Relaxed) / 1000,
                window.frames,
                window.keys,
                window.dropped,
                window.p(0.5).as_millis(),
                window.p(0.95).as_millis(),
            );
            per_phase.push((target.load(Ordering::Relaxed), window));
        }
        report.push(per_phase);
    }
    encoder.abort();
    pumping.abort();
    let paths: Vec<_> = conn.paths().iter().map(|p| format!("{:?}", p.remote_addr())).collect();
    println!("caminos: {paths:?}");

    // Con la red buena va a todo: 30 cuadros/s sin demora.
    let (_, last_good) = report[0].last().unwrap();
    assert!(last_good.frames >= 25, "red buena: {last_good:?}");
    assert!(last_good.p(0.95) < Duration::from_millis(150));
    // Al caer el ancho de banda, el bitrate baja de lo que cabe y la demora vuelve a ser
    // corta: en los últimos 4 s del tramo, sin cola y casi todos los cuadros.
    let fell = &report[1];
    assert!(fell.last().unwrap().0 <= 1_000_000, "no bajó: {} b/s", fell.last().unwrap().0);
    for (_, window) in &fell[fell.len() - 4..] {
        assert!(window.frames >= 20, "con 1 Mb/s llegan pocos: {window:?}");
        assert!(window.p(0.95) < Duration::from_millis(400), "con 1 Mb/s queda cola: p95 {:?}", window.p(0.95));
    }
    // Al volver, el bitrate sube de nuevo (con pérdida no tiene por qué llegar al tope).
    let back = &report[2];
    assert!(back.last().unwrap().0 >= 2_000_000, "no volvió a subir: {} b/s", back.last().unwrap().0);
}
