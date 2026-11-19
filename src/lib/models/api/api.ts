import type {
  DeviceDto,
  ErrorDto,
  KeyAction,
  KeyInput,
  PointerInput,
  ReplaySessionDto,
  RecordingDto,
  RecordingStateDto,
  ScreenshotDto,
  ScrollInput,
  SessionDto,
  RsaFingerprintDto,
  SettingsDto,
  VideoSettingsDto,
} from "./dto";

/**
 * Contract between ViewModels and the desktop shell.
 * Commands reject with `ErrorDto`; implementations wrap them in `ApiError`.
 */
export interface LotuApi {
  listDevices(): Promise<DeviceDto[]>;
  /** M0b: starts the replay actor and returns the loopback WebSocket URL. */
  startReplay(): Promise<ReplaySessionDto>;
  /** M1: starts a live USB session for a `connected` serial. */
  startSession(serial: string): Promise<SessionDto>;
  /** M1: stops the live session; idempotent on the backend. */
  stopSession(): Promise<void>;
  /** M2: input on the live session's control socket. */
  sendPointer(input: PointerInput): Promise<void>;
  sendScroll(input: ScrollInput): Promise<void>;
  sendKey(input: KeyInput): Promise<void>;
  sendText(text: string): Promise<void>;
  sendBack(action: KeyAction): Promise<void>;
  rotate(): Promise<void>;
  /** M3: writes PC text into the device clipboard and injects PASTE. */
  pasteText(text: string): Promise<void>;
  /** M3: returns the device clipboard after injecting COPY. */
  copyText(): Promise<string>;
  /** M3: saves a native-resolution PNG chosen in the OS dialog. */
  captureScreenshot(): Promise<ScreenshotDto>;
  /** M3: starts/stops the MP4 recording of the live stream. */
  startRecording(): Promise<RecordingDto>;
  stopRecording(): Promise<RecordingDto>;
  /** M3: backend-driven state, also emitted on session stop. */
  onRecordingState(listener: (state: RecordingStateDto) => void): () => void;
  /** M4: persisted video settings. */
  getSettings(): Promise<SettingsDto>;
  saveSettings(video: VideoSettingsDto): Promise<SettingsDto>;
  markOnboardingSeen(): Promise<SettingsDto>;
  /** M4: real ADB RSA fingerprint shown in the authorization dialog. */
  getRsaFingerprint(): Promise<RsaFingerprintDto>;
}

/** UI-facing error. Keeps the Rust `ErrorDto` available for localization by `code`. */
export class ApiError extends Error {
  readonly dto: ErrorDto;

  constructor(dto: ErrorDto) {
    super(dto.message);
    this.name = "ApiError";
    this.dto = dto;
  }
}
