import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  CanvasRenderer,
  computeLetterbox,
  scaleLetterbox,
  type LetterboxRect,
} from "./canvasRenderer";

const EMPTY_RECT: LetterboxRect = {
  x: 0,
  y: 0,
  width: 0,
  height: 0,
  scale: 0,
};

function expectRect(actual: LetterboxRect, expected: LetterboxRect): void {
  expect(actual.x).toBeCloseTo(expected.x, 6);
  expect(actual.y).toBeCloseTo(expected.y, 6);
  expect(actual.width).toBeCloseTo(expected.width, 6);
  expect(actual.height).toBeCloseTo(expected.height, 6);
  expect(actual.scale).toBeCloseTo(expected.scale, 6);
}

describe("computeLetterbox", () => {
  it("pillarboxes a vertical video with side bars", () => {
    const rect = computeLetterbox(800, 600, 1080, 2400);

    expectRect(rect, {
      x: 265,
      y: 0,
      width: 270,
      height: 600,
      scale: 0.25,
    });
  });

  it("letterboxes a horizontal video with top and bottom bars", () => {
    const rect = computeLetterbox(800, 600, 2400, 1080);

    expectRect(rect, {
      x: 0,
      y: 120,
      width: 800,
      height: 360,
      scale: 800 / 2400,
    });
  });

  it("fits a portrait video into an extreme wide window", () => {
    const rect = computeLetterbox(10000, 10, 100, 1000);

    expectRect(rect, {
      x: 4999.5,
      y: 0,
      width: 1,
      height: 10,
      scale: 0.01,
    });
  });

  it("fits a 4K video into a 1x1 window", () => {
    const rect = computeLetterbox(1, 1, 3840, 2160);

    expectRect(rect, {
      x: 0,
      y: 0.21875,
      width: 1,
      height: 0.5625,
      scale: 1 / 3840,
    });
  });

  it("centers the frame without changing the aspect ratio", () => {
    const rect = computeLetterbox(500, 900, 1080, 2400);

    expect(rect.width / rect.height).toBeCloseTo(1080 / 2400, 10);
    expect(rect.x * 2 + rect.width).toBeCloseTo(500, 10);
    expect(rect.y * 2 + rect.height).toBeCloseTo(900, 10);
  });

  it("returns an empty rect for degenerate inputs", () => {
    const degenerate: ReadonlyArray<readonly [number, number, number, number]> = [
      [0, 600, 1080, 2400],
      [800, 0, 1080, 2400],
      [800, 600, 0, 2400],
      [800, 600, 1080, 0],
      [-800, 600, 1080, 2400],
      [Number.NaN, 600, 1080, 2400],
      [800, Number.POSITIVE_INFINITY, 1080, 2400],
    ];

    for (const [canvasWidth, canvasHeight, videoWidth, videoHeight] of degenerate) {
      expect(computeLetterbox(canvasWidth, canvasHeight, videoWidth, videoHeight)).toEqual(
        EMPTY_RECT,
      );
    }
  });
});

describe("scaleLetterbox", () => {
  it("scales a vertical-video rect to the DPR backing store", () => {
    const rect = computeLetterbox(800, 600, 1080, 2400);
    const deviceRect = scaleLetterbox(rect, 2);

    expectRect(deviceRect, {
      x: 530,
      y: 0,
      width: 540,
      height: 1200,
      scale: 0.5,
    });
  });

  it("scales a horizontal-video rect to the DPR backing store", () => {
    const rect = computeLetterbox(800, 600, 2400, 1080);
    const deviceRect = scaleLetterbox(rect, 2);

    expectRect(deviceRect, {
      x: 0,
      y: 240,
      width: 1600,
      height: 720,
      scale: 1600 / 2400,
    });
  });

  it("supports a fractional DPR", () => {
    const deviceRect = scaleLetterbox(
      { x: 10, y: 20, width: 30, height: 40, scale: 0.5 },
      1.5,
    );

    expectRect(deviceRect, {
      x: 15,
      y: 30,
      width: 45,
      height: 60,
      scale: 0.75,
    });
  });

  it("returns an empty rect for an invalid DPR", () => {
    const rect = computeLetterbox(800, 600, 1080, 2400);
    const invalidDprs = [0, -1, Number.NaN, Number.POSITIVE_INFINITY];

    for (const dpr of invalidDprs) {
      expect(scaleLetterbox(rect, dpr)).toEqual(EMPTY_RECT);
    }
  });
});

interface DrawCall {
  readonly image: unknown;
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
}

class FakeContext {
  imageSmoothingEnabled = false;
  imageSmoothingQuality: ImageSmoothingQuality = "low";
  readonly draws: DrawCall[] = [];
  clears = 0;

  drawImage(
    image: unknown,
    x: number,
    y: number,
    width: number,
    height: number,
  ): void {
    this.draws.push({ image, x, y, width, height });
  }

  clearRect(): void {
    this.clears += 1;
  }
}

class FakeCanvas {
  clientWidth: number;
  clientHeight: number;
  width = 0;
  height = 0;
  readonly context = new FakeContext();

  constructor(clientWidth: number, clientHeight: number) {
    this.clientWidth = clientWidth;
    this.clientHeight = clientHeight;
  }

  getContext(type: string): CanvasRenderingContext2D | null {
    return type === "2d"
      ? (this.context as unknown as CanvasRenderingContext2D)
      : null;
  }

  asCanvas(): HTMLCanvasElement {
    return this as unknown as HTMLCanvasElement;
  }
}

class FakeVideoFrame {
  readonly displayWidth: number;
  readonly displayHeight: number;
  readonly copies: FakeVideoFrame[] = [];
  closed = false;

  constructor(displayWidth: number, displayHeight: number) {
    this.displayWidth = displayWidth;
    this.displayHeight = displayHeight;
  }

  clone(): FakeVideoFrame {
    if (this.closed) throw new Error("InvalidStateError: frame is closed");

    const copy = new FakeVideoFrame(this.displayWidth, this.displayHeight);
    this.copies.push(copy);

    return copy;
  }

  close(): void {
    this.closed = true;
  }

  asFrame(): VideoFrame {
    return this as unknown as VideoFrame;
  }
}

class BrokenFrame {
  get displayWidth(): number {
    throw new Error("InvalidStateError: frame is closed");
  }

  get displayHeight(): number {
    throw new Error("InvalidStateError: frame is closed");
  }

  clone(): VideoFrame {
    throw new Error("InvalidStateError: frame is closed");
  }

  close(): void {
    // No-op: the fake never owned resources.
  }

  asFrame(): VideoFrame {
    return this as unknown as VideoFrame;
  }
}

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];

  readonly callback: ResizeObserverCallback;
  observed: Element | null = null;
  disconnected = false;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }

  observe(target: Element): void {
    this.observed = target;
  }

  unobserve(): void {
    this.observed = null;
  }

  disconnect(): void {
    this.disconnected = true;
    this.observed = null;
  }

  trigger(): void {
    this.callback([], this as unknown as ResizeObserver);
  }

  static reset(): void {
    FakeResizeObserver.instances = [];
  }
}

const originalWindow = Object.getOwnPropertyDescriptor(globalThis, "window");
const originalResizeObserver = Object.getOwnPropertyDescriptor(
  globalThis,
  "ResizeObserver",
);

function restoreGlobal(name: string, descriptor?: PropertyDescriptor): void {
  if (descriptor === undefined) {
    Reflect.deleteProperty(globalThis, name);
    return;
  }

  Object.defineProperty(globalThis, name, descriptor);
}

function setDevicePixelRatio(value: number): void {
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    writable: true,
    value: { devicePixelRatio: value },
  });
}

function last<T>(items: readonly T[]): T {
  const item = items.at(-1);
  if (item === undefined) throw new Error("Expected at least one item");

  return item;
}

beforeEach(() => {
  FakeResizeObserver.reset();
  Object.defineProperty(globalThis, "ResizeObserver", {
    configurable: true,
    writable: true,
    value: FakeResizeObserver,
  });
  setDevicePixelRatio(1);
});

afterEach(() => {
  restoreGlobal("window", originalWindow);
  restoreGlobal("ResizeObserver", originalResizeObserver);
});

describe("CanvasRenderer", () => {
  it("repaints the last frame when the canvas is resized", () => {
    const canvas = new FakeCanvas(420, 860);
    const renderer = new CanvasRenderer();
    renderer.attach(canvas.asCanvas());

    const frame = new FakeVideoFrame(200, 300);
    renderer.draw(frame.asFrame());

    expect(canvas.context.draws).toHaveLength(1);
    expect(canvas.width).toBe(420);
    expect(canvas.height).toBe(860);

    const clone = last(frame.copies);
    canvas.clientWidth = 500;
    canvas.clientHeight = 900;
    last(FakeResizeObserver.instances).trigger();

    expect(canvas.width).toBe(500);
    expect(canvas.height).toBe(900);
    expect(canvas.context.draws).toHaveLength(2);
    expect(last(canvas.context.draws).image).toBe(clone);
    expect(clone.closed).toBe(false);
  });

  it("keeps the backing store at least as large as the video", () => {
    const canvas = new FakeCanvas(120, 200);
    const renderer = new CanvasRenderer();
    renderer.attach(canvas.asCanvas());
    renderer.draw(new FakeVideoFrame(1080, 2340).asFrame());

    expect(canvas.width).toBe(1080);
    expect(canvas.height).toBe(2340);
    expect(canvas.context.imageSmoothingEnabled).toBe(true);
    expect(canvas.context.imageSmoothingQuality).toBe("high");
  });

  it("uses the backing-store ratio instead of DPR alone", () => {
    const canvas = new FakeCanvas(420, 860);
    const renderer = new CanvasRenderer();
    renderer.attach(canvas.asCanvas());
    renderer.draw(new FakeVideoFrame(1080, 2340).asFrame());

    const rect = computeLetterbox(420, 860, 1080, 2340);
    const draw = last(canvas.context.draws);

    expect(draw.width).toBeCloseTo(rect.width * (1080 / 420), 6);
    expect(draw.height).toBeCloseTo(rect.height * (2340 / 860), 6);
    expect(draw.width).not.toBeCloseTo(rect.width, 3);
  });

  it("closes the previous clone when a new frame arrives", () => {
    const canvas = new FakeCanvas(420, 860);
    const renderer = new CanvasRenderer();
    renderer.attach(canvas.asCanvas());

    const firstFrame = new FakeVideoFrame(1080, 2340);
    renderer.draw(firstFrame.asFrame());
    const firstClone = last(firstFrame.copies);

    const secondFrame = new FakeVideoFrame(1080, 2340);
    renderer.draw(secondFrame.asFrame());
    const secondClone = last(secondFrame.copies);

    expect(firstClone.closed).toBe(true);
    expect(secondClone.closed).toBe(false);
  });

  it("closes the last clone on detach and dispose", () => {
    const canvas = new FakeCanvas(420, 860);
    const renderer = new CanvasRenderer();

    renderer.attach(canvas.asCanvas());
    const detachedFrame = new FakeVideoFrame(1080, 2340);
    renderer.draw(detachedFrame.asFrame());
    const detachedClone = last(detachedFrame.copies);

    renderer.detach(canvas.asCanvas());
    expect(detachedClone.closed).toBe(true);

    renderer.attach(canvas.asCanvas());
    const disposedFrame = new FakeVideoFrame(1080, 2340);
    renderer.draw(disposedFrame.asFrame());
    const disposedClone = last(disposedFrame.copies);

    renderer.dispose();
    expect(disposedClone.closed).toBe(true);
  });

  it("ignores null, closed and broken frames without throwing", () => {
    const canvas = new FakeCanvas(420, 860);
    const renderer = new CanvasRenderer();
    renderer.attach(canvas.asCanvas());

    expect(() => renderer.draw(null as unknown as VideoFrame)).not.toThrow();

    const closed = new FakeVideoFrame(1080, 2340);
    closed.close();
    expect(() => renderer.draw(closed.asFrame())).not.toThrow();
    expect(() => renderer.draw(new BrokenFrame().asFrame())).not.toThrow();
    expect(canvas.context.draws).toHaveLength(0);

    const valid = new FakeVideoFrame(1080, 2340);
    renderer.draw(valid.asFrame());
    const clone = last(valid.copies);

    expect(() => renderer.draw(closed.asFrame())).not.toThrow();
    expect(canvas.context.draws).toHaveLength(1);
    expect(clone.closed).toBe(false);

    canvas.clientWidth = 500;
    last(FakeResizeObserver.instances).trigger();
    expect(canvas.context.draws).toHaveLength(2);
  });
});
