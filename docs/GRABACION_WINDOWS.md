# Grabación de pantalla en Windows: plan para dejarla tan liviana como la nativa

Estado: propuesta, sin implementar (la barra Captura / Grabar de la sección 3
ya está hecha y es común a los dos sistemas). Está pensada para que la evalúes antes de
tocar código. Nada de esto se compiló ni se probó.

Archivos que leí: `apps/atic/src/platform/win32/screen_record.rs` (812
líneas), `apps/atic/src/screen_audio.rs`, `apps/atic/src/screen_recording.rs`
y `apps/atic/src/capture.rs`.

## 1. Cómo funciona hoy y dónde está el costo

### Flujo

1. `capture.rs` (la mira, modo grabar) emite `CaptureEvent::Record { region,
   options }` tras la cuenta de tres (`capture.rs:935`). La pill lo recibe en
   `capture.rs:1880` y llama a `start_screen_recording`
   (`screen_recording.rs:107`), que excluye la ventana de la pill de la
   captura con `SetWindowDisplayAffinity` (`win.rs:203`) y llama a
   `screen_record::start` (`screen_record.rs:121`).
2. `start` lanza un hilo "grabar pantalla" con Media Foundation iniciado
   (`screen_record.rs:136`, `:168`). `record` (`:195`) abre el audio
   (`ScreenAudio::start`, `:206`), la captura (`Capture::open`, `:317`) y el
   codificador (`Encoder::create`, `:488`).
3. `Capture::open` crea un `ID3D11Device` con solo
   `D3D11_CREATE_DEVICE_BGRA_SUPPORT` (`:353`), un
   `Direct3D11CaptureFramePool::CreateFreeThreaded` de 2 cuadros en
   `B8G8R8A8UIntNormalized` (`:374`), activa cursor y desactiva borde
   (`:382-384`) y crea **una textura staging** `D3D11_USAGE_STAGING` con
   `CPU_ACCESS_READ` del tamaño de la zona (`:386-400`).
4. El bucle (`:235-252`) sondea con `TryGetNextFrame` (`grab`, `:416`), se
   queda con el último cuadro y descarta los anteriores, y duerme con
   `thread::sleep` hasta el próximo tick de 30 fps (`:244-251`).
5. Por cada cuadro, `grab` hace `CopySubresourceRegion` a la staging, `Map`
   (`:434-436`), `bgra_to_nv12` en la CPU sobre el mapeo (`:440`, definida en
   `:699`) y, solo el primer cuadro, copia la miniatura BGRA para el estante
   (`:441-445`).
6. `Encoder::push` (`:551`) crea una `IMFSample` con `MFCreateMemoryBuffer` y
   **copia otra vez** los ~3 MB del NV12 (`memory_sample`, `:679`). Retiene el
   cuadro hasta que llega el siguiente para saber su duración (`pending`,
   `:469`, `:555-558`). El Sink Writer se crea con
   `MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS` pero **sin** device manager
   (`:502-510`), así que el MFT de hardware recibe memoria de sistema y sube
   cada cuadro a la GPU por su cuenta.
7. Con audio, `screen_audio.rs` escribe `mic.wav`/`system.wav` y una mezcla
   alineada con `RecordClock` (pausas incluidas, `screen_audio.rs:33-76`).
   Al parar, `mux` (`:596`) relee el MP4 de video temporal con un
   `IMFSourceReader`, escribe el video sin recodificar y codifica el audio PCM
   a AAC en un MP4 nuevo, intercalando por tiempo (`:631-658`). Luego
   `move_file`.

### Dónde está el costo (por orden de peso estimado, a confirmar midiendo)

| Costo | Dónde | Magnitud aproximada a 1080p30 |
| --- | --- | --- |
| Lectura GPU→CPU (`Map` de staging, bloquea hasta que la GPU termina la copia) | `:434-436` | ~8 MB/cuadro, ~250 MB/s, más una sincronización GPU/CPU por cuadro |
| `bgra_to_nv12` en la CPU | `:440`, `:699` | ~2 M píxeles/cuadro por un núcleo; en 4K es 4 veces más |
| Copia a `IMFMediaBuffer` | `:679-691` | ~3 MB/cuadro en 1080p, alloc + memcpy |
| Subida a la GPU por el MFT de hardware | implícita | de nuevo ~3 MB/cuadro |
| `sleep` y sondeo | `:244-251` | cuadros que llegan justo después de un tick esperan hasta 33 ms: jitter de duración y latencia, y despierta el hilo aunque no cambie nada |
| `mux` final | `:596-661` | relee y reescribe todo el archivo; tarda segundos en grabaciones largas y duplica el disco temporal |
| Doble disco | `video.mp4` + `mic.wav` + `system.wav` + mezcla + MP4 final | I/O extra durante y después |

El resultado: el trabajo pesado de pixeles (conversión de color y copias)
corre en la CPU y viaja dos veces por el bus PCIe. La grabadora nativa
mantiene todo en memoria de video: captura, conversión y codificación ocurren
en la GPU y la CPU solo pasa punteros.

## 2. Propuesta: todo en la GPU

Objetivo: el cuadro nunca toca la CPU. La cadena queda:

```
WGC (textura BGRA del monitor)
  -> CopySubresourceRegion / VideoProcessorBlt (recorte + color)  [GPU]
  -> textura NV12 de un pool                                      [GPU]
  -> IMFSample con IMFDXGIBuffer                                  [sin copia]
  -> Sink Writer con IMFDXGIDeviceManager -> MFT H.264 de HW       [GPU]
```

### 2.1 Dispositivo y administrador DXGI

- Crear el `ID3D11Device` con `D3D11_CREATE_DEVICE_BGRA_SUPPORT |
  D3D11_CREATE_DEVICE_VIDEO_SUPPORT` (hoy solo BGRA, `:357`). Conviene pasar
  el adaptador explícito del monitor capturado (enumerar con
  `IDXGIFactory1::EnumAdapters1` y comparar con la salida del monitor) en vez
  de `None`; en portátiles con dos GPU, WGC, el conversor y el codificador
  tienen que vivir en la misma, o aparecen copias entre adaptadores.
- Proteger el contexto contra uso concurrente: `device.cast::<ID3D10Multithread>()`
  y `SetMultithreadProtected(true)`. Es obligatorio: el MFT del codificador y
  nuestro hilo usan el mismo dispositivo.
- `MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)` y
  `manager.ResetDevice(&device, reset_token)`. El `ID3D11Device` que se le da
  debe ser **el mismo** que el del frame pool y el conversor.
- En `with_hardware` (`:502`), además de
  `MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS`, escribir
  `attributes.SetUnknown(&MF_SINK_WRITER_D3D_MANAGER, &manager)`. Opcional:
  `MF_LOW_LATENCY = 1` y `MF_SINK_WRITER_DISABLE_THROTTLING = 1` (el segundo
  evita que el writer frene al productor de video mientras espera otras
  pistas; ver 2.6).
- Con el manager presente, el encoder de hardware acepta texturas NV12 sin
  tocar la CPU. Si no hay MFT de hardware (VM, RDP, GPU sin encoder), el
  fallback actual `with_hardware(.., false)` (`:493-498`) debe seguir
  existiendo y debe usar el camino de memoria de hoy, porque el MFT de
  software no entiende `IMFDXGIBuffer`. Conservar `bgra_to_nv12` y sus tests
  (`:786-812`) como fallback.

### 2.2 Recorte y conversión BGRA->NV12 en la GPU

Dos alternativas; se recomienda implementar la A y dejar la B como prueba
comparativa.

**A. `ID3D11VideoProcessor` (conversión explícita)**

- `device.cast::<ID3D11VideoDevice>()` y `context.cast::<ID3D11VideoContext>()`.
- `CreateVideoProcessorEnumerator` con `D3D11_VIDEO_PROCESSOR_CONTENT_DESC`
  (entrada = tamaño del monitor, salida = tamaño de la zona, `InputFrameFormat
  = PROGRESSIVE`, 30/1), luego `CreateVideoProcessor`. Comprobar con
  `CheckVideoProcessorFormat` que soporte `DXGI_FORMAT_B8G8R8A8_UNORM` de
  entrada y `DXGI_FORMAT_NV12` de salida.
- Por cuadro: `CreateVideoProcessorInputView(texture_wgc, ..)` (puede cachearse
  por textura del frame pool, son pocas), `CreateVideoProcessorOutputView(nv12,
  ..)` y `VideoProcessorBlt` con un `D3D11_VIDEO_PROCESSOR_STREAM` cuyo
  `SourceRect` es la zona (reemplaza el `D3D11_BOX` de `:341`) y
  `VideoProcessorSetOutputTargetRect` / `SetStreamDestRect` en 0,0 con el
  tamaño de salida. Así el recorte y la conversión son **un solo paso** y no
  hace falta el `CopySubresourceRegion`.
- Espacio de color explícito, para no depender de lo que el driver adivine:
  `VideoProcessorSetStreamColorSpace` con
  `D3D11_VIDEO_PROCESSOR_COLOR_SPACE { Usage: 0, RGB_Range: 0 (0-255),
  YCbCr_Matrix: 1 (BT.709), YCbCr_xvYCC: 0, Nominal_Range: 0 }` para la
  entrada RGB, y `VideoProcessorSetOutputColorSpace` con `RGB_Range: 0`,
  `YCbCr_Matrix: 1 (BT.709)`, `Nominal_Range: 1 (16-235)` para la salida. Eso
  coincide con lo que hoy declara el tipo de entrada
  (`MF_MT_YUV_MATRIX = BT709`, `MF_MT_VIDEO_NOMINAL_RANGE = 16_235`,
  `:534-537`) y con lo que hace `bgra_to_nv12`; el test
  `blanco_y_negro_en_rango_limitado` (`:786`) fija esos valores como
  referencia para comparar.
- La textura de salida NV12 se crea con `BindFlags = D3D11_BIND_RENDER_TARGET`
  (algunos drivers lo exigen para output views) y a veces
  `D3D11_BIND_SHADER_RESOURCE`; MiscFlags `0`. Como el encoder la lee, probar
  también `D3D11_RESOURCE_MISC_SHARED` si algún driver lo pide (rara vez).
- Ancho y alto pares, igual que hoy (`:336`). Algunos encoders de hardware
  exigen además alineación a 16 en la altura interna; MF lo maneja con
  recorte de apertura, pero hay que probar zonas de 801x601 como en
  `graba_la_pantalla`.
- El `VideoProcessorBlt` escala con filtro bilineal cuando origen y destino
  difieren, pero aquí son iguales (recorte 1:1), así que no debería haber
  remuestreo. Verificarlo comparando el cuadro con el original.

**B. Entregar ARGB32/RGB32 al encoder y que MF inserte el conversor**

- Cambiar `MFVideoFormat_NV12` por `MFVideoFormat_ARGB32` (o `RGB32`) en el
  `input` (`:527`) y mantener el recorte con `CopySubresourceRegion` a una
  textura de pool `D3D11_USAGE_DEFAULT` con `BIND_SHADER_RESOURCE`
  (`:341`/`:434`, pero a textura GPU, no staging).
- El Sink Writer inserta el conversor de color (MFT "Video Processor", que usa
  la GPU si hay device manager). Menos código, pero: menos control del espacio
  de color (hay que declarar `MF_MT_YUV_MATRIX` en el tipo de salida del
  conversor; con RGB de entrada el writer decide), y en algunos drivers el
  conversor corre en la CPU o hace un viaje extra.
- Se debe medir A contra B con el mismo clip (ver sección 5). Si B da ~lo
  mismo, es preferible por simplicidad y menos superficie de error.

### 2.3 Pool de texturas y `IMFDXGIBuffer`

- `MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &texture, 0, false)` envuelve
  la textura en un `IMFMediaBuffer`; luego `MFCreateSample` + `AddBuffer` y
  `SetSampleTime`/`SetSampleDuration` como hoy. `SetCurrentLength` se calcula
  con `buffer.GetMaxLength()` o `MFGetPlaneSize`; para NV12 en DXGI basta con
  `MFCreate2DMediaBuffer`-estilo longitud = ancho * alto * 3 / 2.
- Riesgo principal: **reusar una textura mientras el encoder todavía la lee**.
  Hoy `Encoder::push` retiene un cuadro (`pending`, `:469`), y el encoder
  asíncrono puede tener varios en vuelo. Dos estrategias:
  1. **Rotación de N texturas** (N = 6 a 8 como mínimo: 1 en `pending`, 2-3 en
     el encoder, 1 en conversión y margen). Simple, pero N fijo puede fallar
     si el encoder se atrasa; no hay señal de que terminó.
  2. **`IMFTrackedSample`** (`MFCreateTrackedSample`): el sample se devuelve al
     pool cuando el encoder lo libera, vía `SetAllocator(callback, state)` con
     un `IMFAsyncCallback` que marca la textura como libre. Es lo correcto y
     evita corrupción visual, a cambio de implementar un `IMFAsyncCallback` en
     Rust (con `#[implement(IMFAsyncCallback)]` de `windows-core`). Si el pool
     se agota, se descarta el cuadro nuevo en vez de bloquear: preferible a
     sobrescribir una textura en uso.
- Recomendación: empezar con la rotación de 8 y descarte si el siguiente no
  está libre medido por un contador de muestras escritas vs. liberadas;
  pasar a `IMFTrackedSample` si se ven artefactos o si hay que subir N en 4K.
- Para la miniatura del primer cuadro (`thumb`, `:441-445`) ya no hay CPU
  mapeada: copiar una vez la zona a una staging pequeña solo para ese cuadro
  (`Map` una sola vez) o reusar la captura de la mira, que ya tiene el
  `Frame` congelado (`capture.rs`, `session.frozen`). La segunda es gratis y
  evita código GPU->CPU; revisar que el recorte coincida con la zona.
- La duración del cuadro: el esquema "se escribe cuando llega el siguiente"
  (`:555-558`) retiene una textura del pool todo el tiempo entre cuadros; en
  pantallas quietas puede ser segundos. Debe contarse como una textura en uso
  permanente (el pool nunca baja de N-1 libres) o cambiarse a duración fija +
  cuadros repetidos. Ver 2.5.

### 2.4 Opciones de `GraphicsCaptureSession` y cuadros por evento

- `IsCursorCaptureEnabled`: ya está en `true` (`:382`). Se puede exponer como
  opción (no es necesaria ahora).
- `IsBorderRequired = false`: hoy se intenta sin más (`:384`), y en Windows 11
  cuando la app no tiene el permiso, el borde amarillo igual sale o la llamada
  falla en silencio. El camino correcto en Win11 (build 22000+):
  `GraphicsCaptureAccess::RequestAccessAsync(GraphicsCaptureAccessKind::Borderless)`
  (espacio `Windows.Graphics.Capture`, feature `Graphics_Capture`, más
  `Foundation` ya incluida). Hay que llamarlo una vez, antes de `StartCapture`,
  y esperar el `IAsyncOperation`. Para apps sin identidad de paquete puede
  devolver `DeniedByUser` o `Allowed` según la política; hay que registrar
  la respuesta en el log. Plan B: dejar el borde si no se concede.
- `MinUpdateInterval` (`GraphicsCaptureSession::SetMinUpdateInterval`,
  Windows 11 24H2, build 26100+): fijarlo a `1/FPS` (`TimeSpan` de 333 333 de
  100 ns) hace que el sistema entregue como máximo 30 cuadros/s y **elimina el
  `sleep`** (`:244-251`). Si la API no existe (build anterior), hay que
  detectarla con `ApiInformation::IsPropertyPresent` (necesita
  `ApplicationModel`... o simplemente tolerar el error de `SetMinUpdateInterval`)
  y caer al limitador por reloj, pero **dentro del manejador de eventos**, no
  durmiendo el hilo: descartar el cuadro si `now - last < tick - margen`.
- Cuadros por evento: `Direct3D11CaptureFramePool` ya se crea con
  `CreateFreeThreaded` (`:374`), que permite `FrameArrived` en un hilo del
  pool de hilos sin dispatcher. Registrar `pool.FrameArrived(&TypedEventHandler)`
  y en el manejador hacer `TryGetNextFrame`, leer
  `frame.SystemRelativeTime()` (el sello del compositor) y encolar la
  conversión. Ventajas: sin sondeo, sin jitter de 0-33 ms y sin hilo
  despertando con la pantalla quieta (la CPU baja a casi 0 cuando nada se
  mueve).
- Usar el tiempo de WGC (`SystemRelativeTime`, en `TimeSpan` relativo al reloj
  de QPC) para los sellos en vez de `Instant::now()`: es el instante real del
  cuadro y da duraciones exactas. Hay que mapearlo al origen de
  `RecordClock` (guardar el `SystemRelativeTime` del primer cuadro y restar).
  El audio se alinea con `Instant` (`screen_audio.rs`), así que la diferencia
  entre el reloj de WGC y el de `Instant` al inicio debe medirse una vez
  (offset). Probar con el tono de `ATIC_REC_TONE` (ver sección 5).
- Tamaño del pool: con eventos conviene `numberOfBuffers = 3` o 4 (hoy 2,
  `:377`) para no perder cuadros mientras se convierte; los cuadros del pool
  se devuelven con `Close()` apenas termina el `VideoProcessorBlt` (la GPU
  seguirá leyéndolos en orden por la cola del dispositivo, pero hay que
  llamar `Flush` o confiar en que WGC espera un `Fence`; probar).
- Redimensionado del monitor o cambio de modo: manejar
  `pool.Recreate(...)` si `frame.ContentSize()` cambia, y que el
  `item.Closed` termine la grabación con un mensaje, no con un cuadro negro.

### 2.5 Tiempos y pausa

- La pausa ya funciona con `RecordClock::now()` (`screen_audio.rs:69-76`):
  devuelve `None` en pausa y resta lo pausado. Se mantiene tal cual. Con
  eventos, el manejador consulta `clock.now()` (o convierte
  `SystemRelativeTime` y le resta `pause.total`) y descarta si es `None`.
  Pausa y cuadros: los cuadros llegados durante la pausa deben soltarse con
  `Close()` sin convertir (hoy se leen y convierten para "no juntarlos",
  `:237-243`; con eventos se evita incluso la conversión, ganancia directa).
- Pantalla quieta: WGC no entrega cuadros si no hay cambios. El esquema de
  "duración hasta el próximo" (`pending`) funciona para MP4 de cuadros
  variables, pero un reproductor puede mostrar saltos al buscar. Opción
  conservadora: si pasan más de ~1 s sin cuadro, reescribir el último cuadro
  una vez cada segundo (un `VideoProcessorBlt` no hace falta: se reusa la
  misma textura NV12, que debe seguir retenida). Medir tamaño y sensación en
  el reproductor antes de decidir.
- El último cuadro dura hasta el stop (`finish`, `:572-575`); no cambia.

### 2.6 Audio: segunda pista en el mismo Sink Writer (opcional) y el `mux`

Hoy el audio se graba a WAV temporal y se junta al final (`mux`, `:596`).
Alternativa: abrir el Sink Writer una sola vez con dos streams (H.264 + AAC) y
escribir el audio por bloques de 100 ms mientras graba, como ya hace `mux`
(`:633-655`, entrada PCM 16-bit estéreo a `RATE`, salida AAC 24 000 B/s). Se
eliminaría el segundo archivo, la relectura y la espera al terminar.

Cuidados:

- **Intercalado.** El Sink Writer de MP4 (`MPEG4 sink`) acumula las muestras
  de una pista si la otra va atrasada y, con
  `MF_SINK_WRITER_DISABLE_THROTTLING` en 0 (valor por defecto), **frena
  `WriteSample` de la pista adelantada** hasta que la otra alcanza. Si el
  audio llega a ritmo de reloj real (bloques cada 100 ms) y el video llega
  solo cuando la pantalla cambia, el video puede quedar "atrasado" mucho
  tiempo y el audio se bloquea, llenando la cola de `atic-audio` (que descarta
  bloques si se atrasa, `screen_audio.rs:117-118`). Eso es corrupción del
  audio, no solo retraso.
- **`SendStreamTick(stream, timestamp)`**: indica al writer que en esa pista
  no habrá muestras hasta `timestamp`. Hay que llamarlo para la pista de video
  cuando pasa un tiempo sin cuadros (pantalla quieta), con el sello del audio
  actual, y alternativamente para audio durante silencios si se deja de
  entregar (no es el caso: el audio llega siempre, incluso en silencio, pero
  conviene el tick por seguridad con la pausa).
- **Pausa.** Con `RecordClock`, durante la pausa ni video ni audio avanzan en
  el tiempo grabado, así que ambas pistas deben detenerse a la vez; los ticks
  deben contar el tiempo ya descontado.
- **Hilos.** `IMFSinkWriter` es seguro entre hilos para `WriteSample` de
  streams distintos, pero `Finalize` debe esperar a que ambos productores
  terminen. Hoy el audio se cierra después del video (`:263`); aquí habría
  que parar ambos y recién entonces `Finalize`.
- **Fallo del audio.** Hoy, si el audio falla, el video queda intacto
  (`:277-280`). Con una sola escritura, un fallo del AAC puede perder todo el
  archivo; hay que mantener un camino de "solo video" cuando `ScreenAudio`
  falla al arrancar (ya existe) y proteger contra errores a mitad de camino
  (descartar solo la pista de audio con `SetInputMediaType`... no se puede
  quitar un stream en curso; habría que seguir escribiendo silencio).
- **Costo real del mux hoy.** Es copia de bytes comprimidos (video) más
  codificación AAC de audio: un MP4 de 10 minutos a ~8 Mbps pesa ~600 MB y
  la relectura/reescritura toma varios segundos de disco, **no de CPU
  notable**. No afecta la fluidez durante la grabación, solo la espera al
  terminar y el disco.

Recomendación: **mantener el `mux` en la primera iteración**. El mayor
beneficio (CPU/GPU durante la grabación) viene de 2.1-2.4; el audio en el
mismo writer añade riesgo de sincronía y de bloqueo sin mejorar la fluidez.
Evaluarlo como paso posterior y solo si la espera al terminar molesta; en
ese caso, mejor optimizar `mux` (escribir con `MF_SINK_WRITER_DISABLE_THROTTLING`
y bloques más grandes) o hacer el audio AAC en vivo hacia su propio archivo
`.m4a` y solo remuxar (aún copia de contenedor, pero sin recodificar nada).

### 2.7 HDR

- Con un monitor en modo HDR (`Usar HDR` activado en Windows), WGC entrega
  cuadros en `R16G16B16A16Float` (scRGB lineal) si se pide ese formato, y con
  `B8G8R8A8UIntNormalized` hace tone mapping a SDR por su cuenta en algunos
  builds, pero **no es lo que se debe asumir**: el contenido puede verse
  lavado o sobreexpuesto.
- Manejo propuesto: detectar HDR por monitor con
  `IDXGIOutput6::GetDesc1` (`ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020`).
  Si es HDR: pedir `R16G16B16A16Float` al frame pool, convertir a SDR BT.709
  con un pase de sombreado (tone mapping simple, p. ej. Reinhard con
  blanco de referencia a 203 nits, como hace el SDR-white-level del
  sistema) a una textura BGRA intermedia, y luego ir al `VideoProcessor`
  normal. Eso requiere un pixel shader, es el trabajo más caro del plan.
- Fase mínima aceptable: **detectar HDR, registrar una advertencia en el log
  y seguir con `B8G8R8A8`** (el sistema hace su propia conversión). Hoy no hay
  ningún manejo de HDR en el archivo; este paso solo lo hace explícito.
- El `ID3D11VideoProcessor` puede hacer HDR->SDR si se le declara el espacio
  de color de entrada (`VideoProcessorSetStreamColorSpace1` con
  `DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709` para scRGB y la salida
  `DXGI_COLOR_SPACE_YCBCR_STUDIO_G22_LEFT_P709`), pero el soporte de tone
  mapping depende del driver. Probar antes de escribir un shader propio.

### 2.8 Por qué WGC + Media Foundation es el equivalente real

- `Windows.Media.AppRecording` (la API detrás de la Game Bar) solo graba **la
  propia app** que la usa (y requiere que esté en primer plano o sea juego
  reconocido): no sirve para grabar el escritorio completo ni una zona.
- La Recortes (Snipping Tool) usa componentes internos de Windows y **no tiene
  API pública**. Lo que expone el sistema a terceros para grabar monitores o
  ventanas con la misma canalización es Windows.Graphics.Capture, y
  Media Foundation para codificar con el encoder de la GPU. Ese par es el
  equivalente real, y es lo que usan OBS (en su captura WGC) y ShareX.
- La diferencia con la nativa hoy no es la API de captura sino que mantenemos
  los cuadros en la CPU; con 2.1-2.4 se cierra casi toda.

## 3. La barra de la mira (selector Captura / Grabar)

La barra de la mira ya existe y es GPUI, común a macOS y Windows
(`CaptureView::toolbar` en `capture.rs`): abajo al centro de la pantalla del
cursor, con el selector **Captura / Grabar** (también la tecla **R**) y, según
el modo, Pizarra y Texto, o Micrófono, Sonido y Pill (`RecordOptions` en
`screen_recording.rs`, las mismas M, A y P). Se esconde mientras se arrastra.
`screen_record::start` recibe ahora `show_pill`: en macOS excluye las ventanas
de Atic del filtro de ScreenCaptureKit; en Windows no se usa (lo hace
`WDA_EXCLUDEFROMCAPTURE`). No requiere cambios para este plan, pero en Windows
hay que revisar:

- **Que la barra se vea y responda.** Clic en cada botón sin que empiece una
  selección, hover, y que la barra quede en el monitor del cursor con DPI
  mixto (se ubica con `cursor_screen`, como la ayuda de arriba).

- **Exclusión de la captura.** La ventana de la mira y la pill se ocultan del
  video con `WDA_EXCLUDEFROMCAPTURE` (`win.rs:203-215`, `glass.rs:221`). Hoy
  `start_screen_recording` la activa antes de `start` y la quita si falla
  (`screen_recording.rs:112-114`, `:136-140`). Comprobar que con WGC por
  evento sigue excluida desde el primer cuadro (no debe salir un cuadro con la
  barra de la mira) y que se restaura al terminar o descartar. Con
  `IsBorderRequired = false` confirmar que la mira tampoco aparece.
- **Atajos.** El atajo de Capturas se mantiene apretado para entrar a grabar
  (`capture_held`, `screen_recording.rs:99`), `Esc` cancela
  (`capture.rs:Cancel`), `Enter`/Espacio elige pantalla o inicia la cuenta
  (`capture.rs:whole_screen`). Revisar que con la grabación en curso (el mismo
  atajo detiene) no se cuelen pulsaciones a la app grabada y que el atajo
  funciona con la ventana de la mira sin foco.
- **DPI mixto y multi-monitor.** `region` viene en píxeles físicos del
  escritorio virtual y `Capture::open` elige el monitor del centro de la zona
  con `MonitorFromPoint` y recorta lo que se salga (`:318-334`). Con monitores
  de DPI distinto (p. ej. 100% y 150%), probar zonas que cruzan el borde: debe
  quedar la parte del monitor del centro, sin desplazamiento. El
  `VideoProcessorBlt` usa el mismo `SourceRect` en píxeles del monitor; hay que
  comprobar que no se mezclan puntos lógicos con físicos.
- **Zona mínima y lados pares** (`MIN_SIDE`, `:63`, `:336`). Se mantienen.

## 4. Plan por pasos

Cada paso es una unidad compilable y comprobable. Compilar la pill toma 4 a
10 minutos (ver `AGENTS.md`): agrupar los pasos 1-4 en una compilación si el
usuario lo prefiere.

| # | Paso | Riesgo | Cómo comprobar |
| --- | --- | --- | --- |
| 0 | Línea base: medir la grabación actual (ver sección 5) | Ninguno | Tabla con CPU, GPU, fps, tamaño, duración del `mux` |
| 1 | `ID3D11Device` con `VIDEO_SUPPORT`, `ID3D10Multithread`, `IMFDXGIDeviceManager` y `MF_SINK_WRITER_D3D_MANAGER`, **todavía con muestras de memoria** | Bajo: el writer acepta memoria con manager | `graba_la_pantalla` produce el MP4 igual; sin regresión |
| 2 | Pool de texturas NV12 + `VideoProcessorBlt` (opción A) + `MFCreateDXGISurfaceBuffer`; borrar staging, `Map` y `bgra_to_nv12` del camino principal (el fallback lo conserva) | **Alto**: colores, reuso de texturas, drivers | Comparar cuadros contra la versión CPU (franja de grises, texto, rojo/azul puros, el test de rango limitado); GPU de Intel, NVIDIA y AMD si hay acceso |
| 2b | Variante B (ARGB32 al writer) para comparar | Medio | Mismo clip, mismas métricas que 2; elegir la ganadora |
| 3 | `FrameArrived`, `MinUpdateInterval` y sellos de `SystemRelativeTime`; quitar `sleep` y sondeo | Medio: sincronía con el audio y pausa | Pausa de 1,5 s (`ATIC_REC_PAUSE`) y tono (`ATIC_REC_TONE`) |
| 4 | `GraphicsCaptureAccess` Borderless y manejo de `item.Closed` y cambios de tamaño | Bajo | Windows 11: sin borde amarillo; Windows 10: sigue funcionando |
| 5 | HDR: detección y aviso; luego tono mapeado si hace falta | Medio-alto | Monitor HDR si lo hay; si no, solo la rama de detección con un log |
| 6 | Pantalla quieta: cuadro repetido cada ~1 s (si el reproductor da problemas) | Bajo | Grabar 30 s de una pantalla quieta, abrir y buscar en el video |
| 7 | (Opcional) Audio como segunda pista del mismo writer | **Alto**: intercalado y bloqueos | 10 minutos con mic + sistema, pausas, y comparar desfase con el tono |
| 8 | Quitar el camino CPU si el fallback ya no se usa; documentar | Bajo | Revisión del diff |

Reglas de seguridad durante el trabajo:

- Mantener `Encoder` con dos implementaciones (GPU y memoria) y elegir en
  `create` según éxito de `ResetDevice`/`AddStream`; así una máquina sin
  encoder de hardware sigue grabando. Con la GPU, si el primer
  `WriteSample` falla con `MF_E_...` de formato, reintentar en modo memoria.
- No hacer `unwrap` sobre HRESULT de drivers: devolver `Err` con el mensaje y
  que la UI diga "grabando sin aceleración" en el log (`eprintln!("grabar: ...")`,
  como hace hoy en `:496`).
- Cuidado con cerrar el device manager antes de `Finalize`, y con
  `MFShutdown` (ya en `with_media_foundation`, `:168-184`): soltar texturas y
  views antes, y que el `ResetDevice` no se repita.

## 5. Cómo medir y probar

### Línea base y comparación (hacer antes del paso 1 y después de cada paso grande)

Misma máquina, mismo clip: 60 s con un video reproduciéndose a pantalla
completa en la zona y 60 s con el escritorio quieto con un cursor moviéndose.
Resoluciones: 1080p, 1440p y 4K (60 fps con `FPS = 60` temporal en
`screen_record.rs:59`; la tasa de bits sube con `bitrate`, `:482`).

| Métrica | Dónde |
| --- | --- |
| CPU de `pill-gpui.exe` (dev) o `atic-pill.exe` | Administrador de tareas > Detalles; o `typeperf "\Process(pill-gpui)\% Processor Time"` |
| GPU: 3D, Copy, Video Encode, Video Decode | Administrador de tareas > Rendimiento > GPU (cambiar los gráficos a "Video Encode" y "Copy"); y por proceso en Detalles con la columna "Motor de GPU" |
| Ancho de banda PCIe | Se verá como motor Copy alto en la línea base |
| fps reales y duraciones | `ffprobe -show_frames -select_streams v -show_entries frame=pts_time,pkt_duration_time` y comparar el histograma; el log ya imprime `grabar: N cuadros de WxH` (`:580`) |
| Cuadros perdidos | Contar los que cierran por pool lleno (agregar contador al log en el paso 3) |
| Latencia de fin | Tiempo entre `stop()` y `finished()`; con `mux` suma segundos |
| Tamaño y calidad | Tamaño del archivo; comparar cuadro a cuadro con `ffmpeg -ss` y un PSNR/SSIM contra la versión CPU |
| Efecto sobre otras apps | Grabar mientras un juego o video corre: fps del juego con y sin grabar |

Criterio de "tan liviana como la nativa": grabar la Recortes (Win+Shift+R) en
la misma zona y clip, y que nuestra CPU y motor Copy queden en el mismo
orden de magnitud que ella (idealmente <2 % de CPU en 1080p30 con el encoder
de hardware, sin picos de Copy sostenidos).

### Casos que hay que recorrer

- 4K a 60 fps: la tasa de bits tope es 40 Mbps (`:484`); confirmar que no se
  pierden cuadros y que el pool aguanta (N sube).
- Multi-monitor con DPI distinto (por ejemplo 100 % y 150 %): zona en cada
  monitor, zona que cruza el borde (se recorta al monitor del centro),
  monitor secundario a la izquierda del principal (coordenadas negativas),
  cambio del monitor principal.
- HDR activado y desactivado (sección 2.7).
- Portátil con dos GPU: grabar con la app en cada GPU y con el monitor en la
  integrada (el adaptador del device debe ser el de la salida).
- Máquina sin encoder de hardware (VM, sesión RDP): cae al camino de
  memoria y no se rompe.
- Pausa, descarte, parar durante la cuenta y parar con la pantalla quieta.
- Ventanas excluidas: la pill y la mira no salen; con `show_pill` activado,
  la pill sí sale.
- Bloqueo de pantalla (`Win+L`) y cambio de resolución durante la grabación:
  termina con un mensaje, no cuelga.

### Cómo probar en la práctica

- Ver el resultado: `scripts/pill-dev.ps1` (ruta absoluta; compila en dev y
  reabre la pill; cierra la pill de dev de otras sesiones, avisar si pasa).
  Con `-Watch` en la terminal del usuario no se debe compilar aparte.
- Grabación de verdad sin la UI: el test manual `graba_la_pantalla`
  (`screen_record.rs:745`):

  ```powershell
  $env:ATIC_REC_OUT = "$env:TEMP\prueba.mp4"
  cargo test --manifest-path apps/atic/Cargo.toml --target-dir "$env:LOCALAPPDATA\atic-gpui" graba_la_pantalla -- --ignored --nocapture
  ```

  Con `ATIC_REC_AUDIO=1|mic|system` incluye audio; con `ATIC_REC_PAUSE=1`
  graba 1 s, pausa 1,5 s y 1 s más (debe durar 2 s); con
  `ATIC_REC_TONE=<wav>` suena un tono al segundo 1 para medir el desfase en
  el audio. Hay que ampliarlo: duración configurable (`ATIC_REC_SECS`), zona
  configurable (hoy `801x601`, `:756`), y asserts de que el MP4 tiene video y
  audio (con `MFCreateSourceReaderFromURL`).
- Tests unitarios que se mantienen: `blanco_y_negro_en_rango_limitado` y
  `el_rojo_va_en_v_y_se_ignora_el_relleno_de_la_fila` (`:786`, `:799`) siguen
  valiendo mientras exista el fallback CPU; sirven de referencia numérica
  para la conversión en la GPU (se puede leer de vuelta una textura NV12
  pequeña en un test manual y comparar). Para lo visual (colores, borde, tema
  claro/oscuro) mirar la grabación en la app; no inventar tests que copien la
  implementación.
- Log de la pill: `%APPDATA%\ciat\atic\data\logs\pill.<fecha>.log` para los
  `grabar: ...`.
- Verificar siempre qué proceso corre: la pill de dev es `pill-gpui.exe`, la
  instalada `atic-pill.exe`.

## 6. Archivos a tocar y features nuevas del crate `windows`

### Archivos

| Archivo | Cambio |
| --- | --- |
| `apps/atic/src/platform/win32/screen_record.rs` | Casi todo: device con `VIDEO_SUPPORT` y multithread, `IMFDXGIDeviceManager`, `VideoProcessor`, pool de texturas, `FrameArrived`, `MinUpdateInterval`, `Encoder` con muestras DXGI, conservar el camino de memoria, HDR, y (opcional) audio en el writer; el `mux` se queda o se simplifica |
| `apps/atic/src/screen_audio.rs` | Solo si el paso 7 va: exponer los bloques AAC en vivo (hoy `align` escribe a archivos, `:218`) y un reloj compartido; el resto no cambia |
| `apps/atic/src/screen_recording.rs` | Mínimo: opciones nuevas de la grabación si hay (p. ej. cursor), y mensajes de error legibles |
| `apps/atic/src/capture.rs` | Nada obligatorio. Posible: entregar el `Frame` congelado para la miniatura (2.3) |
| `apps/atic/Cargo.toml` | Features de `windows` (abajo) |
| `apps/atic/src/platform/mod.rs` | Solo si cambia el contrato de `screen_record` (hoy `start`, `Recording`, `Saved`, `videos_dir`); no debería |
| `apps/atic/assets/changelog.json` | Una entrada en «Novedades» de la versión siguiente cuando salga (ver `AGENTS.md`): "La grabación de pantalla usa menos CPU" y su par en inglés |
| `docs/GRABACION_WINDOWS.md` | Este documento: actualizarlo con las mediciones |

### Features del crate `windows` (0.61) en `apps/atic/Cargo.toml`

Ya están (sección «Grabar la pantalla», `Cargo.toml:182-257`):
`Graphics`, `Graphics_Capture`, `Graphics_DirectX`,
`Graphics_DirectX_Direct3D11`, `Win32_Graphics_Direct3D`,
`Win32_Graphics_Direct3D11`, `Win32_Graphics_Dxgi`,
`Win32_Graphics_Dxgi_Common`, `Win32_Media_MediaFoundation`,
`Win32_System_WinRT_Direct3D11`, `Win32_System_WinRT_Graphics_Capture`,
`Foundation`, `Win32_System_Com`, `Win32_Graphics_Gdi`, `Win32_UI_Shell`.

Falta (a verificar contra la documentación de `windows-rs` al implementar, el
compilador lo dirá):

| Feature | Para qué |
| --- | --- |
| `Win32_Graphics_Direct3D10` | `ID3D10Multithread` (`SetMultithreadProtected`) |
| `Win32_Graphics_Direct3D11on12` | No hace falta; solo si se usara D3D12, no se propone |
| `Foundation_Collections` | Ya está; `Graphics_Capture` usa `IAsyncOperation` del permiso Borderless |
| `Win32_Security` | Solo si el compilador la pide por los tipos de seguridad de Media Foundation (suele no hacer falta) |

Probablemente no se necesiten más, porque `ID3D11VideoDevice`,
`ID3D11VideoContext`, `ID3D11VideoProcessor*` y `D3D11_VIDEO_PROCESSOR_*`
viven en `Win32_Graphics_Direct3D11`; `IMFDXGIDeviceManager`,
`MFCreateDXGIDeviceManager`, `MFCreateDXGISurfaceBuffer`,
`MF_SINK_WRITER_D3D_MANAGER`, `IMFTrackedSample` y `IMFAsyncCallback` viven
en `Win32_Media_MediaFoundation`; `IDXGIOutput6` en `Win32_Graphics_Dxgi`.
`GraphicsCaptureAccess` está en `Graphics_Capture`, y `TypedEventHandler`
en `Foundation`. `Win32_System_Threading` (ya está) sirve si hace falta
elevar la prioridad del hilo con `AvSetMmThreadCharacteristicsW`, que
además requiere `Win32_System_Threading` pero no una feature nueva (revisar).

Cada feature extra del crate `windows` suma tiempo de compilación; agregar
solo las que el compilador pida.

## 7. Preguntas abiertas para decidir

1. ¿Se acepta mantener el camino CPU como fallback (más código) o se exige
   hardware con encoder y se muestra un error? Recomendación: mantenerlo.
2. ¿Opción A (VideoProcessor) o B (ARGB32)? Recomendación: A, y medir B.
3. ¿Se suma el audio al writer (paso 7)? Recomendación: no, hasta que la
   espera del `mux` sea un problema real.
4. ¿Qué soporte de HDR se exige en esta entrega? Recomendación: detectar y
   avisar; tone mapping en una segunda entrega.
5. ¿Qué mínimo de Windows? `MinUpdateInterval` exige 24H2 y borde sin
   solicitud exige 11; el resto funciona en 10 (1903+ para `CreateFreeThreaded`).
