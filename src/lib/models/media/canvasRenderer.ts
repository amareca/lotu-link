/**
 * Canvas presentation for decoded video (M0b).
 *
 * `computeLetterbox` is pure and works in CSS pixels so it can be tested in
 * node. `CanvasRenderer` owns the 2D context, the DPR-aware backing store and
 * the `ResizeObserver`; the ViewModel only forwards decoded frames to `draw`.
 *
 * The backing store is never smaller than the decoded video, so downscaling
 * happens once in the browser with high-quality smoothing instead of painting
 * a blurry CSS-sized frame. The last frame is kept as a `VideoFrame` clone so a
 * resize can repaint immediately without waiting for the next decoded frame.
 */

export interface LetterboxRect {
  readonly x: number;
  readonly y: number;
  readonly width: number;
  readonly height: number;
  readonly scale: number;
}

const EMPTY_RECT: LetterboxRect = { x: 0, y: 0, width: 0, height: 0, scale: 0 };

/**
 * Fits `videoWidth × videoHeight` inside `canvasWidth × canvasHeight`,
 * centered and preserving the aspect ratio. All values are CSS pixels.
 */
export function computeLetterbox(
  canvasWidth: number,
  canvasHeight: number,
  videoWidth: number,
  videoHeight: number,
): LetterboxRect {
  if (
    !isPositive(canvasWidth) ||
    !isPositive(canvasHeight) ||
    !isPositive(videoWidth) ||
    !isPositive(videoHeight)
  ) {
    return EMPTY_RECT;
  }

  const scale = Math.min(canvasWidth / videoWidth, canvasHeight / videoHeight);
  const width = videoWidth * scale;
  const height = videoHeight * scale;

  return {
    x: (canvasWidth - width) / 2,
    y: (canvasHeight - height) / 2,
    width,
    height,
    scale,
  };
}

/** Converts a CSS-pixel letterbox rectangle to the canvas backing store. */
export function scaleLetterbox(rect: LetterboxRect, dpr: number): LetterboxRect {
  if (!isPositive(dpr)) return EMPTY_RECT;

  return {
    x: rect.x * dpr,
    y: rect.y * dpr,
    width: rect.width * dpr,
    height: rect.height * dpr,
    scale: rect.scale * dpr,
  };
}

/**
 * Converts a CSS-pixel letterbox rectangle to the backing store using the
 * actual horizontal and vertical ratios (`canvas.width / clientWidth`). The
 * backing store may be larger than CSS×DPR to match the video resolution, so a
 * single DPR is not enough.
 */
function scaleLetterboxAxes(
  rect: LetterboxRect,
  scaleX: number,
  scaleY: number,
): LetterboxRect {
  if (!isPositive(scaleX) || !isPositive(scaleY)) return EMPTY_RECT;

  return {
    x: rect.x * scaleX,
    y: rect.y * scaleY,
    width: rect.width * scaleX,
    height: rect.height * scaleY,
    scale: rect.scale * Math.min(scaleX, scaleY),
  };
}

function isPositive(value: number): boolean {
  return Number.isFinite(value) && value > 0;
}

function closeQuietly(frame: VideoFrame | null): void {
  if (frame === null) return;

  try {
    frame.close();
  } catch {
    // The frame may already be closed or its decoder may be gone.
  }
}

function readFrameSize(
  frame: VideoFrame,
): { width: number; height: number } | null {
  try {
    const width = frame.displayWidth;
    const height = frame.displayHeight;
    if (!isPositive(width) || !isPositive(height)) return null;

    return { width, height };
  } catch {
    // A closed VideoFrame throws when its dimensions are read.
    return null;
  }
}

/** Surface used by the SessionViewModel; faked in unit tests. */
export interface VideoRenderer {
  attach(canvas: HTMLCanvasElement): void;
  detach(canvas?: HTMLCanvasElement): void;
  draw(frame: VideoFrame): void;
  dispose(): void;
}

export class CanvasRenderer implements VideoRenderer {
  #canvas: HTMLCanvasElement | null = null;
  #context: CanvasRenderingContext2D | null = null;
  #observer: ResizeObserver | null = null;
  #dpr = 1;
  #videoWidth = 0;
  #videoHeight = 0;
  #lastFrame: VideoFrame | null = null;
  #disposed = false;

  attach(canvas: HTMLCanvasElement): void {
    if (this.#disposed || this.#canvas === canvas) return;

    this.detach();
    this.#canvas = canvas;
    this.#context = canvas.getContext("2d");
    this.#resize();

    if (typeof ResizeObserver !== "undefined") {
      this.#observer = new ResizeObserver(() => {
        this.#resize();
      });
      this.#observer.observe(canvas);
    }
  }

  /** Detaches the observer. Passing a canvas only detaches that canvas. */
  detach(canvas?: HTMLCanvasElement): void {
    if (canvas !== undefined && this.#canvas !== canvas) return;

    this.#observer?.disconnect();
    this.#observer = null;
    this.#canvas = null;
    this.#context = null;
    this.#videoWidth = 0;
    this.#videoHeight = 0;
    this.#releaseLastFrame();
  }

  draw(frame: VideoFrame): void {
    const canvas = this.#canvas;
    const context = this.#context;
    if (this.#disposed || canvas === null || context === null) return;
    if (frame === null || frame === undefined) return;

    const size = readFrameSize(frame);
    if (size === null) return;

    let clone: VideoFrame;
    try {
      clone = frame.clone();
    } catch {
      // Closed/consumed frames cannot be cloned; skip instead of throwing.
      return;
    }

    this.#videoWidth = size.width;
    this.#videoHeight = size.height;
    this.#releaseLastFrame();
    this.#lastFrame = clone;

    this.#syncBackingStore();
    this.#paint();
  }

  dispose(): void {
    if (this.#disposed) return;

    this.detach();
    this.#disposed = true;
  }

  /** Resizes and repaints from the retained frame; called by the observer. */
  #resize(): void {
    this.#syncBackingStore();
    this.#paint();
  }

  /**
   * Sizes the backing store to at least the CSS size at the current DPR and at
   * least the native video resolution.
   */
  #syncBackingStore(): void {
    const canvas = this.#canvas;
    if (canvas === null) return;

    const dpr =
      typeof window !== "undefined" && window.devicePixelRatio > 0
        ? window.devicePixelRatio
        : 1;
    this.#dpr = dpr;

    const cssWidth = canvas.clientWidth;
    const cssHeight = canvas.clientHeight;
    const width = Math.max(
      1,
      Math.round(Math.max(cssWidth * dpr, this.#videoWidth)),
    );
    const height = Math.max(
      1,
      Math.round(Math.max(cssHeight * dpr, this.#videoHeight)),
    );

    // Assigning width/height clears the canvas; `#paint` always follows.
    if (canvas.width !== width) canvas.width = width;
    if (canvas.height !== height) canvas.height = height;
  }

  #paint(): void {
    const canvas = this.#canvas;
    const context = this.#context;
    if (canvas === null || context === null) return;

    context.clearRect(0, 0, canvas.width, canvas.height);

    const frame = this.#lastFrame;
    if (frame === null) return;

    const rect = computeLetterbox(
      canvas.clientWidth,
      canvas.clientHeight,
      this.#videoWidth,
      this.#videoHeight,
    );
    if (rect.width <= 0 || rect.height <= 0) return;

    const cssWidth = canvas.clientWidth;
    const cssHeight = canvas.clientHeight;
    const scaleX = cssWidth > 0 ? canvas.width / cssWidth : this.#dpr;
    const scaleY = cssHeight > 0 ? canvas.height / cssHeight : this.#dpr;
    const deviceRect = scaleLetterboxAxes(rect, scaleX, scaleY);
    if (deviceRect.width <= 0 || deviceRect.height <= 0) return;

    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = "high";
    context.drawImage(
      frame,
      deviceRect.x,
      deviceRect.y,
      deviceRect.width,
      deviceRect.height,
    );
  }

  #releaseLastFrame(): void {
    const frame = this.#lastFrame;
    this.#lastFrame = null;
    closeQuietly(frame);
  }
}
