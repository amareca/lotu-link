import type {
  DeviceDto,
  ErrorDto,
  KeyAction,
  KeyInput,
  PointerInput,
  ReplaySessionDto,
  ScrollInput,
  SessionDto,
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
