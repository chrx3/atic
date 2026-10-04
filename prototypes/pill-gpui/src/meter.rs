//! `PILL_FPS=1`: cuadros por segundo y el peor cuadro de una vista, una vez
//! por segundo en stderr. Para medir antes de optimizar.

use std::time::{Duration, Instant};

pub struct Meter {
    label: &'static str,
    enabled: bool,
    last: Option<Instant>,
    since: Instant,
    frames: u32,
    worst_gap: Duration,
    worst_build: Duration,
}

impl Meter {
    pub fn new(label: &'static str) -> Self {
        Self {
            label,
            enabled: std::env::var_os("PILL_FPS").is_some(),
            last: None,
            since: Instant::now(),
            frames: 0,
            worst_gap: Duration::ZERO,
            worst_build: Duration::ZERO,
        }
    }

    /// Un render: `build` es lo que tardó armar los elementos.
    pub fn frame(&mut self, started: Instant) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        if let Some(last) = self.last {
            self.worst_gap = self.worst_gap.max(now.duration_since(last));
        }
        self.worst_build = self.worst_build.max(now.duration_since(started));
        self.last = Some(now);
        self.frames += 1;
        if now.duration_since(self.since) >= Duration::from_secs(1) {
            eprintln!(
                "{}: {} cuadros/s, peor intervalo {:.1} ms, peor armado {:.2} ms",
                self.label,
                self.frames,
                self.worst_gap.as_secs_f32() * 1000.0,
                self.worst_build.as_secs_f32() * 1000.0
            );
            self.since = now;
            self.frames = 0;
            self.worst_gap = Duration::ZERO;
            self.worst_build = Duration::ZERO;
        }
    }
}
