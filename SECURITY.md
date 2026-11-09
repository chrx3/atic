# Seguridad

Si encuentras una vulnerabilidad en Atic, por favor **no** abras un issue público con detalles explotables.

## Cómo reportar

1. Preferible: [GitHub Security Advisories](https://github.com/chrx3/atic/security/advisories/new) (reporte privado).
2. Si no puedes usar Advisories, abre un issue **sin** PoC ni pasos de explotación y pide contacto privado, o escribe al mantenedor vía el perfil de GitHub [@chrx3](https://github.com/chrx3).

Incluye, cuando puedas:

- Versión de Atic / commit
- SO y arquitectura
- Impacto (qué se puede leer, modificar o ejecutar)
- Pasos mínimos para reproducir (en el canal privado)

## Alcance

En alcance, entre otros:

- Filtrado de secretos del llavero o de la configuración
- Ejecución remota o local no intencionada a través de la app o de los agentes que lanza
- El enlace con el celular (`atic-sync`): el pareo por QR y lo que un celular vinculado, o un tercero en la red, puede leer o controlar
- El hub local de Atic Code y `atic-mcp`: el servidor en localhost (puerto y token en `hub.json`) por el que un agente le encarga trabajo a otro
- El updater: la descarga y la comprobación de la firma (minisign) de las actualizaciones
- Escalada de privilegios o bypass de permisos del sistema que la app facilite

Fuera de alcance habitual:

- Uso legítimo de la app para grabar audio con consentimiento del usuario
- Bugs de UX sin impacto de seguridad
- Vulnerabilidades solo en dependencias de terceros ya conocidas y sin vector en Atic (mejor reportar upstream; avísanos si afecta builds/releases)

## Respuesta

Intentaremos acusar recibo en unos días y priorizar según impacto. Gracias por ayudar a mantener Atic seguro.
