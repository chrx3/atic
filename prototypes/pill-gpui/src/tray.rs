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
//! Los permisos necesitan los hooks de Atic, que este prototipo no tiene: sus
//! filas existen pero hoy solo las alimenta `PILL_TRAY_DEMO=1`.
//!
//! El vistazo no toma el foco (el notch aparece solo, mientras escribes en otra
//! app), así que sus clics se resuelven a mano, como los del vistazo de
//! Clipboard: `layout` da las mismas medidas al dibujo y al clic.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, rgb, svg, AnyElement, Div, FontWeight, Hsla, SharedString};

use crate::agents::{self, Session, Status, AGENTS};
use crate::anim::{ease_smooth_out, segment, Tween};
use crate::geometry::Rect;

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
}

/// De dónde salió una fila (para saber cómo «Ver» la trae al frente).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Session(String),
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
    /// `in_front` dice si el usuario está mirando la terminal de ese agente
    /// ahora mismo: un turno que termina delante de él no es noticia.
    pub fn observe(&mut self, sessions: &[Session], now: i64, in_front: &dyn Fn(usize) -> bool) {
        self.working = sessions.iter().filter(|s| s.status == Status::Working).count();
        let mut seen = std::collections::HashMap::new();
        for session in sessions {
            seen.insert(session.id.clone(), session.status);
            if !self.primed {
                continue;
            }
            match (self.seen.get(&session.id), session.status) {
                (Some(Status::Working), Status::Ready) => {
                    if in_front(session.agent) {
                        continue;
                    }
                    let detail = format!(
                        "{} · {}",
                        agents::folder_name(&session.cwd),
                        session.preview.as_deref().unwrap_or("Listo"),
                    );
                    self.push(Item {
                        id: 0,
                        kind: Kind::Review,
                        agent: session.agent,
                        title: format!("{} terminó un turno", AGENTS[session.agent].name),
                        detail,
                        mono: false,
                        at: session.updated.max(now - 1),
                        origin: Origin::Session(session.id.clone()),
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
            .retain(|item| item.kind == Kind::Decision || now - item.at < EXPIRE_SECS);
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
            review: self.items.iter().filter(|i| i.kind == Kind::Review).count(),
            decide: self.items.iter().filter(|i| i.kind == Kind::Decision).count(),
        }
    }

    /// Lo urgente primero: las decisiones y, dentro de cada tipo, lo que más
    /// lleva esperando (como `Alt+J` en el Espacio).
    pub fn ordered(&self) -> Vec<Item> {
        let mut items = self.items.clone();
        items.sort_by_key(|item| (item.kind != Kind::Decision, item.at, item.id));
        items
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

    /// «Aceptar»: la fila se descarta sin abrir nada.
    pub fn accept(&mut self, id: u64) {
        self.items.retain(|item| item.id != id);
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

/// Dónde cae cada contador, dado el centro de la marca. Lo usan el dibujo y el
/// clic.
pub fn chip_rects(chips: Chips, mark: (f32, f32)) -> [Option<Rect>; 2] {
    let y = mark.1 - CHIP_H / 2.0;
    let left = chips.left.map(|(glyph, n)| {
        let w = chip_w(glyph, n);
        Rect::new(mark.0 - CHIP_FROM_MARK - w, y, w, CHIP_H)
    });
    let right = chips
        .right
        .map(|(glyph, n)| Rect::new(mark.0 + CHIP_FROM_MARK, y, chip_w(glyph, n), CHIP_H));
    [left, right]
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
pub fn chip_elements(chips: Chips, mark: (f32, f32), hovered: Option<Side>, alpha: f32) -> Vec<AnyElement> {
    let rects = chip_rects(chips, mark);
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
                    .font_family("Segoe UI")
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
        .font_family("Segoe UI")
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
        // Ver · Aceptar
        Kind::Review => (44.0, 68.0),
        // Negar · Permitir
        Kind::Decision => (58.0, 76.0),
    }
}

pub fn row_height(kind: Kind) -> f32 {
    match kind {
        Kind::Review => ROW_REVIEW,
        Kind::Decision => ROW_DECIDE,
    }
}

pub fn left_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Review => "Ver",
        Kind::Decision => "Negar",
    }
}

pub fn right_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Review => "Aceptar",
        Kind::Decision => "Permitir",
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
        Kind::Review => Glyph::Review,
        Kind::Decision => Glyph::Decision,
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
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(logo(item.agent, 16., hsla(TEXT)))
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
                        .child(glyph_element(kind_glyph(item.kind)))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(px(12.))
                                .child(SharedString::from(item.title.clone())),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(10.))
                                .text_color(hsla(FAINT))
                                .child(SharedString::from(ago)),
                        ),
                )
                .child(
                    div().w_full().flex().child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(hsla(MUTED))
                            .when(item.mono, |el| el.font_family("Cascadia Mono").text_size(px(10.5)))
                            .child(SharedString::from(item.detail.clone())),
                    ),
                ),
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

/// Dónde cae cada fila (y su par de botones) bajo la franja, en el rectángulo
/// del vistazo `area`; y el pie, si lo hay.
pub fn layout(area: Rect, kinds: &[Kind], more: bool) -> (Vec<RowRect>, Option<Rect>) {
    let mut y = area.y + PAD_TOP;
    let mut rows = Vec::new();
    for &kind in kinds {
        let h = row_height(kind);
        let row = Rect::new(area.x + ROW_MX, y, area.w - ROW_MX * 2.0, h);
        let (wl, wr) = button_widths(kind);
        let by = row.y + (h - BTN_H) / 2.0;
        let right = Rect::new(row.right() - ROW_PX - wr, by, wr, BTN_H);
        let left = Rect::new(right.x - BTN_GAP - wl, by, wl, BTN_H);
        rows.push(RowRect { row, left, right });
        y += h;
    }
    let footer = more.then(|| Rect::new(area.x + 6.0, y, area.w - 12.0, FOOTER));
    (rows, footer)
}

/// Lo que mide el vistazo bajo la franja con estas filas.
pub fn banner_height(kinds: &[Kind], more: bool) -> f32 {
    if kinds.is_empty() {
        return 0.0;
    }
    PAD_TOP + kinds.iter().map(|&k| row_height(k)).sum::<f32>() + if more { FOOTER } else { 0.0 } + PAD_BOTTOM
}

pub fn hit_test(area: Rect, kinds: &[Kind], more: bool, p: (f32, f32)) -> Option<Hit> {
    let (rows, footer) = layout(area, kinds, more);
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
pub fn banner_element(area: Rect, items: &[Item], more: usize, hovered: Option<Hit>, alpha: f32, now: i64) -> AnyElement {
    let kinds: Vec<Kind> = items.iter().map(|item| item.kind).collect();
    let (rows, footer) = layout(area, &kinds, more > 0);
    let mut root = div()
        .absolute()
        .left(px(area.x))
        .top(px(area.y))
        .w(px(area.w))
        .h(px(area.h))
        .overflow_hidden()
        .font_family("Segoe UI")
        .text_color(hsla(TEXT))
        .opacity(alpha);
    for (index, (item, rect)) in items.iter().zip(&rows).enumerate() {
        let (wl, wr) = button_widths(item.kind);
        let on_row = hovered == Some(Hit::Row(index));
        let fill = if item.kind == Kind::Decision {
            Some(hsla(DECISION).opacity(0.10))
        } else if on_row {
            Some(hsla(TEXT).opacity(0.06))
        } else {
            None
        };
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
                .child(button(
                    left_label(item.kind),
                    wl,
                    left_is_primary(item.kind),
                    hovered == Some(Hit::Left(index)),
                ))
                .child(button(
                    right_label(item.kind),
                    wr,
                    !left_is_primary(item.kind),
                    hovered == Some(Hit::Right(index)),
                )),
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
                .child("Abrir bandeja")
                .child(
                    div()
                        .text_color(hsla(FAINT))
                        .child(SharedString::from(format!("{more} más"))),
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
    /// Lo que muestra, en orden: la fila y su tipo (la altura sale de ahí).
    shown: Vec<(u64, Kind)>,
    /// Cuántas filas más hay de las que caben.
    more: usize,
    announced: HashSet<u64>,
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
}

impl Banner {
    pub fn new() -> Self {
        Self {
            tween: Tween::new(0.0, Duration::from_millis(MOVE_MS), ease_smooth_out),
            shown: Vec::new(),
            more: 0,
            announced: HashSet::new(),
            until: None,
            last_open: None,
            tool_since: None,
            tool_left_at: None,
            by_hover: false,
            hovered: None,
            press: None,
            chips: Chips::default(),
            chip_hover: None,
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
        banner_height(&self.kinds(), self.more > 0)
    }

    pub fn is_running(&self, now: Instant) -> bool {
        self.tween.is_running(now)
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
                // Mientras lo miras, no se va.
                self.until = Some(now + HOVER_GRACE);
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
                let decision = fresh.iter().any(|i| i.kind == Kind::Decision);
                let spaced = self.last_open.is_none_or(|at| now.duration_since(at) >= MIN_GAP);
                if !busy && !strip_open && (!in_call || decision) && (spaced || decision) {
                    let stay = if decision { DECISION_FOR } else { REVIEW_FOR };
                    self.open(now, items, Some(stay), false);
                }
            }
        }

        // Cerrado del todo: se suelta lo que mostraba.
        if !self.is_open() && self.tween.value(now) <= 0.001 {
            self.shown.clear();
            self.more = 0;
            self.hovered = None;
        }
    }
}

/// Dónde cae `p` en el vistazo, si está abierto del todo.
pub fn banner_hit(banner: &Banner, area: Rect, now: Instant, p: (f32, f32)) -> Option<Hit> {
    if banner.amount(now) < 0.85 || !area.contains(p, 0.0) {
        return None;
    }
    hit_test(area, &banner.kinds(), banner.more > 0, p)
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

use crate::geometry::Edge;
use crate::{beyond_band, Home, NotchTool, Pill, PillShape, AGENTES_TOOL, PANEL_W, TAB_THICK};

impl Pill {
    /// El tab de arriba con su marca: ahí viven los contadores y el vistazo.
    fn tray_tab(&self, now: Instant) -> Option<(Rect, (f32, f32))> {
        match self.shape(now) {
            PillShape::Tab { edge: Edge::Top, rect, .. } => Some((rect, self.mark_center(now))),
            _ => None,
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
        let (_, mark) = self.tray_tab(now)?;
        let [left, right] = chip_rects(self.tray.chips, mark);
        if left.is_some_and(|rect| rect.contains(p, 2.0)) {
            Some(Side::Left)
        } else if right.is_some_and(|rect| rect.contains(p, 2.0)) {
            Some(Side::Right)
        } else {
            None
        }
    }

    /// Cuánto se estiró el tab con el vistazo (0 a 1) y cuánto alto gana.
    pub(crate) fn tray_stretch(&self, edge: Edge, now: Instant) -> (f32, f32) {
        if edge != Edge::Top {
            return (0.0, 0.0);
        }
        (self.tray.amount(now), self.tray.height())
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

    /// El rectángulo bajo la franja donde caen las filas del vistazo.
    fn tray_area(&self, now: Instant) -> Option<Rect> {
        if self.tray.amount(now) <= 0.01 {
            return None;
        }
        match self.shape(now) {
            PillShape::Tab { edge: Edge::Top, rect, .. } => Some(beyond_band(&rect, Edge::Top, TAB_THICK)),
            _ => None,
        }
    }

    pub(crate) fn tray_animating(&self, now: Instant) -> bool {
        self.tray.is_running(now)
    }

    /// Cada sondeo: pone al día los contadores y decide si sale o se va el vistazo.
    pub(crate) fn tray_tick(&mut self, now: Instant, cursor: Option<(f32, f32)>, cx: &mut Context<Self>) {
        let (items, counts) = {
            let inbox = self.agents.read(cx).inbox();
            (inbox.ordered(), inbox.counts())
        };
        let top = matches!(self.home, Home::Docked { edge: Edge::Top, .. }) && self.flight.is_none();
        let busy = !top
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
            Hit::Row(_) | Hit::Footer => self.open_notch(NotchTool::Agentes, window, cx),
            Hit::Left(row) | Hit::Right(row) => {
                let (Some(&id), Some(&kind)) = (ids.get(row), kinds.get(row)) else {
                    return;
                };
                let left = matches!(hit, Hit::Left(_));
                match (kind, left) {
                    (Kind::Review, true) => {
                        // «Ver»: la terminal del agente al frente. Si no se sabe
                        // cuál es, se abre la bandeja, que lo explica.
                        if !self.agents.update(cx, |panel, cx| panel.tray_view(id, cx)) {
                            self.open_notch(NotchTool::Agentes, window, cx);
                        }
                    }
                    (Kind::Review, false) => self.agents.update(cx, |panel, cx| panel.tray_accept(id, cx)),
                    (Kind::Decision, left) => self.agents.update(cx, |panel, cx| panel.tray_decide(id, !left, cx)),
                }
                self.tray.hovered = None;
                cx.notify();
            }
        }
    }

    /// Contadores, vistazo e insignia de la tira, ya en su sitio.
    pub(crate) fn tray_elements(&self, now: Instant, cx: &Context<Self>) -> Vec<AnyElement> {
        let mut out = Vec::new();
        if let Some((_, mark)) = self.tray_tab(now) {
            let alpha = self.tray_chip_alpha(now);
            if alpha > 0.01 && self.tray.chips.any() {
                out.extend(chip_elements(self.tray.chips, mark, self.tray.chip_hover, alpha));
            }
            // La insignia sobre el ícono de Agentes, con la tira abierta.
            let strip = self.strip.value(now).clamp(0.0, 1.0);
            if let (true, Some((glyph, n))) = (strip > 0.6, self.tray.chips.right) {
                let center = (self.strip_tool_along(AGENTES_TOOL, now) + 9.0, mark.1 - 8.0);
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
        }
    }

    fn decision(id: u64) -> Item {
        Item { kind: Kind::Decision, ..review(id) }
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
        inbox.observe(&[session("a", Status::Ready)], 1_001, &|agent| agent == 0);
        assert!(inbox.is_empty());
        // Con otro agente al frente, sí es noticia.
        inbox.observe(&[session("a", Status::Working)], 1_002, &|_| false);
        inbox.observe(&[session("a", Status::Ready)], 1_003, &|agent| agent == 2);
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
        let [left, right] = chip_rects(chips, mark);
        let (left, right) = (left.unwrap(), right.unwrap());
        // La marca mide 32: de 46 a 78.
        assert!(left.right() <= 46.0 + 2.0, "izquierda: {left:?}");
        assert!(right.x >= 78.0 - 2.0, "derecha: {right:?}");
        // En el tab de reposo (124) caben sin salirse.
        assert!(left.x >= 0.0 && right.right() <= 124.0, "{left:?} {right:?}");
        // Y con música (212) no pisan la carátula (9..33) ni la onda (desde ~163).
        let [left, right] = chip_rects(chips, (106.0, 20.0));
        assert!(left.unwrap().x >= 33.0 && right.unwrap().right() <= 163.0);
    }

    #[test]
    fn el_clic_y_el_dibujo_usan_las_mismas_medidas() {
        let area = Rect::new(100.0, 40.0, 440.0, 200.0);
        let kinds = [Kind::Decision, Kind::Review, Kind::Review];
        let (rows, footer) = layout(area, &kinds, true);
        assert_eq!(rows.len(), 3);
        // Las filas se apilan sin huecos y el pie va debajo de la última.
        assert_eq!(rows[0].row.bottom(), rows[1].row.y);
        assert_eq!(rows[2].row.bottom(), footer.unwrap().y);
        assert_eq!(
            banner_height(&kinds, true),
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
        assert_eq!(hit_test(area, &kinds, true, center(&rows[0].right)), Some(Hit::Right(0)));
        assert_eq!(hit_test(area, &kinds, true, center(&rows[0].left)), Some(Hit::Left(0)));
        assert_eq!(hit_test(area, &kinds, true, (rows[1].row.x + 20.0, rows[1].row.y + 4.0)), Some(Hit::Row(1)));
        assert_eq!(hit_test(area, &kinds, true, center(&footer.unwrap())), Some(Hit::Footer));
        assert_eq!(hit_test(area, &kinds, false, (area.x + 2.0, area.y + 1.0)), None);
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
        assert_eq!(banner.height(), banner_height(&[Kind::Review; 3], true));
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
}
