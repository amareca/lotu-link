/**
 * Client for the internal video WebSocket served by `src-tauri` (M0b).
 *
 * Wire protocol (docs/stream-protocol.md):
 *   1. One text frame: `{"v":1,"type":"hello","codec":"h264"}`.
 *   2. Binary messages with a 16-byte little-endian header:
 *      u8 type (1 = config, 2 = frame), u8 flags (bit 0 = keyframe),
 *      u16 reserved, i64 pts_us, u32 payload size, then the payload.
 *
 * Config payload: u32 width, u32 height, avcC bytes.
 * Frame payload: H.264 in AVCC format.
 *
 * The module is transport-only: it never touches Tauri, ViewModels or the DOM
 * besides the `WebSocket` API used by the default socket factory.
 */

const HEADER_BYTES = 16;
const MESSAGE_TYPE_CONFIG = 1;
const MESSAGE_TYPE_FRAME = 2;
const FLAG_KEYFRAME = 0x01;
const MIN_AVCC_BYTES = 7;
const MAX_PTS_US = BigInt(Number.MAX_SAFE_INTEGER);

export type StreamErrorCode =
  | "invalid_hello"
  | "truncated"
  | "invalid_message"
  | "out_of_order"
  | "socket_error"
  | "closed"
  | "decoder";

/** Typed stream failure. `code` is stable for the UI and for tests. */
export class StreamError extends Error {
  readonly code: StreamErrorCode;

  constructor(code: StreamErrorCode, message: string) {
    super(message);
    this.name = "StreamError";
    this.code = code;
  }
}

export function toStreamError(cause: unknown): StreamError {
  if (cause instanceof StreamError) return cause;
  if (cause instanceof Error) {
    return new StreamError("invalid_message", cause.message);
  }

  return new StreamError("invalid_message", "Unexpected stream failure");
}

export interface StreamHello {
  readonly v: 1;
  readonly type: "hello";
  readonly codec: "h264";
}

export interface StreamConfigMessage {
  readonly kind: "config";
  readonly width: number;
  readonly height: number;
  /** avcC record without the payload prefix (profile/level/SPS/PPS). */
  readonly avcc: Uint8Array;
}

export interface StreamFrameMessage {
  readonly kind: "frame";
  readonly ptsUs: number;
  readonly keyframe: boolean;
  /** H.264 access unit in AVCC format. */
  readonly data: Uint8Array;
}

export type StreamMessage = StreamConfigMessage | StreamFrameMessage;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Parses the mandatory first text message. Throws a typed `StreamError`. */
export function parseStreamHello(raw: string): StreamHello {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch {
    throw new StreamError("invalid_hello", "Hello is not valid JSON");
  }

  if (!isRecord(value)) {
    throw new StreamError("invalid_hello", "Hello must be a JSON object");
  }
  if (value.v !== 1) {
    throw new StreamError("invalid_hello", "Unsupported protocol version");
  }
  if (value.type !== "hello") {
    throw new StreamError("invalid_hello", "First text message must be hello");
  }
  if (value.codec !== "h264") {
    throw new StreamError("invalid_hello", "Unsupported codec, expected h264");
  }

  return { v: 1, type: "hello", codec: "h264" };
}

function readPtsUs(view: DataView): number {
  const value = view.getBigInt64(4, true);
  if (value > MAX_PTS_US || value < -MAX_PTS_US) {
    throw new StreamError("invalid_message", "PTS out of the safe integer range");
  }

  return Number(value);
}

/**
 * Parses one binary WebSocket message. The buffer must contain exactly one
 * header plus its payload; a shorter payload is reported as `truncated`.
 */
export function parseStreamMessage(payload: ArrayBuffer): StreamMessage {
  if (payload.byteLength < HEADER_BYTES) {
    throw new StreamError(
      "truncated",
      `Message needs ${HEADER_BYTES} header bytes, got ${payload.byteLength}`,
    );
  }

  const view = new DataView(payload);
  const type = view.getUint8(0);
  const flags = view.getUint8(1);
  const ptsUs = readPtsUs(view);
  const size = view.getUint32(12, true);

  if (payload.byteLength !== HEADER_BYTES + size) {
    throw new StreamError(
      "truncated",
      `Expected ${HEADER_BYTES + size} bytes, got ${payload.byteLength}`,
    );
  }

  if (type === MESSAGE_TYPE_CONFIG) {
    if (size < 8) {
      throw new StreamError("invalid_message", "Config payload is too short");
    }

    const bodyView = new DataView(payload, HEADER_BYTES, size);
    const width = bodyView.getUint32(0, true);
    const height = bodyView.getUint32(4, true);
    const avcc = new Uint8Array(payload, HEADER_BYTES + 8, size - 8).slice();

    if (width === 0 || height === 0) {
      throw new StreamError("invalid_message", "Config resolution must be positive");
    }
    if (avcc.byteLength < MIN_AVCC_BYTES) {
      throw new StreamError("invalid_message", "Config avcC is too short");
    }

    return { kind: "config", width, height, avcc };
  }

  if (type === MESSAGE_TYPE_FRAME) {
    // Copy so the decoder never aliases the socket buffer.
    const data = new Uint8Array(payload, HEADER_BYTES, size).slice();

    return {
      kind: "frame",
      ptsUs,
      keyframe: (flags & FLAG_KEYFRAME) !== 0,
      data,
    };
  }

  throw new StreamError("invalid_message", `Unknown message type ${type}`);
}

export interface StreamCloseInfo {
  readonly code: number;
  readonly reason: string;
}

export interface StreamSocketHandlers {
  onMessage(data: string | ArrayBuffer): void;
  onError(error: StreamError): void;
  onClose(info: StreamCloseInfo): void;
}

/** Minimal socket surface; tests inject a fake instead of opening a port. */
export interface StreamSocket {
  close(): void;
}

export type StreamSocketFactory = (
  url: string,
  handlers: StreamSocketHandlers,
) => StreamSocket;

export interface StreamClientHandlers {
  onHello?(hello: StreamHello): void;
  onMessage?(message: StreamMessage): void;
  onError?(error: StreamError): void;
  /** Server-initiated close. Local `close()` calls never notify. */
  onClose?(error: StreamError): void;
}

/** What the SessionViewModel needs from a stream client. */
export interface StreamConnection {
  open(url: string): void;
  close(): void;
}

const browserSocketFactory: StreamSocketFactory = (url, handlers) => {
  const socket = new WebSocket(url);
  socket.binaryType = "arraybuffer";

  socket.onmessage = (event: MessageEvent) => {
    const data: unknown = event.data;
    if (typeof data === "string" || data instanceof ArrayBuffer) {
      handlers.onMessage(data);
    } else {
      handlers.onError(
        new StreamError("invalid_message", "Unsupported WebSocket payload"),
      );
    }
  };
  socket.onerror = () => {
    handlers.onError(new StreamError("socket_error", "WebSocket transport error"));
  };
  socket.onclose = (event: CloseEvent) => {
    handlers.onClose({ code: event.code, reason: event.reason });
  };

  return {
    close: () => {
      socket.onmessage = null;
      socket.onerror = null;
      socket.onclose = null;
      socket.close();
    },
  };
};

/**
 * Enforces the message order (hello → config → frames) and exposes typed
 * messages. Every protocol failure closes the socket exactly once.
 */
export class StreamClient implements StreamConnection {
  readonly #handlers: StreamClientHandlers;
  readonly #factory: StreamSocketFactory;
  #socket: StreamSocket | null = null;
  #closed = true;
  #helloReceived = false;
  #configReceived = false;

  constructor(
    handlers: StreamClientHandlers = {},
    socketFactory: StreamSocketFactory = browserSocketFactory,
  ) {
    this.#handlers = handlers;
    this.#factory = socketFactory;
  }

  get isClosed(): boolean {
    return this.#closed;
  }

  open(url: string): void {
    if (this.#socket !== null) {
      throw new Error("StreamClient is already open");
    }

    this.#closed = false;
    this.#helloReceived = false;
    this.#configReceived = false;
    this.#socket = this.#factory(url, {
      onMessage: (data) => {
        this.#handleData(data);
      },
      onError: (error) => {
        this.#fail(error);
      },
      onClose: (info) => {
        this.#fail(
          new StreamError(
            "closed",
            `Stream closed by the server (${info.code})${
              info.reason ? `: ${info.reason}` : ""
            }`,
          ),
        );
      },
    });

    // A failing factory (or a synchronous socket error) leaves the client closed.
    if (this.#closed) {
      const socket = this.#socket;
      this.#socket = null;
      socket?.close();
    }
  }

  close(): void {
    this.#closed = true;
    const socket = this.#socket;
    this.#socket = null;
    socket?.close();
  }

  #handleData(data: string | ArrayBuffer): void {
    if (this.#closed) return;

    if (typeof data === "string") {
      this.#handleHello(data);
      return;
    }

    if (!this.#helloReceived) {
      this.#fail(new StreamError("out_of_order", "Binary message before hello"));
      return;
    }

    let message: StreamMessage;
    try {
      message = parseStreamMessage(data);
    } catch (cause) {
      this.#fail(toStreamError(cause));
      return;
    }

    if (message.kind === "config") {
      this.#configReceived = true;
    } else if (!this.#configReceived) {
      this.#fail(new StreamError("out_of_order", "Frame before config"));
      return;
    }

    this.#handlers.onMessage?.(message);
  }

  #handleHello(raw: string): void {
    if (this.#helloReceived) {
      this.#fail(new StreamError("out_of_order", "Duplicate hello message"));
      return;
    }

    let hello: StreamHello;
    try {
      hello = parseStreamHello(raw);
    } catch (cause) {
      this.#fail(toStreamError(cause));
      return;
    }

    this.#helloReceived = true;
    this.#handlers.onHello?.(hello);
  }

  #fail(error: StreamError): void {
    if (this.#closed) return;

    this.#closed = true;
    const socket = this.#socket;
    this.#socket = null;
    socket?.close();

    if (error.code === "closed") {
      this.#handlers.onClose?.(error);
    } else {
      this.#handlers.onError?.(error);
    }
  }
}
