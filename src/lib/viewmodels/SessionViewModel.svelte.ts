import { ApiError, type LotuApi } from "../models/api/api";
import {
  toErrorDto,
  type DeviceDto,
  type ErrorDto,
  type KeyAction,
  type KeyInput,
  type PointerAction,
  type PointerInput,
  type RecordingDto,
  type RecordingStateDto,
  type ResolutionDto,
  type ScreenshotDto,
  type ScrollInput,
  type SessionDto,
} from "../models/api/dto";
import {
  BUTTON_PRIMARY,
  KEYCODES,
  mapKey,
  mapPoint,
  wheelToScroll,
  type KeyEventInput,
  type Point,
  type WheelDeltaMode,
} from "../models/input/inputMapper";
import type { VideoRenderer } from "../models/media/canvasRenderer";
import type {
  StreamClientHandlers,
  StreamConnection,
  StreamError,
  StreamMessage,
} from "../models/media/streamClient";
import type { VideoSink, VideoSinkHandlers } from "../models/media/webCodecsSink";

export type SessionPhase = "idle" | "connecting" | "streaming" | "error";
export type SessionOrigin = "live" | "replay";

/// M4: bounded exponential backoff for live stream disconnects.
export const RECONNECT_DELAYS_MS = [300, 700, 1500] as const;
/// Later retries while the device is still re-enumerating on USB.
export const DEVICE_WAIT_INTERVAL_MS = 2000;

/**
 * Media dependencies injected by the composition root (`main.ts`). Tests pass
 * fakes; the ViewModel never imports Tauri, WebSocket or WebCodecs.
 */
export interface MediaFactory {
  createRenderer(): VideoRenderer;
  createSink(handlers: VideoSinkHandlers): VideoSink;
  createStreamClient(handlers: StreamClientHandlers): StreamConnection;
}

/** Pointer data forwarded by the view (CSS pixels relative to the canvas). */
export interface PointerEventData {
  readonly x: number;
  readonly y: number;
  readonly canvasWidth: number;
  readonly canvasHeight: number;
  readonly pointerId: number;
  readonly button: number;
}

/** Wheel data forwarded by the view. */
export interface WheelEventData {
  readonly x: number;
  readonly y: number;
  readonly canvasWidth: number;
  readonly canvasHeight: number;
  readonly deltaX: number;
  readonly deltaY: number;
  readonly deltaMode: WheelDeltaMode;
}

/**
 * Live/replay stream state (M0b + M1).
 *
 * Owns the stream lifecycle: `startReplay()` and `startSession()` ask Rust for
 * the loopback URL, connect the WebSocket, pipe config/frames into the sink and
 * keep only presentation state reactive. Frames never reach `$state`.
 */
export class SessionViewModel {
  phase = $state<SessionPhase>("idle");
  device = $state<DeviceDto | null>(null);
  resolution = $state<ResolutionDto | null>(null);
  fps = $state(0);
  origin = $state<SessionOrigin | null>(null);
  error = $state<ErrorDto | null>(null);
  /** Diagnostic message kept for the M0 replay overlay and existing tests. */
  errorMessage = $derived(this.error?.message ?? null);
  /** Last input delivery problem; never changes `phase` (§3.4). */
  inputNotice = $state<ErrorDto | null>(null);
  /** M3: last capture/recording problem; never changes `phase`. */
  captureNotice = $state<ErrorDto | null>(null);
  /** M3: authoritative recording slot (also updated by the backend event). */
  recording = $state<RecordingStateDto | null>(null);
  /** M3: elapsed milliseconds, refreshed while recording. */
  recordingElapsed = $state(0);
  /** Transient toast shown by the view; owned here so any view can raise it. */
  toast = $state<string | null>(null);
  /** Input is only meaningful while a live stream is decoding. */
  inputEnabled = $derived(
    this.phase === "streaming" && this.origin === "live" && this.resolution !== null,
  );

  readonly #api: LotuApi;
  readonly #media: MediaFactory;

  #client: StreamConnection | null = null;
  #sink: VideoSink | null = null;
  #renderer: VideoRenderer | null = null;
  #fpsTimer: ReturnType<typeof setInterval> | null = null;
  #frameCount = 0;
  #generation = 0;
  #lastSerial: string | null = null;
  #disposed = false;

  // M2 gesture state is intentionally NOT reactive: only one pointer is
  // supported, moves are coalesced per frame, and frames never enter $state.
  #gesture: {
    pointerId: number;
    last: Point;
    resolution: ResolutionDto;
  } | null = null;
  #pendingMove: {
    point: Point;
    resolution: ResolutionDto;
  } | null = null;
  #rafId: number | null = null;
  #frameTimer: ReturnType<typeof setTimeout> | null = null;
  #unlistenRecording: (() => void) | null = null;
  #recordingTimer: ReturnType<typeof setInterval> | null = null;
  #recordingStartedAt = 0;
  #toastTimer: ReturnType<typeof setTimeout> | null = null;
  #reconnectAttempts = 0;
  #reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  #waitingForDevice = false;

  constructor(api: LotuApi, media: MediaFactory) {
    this.#api = api;
    this.#media = media;
  }

  /** Shows a short, non-blocking message; newer toasts replace older ones. */
  showToast(text: string, ms = 2800): void {
    this.toast = text;
    if (this.#toastTimer !== null) clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => {
      this.toast = null;
      this.#toastTimer = null;
    }, ms);
  }

  /**
   * Starts (or restarts) the dev replay. Never throws: failures land in
   * `phase = "error"` with a typed `error`/`errorMessage`.
   */
  startReplay(): Promise<void> {
    return this.#start("replay", null, () => this.#api.startReplay());
  }

  /**
   * Starts (or replaces) the live session for a `connected` serial. Same flow
   * as replay; never throws to the view.
   */
  startSession(serial: string): Promise<void> {
    return this.#start("live", serial, () => this.#api.startSession(serial));
  }

  /** Retries the last live serial (MirrorView error banner). No-op otherwise. */
  retry(): Promise<void> {
    if (this.origin !== "live" || this.#lastSerial === null) {
      return Promise.resolve();
    }

    return this.startSession(this.#lastSerial);
  }

  /** Stops the stream and clears the session state. Safe to call when idle. */
  stop(): void {
    const wasLive = this.origin === "live";

    this.#generation += 1;
    this.#teardown();
    this.origin = null;
    this.phase = "idle";
    this.device = null;
    this.resolution = null;
    this.fps = 0;
    this.error = null;
    this.inputNotice = null;
    this.captureNotice = null;
    this.recording = null;
    this.#stopRecordingTimer();
    this.#reconnectAttempts = 0;
    this.#waitingForDevice = false;
    this.#cancelGesture();

    if (wasLive) {
      // The backend owns authoritative cleanup; UI stop stays best-effort.
      void this.#api.stopSession().catch(() => {});
    }
  }

  dispose(): void {
    if (this.#disposed) return;

    const wasLive = this.origin === "live";

    this.#disposed = true;
    this.#generation += 1;
    this.#teardown();

    const renderer = this.#renderer;
    this.#renderer = null;
    renderer?.dispose();

    this.origin = null;
    this.#lastSerial = null;
    this.phase = "idle";
    this.device = null;
    this.resolution = null;
    this.fps = 0;
    this.error = null;
    this.inputNotice = null;
    this.captureNotice = null;
    this.recording = null;
    this.#stopRecordingTimer();
    this.#reconnectAttempts = 0;
    this.#waitingForDevice = false;
    this.#cancelGesture();

    if (wasLive) {
      // Fire-and-forget: `dispose()` is synchronous and cannot await IPC.
      void this.#api.stopSession().catch(() => {});
    }
  }

  /**
   * Attaches the mirror canvas. The view calls this from an `$effect` and uses
   * the returned function as cleanup.
   */
  attachCanvas(canvas: HTMLCanvasElement): () => void {
    if (this.#disposed) return () => {};

    let renderer = this.#renderer;
    if (renderer === null) {
      renderer = this.#media.createRenderer();
      this.#renderer = renderer;
    }
    renderer.attach(canvas);

    return () => {
      renderer.detach(canvas);
    };
  }

  async #start(
    origin: SessionOrigin,
    serial: string | null,
    request: () => Promise<SessionDto>,
  ): Promise<void> {
    if (this.#disposed) return;

    // Drop the previous stream without resetting the reconnect budget: a
    // retry is also a `startSession` call.
    this.#generation += 1;
    this.#teardown();
    this.#cancelGesture();
    const generation = ++this.#generation;
    this.#lastSerial = serial;
    this.origin = origin;
    this.phase = "connecting";
    this.error = null;
    this.device = null;
    this.resolution = null;
    this.fps = 0;
    this.inputNotice = null;
    this.captureNotice = null;
    this.recording = null;
    this.#stopRecordingTimer();
    this.#cancelGesture();

    let session: SessionDto;
    try {
      session = await request();
    } catch (cause) {
      if (!this.#isCurrent(generation)) return;
      // While reconnecting, a failed `start_session` is transient (the device
      // may still be re-enumerating on USB) and must not abort the sequence.
      const retrying = this.#reconnectAttempts > 0;
      this.#fail(generation, errorFrom(cause), origin === "live" && retrying);
      return;
    }

    if (!this.#isCurrent(generation)) return;

    this.device = session.device;
    this.#unlistenRecording = this.#api.onRecordingState((state) => {
      if (!this.#isCurrent(generation)) return;
      this.#applyRecordingState(state);
    });
    if (this.#renderer === null) {
      this.#renderer = this.#media.createRenderer();
    }

    const sink = this.#media.createSink({
      draw: (frame) => {
        this.#renderer?.draw(frame);
      },
      onError: (error) => {
        // Decoder/config errors are not fixed by reconnecting.
        this.#fail(generation, this.#streamFailure(error), false);
      },
    });
    const client = this.#media.createStreamClient({
      onMessage: (message) => {
        this.#handleMessage(generation, message);
      },
      onError: (error) => {
        this.#fail(generation, this.#streamFailure(error), true);
      },
      onClose: (error) => {
        this.#fail(generation, this.#streamFailure(error), true);
      },
    });
    this.#sink = sink;
    this.#client = client;

    try {
      client.open(session.wsUrl);
    } catch (cause) {
      this.#fail(generation, errorFrom(cause), origin === "live");
      return;
    }

    this.#startFpsTimer(generation);
  }

  #handleMessage(generation: number, message: StreamMessage): void {
    if (!this.#isCurrent(generation)) return;

    const sink = this.#sink;
    if (sink === null) return;

    if (message.kind === "config") {
      const next = { width: message.width, height: message.height };
      const previous = this.resolution;
      const changed =
        previous !== null &&
        (previous.width !== next.width || previous.height !== next.height);

      // A gesture started in the old coordinate space cannot be continued with
      // new dimensions: the server would discard it and leave a finger stuck.
      if (changed && this.#gesture !== null && previous !== null) {
        void this.#sendPointer("up", this.#gesture.last, previous);
        this.#cancelGesture();
      }

      this.resolution = next;
      sink.onConfig({
        width: message.width,
        height: message.height,
        avcc: message.avcc,
      });
      return;
    }

    // A frame proves the reconnection worked: restart the backoff budget.
    if (this.phase !== "streaming") {
      this.#reconnectAttempts = 0;
      this.#waitingForDevice = false;
    }
    this.phase = "streaming";
    this.#frameCount += 1;
    sink.onFrame({
      ptsUs: message.ptsUs,
      keyframe: message.keyframe,
      data: message.data,
    });
  }

  // ----- M2 input -------------------------------------------------------

  /** Reacts to a pointer press on the mirror canvas. */
  pointerDown(event: PointerEventData): void {
    if (!this.inputEnabled || this.resolution === null) return;

    if (event.button === 2) {
      this.#sendBack("down");
      return;
    }
    if (event.button === 1) {
      this.#sendKey("down", KEYCODES.home, 0);
      this.#sendKey("up", KEYCODES.home, 0);
      return;
    }
    if (event.button !== 0 || this.#gesture !== null) return;

    const point = this.#map(event);
    if (point === null) return;

    this.#gesture = {
      pointerId: event.pointerId,
      last: point,
      resolution: this.resolution,
    };
    void this.#sendPointer("down", point, this.resolution);
  }

  /** Queues a pointer move; only the newest move per frame is sent. */
  pointerMove(event: PointerEventData): void {
    const gesture = this.#gesture;
    if (
      !this.inputEnabled ||
      gesture === null ||
      event.pointerId !== gesture.pointerId ||
      this.resolution === null
    ) {
      return;
    }

    const point = this.#map(event) ?? gesture.last;
    gesture.last = point;
    this.#pendingMove = { point, resolution: this.resolution };
    this.#scheduleMove();
  }

  /** Ends the active gesture, releasing the touch on the device. */
  pointerUp(event: PointerEventData): void {
    const gesture = this.#gesture;
    if (gesture === null || event.pointerId !== gesture.pointerId) return;

    const point = this.#map(event) ?? gesture.last;
    void this.#sendPointer("up", point, gesture.resolution);
    this.#cancelGesture();
  }

  /** Cancellation also releases the touch: the server has no touch timeout. */
  pointerCancel(event: PointerEventData): void {
    const gesture = this.#gesture;
    if (gesture === null || event.pointerId !== gesture.pointerId) return;

    void this.#sendPointer("up", gesture.last, gesture.resolution);
    this.#cancelGesture();
  }

  /** Translates a wheel event into scrcpy scroll notches. */
  wheel(event: WheelEventData): void {
    if (!this.inputEnabled || this.resolution === null) return;

    const delta = wheelToScroll(event.deltaX, event.deltaY, event.deltaMode);
    if (delta.hScroll === 0 && delta.vScroll === 0) return;

    const point = this.#map(event);
    if (point === null) return;

    const input: ScrollInput = {
      x: point.x,
      y: point.y,
      screenWidth: this.resolution.width,
      screenHeight: this.resolution.height,
      hScroll: delta.hScroll,
      vScroll: delta.vScroll,
      buttons: 0,
    };
    void this.#send(input, (api) => api.sendScroll(input));
  }

  /** Handles a DOM keydown; returns true when the event was consumed. */
  keyDown(event: KeyEventInput): boolean {
    if (!this.inputEnabled) return false;

    const mapping = mapKey(event);
    if (mapping === null) return false;

    if (mapping.kind === "key") {
      this.#sendKey("down", mapping.keycode, mapping.metaState);
    } else {
      this.#sendText(mapping.text);
    }
    return true;
  }

  /** Handles a DOM keyup; returns true when the event was consumed. */
  keyUp(event: KeyEventInput): boolean {
    if (!this.inputEnabled) return false;

    const mapping = mapKey(event);
    if (mapping === null || mapping.kind !== "key") return false;

    this.#sendKey("up", mapping.keycode, mapping.metaState);
    return true;
  }

  /** Commits an IME composition as plain text (preedit is not shown). */
  compositionEnd(text: string): void {
    if (!this.inputEnabled || text.length === 0) return;
    this.#sendText(text);
  }

  back(): void {
    this.#sendBack("down");
    this.#sendBack("up");
  }

  home(): void {
    this.#sendKey("down", KEYCODES.home, 0);
    this.#sendKey("up", KEYCODES.home, 0);
  }

  recents(): void {
    this.#sendKey("down", KEYCODES.appSwitch, 0);
    this.#sendKey("up", KEYCODES.appSwitch, 0);
  }

  rotate(): void {
    if (!this.inputEnabled) return;
    void this.#send(null, (api) => api.rotate());
  }

  /** Sends PC clipboard text to the device (the view reads the OS clipboard). */
  pasteText(text: string): void {
    if (!this.inputEnabled || text.length === 0) return;
    void this.#send(text, (api) => api.pasteText(text));
  }

  /** Reads the device clipboard; returns null on error or inactive session. */
  async copyText(): Promise<string | null> {
    if (!this.inputEnabled) return null;
    try {
      return await this.#api.copyText();
    } catch (cause) {
      if (this.#isCurrent(this.#generation)) {
        this.inputNotice = errorFrom(cause);
      }
      return null;
    }
  }

  // ----- M3 capture -----------------------------------------------------

  /** Saves a native-resolution PNG. Returns null on failure (view shows a toast). */
  async capture(): Promise<ScreenshotDto | null> {
    if (this.origin !== "live" || this.phase === "idle") return null;
    this.captureNotice = null;
    try {
      const result = await this.#api.captureScreenshot();
      if (this.#isCurrent(this.#generation)) {
        this.captureNotice = null;
        this.showToast(`Captura guardada · ${result.width}×${result.height}`);
      }
      return result;
    } catch (cause) {
      if (this.#isCurrent(this.#generation)) {
        const error = errorFrom(cause);
        this.captureNotice = error;
        this.showToast(
          error.code === "user_cancelled"
            ? "Captura cancelada."
            : "No se pudo guardar la captura.",
        );
      }
      return null;
    }
  }

  /** Starts recording. Returns null on failure or when no live session is active. */
  async startRecording(): Promise<RecordingDto | null> {
    if (this.origin !== "live" || this.phase === "idle") return null;
    this.captureNotice = null;
    try {
      const result = await this.#api.startRecording();
      if (this.#isCurrent(this.#generation)) {
        this.captureNotice = null;
        this.#applyRecordingState({ active: true, path: result.path });
        this.showToast("Grabando… el primer fotograma clave");
      }
      return result;
    } catch (cause) {
      if (this.#isCurrent(this.#generation)) {
        const error = errorFrom(cause);
        this.captureNotice = error;
        this.showToast(
          error.code === "user_cancelled"
            ? "Grabación cancelada."
            : error.code === "recording_failed" || error.code === "io_failed"
              ? `No se pudo iniciar: ${error.message}`
              : `No se pudo iniciar la grabación (${error.code}).`,
        );
      }
      return null;
    }
  }

  /** Finalizes the recording and stops the local elapsed timer. */
  async stopRecording(): Promise<RecordingDto | null> {
    this.captureNotice = null;
    try {
      const result = await this.#api.stopRecording();
      if (this.#isCurrent(this.#generation)) {
        this.captureNotice = null;
        this.#applyRecordingState({ active: false, path: result.path });
        this.showToast(
          result.segments > 1
            ? `Grabación guardada en ${result.segments} archivos.`
            : "Grabación guardada.",
        );
      }
      return result;
    } catch (cause) {
      if (this.#isCurrent(this.#generation)) {
        this.captureNotice = errorFrom(cause);
        this.showToast("No se pudo guardar la grabación.");
      }
      return null;
    }
  }

  #applyRecordingState(state: RecordingStateDto): void {
    this.recording = state;
    if (state.active) {
      if (this.#recordingTimer === null) {
        this.#recordingStartedAt = Date.now();
        this.recordingElapsed = 0;
        this.#recordingTimer = setInterval(() => {
          this.recordingElapsed = Date.now() - this.#recordingStartedAt;
        }, 250);
      }
    } else {
      this.#stopRecordingTimer();
      this.recordingElapsed = 0;
    }
  }

  #stopRecordingTimer(): void {
    if (this.#recordingTimer === null) return;
    clearInterval(this.#recordingTimer);
    this.#recordingTimer = null;
  }

  #cancelReconnect(): void {
    if (this.#reconnectTimer === null) return;
    clearTimeout(this.#reconnectTimer);
    this.#reconnectTimer = null;
  }

  #map(event: PointerEventData | WheelEventData): Point | null {
    const resolution = this.resolution;
    if (resolution === null) return null;

    return mapPoint(
      event.x,
      event.y,
      event.canvasWidth,
      event.canvasHeight,
      resolution.width,
      resolution.height,
      resolution.width,
      resolution.height,
    );
  }

  #sendPointer(action: PointerAction, point: Point, resolution: ResolutionDto): void {
    const input: PointerInput = {
      action,
      x: point.x,
      y: point.y,
      screenWidth: resolution.width,
      screenHeight: resolution.height,
      pressure: action === "up" ? 0 : 1,
      buttons: action === "up" ? 0 : BUTTON_PRIMARY,
      actionButton: action === "move" ? 0 : BUTTON_PRIMARY,
    };
    void this.#send(input, (api) => api.sendPointer(input));
  }

  #sendKey(action: KeyAction, keycode: number, metaState: number): void {
    if (!this.inputEnabled) return;
    const input: KeyInput = { action, keycode, repeat: 0, metaState };
    void this.#send(input, (api) => api.sendKey(input));
  }

  #sendText(text: string): void {
    if (!this.inputEnabled) return;
    void this.#send(text, (api) => api.sendText(text));
  }

  #sendBack(action: KeyAction): void {
    if (!this.inputEnabled) return;
    void this.#send(action, (api) => api.sendBack(action));
  }

  async #send<T>(_value: T, call: (api: LotuApi) => Promise<void>): Promise<void> {
    try {
      await call(this.#api);
    } catch (cause) {
      if (this.#isCurrent(this.#generation)) {
        this.inputNotice = errorFrom(cause);
      }
    }
  }

  #scheduleMove(): void {
    if (this.#rafId !== null || this.#frameTimer !== null) return;

    const flush = () => {
      this.#rafId = null;
      this.#frameTimer = null;
      const pending = this.#pendingMove;
      this.#pendingMove = null;
      if (pending !== null) {
        this.#sendPointer("move", pending.point, pending.resolution);
      }
    };

    if (typeof requestAnimationFrame === "function") {
      this.#rafId = requestAnimationFrame(flush);
    } else {
      this.#frameTimer = setTimeout(flush, 16);
    }
  }

  #cancelGesture(): void {
    this.#gesture = null;
    this.#pendingMove = null;
    if (this.#rafId !== null) {
      cancelAnimationFrame(this.#rafId);
      this.#rafId = null;
    }
    if (this.#frameTimer !== null) {
      clearTimeout(this.#frameTimer);
      this.#frameTimer = null;
    }
  }

  #fail(generation: number, error: ErrorDto, recoverable = false): void {
    if (!this.#isCurrent(generation)) return;

    this.#teardown();

    if (recoverable && this.origin === "live" && this.#lastSerial !== null) {
      const serial = this.#lastSerial;
      this.#reconnectAttempts += 1;
      const attempt = this.#reconnectAttempts;
      this.phase = "connecting";
      this.error = null;
      this.fps = 0;

      if (attempt <= RECONNECT_DELAYS_MS.length) {
        this.showToast(`Conexión perdida. Reconectando… (${attempt}/3)`);
      } else if (!this.#waitingForDevice) {
        // The device may need several seconds to re-enumerate on USB. Keep
        // waiting instead of dropping the user into an error state.
        this.#waitingForDevice = true;
        this.showToast("Esperando al dispositivo…");
      }

      const delay =
        attempt <= RECONNECT_DELAYS_MS.length
          ? RECONNECT_DELAYS_MS[attempt - 1]
          : DEVICE_WAIT_INTERVAL_MS;
      this.#reconnectTimer = setTimeout(() => {
        this.#reconnectTimer = null;
        if (!this.#isCurrent(generation)) return;
        void this.startSession(serial);
      }, delay);
      return;
    }

    this.phase = "error";
    this.error = error;
    this.fps = 0;
  }

  /** Runtime stream/decoder failures use the live session-failure code. */
  #streamFailure(error: StreamError): ErrorDto {
    return {
      code: this.origin === "live" ? "session_failed" : "internal",
      message: error.message,
    };
  }

  #isCurrent(generation: number): boolean {
    return !this.#disposed && generation === this.#generation;
  }

  #startFpsTimer(generation: number): void {
    this.#stopFpsTimer();
    this.fps = 0;
    this.#frameCount = 0;

    this.#fpsTimer = setInterval(() => {
      if (!this.#isCurrent(generation)) {
        this.#stopFpsTimer();
        return;
      }

      this.fps = this.#frameCount;
      this.#frameCount = 0;
    }, 1000);
  }

  #stopFpsTimer(): void {
    if (this.#fpsTimer === null) return;

    clearInterval(this.#fpsTimer);
    this.#fpsTimer = null;
  }

  #teardown(): void {
    this.#stopFpsTimer();
    this.#stopRecordingTimer();
    this.#cancelReconnect();
    if (this.#toastTimer !== null) {
      clearTimeout(this.#toastTimer);
      this.#toastTimer = null;
    }
    this.toast = null;
    this.#frameCount = 0;

    const unlisten = this.#unlistenRecording;
    this.#unlistenRecording = null;
    unlisten?.();

    const client = this.#client;
    this.#client = null;
    client?.close();

    const sink = this.#sink;
    this.#sink = null;
    sink?.dispose();
  }
}

function errorFrom(cause: unknown): ErrorDto {
  return cause instanceof ApiError ? cause.dto : toErrorDto(cause);
}
