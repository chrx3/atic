//! Las secciones General, Capturas y Lanzador de la ventana de Ajustes: las
//! que antes solo estaban en los Ajustes de la app de Tauri. Todo va a
//! `config.json`; la pill lo aplica al verlo cambiar (`config_watch.rs`).

use std::path::PathBuf;

use atic_core::{AppDirs, Config, SecretKind};
use gpui::{div, prelude::*, px, App, ClickEvent, Context, Entity, SharedString, Window};

use crate::i18n::{t, tf};
use crate::meetings::settings::{button, card, heading, row, save_config, segmented, switch, ConfigPane};

/// Lo que comparten las tres: dónde está `config.json` y lo último leído.
struct Shared {
    path: PathBuf,
    cfg: Config,
}

impl Shared {
    fn load() -> Option<Self> {
        let path = AppDirs::new().ok()?.config_path();
        Some(Self { cfg: Config::load(&path), path })
    }
}

macro_rules! config_pane {
    ($name:ident) => {
        impl ConfigPane for $name {
            fn edit_config(&mut self, cx: &mut Context<Self>, apply: impl FnOnce(&mut Config)) {
                self.shared.cfg = save_config(&self.shared.path, apply);
                cx.notify();
            }
            fn key_notice(&self) -> Option<&(SecretKind, String, bool)> {
                None
            }
            fn set_key_notice(&mut self, _: (SecretKind, String, bool), _: &mut Context<Self>) {}
        }
    };
}

// --- General -------------------------------------------------------------------------

pub struct GeneralPane {
    shared: Shared,
}

config_pane!(GeneralPane);

/// La sección, o `None` sin la carpeta de datos de Atic.
pub fn general_pane(cx: &mut App) -> Option<Entity<GeneralPane>> {
    let shared = Shared::load()?;
    Some(cx.new(|_| GeneralPane { shared }))
}

/// Lo que se conserva: etiqueta y valor (en días o en horas). `0` es
/// «siempre», que va traducido.
const KEEP_DAYS: [(&str, &str); 4] = [("7 d", "7"), ("30 d", "30"), ("90 d", "90"), ("365 d", "365")];
const KEEP_HOURS: [(&str, &str); 4] = [("24 h", "24"), ("3 d", "72"), ("7 d", "168"), ("30 d", "720")];
/// Cuánto se queda la foto del estante; «siempre» = hasta cerrarla.
const SHELF_SECONDS: [(&str, &str); 3] = [("10 s", "10"), ("20 s", "20"), ("60 s", "60")];

fn with_forever(options: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    let mut all = options.to_vec();
    all.push((t("pill.settings.forever"), "0"));
    all
}

impl Render for GeneralPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let cfg = self.shared.cfg.clone();
        let languages = [
            (t("settings.language.system"), "system"),
            (t("settings.language.es"), "es"),
            (t("settings.language.en"), "en"),
        ];
        let days = with_forever(&KEEP_DAYS);
        let autostart = cfg.autostart;
        let detect = cfg.detect_meetings;
        let cleanup = cfg.retention_auto_cleanup;
        let welcome = !cfg.onboarding_done;
        div()
            .flex()
            .flex_col()
            .when(welcome, |el| el.child(self.welcome(cx)).child(heading(t("settings.language.label"))))
            .child(
                card().child(row(
                    t("settings.language.label"),
                    t("settings.language.hint"),
                    segmented("general-language", &languages, &cfg.ui_language, |cfg, v| cfg.ui_language = v.into(), cx),
                )),
            )
            .child(heading(t("settings.startup.title")))
            .child(
                card()
                    .child(row(
                        t("settings.startup.autostart"),
                        t("pill.settings.autostartHint"),
                        switch(
                            "general-autostart",
                            autostart,
                            cx.listener(move |pane, _: &ClickEvent, _, cx| {
                                pane.edit_config(cx, |cfg| cfg.autostart = !autostart)
                            }),
                        ),
                    ))
                    .child(row(
                        t("pill.settings.detectMeetings"),
                        t("pill.settings.detectMeetingsHint"),
                        switch(
                            "general-detect",
                            detect,
                            cx.listener(move |pane, _: &ClickEvent, _, cx| {
                                pane.edit_config(cx, |cfg| cfg.detect_meetings = !detect)
                            }),
                        ),
                    )),
            )
            .child(heading(t("settings.data.title")))
            .child(
                card()
                    .child(row(
                        t("settings.data.keep"),
                        t("settings.data.keepHint"),
                        segmented(
                            "general-keep",
                            &days,
                            &cfg.retention_days.to_string(),
                            |cfg, v| cfg.retention_days = v.parse().unwrap_or(cfg.retention_days),
                            cx,
                        ),
                    ))
                    .child(row(
                        t("settings.data.autoCleanup"),
                        t("pill.settings.autoCleanupHint"),
                        switch(
                            "general-cleanup",
                            cleanup,
                            cx.listener(move |pane, _: &ClickEvent, _, cx| {
                                pane.edit_config(cx, |cfg| cfg.retention_auto_cleanup = !cleanup)
                            }),
                        ),
                    ))
                    .child(row(
                        t("settings.data.folder"),
                        t("settings.data.folderHint"),
                        button("general-folder".into(), t("settings.data.openFolder"), |_, _, _| {
                            if let Ok(dirs) = AppDirs::new() {
                                let _ = std::process::Command::new("explorer").arg(dirs.data_dir()).spawn();
                            }
                        }),
                    )),
            )
    }
}

impl GeneralPane {
    /// La primera vez: qué es Atic, qué hacer antes de usarla y «Empezar».
    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        use crate::settings::{hsla, MUTED, TEXT};
        let line = |text: &'static str| div().text_size(px(12.)).text_color(hsla(MUTED)).child(text);
        card().child(
            div()
                .p(px(16.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .child(div().text_size(px(16.)).text_color(hsla(TEXT)).child(t("pill.onboarding.title")))
                .child(line(t("onboarding.claim1")))
                .child(line(t("onboarding.claim2")))
                .child(line(t("onboarding.claim3")))
                .child(div().pt(px(6.)).text_size(px(13.)).text_color(hsla(TEXT)).child(t("pill.onboarding.steps")))
                .child(line(t("pill.onboarding.step1")))
                .child(line(t("pill.onboarding.step2")))
                .child(line(t("pill.onboarding.step3")))
                .child(
                    div().pt(px(6.)).flex().child(button(
                        "onboarding-start".into(),
                        t("pill.onboarding.start"),
                        cx.listener(|pane, _: &ClickEvent, _, cx| {
                            pane.edit_config(cx, |cfg| cfg.onboarding_done = true)
                        }),
                    )),
                ),
        )
    }
}

// --- Capturas ------------------------------------------------------------------------

pub struct CapturesPane {
    shared: Shared,
    /// Lo que dijo «Limpiar ahora».
    cleaned: Option<SharedString>,
}

config_pane!(CapturesPane);

pub fn captures_pane(cx: &mut App) -> Option<Entity<CapturesPane>> {
    let shared = Shared::load()?;
    Some(cx.new(|_| CapturesPane { shared, cleaned: None }))
}

impl Render for CapturesPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let hours = with_forever(&KEEP_HOURS);
        let current = self.shared.cfg.capture_retention_hours.to_string();
        let seconds = with_forever(&SHELF_SECONDS);
        let shelf_seconds = self.shared.cfg.capture_shelf_timeout_seconds.to_string();
        let sides = [(t("settings.captures.left"), "left"), (t("settings.captures.right"), "right")];
        let side = self.shared.cfg.capture_shelf_side.clone();
        let cursor = self.shared.cfg.capture_include_cursor;
        let shelf = card()
            .child(row(
                t("settings.captures.side"),
                t("settings.captures.shelfHint"),
                segmented(
                    "captures-side",
                    &sides,
                    &side,
                    |cfg, v| cfg.capture_shelf_side = v.to_string(),
                    cx,
                ),
            ))
            .child(row(
                t("settings.captures.timeout"),
                t("settings.captures.timeoutHint"),
                segmented(
                    "captures-shelf-seconds",
                    &seconds,
                    &shelf_seconds,
                    |cfg, v| cfg.capture_shelf_timeout_seconds = v.parse().unwrap_or(cfg.capture_shelf_timeout_seconds),
                    cx,
                ),
            ));
        let image = card()
            .child(row(
                t("settings.captures.cursor"),
                "",
                switch(
                    "captures-cursor",
                    cursor,
                    cx.listener(move |pane, _: &ClickEvent, _, cx| {
                        pane.edit_config(cx, |cfg| cfg.capture_include_cursor = !cursor)
                    }),
                ),
            ))
            .child(row(
                t("settings.captures.keep"),
                t("settings.captures.keepHint"),
                segmented(
                    "captures-keep",
                    &hours,
                    &current,
                    |cfg, v| cfg.capture_retention_hours = v.parse().unwrap_or(cfg.capture_retention_hours),
                    cx,
                ),
            ))
            .child(row(
                t("settings.captures.cleanup"),
                self.cleaned.clone().unwrap_or_else(|| t("settings.captures.cleanupHint").into()),
                button(
                    "captures-cleanup".into(),
                    t("settings.captures.cleanupBtn"),
                    cx.listener(|pane, _: &ClickEvent, _, cx| {
                        pane.cleaned = Some(cleanup_now(&pane.shared.cfg).into());
                        cx.notify();
                    }),
                ),
            ));
        div()
            .flex()
            .flex_col()
            .child(heading(t("settings.captures.shelf")))
            .child(shelf)
            .child(heading(t("settings.captures.image")))
            .child(image)
    }
}

/// Borra las capturas vencidas según lo elegido y dice cuántas.
fn cleanup_now(cfg: &Config) -> String {
    let Ok(dirs) = AppDirs::new() else {
        return t("settings.captures.nothingExpired").into();
    };
    let result = atic_capture::retention::cleanup_captures(
        &dirs.captures_dir(),
        cfg.capture_retention_hours,
        std::time::SystemTime::now(),
    );
    if result.deleted == 0 {
        t("settings.captures.nothingExpired").into()
    } else {
        tf("settings.captures.cleaned", &[("count", &result.deleted)])
    }
}

// --- Lanzador ------------------------------------------------------------------------

pub struct LauncherPane {
    shared: Shared,
}

config_pane!(LauncherPane);

pub fn launcher_pane(cx: &mut App) -> Option<Entity<LauncherPane>> {
    let shared = Shared::load()?;
    Some(cx.new(|_| LauncherPane { shared }))
}

impl Render for LauncherPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let on = self.shared.cfg.launcher_currency;
        card().child(row(
            t("settings.launcher.currency"),
            t("settings.launcher.currencyHint"),
            switch(
                "launcher-currency",
                on,
                cx.listener(move |pane, _: &ClickEvent, _, cx| pane.edit_config(cx, |cfg| cfg.launcher_currency = !on)),
            ),
        ))
    }
}
