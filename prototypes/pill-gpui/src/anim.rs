use std::time::{Duration, Instant};

/// Curva `cubic-bezier(x1, y1, x2, y2)` como en CSS: resuelve x(t) = progreso y
/// devuelve y(t). Admite y fuera de [0, 1] para curvas con rebote.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    let bezier = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    let mut t = progress;
    for _ in 0..24 {
        let x = bezier(x1, x2, t);
        if (x - progress).abs() < 1e-4 {
            break;
        }
        if x < progress {
            low = t;
        } else {
            high = t;
        }
        t = (low + high) / 2.0;
    }
    bezier(y1, y2, t)
}

/// `--ease-island`: abre y cierra la isla con un leve sobrepaso.
pub fn ease_island(t: f32) -> f32 {
    cubic_bezier(0.33, 1.38, 0.46, 1.0, t)
}

/// `--ease-smooth-out`.
pub fn ease_smooth_out(t: f32) -> f32 {
    cubic_bezier(0.22, 1.0, 0.36, 1.0, t)
}

/// Salida con rebote para que las gotas se asienten en su sitio.
pub fn ease_back_out(t: f32) -> f32 {
    cubic_bezier(0.34, 1.56, 0.64, 1.0, t)
}

pub fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Progreso de un tramo `[start, start + duration]` dentro de una línea de tiempo.
pub fn segment(time: f32, start: f32, duration: f32) -> f32 {
    ((time - start) / duration).clamp(0.0, 1.0)
}

/// Valor interpolado que se puede redirigir a mitad de camino sin saltos.
pub struct Tween {
    from: f32,
    to: f32,
    start: Instant,
    duration: Duration,
    ease: fn(f32) -> f32,
}

impl Tween {
    pub fn new(value: f32, duration: Duration, ease: fn(f32) -> f32) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            duration,
            ease,
        }
    }

    pub fn value(&self, now: Instant) -> f32 {
        let progress =
            now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32();
        lerp(self.from, self.to, (self.ease)(progress.clamp(0.0, 1.0)))
    }

    pub fn target(&self) -> f32 {
        self.to
    }

    pub fn set(&mut self, target: f32, now: Instant) {
        if self.to == target {
            return;
        }
        self.from = self.value(now);
        self.to = target;
        self.start = now;
    }

    pub fn is_running(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.start) < self.duration
    }
}
