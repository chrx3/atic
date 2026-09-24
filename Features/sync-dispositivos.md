# Sync entre dispositivos del usuario

**Estado:** `idea`

## Resumen

Sincronizar el historial de clipboard, los textos y (más adelante) la biblioteca
entre los equipos de **un mismo usuario**, sin una nube de Atic: los datos van
de un dispositivo del usuario a otro, cifrados de extremo a extremo. Gratis para
el usuario y casi sin infraestructura para nosotros. Mantiene la promesa
local-first de [`PRODUCT.md`](../PRODUCT.md): apagado por defecto y cada tipo de
dato se activa a propósito.

El caso que más aporta es **Windows ↔ macOS**: entre equipos Apple ya existe el
Portapapeles Universal, pero solo lleva el último ítem y no cruza a Windows.

## Cómo se usa

- Ajustes → Dispositivos → «Vincular»: un equipo muestra un QR o un código
  corto y el otro lo escanea o lo escribe. No hay cuentas ni contraseñas.
- Se elige qué sincronizar: texto del clipboard, imágenes, textos guardados.
- Copias en un equipo y el ítem aparece en el historial del otro. Pegarlo allá
  lo pone en su portapapeles, sin volver a enviarlo.
- «Desvincular» revoca un equipo perdido; los demás dejan de aceptarlo.

## Diseño propuesto

### Transporte, por capas

1. **Red local** — descubrimiento por mDNS (Bonjour en macOS) y conexión
   directa. No usa internet ni tiene costo.
2. **P2P por internet** — [iroh](https://iroh.computer) (Rust, QUIC): atraviesa
   NAT con hole punching; si no puede, pasa por un *relay* que solo ve tráfico
   cifrado. Hay relays públicos de n0 y se puede alojar uno propio en una VPS
   chica, o dejar que el usuario configure el suyo.
3. **Carpeta del usuario (diferido, opcional)** — archivos cifrados en OneDrive,
   Dropbox, iCloud Drive o WebDAV/S3. Cada equipo escribe solo su propio
   archivo, así que no hay conflictos. Sirve para snippets, ajustes y la
   biblioteca del [companion móvil](companion-movil.md); no para el
   «copio acá y pego allá» inmediato, porque tarda de segundos a minutos.

La limitación de 1 y 2 es que dos equipos tienen que estar encendidos a la vez.
Cualquier equipo que esté siempre prendido (PC de escritorio, NAS) puede hacer
de buzón para los demás.

### Datos

- **Clipboard:** log de solo agregar. ID = `(dispositivo, secuencia)`; los
  borrados y desfijados viajan como marcas (*tombstones*). Al reconectar, cada
  lado pide lo posterior al último ID que vio de cada dispositivo. No hace
  falta un CRDT.
- **Textos y ajustes:** gana la última escritura por ítem (*last-writer-wins*).
  Si algún día hay edición colaborativa, pasar a Automerge o Loro.
- **Imágenes:** siempre PNG en la red. Se envía el metadato y el contenido se
  trae al pegarlo o hasta un tope de tamaño.
- **Archivos copiados:** en los dos sistemas son rutas locales que no existen en
  el otro equipo. Se envía el contenido con tope de tamaño o no se sincronizan.
- **Texto:** plano siempre; HTML/RTF opcional. Los saltos de línea viajan tal
  cual.

### Reglas que no se negocian

- **Lo sensible nunca sale del equipo.** Se reutiliza `clipboard_is_sensitive`:
  si no entra al historial local, tampoco se sincroniza.
- **Sin eco.** Al escribir un ítem remoto en el portapapeles se le agrega una
  marca propia (ver tabla) y el vigilante ignora lo que la traiga. Como
  respaldo, se descartan duplicados por hash + ID de origen.
- **Claves en el llavero del sistema**, como las API keys: la identidad del
  nodo y las claves de pareo nunca van a `config.json`. El crate `keyring`
  cubre ambos sistemas.

## Windows y macOS

| Tema | Windows | macOS |
| --- | --- | --- |
| Marcadores de contenido sensible (ya implementado) | `ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` (DWORD `0`) | `org.nspasteboard.ConcealedType`, `org.nspasteboard.TransientType` |
| Marca anti-eco al escribir un ítem remoto | Formato registrado con `RegisterClipboardFormatW("com.atic.synced")` | Tipo `com.atic.synced` en el `NSPasteboard` |
| Rebote extra | Historial en la nube de Windows (`Win+V`) puede reenviarlo a otros PCs | El Portapapeles Universal lo pasa al iPhone/iPad |
| Evitar ese rebote | Poner también `CanIncludeInCloudClipboard = 0` | Poner también `org.nspasteboard.TransientType` si el usuario no quiere que siga viaje |
| Descubrimiento en LAN | mDNS (Windows 10+ lo trae) | Bonjour; exige `NSLocalNetworkUsageDescription` y `NSBonjourServices` (`_atic._udp`) en `Info.plist` desde macOS 15, o falla en silencio |
| Firewall | Defender pregunta al escuchar conexiones entrantes | El firewall de aplicaciones pregunta y vuelve a hacerlo si la firma del binario cambia |
| Mitigación de firewall | Priorizar conexiones salientes con hole punching de iroh en vez de un puerto fijo | Igual, más una firma estable de la app |
| Suspensión | Al volver de la hibernación los sockets quedan muertos: reconectar al despertar | App Nap frena timers y red en segundo plano; reconectar al despertar y, si hace falta, `NSProcessInfo.beginActivity` mientras se sincroniza |
| Llavero | Administrador de credenciales | Llavero (Keychain) |
| Imágenes nativas | `CF_DIB` / PNG → PNG en la red | `public.png` / `public.tiff` → PNG en la red |
| Carpeta en la nube | OneDrive (Archivos a petición) | iCloud Drive o OneDrive; ambos pueden dejar archivos sin descargar, hay que forzar la descarga antes de leer |
| Sandbox | No aplica | Si algún día se distribuye con App Sandbox: entitlements `network.client` y `network.server` |

Para quien usa los dos sistemas, **OneDrive, Dropbox o WebDAV** son mejor
carpeta que iCloud: iCloud para Windows es poco confiable.

## Por dónde empezar

1. Texto del clipboard por LAN (mDNS + conexión directa), pareo por QR,
   marca anti-eco y filtro de sensibles. Probar Windows ↔ macOS.
2. Mismo canal por internet con iroh.
3. Textos guardados y ajustes (LWW).
4. Imágenes con tope de tamaño.
5. Carpeta del usuario como respaldo en diferido y base del companion móvil.

## Código

- [`apps/desktop/src-tauri/src/clipboard_history.rs`](../apps/desktop/src-tauri/src/clipboard_history.rs)
  — vigilante y `clipboard_is_sensitive` (Windows y macOS), punto de entrada
  natural para emitir y recibir ítems.
- Aún no hay código de sync.

## Pendiente / siguiente

- [ ] Prototipo: dos instancias intercambiando texto por LAN.
- [ ] Decidir si alojamos un relay propio o usamos los públicos de n0.
- [ ] Definir el formato del log y del archivo cifrado en la carpeta.
- [ ] UI de vincular / desvincular en Ajustes.

## Relacionado

- [clipboard-historial.md](clipboard-historial.md)
- [snippets.md](snippets.md)
- [companion-movil.md](companion-movil.md) · [`docs/MOBILE.md`](../docs/MOBILE.md)
