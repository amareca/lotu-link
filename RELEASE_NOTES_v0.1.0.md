# LOTU Link v0.1.0 — primer MVP de Windows

Primera versión instalable de LOTU Link: espejo y control de un Android por USB,
capturas PNG y grabación MP4, todo sin instalar nada en el móvil.

## Instalación

1. Descarga `LOTU Link_0.1.0_x64-setup.exe`.
2. Ejecútalo. La instalación es **por usuario**, no pide permisos de administrador.
3. El instalador **no está firmado**: Windows mostrará el aviso de SmartScreen.
   Pulsa **Más información → Ejecutar de todos modos**.
4. Necesitas **WebView2 Runtime** (incluido en Windows 11 y en Windows 10 recientes).

## Primer uso

1. Activa las opciones de desarrollador y la depuración USB en el móvil.
2. Conecta el móvil por USB y acepta el diálogo de autorización RSA.
3. En LOTU Link, abre **Dispositivos** y pulsa **Abrir espejo**.
4. El asistente muestra la huella RSA real de este PC: debe coincidir con la del móvil.

## Funciones incluidas

- Espejo H.264 de baja latencia con WebCodecs, rotación y reconexión automática.
- Ratón, teclado, navegación y portapapeles (Ctrl+C / Ctrl+V).
- Captura PNG a resolución nativa.
- Grabación MP4 sin recompresión; al girar el móvil se guarda un archivo por orientación.
- Ajustes persistentes de resolución, FPS y bitrate.
- ADB incluido en el instalador, sin necesidad de instalar platform-tools.

## Limitaciones del MVP

- Un solo dispositivo y una sola sesión activa.
- Solo USB (sin WiFi), sin audio y sin instalación de APKs.
- macOS y Linux no incluidos todavía.
- Sin actualización automática: cada versión se descarga manualmente.

## Verificación del archivo

`LOTU Link_0.1.0_x64-setup.exe`
SHA-256: `D25F5B6B158726648356E88C761AC91BDEFEE83CE4715ABC7BC7BA1035E1F89A`

## Licencias

LOTU Link se distribuye bajo licencia MIT (`LICENSE`). El instalador incluye los
avisos de terceros (`THIRD_PARTY_NOTICES.md`) y las licencias de scrcpy y ADB.
