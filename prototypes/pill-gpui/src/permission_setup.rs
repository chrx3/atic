//! La pantalla de permisos de macOS: aparece al abrir la pill si falta un
//! permiso que no faltaba ya cuando se cerró (la primera vez, todos; después,
//! uno que se quitó en Ajustes o que llegó con una versión nueva). Muestra
//! cada permiso, cuánto hace falta, para qué es y si está; mientras se pide
//! uno, qué va a hacer macOS y qué tocar. «Pedir todos» los pide de a uno: el
//! siguiente se pide cuando el anterior queda concedido (o con «Saltar»), así
//! no se apilan los diálogos de macOS.
//!
//! Grabación de pantalla solo vale después de reabrir la app: si se concede
//! aquí, la pantalla ofrece reiniciar Atic.

use std::collections::VecDeque;
use std::time::Duration;

use gpui::{
    div, prelude::*, px, size, App, Bounds, ClickEvent, Context, FontWeight, SharedString, Window, WindowBounds,
    WindowKind, WindowOptions,
};

use crate::platform::permissions::{self, Level, Permission, State};
use crate::settings::{card, hsla, text_button, AMBER, FAINT, FILL, INK, MUTED, TEXT};
use crate::space::chrome;

const WINDOW_W: f32 = 600.0;
const WINDOW_H: f32 = 720.0;
const TOP_H: f32 = 44.0;
const GUTTER: f32 = 24.0;
const GREEN: u32 = crate::theme::GREEN;

/// La marca de «Listo» o «No preguntar por estos»: una por titular, como los
/// permisos (los de la `.app` y los de la terminal son distintos para
/// macOS). Guarda los que faltaban al cerrarla, uno por línea.
fn marker() -> Option<std::path::PathBuf> {
    let holder = if crate::platform::bundled() { "app" } else { "terminal" };
    crate::paths::file(&format!(".permisos-listos-{holder}"))
}

fn missing() -> Vec<Permission> {
    Permission::ALL.into_iter().filter(|p| permissions::state(*p) != State::Granted).collect()
}

/// Los que faltaban cuando se cerró la pantalla; `None` si nunca se cerró.
fn dismissed() -> Option<Vec<String>> {
    let text = std::fs::read_to_string(marker()?).ok()?;
    Some(text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect())
}

/// Si hay que mostrarla: falta alguno que no faltaba ya al cerrarla. Una
/// marca vacía (la de «Listo» con todo, o las de antes) no tapa ninguno.
fn should_open(missing: &[Permission], dismissed: Option<&[String]>) -> bool {
    missing.iter().any(|p| !dismissed.is_some_and(|d| d.iter().any(|name| *name == format!("{p:?}"))))
}

/// Al arrancar: si falta un permiso que no faltaba ya al cerrarla.
/// `PILL_OPEN=permissions` la abre igual, para revisarla; en desarrollo no se
/// abre sola (la pill de dev es otro titular para macOS).
pub fn open_if_needed(cx: &mut App) {
    let missing = missing();
    tracing::info!(faltan = ?missing, "permisos de macOS");
    let forced = std::env::var("PILL_OPEN").as_deref() == Ok("permissions");
    if !forced && (crate::dev::on() || !should_open(&missing, dismissed().as_deref())) {
        return;
    }
    open(cx);
}

pub fn open(cx: &mut App) {
    if let Some(handle) = cx.windows().into_iter().find_map(|w| w.downcast::<PermissionSetup>()) {
        if handle.update(cx, |_, window, _| crate::win::bring_to_front(window)).is_ok() {
            return;
        }
    }
    let options = WindowOptions {
        titlebar: Some(chrome::titlebar("Atic · Permisos", TOP_H)),
        window_min_size: Some(size(px(460.), px(560.))),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(WINDOW_W), px(WINDOW_H)), cx))),
        focus: crate::win::FOCUS_ON_OPEN,
        show: true,
        kind: WindowKind::Normal,
        ..Default::default()
    };
    let opened = cx.open_window(options, |window, cx| {
        chrome::setup(window);
        cx.new(|cx| PermissionSetup::new(cx))
    });
    match opened {
        Ok(handle) => {
            let _ = handle.update(cx, |_, window, _| crate::win::bring_to_front(window));
        }
        Err(error) => tracing::warn!(%error, "permisos: no se pudo abrir la ventana"),
    }
}

pub struct PermissionSetup {
    /// Los que quedan por pedir con «Pedir todos».
    queue: VecDeque<Permission>,
    /// El que se está pidiendo ahora.
    asking: Option<Permission>,
    /// Faltaba Grabación de pantalla al abrir: si se concede, hay que reiniciar.
    screen_missing: bool,
    /// El material del sistema puesto detrás (`theme::sync_backdrop`).
    backdrop: Option<bool>,
}

impl PermissionSetup {
    fn new(cx: &mut Context<Self>) -> Self {
        // Se conceden en Ajustes del Sistema, fuera de la pill: se relee cada segundo.
        cx.spawn(async move |view, cx| loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            if view.update(cx, |view, cx| view.tick(cx)).is_err() {
                break;
            }
        })
        .detach();
        Self {
            queue: VecDeque::new(),
            asking: None,
            screen_missing: permissions::state(Permission::ScreenRecording) != State::Granted,
            backdrop: None,
        }
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        if self.asking.is_some_and(|p| permissions::state(p) == State::Granted) {
            self.next();
        }
        cx.notify();
    }

    fn ask_all(&mut self, cx: &mut Context<Self>) {
        self.queue = missing().into();
        self.next();
        cx.notify();
    }

    /// Pide el siguiente de la fila que todavía falte.
    fn next(&mut self) {
        self.asking = None;
        while let Some(permission) = self.queue.pop_front() {
            if permissions::state(permission) != State::Granted {
                self.asking = Some(permission);
                permissions::request(permission);
                return;
            }
        }
    }

    fn ask_one(&mut self, permission: Permission, cx: &mut Context<Self>) {
        self.queue.clear();
        self.asking = Some(permission);
        permissions::request(permission);
        cx.notify();
    }

    /// Cierra y anota lo que falta: eso no la vuelve a abrir, otro que falte sí.
    fn done(&mut self, window: &mut Window) {
        if let Some(marker) = marker() {
            if let Some(dir) = marker.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let names: Vec<String> = missing().iter().map(|p| format!("{p:?}")).collect();
            let _ = std::fs::write(marker, names.join("\n"));
        }
        window.remove_window();
    }

    fn row(&self, permission: Permission, cx: &mut Context<Self>) -> impl IntoElement {
        let state = permissions::state(permission);
        let asking = self.asking == Some(permission);
        let (label, color): (&str, u32) = match state {
            State::Granted => ("Concedido", GREEN),
            _ if asking => ("Esperando…", AMBER),
            State::Denied => ("Falta", AMBER),
            State::NotAsked => ("Sin preguntar", MUTED),
        };
        let id = |suffix: &str| SharedString::from(format!("perm-setup-{permission:?}-{suffix}"));
        let level = permission.level();
        let level_color = match level {
            Level::Required => TEXT,
            Level::Recommended => MUTED,
            Level::Optional => FAINT,
        };
        // Mientras se pide: qué va a hacer macOS y qué tocar.
        let how = (asking && state != State::Granted).then(|| permission.how(state, &permissions::holder()));
        let control = match state {
            State::Granted => div().into_any_element(),
            _ if asking => div()
                .flex()
                .gap(px(6.))
                .child(text_button(
                    leak(id("again")),
                    if state == State::NotAsked { "Pedir de nuevo" } else { "Abrir Ajustes" },
                    move |_, _, _| permissions::request(permission),
                ))
                .when(!self.queue.is_empty(), |el| {
                    el.child(text_button(
                        leak(id("skip")),
                        "Saltar",
                        cx.listener(|view, _: &ClickEvent, _, cx| {
                            view.next();
                            cx.notify();
                        }),
                    ))
                })
                .into_any_element(),
            _ => text_button(
                leak(id("ask")),
                if state == State::NotAsked { "Permitir" } else { "Abrir Ajustes" },
                cx.listener(move |view, _: &ClickEvent, _, cx| view.ask_one(permission, cx)),
            ),
        };
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .child(
                div()
                    .flex_none()
                    .size(px(10.))
                    .rounded_full()
                    .bg(hsla(color)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(13.)).text_color(hsla(TEXT)).child(permission.title()))
                            .child(
                                div()
                                    .px(px(6.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(hsla(level_color).opacity(0.35))
                                    .text_size(px(10.))
                                    .text_color(hsla(level_color))
                                    .child(level.label()),
                            )
                            .child(div().text_size(px(11.)).text_color(hsla(color)).child(label)),
                    )
                    .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(permission.why()))
                    .when_some(how, |el, how| el.child(div().pt(px(2.)).text_size(px(11.)).text_color(hsla(AMBER)).child(how))),
            )
            .child(div().flex_none().child(control))
    }
}

/// Los botones piden un id `'static`; hay pocos y fijos (uno por permiso).
fn leak(id: SharedString) -> &'static str {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static IDS: Mutex<Option<HashMap<SharedString, &'static str>>> = Mutex::new(None);
    let mut ids = IDS.lock().unwrap_or_else(|e| e.into_inner());
    ids.get_or_insert_with(HashMap::new)
        .entry(id.clone())
        .or_insert_with(|| Box::leak(id.to_string().into_boxed_str()))
}

/// El botón principal: relleno en M3 y en el vidrio, claro en «Atic».
fn primary(id: &'static str, label: &'static str, on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> gpui::AnyElement {
    if crate::theme::styled().is_some() {
        return gpui_m3::Button::new(id, label)
            .variant(gpui_m3::ButtonVariant::Filled)
            .on_click(on_click)
            .into_any_element();
    }
    div()
        .id(id)
        .flex_none()
        .px(px(16.))
        .py(px(8.))
        .rounded_full()
        .bg(hsla(FILL))
        .text_color(hsla(INK))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .on_click(on_click)
        .child(label)
        .into_any_element()
}

impl Render for PermissionSetup {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        crate::theme::sync_backdrop(window, &mut self.backdrop);
        let m3_scope = crate::theme::scope_m3(cx);
        let missing = missing();
        let all_granted = missing.is_empty();
        let restart = self.screen_missing && permissions::state(Permission::ScreenRecording) == State::Granted;
        let holder: SharedString =
            format!("Quedan a nombre de «{}» en Ajustes del Sistema → Privacidad y seguridad.", permissions::holder()).into();

        let rows = Permission::ALL.into_iter().fold(card(), |card, permission| card.child(self.row(permission, cx)));
        // Como el pie de Openscreen: lo que falta de lo requerido, o que ya está.
        let required: Vec<&str> =
            missing.iter().filter(|p| p.level() == Level::Required).map(|p| p.title()).collect();
        let status: Option<(SharedString, u32)> = if all_granted {
            None
        } else if required.is_empty() {
            Some(("Lo requerido está listo. Lo demás lo puedes dar ahora o cuando uses esa herramienta.".into(), MUTED))
        } else {
            Some((format!("Falta lo requerido: {}. Sin eso Atic no puede pegar en otras apps.", required.join(", ")).into(), AMBER))
        };

        let actions = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(div().flex_1())
            .when(!all_granted, |el| {
                el.child(text_button(
                    "perm-setup-later",
                    "Más tarde",
                    cx.listener(|_, _: &ClickEvent, window, _| window.remove_window()),
                ))
                .child(primary(
                    "perm-setup-all",
                    if self.asking.is_some() { "Pidiendo…" } else { "Pedir todos" },
                    cx.listener(|view, _: &ClickEvent, _, cx| view.ask_all(cx)),
                ))
            })
            .when(restart, |el| {
                el.child(primary(
                    "perm-setup-restart",
                    "Reiniciar Atic",
                    cx.listener(|view, _: &ClickEvent, window, cx| {
                        view.done(window);
                        if let Some(app) = crate::platform::bundle_path() {
                            if crate::platform::reopen_after_exit(&app).is_ok() {
                                cx.quit();
                            }
                        }
                    }),
                ))
            })
            .when(all_granted && !restart, |el| {
                el.child(primary(
                    "perm-setup-done",
                    "Listo",
                    cx.listener(|view, _: &ClickEvent, window, _| view.done(window)),
                ))
            })
            .when(!all_granted, |el| {
                el.child(text_button(
                    "perm-setup-dismiss",
                    "No preguntar por estos",
                    cx.listener(|view, _: &ClickEvent, window, _| view.done(window)),
                ))
            });

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(crate::theme::window_bg())
            .font_family(crate::theme::font())
            .text_color(hsla(TEXT))
            .child(
                div()
                    .h(px(TOP_H))
                    .flex_none()
                    .pl(px(GUTTER - 8.0))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(chrome::lights())
                    .child(chrome::logo(24.0))
                    .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).child("Permisos"))
                    .child(chrome::drag(TOP_H))
                    .child(chrome::controls(window.is_maximized(), TOP_H)),
            )
            .child(
                div()
                    .id("perm-setup-content")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(GUTTER))
                    .pb(px(GUTTER))
                    .flex()
                    .flex_col()
                    .gap(px(16.))
                    .child(div().pt(px(8.)).text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).child(
                        if all_granted { "Atic ya tiene todo lo que necesita" } else { "Dale a Atic los permisos de macOS" },
                    ))
                    .when(!all_granted, |el| el.child(div().text_size(px(13.)).text_color(hsla(MUTED)).child(
                        "Cada herramienta usa uno. «Pedir todos» los pide de a uno: acepta el diálogo o activa Atic en \
                         la lista de Ajustes del Sistema que se abre, y vuelve aquí; el siguiente se pide solo.",
                    )))
                    .child(rows)
                    .when(restart, |el| {
                        el.child(div().text_size(px(12.)).text_color(hsla(AMBER)).child(
                            "macOS aplica Grabación de pantalla al volver a abrir la app: reinicia Atic para usar Capturas, \
                             la Pizarra y Flip.",
                        ))
                    })
                    .child(div().text_size(px(11.)).text_color(hsla(FAINT)).child(holder))
                    .when_some(status, |el, (text, color)| el.child(div().text_size(px(12.)).text_color(hsla(color)).child(text)))
                    .child(actions),
            )
            .children(m3_scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn se_muestra_si_falta_uno_que_no_faltaba_al_cerrarla() {
        let missing = [Permission::Accessibility, Permission::SystemAudio];
        // Nunca se cerró: se muestra.
        assert!(should_open(&missing, None));
        // Se cerró con esos mismos faltando: no.
        let both = vec!["Accessibility".to_string(), "SystemAudio".to_string()];
        assert!(!should_open(&missing, Some(&both)));
        // Se cerró con solo uno faltando (el otro se quitó después): sí.
        let one = vec!["SystemAudio".to_string()];
        assert!(should_open(&missing, Some(&one)));
        // La marca vacía de antes no tapa nada.
        assert!(should_open(&missing, Some(&[])));
        // Sin nada que falte, nunca.
        assert!(!should_open(&[], None));
    }
}
