//! Exportar una grabación editada (Windows): el mismo video con la cámara (zoom y encuadre), el
//! cursor dibujado, los textos y el marco, sin los tramos cortados y con el audio siguiendo los
//! mismos cortes.
//!
//! El contrato es el de `export_macos.rs`: quien exporta decide qué se ve en cada cuadro
//! (`Export::decor`) y este módulo solo lo dibuja. Media Foundation decodifica (Source Reader),
//! el [`Composer`] (`compose_win.rs`, el mismo de la vista previa) compone en la GPU y el Sink
//! Writer codifica H.264 o HEVC (por hardware si hay) y AAC.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use windows::core::HSTRING;
use windows::Win32::Graphics::Direct3D11::{ID3D11Texture2D, D3D11_BIND_SHADER_RESOURCE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFByteStream, IMFMediaType, IMFSample, IMFSinkWriter, MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_HEVC,
    MFVideoFormat_RGB32, MFVideoInterlace_Progressive, MFTranscodeContainerType_MPEG4, MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE,
    MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_SINK_WRITER_DISABLE_THROTTLING, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_TRANSCODE_CONTAINERTYPE,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;

use super::compose_win::Composer;
use super::export_audio_win::{AudioSeg, AudioStream, AUDIO_RATE, FRAME_BYTES};
use super::gfx_win::Gfx;
use super::info_win::{configure, keep_media_foundation, locked, pack, to_frame, win, Source, TICKS_PER_SECOND};
use super::player_win::HardwareFrame;
use super::types::{bitrate, keep_at, out_length, out_time, output_size, pieces, Codec, Export, Piece, Scene};

/// La primera pista de video (el índice negativo que entiende Media Foundation).
const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
/// `eAVEncH264VProfile_High`.
const H264_HIGH: u32 = 100;
/// Cuánto audio se adelanta al video al escribir (segundos): el escritor intercala por tiempo,
/// pero sin tope el audio entero se acumularía en memoria.
const AUDIO_AHEAD: f64 = 0.5;

fn ticks(seconds: f64) -> i64 {
    (seconds * TICKS_PER_SECOND as f64).round() as i64
}

/// Los cuadros por segundo que declara la pista (0 si no los dice), de `MF_MT_FRAME_RATE`.
fn track_fps(source: &Source) -> f64 {
    // SAFETY: el lector está vivo.
    unsafe {
        source
            .reader
            .GetNativeMediaType(VIDEO, 0)
            .ok()
            .and_then(|media_type| media_type.GetUINT64(&MF_MT_FRAME_RATE).ok())
            .map_or(0.0, |packed| {
                let (num, den) = ((packed >> 32) as f64, (packed & 0xFFFF_FFFF) as f64);
                if den > 0.0 {
                    num / den
                } else {
                    0.0
                }
            })
    }
}

/// Un tipo de medio de video del escritor: `subtype` a `size`, `fps` cuadros por segundo.
unsafe fn video_type(subtype: &windows::core::GUID, size: (u32, u32), fps: u32) -> Result<IMFMediaType, String> {
    unsafe {
        let media_type = MFCreateMediaType().map_err(win)?;
        media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).map_err(win)?;
        media_type.SetGUID(&MF_MT_SUBTYPE, subtype).map_err(win)?;
        media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32).map_err(win)?;
        media_type.SetUINT64(&MF_MT_FRAME_SIZE, pack(size)).map_err(win)?;
        media_type.SetUINT64(&MF_MT_FRAME_RATE, pack((fps, 1))).map_err(win)?;
        media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack((1, 1))).map_err(win)?;
        Ok(media_type)
    }
}

/// El escritor MP4 con su pista de video (y la de audio, si hay): `(escritor, video, audio)`.
unsafe fn open_writer(out: &Path, size: (u32, u32), fps: u32, job: &Export, audio: bool) -> Result<(IMFSinkWriter, u32, Option<u32>), String> {
    unsafe {
        let mut attributes: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attributes, 3).map_err(win)?;
        let attributes = attributes.ok_or("Media Foundation no dio atributos")?;
        // Por hardware si hay; el escritor no frena al leer más rápido que el tiempo real.
        attributes.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1).map_err(win)?;
        attributes.SetUINT32(&MF_SINK_WRITER_DISABLE_THROTTLING, 1).map_err(win)?;
        attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4).map_err(win)?;
        let writer = MFCreateSinkWriterFromURL(&HSTRING::from(out.as_os_str()), None::<&IMFByteStream>, &attributes)
            .map_err(|error| format!("no pude crear el archivo: {}", win(error)))?;

        let options = &job.options;
        let subtype = match options.codec {
            Codec::H264 => MFVideoFormat_H264,
            Codec::Hevc => MFVideoFormat_HEVC,
        };
        let target = video_type(&subtype, size, fps)?;
        target.SetUINT32(&MF_MT_AVG_BITRATE, bitrate(size.0, size.1, options.quality, options.codec) as u32).map_err(win)?;
        if options.codec == Codec::H264 {
            target.SetUINT32(&MF_MT_MPEG2_PROFILE, H264_HIGH).map_err(win)?;
        }
        let video = writer.AddStream(&target).map_err(|error| match options.codec {
            Codec::Hevc => format!("este equipo no tiene codificador HEVC: {}", win(error)),
            Codec::H264 => format!("no hay codificador H.264: {}", win(error)),
        })?;
        // La entrada: BGRA de arriba hacia abajo (el paso positivo).
        let source = video_type(&MFVideoFormat_RGB32, size, fps)?;
        source.SetUINT32(&MF_MT_DEFAULT_STRIDE, size.0 * 4).map_err(win)?;
        writer
            .SetInputMediaType(video, &source, None)
            .map_err(|error| format!("el codificador no acepta el cuadro: {}", win(error)))?;

        let audio_stream = if audio {
            let aac = MFCreateMediaType().map_err(win)?;
            aac.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
            aac.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC).map_err(win)?;
            aac.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
            aac.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).map_err(win)?;
            aac.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(win)?;
            // 160 kbps.
            aac.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, 20_000).map_err(win)?;
            aac.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 0).map_err(win)?;
            aac.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0x29).map_err(win)?;
            let index = writer.AddStream(&aac).map_err(|error| format!("no hay codificador AAC: {}", win(error)))?;
            let pcm = MFCreateMediaType().map_err(win)?;
            pcm.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(win)?;
            pcm.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(win)?;
            pcm.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(win)?;
            pcm.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, AUDIO_RATE).map_err(win)?;
            pcm.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, 2).map_err(win)?;
            pcm.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, FRAME_BYTES as u32).map_err(win)?;
            pcm.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AUDIO_RATE * FRAME_BYTES as u32).map_err(win)?;
            writer.SetInputMediaType(index, &pcm, None).map_err(|error| format!("el codificador de audio no acepta el PCM: {}", win(error)))?;
            Some(index)
        } else {
            None
        };
        writer.BeginWriting().map_err(|error| format!("el escritor no empezó: {}", win(error)))?;
        Ok((writer, video, audio_stream))
    }
}

/// Una muestra de Media Foundation con `bytes`, en `at` (ticks) y de `duration` (ticks).
unsafe fn sample_of(bytes: &[u8], at: i64, duration: i64) -> Result<IMFSample, String> {
    unsafe {
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32).map_err(win)?;
        let mut data = std::ptr::null_mut();
        buffer.Lock(&mut data, None, None).map_err(win)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, bytes.len());
        buffer.Unlock().map_err(win)?;
        buffer.SetCurrentLength(bytes.len() as u32).map_err(win)?;
        let sample = MFCreateSample().map_err(win)?;
        sample.AddBuffer(&buffer).map_err(win)?;
        sample.SetSampleTime(at).map_err(win)?;
        sample.SetSampleDuration(duration).map_err(win)?;
        Ok(sample)
    }
}

/// La textura donde se sube cada cuadro leído (BGRA, de las medidas del video).
fn source_texture(gfx: &Gfx, (w, h): (u32, u32)) -> Result<ID3D11Texture2D, String> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w,
        Height: h,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture: Option<ID3D11Texture2D> = None;
    // SAFETY: crea una textura con el dispositivo de este hilo.
    unsafe { gfx.device.CreateTexture2D(&desc, None, Some(&mut texture)) }.map_err(win)?;
    texture.ok_or_else(|| "Direct3D no creó la textura del cuadro".into())
}

/// Exporta `source` a `out` (MP4 con las opciones de `job.options`) según `job`.
pub fn export(source: &Path, out: &Path, job: Export) -> Result<(), String> {
    // Un pánico del compositor o de Media Foundation no debe tumbar la pill: es un error de exportar.
    let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export_inner(source, out, job))) {
        Ok(result) => result,
        Err(_) => {
            let _ = std::fs::remove_file(out);
            Err("la exportación falló de forma inesperada".into())
        }
    };
    // El dispositivo de este hilo queda para la próxima exportación.
    Gfx::release_thread();
    result
}

fn export_inner(source: &Path, out: &Path, job: Export) -> Result<(), String> {
    keep_media_foundation()?;
    let gfx = Gfx::shared()?;
    let video = Source::open(source)?;
    let (src_w, src_h) = video.size;
    let source_fps = track_fps(&video);
    let options = job.options.clone();
    let (out_w, out_h) = output_size((src_w, src_h), options.max_height, options.aspect);
    let segments: Vec<(f64, f64)> = job.segments.iter().copied().filter(|(a, b)| b > a).collect();
    let pieces: Vec<Piece> = pieces(&segments, &job.speeds);
    if pieces.is_empty() {
        return Err("no queda nada del video".into());
    }
    // Lo que dura el exportado: el progreso y el cierre del archivo cuentan en su tiempo.
    let total = out_length(&pieces);
    let fps = options.fps.clamp(1, 240);

    // El audio, por bloques: nunca entero en memoria.
    let audio_segs: Vec<AudioSeg> = pieces.iter().map(|p| AudioSeg { start: p.a, end: p.b, rate: p.rate }).collect();
    let mut audio = AudioStream::open(source, &audio_segs)?.filter(|a| a.total_frames() > 0);

    // El lector, en BGRA al tamaño del original.
    let (real, stride) = configure(&video.reader, video.size)?;
    let texture = source_texture(&gfx, real)?;

    let mut composer = Composer::new()?;
    composer.set_scene(&Scene {
        src: (src_w as f64, src_h as f64),
        out: (out_w, out_h),
        frame: job.frame.clone(),
        texts: job.texts.clone(),
        cursor_px: job.cursor_px,
        look: job.look,
    })?;

    let _ = std::fs::remove_file(out);
    // SAFETY: todo el trabajo con Media Foundation y Direct3D de este hilo.
    let result = unsafe {
        let (writer, video_stream, audio_stream) = open_writer(out, (out_w, out_h), fps, &job, audio.is_some())?;
        let run = write_all(&job, &video, (real, stride), &texture, &gfx, &mut composer, &pieces, (source_fps, fps), total, &writer, video_stream, audio_stream, audio.as_mut());
        match run {
            Ok(()) => writer.Finalize().map_err(|error| format!("no se pudo cerrar el archivo: {}", win(error))),
            Err(error) => Err(error),
        }
    };
    if let Err(error) = result {
        let _ = std::fs::remove_file(out);
        return Err(error);
    }
    (job.progress)(1.0);
    Ok(())
}

/// Lee, compone y escribe todos los cuadros (y el audio que les toca).
#[allow(clippy::too_many_arguments)]
unsafe fn write_all(
    job: &Export,
    video: &Source,
    (real, stride): ((u32, u32), i32),
    texture: &ID3D11Texture2D,
    gfx: &Gfx,
    composer: &mut Composer,
    pieces: &[Piece],
    (source_fps, fps): (f64, u32),
    total: f64,
    writer: &IMFSinkWriter,
    video_stream: u32,
    audio_stream: Option<u32>,
    mut audio: Option<&mut AudioStream>,
) -> Result<(), String> {
    let first = pieces[0].a;
    let last_end = pieces[pieces.len() - 1].b;
    let frame_duration = (TICKS_PER_SECOND as f64 / fps as f64).round() as i64;
    let audio_block = (AUDIO_RATE / 10) as usize;
    let mut audio_at = 0u64;
    let mut audio_done = audio.is_none();
    let mut serial = 0u64;
    let mut last_out = f64::NEG_INFINITY;
    // El cuadro anterior espera al siguiente para saber cuánto dura.
    let mut pending: Option<(IMFSample, i64)> = None;
    let mut written = 0usize;
    // SAFETY: lector, escritor y textura viven durante toda la llamada.
    unsafe {
        let origin = windows::core::GUID::from_u128(0);
        video.reader.SetCurrentPosition(&origin, &PROPVARIANT::from(ticks(first))).map_err(win)?;
        let started = Instant::now();
        loop {
            if job.cancel.load(Ordering::Relaxed) {
                return Err("cancelada".into());
            }
            let (mut flags, mut time, mut sample) = (0u32, 0i64, None);
            video
                .reader
                .ReadSample(VIDEO, 0, None, Some(&mut flags), Some(&mut time), Some(&mut sample))
                .map_err(|error| format!("el lector del video falló: {}", win(error)))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
            let Some(sample) = sample else { continue };
            let src = time as f64 / TICKS_PER_SECOND as f64;
            if src >= last_end {
                break;
            }
            let Some(at) = out_time(pieces, src) else { continue };
            if !keep_at(pieces, src, last_out, at, fps as f64, source_fps) {
                continue;
            }
            last_out = at;
            // El cuadro a la GPU y de ahí al compositor.
            let Some(frame) = locked(&sample, |bytes| to_frame(bytes, real, stride)).flatten() else { continue };
            gfx.context.UpdateSubresource(texture, 0, None, frame.bgra.as_ptr() as *const _, real.0 * 4, 0);
            serial += 1;
            let hardware = HardwareFrame::from_parts(texture.clone(), serial, real);
            let decor = (job.decor)(src, at);
            let composed = composer.render(&hardware, &decor)?;
            let picture = composed.bgra()?;
            let at_ticks = ticks(at);
            if let Some((previous, previous_at)) = pending.take() {
                previous.SetSampleDuration((at_ticks - previous_at).max(1)).map_err(win)?;
                writer.WriteSample(video_stream, &previous).map_err(|error| format!("no se pudo escribir el cuadro: {}", win(error)))?;
                written += 1;
            }
            pending = Some((sample_of(&picture.bgra, at_ticks, frame_duration)?, at_ticks));
            (job.progress)((at / total).clamp(0.0, 0.999));

            // El audio que cae hasta un poco más allá de este cuadro.
            if let (Some(index), Some(stream)) = (audio_stream, audio.as_deref_mut()) {
                while !audio_done && (audio_at as f64 / AUDIO_RATE as f64) < at + AUDIO_AHEAD {
                    audio_done = write_audio(writer, index, stream, audio_block, &mut audio_at)?;
                }
            }
            // Un lector que no avanza no se espera para siempre.
            if started.elapsed() > Duration::from_secs(60 * 60 * 4) {
                return Err("la exportación tardó demasiado".into());
            }
        }
        if let Some((previous, _)) = pending.take() {
            writer.WriteSample(video_stream, &previous).map_err(|error| format!("no se pudo escribir el cuadro: {}", win(error)))?;
            written += 1;
        }
        if written == 0 {
            return Err("no se escribió ningún cuadro".into());
        }
        // El audio que quede.
        if let (Some(index), Some(stream)) = (audio_stream, audio.as_deref_mut()) {
            while !audio_done {
                audio_done = write_audio(writer, index, stream, audio_block, &mut audio_at)?;
            }
        }
    }
    Ok(())
}

/// Escribe un bloque de audio; `true` cuando ya no queda.
unsafe fn write_audio(writer: &IMFSinkWriter, index: u32, stream: &mut AudioStream, block: usize, audio_at: &mut u64) -> Result<bool, String> {
    unsafe {
        match stream.next_block(block)? {
            None => Ok(true),
            Some(bytes) => {
                let frames = bytes.len() / FRAME_BYTES;
                if frames > 0 {
                    let at = (*audio_at as f64 / AUDIO_RATE as f64 * TICKS_PER_SECOND as f64).round() as i64;
                    let duration = (frames as f64 / AUDIO_RATE as f64 * TICKS_PER_SECOND as f64).round() as i64;
                    let sample = sample_of(&bytes[..frames * FRAME_BYTES], at, duration)?;
                    writer.WriteSample(index, &sample).map_err(|error| format!("no se pudo escribir el audio: {}", win(error)))?;
                    *audio_at += frames as u64;
                }
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::types::{ExportOptions, Quality};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    /// Un video de verdad, a mano: `ATIC_TEST_VIDEO=<video>`. Exporta un trozo con un corte, una
    /// velocidad y el marco, y comprueba que el resultado se abre y dura lo que debe.
    #[test]
    #[ignore]
    fn exporta_de_verdad() {
        let Ok(source) = std::env::var("ATIC_TEST_VIDEO") else { return };
        let out = std::env::temp_dir().join("atic-export-test.mp4");
        let (duration, _) = super::super::info_win::video_info(Path::new(&source)).expect("info");
        let end = duration.min(6.0);
        let codec = if std::env::var("ATIC_TEST_HEVC").is_ok() { Codec::Hevc } else { Codec::H264 };
        let progress = Arc::new(std::sync::Mutex::new(0.0f64));
        let seen = progress.clone();
        let started = Instant::now();
        export(
            Path::new(&source),
            &out,
            Export {
                segments: vec![(0.0, end * 0.4), (end * 0.6, end)],
                speeds: vec![super::super::types::SpeedSpan { start: end * 0.7, end: end * 0.9, rate: 2.0 }],
                texts: Vec::new(),
                cursor_px: 24.0,
                frame: Some(super::super::types::FrameSpec { backdrop: super::super::types::Backdrop::Gradient([0.2, 0.3, 0.9], [0.9, 0.3, 0.5]), padding: 0.06, radius: 0.025, shadow: 0.5 }),
                look: Default::default(),
                options: ExportOptions { codec, quality: Quality::Standard, max_height: Some(720), fps: 30, aspect: None },
                decor: Box::new(|_, _| Default::default()),
                progress: Box::new(move |p| *seen.lock().unwrap() = p),
                cancel: Arc::new(AtomicBool::new(false)),
            },
        )
        .expect("exportar");
        let (len, size) = super::super::info_win::video_info(&out).expect("abrir lo exportado");
        println!("exportado en {:.1} s: {len:.2} s, {size:?} -> {}", started.elapsed().as_secs_f64(), out.display());
        assert!(len > 1.0);
        assert!((*progress.lock().unwrap() - 1.0).abs() < 1e-9);
    }
}
