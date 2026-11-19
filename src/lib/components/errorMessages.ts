import type { ErrorCode } from "../models/api/dto";

/**
 * Presentation-only localization of the stable backend error codes.
 * Rust keeps `message` as an English diagnostic; the UI never shows it as the
 * primary text, only as a `title` tooltip.
 */
export const ERROR_MESSAGES: Record<ErrorCode, string> = {
  adb_unavailable: "No se encuentra ADB. Revisa la instalación.",
  adb_timeout: "ADB no responde. Reintenta.",
  adb_failed: "Error al listar dispositivos.",
  replay_unavailable: "El modo replay no está disponible.",
  fixture_invalid: "El vídeo de replay no es válido.",
  device_not_found: "El dispositivo ya no está disponible. Refresca la lista.",
  device_not_connected:
    "Autoriza o reconecta el dispositivo antes de abrir el espejo.",
  vendor_missing: "Falta el servidor de vídeo. Ejecuta fetch-vendor.",
  session_timeout: "El dispositivo no respondió a tiempo. Reintenta.",
  session_failed: "No se pudo iniciar el espejo. Reintenta.",
  session_inactive: "La sesión no está activa.",
  input_invalid: "Entrada no válida.",
  input_failed: "No se pudo enviar la entrada. Reintenta.",
  internal: "Error interno.",
};

export function errorMessageForCode(code: ErrorCode): string {
  return ERROR_MESSAGES[code];
}
