//! Utilidad de desarrollo: transcribe una o dos pistas WAV desde la línea de
//! comandos, para probar el pipeline sin la UI.
//!
//! Uso:
//!   cargo run --example try_transcribe -p atic-transcribe -- \
//!       <modelo.bin> <mic.wav> [system.wav] [idioma|auto]

use std::path::Path;

use atic_core::Speaker;
use atic_transcribe::{transcribe_recording, TrackInput};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: try_transcribe <modelo.bin> <mic.wav> [system.wav] [idioma|auto]");
        std::process::exit(1);
    }

    let model = Path::new(&args[1]);
    let mic = Path::new(&args[2]);
    let system = args.get(3).map(Path::new);
    let language = match args.get(4).map(String::as_str) {
        Some("auto") | None => None,
        Some(lang) => Some(lang),
    };

    let mut tracks = vec![TrackInput {
        wav: mic,
        speaker: Speaker::Me,
    }];
    if let Some(sys) = system {
        tracks.push(TrackInput {
            wav: sys,
            speaker: Speaker::Others,
        });
    }

    eprintln!("Transcribiendo {} pista(s)…", tracks.len());
    let transcript = transcribe_recording(model, &tracks, language, |p| {
        eprint!("\r{:>3.0}%   ", p * 100.0);
    })
    .expect("la transcripción falló");
    eprintln!();

    println!("Idioma: {:?}", transcript.language);
    println!("{} segmentos:", transcript.segments.len());
    for seg in &transcript.segments {
        let secs = seg.start_ms as f64 / 1000.0;
        println!("[{secs:>6.1}s] {}: {}", seg.speaker.label(), seg.text);
    }
}
