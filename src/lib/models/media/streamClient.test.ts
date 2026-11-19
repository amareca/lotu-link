import { describe, expect, it } from "vitest";
import {
  StreamClient,
  StreamError,
  parseStreamHello,
  parseStreamMessage,
  type StreamClientHandlers,
  type StreamErrorCode,
  type StreamHello,
  type StreamMessage,
  type StreamSocket,
  type StreamSocketHandlers,
} from "./streamClient";

const HELLO = JSON.stringify({ v: 1, type: "hello", codec: "h264" });

/** Minimal avcC: version 1, baseline profile, level 3.1, one SPS and one PPS. */
const AVCC = new Uint8Array([
  0x01, 0x42, 0xc0, 0x1f, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x42, 0xc0, 0x1f, 0x01,
  0x00, 0x04, 0x68, 0xce, 0x3c, 0x80,
]);

function buildMessage(
  type: number,
  flags: number,
  ptsUs: number,
  payload: Uint8Array,
): ArrayBuffer {
  const buffer = new ArrayBuffer(16 + payload.byteLength);
  const view = new DataView(buffer);
  view.setUint8(0, type);
  view.setUint8(1, flags);
  view.setUint16(2, 0, true);
  view.setBigInt64(4, BigInt(ptsUs), true);
  view.setUint32(12, payload.byteLength, true);
  new Uint8Array(buffer).set(payload, 16);

  return buffer;
}

function buildConfig(
  width: number,
  height: number,
  avcc: Uint8Array = AVCC,
): ArrayBuffer {
  const payload = new Uint8Array(8 + avcc.byteLength);
  const view = new DataView(payload.buffer);
  view.setUint32(0, width, true);
  view.setUint32(4, height, true);
  payload.set(avcc, 8);

  return buildMessage(1, 0, 0, payload);
}

function buildFrame(ptsUs: number, keyframe: boolean): ArrayBuffer {
  return buildMessage(2, keyframe ? 0x01 : 0x00, ptsUs, new Uint8Array([0x65, 0x88]));
}

function captureError(action: () => unknown): StreamError {
  try {
    action();
  } catch (error) {
    if (error instanceof StreamError) return error;
    throw error;
  }

  throw new Error("Expected a StreamError");
}

class FakeSocket implements StreamSocket {
  readonly handlers: StreamSocketHandlers;
  closed = false;

  constructor(handlers: StreamSocketHandlers) {
    this.handlers = handlers;
  }

  close(): void {
    this.closed = true;
  }
}

interface Harness {
  client: StreamClient;
  socket: FakeSocket;
  hellos: StreamHello[];
  received: StreamMessage[];
  errors: StreamError[];
  closes: StreamError[];
}

function connect(): Harness {
  const sockets: FakeSocket[] = [];
  const hellos: StreamHello[] = [];
  const received: StreamMessage[] = [];
  const errors: StreamError[] = [];
  const closes: StreamError[] = [];

  const handlers: StreamClientHandlers = {
    onHello: (hello) => {
      hellos.push(hello);
    },
    onMessage: (message) => {
      received.push(message);
    },
    onError: (error) => {
      errors.push(error);
    },
    onClose: (info: StreamError) => {
      closes.push(info);
    },
  };

  const client = new StreamClient(handlers, (_url, socketHandlers) => {
    const socket = new FakeSocket(socketHandlers);
    sockets.push(socket);
    return socket;
  });
  client.open("ws://127.0.0.1:45999/stream?token=test");

  const socket = sockets[0];
  if (socket === undefined) throw new Error("The socket was not created");

  return { client, socket, hellos, received, errors, closes };
}

function expectCode(error: StreamError, code: StreamErrorCode): void {
  expect(error).toBeInstanceOf(StreamError);
  expect(error.code).toBe(code);
}

function firstError(errors: readonly StreamError[]): StreamError {
  const error = errors[0];
  if (error === undefined) throw new Error("Expected a stream error");

  return error;
}

describe("parseStreamHello", () => {
  it("accepts the v1 h264 hello", () => {
    expect(parseStreamHello(HELLO)).toEqual({
      v: 1,
      type: "hello",
      codec: "h264",
    });
  });

  it("rejects another protocol version", () => {
    const error = captureError(() =>
      parseStreamHello(JSON.stringify({ v: 2, type: "hello", codec: "h264" })),
    );
    expectCode(error, "invalid_hello");
  });

  it("rejects a non-h264 codec", () => {
    const error = captureError(() =>
      parseStreamHello(JSON.stringify({ v: 1, type: "hello", codec: "av1" })),
    );
    expectCode(error, "invalid_hello");
  });

  it("rejects malformed JSON", () => {
    const error = captureError(() => parseStreamHello("{nope"));
    expectCode(error, "invalid_hello");
  });
});

describe("parseStreamMessage", () => {
  it("parses a config message with dimensions and avcC", () => {
    const message = parseStreamMessage(buildConfig(1080, 2400));
    expect(message.kind).toBe("config");

    if (message.kind !== "config") return;
    expect(message.width).toBe(1080);
    expect(message.height).toBe(2400);
    expect(Array.from(message.avcc)).toEqual(Array.from(AVCC));
  });

  it("parses a keyframe with its PTS", () => {
    const message = parseStreamMessage(buildFrame(0, true));
    expect(message).toMatchObject({ kind: "frame", ptsUs: 0, keyframe: true });
  });

  it("parses a delta frame", () => {
    const message = parseStreamMessage(buildFrame(33_333, false));
    expect(message).toMatchObject({
      kind: "frame",
      ptsUs: 33_333,
      keyframe: false,
    });
  });

  it("reports a truncated header", () => {
    const error = captureError(() => parseStreamMessage(new ArrayBuffer(8)));
    expectCode(error, "truncated");
  });

  it("reports a payload shorter than the declared size", () => {
    const buffer = buildFrame(0, true);
    new DataView(buffer).setUint32(12, 64, true);

    const error = captureError(() => parseStreamMessage(buffer));
    expectCode(error, "truncated");
  });

  it("rejects an unknown message type", () => {
    const error = captureError(() =>
      parseStreamMessage(buildMessage(9, 0, 0, new Uint8Array([1]))),
    );
    expectCode(error, "invalid_message");
  });

  it("rejects a config with a zero resolution", () => {
    const error = captureError(() => parseStreamMessage(buildConfig(0, 2400)));
    expectCode(error, "invalid_message");
  });
});

describe("StreamClient", () => {
  it("accepts hello and forwards typed messages in order", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(HELLO);
    harness.socket.handlers.onMessage(buildConfig(1080, 2400));
    harness.socket.handlers.onMessage(buildFrame(0, true));

    expect(harness.hellos).toHaveLength(1);
    expect(harness.errors).toEqual([]);
    expect(harness.received.map((message) => message.kind)).toEqual([
      "config",
      "frame",
    ]);
    expect(harness.socket.closed).toBe(false);

    harness.client.close();
    expect(harness.socket.closed).toBe(true);
  });

  it("rejects a binary message before hello and closes the socket", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(buildConfig(1080, 2400));

    expect(harness.received).toEqual([]);
    expect(harness.errors).toHaveLength(1);
    expectCode(firstError(harness.errors), "out_of_order");
    expect(harness.socket.closed).toBe(true);
  });

  it("rejects frames before the first config", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(HELLO);
    harness.socket.handlers.onMessage(buildFrame(0, true));

    expect(harness.received).toEqual([]);
    expectCode(firstError(harness.errors), "out_of_order");
  });

  it("rejects a duplicate hello", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(HELLO);
    harness.socket.handlers.onMessage(HELLO);

    expectCode(firstError(harness.errors), "out_of_order");
  });

  it("reports a malformed binary message as truncated", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(HELLO);
    harness.socket.handlers.onMessage(new ArrayBuffer(12));

    expectCode(firstError(harness.errors), "truncated");
    expect(harness.socket.closed).toBe(true);
  });

  it("reports a server close as a typed error", () => {
    const harness = connect();

    harness.socket.handlers.onClose({ code: 1006, reason: "gone" });

    expect(harness.closes).toHaveLength(1);
    expectCode(firstError(harness.closes), "closed");
  });

  it("ignores late messages after a local close", () => {
    const harness = connect();

    harness.client.close();
    harness.socket.handlers.onMessage(HELLO);

    expect(harness.hellos).toEqual([]);
    expect(harness.errors).toEqual([]);
  });

  it("accepts a new configuration mid-stream (rotation)", () => {
    const harness = connect();

    harness.socket.handlers.onMessage(HELLO);
    harness.socket.handlers.onMessage(buildConfig(1080, 2400));
    harness.socket.handlers.onMessage(buildFrame(0, true));
    harness.socket.handlers.onMessage(buildConfig(2400, 1080));

    expect(harness.errors).toEqual([]);
    expect(harness.received.map((message) => message.kind)).toEqual([
      "config",
      "frame",
      "config",
    ]);
  });
});
