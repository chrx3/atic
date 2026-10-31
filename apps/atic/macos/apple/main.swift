// atic-apple: lo que la pill le pide a macOS y solo existe en Swift.
//
// - status     qué hay disponible: el reconocimiento de voz en el equipo
//              (SpeechAnalyzer) para el idioma, y Apple Intelligence.
// - install    baja el modelo de voz del idioma (lo administra el sistema).
// - transcribe un WAV a texto, en el equipo y sin internet. `--mode dictation`
//              usa el dictado (puntuación); `--mode meeting` devuelve segmentos
//              con su tiempo para Reuniones.
// - stream     dictado en vivo: lee PCM del stdin (u32 LE con la tasa y después
//              f32 LE mono) y va emitiendo lo reconocido; al cerrarse el stdin,
//              el texto final.
// - polish     pule un texto dictado con el modelo de Apple Intelligence.
//
// La pill lo lanza por cada pedido (`src/platform/macos/apple.rs`) y lee un JSON
// por línea en la salida. Los errores salen como {"error": "..."} y código 1.

import AVFoundation
import Foundation
import FoundationModels
import Speech

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object, options: [.sortedKeys]),
          let line = String(data: data, encoding: .utf8)
    else { return }
    print(line)
    fflush(stdout)
}

func fail(_ message: String) -> Never {
    emit(["error": message])
    exit(1)
}

struct Args {
    var command = ""
    var lang = ""
    var mode = "dictation"
    var context: [String] = []
    var file = ""

    init(_ argv: [String]) {
        var rest = argv.dropFirst()
        command = rest.popFirst() ?? ""
        while let arg = rest.popFirst() {
            switch arg {
            case "--lang": lang = rest.popFirst() ?? ""
            case "--mode": mode = rest.popFirst() ?? mode
            case "--context":
                context = (rest.popFirst() ?? "").split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            default: file = arg
            }
        }
    }
}

// --- Idioma ------------------------------------------------------------------------

/// El idioma pedido ("es", "es-CL" o vacío = el del sistema) llevado a uno que el
/// reconocimiento soporta: el del sistema si es de ese idioma (es-CL para un Mac
/// en Chile), si no la primera variante soportada.
func resolveLocale(_ lang: String) async -> Locale? {
    let supported = await SpeechTranscriber.supportedLocales
    let current = Locale.current
    let wanted = lang.isEmpty ? current.identifier(.bcp47) : lang.replacingOccurrences(of: "_", with: "-")
    let wantedLanguage = Locale(identifier: wanted).language.languageCode?.identifier ?? wanted
    func language(_ locale: Locale) -> String? { locale.language.languageCode?.identifier }
    if let exact = supported.first(where: { $0.identifier(.bcp47).caseInsensitiveCompare(wanted) == .orderedSame }) {
        return exact
    }
    if language(current) == wantedLanguage,
       let region = current.region?.identifier,
       let same = supported.first(where: { language($0) == wantedLanguage && $0.region?.identifier == region }) {
        return same
    }
    return supported.first(where: { language($0) == wantedLanguage })
}

func statusName(_ status: AssetInventory.Status) -> String {
    switch status {
    case .unsupported: return "unsupported"
    case .supported: return "supported"
    case .downloading: return "downloading"
    case .installed: return "installed"
    @unknown default: return "unsupported"
    }
}

// --- Apple Intelligence ------------------------------------------------------------

func intelligenceState() -> (String, Bool) {
    let model = SystemLanguageModel.default
    let state: String
    switch model.availability {
    case .available: state = "available"
    case .unavailable(.deviceNotEligible): state = "deviceNotEligible"
    case .unavailable(.appleIntelligenceNotEnabled): state = "appleIntelligenceNotEnabled"
    case .unavailable(.modelNotReady): state = "modelNotReady"
    case .unavailable: state = "unavailable"
    @unknown default: state = "unavailable"
    }
    let language = Locale.current.language.languageCode
    let speaks = model.supportedLanguages.contains { $0.languageCode == language }
    return (state, speaks)
}

// --- Comandos ----------------------------------------------------------------------

func status(_ args: Args) async {
    var speech: [String: Any] = ["status": "unsupported", "locale": ""]
    if SpeechTranscriber.isAvailable, let locale = await resolveLocale(args.lang) {
        let module = SpeechTranscriber(locale: locale, preset: .transcription)
        speech = ["status": statusName(await AssetInventory.status(forModules: [module])), "locale": locale.identifier(.bcp47)]
    }
    let (state, speaks) = intelligenceState()
    emit(["speech": speech, "intelligence": ["state": state, "language": speaks]])
}

/// Deja instalado el modelo de voz del módulo; avisa el avance por línea.
func ensureInstalled(_ modules: [any SpeechModule], quiet: Bool) async throws {
    if await AssetInventory.status(forModules: modules) == .installed { return }
    guard let request = try await AssetInventory.assetInstallationRequest(supporting: modules) else { return }
    let progress = request.progress
    let watcher = Task {
        while !Task.isCancelled {
            if !quiet { emit(["progress": progress.fractionCompleted]) }
            try? await Task.sleep(nanoseconds: 300_000_000)
        }
    }
    defer { watcher.cancel() }
    try await request.downloadAndInstall()
}

func install(_ args: Args) async {
    guard let locale = await resolveLocale(args.lang) else { fail("Este idioma no tiene reconocimiento de voz en el Mac.") }
    do {
        try await ensureInstalled([SpeechTranscriber(locale: locale, preset: .transcription)], quiet: false)
        emit(["ok": true, "locale": locale.identifier(.bcp47)])
    } catch {
        fail("No se pudo bajar el modelo de voz: \(error.localizedDescription)")
    }
}

func milliseconds(_ time: CMTime) -> Int {
    time.isNumeric ? Int((CMTimeGetSeconds(time) * 1000).rounded()) : 0
}

func transcribe(_ args: Args) async {
    guard !args.file.isEmpty else { fail("Falta el archivo de audio.") }
    guard SpeechTranscriber.isAvailable else { fail("Este Mac no tiene reconocimiento de voz en el equipo.") }
    guard let locale = await resolveLocale(args.lang) else { fail("Este idioma no tiene reconocimiento de voz en el Mac.") }
    let file: AVAudioFile
    do {
        file = try AVAudioFile(forReading: URL(fileURLWithPath: args.file))
    } catch {
        fail("No se pudo abrir el audio: \(error.localizedDescription)")
    }

    // Para dictar y para Reuniones sirve el mismo transcriptor: pone puntuación
    // (el de dictado de Apple, no) y es el más rápido. Reuniones además pide el
    // tiempo de cada frase para ordenar las dos pistas.
    let segmentsWanted = args.mode == "meeting"
    let transcriber = SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: [],
        attributeOptions: segmentsWanted ? [.audioTimeRange] : []
    )
    let module: any SpeechModule = transcriber
    let results = transcriber.results

    do {
        try await ensureInstalled([module], quiet: true)
    } catch {
        fail("No se pudo bajar el modelo de voz: \(error.localizedDescription)")
    }

    let analyzer = SpeechAnalyzer(modules: [module])
    if !args.context.isEmpty {
        let context = AnalysisContext()
        context.contextualStrings[.general] = args.context
        try? await analyzer.setContext(context)
    }
    let collect = Task { () throws -> [(String, CMTimeRange?)] in
        var all: [(String, CMTimeRange?)] = []
        for try await result in results { all.append((String(result.text.characters), result.range)) }
        return all
    }
    do {
        if let last = try await analyzer.analyzeSequence(from: file) {
            try await analyzer.finalizeAndFinish(through: last)
        } else {
            await analyzer.cancelAndFinishNow()
        }
        let parts = try await collect.value
        let text = parts.map(\.0).joined().trimmingCharacters(in: .whitespacesAndNewlines)
        var out: [String: Any] = ["text": text, "locale": locale.identifier(.bcp47)]
        if segmentsWanted {
            out["segments"] = parts.compactMap { part -> [String: Any]? in
                let words = part.0.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !words.isEmpty else { return nil }
                let range = part.1
                return [
                    "start_ms": range.map { milliseconds($0.start) } ?? 0,
                    "end_ms": range.map { milliseconds(CMTimeRangeGetEnd($0)) } ?? 0,
                    "text": words,
                ]
            }
        }
        emit(out)
    } catch {
        fail("No se pudo transcribir: \(error.localizedDescription)")
    }
}

/// Lo que llega por el stdin del comando `stream`.
enum Incoming: Sendable {
    /// La tasa de muestreo, al comienzo.
    case rate(Double)
    /// Un trozo de PCM f32 little-endian mono (puede cortar una muestra a medias).
    case pcm([UInt8])
}

/// Lee el stdin en un hilo propio (`read` bloquea) desde el primer momento,
/// aunque el modelo todavía esté cargando: lo leído espera en la cola (sin
/// límite) y el pipe no se llena, así la pill nunca bota audio por esperar.
///
/// Formato de entrada: 4 bytes con la tasa de muestreo (u32 little-endian) y
/// después PCM f32 little-endian mono, sin más cabecera. Así la pill puede abrir
/// el ayudante antes de saber a qué tasa graba el micrófono.
func readStdin(into continuation: AsyncStream<Incoming>.Continuation) {
    defer { continuation.finish() }
    var header = [UInt8](repeating: 0, count: 4)
    var got = 0
    while got < 4 {
        let n = header.withUnsafeMutableBytes { read(0, $0.baseAddress! + got, 4 - got) }
        if n <= 0 { return }
        got += n
    }
    continuation.yield(.rate(Double(UInt32(header[0]) | UInt32(header[1]) << 8 | UInt32(header[2]) << 16 | UInt32(header[3]) << 24)))
    var bytes = [UInt8](repeating: 0, count: 64 * 1024)
    while true {
        // `read` devuelve lo que haya, sin esperar a llenar el búfer.
        let n = bytes.withUnsafeMutableBytes { read(0, $0.baseAddress!, $0.count) }
        if n <= 0 { return }
        continuation.yield(.pcm(Array(bytes[0..<n])))
    }
}

/// Lleva el PCM de la pill al formato que pide el analizador.
final class PCMConverter {
    let source: AVAudioFormat
    let target: AVAudioFormat
    let converter: AVAudioConverter?
    var pending: [UInt8] = []

    init?(rate: Double, target: AVAudioFormat) {
        guard rate >= 8000, rate <= 384_000,
              let source = AVAudioFormat(commonFormat: .pcmFormatFloat32, sampleRate: rate, channels: 1, interleaved: false)
        else { return nil }
        self.source = source
        self.target = target
        if source == target {
            converter = nil
        } else {
            guard let converter = AVAudioConverter(from: source, to: target) else { return nil }
            self.converter = converter
        }
    }

    func feed(_ bytes: [UInt8]) -> AVAudioPCMBuffer? {
        pending.append(contentsOf: bytes)
        let frames = pending.count / 4
        guard frames > 0, let input = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: AVAudioFrameCount(frames)) else { return nil }
        input.frameLength = AVAudioFrameCount(frames)
        pending.withUnsafeBytes { raw in
            input.floatChannelData![0].update(from: raw.bindMemory(to: Float.self).baseAddress!, count: frames)
        }
        pending.removeFirst(frames * 4)
        return pass(input, end: false)
    }

    /// Al soltar el atajo la última palabra puede quedar al borde: 300 ms de
    /// silencio la empujan a través del conversor (que retiene unas muestras al
    /// remuestrear; sin esto se perdía el final) y del reconocimiento.
    func flush() -> [AVAudioPCMBuffer] {
        var out: [AVAudioPCMBuffer] = []
        if let silence = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: AVAudioFrameCount(source.sampleRate * 0.3)) {
            silence.frameLength = silence.frameCapacity
            silence.floatChannelData![0].update(repeating: 0, count: Int(silence.frameLength))
            if let buffer = pass(silence, end: false) { out.append(buffer) }
        }
        if converter != nil, let empty = AVAudioPCMBuffer(pcmFormat: source, frameCapacity: 1),
           let buffer = pass(empty, end: true) {
            out.append(buffer)
        }
        return out
    }

    func pass(_ input: AVAudioPCMBuffer, end: Bool) -> AVAudioPCMBuffer? {
        guard let converter else { return input }
        let capacity = AVAudioFrameCount(Double(input.frameLength) * target.sampleRate / source.sampleRate) + 1024
        guard let out = AVAudioPCMBuffer(pcmFormat: target, frameCapacity: capacity) else { return nil }
        var fed = false
        var error: NSError?
        let status = converter.convert(to: out, error: &error) { _, state in
            if fed || end {
                state.pointee = end ? .endOfStream : .noDataNow
                return nil
            }
            fed = true
            state.pointee = .haveData
            return input
        }
        if status == .error || out.frameLength == 0 { return nil }
        return out
    }
}

/// Dictado en vivo: transcribe el audio que llega por el stdin mientras llega.
///
/// Salida, un JSON por línea:
/// - `{"ready": true, ...}` cuando el modelo está cargado.
/// - `{"final": "...", "volatile": "..."}` en cada resultado: lo confirmado
///   acumulado y lo provisorio, que todavía puede cambiar.
/// - `{"text": "...", "locale": "..."}` al cerrarse el stdin, con todo el texto.
func stream(_ args: Args) async {
    // El stdin se empieza a leer antes de cargar nada.
    let (incoming, incomingContinuation) = AsyncStream<Incoming>.makeStream()
    let reader = Thread { readStdin(into: incomingContinuation) }
    reader.name = "stdin"
    reader.start()

    guard SpeechTranscriber.isAvailable else { fail("Este Mac no tiene reconocimiento de voz en el equipo.") }
    guard let locale = await resolveLocale(args.lang) else { fail("Este idioma no tiene reconocimiento de voz en el Mac.") }
    let transcriber = SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: [.volatileResults, .fastResults],
        attributeOptions: []
    )
    let module: any SpeechModule = transcriber
    let results = transcriber.results
    do {
        try await ensureInstalled([module], quiet: true)
    } catch {
        fail("No se pudo bajar el modelo de voz: \(error.localizedDescription)")
    }
    guard let format = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [module]) else {
        fail("El reconocimiento de voz no acepta audio en este Mac.")
    }
    let analyzer = SpeechAnalyzer(modules: [module])
    if !args.context.isEmpty {
        let context = AnalysisContext()
        context.contextualStrings[.general] = args.context
        try? await analyzer.setContext(context)
    }
    do {
        try await analyzer.prepareToAnalyze(in: format)
    } catch {
        fail("No se pudo preparar el reconocimiento de voz: \(error.localizedDescription)")
    }

    let collect = Task { () throws -> String in
        var confirmed = ""
        for try await result in results {
            let text = String(result.text.characters)
            if result.isFinal {
                confirmed += text
                emit(["final": confirmed, "volatile": ""])
            } else {
                emit(["final": confirmed, "volatile": text])
            }
        }
        return confirmed
    }

    let (inputs, inputContinuation) = AsyncStream<AnalyzerInput>.makeStream()
    emit(["ready": true, "locale": locale.identifier(.bcp47), "rate": format.sampleRate])
    do {
        try await analyzer.start(inputSequence: inputs)
        var converter: PCMConverter?
        for await chunk in incoming {
            switch chunk {
            case .rate(let rate):
                converter = PCMConverter(rate: rate, target: format)
                if converter == nil { emit(["warning": "no se puede convertir el audio a \(rate) Hz"]) }
            case .pcm(let bytes):
                if let buffer = converter?.feed(bytes) { inputContinuation.yield(AnalyzerInput(buffer: buffer)) }
            }
        }
        for buffer in converter?.flush() ?? [] { inputContinuation.yield(AnalyzerInput(buffer: buffer)) }
        inputContinuation.finish()
        try await analyzer.finalizeAndFinishThroughEndOfInput()
        let text = try await collect.value.trimmingCharacters(in: .whitespacesAndNewlines)
        emit(["text": text, "locale": locale.identifier(.bcp47)])
    } catch {
        fail("No se pudo transcribir: \(error.localizedDescription)")
    }
}

/// Lo que se le pide al modelo: arreglar, no reescribir. Un modelo chico tiende a
/// contestar lo que lee, así que se le insiste en que el texto no es para él.
let polishInstructions = """
Eres un corrector de dictado. Recibes un texto transcrito de voz y devuelves el \
mismo texto corregido: puntuación, tildes, mayúsculas y signos de pregunta. Quita \
muletillas («eh», «este», «o sea» al inicio) y repeticiones involuntarias. \
Conserva cada palabra tal como se dijo: no uses sinónimos, no cambies el orden, \
no resumas, no agregues nada y no traduzcas. Si el texto es \
una pregunta o una orden, NO la respondas: corrígela. Devuelve solo el texto \
corregido, sin comillas ni comentarios.
"""

func polish(_ args: Args) async {
    let input = String(data: FileHandle.standardInput.readDataToEndOfFile(), encoding: .utf8)?
        .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    guard !input.isEmpty else { fail("No hay texto que pulir.") }
    let (state, _) = intelligenceState()
    guard state == "available" else { fail("Apple Intelligence no está disponible (\(state)).") }
    do {
        let session = LanguageModelSession(instructions: polishInstructions)
        let response = try await session.respond(to: input, options: GenerationOptions(temperature: 0.1))
        var text = response.content.trimmingCharacters(in: .whitespacesAndNewlines)
        if text.hasPrefix("«") && text.hasSuffix("»") || text.hasPrefix("\"") && text.hasSuffix("\"") {
            text = String(text.dropFirst().dropLast())
        }
        emit(["text": text])
    } catch {
        fail("Apple Intelligence no pudo pulir el texto: \(error.localizedDescription)")
    }
}

@main
struct AticApple {
    static func main() async {
        let args = Args(CommandLine.arguments)
        switch args.command {
        case "status": await status(args)
        case "install": await install(args)
        case "transcribe": await transcribe(args)
        case "polish": await polish(args)
        case "stream": await stream(args)
        default: fail("Uso: atic-apple status|install|transcribe|stream|polish")
        }
    }
}
