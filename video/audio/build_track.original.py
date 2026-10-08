"""
build_track.py
==============
Pista electronica frenetica para video de 30 s.
150 BPM, Fa# menor, 18 compases de 4/4 (1.6 s c/u) => 28.8 s exactos.

Determinista (semilla fija). Sintesis pura con numpy: bombo, sub, bajo,
caja/palmas, hi-hats, arpegio pluck, stabs, lead, pad, risers, crash,
sidechain, reverb, limitador.

Genera:
    track.wav   (44100 Hz, 2 ch, 16 bit, 1 270 080 muestras)
    beats.json  (mapa de compases + tiempos de kicks/snares/hits)
    sfx_*.wav   (efectos sueltos para sincronizar cortes)
"""

import json
import math
import os
import wave

import numpy as np

# ------------------------------------------------------------------#
# Configuracion global                                               #
# ------------------------------------------------------------------#

SR = 44100                 # Hz
DURATION_SEC = 28.8        # exactamente 28.8 s
TOTAL_SAMPLES = int(round(SR * DURATION_SEC))   # 1 270 080

BPM = 150.0
BEAT_SEC = 60.0 / BPM      # 0.4 s
BAR_SEC = BEAT_SEC * 4     # 1.6 s
SIXTEENTH = BEAT_SEC / 4   # 0.1 s

N_BARS = 18
SEED = 1234

# Fa# menor: F# G# A B C# D E (octava central)
F_SHARP_MINOR = [369.99, 415.30, 440.00, 493.88, 554.37, 622.25, 659.25,
                 739.99]  # F#4 ... F#5

SUB_ROOT = F_SHARP_MINOR[0] / 2.0  # ~185 Hz (F#3)

# ------------------------------------------------------------------#
# Utilidades                                                         #
# ------------------------------------------------------------------#

rng = np.random.default_rng(SEED)


def soft_clip(x: np.ndarray, drive: float = 1.5) -> np.ndarray:
    """Soft-clip tanh."""
    return np.tanh(drive * x)


def db_to_gain(db: float) -> float:
    return 10.0 ** (db / 20.0)


def env_adsr(n: int, a=0.005, d=0.05, s=0.7, r=0.10, sr=SR) -> np.ndarray:
    """Envolvente ADSR simple (samples)."""
    a = max(1, int(a * sr))
    d = max(1, int(d * sr))
    r = max(1, int(r * sr))
    s_n = max(0, n - (a + d + r))
    attack = np.linspace(0.0, 1.0, a, endpoint=False)
    decay = np.linspace(1.0, s, d, endpoint=False)
    sustain = np.full(s_n, s)
    release = np.linspace(s, 0.0, r, endpoint=True)
    env = np.concatenate([attack, decay, sustain, release])
    if len(env) < n:
        env = np.concatenate([env, np.zeros(n - len(env))])
    else:
        env = env[:n]
    return env


def lowpass(x: np.ndarray, cutoff_hz: float, sr=SR) -> np.ndarray:
    """Filtro paso bajo de un polo."""
    rc = 1.0 / (2.0 * math.pi * max(cutoff_hz, 1.0))
    dt = 1.0 / sr
    alpha = dt / (rc + dt)
    y = np.empty_like(x)
    y[0] = x[0] * alpha
    for i in range(1, len(x)):
        y[i] = y[i - 1] + alpha * (x[i] - y[i - 1])
    return y


def highpass(x: np.ndarray, cutoff_hz: float, sr=SR) -> np.ndarray:
    """Filtro paso alto de un polo."""
    rc = 1.0 / (2.0 * math.pi * max(cutoff_hz, 1.0))
    dt = 1.0 / sr
    alpha = rc / (rc + dt)
    y = np.empty_like(x)
    y[0] = x[0]
    for i in range(1, len(x)):
        y[i] = alpha * (y[i - 1] + x[i] - x[i - 1])
    return y


def biquad_lp(x: np.ndarray, cutoff: float, q: float = 0.707, sr=SR) -> np.ndarray:
    """Biquad paso bajo (RBJ)."""
    w0 = 2.0 * math.pi * cutoff / sr
    cos_w = math.cos(w0)
    sin_w = math.sin(w0)
    alpha = sin_w / (2.0 * q)
    b0 = (1.0 - cos_w) / 2.0
    b1 = 1.0 - cos_w
    b2 = (1.0 - cos_w) / 2.0
    a0 = 1.0 + alpha
    a1 = -2.0 * cos_w
    a2 = 1.0 - alpha
    b0 /= a0; b1 /= a0; b2 /= a0; a1 /= a0; a2 /= a0
    y = np.zeros_like(x)
    x1 = 0.0; x2 = 0.0; y1 = 0.0; y2 = 0.0
    for i in range(len(x)):
        xv = float(x[i])
        yv = b0 * xv + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yv
        x2 = x1; x1 = xv; y2 = y1; y1 = yv
    return y


def biquad_hp(x: np.ndarray, cutoff: float, q: float = 0.707, sr=SR) -> np.ndarray:
    w0 = 2.0 * math.pi * cutoff / sr
    cos_w = math.cos(w0)
    sin_w = math.sin(w0)
    alpha = sin_w / (2.0 * q)
    b0 = (1.0 + cos_w) / 2.0
    b1 = -(1.0 + cos_w)
    b2 = (1.0 + cos_w) / 2.0
    a0 = 1.0 + alpha
    a1 = -2.0 * cos_w
    a2 = 1.0 - alpha
    b0 /= a0; b1 /= a0; b2 /= a0; a1 /= a0; a2 /= a0
    y = np.zeros_like(x)
    x1 = 0.0; x2 = 0.0; y1 = 0.0; y2 = 0.0
    for i in range(len(x)):
        xv = float(x[i])
        yv = b0 * xv + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yv
        x2 = x1; x1 = xv; y2 = y1; y1 = yv
    return y


def write_wav_mono(path: str, x: np.ndarray):
    x = np.asarray(x, dtype=np.float32)
    if x.ndim == 2:
        x = x.mean(axis=1)
    peak = float(np.max(np.abs(x))) if len(x) else 0.0
    if peak > 0:
        x = x / peak * db_to_gain(-1.0)
    pcm = np.clip(x, -1.0, 1.0)
    pcm16 = (pcm * 32767.0).astype(np.int16)
    with wave.open(path, "wb") as wf:
        wf.setnchannels(1)
        wf.setsampwidth(2)
        wf.setframerate(SR)
        wf.writeframes(pcm16.tobytes())


def write_wav_stereo(path: str, left: np.ndarray, right: np.ndarray, peak_db=-1.0):
    left = np.asarray(left, dtype=np.float32)
    right = np.asarray(right, dtype=np.float32)
    n = min(len(left), len(right))
    left = left[:n]; right = right[:n]
    peak = float(max(np.max(np.abs(left)), np.max(np.abs(right)))) if n else 0.0
    if peak > 0:
        g = db_to_gain(peak_db) / peak
        left = left * g; right = right * g
    left = np.clip(left, -1.0, 1.0)
    right = np.clip(right, -1.0, 1.0)
    l16 = (left * 32767.0).astype(np.int16)
    r16 = (right * 32767.0).astype(np.int16)
    stereo = np.empty(n * 2, dtype=np.int16)
    stereo[0::2] = l16
    stereo[1::2] = r16
    with wave.open(path, "wb") as wf:
        wf.setnchannels(2)
        wf.setsampwidth(2)
        wf.setframerate(SR)
        wf.writeframes(stereo.tobytes())


# ------------------------------------------------------------------#
# Sintetizadores                                                     #
# ------------------------------------------------------------------#

def kick_at(t_sec: float, duration=0.40, gain=1.0) -> np.ndarray:
    """Bombo: seno con pitch sweep 120 Hz -> 45 Hz + click."""
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    f0, f1 = 120.0, 45.0
    k = math.log(f1 / f0) / max(duration, 0.01)
    inst_freq = f0 * np.exp(k * t)
    phase = 2.0 * math.pi * np.cumsum(inst_freq) / SR
    body = np.sin(phase)
    cn = min(int(0.01 * SR), n)
    click_env = np.exp(-np.arange(cn) / (0.004 * SR))
    click = rng.standard_normal(cn).astype(np.float32) * click_env * 0.5
    body_env = np.exp(-t / 0.18)
    sig = body * body_env
    sig[:cn] += click
    return (sig * gain).astype(np.float32)


def snare_at(t_sec: float, duration=0.22, gain=1.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    n_tone = min(int(0.06 * SR), n)
    t = np.arange(n_tone) / SR
    tone = np.sin(2 * math.pi * 180.0 * t) * np.exp(-t / 0.03)
    noise_part = rng.standard_normal(n).astype(np.float32)
    noise_part = highpass(noise_part, 1500.0)
    noise_env = np.exp(-np.arange(n) / SR / 0.06)
    body = noise_part * noise_env * 0.7
    full = np.zeros(n, dtype=np.float32)
    full[:n_tone] += tone.astype(np.float32) * 0.3
    full += body.astype(np.float32)
    return full * gain


def hihat_at(t_sec: float, open_=False, gain=0.25) -> np.ndarray:
    duration = 0.16 if open_ else 0.05
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    sig = rng.standard_normal(n).astype(np.float32)
    sig = highpass(sig, 7000.0)
    env = np.exp(-np.arange(n) / SR / (0.05 if open_ else 0.018))
    return (sig * env * gain).astype(np.float32)


def shaker_at(t_sec: float, gain=0.20) -> np.ndarray:
    n = int(round(0.08 * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    sig = rng.standard_normal(n).astype(np.float32)
    sig = highpass(sig, 5000.0)
    env = np.exp(-np.arange(n) / SR / 0.02)
    return (sig * env * gain).astype(np.float32)


def rim_at(t_sec: float, gain=0.35) -> np.ndarray:
    n = int(round(0.05 * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    tone = np.sin(2 * math.pi * 880.0 * t) * np.exp(-t / 0.012)
    click = rng.standard_normal(int(0.003 * SR)).astype(np.float32) * 0.25
    sig = np.zeros(n, dtype=np.float32)
    sig[:len(click)] += click
    sig += tone.astype(np.float32) * 0.6
    return sig * gain


def pluck_at(t_sec: float, freq: float, duration=0.16, gain=0.30,
             cutoff=2400.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    phase1 = 2.0 * math.pi * freq * t
    phase2 = 2.0 * math.pi * freq * t + math.pi
    sig = (np.sin(phase1) + np.sin(phase2) * 0.6) * 0.4
    sig = biquad_lp(sig.astype(np.float32), cutoff, q=2.0)
    env = np.exp(-t / (duration * 0.35))
    sig = sig * env
    return (sig * gain).astype(np.float32)


def stab_at(t_sec: float, freqs, duration=0.30, gain=0.30,
            cutoff=1800.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    sig = np.zeros(n, dtype=np.float32)
    for f in freqs:
        sig += np.sin(2.0 * math.pi * f * t).astype(np.float32) * 0.5
    sig = biquad_lp(sig, cutoff, q=1.5)
    env = env_adsr(n, a=0.002, d=0.04, s=0.35, r=0.12, sr=SR)
    sig = sig * env
    return (sig * gain).astype(np.float32)


def lead_at(t_sec: float, freq: float, duration=0.5, gain=0.22) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    sig = np.sin(2.0 * math.pi * freq * t).astype(np.float32) * 0.4
    sig += np.sin(2.0 * math.pi * (freq * 1.005) * t).astype(np.float32) * 0.4
    sig += (np.sin(2.0 * math.pi * freq * 2 * t) * 0.18).astype(np.float32)
    sig = biquad_hp(sig, 600.0)
    env = env_adsr(n, a=0.005, d=0.05, s=0.7, r=0.15, sr=SR)
    return (sig * env * gain).astype(np.float32)


def pad_at(t_sec: float, freqs, duration=1.0, gain=0.18,
           cutoff=1000.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    sig = np.zeros(n, dtype=np.float32)
    for i, f in enumerate(freqs):
        detune = 1.0 + (0.003 if i % 2 else -0.003)
        sig += np.sin(2.0 * math.pi * f * detune * t).astype(np.float32) * (0.4 / len(freqs))
    sig = biquad_lp(sig, cutoff, q=0.7)
    env = env_adsr(n, a=0.08, d=0.10, s=0.7, r=0.30, sr=SR)
    return (sig * env * gain).astype(np.float32)


def bass_note_at(t_sec: float, freq: float, duration=0.4, gain=0.40,
                 cutoff=900.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    phase = 2.0 * math.pi * freq * t
    sig = np.zeros(n, dtype=np.float32)
    for h in (1, 3, 5, 7, 9):
        sig += (np.sin(h * phase) / h).astype(np.float32)
    sig *= 0.45
    t_env = np.linspace(0, 1, n)
    env_cutoff = np.exp(-t_env * 4.0)
    chunk = 256
    out = np.zeros_like(sig)
    pos = 0
    while pos < n:
        end = min(pos + chunk, n)
        c = cutoff * (0.5 + 1.5 * env_cutoff[pos])
        out[pos:end] = biquad_lp(sig[pos:end], max(c, 80.0), q=2.0)
        pos = end
    amp_env = env_adsr(n, a=0.003, d=0.05, s=0.6, r=0.08, sr=SR)
    return (out * amp_env * gain).astype(np.float32)


def sub_at(t_sec: float, freq: float, duration=0.8, gain=0.60) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    sig = np.sin(2.0 * math.pi * freq * t).astype(np.float32)
    env = env_adsr(n, a=0.003, d=0.10, s=0.7, r=0.20, sr=SR)
    return (sig * env * gain).astype(np.float32)


def crash_at(t_sec: float, duration=1.5, gain=0.40) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    sig = rng.standard_normal(n).astype(np.float32)
    sig = highpass(sig, 3000.0)
    chunk = 512
    out = np.zeros_like(sig)
    pos = 0
    brightness = 1.0
    while pos < n:
        end = min(pos + chunk, n)
        c = 8000.0 * brightness + 1500.0
        out[pos:end] = biquad_hp(sig[pos:end], c, q=0.7)
        pos = end
        brightness *= 0.985
    env = np.exp(-np.arange(n) / SR / (duration * 0.5))
    return (out * env * gain).astype(np.float32)


def riser_at(t_sec: float, duration=1.0, direction="up", gain=0.25) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    sig = rng.standard_normal(n).astype(np.float32)
    chunk = 256
    out = np.zeros_like(sig)
    pos = 0
    if direction == "up":
        f0, f1 = 200.0, 8000.0
    else:
        f0, f1 = 8000.0, 200.0
    while pos < n:
        end = min(pos + chunk, n)
        frac = pos / n
        c = f0 * (f1 / f0) ** frac
        out[pos:end] = biquad_hp(sig[pos:end], max(c, 80.0), q=0.7)
        pos = end
    env = np.linspace(0.2, 1.0, n).astype(np.float32) ** 1.5
    return (out * env * gain).astype(np.float32)


def sweep_at(t_sec: float, duration=0.8, direction="up", gain=0.25) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    t = np.arange(n) / SR
    if direction == "up":
        f0, f1 = 100.0, 4000.0
    else:
        f0, f1 = 4000.0, 100.0
    k = math.log(f1 / f0) / duration
    inst_freq = f0 * np.exp(k * t)
    phase = 2.0 * math.pi * np.cumsum(inst_freq) / SR
    sig = np.sin(phase).astype(np.float32)
    env = env_adsr(n, a=0.02, d=0.10, s=0.8, r=0.20, sr=SR)
    return (sig * env * gain).astype(np.float32)


def impact_at(t_sec: float, duration=0.9, gain=1.0) -> np.ndarray:
    n = int(round(duration * SR))
    if n <= 0:
        return np.zeros(0, dtype=np.float32)
    sig = np.zeros(n, dtype=np.float32)
    t = np.arange(n) / SR
    f0, f1 = 90.0, 30.0
    k = math.log(f1 / f0) / duration
    inst_freq = f0 * np.exp(k * t)
    phase = 2.0 * math.pi * np.cumsum(inst_freq) / SR
    body = np.sin(phase).astype(np.float32) * np.exp(-t / 0.30)
    sig += body * 0.85
    cn = int(0.01 * SR)
    click_env = np.exp(-np.arange(cn) / (0.003 * SR))
    click = rng.standard_normal(cn).astype(np.float32) * click_env * 0.6
    sig[:cn] += click
    return sig * gain


def whoosh(duration=0.5, direction="up", gain=0.6) -> np.ndarray:
    n = int(round(duration * SR))
    sig = rng.standard_normal(n).astype(np.float32)
    if direction == "up":
        f0, f1 = 400.0, 8000.0
    else:
        f0, f1 = 8000.0, 400.0
    chunk = 256
    out = np.zeros_like(sig)
    pos = 0
    while pos < n:
        end = min(pos + chunk, n)
        frac = pos / n
        c = f0 * (f1 / f0) ** frac
        out[pos:end] = biquad_hp(sig[pos:end], c, q=0.7)
        pos = end
    env = np.exp(-((np.arange(n) / SR - duration / 2) ** 2) / (2 * (duration / 3) ** 2))
    return (out * env * gain).astype(np.float32)


def click_sfx(duration=0.08) -> np.ndarray:
    n = int(round(duration * SR))
    sig = rng.standard_normal(n).astype(np.float32)
    sig = biquad_hp(sig, 2500.0, q=1.5)
    env = np.exp(-np.arange(n) / SR / 0.012)
    return (sig * env * 0.8).astype(np.float32)


def pop_sfx(duration=0.15) -> np.ndarray:
    n = int(round(duration * SR))
    t = np.arange(n) / SR
    f0, f1 = 800.0, 200.0
    k = math.log(f1 / f0) / duration
    inst_freq = f0 * np.exp(k * t)
    phase = 2.0 * math.pi * np.cumsum(inst_freq) / SR
    sig = np.sin(phase).astype(np.float32)
    env = np.exp(-np.arange(n) / SR / 0.04)
    return (sig * env * 0.8).astype(np.float32)


# ------------------------------------------------------------------#
# Reverb por convolucion (impulso sintetico)                         #
# ------------------------------------------------------------------#

def make_reverb(roomsize=0.8, damp=0.5, sr=SR) -> np.ndarray:
    n = int(roomsize * sr)
    if n < 16:
        n = 16
    t = np.arange(n) / sr
    decay = np.exp(-t / (roomsize * 0.5))
    ir = rng.standard_normal(n).astype(np.float32) * decay
    pre = int(0.02 * sr)
    ir_padded = np.zeros(n + pre, dtype=np.float32)
    ir_padded[pre:] = ir
    ir_padded = biquad_lp(ir_padded, 4000.0 * (1.0 - damp) + 800.0, q=0.7)
    m = float(np.max(np.abs(ir_padded)))
    if m > 0:
        ir_padded /= m
    return ir_padded * 0.5


# ------------------------------------------------------------------#
# Sidechain                                                          #
# ------------------------------------------------------------------#

def build_sidechain_env(kick_times, n_samples, sr=SR) -> np.ndarray:
    """Envolvente: cae a ~0.20 en cada kick y recupera en 0.18 s."""
    env = np.ones(n_samples, dtype=np.float32)
    close_samples = int(0.18 * sr)
    for t in kick_times:
        start = int(round(t * sr))
        if start >= n_samples:
            continue
        end = min(start + close_samples, n_samples)
        rec_len = end - start
        if rec_len <= 1:
            continue
        # curva de 0.20 -> 1.0
        rec = np.linspace(0.20, 1.0, rec_len, endpoint=True).astype(np.float32)
        # la primera muestra es la duck minimo
        rec[0] = 0.18
        env[start:end] = np.minimum(env[start:end], rec)
    return env


# ------------------------------------------------------------------#
# Composicion por compases                                           #
# ------------------------------------------------------------------#

def bar_time(bar: int) -> float:
    return (bar - 1) * BAR_SEC


def add(buf, sig, t_sec):
    if len(sig) == 0:
        return
    start = int(round(t_sec * SR))
    if start < 0:
        return
    if start >= len(buf):
        return
    end = min(start + len(sig), len(buf))
    eff = end - start
    buf[start:end] += sig[:eff].astype(np.float32)


# Helpers: una "capa" agrupada para simplificar sidechain
class Layer:
    def __init__(self):
        self.L = np.zeros(TOTAL_SAMPLES, dtype=np.float32)
        self.R = np.zeros(TOTAL_SAMPLES, dtype=np.float32)


HARMONIC = Layer()  # bajo + pad + stabs + arpegio -> recibe sidechain
PERCUSSION = Layer()  # bombo + caja + hats + shaker + rim
LEAD_PAD = Layer()  # lead + pad final + crash final


def add_h(sig, t_sec):
    add(HARMONIC.L, sig, t_sec)
    add(HARMONIC.R, sig, t_sec)


def add_p(sig, t_sec):
    add(PERCUSSION.L, sig, t_sec)
    add(PERCUSSION.R, sig, t_sec)


def add_l(sig, t_sec):
    add(LEAD_PAD.L, sig, t_sec)
    add(LEAD_PAD.R, sig, t_sec)


def compose():
    kick_times = []
    snare_times = []
    hit_times = []

    reverb_ir = make_reverb(roomsize=0.9, damp=0.5)

    # =================================================================
    # COMPAS 1: HOOK -> golpe seco + bombo en t=0.0 + riser al final
    # =================================================================
    t0 = bar_time(1)
    impact = impact_at(t0, duration=0.7, gain=0.85)
    add_p(impact * 0.7, t0)
    add_l(impact * 0.3, t0)
    kick = kick_at(t0, duration=0.40, gain=0.90)
    add_p(kick, t0)
    kick_times.append(t0)
    hit_times.append((t0, "kick+sub"))

    # Sub corto: solo 0.4 s para que el compas 1 quede casi vacio
    sub = sub_at(t0, SUB_ROOT, duration=0.5, gain=0.45)
    add_h(sub, t0)

    # Riser muy suave en los ultimos 0.5 s
    rs = riser_at(t0 + 1.1, duration=0.4, direction="up", gain=0.08)
    add_l(rs, t0 + 1.1)

    # =================================================================
    # COMPAS 2: BUILD
    # =================================================================
    t0 = bar_time(2)
    rs = riser_at(t0, duration=BAR_SEC, direction="up", gain=0.30)
    add_l(rs, t0)
    sw = sweep_at(t0, duration=BAR_SEC, direction="up", gain=0.18)
    add_l(sw, t0)

    for i in range(8):
        ht = t0 + i * (BEAT_SEC / 2)
        hh = hihat_at(ht, open_=(i == 3), gain=0.22)
        add_p(hh, ht)
    for i in range(8, 16):
        ht = t0 + i * SIXTEENTH
        accent = 1.0 if i < 12 else 1.0 + (i - 12) * 0.2
        hh = hihat_at(ht, open_=(i % 4 == 3), gain=0.26 * accent)
        add_p(hh, ht)

    for j in range(4):
        ht = t0 + BEAT_SEC * 3 + j * SIXTEENTH
        sd = snare_at(ht, duration=0.20, gain=0.45 + j * 0.12)
        add_p(sd, ht)
        snare_times.append(ht)

    # =================================================================
    # COMPASES 3-4: DROP A
    # =================================================================
    root = F_SHARP_MINOR[0]
    for bar in (3, 4):
        t0 = bar_time(bar)
        if bar == 3:
            cr = crash_at(t0, duration=1.4, gain=0.40)
            add_l(cr, t0)
            hit_times.append((t0, "crash+sub"))
            sub0 = sub_at(t0, SUB_ROOT, duration=1.2, gain=0.55)
            add_h(sub0, t0)

        # 4 bombos four-on-the-floor
        for k in range(4):
            kt = t0 + k * BEAT_SEC
            kk = kick_at(kt, duration=0.30, gain=0.85)
            add_p(kk, kt)
            kick_times.append(kt)

        for k in (1, 3):
            st = t0 + k * BEAT_SEC
            sd = snare_at(st, duration=0.20, gain=0.70)
            add_p(sd, st)
            snare_times.append(st)

        for i in range(8):
            ht = t0 + i * (BEAT_SEC / 2)
            hh = hihat_at(ht, open_=(i % 4 == 1), gain=0.22)
            add_p(hh, ht)

        b_notes = [root, root, F_SHARP_MINOR[2], root,
                   root, F_SHARP_MINOR[4], root, F_SHARP_MINOR[1]]
        for i, f in enumerate(b_notes):
            bt = t0 + i * (BEAT_SEC / 2) + 0.005
            bass = bass_note_at(bt, f / 2.0, duration=BEAT_SEC * 0.45,
                                gain=0.45, cutoff=900.0)
            add_h(bass, bt)

    # =================================================================
    # COMPASES 5-6: DROP B
    # =================================================================
    arp_scale = [F_SHARP_MINOR[i] for i in (0, 2, 4, 6, 4, 2)]
    for bar in (5, 6):
        t0 = bar_time(bar)
        for k in range(4):
            kt = t0 + k * BEAT_SEC
            kk = kick_at(kt, duration=0.30, gain=0.85)
            add_p(kk, kt)
            kick_times.append(kt)
        for k in (1, 3):
            st = t0 + k * BEAT_SEC
            sd = snare_at(st, duration=0.20, gain=0.70)
            add_p(sd, st)
            snare_times.append(st)
        for i in range(8):
            ht = t0 + i * (BEAT_SEC / 2)
            hh = hihat_at(ht, open_=(i % 4 == 1), gain=0.22)
            add_p(hh, ht)
        for i in range(16):
            st = t0 + i * SIXTEENTH + SIXTEENTH / 2
            sk = shaker_at(st, gain=0.14)
            add_p(sk, st)
        seq = arp_scale if bar == 5 else list(reversed(arp_scale))
        for i in range(16):
            at = t0 + i * SIXTEENTH
            f = seq[i % len(seq)] * 2.0
            pl = pluck_at(at, f, duration=0.14, gain=0.22, cutoff=3200.0)
            add_h(pl, at)
        b_notes = [root/2.0, root/2.0, F_SHARP_MINOR[2]/2.0, F_SHARP_MINOR[4]/2.0,
                   root/2.0, root/2.0, F_SHARP_MINOR[4]/2.0, F_SHARP_MINOR[2]/2.0]
        for i, f in enumerate(b_notes):
            bt = t0 + i * (BEAT_SEC / 2) + 0.005
            bass = bass_note_at(bt, f, duration=BEAT_SEC * 0.45,
                                gain=0.45, cutoff=1000.0)
            add_h(bass, bt)
        for i in range(4):
            rt = t0 + i * BEAT_SEC + BEAT_SEC / 2
            rim = rim_at(rt, gain=0.25)
            add_p(rim, rt)

    # =================================================================
    # COMPASES 7-8: DROP C
    # =================================================================
    f_sharp = F_SHARP_MINOR[0]
    a_note = F_SHARP_MINOR[2]
    c_sharp = F_SHARP_MINOR[4]
    chord_fsm = [f_sharp, a_note, c_sharp, c_sharp * 2.0]
    chord_d = [F_SHARP_MINOR[5], f_sharp * 2.0, a_note * 2.0]
    chord_bm = [F_SHARP_MINOR[3], F_SHARP_MINOR[5], f_sharp * 2.0]

    for bar in (7, 8):
        t0 = bar_time(bar)
        for k in range(4):
            kt = t0 + k * BEAT_SEC
            kk = kick_at(kt, duration=0.30, gain=0.85)
            add_p(kk, kt)
            kick_times.append(kt)
        for k in (1, 3):
            st = t0 + k * BEAT_SEC
            sd = snare_at(st, duration=0.20, gain=0.70)
            add_p(sd, st)
            snare_times.append(st)
        for i in range(8):
            ht = t0 + i * (BEAT_SEC / 2)
            hh = hihat_at(ht, open_=(i % 4 == 1), gain=0.22)
            add_p(hh, ht)

        chords_cycle = [chord_fsm, chord_d, chord_bm, chord_fsm]
        for i in range(8):
            st = t0 + i * (BEAT_SEC / 2) + BEAT_SEC / 4
            ch = chords_cycle[i % 4]
            stab = stab_at(st, ch, duration=0.28, gain=0.30, cutoff=2200.0)
            add_h(stab, st)

        for i in range(16):
            at = t0 + i * SIXTEENTH
            f = arp_scale[i % len(arp_scale)] * 2.0
            pl = pluck_at(at, f, duration=0.12, gain=0.14, cutoff=3000.0)
            add_h(pl, at)

        lead_notes = [c_sharp * 4.0, f_sharp * 4.0]
        ln = lead_notes[bar - 7]
        ld = lead_at(t0, ln, duration=BAR_SEC * 0.9, gain=0.20)
        add_l(ld, t0)

        b_notes = [root/2.0, root/2.0, F_SHARP_MINOR[2]/2.0, F_SHARP_MINOR[4]/2.0,
                   root/2.0, root/2.0, F_SHARP_MINOR[4]/2.0, F_SHARP_MINOR[2]/2.0]
        for i, f in enumerate(b_notes):
            bt = t0 + i * (BEAT_SEC / 2) + 0.005
            bass = bass_note_at(bt, f, duration=BEAT_SEC * 0.45,
                                gain=0.45, cutoff=1100.0)
            add_h(bass, bt)

        if bar == 8:
            sw_t = t0 + BAR_SEC - 0.7
            sw = sweep_at(sw_t, duration=0.7, direction="up", gain=0.22)
            add_l(sw, sw_t)

    # =================================================================
    # COMPASES 9-10: DROP D
    # =================================================================
    for bar in (9, 10):
        t0 = bar_time(bar)
        for k in range(4):
            kt = t0 + k * BEAT_SEC
            kk = kick_at(kt, duration=0.30, gain=0.90)
            add_p(kk, kt)
            kick_times.append(kt)
        for k in (1, 3):
            st = t0 + k * BEAT_SEC
            sd = snare_at(st, duration=0.20, gain=0.70)
            add_p(sd, st)
            snare_times.append(st)
        for j in range(4):
            st = t0 + BEAT_SEC * 3 + j * SIXTEENTH
            sd = snare_at(st, duration=0.18, gain=0.30 + j * 0.10)
            add_p(sd, st)
            snare_times.append(st)

        for i in range(16):
            ht = t0 + i * SIXTEENTH
            hh = hihat_at(ht, open_=(i % 4 == 3), gain=0.20)
            add_p(hh, ht)

        chords_cycle = [chord_fsm, chord_d, chord_bm, chord_fsm]
        for i in range(8):
            st = t0 + i * (BEAT_SEC / 2) + BEAT_SEC / 4
            ch = chords_cycle[i % 4]
            stab = stab_at(st, ch, duration=0.26, gain=0.30, cutoff=2400.0)
            add_h(stab, st)

        offset = (bar - 9) * 2
        for i in range(16):
            at = t0 + i * SIXTEENTH
            idx = (i + offset) % len(arp_scale)
            f = arp_scale[idx] * 2.0
            pl = pluck_at(at, f, duration=0.12, gain=0.22, cutoff=3400.0)
            add_h(pl, at)

        lead_seq = [c_sharp * 4.0, f_sharp * 4.0, a_note * 4.0, c_sharp * 4.0]
        for k in range(4):
            lt = t0 + k * BEAT_SEC + 0.02
            ln = lead_seq[k]
            ld = lead_at(lt, ln, duration=BEAT_SEC * 0.8, gain=0.20)
            add_l(ld, lt)

        b_notes = [root/2.0, root/2.0, F_SHARP_MINOR[2]/2.0, F_SHARP_MINOR[4]/2.0,
                   root/2.0, root/2.0, F_SHARP_MINOR[4]/2.0, F_SHARP_MINOR[2]/2.0]
        for i, f in enumerate(b_notes):
            bt = t0 + i * (BEAT_SEC / 2) + 0.005
            bass = bass_note_at(bt, f, duration=BEAT_SEC * 0.45,
                                gain=0.48, cutoff=1100.0)
            add_h(bass, bt)

    # =================================================================
    # COMPAS 11: LIFT
    # =================================================================
    t0 = bar_time(11)
    # 4 bombos (el brief dice "el bombo sigue" -> 4 four-on-the-floor)
    for k in range(4):
        kt = t0 + k * BEAT_SEC
        kk = kick_at(kt, duration=0.30, gain=0.90)
        add_p(kk, kt)
        kick_times.append(kt)

    for i in range(16):
        st = t0 + i * SIXTEENTH
        amp = 0.18 + 0.06 * i
        sd = snare_at(st, duration=0.18, gain=amp)
        add_p(sd, st)
        snare_times.append(st)

    rs = riser_at(t0, duration=BAR_SEC, direction="up", gain=0.40)
    add_l(rs, t0)
    sw = sweep_at(t0, duration=BAR_SEC, direction="up", gain=0.22)
    add_l(sw, t0)

    for i in range(16):
        ht = t0 + i * SIXTEENTH
        hh = hihat_at(ht, open_=(i % 4 == 3), gain=0.22)
        add_p(hh, ht)

    bass = bass_note_at(t0 + 0.01, F_SHARP_MINOR[0] / 2.0,
                        duration=BAR_SEC * 0.9, gain=0.42, cutoff=900.0)
    add_h(bass, t0)

    # =================================================================
    # COMPASES 12-13: CLIMAX
    # =================================================================
    for bar in (12, 13):
        t0 = bar_time(bar)
        # Crash al inicio (compas 12 cae justo despues de la pausa 1/16,
        # pero aqui el primer evento ya es un kick, no pausa).
        # Para respetar la micro-pausa del 1/16 antes del compás 12,
        # colocamos el primer kick + crash con un offset de 1/16 SOLO en
        # el compas 12. Ademas, no metemos eventos en el ultimo 1/16 del
        # compás 11 (ya está vacio por construcción).
        if bar == 12:
            # Silencio forzado en el primer 1/16 (lo gestiona el silencio
            # natural del final del compás 11 + offset del primer evento)
            first_offset = SIXTEENTH  # 0.1 s
            t_crash = t0 + first_offset
        else:
            first_offset = 0.0
            t_crash = t0

        cr = crash_at(t_crash, duration=1.5, gain=0.55)
        add_l(cr, t_crash)
        hit_times.append((t_crash, "crash"))

        for k in range(4):
            kt = t_crash + k * BEAT_SEC
            kk = kick_at(kt, duration=0.32, gain=0.95)
            add_p(kk, kt)
            kick_times.append(kt)

        for k in (1, 3):
            st = t_crash + k * BEAT_SEC
            sd = snare_at(st, duration=0.22, gain=0.80)
            add_p(sd, st)
            snare_times.append(st)

        for i in range(16):
            ht = t_crash + i * SIXTEENTH
            hh = hihat_at(ht, open_=(i % 4 == 3), gain=0.24)
            add_p(hh, ht)

        seq = arp_scale if bar == 12 else list(reversed(arp_scale))
        for i in range(16):
            at = t_crash + i * SIXTEENTH
            f = seq[i % len(seq)] * 2.0
            pl = pluck_at(at, f, duration=0.16, gain=0.28, cutoff=3800.0)
            add_h(pl, at)

        chords_cycle = [chord_fsm, chord_d, chord_bm, chord_fsm]
        for i in range(8):
            st = t_crash + i * (BEAT_SEC / 2) + BEAT_SEC / 4
            ch = chords_cycle[i % 4]
            stab = stab_at(st, [x * 1.5 for x in ch], duration=0.28,
                           gain=0.36, cutoff=2600.0)
            add_h(stab, st)

        lead_seq = [c_sharp * 4.0, f_sharp * 4.0, a_note * 4.0, c_sharp * 4.0]
        for k in range(4):
            lt = t_crash + k * BEAT_SEC + 0.02
            ln = lead_seq[k]
            ld = lead_at(lt, ln, duration=BEAT_SEC * 0.85, gain=0.24)
            add_l(ld, lt)

        b_notes = [root/2.0, root/2.0, F_SHARP_MINOR[2]/2.0, F_SHARP_MINOR[4]/2.0,
                   root/2.0, root/2.0, F_SHARP_MINOR[4]/2.0, F_SHARP_MINOR[2]/2.0]
        for i, f in enumerate(b_notes):
            bt = t_crash + i * (BEAT_SEC / 2) + 0.005
            bass = bass_note_at(bt, f, duration=BEAT_SEC * 0.45,
                                gain=0.55, cutoff=1200.0)
            add_h(bass, bt)

    # =================================================================
    # COMPASES 14-16: BREAKDOWN
    # =================================================================
    pad_chord = [f_sharp / 2.0, a_note / 2.0, c_sharp / 2.0, f_sharp]
    for bar in (14, 15, 16):
        t0 = bar_time(bar)
        pd = pad_at(t0, pad_chord, duration=BAR_SEC, gain=0.22,
                    cutoff=1000.0)
        add_h(pd, t0)

        for k in (0, 2):
            st = t0 + k * BEAT_SEC
            sub = sub_at(st, SUB_ROOT, duration=0.7, gain=0.45)
            add_h(sub, st)
            hit_times.append((st, "sub"))

        for k in (0, 2):
            bt = t0 + k * BEAT_SEC
            bn = bass_note_at(bt, F_SHARP_MINOR[0] / 2.0,
                              duration=BEAT_SEC * 1.8, gain=0.30,
                              cutoff=900.0)
            add_h(bn, bt)

        for i in range(4):
            ht = t0 + i * BEAT_SEC + BEAT_SEC / 2
            hh = hihat_at(ht, open_=(i == 3), gain=0.13)
            add_p(hh, ht)

        if bar == 16:
            sw = sweep_at(t0 + BAR_SEC * 0.5, duration=BAR_SEC * 0.4,
                          direction="up", gain=0.15)
            add_l(sw, t0 + BAR_SEC * 0.5)
            rs = riser_at(t0 + BAR_SEC * 0.5, duration=BAR_SEC * 0.5,
                          direction="up", gain=0.15)
            add_l(rs, t0 + BAR_SEC * 0.5)

    # Silencio de 1/8 (0.2 s) justo antes del compás 17 (entre 25.4 y 25.6)
    ss = int(round((bar_time(17) - 0.2) * SR))
    se = int(round(bar_time(17) * SR))
    HARMONIC.L[ss:se] = 0.0
    HARMONIC.R[ss:se] = 0.0
    PERCUSSION.L[ss:se] = 0.0
    PERCUSSION.R[ss:se] = 0.0
    LEAD_PAD.L[ss:se] = 0.0
    LEAD_PAD.R[ss:se] = 0.0

    # =================================================================
    # COMPASES 17-18: FINAL
    # =================================================================
    t0 = bar_time(17)
    cr = crash_at(t0, duration=2.2, gain=0.60)
    add_l(cr, t0)
    hit_times.append((t0, "final-crash"))

    sub0 = sub_at(t0, SUB_ROOT, duration=2.5, gain=0.70)
    add_h(sub0, t0)

    # 4 bombos four-on-the-floor en el compás 17 (regla: 4 en 3..13 y 17)
    for k in range(4):
        kt = t0 + k * BEAT_SEC
        kk = kick_at(kt, duration=0.50 + k * 0.15, gain=0.95 - k * 0.15)
        add_p(kk, kt)
        kick_times.append(kt)
    hit_times.append((t0, "final-kick"))

    # Compas 18: bombo apagado, solo cola
    for k in range(4):
        st = t0 + BEAT_SEC + k * BEAT_SEC
        sd = snare_at(st, duration=0.18, gain=0.30)
        add_p(sd, st)
        snare_times.append(st)
    for i in range(8):
        ht = t0 + BEAT_SEC + i * (BEAT_SEC / 2)
        hh = hihat_at(ht, open_=(i % 4 == 1), gain=0.10)
        add_p(hh, ht)

    final_chord = [f_sharp, a_note, c_sharp, f_sharp * 2.0]
    pd = pad_at(t0, final_chord, duration=BAR_SEC * 2.2, gain=0.40,
                cutoff=1500.0)
    add_h(pd, t0)

    ld = lead_at(t0, c_sharp * 4.0, duration=BAR_SEC * 1.9, gain=0.20)
    add_l(ld, t0)

    bn = bass_note_at(t0 + 0.01, F_SHARP_MINOR[0] / 2.0,
                      duration=BAR_SEC * 1.9, gain=0.50, cutoff=900.0)
    add_h(bn, t0)

    for i in range(16):
        at = t0 + i * SIXTEENTH
        idx = (len(arp_scale) - 1 - (i % len(arp_scale)))
        f = arp_scale[idx] * 2.0
        pl = pluck_at(at, f, duration=0.16, gain=0.18, cutoff=2800.0)
        add_h(pl, at)

    # =================================================================
    # Sidechain: aplicar ducking SOLO a la capa "harmonic"
    # =================================================================
    sc = build_sidechain_env(kick_times, TOTAL_SAMPLES)
    # ducking agresivo: minimo 0.15
    duck = 0.15 + 0.85 * sc
    HARMONIC.L *= duck
    HARMONIC.R *= duck

    # =================================================================
    # Mezcla final
    # =================================================================
    L = HARMONIC.L + PERCUSSION.L + LEAD_PAD.L
    R = HARMONIC.R + PERCUSSION.R + LEAD_PAD.R

    # Reverb send por canal (FFT)
    from numpy.fft import fft, ifft
    def conv(x, ir):
        n = len(x) + len(ir) - 1
        N = 1
        while N < n:
            N *= 2
        X = fft(x, N)
        H = fft(ir, N)
        y = np.real(ifft(X * H))[: len(x)]
        return y.astype(np.float32)

    send = 0.18
    # Aplicar reverb solo a partir de compás 2 (compás 1 queda "seco")
    bar1_end = int(BAR_SEC * SR)
    L_wet_full = conv(L, reverb_ir)[: len(L)]
    R_wet_full = conv(R, reverb_ir)[: len(R)]
    L_wet = np.zeros_like(L)
    R_wet = np.zeros_like(R)
    L_wet[bar1_end:] = L_wet_full[bar1_end:]
    R_wet[bar1_end:] = R_wet_full[bar1_end:]
    # Crossfade de 30 ms en el borde para evitar click
    cf = int(0.030 * SR)
    if cf > 0 and bar1_end > cf:
        ramp = np.linspace(0.0, 1.0, cf, endpoint=True).astype(np.float32)
        L_wet[bar1_end - cf:bar1_end] *= ramp
        R_wet[bar1_end - cf:bar1_end] *= ramp
    L = L * (1.0 - send) + L_wet * send
    R = R * (1.0 - send) + R_wet * send

    # Master gain: bajar para preservar headroom y dinamica
    master_gain = 0.25
    L = L * master_gain
    R = R * master_gain

    # Limitador suave
    L = soft_clip(L, drive=1.6)
    R = soft_clip(R, drive=1.6)

    # Normalización a -1 dBFS
    peak = float(max(np.max(np.abs(L)), np.max(np.abs(R))))
    target = db_to_gain(-1.0)
    if peak > 0:
        g = target / peak
        L = L * g
        R = R * g

    # Fade out 30 ms al final
    fade_n = int(0.030 * SR)
    if fade_n > 0:
        fade = np.linspace(1.0, 0.0, fade_n, endpoint=True).astype(np.float32)
        L[-fade_n:] *= fade
        R[-fade_n:] *= fade

    # Garantizar cero al final
    last_n = int(0.005 * SR)
    L[-last_n:] = 0.0
    R[-last_n:] = 0.0

    L = L[:TOTAL_SAMPLES]
    R = R[:TOTAL_SAMPLES]

    events = {
        "kick_times": sorted(set(round(t, 4) for t in kick_times)),
        "snare_times": sorted(set(round(t, 4) for t in snare_times)),
        "hit_times": hit_times,
    }
    return L, R, events


# ------------------------------------------------------------------#
# SFX sueltos                                                        #
# ------------------------------------------------------------------#

def gen_sfx():
    out = {}
    out["sfx_whoosh.wav"] = whoosh(duration=0.5, direction="up", gain=0.85)
    out["sfx_hit.wav"] = impact_at(0.0, duration=0.9, gain=1.0)
    out["sfx_click.wav"] = click_sfx(duration=0.08)
    out["sfx_pop.wav"] = pop_sfx(duration=0.15)
    out["sfx_riser.wav"] = riser_at(0.0, duration=1.6, direction="up", gain=0.85)
    out["sfx_swoosh_down.wav"] = whoosh(duration=0.4, direction="down", gain=0.85)
    return out


# ------------------------------------------------------------------#
# Bars + energia                                                     #
# ------------------------------------------------------------------#

def compute_bar_energy(L, R):
    energies = []
    for b in range(1, N_BARS + 1):
        s = int(round((b - 1) * BAR_SEC * SR))
        e = int(round(b * BAR_SEC * SR))
        e = min(e, len(L))
        seg = (L[s:e] + R[s:e]) * 0.5
        rms = float(np.sqrt(np.mean(seg ** 2))) if len(seg) else 0.0
        db = 20.0 * math.log10(max(rms, 1e-9))
        energies.append(db)
    return energies


SECTION_MAP = {
    1: "HOOK",
    2: "BUILD",
    3: "DROP A", 4: "DROP A",
    5: "DROP B", 6: "DROP B",
    7: "DROP C", 8: "DROP C",
    9: "DROP D", 10: "DROP D",
    11: "LIFT",
    12: "CLIMAX", 13: "CLIMAX",
    14: "BREAKDOWN", 15: "BREAKDOWN", 16: "BREAKDOWN",
    17: "FINAL", 18: "FINAL",
}


def energy_to_0to10(db: float) -> int:
    # -45 dB -> 0, 0 dB -> 10
    v = max(0.0, min(1.0, (db + 45.0) / 45.0))
    return int(round(v * 10))


# ------------------------------------------------------------------#
# Main                                                               #
# ------------------------------------------------------------------#

def main():
    print("[1/5] Componiendo pista...")
    L, R, events = compose()

    print("[2/5] Calculando energia por compas...")
    bar_db = compute_bar_energy(L, R)

    bars = []
    for b in range(1, N_BARS + 1):
        bars.append({
            "bar": b,
            "startSec": round((b - 1) * BAR_SEC, 4),
            "section": SECTION_MAP[b],
            "energy0to10": energy_to_0to10(bar_db[b - 1]),
        })

    beats = {
        "bpm": BPM,
        "beatSec": BEAT_SEC,
        "barSec": BAR_SEC,
        "durationSec": DURATION_SEC,
        "bars": bars,
        "kicks": events["kick_times"],
        "snares": events["snare_times"],
        "hits": [{"t": round(t, 4), "kind": k} for (t, k) in events["hit_times"]],
    }

    print("[3/5] Escribiendo track.wav ...")
    write_wav_stereo("track.wav", L, R, peak_db=-1.0)

    print("[4/5] Escribiendo beats.json ...")
    with open("beats.json", "w", encoding="utf-8") as f:
        json.dump(beats, f, indent=2)

    print("[5/5] Generando SFX sueltos ...")
    sfx = gen_sfx()
    for name, audio in sfx.items():
        write_wav_mono(name, audio)
        print("   ->", name)

    # Verificaciones
    print()
    print("=== Verificacion ===")
    with wave.open("track.wav", "rb") as wf:
        ch = wf.getnchannels()
        sw = wf.getsampwidth()
        fr = wf.getframerate()
        n = wf.getnframes()
    print(f"track.wav: ch={ch}, sampwidth={sw} bytes, fr={fr} Hz, frames={n}")
    assert ch == 2, "Debe ser estereo"
    assert sw == 2, "Debe ser 16 bit"
    assert fr == SR, f"Debe ser {SR} Hz"
    assert n == TOTAL_SAMPLES, f"Debe tener {TOTAL_SAMPLES} muestras"

    with open("beats.json", "r", encoding="utf-8") as f:
        json.load(f)

    print()
    print("Energia RMS (dBFS) por compas:")
    for b in range(1, N_BARS + 1):
        print(f"  Compas {b:2d} [{SECTION_MAP[b]:10s}] "
              f"start={bars[b-1]['startSec']:.2f}s  "
              f"RMS={bar_db[b-1]:6.2f} dBFS  energy={bars[b-1]['energy0to10']}")

    kicks = beats["kicks"]
    issues = 0
    # Agrupar por compas usando floor((k+eps) / BAR_SEC) para evitar floats
    def kick_to_bar(k):
        return int(math.floor((k + 1e-9) / BAR_SEC)) + 1
    kicks_per_bar = {}
    for k in kicks:
        b = kick_to_bar(k)
        kicks_per_bar.setdefault(b, []).append(k)
    for b in list(range(3, 14)) + [17]:
        n = len(kicks_per_bar.get(b, []))
        if n < 4:
            print(f"AVISO: compas {b} tiene {n} bombos (esperaba 4): "
                  f"{kicks_per_bar.get(b, [])}")
            issues += 1
    if not any(abs(k - 0.0) < 1e-6 for k in kicks):
        print("AVISO: no hay kick en t=0.0")
        issues += 1

    print()
    print(f"Kicks totales: {len(kicks)}")
    print(f"Snares totales: {len(beats['snares'])}")
    print(f"Hits totales: {len(beats['hits'])}")

    if issues == 0:
        print("OK - todas las verificaciones pasaron.")
    else:
        print(f"Ojo: {issues} observaciones.")

    print()
    print("=== Listo ===")
    for fn in sorted(os.listdir(".")):
        if fn.endswith((".wav", ".json")):
            sz = os.path.getsize(fn)
            print(f"  {fn}  ({sz} bytes)")


if __name__ == "__main__":
    main()