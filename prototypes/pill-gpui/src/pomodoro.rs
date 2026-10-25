//! Pomodoro: rondas de enfoque con pausas, en la tira, el carrusel y su panel.
//!
//! De fábrica, 25 min de enfoque, 5 de pausa corta y, cada cuatro enfoques,
//! 15 de pausa larga; todo se ajusta en su panel (`pomodoro_panel.rs`). Al
//! terminar uno se pasa al siguiente y llega un aviso del sistema (y un
//! sonido, si está activo); la pausa arranca sola solo si así se eligió.
//!
//! Mientras corre (o quedó a medias), el tiempo se ve en el tab en reposo, en
//! el lugar del reloj de grabar (`Pill::left_clock`).
//!
//! El tiempo sale del reloj (`Instant`), no de contar sondeos: si la pill se
//! atrasa, no se atrasa el Pomodoro. Los ajustes y lo enfocado hoy se guardan
//! en `<datos de Atic>/pill/pomodoro.json`; la ronda en curso no.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Focus,
    Short,
    Long,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Focus, Mode::Short, Mode::Long];

    pub fn label(self) -> &'static str {
        match self {
            Mode::Focus => crate::i18n::t("pill.pomodoro.focus"),
            Mode::Short => crate::i18n::t("pill.pomodoro.short"),
            Mode::Long => crate::i18n::t("pill.pomodoro.long"),
        }
    }
}

/// Lo que se elige en el panel y lo enfocado hoy.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub focus_min: u32,
    pub short_min: u32,
    pub long_min: u32,
    /// Cada cuántos enfoques toca la pausa larga.
    pub rounds: u32,
    /// Al terminar uno, el siguiente arranca solo.
    pub auto_start: bool,
    /// Suena al terminar (el toque de empezar a grabar de Reuniones).
    pub sound: bool,
    /// El día de `today_done` y `today_secs` («2026-10-25»).
    pub today: String,
    pub today_done: u32,
    pub today_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            focus_min: 25,
            short_min: 5,
            long_min: 15,
            rounds: 4,
            auto_start: false,
            sound: true,
            today: String::new(),
            today_done: 0,
            today_secs: 0,
        }
    }
}

/// Los topes de cada ajuste, en minutos (o enfoques).
pub const MINUTES: (u32, u32) = (1, 120);
pub const ROUNDS: (u32, u32) = (2, 8);

fn file() -> Option<std::path::PathBuf> {
    crate::paths::file("pomodoro.json")
}

fn day() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub struct Pomodoro {
    settings: Settings,
    mode: Mode,
    /// Lo que quedaba al pausar (o todo, sin empezar).
    left: Duration,
    /// Corriendo desde entonces, con `left` por delante.
    since: Option<Instant>,
    /// Enfoques terminados en esta tanda: cada `rounds` toca la larga.
    done: u32,
}

impl Default for Pomodoro {
    fn default() -> Self {
        Self::with(Settings::default())
    }
}

impl Pomodoro {
    fn with(settings: Settings) -> Self {
        let mut pomodoro = Self { settings, mode: Mode::Focus, left: Duration::ZERO, since: None, done: 0 };
        pomodoro.left = pomodoro.length(Mode::Focus);
        pomodoro
    }

    /// Con los ajustes guardados.
    pub fn load() -> Self {
        let settings = file()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self::with(settings)
    }

    fn save(&self) {
        // Las pruebas no tocan los datos de verdad.
        if cfg!(test) {
            return;
        }
        let Some(path) = file() else {
            return;
        };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&self.settings) {
            let _ = std::fs::write(path, bytes);
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn length(&self, mode: Mode) -> Duration {
        let minutes = match mode {
            Mode::Focus => self.settings.focus_min,
            Mode::Short => self.settings.short_min,
            Mode::Long => self.settings.long_min,
        };
        Duration::from_secs(u64::from(minutes.max(1)) * 60)
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    /// Empezado: corriendo o pausado a medias.
    pub fn started(&self) -> bool {
        self.running() || self.left < self.length(self.mode)
    }

    pub fn rounds(&self) -> u32 {
        self.settings.rounds.max(1)
    }

    /// El enfoque en que va (1 a `rounds`).
    pub fn round(&self) -> u32 {
        self.done % self.rounds() + 1
    }

    /// Enfoques terminados hoy y cuánto tiempo sumaron.
    pub fn today(&self) -> (u32, Duration) {
        if self.settings.today == day() {
            (self.settings.today_done, Duration::from_secs(self.settings.today_secs))
        } else {
            (0, Duration::ZERO)
        }
    }

    pub fn remaining(&self, now: Instant) -> Duration {
        match self.since {
            Some(since) => self.left.saturating_sub(now.saturating_duration_since(since)),
            None => self.left,
        }
    }

    /// Cuánto lleva (0 a 1).
    pub fn progress(&self, now: Instant) -> f32 {
        let total = self.length(self.mode).as_secs_f32();
        (1.0 - self.remaining(now).as_secs_f32() / total).clamp(0.0, 1.0)
    }

    /// Empezar, pausar o seguir.
    pub fn toggle(&mut self, now: Instant) {
        if self.since.is_some() {
            self.left = self.remaining(now);
            self.since = None;
        } else {
            self.since = Some(now);
        }
    }

    /// Vuelve al principio del modo, en pausa.
    pub fn reset(&mut self) {
        self.left = self.length(self.mode);
        self.since = None;
    }

    /// Otro modo, desde el principio y en pausa.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.reset();
    }

    /// Pasa al que sigue sin esperar (un enfoque saltado no suma a hoy).
    pub fn skip(&mut self, now: Instant) {
        self.finish(now, false);
    }

    /// `completed`: el tiempo se cumplió (suma a hoy si era enfoque).
    fn finish(&mut self, now: Instant, completed: bool) -> Mode {
        let ended = self.mode;
        if ended == Mode::Focus {
            self.done += 1;
            if completed {
                let today = day();
                if self.settings.today != today {
                    self.settings.today = today;
                    self.settings.today_done = 0;
                    self.settings.today_secs = 0;
                }
                self.settings.today_done += 1;
                self.settings.today_secs += self.length(Mode::Focus).as_secs();
                self.save();
            }
        }
        let next = match ended {
            Mode::Focus if self.done % self.rounds() == 0 => Mode::Long,
            Mode::Focus => Mode::Short,
            _ => Mode::Focus,
        };
        self.set_mode(next);
        if completed && self.settings.auto_start {
            self.since = Some(now);
        }
        ended
    }

    /// En cada sondeo: si se acabó el tiempo, pasa al siguiente y devuelve el
    /// que terminó, para avisar.
    pub fn tick(&mut self, now: Instant) -> Option<Mode> {
        (self.running() && self.remaining(now).is_zero()).then(|| self.finish(now, true))
    }

    /// El reloj del tab: mientras esté empezado.
    pub fn clock(&self, now: Instant) -> Option<String> {
        self.started().then(|| format(self.remaining(now)))
    }

    /// Cambia los minutos de un modo. Si ese modo no ha empezado, toma el
    /// largo nuevo al tiro.
    pub fn nudge_minutes(&mut self, mode: Mode, delta: i32) {
        let value = match mode {
            Mode::Focus => &mut self.settings.focus_min,
            Mode::Short => &mut self.settings.short_min,
            Mode::Long => &mut self.settings.long_min,
        };
        *value = (*value as i32 + delta).clamp(MINUTES.0 as i32, MINUTES.1 as i32) as u32;
        if mode == self.mode && !self.started() {
            self.reset();
        }
        self.save();
    }

    pub fn nudge_rounds(&mut self, delta: i32) {
        self.settings.rounds = (self.settings.rounds as i32 + delta).clamp(ROUNDS.0 as i32, ROUNDS.1 as i32) as u32;
        self.save();
    }

    pub fn set_auto_start(&mut self, on: bool) {
        self.settings.auto_start = on;
        self.save();
    }

    pub fn set_sound(&mut self, on: bool) {
        self.settings.sound = on;
        self.save();
    }
}

/// «24:13»: los segundos se redondean hacia arriba (en 0 ya terminó).
pub fn format(left: Duration) -> String {
    let secs = left.as_secs() + u64::from(left.subsec_nanos() > 0);
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// «1 h 15 min» o «40 min».
pub fn span(total: Duration) -> String {
    let minutes = total.as_secs() / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_reloj_redondea_hacia_arriba() {
        assert_eq!(format(Duration::from_secs(25 * 60)), "25:00");
        assert_eq!(format(Duration::from_millis(59_400)), "01:00");
        assert_eq!(format(Duration::from_millis(400)), "00:01");
        assert_eq!(format(Duration::ZERO), "00:00");
        assert_eq!(span(Duration::from_secs(75 * 60)), "1 h 15 min");
        assert_eq!(span(Duration::from_secs(40 * 60)), "40 min");
    }

    #[test]
    fn pausar_guarda_lo_que_quedaba() {
        let start = Instant::now();
        let mut p = Pomodoro::default();
        assert!(!p.started() && p.clock(start).is_none());
        p.toggle(start);
        let later = start + Duration::from_secs(60);
        p.toggle(later);
        assert!(!p.running());
        assert_eq!(p.remaining(later + Duration::from_secs(600)), Duration::from_secs(24 * 60));
        assert_eq!(p.clock(later).as_deref(), Some("24:00"));
    }

    #[test]
    fn cada_cuatro_enfoques_la_pausa_larga() {
        let now = Instant::now();
        let mut p = Pomodoro::default();
        let mut seen = Vec::new();
        for _ in 0..8 {
            p.skip(now);
            seen.push(p.mode());
        }
        use Mode::*;
        assert_eq!(seen, [Short, Focus, Short, Focus, Short, Focus, Long, Focus]);
    }

    #[test]
    fn al_terminar_pasa_al_siguiente_en_pausa() {
        let start = Instant::now();
        let mut p = Pomodoro::default();
        p.toggle(start);
        assert_eq!(p.tick(start + Duration::from_secs(60)), None);
        assert_eq!(p.tick(start + Duration::from_secs(25 * 60)), Some(Mode::Focus));
        assert_eq!(p.mode(), Mode::Short);
        assert!(!p.running() && p.round() == 2);
    }

    #[test]
    fn con_arranque_solo_la_pausa_corre() {
        let start = Instant::now();
        let mut p = Pomodoro::with(Settings { auto_start: true, focus_min: 1, ..Settings::default() });
        p.toggle(start);
        assert_eq!(p.tick(start + Duration::from_secs(60)), Some(Mode::Focus));
        assert!(p.running() && p.mode() == Mode::Short);
    }
}
