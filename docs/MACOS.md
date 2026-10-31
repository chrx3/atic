# Atic en macOS — instalar, permisos y firma local

Guía de la Atic de Mac (la app GPUI, `apps/atic`): cómo instalarla, qué
permisos pide y cómo darle una firma local estable si la compilas tú. Si vas
a desarrollar, mira [Para quien desarrolla](#para-quien-desarrolla).

---

## 1. Instalar

1. Baja el `.dmg` de la última versión en
   [GitHub Releases](https://github.com/chrx3/atic/releases) (`Atic_<versión>_universal.dmg`,
   sirve en Apple Silicon e Intel).
2. Ábrelo y arrastra **Atic** a **Aplicaciones**.
3. La primera vez, la app no está notarizada y Gatekeeper la bloquea: clic
   derecho sobre Atic → **Abrir**, o corre en la terminal
   `xattr -cr /Applications/Atic.app`.

Después de la primera vez, Atic se actualiza sola desde las Novedades y el
actualizador integrado.

Datos locales:

```text
~/Library/Application Support/com.ciat.atic/
```

El log de la pill queda en `logs/pill.<fecha>.log` dentro de esa carpeta.

---

## 2. Permisos

Atic se maneja desde **Ajustes de Atic → Permisos**: ahí ves el estado de cada
permiso, para qué lo usa y un botón para pedirlo (macOS te muestra el diálogo
o abre el panel correcto de Ajustes del Sistema). También se piden en el
recorrido de bienvenida.

| Permiso | Para qué | Dónde se concede |
|---|---|---|
| **Accesibilidad** | Pegar lo que copias o dictas, teclas de música, traer ventanas al frente | Ajustes del Sistema → Privacidad y seguridad → Accesibilidad |
| **Monitoreo de entrada** | Saber si mantienes el atajo del dictado o de la rueda mientras usas otra app | … → Monitoreo de entrada |
| **Grabación de pantalla** | Capturas, Pizarra, Flip, grabar la pantalla, cuentagotas | … → Grabación de pantalla y audio del sistema |
| **Micrófono** | Dictado y Reuniones | … → Micrófono |
| **Grabación de audio del sistema** | La onda de la pill con lo que suena y el volumen por app | … → Grabación de pantalla y audio del sistema, en «Solo audio del sistema» |

Notas:

- Tras conceder **Grabación de pantalla** o **Accesibilidad** hay que
  **reiniciar Atic**: macOS no le da el permiso a un proceso ya corriendo.
- El audio del sistema usa ScreenCaptureKit (macOS 13 o superior) y enciende el
  indicador de captura mientras graba. En macOS 11/12 solo hay micrófono.
- Monitores con escalas distintas (Retina + 1x) funcionan: cada captura sale a
  la resolución nativa de su monitor.

### Si un permiso no funciona

- **Después de actualizar** Atic sigue pidiendo el permiso, o aparece activado
  pero no hace nada: en Privacidad y seguridad borra la fila de Atic con el
  botón **−**, vuelve a agregarla con **+** (desde Aplicaciones) y reinicia la
  app. Con la firma estable de la sección siguiente esto no debería pasar.
- **Si lo denegaste por error:** actívalo a mano en el panel de la tabla de
  arriba, o desde Ajustes de Atic → Permisos, que abre el panel exacto.
- **Cuando corres la pill desde una terminal** (desarrollo), macOS atribuye los
  permisos a la terminal o a la app responsable, no a Atic.

---

## 3. Firma local estable (certificado propio)

Atic no usa el certificado de pago de Apple Developer. Con firma ad-hoc el
permiso queda atado al hash del binario: cada build lo invalida y reaparecen
los diálogos de Privacidad. Firmando siempre con el mismo certificado
autofirmado, los permisos (TCC) sobreviven a los rebuilds y a las
actualizaciones que distribuyas con ese certificado.

1. Crea el certificado una vez: **Acceso a Llavero → Asistente de
   certificados → Crear un certificado…** → nombre `Atic Dev`, tipo
   **Firma de código**, autofirmado, con validez larga (p. ej. 10 años).
2. Guarda el nombre en `~/.tauri/atic-signing-identity` (una línea, sin
   espacios extra). Lo leen `scripts/pill-dev-mac.sh`,
   `scripts/build-pill-mac.sh` y `scripts/release-macos.sh`; sin el archivo
   caen a firma ad-hoc. Cada máquina pone el suyo.
3. Borra las filas viejas de Atic en Ajustes → Privacidad y seguridad, abre la
   app firmada, concede el permiso una vez y reiníciala.

Los scripts firman con Hardened Runtime y aplican
`apps/atic/macos/entitlements.plist` (entre otros, el acceso al micrófono). Sin
ese entitlement, un build firmado le entrega **silencio** a la app por el
micrófono y el dictado transcribe «Gracias.».

Si al firmar aparece `resource fork, Finder information, or similar detritus
not allowed`: corre `xattr -cr RUTA/Atic.app` y reintenta.

---

## Para quien desarrolla

- Compilar, abrir y dejar la pill de dev corriendo:
  `bash scripts/pill-dev-mac.sh` (con `--watch` recompila y reabre al guardar).
  Usa la firma de la sección anterior si existe.
- Cómo se arma el `.app`, qué hay en la capa nativa y qué falta:
  [`MACOS_GPUI.md`](MACOS_GPUI.md).
- Reglas de trabajo, validación y publicación: [`AGENTS.md`](../AGENTS.md).
- Repo: https://github.com/chrx3/atic
