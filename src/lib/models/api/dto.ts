export type DeviceStateDto = "connected" | "unauthorized" | "offline";

export interface ResolutionDto {
  width: number;
  height: number;
}

export interface DeviceDto {
  serial: string;
  name: string;
  state: DeviceStateDto;
  androidVersion: string | null;
  resolution: ResolutionDto | null;
  batteryPercent: number | null;
  transport: "usb";
}

export type ErrorCode =
  | "adb_unavailable"
  | "adb_timeout"
  | "adb_failed"
  | "replay_unavailable"
  | "fixture_invalid"
  | "device_not_found"
  | "device_not_connected"
  | "vendor_missing"
  | "session_timeout"
  | "session_failed"
  | "session_inactive"
  | "input_invalid"
  | "input_failed"
  | "capture_timeout"
  | "capture_invalid"
  | "capture_failed"
  | "recording_already_active"
  | "recording_inactive"
  | "recording_failed"
  | "user_cancelled"
  | "io_failed"
  | "settings_invalid"
  | "settings_io"
  | "internal";

export interface ErrorDto {
  code: ErrorCode;
  message: string;
}

/** Mirror of `SessionDto` in `lotu-app` for both `start_session` and `start_replay`. */
export interface SessionDto {
  wsUrl: string;
  device: DeviceDto;
}

/** M0b alias: the replay command returns the same payload as a live session. */
export type ReplaySessionDto = SessionDto;

/** M4: video settings persisted by `save_settings`. */
export interface VideoSettingsDto {
  maxSize: number;
  maxFps: number;
  bitRate: number;
}

export interface SettingsDto {
  video: VideoSettingsDto;
  restartRequired: boolean;
  onboardingSeen: boolean;
}

/** M4 onboarding: the real ADB RSA fingerprint, or a safe failure reason. */
export interface RsaFingerprintDto {
  fingerprint: string | null;
  error: string | null;
}

export function parseRsaFingerprintDto(value: unknown): RsaFingerprintDto {
  if (!isRecord(value)) return { fingerprint: null, error: "Respuesta no válida" };
  const fingerprint = typeof value.fingerprint === "string" ? value.fingerprint : null;
  const error = typeof value.error === "string" ? value.error : null;
  return { fingerprint, error };
}

export function parseSettingsDto(value: unknown): SettingsDto {
  if (!isRecord(value)) throw new Error("Invalid settings payload");
  const video = value.video;
  if (!isRecord(video)) throw new Error("Invalid settings payload: missing video");
  const { maxSize, maxFps, bitRate } = video;
  if (!isNonNegativeInt(maxSize) || maxSize > 8192) {
    throw new Error("Invalid settings payload: bad maxSize");
  }
  if (!isNonNegativeInt(maxFps) || maxFps > 240) {
    throw new Error("Invalid settings payload: bad maxFps");
  }
  if (!isNonNegativeInt(bitRate) || bitRate === 0) {
    throw new Error("Invalid settings payload: bad bitRate");
  }
  return {
    video: { maxSize, maxFps, bitRate },
    restartRequired: value.restartRequired === true,
    onboardingSeen: value.onboardingSeen === true,
  };
}

/** M3: a PNG saved by `capture_screenshot`. */
export interface ScreenshotDto {
  path: string;
  width: number;
  height: number;
}

/** M3: a finished (or just started) MP4 recording. */
export interface RecordingDto {
  path: string;
  frames: number;
  durationUs: number;
  segments: number;
  paths: string[];
}

/** M3: snapshot pushed by the `recording-state` Tauri event. */
export interface RecordingStateDto {
  active: boolean;
  path: string | null;
}

export function parseScreenshotDto(value: unknown): ScreenshotDto {
  if (!isRecord(value)) throw new Error("Invalid screenshot payload");
  const { path, width, height } = value;
  if (typeof path !== "string" || path.length === 0) {
    throw new Error("Invalid screenshot payload: missing path");
  }
  if (!isNonNegativeInt(width) || !isNonNegativeInt(height) || width <= 0 || height <= 0) {
    throw new Error("Invalid screenshot payload: bad dimensions");
  }
  return { path, width, height };
}

export function parseRecordingDto(value: unknown): RecordingDto {
  if (!isRecord(value)) throw new Error("Invalid recording payload");
  const { path, frames, durationUs } = value;
  if (typeof path !== "string" || path.length === 0) {
    throw new Error("Invalid recording payload: missing path");
  }
  if (!isNonNegativeInt(frames) || !isNonNegativeInt(durationUs)) {
    throw new Error("Invalid recording payload: bad statistics");
  }
  const segments = isNonNegativeInt(value.segments) ? value.segments : 1;
  const paths = Array.isArray(value.paths)
    ? value.paths.filter((item): item is string => typeof item === "string")
    : [path];
  return { path, frames, durationUs, segments: Math.max(segments, 1), paths };
}

export function parseRecordingStateDto(value: unknown): RecordingStateDto {
  if (!isRecord(value)) return { active: false, path: null };
  const path = typeof value.path === "string" ? value.path : null;
  return { active: value.active === true, path };
}

function isNonNegativeInt(value: unknown): value is number {
  return (
    typeof value === "number" && Number.isInteger(value) && value >= 0
  );
}

/** M2 input contracts (mirrors of the Rust DTOs in `lotu-app`). */
export type PointerAction = "down" | "move" | "up";
export type KeyAction = "down" | "up";

export interface PointerInput {
  action: PointerAction;
  x: number;
  y: number;
  screenWidth: number;
  screenHeight: number;
  pressure: number;
  buttons: number;
  actionButton: number;
}

export interface ScrollInput {
  x: number;
  y: number;
  screenWidth: number;
  screenHeight: number;
  hScroll: number;
  vScroll: number;
  buttons: number;
}

export interface KeyInput {
  action: KeyAction;
  keycode: number;
  repeat: number;
  metaState: number;
}

const DEVICE_STATES: readonly DeviceStateDto[] = [
  "connected",
  "unauthorized",
  "offline",
];

const ERROR_CODES: readonly ErrorCode[] = [
  "adb_unavailable",
  "adb_timeout",
  "adb_failed",
  "replay_unavailable",
  "fixture_invalid",
  "device_not_found",
  "device_not_connected",
  "vendor_missing",
  "session_timeout",
  "session_failed",
  "session_inactive",
  "input_invalid",
  "input_failed",
  "capture_timeout",
  "capture_invalid",
  "capture_failed",
  "recording_already_active",
  "recording_inactive",
  "recording_failed",
  "user_cancelled",
  "io_failed",
  "settings_invalid",
  "settings_io",
  "internal",
];

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isDeviceState(value: unknown): value is DeviceStateDto {
  return (
    typeof value === "string" &&
    (DEVICE_STATES as readonly string[]).includes(value)
  );
}

function isErrorCode(value: unknown): value is ErrorCode {
  return (
    typeof value === "string" &&
    (ERROR_CODES as readonly string[]).includes(value)
  );
}

function parseOptionalString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function parseResolution(value: unknown): ResolutionDto | null {
  if (!isRecord(value)) return null;

  const { width, height } = value;
  if (typeof width !== "number" || typeof height !== "number") return null;
  if (!Number.isInteger(width) || !Number.isInteger(height)) return null;
  if (width <= 0 || height <= 0) return null;

  return { width, height };
}

function parseBatteryPercent(value: unknown): number | null {
  if (typeof value !== "number" || !Number.isInteger(value)) return null;
  if (value < 0 || value > 100) return null;

  return value;
}

/**
 * Validates one `DeviceDto` as serialized by Rust (`serde(rename_all = "camelCase")`).
 * Throws when the required fields break the contract; optional fields degrade to null.
 */
export function parseDeviceDto(value: unknown): DeviceDto {
  if (!isRecord(value)) {
    throw new Error("Invalid device payload: expected an object");
  }

  const { serial, name, state, transport } = value;
  if (typeof serial !== "string" || serial.length === 0) {
    throw new Error("Invalid device payload: missing serial");
  }
  if (typeof name !== "string" || name.length === 0) {
    throw new Error("Invalid device payload: missing name");
  }
  if (!isDeviceState(state)) {
    throw new Error("Invalid device payload: unknown state");
  }
  if (transport !== "usb") {
    throw new Error("Invalid device payload: unknown transport");
  }

  return {
    serial,
    name,
    state,
    androidVersion: parseOptionalString(value.androidVersion),
    resolution: parseResolution(value.resolution),
    batteryPercent: parseBatteryPercent(value.batteryPercent),
    transport: "usb",
  };
}

export function parseDeviceDtos(value: unknown): DeviceDto[] {
  if (!Array.isArray(value)) {
    throw new Error("Invalid devices payload: expected an array");
  }

  return value.map((device) => parseDeviceDto(device));
}

/**
 * Validates one session payload (`start_session` / `start_replay`). The URL is
 * only checked for shape (absolute WebSocket URL); the token and binding are a
 * Rust concern.
 */
export function parseSessionDto(value: unknown): SessionDto {
  if (!isRecord(value)) {
    throw new Error("Invalid session payload: expected an object");
  }

  const { wsUrl } = value;
  if (typeof wsUrl !== "string" || wsUrl.length === 0) {
    throw new Error("Invalid session payload: missing wsUrl");
  }

  let parsedUrl: URL;
  try {
    parsedUrl = new URL(wsUrl);
  } catch {
    throw new Error("Invalid session payload: wsUrl is not an absolute URL");
  }
  if (parsedUrl.protocol !== "ws:" && parsedUrl.protocol !== "wss:") {
    throw new Error("Invalid session payload: wsUrl must use ws or wss");
  }

  return { wsUrl, device: parseDeviceDto(value.device) };
}

/** M0b entry point; kept as a delegate so replay callers stay unchanged. */
export function parseReplaySessionDto(value: unknown): ReplaySessionDto {
  return parseSessionDto(value);
}

export function isErrorDto(value: unknown): value is ErrorDto {
  return (
    isRecord(value) &&
    isErrorCode(value.code) &&
    typeof value.message === "string"
  );
}

/** Normalizes any rejection into the stable `ErrorDto` shape used by the UI. */
export function toErrorDto(value: unknown): ErrorDto {
  if (isErrorDto(value)) return value;
  if (value instanceof Error) {
    return { code: "internal", message: value.message };
  }
  if (typeof value === "string") {
    return { code: "internal", message: value };
  }

  return { code: "internal", message: "Unexpected error" };
}
