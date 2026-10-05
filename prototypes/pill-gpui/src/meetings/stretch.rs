//! Cambiar la velocidad sin que la voz suba de tono: WSOLA (superposición de
//! ventanas con búsqueda de similitud) sobre el flujo ya mezclado y a la tasa
//! del dispositivo.
//!
//! Cada ~12 ms de salida se toma una ventana de ~25 ms de la entrada cerca de
//! donde «debería» ir a esa velocidad, corrida unos ms hacia donde mejor
//! empalma con lo anterior (así las ondas se suman en fase y no hay ecos ni
//! clics), y se suma con una Hann que con salto de media ventana da 1 exacto.
//!
//! A 1× no hace nada: los cuadros pasan tal cual.

use std::collections::VecDeque;

use super::player::Frame;

/// Las velocidades que ofrece la barra, en el orden en que se ciclan.
pub const SPEEDS: [f32; 4] = [1.0, 1.25, 1.5, 2.0];

/// La siguiente de `SPEEDS` (vuelve a 1× después de la última).
pub fn next_speed(speed: f32) -> f32 {
    let at = SPEEDS.iter().position(|s| (s - speed).abs() < 0.01);
    SPEEDS[at.map_or(0, |i| (i + 1) % SPEEDS.len())]
}

/// «1×», «1,25×»: con coma, como se escribe aquí.
pub fn speed_label(speed: f32) -> String {
    let text = format!("{:.2}", speed);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    format!("{}×", text.replace('.', ","))
}

pub fn is_normal(speed: f32) -> bool {
    (speed - 1.0).abs() < 0.001
}

/// Ventana de análisis.
const WINDOW_MS: f32 = 25.0;
/// Cuánto puede correrse cada ventana buscando dónde empalmar: más que el
/// período de una voz grave (~12 ms a 80 Hz).
const SEEK_MS: f32 = 8.0;
/// La comparación mira una de cada tantas muestras: la forma de la onda de
/// voz se ve igual y cuesta un tercio.
const SEEK_STRIDE: usize = 3;

pub struct Stretch {
    speed: f64,
    /// Largo de la ventana y salto de salida (la mitad).
    n: usize,
    hop: usize,
    delta: usize,
    window: Vec<f32>,
    /// La entrada pendiente; `base` es el índice absoluto de `input[0]`.
    input: VecDeque<Frame>,
    base: u64,
    /// Ventanas ya escritas desde el último `reset`.
    k: u64,
    /// Dónde empezó la última ventana tomada.
    prev: Option<u64>,
    /// La suma en curso (una ventana de largo).
    acc: Vec<Frame>,
    out: VecDeque<Frame>,
}

impl Stretch {
    pub fn new(rate: u32, speed: f32) -> Self {
        let rate = rate.max(8_000) as f32;
        let n = (((rate * WINDOW_MS / 1000.0) as usize) / 2 * 2).max(64);
        let window = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
            .collect();
        Self {
            speed: speed as f64,
            n,
            hop: n / 2,
            delta: (rate * SEEK_MS / 1000.0) as usize,
            window,
            input: VecDeque::with_capacity(n * 4),
            base: 0,
            k: 0,
            prev: None,
            acc: vec![[0.0; 2]; n],
            out: VecDeque::with_capacity(n),
        }
    }

    pub fn speed(&self) -> f32 {
        self.speed as f32
    }

    /// Tras saltar o cambiar de velocidad: lo pendiente ya no corresponde.
    pub fn reset(&mut self, speed: f32) {
        self.speed = speed as f64;
        self.input.clear();
        self.out.clear();
        self.acc.iter_mut().for_each(|f| *f = [0.0; 2]);
        self.base = 0;
        self.k = 0;
        self.prev = None;
    }

    pub fn push(&mut self, frame: Frame) {
        if is_normal(self.speed as f32) {
            self.out.push_back(frame);
            return;
        }
        self.input.push_back(frame);
        while self.step() {}
    }

    pub fn pop(&mut self) -> Option<Frame> {
        self.out.pop_front()
    }

    fn at(&self, abs: u64) -> Frame {
        self.input[(abs - self.base) as usize]
    }

    /// Escribe una ventana si ya llegó la entrada que necesita.
    fn step(&mut self) -> bool {
        let ideal = (self.k as f64 * self.hop as f64 * self.speed) as u64;
        let natural = self.prev.map(|p| p + self.hop as u64);
        let lo = ideal.saturating_sub(self.delta as u64).max(self.base);
        let hi = ideal + self.delta as u64;
        let end = self.base + self.input.len() as u64;
        let need = (hi + self.n as u64).max(natural.map_or(0, |p| p + self.hop as u64));
        if end < need {
            return false;
        }
        let start = match natural {
            None => ideal.max(self.base),
            Some(natural) => self.best_match(natural, lo, hi),
        };
        for i in 0..self.n {
            let f = self.at(start + i as u64);
            let w = self.window[i];
            self.acc[i][0] += f[0] * w;
            self.acc[i][1] += f[1] * w;
        }
        self.out.extend(self.acc.drain(..self.hop));
        self.acc.extend(std::iter::repeat([0.0; 2]).take(self.hop));
        self.prev = Some(start);
        self.k += 1;
        // Lo que ya no puede volver a usarse.
        let next_lo = ((self.k as f64 * self.hop as f64 * self.speed) as u64).saturating_sub(self.delta as u64);
        let keep = next_lo.min(start + self.hop as u64);
        while self.base < keep && !self.input.is_empty() {
            self.input.pop_front();
            self.base += 1;
        }
        true
    }

    /// El comienzo en `lo..=hi` cuya primera media ventana más se parece a
    /// la continuación natural de la anterior (correlación normalizada).
    fn best_match(&self, natural: u64, lo: u64, hi: u64) -> u64 {
        let mono = |f: Frame| f[0] + f[1];
        let target: Vec<f32> = (0..self.hop)
            .step_by(SEEK_STRIDE)
            .map(|i| mono(self.at(natural + i as u64)))
            .collect();
        let mut best = (f32::MIN, natural.clamp(lo, hi));
        for c in lo..=hi {
            let (mut dot, mut energy) = (0.0f32, 0.0f32);
            for (j, t) in target.iter().enumerate() {
                let x = mono(self.at(c + (j * SEEK_STRIDE) as u64));
                dot += x * t;
                energy += x * x;
            }
            let score = dot / (energy + 1e-6).sqrt();
            if score > best.0 {
                best = (score, c);
            }
        }
        best.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(n: usize, rate: f32, hz: f32) -> Vec<Frame> {
        (0..n)
            .map(|i| {
                let v = 0.8 * (std::f32::consts::TAU * hz * i as f32 / rate).sin();
                [v, v]
            })
            .collect()
    }

    fn run(stretch: &mut Stretch, input: &[Frame]) -> Vec<Frame> {
        let mut out = Vec::new();
        for f in input {
            stretch.push(*f);
            while let Some(o) = stretch.pop() {
                out.push(o);
            }
        }
        out
    }

    #[test]
    fn a_1x_pasa_identico() {
        let input = sine(5_000, 48_000.0, 220.0);
        let mut s = Stretch::new(48_000, 1.0);
        assert_eq!(run(&mut s, &input), input);
    }

    #[test]
    fn la_duracion_se_divide_por_la_velocidad() {
        let rate = 48_000;
        let input = sine(rate as usize * 3, rate as f32, 180.0);
        for speed in [1.25f32, 1.5, 2.0] {
            let mut s = Stretch::new(rate, speed);
            let out = run(&mut s, &input).len() as f32;
            let want = input.len() as f32 / speed;
            // Lo que queda esperando en el búfer: menos de unas ventanas.
            assert!((out - want).abs() < 0.03 * rate as f32, "{speed}×: {out} vs {want}");
        }
    }

    #[test]
    fn sin_clics_en_las_uniones() {
        let rate = 48_000.0;
        let hz = 440.0;
        let input = sine(48_000 * 2, rate, hz);
        // La pendiente máxima de la senoidal: un salto mayor sería un clic.
        let slope = 0.8 * std::f32::consts::TAU * hz / rate;
        for speed in [1.25f32, 1.5, 2.0] {
            let mut s = Stretch::new(48_000, speed);
            let out = run(&mut s, &input);
            // Pasado el fundido de entrada de la primera ventana.
            let steady = &out[2_000..];
            let worst = steady.windows(2).map(|w| (w[1][0] - w[0][0]).abs()).fold(0.0, f32::max);
            assert!(worst < slope * 1.3, "{speed}×: salto {worst} > {slope}");
            // Y sin huecos: las ventanas se suman en fase (sin cancelarse).
            for chunk in steady.chunks(1_200) {
                let peak = chunk.iter().map(|f| f[0].abs()).fold(0.0, f32::max);
                assert!(peak > 0.7, "{speed}×: pico {peak}");
            }
        }
    }

    #[test]
    fn reiniciar_suelta_lo_pendiente() {
        let mut s = Stretch::new(48_000, 1.5);
        run(&mut s, &sine(3_000, 48_000.0, 300.0));
        s.reset(2.0);
        assert!(s.pop().is_none());
        assert_eq!(s.speed(), 2.0);
        s.reset(1.0);
        s.push([0.25, -0.25]);
        assert_eq!(s.pop(), Some([0.25, -0.25]));
    }

    #[test]
    fn las_velocidades_se_ciclan_y_se_escriben_con_coma() {
        assert_eq!(next_speed(1.0), 1.25);
        assert_eq!(next_speed(2.0), 1.0);
        assert_eq!(next_speed(1.7), 1.0);
        assert_eq!(speed_label(1.0), "1×");
        assert_eq!(speed_label(1.25), "1,25×");
        assert_eq!(speed_label(1.5), "1,5×");
        assert_eq!(speed_label(2.0), "2×");
    }
}
