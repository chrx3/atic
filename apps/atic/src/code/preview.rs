//! La vista previa de Atic Code: una página web en el panel de la terminal (una
//! pestaña más), con la vista web del sistema (wry: WKWebView en macOS, WebView2 en
//! Windows). Muestra el servidor local que levantó un agente (`localhost:5173`) o un
//! HTML que escribió, sin salir de Atic Code.
//!
//! La página la dibuja el sistema encima de la ventana: GPUI no puede pintar sobre
//! ella ni recortarla. Por eso:
//! - se mueve cada cuadro al rectángulo que le deja el panel, con un margen para que
//!   no asome por las esquinas redondeadas;
//! - con un menú o un diálogo de GPUI encima (`covered`), se cambia por una foto fija
//!   de la página (macOS) o se oculta (Windows), y vuelve al cerrarse;
//! - oculta un buen rato, se suelta (los procesos de WebKit liberan su memoria) y se
//!   vuelve a crear al mostrarla.
//!
//! Copiar y pegar: WebKit espera esos comandos del menú Edición, y GPUI no se los
//! pasa. Un script en cada página los hace en el mismo `keydown`; pegar se lo pide a
//! Rust, que lee el portapapeles (`EDIT_KEYS`).
//!
//! Lo que se tomó de otras herramientas (cmux, Live Preview de VS Code) y de los
//! problemas conocidos de WebKit:
//! - un servidor local que todavía no responde no muestra un error: se espera hasta
//!   que conteste (`waiting`) y ahí se carga;
//! - atajos dentro de la página (Cmd+R, Cmd+L, Cmd+[ ], zoom, Alt+Cmd+I) y gestos de
//!   atrás y adelante;
//! - el fondo bajo la página es el del panel (`underPageBackgroundColor`), sin el gris
//!   de WebKit al cargar o al rebotar;
//! - si el proceso de la página se cae, deja de contestar: se rehace en la misma
//!   dirección (`health_check`), como cmux;
//! - un HTML del disco se recarga cuando un agente edita algo de su carpeta.
//!
//! Lo probado está en `examples/webview_probe.rs`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::channel::mpsc::UnboundedSender;
use gpui::{Bounds, Image, Pixels, Window};
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::http::{header, Response, StatusCode};
use wry::raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
use wry::{NewWindowResponse, PageLoadEvent, Rect, WebView, WebViewBuilder};

/// La clave de la pestaña de la vista previa entre las de la terminal.
pub const TAB_ID: u64 = u64::MAX;
/// Oculta más que esto, la vista web se suelta.
pub const RELEASE_AFTER: Duration = Duration::from_secs(120);
/// El protocolo con que se sirven los HTML locales: `file://` no carga en WKWebView
/// sin `loadFileURL`, que wry no usa.
const FILE_SCHEME: &str = "aticfile";

/// Los atajos dentro de la página: la edición se hace aquí mismo (es un gesto del
/// usuario, así WebKit la permite) y lo demás se le pide a Atic Code (`Msg::Key`).
const EDIT_KEYS: &str = r#"
addEventListener('keydown', e => {
  const mod = navigator.platform.startsWith('Mac') ? e.metaKey : e.ctrlKey;
  if (!mod) return;
  const k = e.key.toLowerCase();
  const send = m => { e.preventDefault(); e.stopPropagation(); window.ipc.postMessage(m); };
  if (e.altKey) { if (e.code === 'KeyI') send('devtools'); return; }
  if (k === 'v' && !e.shiftKey) return send('paste');
  const nav = { r: 'reload', l: 'address', '[': 'back', ']': 'forward', '=': 'zoomIn', '+': 'zoomIn', '-': 'zoomOut', '0': 'zoomReset' }[k];
  if (nav) return send(nav);
  const cmd = { c: 'copy', x: 'cut', a: 'selectAll', z: e.shiftKey ? 'redo' : 'undo' }[k];
  if (cmd) { e.preventDefault(); document.execCommand(cmd); }
}, true);
// Un clic en la página: la barra de dirección deja de verse con el foco.
addEventListener('mousedown', () => window.ipc.postMessage('focused'), true);
"#;
/// Sin respuesta de la página en este tiempo, su proceso se cayó: se rehace.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(8);
/// Cada cuánto se pregunta, con la página a la vista.
const HEALTH_EVERY: Duration = Duration::from_secs(10);
const ZOOM_STEPS: [f64; 9] = [0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 2.0];

/// Lo que avisa la vista web (desde sus callbacks) a la terminal.
pub enum Msg {
    Loading(String),
    Loaded(String),
    Title(String),
    /// Un atajo de la página (`reload`, `address`, `back`, `paste`…).
    Key(String),
    /// La página contestó la pregunta de `health_check`.
    Pong,
    /// Un enlace que pidió otra ventana: va al navegador.
    External(String),
    /// La foto fija pedida antes de taparla (`None`: no se pudo).
    Snapshot(Option<Arc<Image>>),
}

pub struct Preview {
    /// La dirección a la vista (la que se pidió o adonde navegó la página).
    pub url: String,
    pub title: Option<String>,
    pub loading: bool,
    web: Option<Rc<WebView>>,
    events: UnboundedSender<Msg>,
    /// La carpeta que puede servir `aticfile://` (la del HTML abierto).
    root: Rc<RefCell<Option<PathBuf>>>,
    /// Está a la vista (no oculta ni cambiada por la foto).
    live: bool,
    /// Desde cuándo no está a la vista.
    pub hidden_since: Option<Instant>,
    /// La foto fija que la reemplaza mientras algo de GPUI la tapa.
    pub snapshot: Option<Arc<Image>>,
    snapping: bool,
    /// Las herramientas de desarrollo estaban abiertas en la última vuelta.
    inspector_open: bool,
    /// El servidor local que se espera que conteste (`localhost:5173`) antes de cargar.
    pub waiting: Option<String>,
    /// Cambia con cada navegación: una espera vieja no carga encima de una nueva.
    pub nav_gen: u64,
    zoom: f64,
    /// El fondo bajo la página (el del panel), en RGBA.
    bg: Option<[f32; 4]>,
    ping_sent: Option<Instant>,
    last_ping: Instant,
    /// La vista web pedida en el cuadro, que la terminal crea afuera (`take_new_web`).
    new_web: Option<NewWeb>,
    /// Se está creando la vista web: no se pide otra.
    building: bool,
}

impl Preview {
    pub fn new(url: String, events: UnboundedSender<Msg>) -> Self {
        let root = Rc::new(RefCell::new(None));
        let url = local_url(&url, &root).unwrap_or(url);
        let loading = !url.is_empty();
        let waiting = local_addr(&url).map(|_| short_url(&url));
        Self {
            url,
            title: None,
            loading,
            web: None,
            events,
            root,
            live: false,
            hidden_since: None,
            snapshot: None,
            snapping: false,
            inspector_open: false,
            waiting,
            nav_gen: 0,
            zoom: 1.0,
            bg: None,
            ping_sent: None,
            last_ping: Instant::now(),
            new_web: None,
            building: false,
        }
    }

    /// El servidor que se espera y la navegación a la que pertenece la espera.
    pub fn waiting_target(&self) -> Option<(u64, String, u16)> {
        self.waiting.as_ref()?;
        let (host, port) = local_addr(&self.url)?;
        Some((self.nav_gen, host, port))
    }

    /// El servidor contestó (o se dejó de esperar): se carga la página.
    pub fn ready(&mut self, generation: u64) {
        if generation != self.nav_gen || self.waiting.take().is_none() {
            return;
        }
        if let Some(web) = &self.web {
            let _ = web.load_url(&self.url);
        }
    }

    /// Lleva la vista a `url` (una dirección escrita, un servidor local o un HTML).
    pub fn navigate(&mut self, url: &str) {
        if url.trim().is_empty() {
            return;
        }
        let url = local_url(url, &self.root).unwrap_or_else(|| normalize_url(url));
        self.url = url.clone();
        self.title = None;
        self.loading = true;
        self.nav_gen += 1;
        // Un servidor local se espera hasta que conteste (lo hace la terminal).
        self.waiting = local_addr(&url).map(|_| short_url(&url));
        if let (Some(web), None) = (&self.web, &self.waiting) {
            let _ = web.load_url(&url);
        }
    }

    pub fn forward(&self) {
        if let Some(web) = &self.web {
            let _ = web.evaluate_script("history.forward()");
        }
    }

    /// Un paso de zoom (`None`: al 100 %).
    pub fn zoom(&mut self, step: Option<i32>) {
        let at = ZOOM_STEPS.iter().position(|z| (*z - self.zoom).abs() < 0.01).unwrap_or(4) as i32;
        self.zoom = match step {
            Some(step) => ZOOM_STEPS[(at + step).clamp(0, ZOOM_STEPS.len() as i32 - 1) as usize],
            None => 1.0,
        };
        if let Some(web) = &self.web {
            let _ = web.zoom(self.zoom);
        }
    }

    /// Un agente editó estos archivos: un HTML del disco se recarga si son de su carpeta.
    pub fn files_changed(&mut self, paths: &[PathBuf]) {
        let root = self.root.borrow().clone();
        let local = self.url.starts_with(FILE_SCHEME) || self.url.contains(&format!("{FILE_SCHEME}.localhost"));
        if let (true, Some(root)) = (local, root) {
            if paths.iter().any(|path| path.starts_with(&root)) {
                self.reload();
            }
        }
    }

    /// Cada tanto, con la página a la vista: le pregunta algo; si no contesta, su
    /// proceso se cayó y la vista se suelta para rehacerla (`true`: hay que redibujar).
    pub fn health_check(&mut self) -> bool {
        let Some(web) = self.web.as_ref().filter(|_| self.live && self.waiting.is_none()) else {
            self.ping_sent = None;
            return false;
        };
        if let Some(sent) = self.ping_sent {
            if sent.elapsed() > HEALTH_TIMEOUT {
                log::warn!("vista previa: la página no contesta, se rehace ({})", self.url);
                self.drop_web();
                self.live = false;
                self.ping_sent = None;
                return true;
            }
            return false;
        }
        if self.last_ping.elapsed() >= HEALTH_EVERY {
            let events = self.events.clone();
            if web.evaluate_script_with_callback("1", move |_| { let _ = events.unbounded_send(Msg::Pong); }).is_ok() {
                self.ping_sent = Some(Instant::now());
            }
            self.last_ping = Instant::now();
        }
        false
    }

    pub fn reload(&mut self) {
        if let Some(web) = &self.web {
            self.loading = true;
            let _ = web.reload();
        }
    }

    /// Abre las herramientas de desarrollo de la página, en su propia ventana.
    pub fn inspect(&mut self) {
        if let Some(web) = &self.web {
            web.open_devtools();
            detach_inspector(web);
            self.inspector_open = true;
        }
    }

    /// «Inspeccionar elemento» (clic derecho) las abre ancladas dentro de la ventana, y
    /// tapaban Atic Code entero: apenas aparecen, pasan a su propia ventana.
    pub fn keep_inspector_detached(&mut self) {
        let Some(web) = &self.web else {
            return;
        };
        let open = web.is_devtools_open();
        if open && !self.inspector_open {
            detach_inspector(web);
        }
        self.inspector_open = open;
    }

    pub fn back(&self) {
        if let Some(web) = &self.web {
            let _ = web.evaluate_script("history.back()");
        }
    }

    /// Lo que se ve en la pestaña: el título de la página o su dirección corta.
    pub fn label(&self) -> String {
        if self.url.is_empty() {
            return crate::i18n::t("pill.code.preview.open").to_string();
        }
        match &self.title {
            Some(title) if !title.trim().is_empty() => title.trim().to_string(),
            _ => short_url(&self.url),
        }
    }

    /// Lo que avisó la vista web.
    pub fn on_msg(&mut self, msg: Msg, clipboard: Option<String>) {
        match msg {
            Msg::Loading(url) => {
                self.loading = true;
                self.url = url;
            }
            Msg::Loaded(url) => {
                self.loading = false;
                self.url = url;
            }
            Msg::Title(title) => self.title = Some(title),
            Msg::Pong => self.ping_sent = None,
            // Lo abre la terminal (`cx.open_url`).
            Msg::External(_) => {}
            Msg::Key(key) => match key.as_str() {
                "paste" => {
                    if let (Some(web), Some(text)) = (&self.web, clipboard) {
                        let json = serde_json::to_string(&text).unwrap_or_default();
                        let _ = web.evaluate_script(&format!("document.execCommand('insertText', false, {json})"));
                    }
                }
                "reload" => self.reload(),
                "back" => self.back(),
                "forward" => self.forward(),
                "zoomIn" => self.zoom(Some(1)),
                "zoomOut" => self.zoom(Some(-1)),
                "zoomReset" => self.zoom(None),
                "devtools" => self.inspect(),
                // `address` lo atiende la terminal: necesita la ventana para dar el foco.
                _ => {}
            },
            Msg::Snapshot(image) => {
                self.snapping = false;
                // Llegó la foto: recién ahora se esconde la página viva, sin hueco entre medio.
                if image.is_some() {
                    self.snapshot = image;
                }
                self.hide();
            }
        }
    }

    /// Cada cuadro: `on_screen` (la pestaña se ve), `covered` (algo de GPUI encima) y
    /// el rectángulo donde va la página.
    pub fn sync(&mut self, on_screen: bool, covered: bool, bounds: Option<Bounds<Pixels>>, bg: gpui::Hsla, window: &Window) {
        let rgba = bg.to_rgb();
        let bg = [rgba.r, rgba.g, rgba.b, 1.0];
        if self.bg != Some(bg) {
            self.bg = Some(bg);
            if let Some(web) = &self.web {
                under_page(web, bg);
            }
        }
        if !on_screen {
            self.snapshot = None;
            self.hide();
            return;
        }
        if covered {
            if self.live && !self.snapping {
                self.snapping = true;
                if !snapshot(self.web.as_deref(), self.events.clone()) {
                    self.snapping = false;
                    self.hide();
                }
            }
            return;
        }
        // Sin dirección todavía, o esperando al servidor: el panel muestra su estado.
        let (Some(bounds), false) = (bounds, self.url.is_empty() || self.waiting.is_some()) else {
            self.hide();
            return;
        };
        if self.web.is_none() {
            // Se crea fuera del cuadro (`take_new_web`): la terminal la pide y la pega.
            if !self.building {
                self.new_web = parent_of(window).map(|parent| NewWeb {
                    url: self.url.clone(),
                    bounds,
                    events: self.events.clone(),
                    root: self.root.clone(),
                    parent,
                });
            }
            return;
        }
        if let Some(web) = &self.web {
            let _ = web.set_bounds(rect(bounds));
            if !self.live {
                let _ = web.set_visible(true);
                self.live = true;
                self.hidden_since = None;
                self.snapshot = None;
            }
        }
    }

    /// La vista web que pidió `sync`, para crearla fuera del cuadro (ver `NewWeb`).
    pub fn take_new_web(&mut self) -> Option<NewWeb> {
        let new_web = self.new_web.take()?;
        self.building = true;
        Some(new_web)
    }

    /// Pega la vista web creada con `NewWeb::build`; el cuadro siguiente la muestra.
    pub fn attach(&mut self, web: Option<WebView>) {
        if !self.building {
            return;
        }
        self.building = false;
        self.web = web.map(Rc::new);
        if let Some(web) = &self.web {
            if let Some(bg) = self.bg {
                under_page(web, bg);
            }
            if self.zoom != 1.0 {
                let _ = web.zoom(self.zoom);
            }
        }
        self.last_ping = Instant::now();
    }

    fn hide(&mut self) {
        if self.live {
            if let Some(web) = &self.web {
                // El teclado vuelve a GPUI: escondida, la página se lo quedaba.
                let _ = web.focus_parent();
                let _ = web.set_visible(false);
                // Escondida en su lugar seguía recibiendo arrastres (Cydonia): lejos de todo.
                let _ = web.set_bounds(Rect { position: LogicalPosition::new(-20000., -20000.).into(), size: LogicalSize::new(10., 10.).into() });
            }
            self.live = false;
        }
        if self.hidden_since.is_none() {
            self.hidden_since = Some(Instant::now());
        }
    }

    /// Suelta la vista web si lleva oculta más de `RELEASE_AFTER`: la próxima vez se
    /// vuelve a crear en la misma dirección.
    pub fn release_if_idle(&mut self) -> bool {
        let idle = !self.live && self.hidden_since.is_some_and(|t| t.elapsed() >= RELEASE_AFTER);
        if idle && self.web.is_some() {
            self.drop_web();
            self.loading = true;
            return true;
        }
        false
    }

    /// Suelta la vista web devolviéndole antes el teclado a GPUI: si se quitaba con el
    /// foco, AppKit se lo dejaba a la ventana y no se podía escribir en ninguna parte.
    fn drop_web(&mut self) {
        if let Some(web) = self.web.take() {
            let _ = web.focus_parent();
        }
        self.live = false;
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.drop_web();
    }
}

fn rect(bounds: Bounds<Pixels>) -> Rect {
    Rect {
        position: LogicalPosition::new(f32::from(bounds.origin.x) as f64, f32::from(bounds.origin.y) as f64).into(),
        size: LogicalSize::new(f32::from(bounds.size.width).max(1.) as f64, f32::from(bounds.size.height).max(1.) as f64).into(),
    }
}

/// Una vista web por crear. En Windows, WebView2 bombea mensajes mientras nace y ahí
/// GPUI corre sus tareas: creada dentro de un cuadro (con la app tomada), la pill se
/// caía con «RefCell already borrowed». Por eso se crea desde una tarea, fuera de
/// cualquier `update`.
pub struct NewWeb {
    url: String,
    bounds: Bounds<Pixels>,
    events: UnboundedSender<Msg>,
    root: Rc<RefCell<Option<PathBuf>>>,
    parent: RawWindowHandle,
}

impl NewWeb {
    pub fn build(self) -> Option<WebView> {
        build(&self.url, self.bounds, self.events, self.root, &Parent(self.parent))
    }
}

/// La ventana de Atic Code, para colgarle la vista web sin tener el `Window`.
struct Parent(RawWindowHandle);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        // La ventana vive mientras exista su vista previa; si se cerró, wry falla.
        Ok(unsafe { WindowHandle::borrow_raw(self.0) })
    }
}

fn parent_of(window: &Window) -> Option<RawWindowHandle> {
    HasWindowHandle::window_handle(window).ok().map(|handle| handle.as_raw())
}

fn build(url: &str, bounds: Bounds<Pixels>, events: UnboundedSender<Msg>, root: Rc<RefCell<Option<PathBuf>>>, window: &Parent) -> Option<WebView> {
    let (load, title, keys, external) = (events.clone(), events.clone(), events.clone(), events);
    let builder = WebViewBuilder::new()
        .with_url(url)
        .with_bounds(rect(bounds))
        .with_devtools(true)
        // Deslizar con dos dedos va atrás y adelante; el primer clic vale aunque la
        // ventana no esté activa.
        .with_back_forward_navigation_gestures(true)
        .with_accept_first_mouse(true)
        .with_initialization_script(EDIT_KEYS)
        .with_ipc_handler(move |request| {
            let _ = keys.unbounded_send(Msg::Key(request.body().clone()));
        })
        .with_on_page_load_handler(move |event, url| {
            let _ = load.unbounded_send(match event {
                PageLoadEvent::Started => Msg::Loading(url),
                PageLoadEvent::Finished => Msg::Loaded(url),
            });
        })
        .with_document_title_changed_handler(move |text| {
            let _ = title.unbounded_send(Msg::Title(text));
        })
        // Los enlaces que piden otra ventana (`target=_blank`) van al navegador.
        .with_new_window_req_handler(move |url, _| {
            if is_web_url(&url) {
                let _ = external.unbounded_send(Msg::External(url));
            }
            NewWindowResponse::Deny
        })
        .with_custom_protocol(FILE_SCHEME.into(), move |_, request| serve_file(request.uri().path(), root.borrow().as_deref()));
    match builder.build_as_child(window) {
        Ok(web) => Some(web),
        Err(error) => {
            log::warn!("vista previa: no se pudo crear la vista web: {error}");
            None
        }
    }
}

/// Un archivo de la carpeta del HTML abierto, para `aticfile://`. Fuera de ella, nada:
/// la página no puede leer otros archivos del disco.
fn serve_file(path: &str, root: Option<&Path>) -> Response<Cow<'static, [u8]>> {
    let not_found = || Response::builder().status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&b""[..])).unwrap();
    let Some(root) = root else {
        return not_found();
    };
    let decoded = percent_decode(path);
    let Some(file) = inside(root, Path::new(&decoded)) else {
        return not_found();
    };
    match std::fs::read(&file) {
        Ok(bytes) => Response::builder()
            .header(header::CONTENT_TYPE, mime_of(&file))
            .body(Cow::Owned(bytes))
            .unwrap_or_else(|_| not_found()),
        Err(_) => not_found(),
    }
}

/// `path` resuelto y dentro de `root` (sin `..` que se escape).
fn inside(root: &Path, path: &Path) -> Option<PathBuf> {
    let file = std::fs::canonicalize(path).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    file.starts_with(&root).then_some(file)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("wasm") => "application/wasm",
        Some("txt" | "md") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Un HTML del disco (ruta absoluta o `file://`): la dirección `aticfile://` que lo sirve,
/// y su carpeta pasa a ser la que se puede leer.
fn local_url(text: &str, root: &RefCell<Option<PathBuf>>) -> Option<String> {
    let path = text.strip_prefix("file://").unwrap_or(text);
    let path = Path::new(path);
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    *root.borrow_mut() = path.parent().map(Path::to_path_buf);
    let path = path.to_string_lossy().replace('\\', "/");
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    // En Windows, wry sirve los protocolos propios como `http://<nombre>.localhost`.
    Some(if cfg!(windows) { format!("http://{FILE_SCHEME}.localhost{path}") } else { format!("{FILE_SCHEME}://localhost{path}") })
}

fn is_web_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Lo escrito en la barra, como dirección: `localhost:3000` → `http://localhost:3000`,
/// `ejemplo.com` → `https://ejemplo.com`.
pub fn normalize_url(text: &str) -> String {
    let text = text.trim();
    if text.contains("://") || text.starts_with("about:") || text.starts_with("data:") {
        return text.to_string();
    }
    let host = text.split(['/', ':']).next().unwrap_or_default();
    let local = host == "localhost" || host.parse::<std::net::Ipv4Addr>().is_ok() || host.ends_with(".local") || host.ends_with(".localhost");
    format!("{}://{text}", if local { "http" } else { "https" })
}

/// La dirección corta, para la pestaña: `localhost:5173`, `index.html`, `ejemplo.com`.
pub fn short_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    if url.starts_with(FILE_SCHEME) || url.contains(&format!("{FILE_SCHEME}.localhost")) {
        return rest.rsplit('/').next().unwrap_or(rest).to_string();
    }
    let host = rest.split('/').next().unwrap_or(rest);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

/// Lo que se puede abrir en la vista previa a partir de lo que hizo una herramienta: el
/// servidor local que imprimió un comando (`Local: http://localhost:5173/`) o el HTML
/// que se escribió.
pub fn target_of(tool: &str, input: Option<&serde_json::Value>, result: &str) -> Option<String> {
    match tool {
        "Bash" | "BashOutput" => local_server(result),
        "Write" => input
            .and_then(|input| input.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .filter(|path| path.ends_with(".html") || path.ends_with(".htm"))
            .map(str::to_string),
        _ => None,
    }
}

/// La primera dirección de un servidor local en `text` (`http://localhost:5173/`,
/// `http://127.0.0.1:8000`), sin la puntuación que la rodea.
pub fn local_server(text: &str) -> Option<String> {
    for start in ["http://localhost", "http://127.0.0.1", "http://0.0.0.0", "http://[::1]"] {
        let Some(at) = text.find(start) else {
            continue;
        };
        let url: String = text[at..]
            .chars()
            .take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '`'))
            .collect();
        let url = url.trim_end_matches(['.', ',', ';']).replace("0.0.0.0", "localhost");
        // Sin puerto, `localhost` solo es casi siempre texto suelto, no un servidor.
        let after = &url[url.find("://").map_or(0, |i| i + 3)..];
        if after.contains(':') {
            return Some(url);
        }
    }
    None
}

/// El servidor local de `url` (`http://localhost:5173/…`): host y puerto, para
/// esperar a que conteste. Solo con puerto explícito.
pub fn local_addr(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://"))?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let (host, port) = authority.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let local = matches!(host, "localhost" | "127.0.0.1" | "0.0.0.0" | "::1") || host.ends_with(".localhost");
    let port = port.parse().ok()?;
    local.then(|| (if host == "0.0.0.0" { "127.0.0.1".to_string() } else { host.to_string() }, port))
}

/// Algo contesta en `host:port` (fuera del hilo de la interfaz: puede tardar).
pub fn server_up(host: &str, port: u16) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    let Ok(addrs) = (host, port).to_socket_addrs() else {
        return false;
    };
    addrs.into_iter().any(|addr| TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_ok())
}

/// El color bajo la página (el que se ve al cargar o al rebotar el desplazamiento):
/// el del panel, no el gris de WebKit.
#[cfg(target_os = "macos")]
fn under_page(web: &WebView, [r, g, b, a]: [f32; 4]) {
    use objc2::runtime::NSObjectProtocol;
    use objc2_app_kit::NSColor;
    use wry::WebViewExtMacOS;

    let view = web.webview();
    if view.respondsToSelector(objc2::sel!(setUnderPageBackgroundColor:)) {
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r as f64, g as f64, b as f64, a as f64);
        // SAFETY: macOS 12+, comprobado arriba; el color se copia.
        unsafe { view.setUnderPageBackgroundColor(Some(&color)) };
    }
}

#[cfg(not(target_os = "macos"))]
fn under_page(_: &WebView, _: [f32; 4]) {}

/// Las herramientas de desarrollo de WebKit, en su propia ventana (`_inspector detach`,
/// API privada de WKWebView: se mira antes que exista).
#[cfg(target_os = "macos")]
fn detach_inspector(web: &WebView) {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol};
    use wry::WebViewExtMacOS;

    let view = web.webview();
    if !view.respondsToSelector(objc2::sel!(_inspector)) {
        return;
    }
    // SAFETY: `_inspector` devuelve el `_WKInspector` de la vista (o nil).
    let tool: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![&*view, _inspector] };
    let Some(tool) = tool else {
        return;
    };
    let can: bool = unsafe { objc2::msg_send![&*tool, respondsToSelector: objc2::sel!(detach)] };
    if can {
        // SAFETY: comprobado arriba que responde a `detach`.
        let () = unsafe { objc2::msg_send![&*tool, detach] };
    }
}

#[cfg(not(target_os = "macos"))]
fn detach_inspector(_: &WebView) {}

/// Pide la foto fija de la página; `false` si no se puede (sin vista o fuera de macOS).
#[cfg(target_os = "macos")]
fn snapshot(web: Option<&WebView>, events: UnboundedSender<Msg>) -> bool {
    use block2::RcBlock;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use wry::WebViewExtMacOS;

    let Some(web) = web else {
        return false;
    };
    let view = web.webview();
    let done = RcBlock::new(move |image: *mut NSImage, _: *mut NSError| {
        // SAFETY: WebKit entrega la imagen (o nada) en el hilo principal.
        let jpeg = unsafe { image.as_ref() }.and_then(|image| {
            let tiff = image.TIFFRepresentation()?;
            let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
            let data = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::JPEG, &NSDictionary::new()) }?;
            Some(data.to_vec())
        });
        let image = jpeg.map(|bytes| Arc::new(Image::from_bytes(gpui::ImageFormat::Jpeg, bytes)));
        let _ = events.unbounded_send(Msg::Snapshot(image));
    });
    // SAFETY: la vista web vive mientras la terminal la tenga; el bloque se copia.
    unsafe { view.takeSnapshotWithConfiguration_completionHandler(None, &done) };
    true
}

#[cfg(not(target_os = "macos"))]
fn snapshot(_: Option<&WebView>, _: UnboundedSender<Msg>) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encuentra_el_servidor_local_que_imprimio_un_comando() {
        assert_eq!(local_server("  ➜  Local:   http://localhost:5173/\n  ➜  Network: use --host"), Some("http://localhost:5173/".into()));
        assert_eq!(local_server("Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ..."), Some("http://localhost:8000/".into()));
        assert_eq!(local_server("listo en `http://127.0.0.1:3000`."), Some("http://127.0.0.1:3000".into()));
        // Sin puerto no es un servidor que se pueda abrir.
        assert_eq!(local_server("ver http://localhost para más"), None);
        assert_eq!(local_server("nada que abrir"), None);
    }

    #[test]
    fn un_html_escrito_se_puede_abrir() {
        let input = json!({ "file_path": "/tmp/demo/index.html", "content": "<h1>hola</h1>" });
        assert_eq!(target_of("Write", Some(&input), ""), Some("/tmp/demo/index.html".into()));
        let css = json!({ "file_path": "/tmp/demo/app.css" });
        assert_eq!(target_of("Write", Some(&css), ""), None);
        assert_eq!(target_of("Bash", None, "Local: http://localhost:4321/"), Some("http://localhost:4321/".into()));
    }

    #[test]
    fn lo_escrito_en_la_barra_pasa_a_direccion() {
        assert_eq!(normalize_url("localhost:3000"), "http://localhost:3000");
        assert_eq!(normalize_url("127.0.0.1:8080/app"), "http://127.0.0.1:8080/app");
        assert_eq!(normalize_url("ejemplo.com"), "https://ejemplo.com");
        assert_eq!(normalize_url("https://ejemplo.com/x"), "https://ejemplo.com/x");
    }

    #[test]
    fn solo_se_espera_a_un_servidor_local_con_puerto() {
        assert_eq!(local_addr("http://localhost:5173/"), Some(("localhost".into(), 5173)));
        assert_eq!(local_addr("http://0.0.0.0:8000/app?x=1"), Some(("127.0.0.1".into(), 8000)));
        assert_eq!(local_addr("http://[::1]:3000"), Some(("::1".into(), 3000)));
        assert_eq!(local_addr("https://ejemplo.com:8443/"), None);
        assert_eq!(local_addr("http://localhost/"), None);
        assert_eq!(local_addr("aticfile://localhost/tmp/index.html"), None);
    }

    #[test]
    fn la_pestana_muestra_la_direccion_corta() {
        assert_eq!(short_url("http://localhost:5173/"), "localhost:5173");
        assert_eq!(short_url("https://www.ejemplo.com/a/b"), "ejemplo.com");
        assert_eq!(short_url("aticfile://localhost/Users/x/demo/index.html"), "index.html");
    }

    #[test]
    fn el_protocolo_local_no_sale_de_la_carpeta() {
        let dir = std::env::temp_dir().join(format!("atic-preview-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/index.html"), "<p>hola</p>").unwrap();
        std::fs::write(dir.join("secreto.txt"), "no").unwrap();
        let root = dir.join("sub");
        assert!(inside(&root, &root.join("index.html")).is_some());
        assert!(inside(&root, &root.join("../secreto.txt")).is_none());
        assert_eq!(serve_file(&format!("{}/../secreto.txt", root.display()), Some(&root)).status(), StatusCode::NOT_FOUND);
        assert_eq!(serve_file(&format!("{}/index.html", root.display()), Some(&root)).status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
