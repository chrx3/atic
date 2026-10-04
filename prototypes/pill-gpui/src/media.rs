//! Ahora suena: lo que reproduce cualquier app que se lo cuenta a Windows
//! (Spotify, el navegador con YouTube…), con carátula y controles.
//!
//! Un hilo lee la sesión de medios del sistema
//! (`GlobalSystemMediaTransportControlsSessionManager`, la misma de las
//! teclas multimedia) y el nivel real de la salida de audio
//! (`IAudioMeterInformation`) para la onda de la pill. La pill solo mira el
//! estado compartido: no espera a Windows en ningún cuadro.
//!
//! El panel es la «Portada» del diseño (Claude Design, «Ahora suena · notch
//! de Atic»): la carátula grande y nítida arriba, que se funde hacia abajo
//! en su propio difuminado, con el título encima; debajo la barra de
//! progreso que se arrastra, ±10 s, y el volumen y la salida de audio (que
//! despliega la lista de parlantes y audífonos).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{
    actions, canvas, div, img, Animation, AnimationExt, linear_color_stop, linear_gradient, prelude::*, px, rgb, svg, App,
    Bounds, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, Hsla, KeyBinding,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, RenderImage,
    SharedString, Window,
};

use crate::clipboard::BAND_H;

pub mod lyrics;
use crate::system::os::{Backend, Cmd, Snapshot, Want};

/// Lo que suena ahora.
#[derive(Clone, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    /// La app que reproduce (su AppUserModelID, para el nombre).
    pub source: String,
    pub playing: bool,
    pub art: Option<Arc<RenderImage>>,
    /// La carátula chica y difuminada, para el fondo del panel.
    pub backdrop: Option<Arc<RenderImage>>,
    /// La carátula grande, nítida y desvanecida hacia abajo: la portada.
    pub hero: Option<Arc<RenderImage>>,
    /// Posición y duración en segundos, si la app las informa.
    pub position: Option<(f32, f32)>,
    /// Cuándo se leyó la posición: entre lecturas se adelanta sola.
    pub read_at: Option<Instant>,
}

impl Track {
    /// La posición ahora, contando lo que pasó desde la última lectura.
    pub fn position_now(&self) -> Option<(f32, f32)> {
        let (pos, end) = self.position?;
        let elapsed = if self.playing {
            self.read_at.map_or(0.0, |at| at.elapsed().as_secs_f32())
        } else {
            0.0
        };
        Some(((pos + elapsed).min(end), end))
    }
}

pub enum Control {
    Toggle,
    Next,
    Previous,
    /// Ir a un segundo de la canción.
    Seek(f32),
    /// Adelantar o retroceder tantos segundos.
    Jump(f32),
}

/// El estado que comparte el hilo con la pill.
#[derive(Clone)]
pub struct Media {
    track: Arc<Mutex<Option<Track>>>,
    /// Sube cada vez que cambia la pista o su estado.
    version: Arc<AtomicU64>,
    /// Nivel pico de la salida (0..1), como bits de `f32`.
    level: Arc<AtomicU32>,
    /// Después de mover la canción: a dónde se fue y cuándo. Muchas apps
    /// tardan en informar la posición nueva; mientras tanto manda esta.
    hold: Arc<Mutex<Option<(f32, Instant)>>>,
    /// La carátula ya preparada. Va aparte de la pista: se decodifica en su
    /// propio hilo y llega cuando está lista, sin frenar la lectura.
    art: Arc<Mutex<ArtSet>>,
    /// La escala de la pantalla del panel (bits de `f32`): la portada se
    /// arma a pixeles reales para que GPUI no la estire.
    scale: Arc<AtomicU32>,
    /// La letra sincronizada de la pista, cuando llega (`lyrics.rs`).
    lyrics: Arc<Mutex<lyrics::Slot>>,
    /// Pide la letra de cada pista nueva; `None` sin red (`PILL_MEDIA_ONLINE=0`).
    lyrics_tx: Option<mpsc::Sender<lyrics::Ask>>,
    /// La última pista cuya letra se pidió.
    asked: Arc<Mutex<String>>,
    /// Mostrar la letra, en el notch y en el panel (se elige en Ahora suena y
    /// se guarda en `%LOCALAPPDATA%\atic-gpui\media.txt`).
    show_lyrics: Arc<std::sync::atomic::AtomicBool>,
    tx: mpsc::Sender<Control>,
}

/// Carátula chica (para la pill), fondo difuminado y portada.
#[derive(Clone, Default)]
struct ArtSet {
    art: Option<Arc<RenderImage>>,
    backdrop: Option<Arc<RenderImage>>,
    hero: Option<Arc<RenderImage>>,
}

/// Lo que dura el salto «a la espera» antes de creerle a la app.
const HOLD_FOR: Duration = Duration::from_secs(3);

impl Media {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let media = Self {
            track: Default::default(),
            version: Default::default(),
            level: Default::default(),
            hold: Default::default(),
            art: Default::default(),
            scale: Arc::new(AtomicU32::new(1f32.to_bits())),
            lyrics: Default::default(),
            lyrics_tx: None,
            asked: Default::default(),
            show_lyrics: Arc::new(std::sync::atomic::AtomicBool::new(load_show_lyrics())),
            tx,
        };
        let mut media = media;
        if itunes::enabled() && std::env::var_os("PILL_MEDIA_DEMO").is_none() {
            let version = media.version.clone();
            media.lyrics_tx = Some(lyrics::spawn(media.lyrics.clone(), move || {
                version.fetch_add(1, Ordering::Relaxed);
            }));
        }
        // `PILL_MEDIA_DEMO=1`: una pista inventada sonando, para ver el tab y la
        // bandeja con música sin tocar el reproductor de verdad.
        if std::env::var_os("PILL_MEDIA_DEMO").is_some() {
            media.publish(Some(Track {
                title: "Luz de neón".into(),
                artist: "Marea Alta".into(),
                source: "Spotify".into(),
                playing: true,
                position: Some((84.0, 222.0)),
                read_at: Some(Instant::now()),
                ..Default::default()
            }));
            // Una letra inventada que avanza con la pista, para ver los versos.
            let demo: String = (0..60)
                .map(|i| {
                    let at = 70.0 + i as f32 * 4.0;
                    let text = if i % 6 == 5 { "" } else { DEMO_VERSES[i % DEMO_VERSES.len()] };
                    format!("[{:02}:{:05.2}]{text}\n", (at / 60.0) as u32, at % 60.0)
                })
                .collect();
            if let Ok(mut slot) = media.lyrics.lock() {
                *slot = lyrics::Slot { key: "Luz de neón\u{1}Marea Alta".into(), lines: Some(Arc::new(lyrics::parse(&demo))) };
            }
            return media;
        }
        let shared = media.clone();
        std::thread::Builder::new()
            .name("medios".into())
            .spawn(move || imp::run(shared, rx))
            .expect("hilo de medios");
        media
    }

    pub fn track(&self) -> Option<Track> {
        let mut track = self.track.lock().ok()?.clone()?;
        if let Ok(set) = self.art.lock() {
            track.art = set.art.clone();
            track.backdrop = set.backdrop.clone();
            track.hero = set.hero.clone();
        }
        Some(track)
    }

    /// El panel avisa la escala de su pantalla; si cambia, la portada se rehace.
    pub fn set_scale(&self, scale: f32) {
        self.scale.store(scale.clamp(1.0, 4.0).to_bits(), Ordering::Relaxed);
    }

    fn scale(&self) -> f32 {
        f32::from_bits(self.scale.load(Ordering::Relaxed))
    }

    /// Si se muestra la letra.
    pub fn show_lyrics(&self) -> bool {
        self.show_lyrics.load(Ordering::Relaxed)
    }

    pub fn set_show_lyrics(&self, on: bool) {
        self.show_lyrics.store(on, Ordering::Relaxed);
        save_show_lyrics(on);
    }

    /// La letra de la pista que suena, si ya llegó y es de esta pista.
    /// Apagada (botón «Letra» de Ahora suena) no hay: ni en el notch ni en el panel.
    pub fn lyrics(&self) -> Option<lyrics::Lines> {
        if !self.show_lyrics() {
            return None;
        }
        let key = self.track.lock().ok()?.as_ref().map(track_key)?;
        let slot = self.lyrics.lock().ok()?;
        (slot.key == key).then(|| slot.lines.clone()).flatten()
    }

    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Relaxed)
    }

    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    pub fn control(&self, control: Control) {
        // Que se vea al tiro: la posición cambia antes de que Windows confirme.
        if let Ok(mut slot) = self.track.lock() {
            if let Some(track) = slot.as_mut() {
                let now = track.position_now();
                let target = match (&control, now) {
                    (Control::Seek(to), Some((_, end))) => Some((to.clamp(0.0, end), end)),
                    (Control::Jump(by), Some((pos, end))) => Some(((pos + by).clamp(0.0, end), end)),
                    _ => None,
                };
                if let Some((to, end)) = target {
                    track.position = Some((to, end));
                    track.read_at = Some(Instant::now());
                    if let Ok(mut hold) = self.hold.lock() {
                        *hold = Some((to, Instant::now()));
                    }
                }
                match (&control, now) {
                    (Control::Toggle, _) => {
                        track.position = now;
                        track.read_at = Some(Instant::now());
                        track.playing = !track.playing;
                    }
                    _ => {}
                }
            }
        }
        let _ = self.tx.send(control);
    }

    fn publish(&self, mut track: Option<Track>) {
        // Recién movida la canción, una lectura que todavía trae la posición
        // vieja no la devuelve atrás.
        if let (Some(t), Ok(mut hold)) = (track.as_mut(), self.hold.lock()) {
            if let (Some((to, at)), Some((pos, end))) = (*hold, t.position) {
                let expected = to + if t.playing { at.elapsed().as_secs_f32() } else { 0.0 };
                if at.elapsed() < HOLD_FOR && (pos - expected).abs() > 1.5 {
                    t.position = Some((expected.min(end), end));
                    t.read_at = Some(Instant::now());
                } else {
                    *hold = None;
                }
            }
        }
        // Pista nueva: se pide su letra (una vez por pista).
        if let (Some(t), Some(tx)) = (track.as_ref(), self.lyrics_tx.as_ref()) {
            let key = track_key(t);
            if let Ok(mut asked) = self.asked.lock() {
                if *asked != key {
                    *asked = key.clone();
                    let _ = tx.send(lyrics::Ask {
                        key,
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        duration: t.position.map(|(_, end)| end),
                    });
                }
            }
        }
        if let Ok(mut slot) = self.track.lock() {
            *slot = track;
        }
        self.version.fetch_add(1, Ordering::Relaxed);
    }
}

fn media_file() -> Option<std::path::PathBuf> {
    Some(std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("atic-gpui").join("media.txt"))
}

/// Por omisión hay letra; `letra=no` la apaga (en el notch y en el panel).
fn load_show_lyrics() -> bool {
    let text = media_file().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    !text.lines().any(|l| l.trim() == "letra=no")
}

fn save_show_lyrics(on: bool) {
    let Some(path) = media_file() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, if on { "letra=si\n" } else { "letra=no\n" });
}

/// Qué pista es: título y artista.
fn track_key(track: &Track) -> String {
    format!("{}\u{1}{}", track.title, track.artist)
}

const DEMO_VERSES: [&str; 5] = [
    "Las luces de la ciudad se apagan una a una",
    "y yo sigo acá, contando los semáforos",
    "Neón en el vidrio",
    "un reflejo que no es mío pero se le parece mucho a lo que fui",
    "Luz de neón",
];

/// El nombre de la app que reproduce, de su AppUserModelID.
pub fn source_name(aumid: &str) -> String {
    let lower = aumid.to_lowercase();
    // Algunas apps (Zen, por ejemplo) se presentan con un código: mejor nada.
    if lower.len() >= 12 && lower.chars().all(|c| c.is_ascii_hexdigit()) {
        return String::new();
    }
    let known = [
        ("spotify", "Spotify"),
        ("chrome", "Chrome"),
        ("msedge", "Edge"),
        ("firefox", "Firefox"),
        ("zen", "Zen"),
        ("vlc", "VLC"),
        ("zunemusic", "Reproductor multimedia"),
        ("music", "Música"),
    ];
    if let Some((_, name)) = known.iter().find(|(key, _)| lower.contains(key)) {
        return (*name).into();
    }
    // `Empresa.App_xxxx!App` o `app.exe`: lo legible es el nombre del medio.
    let base = aumid.split('!').next().unwrap_or(aumid);
    let base = base.rsplit(['\\', '/']).next().unwrap_or(base);
    let base = base.split('_').next().unwrap_or(base);
    let base = base.trim_end_matches(".exe");
    base.rsplit('.').next().unwrap_or(base).to_string()
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_core::Interface;
    use windows_future::{AsyncStatus, IAsyncOperation};
    use windows::Media::Control::{
        GlobalSystemMediaTransportControlsSession as Session,
        GlobalSystemMediaTransportControlsSessionManager as Manager,
        GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
    };
    use windows::Storage::Streams::{DataReader, IRandomAccessStreamReference};
    use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

    /// El nivel de la salida se mide a ~30 veces por segundo.
    const TICK: Duration = Duration::from_millis(33);
    /// Cada cuánto se consulta la sesión si nadie apura (un control
    /// despierta al tiro).
    const POLL: Duration = Duration::from_millis(250);
    /// Tras cambiar de pista la carátula se sigue releyendo un rato (una
    /// vez por segundo): el navegador cambia el título primero y la imagen
    /// después.
    const ART_SETTLE: Duration = Duration::from_secs(8);
    /// Lo más que se espera una consulta. Si no vuelve (la sesión de una
    /// pista que ya terminó puede quedarse sin contestar), se la abandona
    /// y la siguiente sale igual, en otro hilo.
    const QUERY_LIMIT: Duration = Duration::from_millis(1200);
    /// Lo más que se espera una operación asíncrona de Windows o de la app.
    const ASYNC_LIMIT: Duration = Duration::from_millis(1000);

    /// En qué llamada va una consulta: si no vuelve, el log dice dónde.
    const STAGES: [&str; 10] = [
        "pedir el administrador",
        "listar sesiones",
        "estado de una sesión",
        "sesión actual",
        "mandar un control",
        "datos de la pista",
        "estado de reproducción",
        "carátula",
        "posición",
        "app de origen",
    ];
    const MANAGER: u32 = 0;
    const SESSIONS: u32 = 1;
    const SESSION_STATUS: u32 = 2;
    const CURRENT: u32 = 3;
    const CONTROL: u32 = 4;
    const PROPERTIES: u32 = 5;
    const PLAYBACK: u32 = 6;
    const THUMBNAIL: u32 = 7;
    const TIMELINE: u32 = 8;
    const SOURCE: u32 = 9;

    /// Espera una operación asíncrona de WinRT con límite de tiempo; si no
    /// termina, la cancela.
    fn wait<T: windows_core::RuntimeType + 'static>(op: windows_core::Result<IAsyncOperation<T>>) -> Option<T> {
        let op = op.ok()?;
        let start = Instant::now();
        loop {
            match op.Status().ok()? {
                AsyncStatus::Completed => return op.GetResults().ok(),
                AsyncStatus::Started => {}
                _ => return None,
            }
            if start.elapsed() > ASYNC_LIMIT {
                let _ = op.Cancel();
                return None;
            }
            std::thread::sleep(Duration::from_millis(4));
        }
    }

    /// Lo último leído de la pista, para saber cuándo releer la carátula.
    #[derive(Default)]
    struct Seen {
        key: String,
        changed_at: Option<Instant>,
        /// Huella de los bytes de la carátula enviada al hilo de imágenes.
        art: Option<u64>,
        /// Consultas desde la última vez que se miró la carátula.
        since_art: u32,
    }

    fn open_meter() -> Option<IAudioMeterInformation> {
        unsafe {
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None).ok()
        }
    }

    /// Mide el nivel de la salida para la onda de la pill. La sesión se
    /// consulta en otro hilo (`poll`), que nunca toca a Windows directo.
    pub fn run(media: Media, rx: mpsc::Receiver<Control>) {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let art_tx = spawn_art(media.clone());
        let shared = media.clone();
        std::thread::Builder::new()
            .name("medios-sondeo".into())
            .spawn(move || poll(shared, rx, art_tx))
            .expect("hilo de sondeo");
        let mut meter = open_meter();
        let mut tick = 0u32;
        loop {
            tick += 1;
            if tick % 15 == 0 {
                // La salida por omisión puede haber cambiado (audífonos).
                meter = open_meter().or(meter);
            }
            let level = meter
                .as_ref()
                .and_then(|m| unsafe { m.GetPeakValue().ok() })
                .unwrap_or(0.0);
            media.level.store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
            std::thread::sleep(TICK);
        }
    }

    /// Lanza una consulta tras otra, como la pill de Atic: cada una en un
    /// hilo propio, con el administrador y la sesión pedidos de nuevo (una
    /// sesión vieja puede no contestar más). Si una no vuelve a tiempo se
    /// abandona: queda colgada sola y la siguiente ya trae la pista nueva.
    fn poll(media: Media, rx: mpsc::Receiver<Control>, art_tx: mpsc::Sender<ArtJob>) {
        let mut seen = Seen::default();
        let mut pending: Vec<Control> = Vec::new();
        loop {
            let controls = std::mem::take(&mut pending);
            let controlled = !controls.is_empty();
            let stage = Arc::new(AtomicU32::new(MANAGER));
            let (done_tx, done_rx) = mpsc::channel();
            let (art, probe) = (art_tx.clone(), stage.clone());
            let mut job_seen = std::mem::take(&mut seen);
            let spawned = std::thread::Builder::new().name("medios-consulta".into()).spawn(move || {
                unsafe {
                    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                }
                let track = query(controls, &mut job_seen, &art, &probe);
                let _ = done_tx.send((track, job_seen));
            });
            if spawned.is_err() {
                std::thread::sleep(POLL);
                continue;
            }
            match done_rx.recv_timeout(QUERY_LIMIT) {
                // Una lectura fallida no es «nada suena»: se queda lo último.
                // Si no, el tab con música parpadea y la letra se recoge.
                Ok((Some(track), back)) => {
                    seen = back;
                    media.publish(track);
                }
                Ok((None, back)) => seen = back,
                Err(_) => {
                    // Lo visto se perdió con la consulta: la próxima vuelve a
                    // mirar la carátula (se descarta si es la misma).
                    if std::env::var_os("PILL_DEBUG").is_some() {
                        let at = STAGES.get(stage.load(Ordering::Relaxed) as usize).unwrap_or(&"?");
                        eprintln!("[medios] una consulta no volvió (en «{at}»); sigue otra");
                    }
                }
            }
            // Después de un control, otra consulta pronto para ver el efecto.
            let wait_for = if controlled { Duration::from_millis(150) } else { POLL };
            if let Ok(first) = rx.recv_timeout(wait_for) {
                pending.push(first);
                pending.extend(std::iter::from_fn(|| rx.try_recv().ok()));
            }
        }
    }

    /// Una consulta: la sesión, los controles pendientes y la pista.
    /// `None` si la consulta falló (se queda lo último); `Some(None)` si de
    /// verdad no hay nada.
    fn query(
        controls: Vec<Control>,
        seen: &mut Seen,
        art_tx: &mpsc::Sender<ArtJob>,
        probe: &AtomicU32,
    ) -> Option<Option<Track>> {
        let at = |stage: u32| probe.store(stage, Ordering::Relaxed);
        at(MANAGER);
        let manager = wait(Manager::RequestAsync())?;
        let Some(session) = pick(&manager, &at) else {
            return Some(None);
        };
        let controlled = !controls.is_empty();
        for control in controls {
            at(CONTROL);
            // La línea de tiempo puede no empezar en cero: las posiciones
            // son relativas a su inicio, igual que las que se leen.
            let timeline = || session.GetTimelineProperties().ok();
            let start = || timeline().and_then(|t| t.StartTime().ok()).map_or(0, |t| t.Duration);
            let op = match control {
                Control::Toggle => session.TryTogglePlayPauseAsync(),
                Control::Next => session.TrySkipNextAsync(),
                Control::Previous => session.TrySkipPreviousAsync(),
                Control::Seek(to) => session.TryChangePlaybackPositionAsync(start() + ticks(to)),
                Control::Jump(by) => {
                    let t = timeline();
                    let start = t.as_ref().and_then(|t| t.StartTime().ok()).map_or(0, |t| t.Duration);
                    let now = t.and_then(|t| t.Position().ok()).map_or(start, |p| p.Duration);
                    session.TryChangePlaybackPositionAsync((now + ticks(by.abs()) * by.signum() as i64).max(start))
                }
            };
            let _ = wait(op);
        }
        if controlled {
            // Darle un respiro a la app antes de leer.
            std::thread::sleep(Duration::from_millis(120));
        }
        read(&session, seen, art_tx, &at)
    }

    fn ticks(seconds: f32) -> i64 {
        (seconds.max(0.0) as f64 * 1e7) as i64
    }

    /// La sesión a mostrar: la primera que esté sonando. Windows llama
    /// «actual» a la última que tomó el control, que puede ser una pestaña
    /// en pausa mientras la música suena en otra app.
    fn pick(manager: &Manager, at: &dyn Fn(u32)) -> Option<Session> {
        at(SESSIONS);
        let sessions = manager.GetSessions().ok()?;
        let count = sessions.Size().unwrap_or(0);
        let all: Vec<Session> = (0..count).filter_map(|i| sessions.GetAt(i).ok()).collect();
        let states: Vec<bool> = all
            .iter()
            .map(|s| {
                at(SESSION_STATUS);
                s.GetPlaybackInfo()
                    .and_then(|info| info.PlaybackStatus())
                    .is_ok_and(|status| status == Status::Playing)
            })
            .collect();
        if std::env::var_os("PILL_DEBUG").is_some() {
            static LAST: Mutex<String> = Mutex::new(String::new());
            let list: Vec<String> = all
                .iter()
                .zip(&states)
                .map(|(s, on)| {
                    format!(
                        "{}{}",
                        s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default(),
                        if *on { " (suena)" } else { "" }
                    )
                })
                .collect();
            // Solo cuando cambia: si no, llena el log.
            let list = format!("{list:?}");
            if let Ok(mut last) = LAST.lock() {
                if *last != list {
                    eprintln!("[medios] sesiones: {list}");
                    *last = list;
                }
            }
        }
        if let Some(i) = states.iter().position(|on| *on) {
            return all.into_iter().nth(i);
        }
        at(CURRENT);
        manager.GetCurrentSession().ok()
    }

    /// Cuánto tapa el fondo arriba, donde es más fuerte.
    const BACKDROP_ALPHA: f32 = 0.9;
    /// La portada en puntos: el ancho del notch por el alto de la portada.
    const HERO_PT: (f32, f32) = (440.0, HERO_H);

    fn read(session: &Session, seen: &mut Seen, art_tx: &mpsc::Sender<ArtJob>, at: &dyn Fn(u32)) -> Option<Option<Track>> {
        at(PROPERTIES);
        let props = wait(session.TryGetMediaPropertiesAsync())?;
        let title = props.Title().map(|t| t.to_string()).unwrap_or_default();
        let artist = props.Artist().map(|a| a.to_string()).unwrap_or_default();
        if title.is_empty() {
            return Some(None);
        }
        at(PLAYBACK);
        let playing = session
            .GetPlaybackInfo()
            .and_then(|info| info.PlaybackStatus())
            .is_ok_and(|status| status == Status::Playing);
        // La carátula se relee al cambiar de pista y, una vez por segundo,
        // durante unos segundos después: muchas apps (el navegador) mandan el
        // título nuevo antes que la imagen. Solo se decodifica si los bytes
        // cambiaron, y en otro hilo.
        let key = format!("{title}\u{1}{artist}");
        let new_track = seen.key != key;
        if new_track {
            seen.key = key.clone();
            seen.changed_at = Some(Instant::now());
        }
        seen.since_art += 1;
        let settling = seen.changed_at.is_some_and(|at| at.elapsed() < ART_SETTLE) && seen.since_art >= 4;
        if new_track || settling {
            seen.since_art = 0;
            at(THUMBNAIL);
            let bytes = props.Thumbnail().ok().and_then(|thumb| read_all(&thumb));
            let hash = bytes.as_ref().map(|b| {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                b.hash(&mut h);
                h.finish()
            });
            // Sin imagen en una pista nueva se borra la anterior; una lectura
            // fallida a medio camino no borra la que ya hay.
            if hash != seen.art && (hash.is_some() || new_track) {
                seen.art = hash;
                let _ = art_tx.send(ArtJob::Thumb { key, title: title.clone(), artist: artist.clone(), bytes });
            }
        }
        // La app informa la posición de cuando la midió (`LastUpdatedTime`),
        // que puede ser hace varios segundos: se adelanta hasta ahora. Todo
        // relativo al inicio de la línea de tiempo, que no siempre es cero.
        at(TIMELINE);
        let position = session.GetTimelineProperties().ok().and_then(|t| {
            let start = t.StartTime().map_or(0, |s| s.Duration);
            let mut pos = (t.Position().ok()?.Duration - start) as f32 / 1e7;
            let end = (t.EndTime().ok()?.Duration - start) as f32 / 1e7;
            if playing {
                if let Ok(updated) = t.LastUpdatedTime() {
                    let now = unsafe { windows::Win32::System::SystemInformation::GetSystemTimeAsFileTime() };
                    let now = (now.dwHighDateTime as i64) << 32 | now.dwLowDateTime as i64;
                    let ago = (now - updated.UniversalTime) as f32 / 1e7;
                    if (0.0..3600.0).contains(&ago) {
                        pos += ago;
                    }
                }
            }
            (end > 1.0).then_some((pos.clamp(0.0, end), end))
        });
        at(SOURCE);
        let source = session.SourceAppUserModelId().map(|s| s.to_string()).unwrap_or_default();
        Some(Some(Track {
            title,
            artist,
            source,
            playing,
            position,
            read_at: Some(Instant::now()),
            ..Default::default()
        }))
    }

    /// Lo que recibe el hilo de imágenes.
    pub(super) enum ArtJob {
        /// La carátula que da Windows (o `None`: la pista no trae).
        Thumb { key: String, title: String, artist: String, bytes: Option<Vec<u8>> },
        /// Una versión grande hallada en línea para la pista `key`.
        HiRes { key: String, bytes: Vec<u8> },
    }

    /// Bajo este lado (en px) la carátula de Windows se busca más grande en
    /// línea: Zen y Firefox la entregan de 60×60.
    const SMALL_ART: u32 = 240;

    /// Cuánto se espera la carátula grande antes de mostrar la chica: así
    /// no se ve primero borrosa y después nítida.
    const HIRES_WAIT: Duration = Duration::from_millis(1500);

    /// El hilo de imágenes: recibe las carátulas y deja lista la versión para
    /// la pill, el fondo y la portada. Si la de Windows es chica, pide una
    /// grande a iTunes y mientras tanto deja la anterior; si la grande no
    /// llega a tiempo, muestra la chica. Las grandes quedan guardadas por
    /// pista: una consulta repetida no vuelve a la chica. Si cambia la
    /// escala del panel, rehace la portada.
    fn spawn_art(media: Media) -> mpsc::Sender<ArtJob> {
        let (tx, rx) = mpsc::channel::<ArtJob>();
        let lookups = tx.clone();
        std::thread::Builder::new()
            .name("caratulas".into())
            .spawn(move || {
                let mut key = String::new();
                let mut bytes: Option<Vec<u8>> = None;
                let mut hires = false;
                let mut used_scale = 0.0f32;
                let mut searched = std::collections::HashSet::<String>::new();
                let mut found = std::collections::HashMap::<String, Vec<u8>>::new();
                // La chica de la pista actual, mientras se espera la grande.
                let mut waiting: Option<(Instant, Option<Vec<u8>>)> = None;
                loop {
                    let timeout = if waiting.is_some() { 100 } else { 400 };
                    let first = match rx.recv_timeout(Duration::from_millis(timeout)) {
                        Ok(job) => Some(job),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut changed = false;
                    // Si llegaron varias seguidas se aplican todas y se dibuja una vez.
                    for job in first.into_iter().chain(std::iter::from_fn(|| rx.try_recv().ok())) {
                        match job {
                            ArtJob::Thumb { key: k, title, artist, bytes: b } => {
                                // Con la grande ya puesta, la chica de la misma pista sobra.
                                if k == key && hires {
                                    continue;
                                }
                                if k == key {
                                    if let Some((_, pending)) = waiting.as_mut() {
                                        *pending = b;
                                        continue;
                                    }
                                }
                                if let Some(big) = found.get(&k) {
                                    (key, bytes, hires, waiting) = (k, Some(big.clone()), true, None);
                                    changed = true;
                                    continue;
                                }
                                let small = b.as_deref().map_or(true, |b| {
                                    image::ImageReader::new(std::io::Cursor::new(b))
                                        .with_guessed_format()
                                        .ok()
                                        .and_then(|r| r.into_dimensions().ok())
                                        .is_none_or(|(w, h)| w.min(h) < SMALL_ART)
                                });
                                hires = false;
                                key = k.clone();
                                if small && itunes::enabled() && searched.insert(k.clone()) {
                                    itunes::spawn_lookup(k, title, artist, lookups.clone());
                                    waiting = Some((Instant::now(), b));
                                } else {
                                    waiting = None;
                                    bytes = b;
                                    changed = true;
                                }
                            }
                            ArtJob::HiRes { key: k, bytes: b } => {
                                if found.len() > 40 {
                                    found.clear();
                                }
                                found.insert(k.clone(), b.clone());
                                if k == key {
                                    (bytes, hires, waiting) = (Some(b), true, None);
                                    changed = true;
                                }
                            }
                        }
                    }
                    // La grande no llegó (o iTunes no la tiene): la chica.
                    if waiting.as_ref().is_some_and(|(since, _)| since.elapsed() > HIRES_WAIT) {
                        bytes = waiting.take().and_then(|(_, b)| b);
                        changed = true;
                    }
                    let scale = media.scale();
                    if !changed && (bytes.is_none() || scale == used_scale) {
                        continue;
                    }
                    used_scale = scale;
                    let set = bytes.as_deref().map(|b| thumbnail(b, scale)).unwrap_or_default();
                    if let Ok(mut slot) = media.art.lock() {
                        *slot = set;
                    }
                    media.version.fetch_add(1, Ordering::Relaxed);
                }
            })
            .expect("hilo de carátulas");
        tx
    }

    /// La carátula (cuadrada, 160 px) y su versión difuminada para el fondo,
    /// en BGRA, que es lo que GPUI dibuja directo. El fondo se difumina
    /// chico (40 px) y GPUI lo estira: queda suave sin costo.
    fn thumbnail(bytes: &[u8], scale: f32) -> ArtSet {
        let Ok(decoded) = image::load_from_memory(bytes) else {
            return ArtSet::default();
        };
        if std::env::var_os("PILL_DEBUG").is_some() {
            eprintln!("[medios] carátula {}×{} (escala {scale})", decoded.width(), decoded.height());
        }
        let side = decoded.width().min(decoded.height());
        let square = decoded.crop_imm(
            (decoded.width() - side) / 2,
            (decoded.height() - side) / 2,
            side,
            side,
        );
        let to_render = |mut rgba: image::RgbaImage| {
            for pixel in rgba.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            Arc::new(RenderImage::new([image::Frame::new(rgba)]))
        };
        let art = square
            .resize_exact(160, 160, image::imageops::FilterType::Triangle)
            .to_rgba8();
        // El difuminado oscurece los bordes (mezcla con lo de afuera): se
        // difumina más grande y se queda con el centro. Después se desvanece
        // hacia abajo en la imagen misma: así nunca llega a las esquinas del
        // notch (GPUI no recorta en redondo) y al crecer el notch no hay un
        // borde que se corte.
        let small = square
            .resize_exact(60, 60, image::imageops::FilterType::Triangle)
            .to_rgba8();
        let blurred = image::imageops::blur(&small, 4.0);
        let mut backdrop = image::imageops::crop_imm(&blurred, 12, 12, 36, 36).to_image();
        let h = backdrop.height() as f32;
        for (_, y, pixel) in backdrop.enumerate_pixels_mut() {
            let t = y as f32 / (h - 1.0);
            let fade = (1.0 - t).powf(1.4);
            pixel.0[3] = (pixel.0[3] as f32 * fade * BACKDROP_ALPHA) as u8;
        }
        // La portada: el ancho del notch por el alto de la portada, a
        // pixeles reales de la pantalla (GPUI la dibuja 1:1, sin estirar),
        // desvanecida en su último tramo para fundirse con el difuminado.
        // Se oscurece un poco hacia abajo para que el título se lea encima.
        let (w, h) = ((HERO_PT.0 * scale).round() as u32, (HERO_PT.1 * scale).round() as u32);
        // Se recorta la imagen original a la proporción de la portada (no al
        // cuadrado y después otra vez): una miniatura 16:9 de YouTube se usa
        // casi entera en vez de una franja angosta agrandada.
        let (sw, sh) = (decoded.width(), decoded.height());
        let (cw, ch) = if sw as f32 / sh as f32 > w as f32 / h as f32 {
            (((sh as f32 * w as f32 / h as f32).round() as u32).clamp(1, sw), sh)
        } else {
            (sw, ((sw as f32 * h as f32 / w as f32).round() as u32).clamp(1, sh))
        };
        let cropped = decoded.crop_imm((sw - cw) / 2, (sh - ch) / 2, cw, ch);
        let mut hero = cropped
            .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        // Las carátulas llegan chicas (y en JPEG): agrandadas mucho se ven los
        // bloques de la compresión. Un difuminado leve, según cuánto se
        // agrandó, los esconde sin que se note borrosa.
        let grow = w as f32 / cw as f32;
        if grow > 1.4 {
            hero = image::imageops::blur(&hero, (grow - 1.0) * 0.45);
        }
        for (_, y, pixel) in hero.enumerate_pixels_mut() {
            let t = y as f32 / (h - 1) as f32;
            let fade = if t < 0.55 { 1.0 } else { 1.0 - ((t - 0.55) / 0.45).powf(1.3) };
            let shade = 1.0 - 0.5 * t * t;
            for c in 0..3 {
                pixel.0[c] = (pixel.0[c] as f32 * shade) as u8;
            }
            pixel.0[3] = (pixel.0[3] as f32 * fade.clamp(0.0, 1.0)) as u8;
        }
        ArtSet {
            art: Some(to_render(art)),
            backdrop: Some(to_render(backdrop)),
            hero: Some(to_render(hero)),
        }
    }

    fn read_all(reference: &IRandomAccessStreamReference) -> Option<Vec<u8>> {
        let stream = wait(reference.OpenReadAsync())?;
        let size = stream.Size().ok()? as u32;
        if size == 0 {
            return None;
        }
        let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).ok()?).ok()?;
        wait(reader.LoadAsync(size).and_then(|op| op.cast::<IAsyncOperation<u32>>()))?;
        let mut bytes = vec![0u8; size as usize];
        reader.ReadBytes(&mut bytes).ok()?;
        Some(bytes)
    }
}

/// Carátulas grandes de la API pública de búsqueda de iTunes, para las apps
/// que le dan a Windows una miniatura diminuta (Zen y Firefox: 60×60).
///
/// Manda a Apple el título y el artista de lo que suena. Se apaga con
/// `PILL_MEDIA_ONLINE=0`. Sin dependencias nuevas: usa el `curl.exe` que
/// trae Windows. Solo acepta un resultado cuyo título y artista coincidan;
/// si no, se queda con la miniatura.
mod itunes {
    use std::process::Command;
    use std::sync::mpsc;

    use super::imp::ArtJob;

    /// Lado de la carátula que se pide (iTunes la escala en su servidor).
    const SIZE: &str = "1000x1000bb";

    pub fn enabled() -> bool {
        std::env::var("PILL_MEDIA_ONLINE").map_or(true, |v| v != "0")
    }

    pub fn spawn_lookup(key: String, title: String, artist: String, tx: mpsc::Sender<ArtJob>) {
        let _ = std::thread::Builder::new().name("itunes".into()).spawn(move || {
            let found = lookup(&title, &artist);
            if std::env::var_os("PILL_DEBUG").is_some() {
                let result = if found.is_some() { "sí" } else { "no" };
                eprintln!("[medios] iTunes «{title}» / «{artist}»: {result}");
            }
            if let Some(bytes) = found {
                let _ = tx.send(ArtJob::HiRes { key, bytes });
            }
        });
    }

    fn lookup(title: &str, artist: &str) -> Option<Vec<u8>> {
        let exact = normalize(title);
        for (title, artists) in candidates(title, artist) {
            let first = artists.first().map(String::as_str).unwrap_or("");
            let url = format!(
                "https://itunes.apple.com/search?media=music&entity=song&limit=10&term={}",
                encode(format!("{first} {title}").trim())
            );
            let Some(json) = fetch(&url).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) else {
                continue;
            };
            let results = json.get("results").and_then(|r| r.as_array()).cloned().unwrap_or_default();
            let fits: Vec<_> = results
                .iter()
                .filter(|r| {
                    let name = r.get("trackName").and_then(|v| v.as_str()).unwrap_or("");
                    let by = r.get("artistName").and_then(|v| v.as_str()).unwrap_or("");
                    matches(&title, &artists, name, by)
                })
                .collect();
            // Entre las versiones («SLOWED», «NIGHTCORE»…), la del título exacto.
            let hit = fits
                .iter()
                .find(|r| r.get("trackName").and_then(|v| v.as_str()).is_some_and(|n| normalize(n) == exact))
                .or(fits.first());
            if let Some(art) = hit.and_then(|r| r.get("artworkUrl100")).and_then(|v| v.as_str()) {
                return fetch(&art.replace("100x100bb", SIZE));
            }
        }
        None
    }

    pub(super) fn fetch(url: &str) -> Option<Vec<u8>> {
        let mut cmd = Command::new("curl.exe");
        cmd.args(["-sSfL", "--max-time", "8", "-A", "Atic-pill-gpui", url]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().ok()?;
        (out.status.success() && !out.stdout.is_empty()).then_some(out.stdout)
    }

    pub(super) fn encode(text: &str) -> String {
        text.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
                b' ' => "+".into(),
                _ => format!("%{b:02X}"),
            })
            .collect()
    }

    /// Minúsculas, sin tildes ni signos, palabras separadas por un espacio.
    pub(super) fn normalize(text: &str) -> String {
        let plain: String = text
            .to_lowercase()
            .chars()
            .map(|c| match c {
                'á' | 'à' | 'ä' | 'â' => 'a',
                'é' | 'è' | 'ë' | 'ê' => 'e',
                'í' | 'ì' | 'ï' | 'î' => 'i',
                'ó' | 'ò' | 'ö' | 'ô' => 'o',
                'ú' | 'ù' | 'ü' | 'û' => 'u',
                'ñ' => 'n',
                c if c.is_alphanumeric() => c,
                _ => ' ',
            })
            .collect();
        plain.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// El título sin lo que agregan los videos: «(Official Video)»,
    /// «[Lyrics]», «(ULTRASLOWED)», « | …», «ft. …».
    pub(super) fn clean_title(title: &str) -> String {
        let mut out = String::new();
        let mut depth = 0i32;
        for c in title.split(" | ").next().unwrap_or(title).chars() {
            match c {
                '(' | '[' | '【' => depth += 1,
                ')' | ']' | '】' => depth = (depth - 1).max(0),
                _ if depth == 0 => out.push(c),
                _ => {}
            }
        }
        let lower = out.to_lowercase();
        let cut = [" ft.", " feat.", " feat ", " ft "]
            .iter()
            .filter_map(|k| lower.find(k))
            .min()
            .unwrap_or(out.len());
        normalize(&out[..cut])
    }

    /// Los artistas por separado: «A, B & C» → [a, b, c]. Sin «- Topic» ni «VEVO».
    pub(super) fn split_artists(artist: &str) -> Vec<String> {
        let artist = artist.replace(" - Topic", "").replace("VEVO", "");
        let mut parts = vec![artist];
        for sep in [",", "&", " feat. ", " feat ", " ft. ", " x ", " X ", " and "] {
            parts = parts.iter().flat_map(|p| p.split(sep).map(str::to_string).collect::<Vec<_>>()).collect();
        }
        parts.iter().map(|p| normalize(p)).filter(|p| !p.is_empty()).collect()
    }

    /// Qué buscar: si el título es «Artista - Canción» (lo típico en
    /// YouTube, donde el «artista» es el canal), primero partido así; después
    /// tal cual.
    pub(super) fn candidates(title: &str, artist: &str) -> Vec<(String, Vec<String>)> {
        let mut out = Vec::new();
        if let Some((left, right)) = title.split_once(" - ") {
            let t = clean_title(right);
            let a = split_artists(left);
            if !t.is_empty() && !a.is_empty() {
                out.push((t, a));
            }
        }
        let t = clean_title(title);
        let a = split_artists(artist);
        if !t.is_empty() && !a.is_empty() {
            out.push((t, a));
        }
        out
    }

    /// El resultado sirve si el nombre coincide con el título y comparte un
    /// artista con lo que suena.
    pub(super) fn matches(title: &str, artists: &[String], name: &str, by: &str) -> bool {
        let name = clean_title(name);
        let by = normalize(by);
        let same_title =
            !name.is_empty() && (name == title || name.starts_with(title) || title.starts_with(name.as_str()));
        let same_artist = artists
            .iter()
            .any(|a| a.len() >= 2 && (by.contains(a.as_str()) || a.contains(by.as_str())));
        same_title && same_artist
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    pub fn run(_: Media, _: mpsc::Receiver<Control>) {}
    pub(super) enum ArtJob {
        HiRes { key: String, bytes: Vec<u8> },
    }
}

// --- Panel ------------------------------------------------------------------

actions!(media_panel, [Dismiss, TogglePlay, NextTrack, PreviousTrack, Back10, Forward10]);

const KEY_CONTEXT: &str = "MediaPanel";

pub fn bind_keys(cx: &mut App) {
    let context = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("escape", Dismiss, context),
        KeyBinding::new("space", TogglePlay, context),
        KeyBinding::new("right", Forward10, context),
        KeyBinding::new("left", Back10, context),
        KeyBinding::new("shift-right", NextTrack, context),
        KeyBinding::new("shift-left", PreviousTrack, context),
    ]);
}

pub enum MediaEvent {
    Close,
}

const SIDE: f32 = 16.0;
const PROGRESS_H: f32 = 26.0;
const CONTROLS_H: f32 = 50.0;
const BOTTOM_H: f32 = 34.0;
/// El slider de volumen, como los de Sistema: un relleno que crece desde la
/// izquierda con el ícono adentro.
const VOLUME_H: f32 = 34.0;
const OUTPUT_ROW_H: f32 = 32.0;
const JUMP: f32 = 10.0;
/// Alto del fondo con la carátula difuminada: se desvanece antes de llegar
/// abajo, así nunca toca las esquinas del notch (GPUI no recorta en redondo).
const BACKDROP_H: f32 = 400.0;
/// Alto de la portada, nítida, detrás del título.
const HERO_H: f32 = 260.0;
/// Espacio entre la franja y el título, que queda sobre la portada.
const HERO_SPACER: f32 = 148.0;
const TITLE_H: f32 = 46.0;
/// Los versos: el anterior, el actual (hasta dos renglones) y el siguiente.
/// Alto fijo: la caja no salta entre versos cortos y largos.
const LYRIC_SIDE_H: f32 = 18.0;
const LYRIC_NOW_H: f32 = 46.0;
const LYRICS_H: f32 = LYRIC_SIDE_H * 2.0 + LYRIC_NOW_H + 12.0;

/// Qué barra se arrastra.
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    /// La canción: la fracción donde va el dedo (se aplica al soltar).
    Seek(f32),
    Volume,
}

pub struct MediaPanel {
    focus: FocusHandle,
    pub media: Media,
    backend: Backend,
    /// Volumen y salidas, del hilo del sistema.
    sound: Option<Snapshot>,
    /// Se relee el sonido mientras el notch muestra Ahora suena.
    pub active: bool,
    outputs_open: bool,
    drag: Option<Drag>,
    progress_bounds: Rc<Cell<Bounds<Pixels>>>,
    volume_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub pinned: bool,
    seen: u64,
}

impl EventEmitter<MediaEvent> for MediaPanel {}

impl Focusable for MediaPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn white(alpha: f32) -> Hsla {
    gpui::white().opacity(alpha)
}

impl MediaPanel {
    pub fn new(media: Media, backend: Backend, cx: &mut Context<Self>) -> Self {
        // Un cambio (pista, pausa, carátula lista) se pinta en ≤100 ms; la
        // barra de progreso avanza sola con dos repintados por segundo.
        let mut ticks = 0u32;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(Duration::from_millis(100)).await;
            ticks = ticks.wrapping_add(1);
            if this
                .update(cx, |panel, cx| {
                    let version = panel.media.version();
                    let advance = ticks % 5 == 0 && panel.media.track().is_some_and(|t| t.playing);
                    if version != panel.seen || advance {
                        panel.seen = version;
                        cx.notify();
                    }
                })
                .is_err()
            {
                break;
            }
        })
        .detach();
        Self {
            focus: cx.focus_handle(),
            media,
            backend,
            sound: None,
            active: false,
            outputs_open: false,
            drag: None,
            progress_bounds: Default::default(),
            volume_bounds: Default::default(),
            pinned: false,
            seen: 0,
        }
    }

    pub fn reset(&mut self, cx: &mut Context<Self>) {
        let first = !self.active;
        self.active = true;
        // `PILL_MEDIA_OUTPUTS=1`: la lista de salidas abierta, para revisar
        // el diseño con una captura.
        self.outputs_open = std::env::var_os("PILL_MEDIA_OUTPUTS").is_some();
        self.refresh(cx);
        if first {
            cx.spawn(async move |this, cx| loop {
                cx.background_executor().timer(Duration::from_millis(1500)).await;
                let go = this
                    .update(cx, |panel, cx| {
                        if panel.active && panel.drag.is_none() {
                            panel.refresh(cx);
                        }
                        panel.active
                    })
                    .unwrap_or(false);
                if !go {
                    break;
                }
            })
            .detach();
        }
        cx.notify();
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
        let reply = self.backend.read(Want::default());
        cx.spawn(async move |this, cx| {
            if let Ok(snapshot) = reply.await {
                let _ = this.update(cx, |panel, cx| {
                    if panel.drag != Some(Drag::Volume) {
                        panel.sound = Some(snapshot);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    pub fn desired_height(&self) -> f32 {
        let outputs = if self.outputs_open {
            8.0 + self.sound.as_ref().map_or(0, |s| s.outputs.len()) as f32 * OUTPUT_ROW_H
        } else {
            0.0
        };
        // Sin posición (YouTube en el navegador no la informa) no hay barra.
        let progress = if self.timeline() { PROGRESS_H + 6.0 } else { 0.0 };
        let lyrics = if self.lyric().is_some() { LYRICS_H } else { 0.0 };
        BAND_H + HERO_SPACER + TITLE_H + 12.0 + progress + CONTROLS_H + lyrics + 10.0 + BOTTOM_H + outputs + 14.0
    }

    /// El verso que suena y sus vecinos, si la pista tiene letra
    /// sincronizada: (índice, anterior, actual, siguiente). Antes del primer
    /// verso el actual va vacío.
    fn lyric(&self) -> Option<(Option<usize>, String, String, String)> {
        let lines = self.media.lyrics()?;
        let (position, _) = self.media.track()?.position_now()?;
        let i = lyrics::index(&lines, position);
        let text = |at: Option<usize>| at.and_then(|at| lines.get(at)).map(|l| l.text.clone()).unwrap_or_default();
        Some((
            i,
            text(i.and_then(|i| i.checked_sub(1))),
            text(i),
            text(Some(i.map_or(0, |i| i + 1))),
        ))
    }

    /// A lo Apple Music: el verso actual grande y blanco, sube con un
    /// fundido al cambiar; los vecinos chicos y apagados.
    fn render_lyrics(&self) -> Option<impl IntoElement> {
        let (index, prev, now, next) = self.lyric()?;
        let side = |text: String| {
            div()
                .h(px(LYRIC_SIDE_H))
                .w_full()
                .text_center()
                .truncate()
                .text_size(px(12.5))
                .text_color(white(0.32))
                .child(SharedString::from(text))
        };
        let key = index.map_or(0, |i| i + 1);
        let current = div()
            .id(SharedString::from(format!("media-lyric-{key}")))
            .relative()
            .h(px(LYRIC_NOW_H))
            .w_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w_full()
                    .text_center()
                    .line_clamp(2)
                    .text_size(px(17.))
                    .line_height(px(21.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(SharedString::from(if now.trim().is_empty() { "♪".to_string() } else { now })),
            )
            .with_animation(
                SharedString::from(format!("media-lyric-in-{key}")),
                Animation::new(Duration::from_millis(220)).with_easing(gpui::ease_out_quint()),
                |el, t| el.opacity(0.3 + 0.7 * t).top(px(8.0 * (1.0 - t))),
            );
        Some(
            div()
                .h(px(LYRICS_H))
                .px(px(SIDE + 8.))
                .py(px(6.))
                .flex()
                .flex_col()
                .child(side(prev))
                .child(current)
                .child(side(next)),
        )
    }

    /// Encender o apagar la letra: la que cuelga del notch y los versos del panel.
    fn render_hang_toggle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.media.show_lyrics();
        let fg: Hsla = if on { rgb(0xf0f0ea).into() } else { rgb(0x8a8a82).into() };
        div()
            .id("media-hang-toggle")
            .h(px(24.))
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(12.))
            .bg(white(if on { 0.16 } else { 0.06 }))
            .hover(|el| el.bg(white(0.22)))
            .cursor_pointer()
            .text_size(px(11.))
            .text_color(fg)
            .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| {
                let on = !panel.media.show_lyrics();
                panel.media.set_show_lyrics(on);
                cx.notify();
            }))
            .child(svg().path("icons/mic-vocal.svg").size(px(13.)).text_color(fg))
            .child(if on { "Letra" } else { "Sin letra" })
    }

    /// La app informa posición y duración: se puede mover la canción.
    fn timeline(&self) -> bool {
        self.media.track().is_some_and(|t| t.position.is_some())
    }

    fn fraction(bounds: &Rc<Cell<Bounds<Pixels>>>, x: Pixels) -> f32 {
        let b = bounds.get();
        let w = f32::from(b.size.width).max(1.0);
        (f32::from(x - b.origin.x) / w).clamp(0.0, 1.0)
    }

    fn set_volume(&mut self, value: f32, cx: &mut Context<Self>) {
        if let Some(sound) = self.sound.as_mut() {
            sound.volume = value;
            if sound.muted {
                sound.muted = false;
                self.backend.send(Cmd::Mute(false));
            }
        }
        self.backend.send(Cmd::Volume(value));
        cx.notify();
    }

    fn round_button(
        &self,
        id: &'static str,
        icon: &'static str,
        badge: Option<&'static str>,
        big: bool,
        control: fn() -> Control,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let size = if big { CONTROLS_H } else { 38.0 };
        let fg: Hsla = if big { rgb(0x1a1a18).into() } else { rgb(0xf0f0ea).into() };
        div()
            .id(id)
            .relative()
            .size(px(size))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(size / 2.0))
            .when(big, |el| el.bg(rgb(0xf0f0ea)))
            .when(!big, |el| el.hover(|el| el.bg(white(0.12))))
            .cursor_pointer()
            .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                panel.media.control(control());
                cx.notify();
            }))
            .child(svg().path(icon).size(px(if big { 22. } else { 19. })).text_color(fg))
            .children(badge.map(|text| {
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .pt(px(1.))
                    .text_size(px(7.5))
                    .text_color(fg)
                    .child(text)
            }))
    }

    fn render_progress(&self, track: &Track, cx: &mut Context<Self>) -> impl IntoElement {
        let muted: Hsla = rgb(0xb8b8ae).into();
        let store = self.progress_bounds.clone();
        let (pos, end) = track.position_now().unwrap_or((0.0, 0.0));
        let known = end > 0.0;
        let fraction = match self.drag {
            Some(Drag::Seek(f)) => f,
            _ if known => (pos / end).clamp(0.0, 1.0),
            _ => 0.0,
        };
        let shown_pos = if known { fraction * end } else { 0.0 };
        let dragging = matches!(self.drag, Some(Drag::Seek(_)));
        let bar_h = if dragging { 7.0 } else { 5.0 };
        div()
            .h(px(PROGRESS_H))
            .px(px(SIDE))
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(10.))
            .text_color(muted)
            .child(div().w(px(32.)).child(if known { clock(shown_pos) } else { String::new() }))
            .child(
                div()
                    .id("media-progress")
                    .flex_1()
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .when(known, |el| el.cursor_pointer())
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(bar_h))
                            .rounded(px(bar_h / 2.0))
                            .bg(white(0.18))
                            .child(
                                canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {})
                                    .absolute()
                                    .size_full(),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .rounded(px(bar_h / 2.0))
                                    .bg(white(if dragging { 1.0 } else { 0.9 }))
                                    .w(gpui::relative(fraction)),
                            ),
                    )
                    .when(known, |el| {
                        el.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                                let f = Self::fraction(&panel.progress_bounds, event.position.x);
                                panel.drag = Some(Drag::Seek(f));
                                cx.notify();
                            }),
                        )
                    }),
            )
            .child(
                div()
                    .w(px(32.))
                    .flex()
                    .justify_end()
                    .child(if known { format!("-{}", clock(end - shown_pos)) } else { String::new() }),
            )
    }

    /// Del cursor al volumen: el centro de la punta redonda queda bajo el
    /// cursor, como en los sliders de Sistema.
    fn volume_at(&self, x: Pixels) -> f32 {
        let b = self.volume_bounds.get();
        let w = f32::from(b.size.width).max(VOLUME_H + 1.0);
        ((f32::from(x - b.origin.x) - VOLUME_H / 2.0) / (w - VOLUME_H)).clamp(0.0, 1.0)
    }

    fn render_bottom(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted: Hsla = rgb(0xb8b8ae).into();
        let ink: Hsla = rgb(0x1a1a18).into();
        let sound = self.sound.clone().unwrap_or_default();
        let volume = if sound.muted { 0.0 } else { sound.volume.clamp(0.0, 1.0) };
        let store = self.volume_bounds.clone();
        let width = f32::from(store.get().size.width);
        let fill = if width > VOLUME_H {
            px(VOLUME_H + (width - VOLUME_H) * volume)
        } else {
            px(VOLUME_H)
        };
        let dragging = self.drag == Some(Drag::Volume);
        let icon = if volume <= 0.001 {
            "icons/volume-x.svg"
        } else if volume < 0.5 {
            "icons/volume-1.svg"
        } else {
            "icons/volume-2.svg"
        };
        let output = sound
            .outputs
            .iter()
            .find(|o| o.default)
            .map(|o| o.name.split(" (").next().unwrap_or(&o.name).chars().take(22).collect::<String>())
            .unwrap_or_else(|| "Salida".into());
        let lower = output.to_lowercase();
        let headphones = ["auricular", "headphone", "headset", "airpods", "buds"]
            .iter()
            .any(|k| lower.contains(k));
        div()
            .h(px(BOTTOM_H))
            .px(px(SIDE))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id("media-volume")
                    .relative()
                    .flex_1()
                    .h(px(VOLUME_H))
                    .rounded(px(VOLUME_H / 2.0))
                    .bg(white(0.12))
                    .overflow_hidden()
                    .cursor_pointer()
                    .child(
                        canvas(move |bounds, _, _| store.set(bounds), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .left_0()
                            .h_full()
                            .w(fill)
                            .rounded(px(VOLUME_H / 2.0))
                            .bg(if dragging { gpui::white() } else { rgb(0xe8e8e0).into() }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .size(px(VOLUME_H))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(svg().path(icon).size(px(15.)).text_color(ink)),
                    )
                    .child(
                        div()
                            .absolute()
                            .right(px(12.))
                            .top_0()
                            .h_full()
                            .flex()
                            .items_center()
                            .text_size(px(10.))
                            .text_color(if volume > 0.85 { ink } else { muted })
                            .child(format!("{:.0} %", volume * 100.0)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|panel, event: &MouseDownEvent, _, cx| {
                            panel.drag = Some(Drag::Volume);
                            let value = panel.volume_at(event.position.x);
                            panel.set_volume(value, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("media-output")
                    .h(px(VOLUME_H))
                    .px(px(12.))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(VOLUME_H / 2.0))
                    .bg(white(if self.outputs_open { 0.2 } else { 0.12 }))
                    .hover(|el| el.bg(white(0.2)))
                    .cursor_pointer()
                    .text_size(px(11.))
                    .on_click(cx.listener(|panel, _: &ClickEvent, _, cx| {
                        panel.outputs_open = !panel.outputs_open;
                        panel.refresh(cx);
                        cx.notify();
                    }))
                    .child(
                        svg()
                            .path(if headphones { "icons/headphones.svg" } else { "icons/speaker.svg" })
                            .size(px(14.))
                            .text_color(rgb(0xf0f0ea)),
                    )
                    .child(output)
                    .child(
                        svg()
                            .path(if self.outputs_open { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" })
                            .size(px(12.))
                            .text_color(muted),
                    ),
            )
    }

    fn render_outputs(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        if !self.outputs_open {
            return None;
        }
        let outputs = self.sound.as_ref()?.outputs.clone();
        let muted: Hsla = rgb(0xb8b8ae).into();
        Some(
            div()
                .mt(px(8.))
                .px(px(SIDE - 6.))
                .flex()
                .flex_col()
                .children(outputs.into_iter().map(|output| {
                    let (short, card) = match output.name.split_once(" (") {
                        Some((short, rest)) => (short.to_string(), rest.trim_end_matches(')').to_string()),
                        None => (output.name.clone(), String::new()),
                    };
                    let id = output.id.clone();
                    div()
                        .id(SharedString::from(format!("media-out-{}", output.id)))
                        .h(px(OUTPUT_ROW_H))
                        .px(px(6.))
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .rounded(px(10.))
                        .hover(|el| el.bg(white(0.1)))
                        .cursor_pointer()
                        .on_click(cx.listener(move |panel, _: &ClickEvent, _, cx| {
                            if let Some(sound) = panel.sound.as_mut() {
                                for o in &mut sound.outputs {
                                    o.default = o.id == id;
                                }
                            }
                            panel.backend.send(Cmd::Output(id.clone()));
                            panel.outputs_open = false;
                            panel.refresh(cx);
                            cx.notify();
                        }))
                        .child(svg().path("icons/speaker.svg").size(px(14.)).text_color(muted))
                        .child(div().text_size(px(12.)).child(short))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(10.))
                                .text_color(muted)
                                .child(card),
                        )
                        .when(output.default, |el| {
                            el.child(
                                svg()
                                    .path("icons/circle-check.svg")
                                    .size(px(14.))
                                    .text_color(rgb(0xf0f0ea)),
                            )
                        })
                })),
        )
    }
}

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

impl Render for MediaPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.media.set_scale(window.scale_factor());
        let text: Hsla = rgb(0xf0f0ea).into();
        let muted: Hsla = rgb(0xb8b8ae).into();
        let track = self.media.track();
        let source = track.as_ref().map(|t| source_name(&t.source)).unwrap_or_default();

        // Fondo: la carátula difuminada, y encima un degradado que oscurece
        // hacia abajo para que los controles se lean sobre cualquier imagen.
        // Fondo: la carátula difuminada (tamaño fijo, ya desvanecida hacia
        // abajo) y encima la portada nítida, que se funde en ella. Un velo
        // suave arriba deja leer la franja sobre carátulas claras.
        let backdrop = track.as_ref().map(|t| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(px(BACKDROP_H))
                .children(t.backdrop.clone().map(|image| {
                    img(image).absolute().inset_0().size_full().object_fit(ObjectFit::Fill)
                }))
                .children(t.hero.clone().map(|image| {
                    img(image)
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(px(HERO_H))
                        .object_fit(ObjectFit::Fill)
                }))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(px(90.))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(gpui::black().opacity(0.45), 0.0),
                            linear_color_stop(gpui::black().opacity(0.0), 1.0),
                        )),
                )
        });

        let band = div()
            .h(px(BAND_H))
            .flex()
            .items_center()
            .gap(px(8.))
            .pr(px(SIDE))
            .child(
                div()
                    .id("media-mark")
                    .w(px(40.))
                    .h_full()
                    .flex_none()
                    .cursor_pointer()
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(MediaEvent::Close))),
            )
            .child(div().text_size(px(12.)).child("Ahora suena"))
            .child(div().flex_1())
            .when(!source.is_empty(), |el| {
                el.child(div().text_size(px(11.)).text_color(muted).child(source.clone()))
            })
            .when(track.is_some(), |el| el.child(self.render_hang_toggle(cx)));

        let Some(t) = track.as_ref() else {
            return div()
                .key_context(KEY_CONTEXT)
                .track_focus(&self.focus)
                .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(MediaEvent::Close)))
                .size_full()
                .flex()
                .flex_col()
                .font_family("Segoe UI")
                .text_color(text)
                .child(band)
                .child(
                    div()
                        .px(px(SIDE))
                        .pt(px(16.))
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("Nada suena ahora."),
                )
                .into_any_element();
        };

        let playing = t.playing;
        // Sin carátula, un ícono grande ocupa el lugar de la portada.
        let placeholder = (t.hero.is_none()).then(|| {
            div()
                .absolute()
                .top(px(BAND_H))
                .left_0()
                .right_0()
                .h(px(HERO_SPACER))
                .flex()
                .items_center()
                .justify_center()
                .child(svg().path("icons/audio-lines.svg").size(px(48.)).text_color(white(0.25)))
        });
        let header = div()
            .h(px(TITLE_H))
            .px(px(SIDE))
            .flex()
            .flex_col()
            .justify_end()
            .gap(px(2.))
            .child(
                div()
                    .text_size(px(18.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .truncate()
                    .child(SharedString::from(t.title.clone())),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(white(0.8))
                    .truncate()
                    .child(SharedString::from(t.artist.clone())),
            );
        let timeline = t.position.is_some();
        let controls = div()
            .h(px(CONTROLS_H))
            .flex()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .when(timeline, |el| {
                el.child(self.round_button("media-back", "icons/rotate-ccw.svg", Some("10"), false, || Control::Jump(-JUMP), cx))
            })
            .child(self.round_button("media-prev", "icons/skip-back.svg", None, false, || Control::Previous, cx))
            .child(self.round_button(
                "media-toggle",
                if playing { "icons/pause.svg" } else { "icons/play.svg" },
                None,
                true,
                || Control::Toggle,
                cx,
            ))
            .child(self.round_button("media-next", "icons/skip-forward.svg", None, false, || Control::Next, cx))
            .when(timeline, |el| {
                el.child(self.round_button("media-forward", "icons/rotate-cw.svg", Some("10"), false, || Control::Jump(JUMP), cx))
            });

        let progress = timeline.then(|| self.render_progress(t, cx));
        let lyrics_block = self.render_lyrics();
        let bottom = self.render_bottom(cx);
        let outputs = self.render_outputs(cx);

        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &Dismiss, _, cx| cx.emit(MediaEvent::Close)))
            .on_action(cx.listener(|panel, _: &TogglePlay, _, cx| {
                panel.media.control(Control::Toggle);
                cx.notify();
            }))
            .on_action(cx.listener(|panel, _: &NextTrack, _, _| panel.media.control(Control::Next)))
            .on_action(cx.listener(|panel, _: &PreviousTrack, _, _| panel.media.control(Control::Previous)))
            .on_action(cx.listener(|panel, _: &Back10, _, cx| {
                panel.media.control(Control::Jump(-JUMP));
                cx.notify();
            }))
            .on_action(cx.listener(|panel, _: &Forward10, _, cx| {
                panel.media.control(Control::Jump(JUMP));
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|panel, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button != Some(MouseButton::Left) {
                    return;
                }
                match panel.drag {
                    Some(Drag::Seek(_)) => {
                        let f = Self::fraction(&panel.progress_bounds, event.position.x);
                        panel.drag = Some(Drag::Seek(f));
                        cx.notify();
                    }
                    Some(Drag::Volume) => {
                        let value = panel.volume_at(event.position.x);
                        panel.set_volume(value, cx);
                    }
                    None => {}
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|panel, _: &MouseUpEvent, _, cx| {
                    // La canción se mueve al soltar: arrastrando no se manda
                    // un salto por cada píxel.
                    if let Some(Drag::Seek(f)) = panel.drag {
                        if let Some((_, end)) = panel.media.track().and_then(|t| t.position_now()) {
                            panel.media.control(Control::Seek(f * end));
                        }
                    }
                    panel.drag = None;
                    cx.notify();
                }),
            )
            .relative()
            .size_full()
            .font_family("Segoe UI")
            .text_color(text)
            .children(backdrop)
            .children(placeholder)
            .child(
                // El alto final desde el principio: mientras el notch crece
                // (al desplegar las salidas) nada se encoge ni se empuja;
                // el notch solo va destapando lo nuevo.
                div()
                    .relative()
                    .w_full()
                    .h(px(self.desired_height()))
                    .flex_none()
                    .flex()
                    .flex_col()
                    .child(band)
                    .child(div().h(px(HERO_SPACER)).flex_none())
                    .child(header)
                    .child(div().h(px(12.)).flex_none())
                    .children(progress.map(|p| div().pb(px(6.)).child(p)))
                    .child(controls)
                    .children(lyrics_block)
                    .child(div().h(px(10.)))
                    .child(bottom)
                    .children(outputs),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nombres_de_apps() {
        assert_eq!(source_name("Spotify.exe"), "Spotify");
        assert_eq!(source_name("F0DC299D809B9700"), "");
        assert_eq!(source_name("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify"), "Spotify");
        assert_eq!(source_name("Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic"), "Reproductor multimedia");
    }

    #[test]
    fn la_posicion_avanza_sola_mientras_suena() {
        let track = Track {
            playing: true,
            position: Some((10.0, 200.0)),
            read_at: Some(Instant::now() - Duration::from_secs(5)),
            ..Track::default()
        };
        let (pos, end) = track.position_now().unwrap();
        assert!((14.9..15.5).contains(&pos));
        assert_eq!(end, 200.0);
        let paused = Track { playing: false, ..track };
        assert_eq!(paused.position_now().unwrap().0, 10.0);
    }
    #[test]
    fn titulos_y_artistas_para_buscar_en_itunes() {
        use itunes::*;
        assert_eq!(clean_title("VOCALOID FUNK (ULTRASLOWED)"), "vocaloid funk");
        assert_eq!(clean_title("Canción [Official Video] | Sello"), "cancion");
        assert_eq!(clean_title("Tema ft. Otro"), "tema");
        assert_eq!(split_artists("FXRCE, Scythermane, & Lurk"), vec!["fxrce", "scythermane", "lurk"]);
        assert_eq!(split_artists("Daft Punk - Topic"), vec!["daft punk"]);
        let c = candidates("Daft Punk - One More Time (Official Video)", "DaftPunkVEVO");
        assert_eq!(c[0], ("one more time".to_string(), vec!["daft punk".to_string()]));
        let artists = split_artists("FXRCE, Scythermane, & Lurk");
        assert!(matches("vocaloid funk", &artists, "VOCALOID FUNK (Slowed)", "FXRCE & Scythermane"));
        assert!(!matches("vocaloid funk", &artists, "Otra Canción", "FXRCE"));
        assert!(!matches("vocaloid funk", &artists, "Vocaloid Funk", "Alguien Más"));
    }
}
