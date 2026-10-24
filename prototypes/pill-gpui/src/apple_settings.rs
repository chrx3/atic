//! La voz de macOS y Apple Intelligence en Ajustes: si están, y cómo
//! activarlas cuando no (bajar el idioma o abrir Ajustes del Sistema).
//!
//! El estado lo da el ayudante `atic-apple` (`platform::apple`), que tarda unos
//! cientos de ms: se guarda aquí, se relee en un hilo al abrir la sección o
//! con «Revisar», y las vistas dibujan lo último que se supo.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gpui::{div, prelude::*, px, AnyElement, App, ClickEvent, Context, SharedString, Window};

use crate::i18n::{t, tf};
use crate::settings::{hsla, MUTED};
use crate::theme::GREEN;

use crate::platform::apple::{self, Intelligence, Speech, Status};

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub status: Option<Status>,
    /// Bajando el modelo de voz: de 0 a 1.
    pub installing: Option<f32>,
    /// El último fallo del ayudante (no está, o no respondió).
    pub error: Option<String>,
}

struct Shared {
    snapshot: Snapshot,
    checked: Option<Instant>,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    snapshot: Snapshot { status: None, installing: None, error: None },
    checked: None,
});

pub fn snapshot() -> Snapshot {
    SHARED.lock().map(|s| s.snapshot.clone()).unwrap_or_default()
}

fn edit(apply: impl FnOnce(&mut Shared)) {
    if let Ok(mut shared) = SHARED.lock() {
        apply(&mut shared);
    }
}

/// El idioma de Reuniones y del dictado (vacío = el del sistema).
fn language() -> Option<String> {
    let dirs = atic_core::AppDirs::new().ok()?;
    atic_core::Config::load(&dirs.config_path()).whisper_language()
}

/// Relee el estado en un hilo y vuelve a dibujar la vista.
pub fn refresh<V: 'static>(cx: &mut Context<V>) {
    edit(|s| s.checked = Some(Instant::now()));
    cx.spawn(async move |view, cx| {
        let result = cx
            .background_spawn(async move {
                if !apple::installed() {
                    return Err(t("pill.apple.missingHelper").to_string());
                }
                apple::status(language().as_deref())
            })
            .await;
        edit(|s| match result {
            Ok(status) => {
                s.snapshot.status = Some(status);
                s.snapshot.error = None;
            }
            Err(error) => s.snapshot.error = Some(error),
        });
        let _ = view.update(cx, |_, cx| cx.notify());
    })
    .detach();
}

/// Relee si nunca se miró o si pasó un rato (al volver de Ajustes del Sistema).
pub fn refresh_if_stale<V: 'static>(cx: &mut Context<V>) {
    let stale = SHARED
        .lock()
        .map(|s| s.checked.is_none_or(|at| at.elapsed() > Duration::from_secs(15)))
        .unwrap_or(false);
    if stale {
        refresh(cx);
    }
}

/// Baja el modelo de voz del idioma, mostrando el avance.
pub fn install<V: 'static>(cx: &mut Context<V>) {
    if snapshot().installing.is_some() {
        return;
    }
    edit(|s| s.snapshot.installing = Some(0.0));
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    std::thread::Builder::new()
        .name("apple-idioma".into())
        .spawn(move || {
            let result = apple::install(language().as_deref(), |p| edit(|s| s.snapshot.installing = Some(p)));
            edit(|s| {
                s.snapshot.installing = None;
                s.snapshot.error = result.err();
            });
            finished.store(true, Ordering::Release);
        })
        .ok();
    cx.spawn(async move |view, cx| {
        while !done.load(Ordering::Acquire) {
            cx.background_executor().timer(Duration::from_millis(300)).await;
            if view.update(cx, |_, cx| cx.notify()).is_err() {
                return;
            }
        }
        let _ = view.update(cx, |_, cx| refresh(cx));
    })
    .detach();
}

/// Lo que hace el botón de una fila.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    /// Bajar el modelo de voz del idioma.
    Download,
    /// Abrir Ajustes del Sistema → Apple Intelligence y Siri.
    OpenSettings,
    /// Volver a mirar (algo que el sistema está terminando).
    Recheck,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::None => "",
            Action::Download => t("pill.apple.download"),
            Action::OpenSettings => t("pill.apple.enable"),
            Action::Recheck => t("pill.apple.recheck"),
        }
    }
}

/// Una fila: el valor a la vista, la explicación y qué ofrecer.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub value: String,
    pub hint: String,
    pub action: Action,
    /// Se puede usar ya.
    pub ready: bool,
}

pub fn speech_line(snap: &Snapshot) -> Line {
    if let Some(p) = snap.installing {
        return Line {
            value: tf("pill.apple.downloadingPct", &[("pct", &((p * 100.0).round() as u32))]),
            hint: t("pill.apple.speechDownloadHint").into(),
            action: Action::None,
            ready: false,
        };
    }
    let Some(status) = &snap.status else {
        return match &snap.error {
            Some(error) => Line { value: t("pill.apple.unavailable").into(), hint: error.clone(), action: Action::Recheck, ready: false },
            None => Line { value: t("pill.apple.checking").into(), hint: String::new(), action: Action::None, ready: false },
        };
    };
    match status.speech {
        Speech::Ready => Line {
            value: tf("pill.apple.speechReady", &[("locale", &status.locale)]),
            hint: t("pill.apple.speechReadyHint").into(),
            action: Action::None,
            ready: true,
        },
        Speech::Downloading => Line {
            value: t("pill.apple.downloading").into(),
            hint: t("pill.apple.speechDownloadingHint").into(),
            action: Action::Recheck,
            ready: false,
        },
        Speech::Unsupported if status.locale.is_empty() => Line {
            value: t("pill.apple.noLanguage").into(),
            hint: t("pill.apple.noLanguageHint").into(),
            action: Action::None,
            ready: false,
        },
        Speech::Unsupported => Line {
            value: t("pill.apple.missingLanguage").into(),
            hint: tf("pill.apple.missingLanguageHint", &[("locale", &status.locale)]),
            action: Action::Download,
            ready: false,
        },
    }
}

pub fn intelligence_line(snap: &Snapshot) -> Line {
    let Some(status) = &snap.status else {
        return Line {
            value: if snap.error.is_some() { t("pill.apple.unavailable") } else { t("pill.apple.checking") }.into(),
            hint: String::new(),
            action: if snap.error.is_some() { Action::Recheck } else { Action::None },
            ready: false,
        };
    };
    match status.intelligence {
        Intelligence::Available if !status.intelligence_speaks => Line {
            value: t("pill.apple.aiNoLanguage").into(),
            hint: t("pill.apple.aiNoLanguageHint").into(),
            action: Action::None,
            ready: false,
        },
        Intelligence::Available => Line {
            value: t("pill.apple.aiOn").into(),
            hint: t("pill.apple.aiOnHint").into(),
            action: Action::None,
            ready: true,
        },
        Intelligence::NotEnabled => Line {
            value: t("pill.apple.aiOff").into(),
            hint: t("pill.apple.aiOffHint").into(),
            action: Action::OpenSettings,
            ready: false,
        },
        Intelligence::NotReady => Line {
            value: t("pill.apple.aiPreparing").into(),
            hint: t("pill.apple.aiPreparingHint").into(),
            action: Action::Recheck,
            ready: false,
        },
        Intelligence::NotEligible => Line {
            value: t("pill.apple.unavailable").into(),
            hint: t("pill.apple.aiNotEligibleHint").into(),
            action: Action::None,
            ready: false,
        },
        Intelligence::Unavailable => Line {
            value: t("pill.apple.unavailable").into(),
            hint: t("pill.apple.aiUnavailableHint").into(),
            action: Action::Recheck,
            ready: false,
        },
    }
}

/// Qué hace el botón de una acción en la vista `V`.
pub fn on_action<V: 'static>(action: Action, cx: &mut Context<V>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static + use<V> {
    let view = cx.entity().downgrade();
    move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
        let _ = view.update(cx, |_, cx| run_action(action, cx));
    }
}

fn run_action<V: 'static>(action: Action, cx: &mut Context<V>) {
    match action {
        Action::None => {}
        Action::Download => install(cx),
        Action::OpenSettings => {
            apple::open_intelligence_settings();
            // Al volver de Ajustes del Sistema la fila ya debería decir «Activo».
            cx.spawn(async move |view, cx| {
                for _ in 0..12 {
                    cx.background_executor().timer(Duration::from_secs(5)).await;
                    if view.update(cx, |_, cx| refresh(cx)).is_err() {
                        return;
                    }
                }
            })
            .detach();
        }
        Action::Recheck => refresh(cx),
    }
}

/// El valor de la fila y, si hay algo que hacer, su botón.
pub fn status_control<V: 'static>(id: &'static str, line: &Line, cx: &mut Context<V>) -> AnyElement {
    let value: SharedString = line.value.clone().into();
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .text_size(px(12.))
                .text_color(if line.ready { hsla(GREEN) } else { hsla(MUTED) })
                .child(value),
        )
        .when(line.action != Action::None, |el| {
            el.child(crate::meetings::settings::button(SharedString::from(id), line.action.label(), on_action(line.action, cx)))
        })
        .into_any_element()
}

/// La explicación con el valor delante, para las filas que no tienen columna de valor.
pub fn hint_with_value(line: &Line) -> SharedString {
    if line.hint.is_empty() {
        line.value.clone().into()
    } else {
        format!("{} · {}", line.value, line.hint).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(speech: Speech, intelligence: Intelligence) -> Snapshot {
        Snapshot {
            status: Some(Status { speech, locale: "es-CL".into(), intelligence, intelligence_speaks: true }),
            installing: None,
            error: None,
        }
    }

    #[test]
    fn la_voz_lista_no_ofrece_nada() {
        let line = speech_line(&status(Speech::Ready, Intelligence::Available));
        assert!(line.ready);
        assert_eq!(line.action, Action::None);
        assert!(line.value.contains("es-CL"));
    }

    #[test]
    fn sin_el_idioma_ofrece_bajarlo_y_muestra_el_avance() {
        let mut snap = status(Speech::Unsupported, Intelligence::Available);
        assert_eq!(speech_line(&snap).action, Action::Download);
        snap.installing = Some(0.42);
        let line = speech_line(&snap);
        assert_eq!(line.value, "Bajando… 42 %");
        assert_eq!(line.action, Action::None);
    }

    #[test]
    fn apple_intelligence_apagado_lleva_a_ajustes() {
        let line = intelligence_line(&status(Speech::Ready, Intelligence::NotEnabled));
        assert_eq!(line.action, Action::OpenSettings);
        assert!(!line.ready);
        assert_eq!(intelligence_line(&status(Speech::Ready, Intelligence::NotEligible)).action, Action::None);
        let mut mute = status(Speech::Ready, Intelligence::Available);
        if let Some(s) = mute.status.as_mut() {
            s.intelligence_speaks = false;
        }
        assert!(!intelligence_line(&mute).ready);
    }

    #[test]
    fn sin_ayudante_se_puede_volver_a_mirar() {
        let snap = Snapshot { status: None, installing: None, error: Some("falta".into()) };
        assert_eq!(speech_line(&snap).action, Action::Recheck);
        assert_eq!(intelligence_line(&snap).action, Action::Recheck);
        assert_eq!(speech_line(&Snapshot::default()).value, "Revisando…");
    }
}
