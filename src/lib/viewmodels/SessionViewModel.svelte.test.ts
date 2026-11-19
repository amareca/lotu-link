import { afterEach, describe, expect, it, vi } from "vitest";
import type { LotuApi } from "../models/api/api";
import type {
  DeviceDto,
  ReplaySessionDto,
  SessionDto,
} from "../models/api/dto";
import { FAKE_REPLAY_DEVICE, FakeApi } from "../models/api/fakeApi";
import type { VideoRenderer } from "../models/media/canvasRenderer";
import {
  StreamError,
  type StreamClientHandlers,
  type StreamConfigMessage,
  type StreamConnection,
  type StreamFrameMessage,
  type StreamMessage,
} from "../models/media/streamClient";
import type {
  VideoConfig,
  VideoFrameData,
  VideoSink,
  VideoSinkHandlers,
} from "../models/media/webCodecsSink";
import { SessionViewModel, type MediaFactory } from "./SessionViewModel.svelte";

const REPLAY_SESSION: ReplaySessionDto = {
  wsUrl: "ws://127.0.0.1:45999/stream?token=test",
  device: FAKE_REPLAY_DEVICE,
};

const LIVE_DEVICE: DeviceDto = {
  serial: "PIXEL8PRO",
  name: "Pixel 8 Pro",
  state: "connected",
  androidVersion: "15",
  resolution: { width: 1080, height: 2400 },
  batteryPercent: 78,
  transport: "usb",
};

const LIVE_SESSION: SessionDto = {
  wsUrl: "ws://127.0.0.1:45998/stream?token=live",
  device: LIVE_DEVICE,
};

const AVCC = new Uint8Array([
  0x01, 0x64, 0x00, 0x28, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x28,
]);

const CONFIG: StreamConfigMessage = {
  kind: "config",
  width: 1080,
  height: 2400,
  avcc: AVCC,
};

const FRAME: StreamFrameMessage = {
  kind: "frame",
  ptsUs: 33_333,
  keyframe: true,
  data: new Uint8Array([0x65, 0x88]),
};

class FakeRenderer implements VideoRenderer {
  readonly attached: HTMLCanvasElement[] = [];
  readonly detached: HTMLCanvasElement[] = [];
  readonly frames: VideoFrame[] = [];
  detachCalls = 0;
  disposed = false;

  attach(canvas: HTMLCanvasElement): void {
    this.attached.push(canvas);
  }

  detach(canvas?: HTMLCanvasElement): void {
    this.detachCalls += 1;
    if (canvas !== undefined) this.detached.push(canvas);
  }

  draw(frame: VideoFrame): void {
    this.frames.push(frame);
  }

  dispose(): void {
    this.disposed = true;
  }
}

class FakeSink implements VideoSink {
  readonly handlers: VideoSinkHandlers;
  readonly configs: VideoConfig[] = [];
  readonly frames: VideoFrameData[] = [];
  disposed = false;

  constructor(handlers: VideoSinkHandlers) {
    this.handlers = handlers;
  }

  onConfig(config: VideoConfig): void {
    this.configs.push(config);
  }

  onFrame(frame: VideoFrameData): void {
    this.frames.push(frame);
  }

  dispose(): void {
    this.disposed = true;
  }
}

class FakeStreamConnection implements StreamConnection {
  readonly handlers: StreamClientHandlers;
  readonly urls: string[] = [];
  opened = false;
  closed = false;

  constructor(handlers: StreamClientHandlers) {
    this.handlers = handlers;
  }

  open(url: string): void {
    this.urls.push(url);
    this.opened = true;
  }

  close(): void {
    this.closed = true;
  }

  emit(message: StreamMessage): void {
    this.handlers.onMessage?.(message);
  }

  fail(error: StreamError): void {
    this.handlers.onError?.(error);
  }

  serverClose(error: StreamError): void {
    this.handlers.onClose?.(error);
  }
}

interface MediaHarness {
  readonly factory: MediaFactory;
  readonly renderers: FakeRenderer[];
  readonly sinks: FakeSink[];
  readonly clients: FakeStreamConnection[];
}

function createMedia(): MediaHarness {
  const renderers: FakeRenderer[] = [];
  const sinks: FakeSink[] = [];
  const clients: FakeStreamConnection[] = [];

  const factory: MediaFactory = {
    createRenderer: () => {
      const renderer = new FakeRenderer();
      renderers.push(renderer);
      return renderer;
    },
    createSink: (handlers) => {
      const sink = new FakeSink(handlers);
      sinks.push(sink);
      return sink;
    },
    createStreamClient: (handlers) => {
      const client = new FakeStreamConnection(handlers);
      clients.push(client);
      return client;
    },
  };

  return { factory, renderers, sinks, clients };
}

function last<T>(items: readonly T[]): T {
  const item = items[items.length - 1];
  if (item === undefined) throw new Error("Expected at least one item");

  return item;
}

function fakeCanvas(): HTMLCanvasElement {
  return {} as unknown as HTMLCanvasElement;
}

function fakeFrame(): VideoFrame {
  return { displayWidth: 1080, displayHeight: 2400 } as unknown as VideoFrame;
}

afterEach(() => {
  vi.useRealTimers();
});

describe("SessionViewModel", () => {
  it("starts the replay, streams config/frames and stops cleanly", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();

    expect(session.phase).toBe("connecting");
    expect(session.device).toEqual(REPLAY_SESSION.device);
    expect(session.resolution).toBeNull();
    expect(session.errorMessage).toBeNull();
    expect(media.renderers).toHaveLength(1);
    expect(media.sinks).toHaveLength(1);
    expect(media.clients).toHaveLength(1);

    const client = last(media.clients);
    const sink = last(media.sinks);
    expect(client.opened).toBe(true);
    expect(client.urls).toEqual([REPLAY_SESSION.wsUrl]);
    expect(sink.disposed).toBe(false);

    client.emit(CONFIG);
    expect(session.resolution).toEqual({ width: 1080, height: 2400 });
    expect(session.phase).toBe("connecting");
    expect(last(sink.configs).avcc).toBe(AVCC);

    client.emit(FRAME);
    expect(session.phase).toBe("streaming");
    expect(sink.frames).toHaveLength(1);
    expect(last(sink.frames)).toMatchObject({
      ptsUs: 33_333,
      keyframe: true,
    });

    session.stop();

    expect(session.phase).toBe("idle");
    expect(session.device).toBeNull();
    expect(session.resolution).toBeNull();
    expect(session.fps).toBe(0);
    expect(client.closed).toBe(true);
    expect(sink.disposed).toBe(true);
    // The renderer survives stop so the canvas binding keeps working.
    expect(last(media.renderers).disposed).toBe(false);

    client.emit(FRAME);
    expect(session.phase).toBe("idle");
    expect(sink.frames).toHaveLength(1);

    session.dispose();
    expect(last(media.renderers).disposed).toBe(true);
  });

  it("surfaces a start_replay failure as a typed error", async () => {
    const api = new FakeApi({
      replayFailure: {
        code: "replay_unavailable",
        message: "Replay requires the Tauri backend",
      },
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();

    expect(session.phase).toBe("error");
    expect(session.errorMessage).toBe("Replay requires the Tauri backend");
    expect(session.device).toBeNull();
    expect(media.clients).toHaveLength(0);
    expect(media.sinks).toHaveLength(0);

    session.dispose();
  });

  it("normalizes an unexpected start failure", async () => {
    const api: LotuApi = {
      listDevices: async () => [],
      startReplay: async () => {
        throw new Error("unexpected boom");
      },
      startSession: async () => {
        throw new Error("live start is not expected");
      },
      stopSession: async () => {},
      sendPointer: async () => {},
      sendScroll: async () => {},
      sendKey: async () => {},
      sendText: async () => {},
      sendBack: async () => {},
      rotate: async () => {},
      pasteText: async () => {},
      copyText: async () => "",
      captureScreenshot: async () => ({ path: "capture.png", width: 1080, height: 2400 }),
      startRecording: async () => ({
        path: "clip.mp4",
        frames: 0,
        durationUs: 0,
        segments: 1,
        paths: ["clip.mp4"],
      }),
      stopRecording: async () => ({
        path: "clip.mp4",
        frames: 1,
        durationUs: 16_000,
        segments: 1,
        paths: ["clip.mp4"],
      }),
      onRecordingState: () => () => {},
      getSettings: async () => ({
        video: { maxSize: 0, maxFps: 60, bitRate: 8_000_000 },
        restartRequired: false,
        onboardingSeen: false,
      }),
      saveSettings: async (video) => ({
        video,
        restartRequired: false,
        onboardingSeen: false,
      }),
      markOnboardingSeen: async () => ({
        video: { maxSize: 0, maxFps: 60, bitRate: 8_000_000 },
        restartRequired: false,
        onboardingSeen: true,
      }),
      getRsaFingerprint: async () => ({
        fingerprint: "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99",
        error: null,
      }),
    };
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();

    expect(session.phase).toBe("error");
    expect(session.errorMessage).toBe("unexpected boom");

    session.dispose();
  });

  it("moves to error and tears down when the stream fails", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startReplay();

    const client = last(media.clients);
    const sink = last(media.sinks);
    client.emit(CONFIG);
    client.emit(FRAME);
    expect(session.phase).toBe("streaming");

    client.fail(new StreamError("socket_error", "transport error"));

    expect(session.phase).toBe("error");
    expect(session.errorMessage).toBe("transport error");
    expect(session.fps).toBe(0);
    expect(client.closed).toBe(true);
    expect(sink.disposed).toBe(true);

    session.dispose();
  });

  it("moves to error when the server closes the stream", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startReplay();

    last(media.clients).serverClose(
      new StreamError("closed", "Stream closed by the server (1006)"),
    );

    expect(session.phase).toBe("error");
    expect(session.errorMessage).toBe("Stream closed by the server (1006)");
    expect(last(media.sinks).disposed).toBe(true);

    session.dispose();
  });

  it("moves to error when the decoder fails", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startReplay();

    last(media.sinks).handlers.onError(
      new StreamError("decoder", "unsupported profile"),
    );

    expect(session.phase).toBe("error");
    expect(session.errorMessage).toBe("unsupported profile");
    expect(last(media.clients).closed).toBe(true);

    session.dispose();
  });

  it("replaces the client, sink and session state on restart", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();
    const firstClient = last(media.clients);
    const firstSink = last(media.sinks);

    await session.startReplay();

    expect(media.clients).toHaveLength(2);
    expect(media.sinks).toHaveLength(2);
    expect(firstClient.closed).toBe(true);
    expect(firstSink.disposed).toBe(true);
    expect(session.phase).toBe("connecting");
    expect(session.device).toEqual(REPLAY_SESSION.device);
    // A single renderer is reused across sessions.
    expect(media.renderers).toHaveLength(1);

    // Late messages from the replaced stream are ignored.
    firstClient.emit(CONFIG);
    expect(session.resolution).toBeNull();

    const secondClient = last(media.clients);
    secondClient.emit(CONFIG);
    expect(session.resolution).toEqual({ width: 1080, height: 2400 });

    session.dispose();
  });

  it("ignores a session that resolves after dispose", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION, delayMs: 10 });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    const pending = session.startReplay();
    session.dispose();
    await pending;

    expect(session.phase).toBe("idle");
    expect(session.device).toBeNull();
    expect(session.errorMessage).toBeNull();
    expect(media.clients).toHaveLength(0);
    expect(media.sinks).toHaveLength(0);
  });

  it("ignores a session that resolves after stop", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION, delayMs: 10 });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    const pending = session.startReplay();
    session.stop();
    await pending;

    expect(session.phase).toBe("idle");
    expect(session.device).toBeNull();
    expect(media.clients).toHaveLength(0);
    expect(media.sinks).toHaveLength(0);

    session.dispose();
  });

  it("reports frames per second with fake timers", async () => {
    vi.useFakeTimers();
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();
    const client = last(media.clients);
    client.emit(CONFIG);

    client.emit(FRAME);
    client.emit(FRAME);
    client.emit(FRAME);
    expect(session.fps).toBe(0);

    vi.advanceTimersByTime(1000);
    expect(session.fps).toBe(3);

    vi.advanceTimersByTime(1000);
    expect(session.fps).toBe(0);

    client.emit(FRAME);
    client.emit(FRAME);
    vi.advanceTimersByTime(999);
    expect(session.fps).toBe(0);
    vi.advanceTimersByTime(1);
    expect(session.fps).toBe(2);

    session.stop();
    expect(vi.getTimerCount()).toBe(0);

    session.dispose();
  });

  it("attaches and detaches the mirror canvas through the renderer", () => {
    const media = createMedia();
    const session = new SessionViewModel(
      new FakeApi({ replay: REPLAY_SESSION }),
      media.factory,
    );
    const canvas = fakeCanvas();

    const detach = session.attachCanvas(canvas);

    expect(media.renderers).toHaveLength(1);
    const renderer = last(media.renderers);
    expect(renderer.attached).toEqual([canvas]);

    detach();
    expect(renderer.detachCalls).toBe(1);
    expect(renderer.detached).toEqual([canvas]);

    session.dispose();
  });

  it("routes decoded frames to the attached renderer", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startReplay();

    const canvas = fakeCanvas();
    const detach = session.attachCanvas(canvas);
    const renderer = last(media.renderers);
    expect(renderer.attached).toEqual([canvas]);

    const frame = fakeFrame();
    last(media.sinks).handlers.draw(frame);

    expect(renderer.frames).toEqual([frame]);

    detach();
    session.dispose();
  });

  it("ignores attachCanvas after dispose", () => {
    const media = createMedia();
    const session = new SessionViewModel(
      new FakeApi({ replay: REPLAY_SESSION }),
      media.factory,
    );
    session.dispose();

    const detach = session.attachCanvas(fakeCanvas());

    expect(media.renderers).toHaveLength(0);
    detach();
  });
});

describe("SessionViewModel live sessions", () => {
  it("starts a live session and reaches streaming", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    const pending = session.startSession("PIXEL8PRO");
    expect(session.phase).toBe("connecting");
    expect(session.origin).toBe("live");
    expect(session.device).toBeNull();

    await pending;

    expect(session.phase).toBe("connecting");
    expect(session.origin).toBe("live");
    expect(session.device).toEqual(LIVE_SESSION.device);
    expect(session.error).toBeNull();
    expect(session.errorMessage).toBeNull();
    expect(api.startSessionCalls).toEqual(["PIXEL8PRO"]);
    expect(media.renderers).toHaveLength(1);
    expect(media.sinks).toHaveLength(1);
    expect(media.clients).toHaveLength(1);

    const client = last(media.clients);
    const sink = last(media.sinks);
    expect(client.urls).toEqual([LIVE_SESSION.wsUrl]);

    client.emit(CONFIG);
    expect(session.resolution).toEqual({ width: 1080, height: 2400 });
    expect(last(sink.configs).avcc).toBe(AVCC);

    client.emit(FRAME);
    expect(session.phase).toBe("streaming");
    expect(sink.frames).toHaveLength(1);

    session.stop();
    expect(session.phase).toBe("idle");
    expect(session.origin).toBeNull();
    expect(session.device).toBeNull();
    expect(api.stopSessionCalls).toBe(1);

    session.dispose();
  });

  it("surfaces a typed start_session failure", async () => {
    const api = new FakeApi({
      sessionFailure: {
        code: "device_not_connected",
        message: "device is not connected",
      },
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startSession("GALAXYS23");

    expect(session.phase).toBe("error");
    expect(session.origin).toBe("live");
    expect(session.error).toEqual({
      code: "device_not_connected",
      message: "device is not connected",
    });
    expect(session.errorMessage).toBe("device is not connected");
    expect(session.device).toBeNull();
    expect(media.clients).toHaveLength(0);
    expect(media.sinks).toHaveLength(0);
    expect(api.startSessionCalls).toEqual(["GALAXYS23"]);

    session.dispose();
  });

  it("calls stopSession only when the origin is live", async () => {
    const api = new FakeApi({ session: LIVE_SESSION, replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startReplay();
    expect(session.origin).toBe("replay");
    session.stop();
    expect(api.stopSessionCalls).toBe(0);

    await session.startSession("PIXEL8PRO");
    expect(session.origin).toBe("live");
    session.stop();
    expect(api.stopSessionCalls).toBe(1);

    session.dispose();
    expect(api.stopSessionCalls).toBe(1);
  });

  it("calls stopSession when disposing a live session", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startSession("PIXEL8PRO");
    session.dispose();

    expect(session.phase).toBe("idle");
    expect(session.origin).toBeNull();
    expect(api.stopSessionCalls).toBe(1);
  });

  it("ignores a live session that resolves after stop", async () => {
    const api = new FakeApi({ session: LIVE_SESSION, delayMs: 10 });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    const pending = session.startSession("PIXEL8PRO");
    session.stop();
    await pending;

    expect(session.phase).toBe("idle");
    expect(session.origin).toBeNull();
    expect(session.device).toBeNull();
    expect(session.errorMessage).toBeNull();
    expect(media.clients).toHaveLength(0);
    expect(media.sinks).toHaveLength(0);
    // The pending live start was stopped while it was already the live origin.
    expect(api.stopSessionCalls).toBe(1);

    session.dispose();
  });

  it("retries the last live serial after a failure", async () => {
    const api = new FakeApi({
      sessionFailure: { code: "session_timeout", message: "timed out" },
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);

    await session.startSession("PIXEL8PRO");
    expect(session.error?.code).toBe("session_timeout");

    api.setSessionFailure(null);
    api.setSession(LIVE_SESSION);
    await session.retry();

    expect(api.startSessionCalls).toEqual(["PIXEL8PRO", "PIXEL8PRO"]);
    expect(session.phase).toBe("connecting");
    expect(session.origin).toBe("live");
    expect(session.error).toBeNull();
    expect(session.device).toEqual(LIVE_SESSION.device);

    session.dispose();
  });

  it("maps an unexpected live stream close to session_failed", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");

    last(media.clients).serverClose(
      new StreamError("closed", "Stream closed by the server (1006)"),
    );

    expect(session.phase).toBe("error");
    expect(session.origin).toBe("live");
    expect(session.error?.code).toBe("session_failed");
    expect(session.errorMessage).toBe("Stream closed by the server (1006)");
    expect(last(media.sinks).disposed).toBe(true);

    session.dispose();
  });

  it("keeps stream frames out of reactive state", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");

    const client = last(media.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    // The frame is owned by the sink; the ViewModel only tracks presentation.
    expect(session).not.toHaveProperty("frames");
    expect(session).not.toHaveProperty("frame");
    expect(session).not.toHaveProperty("lastFrame");
    expect(Object.keys(session)).not.toContain("frames");
    expect(last(media.sinks).frames).toHaveLength(1);

    session.dispose();
  });

  it("sends a pointer gesture with one coalesced move per frame", async () => {
    vi.useFakeTimers();
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");
    const client = last(media.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    expect(session.inputEnabled).toBe(true);
    const base = {
      canvasWidth: 322,
      canvasHeight: 700,
      pointerId: 7,
      button: 0,
    };
    session.pointerDown({ ...base, x: 160, y: 350 });
    await Promise.resolve();
    session.pointerMove({ ...base, x: 170, y: 360 });
    session.pointerMove({ ...base, x: 180, y: 370 });
    expect(api.pointerCalls).toHaveLength(1);
    expect(api.pointerCalls[0]?.action).toBe("down");

    vi.advanceTimersByTime(20);
    await Promise.resolve();
    expect(api.pointerCalls).toHaveLength(2);
    expect(api.pointerCalls[1]?.action).toBe("move");

    session.pointerUp({ ...base, x: 180, y: 370 });
    await Promise.resolve();
    expect(api.pointerCalls).toHaveLength(3);
    expect(api.pointerCalls[2]?.action).toBe("up");
    expect(api.pointerCalls[2]?.pressure).toBe(0);

    session.dispose();
    vi.useRealTimers();
  });

  it("maps wheel, keyboard, navigation and rotate to API calls", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");
    const client = last(media.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    session.wheel({
      x: 160,
      y: 350,
      canvasWidth: 322,
      canvasHeight: 700,
      deltaX: 0,
      deltaY: -100,
      deltaMode: 0,
    });
    await Promise.resolve();
    expect(api.scrollCalls[0]?.vScroll).toBe(1);

    expect(
      session.keyDown({
        key: "a",
        location: 0,
        shiftKey: false,
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        isComposing: false,
        capsLock: false,
        altGraph: false,
      }),
    ).toBe(true);
    await Promise.resolve();
    expect(api.textCalls).toEqual(["a"]);

    expect(
      session.keyDown({
        key: "ArrowDown",
        location: 0,
        shiftKey: false,
        ctrlKey: false,
        altKey: false,
        metaKey: false,
        isComposing: false,
        capsLock: false,
        altGraph: false,
      }),
    ).toBe(true);
    await Promise.resolve();
    expect(api.keyCalls[0]?.keycode).toBe(20);

    session.back();
    await Promise.resolve();
    expect(api.backCalls).toEqual(["down", "up"]);
    session.home();
    await Promise.resolve();
    expect(api.keyCalls.at(-2)?.keycode).toBe(3);
    session.recents();
    await Promise.resolve();
    expect(api.keyCalls.at(-2)?.keycode).toBe(187);
    session.rotate();
    await Promise.resolve();
    expect(api.rotateCalls).toBe(1);

    session.dispose();
  });

  it("does not send input in replay and keeps the phase on input failure", async () => {
    const api = new FakeApi({ replay: REPLAY_SESSION });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startReplay();
    last(media.clients).emit(CONFIG);
    last(media.clients).emit(FRAME);

    session.pointerDown({
      x: 10,
      y: 10,
      canvasWidth: 100,
      canvasHeight: 200,
      pointerId: 1,
      button: 0,
    });
    session.back();
    expect(api.pointerCalls).toHaveLength(0);
    expect(api.backCalls).toHaveLength(0);

    session.dispose();

    const failing = new FakeApi({
      session: LIVE_SESSION,
      inputFailure: { code: "input_failed", message: "writer closed" },
    });
    const liveMedia = createMedia();
    const live = new SessionViewModel(failing, liveMedia.factory);
    await live.startSession("PIXEL8PRO");
    const client = last(liveMedia.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    live.rotate();
    expect(live.phase).toBe("streaming");
    await vi.waitFor(() => expect(live.inputNotice?.code).toBe("input_failed"));

    live.dispose();
  });

  it("copies device text and pastes PC text through the API", async () => {
    const api = new FakeApi({
      session: LIVE_SESSION,
      clipboardText: "texto del móvil",
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");
    const client = last(media.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    expect(await session.copyText()).toBe("texto del móvil");
    expect(api.copyCalls).toBe(1);

    session.pasteText("texto del PC");
    await vi.waitFor(() => expect(api.pasteCalls).toEqual(["texto del PC"]));

    session.dispose();
  });

  it("captures a PNG and drives the MP4 recording lifecycle", async () => {
    const api = new FakeApi({
      session: LIVE_SESSION,
      screenshotResult: {
        path: "captures/shot.png",
        width: 1080,
        height: 2400,
      },
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");
    const client = last(media.clients);
    client.emit(CONFIG);
    client.emit(FRAME);

    expect(await session.capture()).toEqual({
      path: "captures/shot.png",
      width: 1080,
      height: 2400,
    });
    expect(api.captureCalls).toBe(1);

    const started = await session.startRecording();
    expect(started?.frames).toBe(0);
    expect(api.startRecordingCalls).toBe(1);
    expect(session.recording?.active).toBe(true);

    const stopped = await session.stopRecording();
    expect(stopped?.frames).toBe(120);
    expect(api.stopRecordingCalls).toBe(1);
    expect(session.recording?.active).toBe(false);

    session.dispose();
  });

  it("reports capture failures without changing the session phase", async () => {
    const api = new FakeApi({
      session: LIVE_SESSION,
      captureFailure: { code: "capture_failed", message: "disk full" },
    });
    const media = createMedia();
    const session = new SessionViewModel(api, media.factory);
    await session.startSession("PIXEL8PRO");
    last(media.clients).emit(CONFIG);
    last(media.clients).emit(FRAME);

    expect(await session.capture()).toBeNull();
    expect(session.captureNotice?.code).toBe("capture_failed");
    expect(session.phase).toBe("streaming");

    session.dispose();
  });
});
