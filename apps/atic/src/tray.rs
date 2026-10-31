//! La bandeja del notch: lo que los agentes dejan para ti.
//!
//! Tres cosas viven aquí, todas dentro del notch y sin ventanas aparte:
//!
//! - **La bandeja** (`Inbox`): lo que espera tu atención. Un turno que
//!   terminó (*por revisar*) o un permiso (*decisión*). Sale de las sesiones
//!   que ya lee `agents.rs`: cuando una pasa de trabajando a lista, deja una
//!   fila; si vuelves a escribirle, la fila se va sola.
//! - **Los contadores del tab**: un punto ámbar a la izquierda de la marca con
//!   cuántos agentes van, y a la derecha un ✓ verde (por revisar) o un ◆ azul
//!   (decisión) con cuántos esperan. Se pegan a la marca para dejar los bordes
//!   a la música (carátula y onda).
//! - **El vistazo de Agentes** (`Banner`): cuando llega algo, el tab se ensancha
//!   al ancho de un panel y baja unas filas con lo que pasó. Es el mismo gesto
//!   que el vistazo de Clipboard (filas bajo la franja): la franja no cambia,
//!   así que la música sigue a la vista. Un turno terminado se recoge solo; una
//!   decisión se queda más. También sale al pasar el cursor por Agentes en la
//!   tira.
//!
//! Los permisos salen de los hooks de Atic en las consolas de la pill
//! (`agent_prompts.rs`); `PILL_TRAY_DEMO=1` agrega filas inventadas.
//!
//! El vistazo no toma el foco (el notch aparece solo, mientras escribes en otra
//! app), así que sus clics se resuelven a mano, como los del vistazo de
//! Clipboard: `layout` da las mismas medidas al dibujo y al clic.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, rgb, svg, AnyElement, Div, FontWeight, Hsla, SharedString};

use crate::agents::{self, Session, Status, AGENTS};
use crate::anim::{ease_smooth_out, segment, Tween};
use crate::geometry::{Edge, Rect};
use crate::i18n::{t, tf};

// --- Colores: los del panel de Agentes ------------------------------------------------

const TEXT: u32 = 0xf0f0ea;
const MUTED: u32 = 0x9a9a90;
const FAINT: u32 = 0x6e6e66;
const WORKING: u32 = 0xe8b04b;
const READY: u32 = 0x6cc48a;
const DECISION: u32 = 0x86b6ff;
const INK: u32 = 0x1a1a18;
const CLAUDE: u32 = 0xd97757;

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

// --- La bandeja -----------------------------------------------------------------------

/// Lo que espera al usuario.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Un permiso: pide una respuesta.
    Decision,
    /// Un turno o un proceso que terminó.
    Review,
    /// Hay una versión nueva de Atic para instalar (`updater.rs`).
    Update,
    /// Un aviso del celular (`phone.rs`).
    Notice,
    /// Un mensaje del celular que se puede contestar desde aquí.
    Message,
    /// Un aviso del celular con un código de verificación: copiarlo desde aquí.
    Code,
    /// El celular pide permiso para controlar el PC o escribirle a los agentes.
    /// El vistazo no se va hasta que se conteste.
    Access,
    /// El celular suena: contestar o colgar desde aquí.
    Ringing,
    /// Se habla por el celular: colgar desde aquí.
    OnCall,
}

/// De dónde salió una fila (para saber cómo «Ver» la trae al frente).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Origin {
    Session(String),
    /// Un permiso de una consola de la pill (`agent_prompts`).
    Prompt { session: String, id: String },
    /// La actualización a esta versión.
    Update(String),
    /// Los avisos de una conversación del celular `device` (la app y el título).
    PhoneNotice { device: String, group: String },
    /// La llamada del celular `device`.
    PhoneCall { device: String },
    /// Lo que pide el celular `device` (`phone::Alert::Access`).
    PhoneAccess { device: String },
    Demo,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: u64,
    pub kind: Kind,
    /// Índice en `agents::AGENTS`, para el logo.
    pub agent: usize,
    pub title: String,
    pub detail: String,
    /// El detalle es código o una ruta: va en monoespaciada.
    pub mono: bool,
    /// Cuándo pasó, en segundos desde 1970.
    pub at: i64,
    pub origin: Origin,
    /// Lo propio de un aviso o una llamada del celular.
    pub phone: Option<PhoneRow>,
}

/// Lo que una fila del celular trae además: la foto o el ícono, de qué app y
/// los avisos que junta (los de una misma conversación van en una fila).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhoneRow {
    pub device: String,
    pub app: String,
    pub icon: Option<std::sync::Arc<gpui::Image>>,
    /// Las claves de Android de los avisos juntados, del más viejo al último.
    pub keys: Vec<String>,
    /// El código de verificación del último aviso (`otp_code`), para copiarlo.
    pub code: Option<String>,
    /// La conversación se puede contestar: con un código, «Responder» queda a
    /// la izquierda (`left_label_of`).
    pub can_reply: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub working: usize,
    pub review: usize,
    pub decide: usize,
}

impl Counts {
    pub fn waiting(&self) -> usize {
        self.review + self.decide
    }
}

/// Lo que se junta de las sesiones: filas por revisar y cuántas trabajan.
#[derive(Default)]
pub struct Inbox {
    items: Vec<Item>,
    next_id: u64,
    /// El estado anterior de cada sesión: la fila nace del cambio.
    seen: std::collections::HashMap<String, Status>,
    /// La primera lectura solo toma nota: lo que ya había terminado antes de
    /// abrir el notch no es noticia.
    primed: bool,
    working: usize,
    /// `PILL_TRAY_DEMO=1`: agentes inventados trabajando.
    demo_working: usize,
    /// La versión que se dejó para «Luego»: no vuelve a avisarse hasta otra.
    skipped_update: Option<String>,
    /// Lo del celular que se ocultó aquí sin resolverlo allá («Listo» en un
    /// aviso, «Ocultar» en una llamada): no vuelve mientras siga en el celular.
    phone_hidden: std::collections::HashMap<Origin, String>,
}

/// Lo que dura una fila sin que nadie la mire (como lo listo en «En curso»).
const EXPIRE_SECS: i64 = 30 * 60;
/// Para que el notch no se llene si no se revisa nada.
const MAX_ITEMS: usize = 12;

impl Inbox {
    /// Con `PILL_TRAY_DEMO=1` arranca con tres filas y dos agentes trabajando.
    pub fn from_env() -> Self {
        let mut inbox = Self::default();
        if std::env::var_os("PILL_TRAY_DEMO").is_some() {
            inbox.demo();
        }
        inbox
    }

    fn demo(&mut self) {
        let now = agents::now_secs();
        let agent = |cli: &str| AGENTS.iter().position(|a| a.cli == cli).unwrap_or(0);
        self.demo_working = 2;
        self.push(Item {
            id: 0,
            kind: Kind::Decision,
            agent: agent("cursor-agent"),
            title: "Cursor pide editar refresh.rs".into(),
            detail: "src/api/refresh.rs · +48 −3".into(),
            mono: true,
            at: now - 20,
            origin: Origin::Demo,
            phone: None,
        });
        self.push(Item {
            id: 0,
            kind: Kind::Review,
            agent: agent("codex"),
            title: "Codex terminó un turno".into(),
            detail: "pill-gpui · Listo: las 22 pruebas pasan".into(),
            mono: false,
            at: now - 8,
            origin: Origin::Demo,
            phone: None,
        });
        self.push(Item {
            id: 0,
            kind: Kind::Review,
            agent: agent("opencode"),
            title: "OpenCode terminó el proceso".into(),
            detail: "atic · fin del proceso".into(),
            mono: false,
            at: now - 70,
            origin: Origin::Demo,
            phone: None,
        });
    }

    fn push(&mut self, mut item: Item) {
        self.next_id += 1;
        item.id = self.next_id;
        // Una sesión deja una sola fila: la última.
        if let Origin::Session(id) = &item.origin {
            self.items.retain(|old| old.origin != Origin::Session(id.clone()));
        }
        self.items.push(item);
        if self.items.len() > MAX_ITEMS {
            // Se va la fila de revisión más vieja; una decisión no se pierde.
            if let Some(index) = self.items.iter().position(|i| i.kind == Kind::Review) {
                self.items.remove(index);
            }
        }
    }

    /// Mira las sesiones de este segundo y deja filas por lo que cambió.
    /// `in_front` dice si el usuario está mirando esa sesión ahora mismo (la
    /// terminal de su agente o su conversación de Atic Code): un turno que
    /// termina delante de él no es noticia.
    pub fn observe(&mut self, sessions: &[Session], now: i64, in_front: &dyn Fn(&Session) -> bool) {
        self.working = sessions.iter().filter(|s| s.status == Status::Working).count();
        let mut seen = std::collections::HashMap::new();
        for session in sessions {
            seen.insert(session.id.clone(), session.status);
            if !self.primed {
                continue;
            }
            match (self.seen.get(&session.id), session.status) {
                (Some(Status::Working), Status::Ready) => {
                    if in_front(session) {
                        continue;
                    }
                    let detail = format!(
                        "{} · {}",
                        agents::folder_name(&session.cwd),
                        session.preview.as_deref().unwrap_or(t("pill.tray.done")),
                    );
                    self.push(Item {
                        id: 0,
                        kind: Kind::Review,
                        agent: session.agent,
                        title: tf("pill.tray.turnDone", &[("agent", &AGENTS[session.agent].name)]),
                        detail,
                        mono: false,
                        at: session.updated.max(now - 1),
                        origin: Origin::Session(session.id.clone()),
                        phone: None,
                    });
                }
                // Volvió a trabajar: ya le respondiste desde su terminal.
                (_, Status::Working) => {
                    let origin = Origin::Session(session.id.clone());
                    self.items.retain(|item| item.origin != origin);
                }
                _ => {}
            }
        }
        self.seen = seen;
        self.primed = true;
        self.items
            .retain(|item| item.kind != Kind::Review || now - item.at < EXPIRE_SECS);
    }

    /// Una fila de decisión por cada permiso que espera en una consola de la
    /// pill; las de los que ya se contestaron (aquí o en la consola) se van.
    pub fn sync_prompts(&mut self, waiting: &[crate::agent_prompts::Waiting], now: i64) {
        self.items.retain(|item| match &item.origin {
            Origin::Prompt { session, id } => waiting.iter().any(|w| &w.session == session && &w.id == id),
            _ => true,
        });
        for w in waiting {
            let origin = Origin::Prompt { session: w.session.clone(), id: w.id.clone() };
            if self.items.iter().any(|item| item.origin == origin) {
                continue;
            }
            let agent = AGENTS.iter().position(|a| a.cli == w.agent).unwrap_or(0);
            self.push(Item {
                id: 0,
                kind: Kind::Decision,
                agent,
                title: crate::i18n::tf(
                    "pill.agents.wantsTool",
                    &[("agent", &AGENTS[agent].name), ("tool", &w.tool)],
                ),
                detail: w.detail.clone(),
                mono: true,
                at: now,
                origin,
                phone: None,
            });
        }
    }

    /// Con `PILL_TRAY_DEMO=1` el mensaje rápido se ve aunque el espacio esté
    /// cerrado, como si hubiera cuatro agentes.
    pub fn demo_consoles(&self) -> usize {
        if self.demo_working > 0 {
            4
        } else {
            0
        }
    }

    pub fn counts(&self) -> Counts {
        Counts {
            working: self.working + self.demo_working,
            // La llamada en curso también cuenta: plegado, el tab dice que hay algo.
            review: self.items.iter().filter(|i| matches!(i.kind, Kind::Review | Kind::Notice | Kind::Message | Kind::Code | Kind::OnCall)).count(),
            decide: self.items.iter().filter(|i| matches!(i.kind, Kind::Decision | Kind::Ringing | Kind::Access)).count(),
        }
    }

    /// Lo urgente primero: una llamada que suena, las decisiones y, dentro de
    /// cada tipo, lo que más lleva esperando.
    pub fn ordered(&self) -> Vec<Item> {
        let mut items = self.items.clone();
        let rank = |kind: Kind| match kind {
            Kind::Ringing => 0,
            Kind::Decision | Kind::Access => 1,
            Kind::OnCall => 2,
            _ => 3,
        };
        items.sort_by_key(|item| (rank(item.kind), item.at, item.id));
        items
    }

    /// Los avisos y la llamada del celular (`phone::alerts`). Los avisos de una
    /// misma conversación (app y título) van en una fila con el último mensaje;
    /// uno nuevo la vuelve a anunciar. La llamada es una fila que se pone al
    /// día (de sonando a en curso, con su contador). Lo que el celular resolvió
    /// se va.
    pub fn sync_phone(&mut self, alerts: &[crate::phone::Alert], now: i64) {
        use crate::phone::Alert;
        let mut rows: Vec<(Origin, Kind, String, String, bool, i64, PhoneRow)> = Vec::new();
        for alert in alerts {
            match alert {
                Alert::Notice { device, key, app, title, text, at_ms, can_reply, icon } => {
                    let origin = Origin::PhoneNotice { device: device.clone(), group: format!("{app}\u{1f}{title}") };
                    match rows.iter_mut().find(|row| row.0 == origin) {
                        Some(row) => {
                            row.6.keys.push(key.clone());
                            if *at_ms / 1000 >= row.5 {
                                row.3 = one_line(text);
                                row.5 = at_ms / 1000;
                                row.6.icon = icon.clone().or(row.6.icon.take());
                            }
                            if *can_reply {
                                row.1 = Kind::Message;
                                row.6.can_reply = true;
                            }
                        }
                        None => {
                            let kind = if *can_reply { Kind::Message } else { Kind::Notice };
                            let title = one_line(if title.trim().is_empty() { app } else { title });
                            let phone = PhoneRow { device: device.clone(), app: app.clone(), icon: icon.clone(), keys: vec![key.clone()], code: None, can_reply: *can_reply };
                            rows.push((origin, kind, title, one_line(text), false, at_ms / 1000, phone));
                        }
                    }
                }
                Alert::Call { device, name, incoming, since_ms, .. } => {
                    let origin = Origin::PhoneCall { device: device.clone() };
                    let phone = PhoneRow { device: device.clone(), app: t("pill.tray.phoneApp").into(), icon: None, keys: Vec::new(), code: None, can_reply: false };
                    let row = match (incoming, since_ms) {
                        (false, Some(since)) => {
                            (origin, Kind::OnCall, tf("pill.tray.onCall", &[("name", &name)]), call_clock(now * 1000 - since), true, since / 1000, phone)
                        }
                        _ => (origin, Kind::Ringing, tf("pill.tray.ringing", &[("name", &name)]), t("pill.tray.ringingDetail").to_string(), false, now, phone),
                    };
                    rows.push(row);
                }
                Alert::Access { device, name, features } => {
                    let name = if name.trim().is_empty() { t("pill.tray.phoneApp").to_string() } else { one_line(name) };
                    let control = features.iter().any(|f| f == "control" || f == "remote");
                    let prompt = features.iter().any(|f| f == "prompt");
                    let title = match (control, prompt) {
                        (true, true) => tf("pill.tray.accessBoth", &[("name", &name)]),
                        (true, false) => tf("pill.tray.accessControl", &[("name", &name)]),
                        _ => tf("pill.tray.accessPrompt", &[("name", &name)]),
                    };
                    let phone = PhoneRow { device: device.clone(), app: t("pill.tray.phoneApp").into(), icon: None, keys: Vec::new(), code: None, can_reply: false };
                    let origin = Origin::PhoneAccess { device: device.clone() };
                    rows.push((origin, Kind::Access, title, t("pill.tray.accessDetail").to_string(), false, now, phone));
                }
            }
        }
        // Un código de verificación en el último aviso: la fila lo ofrece copiar.
        for row in &mut rows {
            if matches!(row.1, Kind::Notice | Kind::Message) {
                row.6.code = otp_code(&row.2, &row.3);
                if row.6.code.is_some() {
                    row.1 = Kind::Code;
                }
            }
        }
        // Varios avisos juntos: cuántos son, antes del último.
        for row in &mut rows {
            if row.6.keys.len() > 1 {
                row.3 = format!("{} mensajes · {}", row.6.keys.len(), row.3);
            }
        }
        let live: Vec<Origin> = rows.iter().map(|row| row.0.clone()).collect();
        self.items.retain(|item| match &item.origin {
            Origin::PhoneNotice { .. } | Origin::PhoneCall { .. } | Origin::PhoneAccess { .. } => live.contains(&item.origin),
            _ => true,
        });
        // Lo que se ocultó vuelve si llega algo nuevo (otra última clave).
        self.phone_hidden.retain(|origin, last| {
            rows.iter().any(|row| row.0 == *origin && row.6.keys.last().map_or(String::new(), Clone::clone) == *last)
        });
        for (origin, kind, title, detail, mono, at, phone) in rows {
            if self.phone_hidden.contains_key(&origin) && !matches!(kind, Kind::Ringing | Kind::Access) {
                continue;
            }
            match self.items.iter().position(|item| item.origin == origin) {
                // Un mensaje nuevo en la conversación: la fila se anuncia otra vez.
                Some(index) if self.items[index].phone.as_ref().and_then(|p| p.keys.last()) != phone.keys.last() => {
                    self.items.remove(index);
                    self.push(Item { id: 0, kind, agent: 0, title, detail, mono, at, origin, phone: Some(phone) });
                }
                Some(index) => {
                    let item = &mut self.items[index];
                    item.kind = kind;
                    item.title = title;
                    item.detail = detail;
                    item.mono = mono;
                    item.phone = Some(phone);
                }
                None => self.push(Item { id: 0, kind, agent: 0, title, detail, mono, at, origin, phone: Some(phone) }),
            }
        }
    }

    /// «Listo» en un aviso u «Ocultar» en la llamada: la fila se va de aquí
    /// aunque siga en el celular (hasta que llegue algo nuevo en ella).
    pub fn hide_phone(&mut self, id: u64) {
        if let Some(item) = self.get(id) {
            let last = item.phone.as_ref().and_then(|p| p.keys.last().cloned()).unwrap_or_default();
            self.phone_hidden.insert(item.origin.clone(), last);
        }
        self.accept(id);
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Si esa sesión ya tiene su fila en la bandeja: «En curso» no la repite.
    pub fn has_session(&self, id: &str) -> bool {
        self.items.iter().any(|item| matches!(&item.origin, Origin::Session(session) if session == id))
    }

    /// La fila de la actualización sigue a `version` (la que ofrece `updater`):
    /// aparece cuando hay una, se va cuando ya no, y no vuelve si se dejó para
    /// «Luego».
    pub fn sync_update(&mut self, version: Option<&str>, now: i64) {
        self.items.retain(|item| match &item.origin {
            Origin::Update(v) => Some(v.as_str()) == version,
            _ => true,
        });
        let Some(version) = version else {
            return;
        };
        let shown = self.items.iter().any(|item| item.origin == Origin::Update(version.to_string()));
        if shown || self.skipped_update.as_deref() == Some(version) {
            return;
        }
        self.push(Item {
            id: 0,
            kind: Kind::Update,
            agent: 0,
            title: tf("pill.tray.updateTitle", &[("version", &version)]),
            detail: t("pill.tray.updateDetail").into(),
            mono: false,
            at: now,
            origin: Origin::Update(version.to_string()),
            phone: None,
        });
    }

    /// «Luego»: la actualización se esconde hasta que salga otra versión.
    pub fn skip_update(&mut self, id: u64) {
        if let Some(Origin::Update(version)) = self.get(id).map(|item| item.origin.clone()) {
            self.skipped_update = Some(version);
        }
        self.accept(id);
    }

    /// «Aceptar»: la fila se descarta sin abrir nada.
    pub fn accept(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
    }
}

/// «03:12» o «1:02:03»: lo que va de una llamada.
fn call_clock(ms: i64) -> String {
    let secs = (ms / 1000).max(0);
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

// --- Los contadores del tab -----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    /// Punto ámbar: trabajando.
    Work,
    /// Verificación verde: por revisar.
    Review,
    /// Rombo azul: una decisión.
    Decision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Lo que dice cada lado de la marca.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Chips {
    pub left: Option<(Glyph, usize)>,
    pub right: Option<(Glyph, usize)>,
}

impl Chips {
    /// Izquierda, lo que va; derecha, lo que espera. La forma de la derecha
    /// dice lo más urgente, el número cuenta todo.
    pub fn from_counts(counts: Counts) -> Self {
        Self {
            left: (counts.working > 0).then_some((Glyph::Work, counts.working)),
            right: (counts.waiting() > 0).then_some((
                if counts.decide > 0 { Glyph::Decision } else { Glyph::Review },
                counts.waiting(),
            )),
        }
    }

    pub fn any(&self) -> bool {
        self.left.is_some() || self.right.is_some()
    }
}

const CHIP_H: f32 = 20.0;
const CHIP_PAD: f32 = 6.0;
const CHIP_GAP: f32 = 5.0;
const CHIP_DIGIT: f32 = 7.0;
/// Del centro de la marca al borde cercano del contador: la marca mide 32 y
/// sobra poco aire para que quepan la carátula y la onda.
const CHIP_FROM_MARK: f32 = 18.0;
/// Entre los dos contadores, cuando van seguidos (fundida con el notch).
const CHIPS_APART: f32 = 6.0;

fn count_text(n: usize) -> String {
    if n > 99 {
        "99+".into()
    } else {
        n.to_string()
    }
}

fn glyph_w(glyph: Glyph) -> f32 {
    match glyph {
        Glyph::Work => 6.0,
        Glyph::Review => 12.0,
        Glyph::Decision => 8.0,
    }
}

fn chip_w(glyph: Glyph, n: usize) -> f32 {
    CHIP_PAD * 2.0 + glyph_w(glyph) + CHIP_GAP + count_text(n).chars().count() as f32 * CHIP_DIGIT
}

/// Hasta dónde llega cada contador desde el centro de la marca (antes,
/// después), o 0 si no está: el largo del tab los tiene en cuenta.
pub fn chip_reach(chips: Chips) -> (f32, f32) {
    let reach = |chip: Option<(Glyph, usize)>| chip.map_or(0.0, |(glyph, n)| CHIP_FROM_MARK + chip_w(glyph, n));
    (reach(chips.left), reach(chips.right))
}

/// Fundida con el notch, los dos contadores van después de la marca, uno
/// tras otro (`chip_rects`): hasta dónde llegan desde su centro.
pub fn fused_reach(chips: Chips) -> f32 {
    let w = |chip: Option<(Glyph, usize)>| chip.map(|(glyph, n)| chip_w(glyph, n));
    match (w(chips.right), w(chips.left)) {
        (Some(a), Some(b)) => CHIP_FROM_MARK + a + CHIPS_APART + b,
        (Some(w), None) | (None, Some(w)) => CHIP_FROM_MARK + w,
        (None, None) => 0.0,
    }
}

/// Dónde cae cada contador, dado el centro de la marca y el borde del tab. Lo
/// usan el dibujo y el clic. «Izquierda» y «derecha» son antes y después de
/// la marca a lo largo del tab: en un costado, el tab va de pie y quedan
/// arriba y abajo, centrados en la columna (el texto sigue derecho). Con la
/// pill fundida con el `notch` de la cámara, el que caería en él sale al otro
/// lado (`geometry::out_of_notch`); con la marca en el ala derecha, los dos
/// van después de ella, uno tras otro.
pub fn chip_rects(chips: Chips, mark: (f32, f32), edge: Edge, notch: Option<&Rect>) -> [Option<Rect>; 2] {
    if notch.is_some_and(|notch| !edge.is_vertical() && mark.0 > notch.right()) {
        let at = |x: f32, (glyph, n): (Glyph, usize)| Rect::new(x, mark.1 - CHIP_H / 2.0, chip_w(glyph, n), CHIP_H);
        let right = chips.right.map(|chip| at(mark.0 + CHIP_FROM_MARK, chip));
        let x = right.map_or(mark.0 + CHIP_FROM_MARK, |rect| rect.right() + CHIPS_APART);
        return [chips.left.map(|chip| at(x, chip)), right];
    }
    let place = |(glyph, n): (Glyph, usize), before: bool| {
        let w = chip_w(glyph, n);
        if edge.is_vertical() {
            let y = if before { mark.1 - CHIP_FROM_MARK - CHIP_H } else { mark.1 + CHIP_FROM_MARK };
            Rect::new(mark.0 - w / 2.0, y, w, CHIP_H)
        } else {
            let x = if before { mark.0 - CHIP_FROM_MARK - w } else { mark.0 + CHIP_FROM_MARK };
            crate::geometry::out_of_notch(Rect::new(x, mark.1 - CHIP_H / 2.0, w, CHIP_H), notch, !before)
        }
    };
    [chips.left.map(|chip| place(chip, true)), chips.right.map(|chip| place(chip, false))]
}

fn glyph_element(glyph: Glyph) -> AnyElement {
    match glyph {
        Glyph::Work => div()
            .size(px(6.))
            .flex_none()
            .rounded(px(3.))
            .bg(hsla(WORKING))
            .into_any_element(),
        Glyph::Review => svg()
            .path("icons/check.svg")
            .size(px(12.))
            .flex_none()
            .text_color(hsla(READY))
            .into_any_element(),
        Glyph::Decision => div()
            .w(px(8.))
            .h(px(8.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.))
            .text_color(hsla(DECISION))
            .child("◆")
            .into_any_element(),
    }
}

/// Los contadores como elementos, ya en su sitio. `alpha` los desvanece cuando
/// la tira o un panel los tapan.
pub fn chip_elements(
    chips: Chips,
    mark: (f32, f32),
    edge: Edge,
    notch: Option<&Rect>,
    hovered: Option<Side>,
    alpha: f32,
) -> Vec<AnyElement> {
    let rects = chip_rects(chips, mark, edge, notch);
    [(Side::Left, chips.left, rects[0]), (Side::Right, chips.right, rects[1])]
        .into_iter()
        .filter_map(|(side, chip, rect)| {
            let ((glyph, n), rect) = chip.zip(rect)?;
            Some(
                div()
                    .absolute()
                    .left(px(rect.x))
                    .top(px(rect.y))
                    .w(px(rect.w))
                    .h(px(rect.h))
                    .px(px(CHIP_PAD))
                    .flex()
                    .items_center()
                    .gap(px(CHIP_GAP))
                    .rounded(px(CHIP_H / 2.0))
                    .font_family(crate::fonts::UI)
                    .text_size(px(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(hsla(TEXT))
                    .opacity(alpha)
                    .when(hovered == Some(side), |el| el.bg(hsla(TEXT).opacity(0.08)))
                    .child(glyph_element(glyph))
                    .child(SharedString::from(count_text(n)))
                    .into_any_element(),
            )
        })
        .collect()
}

/// La insignia del ícono de Agentes en la tira abierta.
pub fn badge_element(center: (f32, f32), glyph: Glyph, n: usize, alpha: f32) -> AnyElement {
    let color = if glyph == Glyph::Decision { DECISION } else { READY };
    let text = count_text(n);
    let w = (text.chars().count() as f32 * 6.0 + 8.0).max(14.0);
    div()
        .absolute()
        .left(px(center.0 - w / 2.0))
        .top(px(center.1 - 7.0))
        .min_w(px(14.))
        .h(px(14.))
        .px(px(3.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(hsla(color))
        .font_family(crate::fonts::UI)
        .text_size(px(9.))
        .font_weight(FontWeight::BOLD)
        .text_color(hsla(INK))
        .opacity(alpha)
        .child(SharedString::from(text))
        .into_any_element()
}

// --- Las filas ------------------------------------------------------------------------

const PAD_TOP: f32 = 4.0;
const PAD_BOTTOM: f32 = 6.0;
const ROW_REVIEW: f32 = 44.0;
const ROW_DECIDE: f32 = 52.0;
const FOOTER: f32 = 30.0;
const ROW_MX: f32 = 8.0;
const ROW_PX: f32 = 8.0;
const BTN_H: f32 = 26.0;
const BTN_GAP: f32 = 6.0;
/// Filas que cabe mostrar en el vistazo; el resto queda tras «Abrir bandeja».
pub const MAX_SHOWN: usize = 3;

/// Los botones tienen ancho fijo para que el dibujo y el clic coincidan.
pub fn button_widths(kind: Kind) -> (f32, f32) {
    match kind {
        // Descartar · Listo
        Kind::Notice => (78.0, 56.0),
        // Descartar · Responder
        Kind::Message => (78.0, 86.0),
        // Descartar (o Responder) · Copiar código
        Kind::Code => (86.0, 104.0),
        // Negar · Permitir
        Kind::Access => (58.0, 76.0),
        // Colgar · Contestar
        Kind::Ringing => (62.0, 84.0),
        // Ocultar · Colgar
        Kind::OnCall => (64.0, 62.0),
        // Ver · Aceptar
        Kind::Review => (44.0, 68.0),
        // Negar · Permitir
        Kind::Decision => (58.0, 76.0),
        // Luego · Instalar
        Kind::Update => (56.0, 72.0),
    }
}

pub fn row_height(kind: Kind) -> f32 {
    match kind {
        Kind::Review | Kind::Update | Kind::Notice | Kind::Message | Kind::Code => ROW_REVIEW,
        // La llamada mide lo mismo sonando que en curso: al contestar el notch no
        // cambia de alto (moverse lo deja «ocupado» y cerraba el vistazo).
        Kind::Decision | Kind::Ringing | Kind::OnCall | Kind::Access => ROW_DECIDE,
    }
}

pub fn left_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Review => t("pill.tray.view"),
        Kind::Decision | Kind::Access => t("pill.tray.deny"),
        Kind::Update => t("pill.tray.later"),
        Kind::Notice | Kind::Message | Kind::Code => t("page.meetings.discard"),
        Kind::Ringing => t("pill.tray.hangUp"),
        Kind::OnCall => t("pill.tray.hide"),
    }
}

/// El texto de un aviso en una línea: los saltos (un mensaje largo, una
/// invitación con hora y lugar) desbordaban la fila sobre la siguiente.
/// `left_label` de esa fila: un código en una conversación que se puede
/// contestar deja «Responder» en vez de «Descartar».
pub fn left_label_of(item: &Item) -> &'static str {
    if item.kind == Kind::Code && item.phone.as_ref().is_some_and(|p| p.can_reply) {
        t("pill.tray.reply")
    } else {
        left_label(item.kind)
    }
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Las palabras que delatan un código de verificación en un aviso.
const CODE_WORDS: &[&str] = &[
    "código", "codigo", "code", "otp", "verificación", "verificacion", "verification", "verify", "contraseña", "clave",
    "pin", "passcode", "password", "2fa", "token", "autenticación", "autenticacion", "authentication",
];

/// El código de verificación de un aviso («482913 es tu código de Google»,
/// «G-482913», «123 456»): de 4 a 8 cifras sueltas, solo si el aviso habla de
/// un código. Un monto (`$12.000`), una hora o una fecha no cuentan.
///
/// Se busca primero en el texto: el título suele ser quien lo manda, y un
/// número corto («24273») o un teléfono no es el código.
pub fn otp_code(title: &str, text: &str) -> Option<String> {
    let lower = format!("{title} {text}").to_lowercase();
    if !lower.split(|c: char| !c.is_alphanumeric()).any(|word| CODE_WORDS.contains(&word)) {
        return None;
    }
    code_in(text).or_else(|| code_in(title))
}

/// El primer número de 4 a 8 cifras suelto de `all` (ver `otp_code`).
fn code_in(all: &str) -> Option<String> {
    let chars: Vec<char> = all.chars().collect();
    let blocks = |c: char| c.is_alphanumeric() || "$€£%:/.,_".contains(c);
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].is_ascii_digit() || (i > 0 && blocks(chars[i - 1])) {
            i += 1;
            continue;
        }
        // Cifras seguidas, y grupos de 3 o 4 separados por un espacio o un guion.
        let mut code = String::new();
        let mut j = i;
        loop {
            let start = j;
            while j < chars.len() && chars[j].is_ascii_digit() {
                j += 1;
            }
            code.extend(&chars[start..j]);
            let run = j - start;
            let joins = |k: usize| {
                let next = chars[k..].iter().take_while(|c| c.is_ascii_digit()).count();
                (3..=4).contains(&run) && (3..=4).contains(&next)
            };
            if j + 1 < chars.len() && matches!(chars[j], ' ' | '-') && chars[j + 1].is_ascii_digit() && joins(j + 1) {
                j += 1;
                continue;
            }
            break;
        }
        // Un punto o una coma al final de la frase sí; seguidos de cifras, no.
        let after = chars.get(j).copied();
        let ends = match after {
            None => true,
            Some('.' | ',') => !chars.get(j + 1).is_some_and(|c| c.is_ascii_digit()),
            Some(c) => !blocks(c),
        };
        if ends && (4..=8).contains(&code.len()) {
            return Some(code);
        }
        i = j.max(i + 1);
    }
    None
}

/// Si la fila es del celular (va en «Celular», no en Agentes).
pub fn is_phone(kind: Kind) -> bool {
    matches!(kind, Kind::Notice | Kind::Message | Kind::Code | Kind::Ringing | Kind::OnCall | Kind::Access)
}

pub fn right_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Review => t("pill.tray.accept"),
        Kind::Decision | Kind::Access => t("permissions.allow"),
        Kind::Update => t("pill.tray.install"),
        Kind::Notice => t("pill.tray.done"),
        Kind::Message => t("pill.tray.reply"),
        Kind::Code => t("pill.tray.copyCode"),
        Kind::Ringing => t("pill.tray.answer"),
        Kind::OnCall => t("pill.tray.hangUp"),
    }
}

/// Qué botón es el principal de la fila (el de color crema).
pub fn left_is_primary(kind: Kind) -> bool {
    kind == Kind::Review
}

/// El logo del agente: Claude con su color; los demás, del color que se pida.
pub fn logo(agent: usize, size: f32, color: Hsla) -> impl IntoElement {
    let color = if AGENTS[agent].cli == "claude" { hsla(CLAUDE) } else { color };
    svg().path(AGENTS[agent].logo).size(px(size)).flex_none().text_color(color)
}

fn kind_glyph(kind: Kind) -> Glyph {
    match kind {
        Kind::Review | Kind::Update | Kind::Notice | Kind::Message | Kind::Code | Kind::OnCall => Glyph::Review,
        Kind::Decision | Kind::Ringing | Kind::Access => Glyph::Decision,
    }
}

/// Un botón de la fila, solo dibujo. `hovered` lo aclara un poco.
pub fn button(label: &'static str, w: f32, primary: bool, hovered: bool) -> Div {
    let (bg, fg) = if primary {
        (hsla(TEXT).opacity(if hovered { 1.0 } else { 0.92 }), hsla(INK))
    } else {
        (hsla(TEXT).opacity(if hovered { 0.18 } else { 0.10 }), hsla(TEXT))
    };
    div()
        .w(px(w))
        .h(px(BTN_H))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(BTN_H / 2.0))
        .text_size(px(12.))
        .bg(bg)
        .text_color(fg)
        .child(label)
}

/// Lo de la izquierda de los botones: el logo y las dos líneas.
pub fn row_body(item: &Item, now: i64) -> Div {
    let ago = agents::ago(now - item.at);
    let call = matches!(item.kind, Kind::Ringing | Kind::OnCall);
    // Lo del celular: su foto o ícono a la izquierda y, en vez del glifo de los
    // agentes, un celular chico con la app (no se confunde con un agente).
    let lead = if item.kind == Kind::Update {
        svg().path("icons/arrow-up.svg").size(px(16.)).flex_none().text_color(hsla(READY)).into_any_element()
    } else if call {
        div()
            .size(px(26.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(hsla(READY).opacity(if item.kind == Kind::Ringing { 0.28 } else { 0.16 }))
            .child(svg().path("icons/phone.svg").size(px(14.)).text_color(hsla(READY)))
            .into_any_element()
    } else if let Some(phone) = &item.phone {
        match &phone.icon {
            Some(icon) => gpui::img(icon.clone()).size(px(26.)).flex_none().rounded_full().into_any_element(),
            None => div()
                .size(px(26.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(hsla(TEXT).opacity(0.10))
                .child(svg().path("icons/smartphone.svg").size(px(14.)).text_color(hsla(TEXT)))
                .into_any_element(),
        }
    } else {
        logo(item.agent, 16., hsla(TEXT)).into_any_element()
    };
    let from_phone = item.phone.is_some();
    let mark = glyph_element(kind_glyph(item.kind));
    let detail = div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(px(11.))
        .text_color(hsla(MUTED))
        .when(item.mono, |el| el.font_family(crate::fonts::MONO).text_size(px(10.5)))
        // El contador de la llamada se lee de lejos.
        .when(item.kind == Kind::OnCall, |el| el.text_size(px(12.)).text_color(hsla(READY)))
        .child(SharedString::from(item.detail.clone()));
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(lead)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        // Agentes: el glifo antes del título. Celular: «celular · app» al final.
                        .map(|el| if from_phone { el } else { el.child(mark) })
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .when(from_phone, |el| el.font_weight(FontWeight::MEDIUM))
                                .child(SharedString::from(item.title.clone())),
                        )
                        .child(div().flex_1())
                        .when(from_phone, |el| {
                            el.child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(px(3.))
                                    .text_size(px(10.))
                                    .text_color(hsla(FAINT))
                                    .child(svg().path("icons/smartphone.svg").size(px(10.)).text_color(hsla(FAINT)))
                                    .child(SharedString::from(item.phone.as_ref().map(|p| p.app.clone()).unwrap_or_default())),
                            )
                        })
                        .child(div().flex_none().text_size(px(10.)).text_color(hsla(FAINT)).child(SharedString::from(ago))),
                )
                .child(div().w_full().flex().items_center().gap(px(6.)).child(detail)),
        )
}

// --- Medidas del vistazo --------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    /// El cuerpo de una fila: abre la bandeja.
    Row(usize),
    Left(usize),
    Right(usize),
    /// «Abrir bandeja», cuando hay más filas de las que caben.
    Footer,
}

pub struct RowRect {
    pub row: Rect,
    pub left: Rect,
    pub right: Rect,
}

/// Lo que crece una fila angosta: los botones van en su propio renglón.
const NARROW_EXTRA: f32 = BTN_H + 8.0;

/// El alto de una fila: en el bloque angosto de un costado (`narrow`) los
/// botones van bajo el texto, no a su lado.
fn row_h(kind: Kind, narrow: bool) -> f32 {
    row_height(kind) + if narrow { NARROW_EXTRA } else { 0.0 }
}

/// Dónde cae cada fila (y su par de botones) bajo la franja, en el rectángulo
/// del vistazo `area`; y el pie, si lo hay.
pub fn layout(area: Rect, kinds: &[Kind], more: bool, narrow: bool) -> (Vec<RowRect>, Option<Rect>) {
    let mut y = area.y + PAD_TOP;
    let mut rows = Vec::new();
    for &kind in kinds {
        let h = row_h(kind, narrow);
        let row = Rect::new(area.x + ROW_MX, y, area.w - ROW_MX * 2.0, h);
        let (wl, wr) = button_widths(kind);
        let by = if narrow { row.bottom() - 8.0 - BTN_H } else { row.y + (h - BTN_H) / 2.0 };
        let right = Rect::new(row.right() - ROW_PX - wr, by, wr, BTN_H);
        let left = Rect::new(right.x - BTN_GAP - wl, by, wl, BTN_H);
        rows.push(RowRect { row, left, right });
        y += h;
    }
    let footer = more.then(|| Rect::new(area.x + 6.0, y, area.w - 12.0, FOOTER));
    (rows, footer)
}

/// Lo que mide el vistazo bajo la franja con estas filas.
pub fn banner_height(kinds: &[Kind], more: bool, narrow: bool) -> f32 {
    if kinds.is_empty() {
        return 0.0;
    }
    PAD_TOP + kinds.iter().map(|&k| row_h(k, narrow)).sum::<f32>() + if more { FOOTER } else { 0.0 } + PAD_BOTTOM
}

pub fn hit_test(area: Rect, kinds: &[Kind], more: bool, narrow: bool, p: (f32, f32)) -> Option<Hit> {
    let (rows, footer) = layout(area, kinds, more, narrow);
    for (index, rect) in rows.iter().enumerate() {
        if rect.left.contains(p, 0.0) {
            return Some(Hit::Left(index));
        }
        if rect.right.contains(p, 0.0) {
            return Some(Hit::Right(index));
        }
        if rect.row.contains(p, 0.0) {
            return Some(Hit::Row(index));
        }
    }
    footer.filter(|rect| rect.contains(p, 0.0)).map(|_| Hit::Footer)
}

/// Las filas del vistazo, dentro del notch estirado. `area` es lo que queda bajo
/// la franja.
pub fn banner_element(
    area: Rect,
    items: &[Item],
    more: usize,
    hovered: Option<Hit>,
    alpha: f32,
    now: i64,
    narrow: bool,
) -> AnyElement {
    let kinds: Vec<Kind> = items.iter().map(|item| item.kind).collect();
    let (rows, footer) = layout(area, &kinds, more > 0, narrow);
    let mut root = div()
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .overflow_hidden()
        .font_family(crate::fonts::UI)
        .text_color(hsla(TEXT))
        .opacity(alpha);
    for (index, (item, rect)) in items.iter().zip(&rows).enumerate() {
        let (wl, wr) = button_widths(item.kind);
        let on_row = hovered == Some(Hit::Row(index));
        let fill = if matches!(item.kind, Kind::Decision | Kind::Access) {
            Some(hsla(DECISION).opacity(0.10))
        } else if item.kind == Kind::Ringing {
            Some(hsla(READY).opacity(0.12))
        } else if on_row {
            Some(hsla(TEXT).opacity(0.06))
        } else {
            None
        };
        let left = button(left_label_of(item), wl, left_is_primary(item.kind), hovered == Some(Hit::Left(index)));
        let right = button(right_label(item.kind), wr, !left_is_primary(item.kind), hovered == Some(Hit::Right(index)));
        if narrow {
            // Angosta: el texto arriba, a todo el ancho, y los botones abajo
            // a la derecha, justo donde los pone `layout` (el clic los busca
            // ahí).
            let at = |r: &Rect| (px(r.x - rect.row.x), px(r.y - rect.row.y));
            let (lx, ly) = at(&rect.left);
            let (rx, ry) = at(&rect.right);
            root = root.child(
                div()
                    .absolute()
                    .left(px(rect.row.x - area.x))
                    .top(px(rect.row.y - area.y))
                    .w(px(rect.row.w))
                    .h(px(rect.row.h))
                    .rounded(px(10.))
                    .when_some(fill, |el, fill| el.bg(fill))
                    .child(
                        div()
                            .h(px(row_height(item.kind)))
                            .px(px(ROW_PX))
                            .flex()
                            .items_center()
                            .child(row_body(item, now)),
                    )
                    .child(left.absolute().left(lx).top(ly))
                    .child(right.absolute().left(rx).top(ry)),
            );
            continue;
        }
        root = root.child(
            div()
                .absolute()
                .left(px(rect.row.x - area.x))
                .top(px(rect.row.y - area.y))
                .w(px(rect.row.w))
                .h(px(rect.row.h))
                .px(px(ROW_PX))
                .flex()
                .items_center()
                .gap(px(BTN_GAP * 1.6))
                .rounded(px(10.))
                .when_some(fill, |el, fill| el.bg(fill))
                .child(row_body(item, now))
                .child(left)
                .child(right),
        );
    }
    if let Some(rect) = footer {
        root = root.child(
            div()
                .absolute()
                .left(px(rect.x - area.x))
                .top(px(rect.y - area.y))
                .w(px(rect.w))
                .h(px(rect.h))
                .px(px(10.))
                .flex()
                .items_center()
                .justify_between()
                .rounded(px(10.))
                .text_size(px(11.))
                .text_color(hsla(MUTED))
                .when(hovered == Some(Hit::Footer), |el| el.bg(hsla(TEXT).opacity(0.08)))
                .child(t("pill.tray.openTray"))
                .child(
                    div()
                        .text_color(hsla(FAINT))
                        .child(SharedString::from(tf("pill.tray.more", &[("more", &more)]))),
                ),
        );
    }
    root.into_any_element()
}

// --- Cuándo sale el vistazo -----------------------------------------------------------

/// Un turno terminado se recoge solo.
const REVIEW_FOR: Duration = Duration::from_secs(5);
/// Una decisión se queda más, pero no para siempre: el contador sigue ahí.
const DECISION_FOR: Duration = Duration::from_secs(20);
/// Una llamada que suena: lo que suele sonar antes del buzón.
const RINGING_FOR: Duration = Duration::from_secs(45);
/// La llamada recién contestada: se ve un momento y queda en el contador.
const ON_CALL_FOR: Duration = Duration::from_secs(8);
/// Mientras el cursor esté encima, se queda un poco más tras salir.
const HOVER_GRACE: Duration = Duration::from_millis(1200);
/// Entre un vistazo y el siguiente (salvo una decisión).
const MIN_GAP: Duration = Duration::from_secs(6);
/// Sobre Agentes en la tira, como el vistazo de Clipboard.
const TOOL_DELAY: Duration = Duration::from_millis(350);
const TOOL_GRACE: Duration = Duration::from_millis(150);
const MOVE_MS: u64 = 260;

/// Lo que el vistazo necesita saber de su entorno en cada sondeo.
pub struct Inputs<'a> {
    /// Algo ocupa el notch (un panel, un arrastre, el vistazo de Clipboard…) o
    /// no está arriba: nada de avisos solos.
    pub busy: bool,
    /// Algo usa el micrófono o la cámara (una llamada): el contador cuenta, el
    /// vistazo no sale solo.
    pub in_call: bool,
    /// La tira de herramientas está abierta.
    pub strip_open: bool,
    /// El cursor está sobre el vistazo.
    pub over: bool,
    /// El cursor está sobre Agentes en la tira.
    pub on_tool: bool,
    pub items: &'a [Item],
}

pub struct Banner {
    tween: Tween,
    /// El alto dibujado: si entra o sale una fila con el aviso abierto, va
    /// hasta el nuevo sin saltar.
    grow: Tween,
    /// Lo que muestra, en orden: la fila y su tipo (la altura sale de ahí).
    shown: Vec<(u64, Kind)>,
    /// Cuántas filas más hay de las que caben.
    more: usize,
    announced: HashSet<u64>,
    /// Las llamadas en curso que ya se mostraron: la que se contesta (o
    /// vuelve con otra fila si el celular pasó un instante sin llamada) baja
    /// otra vez.
    calls_shown: HashSet<u64>,
    until: Option<Instant>,
    last_open: Option<Instant>,
    tool_since: Option<Instant>,
    tool_left_at: Option<Instant>,
    /// Abierto por pasar el cursor por Agentes (se cierra al irse).
    by_hover: bool,
    pub hovered: Option<Hit>,
    pub press: Option<(Hit, (f32, f32))>,
    /// Los contadores del tab, al día en cada sondeo (el dibujo y el clic los
    /// leen sin pedirle nada a la bandeja).
    pub chips: Chips,
    pub chip_hover: Option<Side>,
    /// En el bloque angosto de un costado: filas más altas, con los botones
    /// abajo (`layout`).
    pub narrow: bool,
}

impl Banner {
    pub fn new() -> Self {
        Self {
            tween: Tween::new(0.0, Duration::from_millis(MOVE_MS), ease_smooth_out),
            grow: Tween::new(0.0, Duration::from_millis(MOVE_MS), ease_smooth_out),
            shown: Vec::new(),
            more: 0,
            announced: HashSet::new(),
            calls_shown: HashSet::new(),
            until: None,
            last_open: None,
            tool_since: None,
            tool_left_at: None,
            by_hover: false,
            hovered: None,
            press: None,
            chips: Chips::default(),
            chip_hover: None,
            narrow: false,
        }
    }

    /// Cuánto se ve, de 0 a 1.
    pub fn amount(&self, now: Instant) -> f32 {
        self.tween.value(now).clamp(0.0, 1.0)
    }

    pub fn is_open(&self) -> bool {
        self.tween.target() == 1.0
    }

    pub fn kinds(&self) -> Vec<Kind> {
        self.shown.iter().map(|&(_, kind)| kind).collect()
    }

    pub fn ids(&self) -> Vec<u64> {
        self.shown.iter().map(|&(id, _)| id).collect()
    }

    pub fn more(&self) -> usize {
        self.more
    }

    /// Lo que gana el notch en alto con el vistazo abierto del todo.
    pub fn height(&self) -> f32 {
        banner_height(&self.kinds(), self.more > 0, self.narrow)
    }

    /// El alto que se ve ahora (`height` es al que va).
    pub fn shown_height(&self, now: Instant) -> f32 {
        self.grow.value(now)
    }

    pub fn is_running(&self, now: Instant) -> bool {
        self.tween.is_running(now) || self.grow.is_running(now)
    }

    pub fn close(&mut self, now: Instant) {
        self.tween.set(0.0, now);
        self.until = None;
        self.by_hover = false;
        self.press = None;
    }

    fn open(&mut self, now: Instant, items: &[Item], until: Option<Duration>, by_hover: bool) {
        self.fill(items);
        if self.shown.is_empty() {
            return;
        }
        self.tween.set(1.0, now);
        self.until = until.map(|d| now + d);
        self.by_hover = by_hover;
        self.last_open = Some(now);
    }

    /// Las filas que caben, con la decisión primero (`items` ya viene así).
    fn fill(&mut self, items: &[Item]) {
        self.shown = items.iter().take(MAX_SHOWN).map(|i| (i.id, i.kind)).collect();
        self.more = items.len().saturating_sub(MAX_SHOWN);
    }

    pub fn update(&mut self, now: Instant, input: Inputs) {
        let Inputs { busy, in_call, strip_open, over, on_tool, items } = input;
        self.announced.retain(|id| items.iter().any(|i| i.id == *id));

        // Una llamada contestada cambia de botones: el tipo sale de la fila.
        for (id, kind) in &mut self.shown {
            if let Some(item) = items.iter().find(|i| i.id == *id) {
                *kind = item.kind;
            }
        }
        // Lo que ya se resolvió sale de las filas mostradas.
        if !self.shown.is_empty() {
            let before = self.shown.len();
            self.shown.retain(|(id, _)| items.iter().any(|i| i.id == *id));
            if self.shown.len() != before {
                self.more = items.len().saturating_sub(self.shown.len());
                if self.shown.is_empty() {
                    self.close(now);
                }
            }
        }
        // Contestado el pedido de permiso (lo único sin plazo), lo que queda
        // abajo se va como cualquier aviso.
        if self.is_open() && self.until.is_none() && !self.by_hover && !self.shown.iter().any(|(_, kind)| *kind == Kind::Access) {
            self.until = Some(now + REVIEW_FOR);
        }

        // Una llamada en curso nueva (recién contestada): baja, o renueva lo
        // que se ve, aunque acabe de bajar otro aviso.
        self.calls_shown.retain(|id| items.iter().any(|i| i.id == *id && i.kind == Kind::OnCall));
        let calls: Vec<u64> =
            items.iter().filter(|i| i.kind == Kind::OnCall && !self.calls_shown.contains(&i.id)).map(|i| i.id).collect();
        // Se marca como mostrada solo si bajó: con el notch ocupado (moviéndose,
        // un panel), se intenta en el sondeo siguiente.
        if !calls.is_empty() && !busy && !strip_open && !self.by_hover {
            self.calls_shown.extend(calls.iter().copied());
            self.announced.extend(calls);
            self.open(now, items, Some(ON_CALL_FOR), false);
        }

        if self.is_open() {
            // Un panel, un arrastre o la tira (salvo sobre Agentes) lo cierran.
            if busy || (strip_open && !on_tool && !over) {
                self.close(now);
            } else if self.by_hover {
                if on_tool || over {
                    self.tool_left_at = None;
                } else {
                    let since = *self.tool_left_at.get_or_insert(now);
                    if now.duration_since(since) >= TOOL_GRACE {
                        self.close(now);
                        self.tool_left_at = None;
                    }
                }
            } else if over || on_tool {
                // Mientras lo miras, no se va. El que no tiene plazo (un
                // pedido de permiso) tampoco se va al dejar de mirarlo.
                if self.until.is_some() {
                    self.until = Some(now + HOVER_GRACE);
                }
            } else if self.until.is_some_and(|until| now >= until) {
                self.close(now);
            }
        } else {
            // Sobre Agentes en la tira: el vistazo con lo que espera.
            if on_tool && !busy && !items.is_empty() {
                let since = *self.tool_since.get_or_insert(now);
                if now.duration_since(since) >= TOOL_DELAY {
                    self.open(now, items, None, true);
                    for item in items {
                        self.announced.insert(item.id);
                    }
                }
            } else {
                self.tool_since = None;
            }
            // Algo nuevo: se avisa una vez, si el notch está quieto.
            let fresh: Vec<&Item> = items.iter().filter(|i| !self.announced.contains(&i.id)).collect();
            if !fresh.is_empty() && !self.is_open() {
                for item in &fresh {
                    self.announced.insert(item.id);
                }
                let ringing = fresh.iter().any(|i| i.kind == Kind::Ringing);
                let access = fresh.iter().any(|i| i.kind == Kind::Access);
                let decision = ringing || access || fresh.iter().any(|i| i.kind == Kind::Decision);
                let spaced = self.last_open.is_none_or(|at| now.duration_since(at) >= MIN_GAP);
                if !busy && !strip_open && (!in_call || decision) && (spaced || decision) {
                    let stay = if access {
                        None
                    } else if ringing {
                        Some(RINGING_FOR)
                    } else if decision {
                        Some(DECISION_FOR)
                    } else {
                        Some(REVIEW_FOR)
                    };
                    self.open(now, items, stay, false);
                }
            }
        }

        // Un pedido de permiso del celular no se va solo: si algo lo tapó (un
        // panel, la tira), vuelve a bajar apenas el notch queda libre.
        if !self.is_open() && !busy && !strip_open && items.iter().any(|i| i.kind == Kind::Access) {
            self.open(now, items, None, false);
        }

        // Cerrado del todo: se suelta lo que mostraba.
        if !self.is_open() && self.tween.value(now) <= 0.001 {
            self.shown.clear();
            self.more = 0;
            self.hovered = None;
        }
        // Al bajar nace con su alto; abierto, los cambios van con la curva.
        let target = self.height();
        if self.tween.value(now) <= 0.001 {
            self.grow.snap(target);
        } else {
            self.grow.set(target, now);
        }
    }
}

/// Dónde cae `p` en el vistazo, si está abierto del todo.
pub fn banner_hit(banner: &Banner, area: Rect, now: Instant, p: (f32, f32)) -> Option<Hit> {
    if banner.amount(now) < 0.85 || !area.contains(p, 0.0) {
        return None;
    }
    hit_test(area, &banner.kinds(), banner.more > 0, banner.narrow, p)
}

/// La opacidad del contenido: aparece cuando el notch ya casi llegó.
pub fn banner_alpha(banner: &Banner, now: Instant) -> f32 {
    segment(banner.amount(now), 0.55, 0.45)
}

// --- El enganche con la pill ----------------------------------------------------------
//
// Son métodos de `Pill` escritos aquí para que `main.rs` solo tenga ganchos de
// una línea: el módulo es hijo del raíz y ve sus campos privados.

use gpui::{Context, Window};

use crate::pill_tools::StripId;
use crate::{NotchTool, Pill, PillShape, AGENTES_TOOL, PANEL_W};

impl Pill {
    /// El tab acoplado (a cualquier borde) con su marca: ahí viven los
    /// contadores y el vistazo.
    fn tray_tab(&self, now: Instant) -> Option<(Edge, (f32, f32))> {
        match self.shape(now) {
            PillShape::Tab { edge, .. } => Some((edge, self.mark_center(now))),
            PillShape::Disc { .. } => None,
        }
    }

    /// Cuánto se ven los contadores: la tira abierta y el panel los tapan.
    fn tray_chip_alpha(&self, now: Instant) -> f32 {
        let strip = self.strip.value(now).clamp(0.0, 1.0);
        let morph = self.notch_morph().clamp(0.0, 1.0);
        ((1.0 - strip) * (1.0 - morph)).clamp(0.0, 1.0)
    }

    /// El contador bajo el cursor. Con la tira abierta no hay: está tapado.
    pub(crate) fn tray_chip_at(&self, p: (f32, f32), now: Instant) -> Option<Side> {
        if !self.tray.chips.any() || self.tray_chip_alpha(now) < 0.5 {
            return None;
        }
        let (edge, mark) = self.tray_tab(now)?;
        let [left, right] = chip_rects(self.tray.chips, mark, edge, self.fused().as_ref());
        if left.is_some_and(|rect| rect.contains(p, 2.0)) {
            Some(Side::Left)
        } else if right.is_some_and(|rect| rect.contains(p, 2.0)) {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// Cuánto se estiró el tab con el vistazo (0 a 1) y cuánto alto gana (a
    /// lo largo de la columna, en un costado).
    pub(crate) fn tray_stretch(&self, now: Instant) -> (f32, f32) {
        (self.tray.amount(now), self.tray.shown_height(now))
    }

    /// El largo del tab con el vistazo: al ancho de un panel (o al de la tira,
    /// si es más ancha).
    pub(crate) fn tray_length(&self, length: f32, amount: f32) -> f32 {
        length + (PANEL_W.max(length) - length) * amount
    }

    /// El cursor está sobre las filas del vistazo (bajo la franja). Como los
    /// contadores, no abren la tira: son atajos, y la tira ensancharía el
    /// notch justo bajo el botón que se va a pulsar.
    pub(crate) fn tray_over(&self, p: (f32, f32), now: Instant) -> bool {
        self.tray_area(now).is_some_and(|area| area.contains(p, 0.0))
    }

    /// El rectángulo donde caen las filas del vistazo: bajo la franja (al
    /// lado de la columna en un costado).
    fn tray_area(&self, now: Instant) -> Option<Rect> {
        if self.tray.amount(now) <= 0.01 {
            return None;
        }
        self.drawers_area(now)
    }

    pub(crate) fn tray_animating(&self, now: Instant) -> bool {
        self.tray.is_running(now)
    }

    /// Cada sondeo: pone al día los contadores y decide si sale o se va el vistazo.
    pub(crate) fn tray_tick(&mut self, now: Instant, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        // Lo del celular entra al tiro, sin esperar el sondeo de cada segundo:
        // una llamada que suena o se contesta tiene que verse ya.
        if crate::phone::take_alerts_changed() {
            self.agents.update(cx, |panel, cx| panel.sync_phone_now(cx));
        }
        let (items, counts) = {
            let inbox = self.agents.read(cx).inbox();
            (inbox.ordered(), inbox.counts())
        };
        let busy = !self.docked_still()
            || self.panel_visible()
            || self.dragging()
            || self.press.is_some()
            || self.wheel_target_open
            || self.peek.target() > 0.0;
        let on_tool = !self.panel_visible()
            && cursor.and_then(|c| self.strip_tool_at(c, now)) == Some(AGENTES_TOOL);
        // El uso de los agentes se apila bajo el aviso (`usage.rs`): estar en
        // él cuenta como estar en el aviso, que si no se cerraba, el uso subía
        // y el cursor quedaba fuera del notch.
        let over = cursor.is_some_and(|c| {
            self.tray_area(now).is_some_and(|area| area.contains(c, 4.0)) || self.usage_over(c, now)
        });
        let input = Inputs {
            busy,
            in_call: !self.privacy.uses().is_empty(),
            strip_open: self.strip.target() == 1.0,
            over,
            on_tool,
            items: &items,
        };
        self.tray.narrow = self.side_drawers();
        self.tray.update(now, input);
        self.tray.chips = Chips::from_counts(counts);
        let area = self.tray_area(now);
        self.tray.hovered = cursor.zip(area).and_then(|(c, area)| banner_hit(&self.tray, area, now, c));
        self.tray.chip_hover = cursor.and_then(|c| self.tray_chip_at(c, now));
    }

    /// El botón se apretó sobre el vistazo: se resuelve al soltarlo. Dice si lo
    /// tomó.
    pub(crate) fn tray_mouse_down(&mut self, p: (f32, f32), now: Instant) -> bool {
        let Some(area) = self.tray_area(now) else {
            return false;
        };
        let Some(hit) = banner_hit(&self.tray, area, now, p) else {
            return false;
        };
        self.tray.press = Some((hit, p));
        true
    }

    /// El botón se soltó: si sigue sobre lo mismo que se apretó, fue un clic.
    pub(crate) fn tray_release(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some((hit, _)) = self.tray.press.take() else {
            return false;
        };
        let now = Instant::now();
        let cursor = self.overlay.as_ref().and_then(|overlay| overlay.cursor(self.scale_factor));
        let still = cursor
            .zip(self.tray_area(now))
            .and_then(|(c, area)| banner_hit(&self.tray, area, now, c));
        if still == Some(hit) {
            self.tray_activate(hit, window, cx);
        }
        true
    }

    fn tray_activate(&mut self, hit: Hit, window: &mut Window, cx: &mut Context<Self>) {
        let (ids, kinds) = (self.tray.ids(), self.tray.kinds());
        match hit {
            // Una fila del celular abre «Celular»; lo demás, Agentes.
            Hit::Row(row) if kinds.get(row).is_some_and(|kind| is_phone(*kind)) => self.open_notch(NotchTool::Celular, window, cx),
            Hit::Row(_) | Hit::Footer => self.open_notch(NotchTool::Agentes, window, cx),
            Hit::Left(row) | Hit::Right(row) => {
                let (Some(&id), Some(&kind)) = (ids.get(row), kinds.get(row)) else {
                    return;
                };
                let left = matches!(hit, Hit::Left(_));
                match (kind, left) {
                    (Kind::Review, true) => {
                        // «Ver»: la terminal del agente al frente, o su conversación
                        // si vive en Atic Code. Si no se sabe cuál terminal es, se
                        // abre la bandeja, que lo explica (`AgentsEvent::Explain`,
                        // porque en macOS la búsqueda corre aparte).
                        if !self.agents.update(cx, |panel, cx| panel.tray_view(id, cx)) {
                            self.open_notch(NotchTool::Agentes, window, cx);
                        }
                    }
                    (Kind::Review, false) => self.agents.update(cx, |panel, cx| panel.tray_accept(id, cx)),
                    (Kind::Decision, left) => self.agents.update(cx, |panel, cx| panel.tray_decide(id, !left, cx)),
                    (Kind::Update, true) => self.agents.update(cx, |panel, cx| panel.tray_skip_update(id, cx)),
                    (Kind::Update, false) => self.agents.update(cx, |panel, cx| panel.tray_install(cx)),
                    // «Responder»: «Celular» con el mensaje y donde escribir. Con
                    // un código, en una conversación, va a la izquierda.
                    (Kind::Code, true) if self.agents.read(cx).can_reply(id) => {
                        self.agents.update(cx, |panel, cx| panel.start_reply(id, cx));
                        self.open_notch(NotchTool::Celular, window, cx);
                    }
                    (Kind::Message, false) => {
                        self.agents.update(cx, |panel, cx| panel.start_reply(id, cx));
                        self.open_notch(NotchTool::Celular, window, cx);
                    }
                    (Kind::Notice | Kind::Message | Kind::Code | Kind::Ringing | Kind::OnCall | Kind::Access, left) => {
                        self.agents.update(cx, |panel, cx| panel.tray_phone(id, left, cx))
                    }
                }
                self.tray.hovered = None;
                cx.notify();
            }
        }
    }

    /// Contadores, vistazo e insignia de la tira, ya en su sitio.
    pub(crate) fn tray_elements(&self, now: Instant, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some((edge, mark)) = self.tray_tab(now) {
            let alpha = self.tray_chip_alpha(now);
            if alpha > 0.01 && self.tray.chips.any() {
                out.extend(chip_elements(self.tray.chips, mark, edge, self.fused().as_ref(), self.tray.chip_hover, alpha));
            }
            // La insignia sobre el ícono de Agentes, con la tira abierta:
            // arriba a la derecha del ícono, en la franja de cualquier borde.
            // Con Agentes detrás de «Más», va sobre esa puerta; fuera de la
            // tira, no va.
            let strip = self.strip.value(now).clamp(0.0, 1.0);
            let slots = self.strip_slots();
            let slot = slots
                .iter()
                .position(|&slot| slot == StripId::Tool(AGENTES_TOOL))
                .or_else(|| {
                    self.layout
                        .more
                        .contains(&AGENTES_TOOL)
                        .then(|| slots.iter().position(|&slot| slot == StripId::More))
                        .flatten()
                });
            if let (true, Some(slot), Some((glyph, n))) = (strip > 0.6, slot, self.tray.chips.right) {
                let icon = edge.point(&self.work, self.strip_tool_along(slot, now), edge.depth_of(&self.work, mark));
                let center = (icon.0 + 9.0, icon.1 - 8.0);
                out.push(badge_element(center, glyph, n, segment(strip, 0.6, 0.4)));
            }
        }
        if let Some(area) = self.tray_area(now) {
            let inbox = self.agents.read(cx).inbox();
            let items: Vec<Item> = self.tray.ids().iter().filter_map(|&id| inbox.get(id).cloned()).collect();
            if !items.is_empty() {
                out.push(banner_element(
                    area,
                    &items,
                    self.tray.more(),
                    self.tray.hovered,
                    banner_alpha(&self.tray, now),
                    agents::now_secs(),
                    self.tray.narrow,
                ));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, status: Status) -> Session {
        Session {
            id: id.into(),
            agent: 0,
            cwd: "C:\\Users\\u\\atic".into(),
            status,
            preview: Some("Listo: subí el cambio.".into()),
            activity: None,
            updated: 1_000,
        }
    }

    fn review(id: u64) -> Item {
        Item {
            id,
            kind: Kind::Review,
            agent: 0,
            title: "terminó".into(),
            detail: "x".into(),
            mono: false,
            at: id as i64,
            origin: Origin::Demo,
            phone: None,
        }
    }

    fn decision(id: u64) -> Item {
        Item { kind: Kind::Decision, ..review(id) }
    }

    #[test]
    fn los_permisos_de_las_consolas_son_decisiones_mientras_esperan() {
        let waiting = |id: &str| crate::agent_prompts::Waiting {
            session: "s1".into(),
            id: id.into(),
            agent: "codex",
            tool: "Bash".into(),
            detail: "cargo test".into(),
        };
        let mut inbox = Inbox::default();
        inbox.sync_prompts(&[waiting("p-1")], 100);
        inbox.sync_prompts(&[waiting("p-1")], 101);
        let items = inbox.ordered();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, Kind::Decision);
        assert_eq!(items[0].agent, AGENTS.iter().position(|a| a.cli == "codex").unwrap());
        assert!(items[0].title.contains("Bash"));
        // Otro permiso de la misma sesión reemplaza al contestado.
        inbox.sync_prompts(&[waiting("p-2")], 102);
        assert_eq!(inbox.ordered().len(), 1);
        inbox.sync_prompts(&[], 103);
        assert!(inbox.is_empty());
    }

    fn quiet<'a>(items: &'a [Item]) -> Inputs<'a> {
        Inputs { busy: false, in_call: false, strip_open: false, over: false, on_tool: false, items }
    }

    #[test]
    fn un_turno_que_termina_deja_una_fila() {
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Working)], 1_000, &|_| false);
        assert!(inbox.is_empty());
        inbox.observe(&[session("a", Status::Ready)], 1_001, &|_| false);
        let items = inbox.ordered();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, Kind::Review);
        assert_eq!(items[0].title, "Claude Code terminó un turno");
        assert_eq!(items[0].detail, "atic · Listo: subí el cambio.");
        assert_eq!(inbox.counts(), Counts { working: 0, review: 1, decide: 0 });
    }

    #[test]
    fn un_turno_que_termina_delante_del_usuario_no_deja_fila() {
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Working)], 1_000, &|_| false);
        // Está mirando la terminal de ese agente cuando termina.
        inbox.observe(&[session("a", Status::Ready)], 1_001, &|s: &Session| s.agent == 0);
        assert!(inbox.is_empty());
        // Con otro agente al frente, sí es noticia.
        inbox.observe(&[session("a", Status::Working)], 1_002, &|_| false);
        inbox.observe(&[session("a", Status::Ready)], 1_003, &|s: &Session| s.agent == 2);
        assert_eq!(inbox.counts().review, 1);
    }

    #[test]
    fn lo_que_ya_estaba_listo_al_abrir_no_es_noticia() {
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Ready)], 1_000, &|_| false);
        inbox.observe(&[session("a", Status::Ready)], 1_001, &|_| false);
        assert!(inbox.is_empty());
    }

    #[test]
    fn si_le_escribes_de_nuevo_la_fila_se_va() {
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Working)], 1_000, &|_| false);
        inbox.observe(&[session("a", Status::Ready)], 1_001, &|_| false);
        assert_eq!(inbox.counts().review, 1);
        inbox.observe(&[session("a", Status::Working)], 1_002, &|_| false);
        assert_eq!(inbox.counts(), Counts { working: 1, review: 0, decide: 0 });
    }

    #[test]
    fn una_sesion_deja_una_sola_fila() {
        let mut inbox = Inbox::default();
        for t in 0..3 {
            inbox.observe(&[session("a", Status::Working)], 1_000 + t * 2, &|_| false);
            inbox.observe(&[session("a", Status::Ready)], 1_001 + t * 2, &|_| false);
        }
        assert_eq!(inbox.counts().review, 1);
    }

    #[test]
    fn aceptar_descarta_y_lo_viejo_caduca() {
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Working), session("b", Status::Working)], 1_000, &|_| false);
        inbox.observe(&[session("a", Status::Ready), session("b", Status::Ready)], 1_001, &|_| false);
        let id = inbox.ordered()[0].id;
        inbox.accept(id);
        assert_eq!(inbox.counts().review, 1);
        inbox.observe(&[session("a", Status::Ready), session("b", Status::Ready)], 1_001 + EXPIRE_SECS + 1, &|_| false);
        assert!(inbox.is_empty());
    }

    #[test]
    fn la_decision_va_primero_y_las_decisiones_no_caducan() {
        let mut inbox = Inbox::default();
        inbox.demo();
        let ordered = inbox.ordered();
        assert_eq!(ordered[0].kind, Kind::Decision);
        // Dentro de las revisiones, la que más lleva esperando va antes.
        assert!(ordered[1].at <= ordered[2].at);
        inbox.observe(&[], agents::now_secs() + EXPIRE_SECS * 2, &|_| false);
        assert_eq!(inbox.counts().decide, 1);
        assert_eq!(inbox.counts().review, 0);
    }

    #[test]
    fn los_contadores_cuentan_lo_que_va_y_lo_que_espera() {
        assert_eq!(Chips::from_counts(Counts::default()), Chips::default());
        let chips = Chips::from_counts(Counts { working: 2, review: 1, decide: 0 });
        assert_eq!(chips.left, Some((Glyph::Work, 2)));
        assert_eq!(chips.right, Some((Glyph::Review, 1)));
        // Con una decisión la forma es el rombo y el número cuenta todo.
        let chips = Chips::from_counts(Counts { working: 0, review: 2, decide: 1 });
        assert_eq!(chips.left, None);
        assert_eq!(chips.right, Some((Glyph::Decision, 3)));
    }

    #[test]
    fn los_contadores_se_pegan_a_la_marca_sin_taparla() {
        let chips = Chips::from_counts(Counts { working: 2, review: 1, decide: 0 });
        let mark = (62.0, 20.0);
        let [left, right] = chip_rects(chips, mark, Edge::Top, None);
        let (left, right) = (left.unwrap(), right.unwrap());
        // La marca mide 32: de 46 a 78.
        assert!(left.right() <= 46.0 + 2.0, "izquierda: {left:?}");
        assert!(right.x >= 78.0 - 2.0, "derecha: {right:?}");
        // En el tab de reposo (124) caben sin salirse.
        assert!(left.x >= 0.0 && right.right() <= 124.0, "{left:?} {right:?}");
        // Y con música (212) no pisan la carátula (9..33) ni la onda (desde ~163).
        let [left, right] = chip_rects(chips, (106.0, 20.0), Edge::Top, None);
        assert!(left.unwrap().x >= 33.0 && right.unwrap().right() <= 163.0);
    }

    #[test]
    fn en_un_costado_los_contadores_van_arriba_y_abajo_de_la_marca() {
        let chips = Chips::from_counts(Counts { working: 2, review: 1, decide: 0 });
        // Tab de pie a la izquierda: columna de 0 a 40, marca al medio (62).
        let [above, below] = chip_rects(chips, (20.0, 62.0), Edge::Left, None);
        let (above, below) = (above.unwrap(), below.unwrap());
        // La marca va de 46 a 78: no la pisan.
        assert!(above.bottom() <= 46.0 + 2.0 && below.y >= 78.0 - 2.0, "{above:?} {below:?}");
        // Caben en la columna y en el tab de reposo (124).
        for chip in [above, below] {
            assert!(chip.x >= 0.0 && chip.right() <= 40.0, "{chip:?}");
            assert_eq!(chip.center().0, 20.0);
        }
        assert!(above.y >= 0.0 && below.bottom() <= 124.0);
        // A la derecha, el mismo orden (el texto no se da vuelta).
        let [above_r, _] = chip_rects(chips, (1900.0, 62.0), Edge::Right, None);
        assert_eq!(above_r.unwrap().y, above.y);
    }

    #[test]
    fn el_clic_y_el_dibujo_usan_las_mismas_medidas() {
        let area = Rect::new(100.0, 40.0, 440.0, 200.0);
        let kinds = [Kind::Decision, Kind::Review, Kind::Review];
        let (rows, footer) = layout(area, &kinds, true, false);
        assert_eq!(rows.len(), 3);
        // Las filas se apilan sin huecos y el pie va debajo de la última.
        assert_eq!(rows[0].row.bottom(), rows[1].row.y);
        assert_eq!(rows[2].row.bottom(), footer.unwrap().y);
        assert_eq!(
            banner_height(&kinds, true, false),
            PAD_TOP + ROW_DECIDE + ROW_REVIEW * 2.0 + FOOTER + PAD_BOTTOM
        );
        // Cada botón cae dentro de su fila, a la derecha, sin tocarse.
        for (row, kind) in rows.iter().zip(kinds) {
            assert!(row.left.right() < row.right.x);
            assert!(row.right.right() <= row.row.right());
            assert_eq!(row.right.h, BTN_H);
            let (wl, wr) = button_widths(kind);
            assert_eq!((row.left.w, row.right.w), (wl, wr));
        }
        let center = |r: &Rect| r.center();
        assert_eq!(hit_test(area, &kinds, true, false, center(&rows[0].right)), Some(Hit::Right(0)));
        assert_eq!(hit_test(area, &kinds, true, false, center(&rows[0].left)), Some(Hit::Left(0)));
        assert_eq!(hit_test(area, &kinds, true, false, (rows[1].row.x + 20.0, rows[1].row.y + 4.0)), Some(Hit::Row(1)));
        assert_eq!(hit_test(area, &kinds, true, false, center(&footer.unwrap())), Some(Hit::Footer));
        assert_eq!(hit_test(area, &kinds, false, false, (area.x + 2.0, area.y + 1.0)), None);
    }

    #[test]
    fn en_el_bloque_angosto_los_botones_van_bajo_el_texto() {
        // El bloque de un costado (`SIDE_W`).
        let area = Rect::new(40.0, 300.0, 300.0, 300.0);
        let kinds = [Kind::Decision, Kind::Review];
        let (rows, _) = layout(area, &kinds, false, true);
        for (row, kind) in rows.iter().zip(kinds) {
            // Bajo el renglón del texto, a la derecha y dentro de la fila.
            assert!(row.left.y >= row.row.y + row_height(kind) - 2.0, "{:?}", row.left);
            assert!(row.right.right() <= row.row.right() && row.right.bottom() <= row.row.bottom());
            assert!(row.left.right() < row.right.x);
        }
        assert_eq!(rows[0].row.bottom(), rows[1].row.y);
        assert_eq!(banner_height(&kinds, false, true), PAD_TOP + ROW_DECIDE + ROW_REVIEW + 2.0 * NARROW_EXTRA + PAD_BOTTOM);
        assert_eq!(hit_test(area, &kinds, false, true, rows[1].right.center()), Some(Hit::Right(1)));
        assert_eq!(hit_test(area, &kinds, false, true, (rows[1].row.x + 20.0, rows[1].row.y + 10.0)), Some(Hit::Row(1)));
    }

    #[test]
    fn algo_nuevo_abre_el_vistazo_y_un_turno_se_recoge_solo() {
        let mut banner = Banner::new();
        let t0 = Instant::now();
        let items = [review(1)];
        banner.update(t0, quiet(&items));
        assert!(banner.is_open());
        assert_eq!(banner.ids(), vec![1]);
        // Pasados 5 s sin cursor encima se cierra.
        banner.update(t0 + REVIEW_FOR + Duration::from_millis(10), quiet(&items));
        assert!(!banner.is_open());
    }

    #[test]
    fn mientras_lo_miras_no_se_va_y_una_decision_dura_mas() {
        let mut banner = Banner::new();
        let t0 = Instant::now();
        let items = [decision(1)];
        banner.update(t0, quiet(&items));
        assert!(banner.is_open());
        // A los 6 s una decisión sigue ahí.
        banner.update(t0 + Duration::from_secs(6), quiet(&items));
        assert!(banner.is_open());
        // Con el cursor encima se extiende.
        let over = Inputs { over: true, ..quiet(&items) };
        banner.update(t0 + DECISION_FOR - Duration::from_millis(100), over);
        banner.update(t0 + DECISION_FOR + Duration::from_millis(500), quiet(&items));
        assert!(banner.is_open());
        banner.update(t0 + DECISION_FOR + HOVER_GRACE + Duration::from_secs(1), quiet(&items));
        assert!(!banner.is_open());
    }

    #[test]
    fn no_avisa_solo_si_el_notch_esta_ocupado_o_hay_una_llamada() {
        let t0 = Instant::now();
        let items = [review(1)];
        // Un panel abierto: la fila ya se ve ahí; el vistazo no sale después.
        let mut banner = Banner::new();
        banner.update(t0, Inputs { busy: true, ..quiet(&items) });
        banner.update(t0 + Duration::from_secs(30), quiet(&items));
        assert!(!banner.is_open());
        // En una llamada, un turno terminado no interrumpe; un permiso sí.
        let mut banner = Banner::new();
        banner.update(t0, Inputs { in_call: true, ..quiet(&items) });
        assert!(!banner.is_open());
        let mut banner = Banner::new();
        let items = [decision(1)];
        banner.update(t0, Inputs { in_call: true, ..quiet(&items) });
        assert!(banner.is_open());
    }

    #[test]
    fn dos_avisos_seguidos_no_se_pisan_pero_una_decision_si_pasa() {
        let t0 = Instant::now();
        let mut banner = Banner::new();
        let one = [review(1)];
        banner.update(t0, quiet(&one));
        banner.update(t0 + REVIEW_FOR + Duration::from_millis(10), quiet(&one));
        assert!(!banner.is_open());
        // Otro turno a los 5,5 s de la apertura anterior: antes del intervalo
        // mínimo (6 s), así que solo suma al contador.
        let two = [review(1), review(2)];
        banner.update(t0 + Duration::from_millis(5_500), quiet(&two));
        assert!(!banner.is_open(), "demasiado pronto tras el anterior");
        // Una decisión pasa siempre, y va primera (la bandeja viene ordenada).
        let three = [decision(3), review(1), review(2)];
        banner.update(t0 + Duration::from_secs(7), quiet(&three));
        assert!(banner.is_open(), "una decisión siempre pasa");
        assert_eq!(banner.ids()[0], 3);
    }

    #[test]
    fn el_vistazo_muestra_tres_filas_y_cuenta_las_demas() {
        let mut banner = Banner::new();
        let t0 = Instant::now();
        let items: Vec<Item> = (1..=5).map(review).collect();
        banner.update(t0, quiet(&items));
        assert_eq!(banner.ids().len(), MAX_SHOWN);
        assert_eq!(banner.more(), 2);
        assert_eq!(banner.height(), banner_height(&[Kind::Review; 3], true, false));
    }

    #[test]
    fn aceptar_la_ultima_fila_cierra_el_vistazo() {
        let mut banner = Banner::new();
        let t0 = Instant::now();
        banner.update(t0, quiet(&[review(1)]));
        assert!(banner.is_open());
        banner.update(t0 + Duration::from_millis(50), quiet(&[]));
        assert!(!banner.is_open());
    }

    #[test]
    fn sobre_agentes_en_la_tira_sale_el_vistazo_y_se_va_al_salir() {
        let mut banner = Banner::new();
        let t0 = Instant::now();
        let items = [review(1)];
        // Ya anunciado antes: ahora solo sale por el cursor.
        banner.update(t0, Inputs { busy: true, ..quiet(&items) });
        let on_tool = |strip_open| Inputs { on_tool: true, strip_open, ..quiet(&items) };
        banner.update(t0 + Duration::from_millis(100), on_tool(true));
        assert!(!banner.is_open(), "todavía no pasan 350 ms");
        banner.update(t0 + Duration::from_millis(500), on_tool(true));
        assert!(banner.is_open());
        banner.update(t0 + Duration::from_millis(700), quiet(&items));
        assert!(banner.is_open(), "150 ms de gracia");
        banner.update(t0 + Duration::from_millis(900), quiet(&items));
        assert!(!banner.is_open());
    }

    #[test]
    fn la_actualizacion_avisa_una_vez_y_respeta_luego() {
        let mut inbox = Inbox::default();
        inbox.sync_update(Some("0.4.46"), 1_000);
        let items = inbox.ordered();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].kind, Kind::Update);
        assert_eq!(items[0].title, "Atic 0.4.46 disponible");
        // La misma versión no se repite.
        inbox.sync_update(Some("0.4.46"), 1_001);
        assert_eq!(inbox.ordered().len(), 1);
        // «Luego»: no vuelve con la misma versión, sí con otra.
        let id = inbox.ordered()[0].id;
        inbox.skip_update(id);
        inbox.sync_update(Some("0.4.46"), 1_002);
        assert!(inbox.is_empty());
        inbox.sync_update(Some("0.4.47"), 1_003);
        assert_eq!(inbox.ordered()[0].title, "Atic 0.4.47 disponible");
        // Sin actualización, la fila se va. No cuenta como «por revisar».
        assert_eq!(inbox.counts().waiting(), 0);
        inbox.sync_update(None, 1_004);
        assert!(inbox.is_empty());
    }


    #[test]
    fn los_avisos_y_la_llamada_del_celular_son_filas() {
        use crate::phone::Alert;
        let notice = Alert::Notice {
            device: "cel".into(),
            key: "k1".into(),
            app: "WhatsApp".into(),
            title: "Ana".into(),
            text: "¿Vienes?".into(),
            at_ms: 1_000_000,
            can_reply: false,
            icon: None,
        };
        let ringing = Alert::Call {
            device: "cel".into(),
            name: "Mamá".into(),
            incoming: true,
            since_ms: None,
            can_answer: true,
            can_hang_up: true,
        };
        let mut inbox = Inbox::default();
        inbox.sync_phone(&[notice.clone(), ringing], 2_000);
        let items = inbox.ordered();
        // La llamada que suena va primero.
        assert_eq!(items.iter().map(|i| i.kind).collect::<Vec<_>>(), vec![Kind::Ringing, Kind::Notice]);
        assert_eq!(items[1].title, "Ana");
        assert_eq!(items[1].phone.as_ref().unwrap().app, "WhatsApp");
        let call_id = items[0].id;

        // Contestada: la misma fila pasa a «en llamada», con su contador.
        let talking = Alert::Call {
            device: "cel".into(),
            name: "Mamá".into(),
            incoming: false,
            since_ms: Some(2_000_000 - 75_000),
            can_answer: false,
            can_hang_up: true,
        };
        inbox.sync_phone(&[notice.clone(), talking.clone()], 2_000);
        let call = inbox.ordered().into_iter().find(|i| i.id == call_id).unwrap();
        assert_eq!(call.kind, Kind::OnCall);
        assert_eq!(call.detail, "01:15");

        // «Listo» oculta el aviso aquí; no vuelve mientras siga en el celular.
        let notice_id = inbox.ordered().into_iter().find(|i| i.kind == Kind::Notice).unwrap().id;
        inbox.hide_phone(notice_id);
        inbox.sync_phone(&[notice, talking], 2_001);
        assert!(inbox.ordered().iter().all(|i| i.kind != Kind::Notice));

        // Lo que el celular resolvió se va.
        inbox.sync_phone(&[], 2_002);
        assert!(inbox.is_empty());
    }

    #[test]
    fn la_llamada_contestada_vuelve_a_bajar() {
        let call = |id: u64, kind: Kind| Item { kind, ..review(id) };
        let mut banner = Banner::new();
        let t0 = Instant::now();
        // Suena: baja.
        banner.update(t0, quiet(&[call(1, Kind::Ringing)]));
        assert!(banner.is_open());
        // El celular pasa un instante sin llamada y vuelve contestada, con otra fila:
        // el vistazo no se queda cerrado aunque acabe de bajar.
        banner.update(t0 + Duration::from_secs(2), quiet(&[]));
        banner.update(t0 + Duration::from_secs(3), quiet(&[call(2, Kind::OnCall)]));
        assert!(banner.is_open());
        assert_eq!(banner.kinds(), vec![Kind::OnCall]);
        // Con el notch ocupado justo al contestar, baja en cuanto se desocupa.
        let mut banner = Banner::new();
        banner.update(t0, quiet(&[call(1, Kind::Ringing)]));
        banner.update(t0 + Duration::from_secs(1), Inputs { busy: true, ..quiet(&[call(1, Kind::OnCall)]) });
        assert!(!banner.is_open());
        banner.update(t0 + Duration::from_millis(1300), quiet(&[call(1, Kind::OnCall)]));
        assert!(banner.is_open());
        // La misma fila que pasa de sonando a en curso también renueva el vistazo.
        let mut banner = Banner::new();
        banner.update(t0, quiet(&[call(1, Kind::Ringing)]));
        banner.update(t0 + Duration::from_secs(1), quiet(&[call(1, Kind::OnCall)]));
        assert!(banner.is_open());
        assert_eq!(banner.kinds(), vec![Kind::OnCall]);
        // Se recoge sola y queda en el contador.
        banner.update(t0 + Duration::from_secs(1) + ON_CALL_FOR + Duration::from_millis(10), quiet(&[call(1, Kind::OnCall)]));
        assert!(!banner.is_open());
    }

    #[test]
    fn los_mensajes_de_una_conversacion_van_juntos_y_se_pueden_responder() {
        use crate::phone::Alert;
        let msg = |key: &str, text: &str, at_ms: i64| Alert::Notice {
            device: "cel".into(),
            key: key.into(),
            app: "WhatsApp".into(),
            title: "Ana".into(),
            text: text.into(),
            at_ms,
            can_reply: true,
            icon: None,
        };
        let mut inbox = Inbox::default();
        inbox.sync_phone(&[msg("k1", "Hola", 1_000_000)], 2_000);
        let first = inbox.ordered()[0].clone();
        assert_eq!(first.kind, Kind::Message);
        assert_eq!(first.detail, "Hola");
        // Otro mensaje de Ana: una sola fila, con el último y cuántos son, y se anuncia de nuevo.
        inbox.sync_phone(&[msg("k1", "Hola", 1_000_000), msg("k2", "¿Vienes?", 1_005_000)], 2_005);
        let items = inbox.ordered();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "2 mensajes · ¿Vienes?");
        assert_eq!(items[0].phone.as_ref().unwrap().keys, vec!["k1".to_string(), "k2".to_string()]);
        assert_ne!(items[0].id, first.id);
        // Respondida (u ocultada): no vuelve hasta que llegue otro mensaje.
        inbox.hide_phone(items[0].id);
        inbox.sync_phone(&[msg("k1", "Hola", 1_000_000), msg("k2", "¿Vienes?", 1_005_000)], 2_006);
        assert!(inbox.is_empty());
        inbox.sync_phone(&[msg("k1", "Hola", 1_000_000), msg("k2", "¿Vienes?", 1_005_000), msg("k3", "Avisa", 1_007_000)], 2_007);
        assert_eq!(inbox.ordered().len(), 1);
        assert_eq!(right_label(Kind::Message), "Responder");
    }

    #[test]
    fn el_codigo_de_verificacion_se_encuentra() {
        assert_eq!(otp_code("Google", "G-482913 es tu código de verificación."), Some("482913".into()));
        assert_eq!(otp_code("Banco", "Tu clave dinámica es 123 456. No la compartas"), Some("123456".into()));
        assert_eq!(otp_code("", "Your verification code is 5521"), Some("5521".into()));
        assert_eq!(otp_code("WhatsApp", "Código de WhatsApp: 382-104"), Some("382104".into()));
        assert_eq!(otp_code("Compra", "Pago de $12.000 aprobado, código 9931"), Some("9931".into()));
        // Sin hablar de un código, o sin uno que parezca código.
        assert_eq!(otp_code("Ana", "Nos vemos a las 18:30 en el 4512"), None);
        assert_eq!(otp_code("Banco", "Tu código vence el 12/05/2026"), None);
        assert_eq!(otp_code("Tienda", "Usa el código PROMO2024"), None);
        assert_eq!(otp_code("Banco", "Tu clave es 12"), None);
        // Quien lo manda (un número corto, un teléfono) no es el código.
        assert_eq!(otp_code("24273", "Tu código es 918273"), Some("918273".into()));
        assert_eq!(otp_code("+56 9 8765 4321", "Tu código de acceso: 5521"), Some("5521".into()));
    }

    #[test]
    fn un_aviso_con_codigo_se_ofrece_copiar() {
        use crate::phone::Alert;
        let sms = Alert::Notice {
            device: "cel".into(),
            key: "k1".into(),
            app: "Mensajes".into(),
            title: "Google".into(),
            text: "G-482913 es tu código de verificación".into(),
            at_ms: 1_000_000,
            can_reply: true,
            icon: None,
        };
        let mut inbox = Inbox::default();
        inbox.sync_phone(&[sms], 2_000);
        let item = inbox.ordered()[0].clone();
        assert_eq!(item.kind, Kind::Code);
        assert_eq!(item.phone.as_ref().unwrap().code.as_deref(), Some("482913"));
        assert_eq!(right_label(Kind::Code), "Copiar código");
        // Se puede contestar: «Responder» sigue a mano, a la izquierda.
        assert_eq!(left_label_of(&item), "Responder");
        let push = Alert::Notice {
            device: "cel".into(),
            key: "k2".into(),
            app: "Banco".into(),
            title: "Banco".into(),
            text: "Tu código es 5521".into(),
            at_ms: 1_000_000,
            can_reply: false,
            icon: None,
        };
        let mut inbox = Inbox::default();
        inbox.sync_phone(&[push], 2_000);
        assert_eq!(left_label_of(&inbox.ordered()[0]), "Descartar");
    }

    #[test]
    fn el_pedido_de_permiso_no_se_va_hasta_contestarlo() {
        use crate::phone::Alert;
        let ask = Alert::Access { device: "cel".into(), name: "S24".into(), features: vec!["control".into(), "remote".into()] };
        let mut inbox = Inbox::default();
        inbox.sync_phone(std::slice::from_ref(&ask), 2_000);
        let items = inbox.ordered();
        assert_eq!(items[0].kind, Kind::Access);
        assert_eq!(items[0].title, "S24 pide controlar este PC");
        // «Listo» u ocultar no existen: sigue mientras el celular lo pida.
        inbox.hide_phone(items[0].id);
        inbox.sync_phone(std::slice::from_ref(&ask), 2_001);
        assert_eq!(inbox.ordered().len(), 1);

        let t0 = Instant::now();
        let mut banner = Banner::new();
        let items = inbox.ordered();
        banner.update(t0, quiet(&items));
        assert!(banner.is_open());
        // Pasa el plazo de cualquier aviso y sigue ahí.
        banner.update(t0 + Duration::from_secs(120), quiet(&items));
        assert!(banner.is_open());
        // Un panel lo tapa; al cerrarse, vuelve.
        banner.update(t0 + Duration::from_secs(121), Inputs { busy: true, ..quiet(&items) });
        assert!(!banner.is_open());
        banner.update(t0 + Duration::from_secs(122), quiet(&items));
        assert!(banner.is_open());
        // Contestado (el celular ya no lo pide): se va.
        inbox.sync_phone(&[], 2_100);
        banner.update(t0 + Duration::from_secs(123), quiet(&inbox.ordered()));
        assert!(!banner.is_open());
    }

    #[test]
    fn contestado_el_permiso_lo_demas_se_va_a_su_tiempo() {
        use crate::phone::Alert;
        let ask = Alert::Access { device: "cel".into(), name: "S24".into(), features: vec!["prompt".into()] };
        let mut inbox = Inbox::default();
        inbox.observe(&[session("a", Status::Working)], 1_000, &|_| false);
        inbox.observe(&[session("a", Status::Ready)], 1_010, &|_| false);
        inbox.sync_phone(std::slice::from_ref(&ask), 1_020);
        let t0 = Instant::now();
        let mut banner = Banner::new();
        banner.update(t0, quiet(&inbox.ordered()));
        assert!(banner.is_open());
        assert_eq!(banner.shown.len(), 2);
        // Se contesta: queda la otra fila, que ya no se queda para siempre.
        inbox.sync_phone(&[], 1_030);
        let items = inbox.ordered();
        banner.update(t0 + Duration::from_secs(60), quiet(&items));
        assert!(banner.is_open());
        banner.update(t0 + Duration::from_secs(60) + REVIEW_FOR, quiet(&items));
        assert!(!banner.is_open());
    }

    #[test]
    fn los_avisos_van_en_una_linea() {
        assert_eq!(one_line("EL TIGRE\nSIN LIMITES  \n\n https://x"), "EL TIGRE SIN LIMITES https://x");
    }
}
