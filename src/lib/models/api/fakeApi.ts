import { ApiError, type LotuApi } from "./api";
import type {
  DeviceDto,
  ErrorDto,
  KeyAction,
  KeyInput,
  PointerInput,
  RecordingDto,
  RecordingStateDto,
  ReplaySessionDto,
  RsaFingerprintDto,
  ScreenshotDto,
  ScrollInput,
  SessionDto,
  SettingsDto,
  VideoSettingsDto,
} from "./dto";

/** Deterministic data used by `pnpm dev` without Tauri and by the ViewModel tests. */
export const FAKE_DEVICES: readonly DeviceDto[] = [
  {
    serial: "PIXEL8PRO",
    name: "Pixel 8 Pro",
    state: "connected",
    androidVersion: "15",
    resolution: { width: 1080, height: 2400 },
    batteryPercent: 78,
    transport: "usb",
  },
  {
    serial: "GALAXYS23",
    name: "Galaxy S23",
    state: "unauthorized",
    androidVersion: null,
    resolution: null,
    batteryPercent: null,
    transport: "usb",
  },
];

/** Synthetic fixture metadata mirroring `fixtures/replay/pixel-portrait.json`. */
export const FAKE_REPLAY_DEVICE: DeviceDto = {
  serial: "REPLAY0001",
  name: "Pixel 8 Pro (replay)",
  state: "connected",
  androidVersion: "15",
  resolution: { width: 1080, height: 2400 },
  batteryPercent: 85,
  transport: "usb",
};

export interface FakeApiOptions {
  devices?: readonly DeviceDto[];
  failure?: ErrorDto | null;
  delayMs?: number;
  /**
   * Replay session returned by `startReplay`. It is never a real video source:
   * only the ViewModel tests use it with a fake media factory.
   */
  replay?: ReplaySessionDto | null;
  replayFailure?: ErrorDto | null;
  /**
   * Live session returned by `startSession`. It never paints video; tests pair
   * it with a fake media factory. Without it, `startSession` fails with
   * `session_failed` like the real API does without the Tauri backend.
   */
  session?: SessionDto | null;
  sessionFailure?: ErrorDto | null;
  /** M2: makes every input command reject with this error (tests only). */
  inputFailure?: ErrorDto | null;
  /** M3: text returned by `copyText` (tests only). */
  clipboardText?: string | null;
  /** M3: PNG result returned by `captureScreenshot` (tests only). */
  screenshotResult?: ScreenshotDto | null;
  /** M3: makes capture/recording commands reject (tests only). */
  captureFailure?: ErrorDto | null;
  recordingFailure?: ErrorDto | null;
  /** M4: initial settings returned by `getSettings` (tests only). */
  settings?: SettingsDto | null;
  settingsFailure?: ErrorDto | null;
  /** M4: fingerprint returned by `getRsaFingerprint` (tests only). */
  fingerprint?: RsaFingerprintDto | null;
}

export class FakeApi implements LotuApi {
  #devices: DeviceDto[];
  #failure: ErrorDto | null;
  #replay: ReplaySessionDto | null;
  #replayFailure: ErrorDto | null;
  #session: SessionDto | null;
  #sessionFailure: ErrorDto | null;
  #startSessionCalls: string[] = [];
  #stopSessionCalls = 0;
  #inputFailure: ErrorDto | null;
  #pointers: PointerInput[] = [];
  #scrolls: ScrollInput[] = [];
  #keys: KeyInput[] = [];
  #texts: string[] = [];
  #backs: KeyAction[] = [];
  #rotates = 0;
  #pastes: string[] = [];
  #copies = 0;
  #clipboardText: string;
  readonly #clipboardFailure: ErrorDto | null;
  readonly #captureFailure: ErrorDto | null;
  readonly #recordingFailure: ErrorDto | null;
  readonly #settingsFailure: ErrorDto | null;
  #settings: SettingsDto;
  #saveSettingsCalls = 0;
  #markSeenCalls = 0;
  readonly #fingerprint: RsaFingerprintDto;
  readonly #screenshotResult: ScreenshotDto;
  #captureCalls = 0;
  #startRecordingCalls = 0;
  #stopRecordingCalls = 0;
  readonly #delayMs: number;

  constructor(options: FakeApiOptions = {}) {
    this.#devices = cloneDevices(options.devices ?? FAKE_DEVICES);
    this.#failure = options.failure ?? null;
    this.#replay = options.replay ? cloneSession(options.replay) : null;
    this.#replayFailure = options.replayFailure ?? null;
    this.#session = options.session ? cloneSession(options.session) : null;
    this.#sessionFailure = options.sessionFailure ?? null;
    this.#inputFailure = options.inputFailure ?? null;
    this.#clipboardText = options.clipboardText ?? "texto de prueba";
    this.#clipboardFailure = options.inputFailure ?? null;
    this.#captureFailure = options.captureFailure ?? null;
    this.#recordingFailure = options.recordingFailure ?? null;
    this.#settingsFailure = options.settingsFailure ?? null;
    this.#settings = options.settings ?? {
      video: { maxSize: 0, maxFps: 60, bitRate: 8_000_000 },
      restartRequired: false,
      onboardingSeen: false,
    };
    this.#fingerprint = options.fingerprint ?? {
      fingerprint: "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99",
      error: null,
    };
    this.#screenshotResult = options.screenshotResult ?? {
      path: "captures/lotu-capture.png",
      width: 1080,
      height: 2400,
    };
    this.#delayMs = options.delayMs ?? 0;
  }

  setDevices(devices: readonly DeviceDto[]): void {
    this.#devices = cloneDevices(devices);
  }

  setFailure(failure: ErrorDto | null): void {
    this.#failure = failure;
  }

  setSession(session: SessionDto | null): void {
    this.#session = session ? cloneSession(session) : null;
  }

  setSessionFailure(failure: ErrorDto | null): void {
    this.#sessionFailure = failure;
  }

  /** Serials passed to `startSession`, in call order. */
  get startSessionCalls(): readonly string[] {
    return [...this.#startSessionCalls];
  }

  get stopSessionCalls(): number {
    return this.#stopSessionCalls;
  }

  get pointerCalls(): readonly PointerInput[] {
    return [...this.#pointers];
  }

  get scrollCalls(): readonly ScrollInput[] {
    return [...this.#scrolls];
  }

  get keyCalls(): readonly KeyInput[] {
    return [...this.#keys];
  }

  get textCalls(): readonly string[] {
    return [...this.#texts];
  }

  get backCalls(): readonly KeyAction[] {
    return [...this.#backs];
  }

  get rotateCalls(): number {
    return this.#rotates;
  }

  get pasteCalls(): readonly string[] {
    return [...this.#pastes];
  }

  get copyCalls(): number {
    return this.#copies;
  }

  get captureCalls(): number {
    return this.#captureCalls;
  }

  get startRecordingCalls(): number {
    return this.#startRecordingCalls;
  }

  get stopRecordingCalls(): number {
    return this.#stopRecordingCalls;
  }

  get saveSettingsCalls(): number {
    return this.#saveSettingsCalls;
  }

  get markSeenCalls(): number {
    return this.#markSeenCalls;
  }

  get currentSettings(): SettingsDto {
    return { ...this.#settings, video: { ...this.#settings.video } };
  }

  setInputFailure(failure: ErrorDto | null): void {
    this.#inputFailure = failure;
  }

  async listDevices(): Promise<DeviceDto[]> {
    await this.#wait();
    if (this.#failure !== null) {
      throw new ApiError(this.#failure);
    }

    return cloneDevices(this.#devices);
  }

  async startReplay(): Promise<ReplaySessionDto> {
    await this.#wait();
    if (this.#replayFailure !== null) {
      throw new ApiError(this.#replayFailure);
    }
    if (this.#replay === null) {
      // Without the Tauri replay server there is no video source to connect to.
      throw new ApiError({
        code: "replay_unavailable",
        message: "Replay requires the Tauri backend",
      });
    }

    return cloneSession(this.#replay);
  }

  async startSession(serial: string): Promise<SessionDto> {
    this.#startSessionCalls.push(serial);
    await this.#wait();
    if (this.#sessionFailure !== null) {
      throw new ApiError(this.#sessionFailure);
    }
    if (this.#session === null) {
      // Mirror `start_session` without the Tauri backend; never a video source.
      throw new ApiError({
        code: "session_failed",
        message: "Live session requires the Tauri backend",
      });
    }

    return cloneSession(this.#session);
  }

  async stopSession(): Promise<void> {
    this.#stopSessionCalls += 1;
    await this.#wait();
  }

  async sendPointer(input: PointerInput): Promise<void> {
    await this.#inputCall(() => this.#pointers.push(input));
  }

  async sendScroll(input: ScrollInput): Promise<void> {
    await this.#inputCall(() => this.#scrolls.push(input));
  }

  async sendKey(input: KeyInput): Promise<void> {
    await this.#inputCall(() => this.#keys.push(input));
  }

  async sendText(text: string): Promise<void> {
    await this.#inputCall(() => this.#texts.push(text));
  }

  async sendBack(action: KeyAction): Promise<void> {
    await this.#inputCall(() => this.#backs.push(action));
  }

  async rotate(): Promise<void> {
    await this.#inputCall(() => {
      this.#rotates += 1;
    });
  }

  async pasteText(text: string): Promise<void> {
    await this.#inputCall(() => this.#pastes.push(text));
  }

  async copyText(): Promise<string> {
    this.#copies += 1;
    await this.#wait();
    if (this.#clipboardFailure !== null) {
      throw new ApiError(this.#clipboardFailure);
    }
    return this.#clipboardText;
  }

  async captureScreenshot(): Promise<ScreenshotDto> {
    this.#captureCalls += 1;
    await this.#wait();
    if (this.#captureFailure !== null) throw new ApiError(this.#captureFailure);
    return { ...this.#screenshotResult };
  }

  async startRecording(): Promise<RecordingDto> {
    this.#startRecordingCalls += 1;
    await this.#wait();
    if (this.#recordingFailure !== null) throw new ApiError(this.#recordingFailure);
    return {
      path: "captures/lotu-recording.mp4",
      frames: 0,
      durationUs: 0,
      segments: 1,
      paths: ["captures/lotu-recording.mp4"],
    };
  }

  async stopRecording(): Promise<RecordingDto> {
    this.#stopRecordingCalls += 1;
    await this.#wait();
    if (this.#recordingFailure !== null) throw new ApiError(this.#recordingFailure);
    return {
      path: "captures/lotu-recording.mp4",
      frames: 120,
      durationUs: 4_000_000,
      segments: 1,
      paths: ["captures/lotu-recording.mp4"],
    };
  }

  async getSettings(): Promise<SettingsDto> {
    await this.#wait();
    if (this.#settingsFailure !== null) throw new ApiError(this.#settingsFailure);
    return this.currentSettings;
  }

  async saveSettings(video: VideoSettingsDto): Promise<SettingsDto> {
    this.#saveSettingsCalls += 1;
    await this.#wait();
    if (this.#settingsFailure !== null) throw new ApiError(this.#settingsFailure);
    if (video.bitRate <= 0 || video.maxSize > 8192 || video.maxFps > 240) {
      throw new ApiError({ code: "settings_invalid", message: "invalid video settings" });
    }
    this.#settings = {
      video: { ...video },
      restartRequired: false,
      onboardingSeen: this.#settings.onboardingSeen,
    };
    return this.currentSettings;
  }

  async markOnboardingSeen(): Promise<SettingsDto> {
    this.#markSeenCalls += 1;
    await this.#wait();
    if (this.#settingsFailure !== null) throw new ApiError(this.#settingsFailure);
    this.#settings = { ...this.#settings, onboardingSeen: true };
    return this.currentSettings;
  }

  async getRsaFingerprint(): Promise<RsaFingerprintDto> {
    await this.#wait();
    return { ...this.#fingerprint };
  }

  onRecordingState(_listener: (state: RecordingStateDto) => void): () => void {
    // The fake backend has no event bus; the ViewModel keeps its own state.
    return () => {};
  }

  async #inputCall(record: () => void): Promise<void> {
    await this.#wait();
    if (this.#inputFailure !== null) {
      throw new ApiError(this.#inputFailure);
    }
    record();
  }

  async #wait(): Promise<void> {
    if (this.#delayMs > 0) {
      await delay(this.#delayMs);
    }
  }
}

function cloneDevices(devices: readonly DeviceDto[]): DeviceDto[] {
  return devices.map(cloneDevice);
}

function cloneDevice(device: DeviceDto): DeviceDto {
  return {
    ...device,
    resolution: device.resolution ? { ...device.resolution } : null,
  };
}

function cloneSession(session: SessionDto): SessionDto {
  return { wsUrl: session.wsUrl, device: cloneDevice(session.device) };
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}
