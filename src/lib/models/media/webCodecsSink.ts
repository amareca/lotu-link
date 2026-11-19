import { StreamError } from "./streamClient";

/**
 * WebCodecs H.264 sink (M0b).
 *
 * A configuration message recreates the `VideoDecoder` with the avcC record;
 * frames become `EncodedVideoChunk`s whose timestamp is the PTS in µs. Decoded
 * frames go to the renderer and are always closed before returning.
 *
 * WebCodecs globals are only touched by the default `CodecAdapter`, so tests
 * (node environment) inject a fake adapter and never need the DOM.
 */

export interface VideoConfig {
  readonly width: number;
  readonly height: number;
  readonly avcc: Uint8Array;
}

export interface VideoFrameData {
  readonly ptsUs: number;
  readonly keyframe: boolean;
  readonly data: Uint8Array;
}

/** Contract consumed by the SessionViewModel. */
export interface VideoSink {
  onConfig(config: VideoConfig): void;
  onFrame(frame: VideoFrameData): void;
  dispose(): void;
}

export interface VideoSinkHandlers {
  draw(frame: VideoFrame): void;
  onError(error: StreamError): void;
}

export interface VideoDecoderConfigLike {
  codec: string;
  description: Uint8Array;
  optimizeForLatency: boolean;
}

export interface VideoChunkLike {
  readonly type: "key" | "delta";
  readonly timestamp: number;
}

export interface VideoChunkInit {
  type: "key" | "delta";
  timestamp: number;
  data: Uint8Array;
}

export interface VideoDecoderInitLike {
  output(frame: VideoFrame): void;
  error(error: DOMException): void;
}

export interface VideoDecoderLike {
  configure(config: VideoDecoderConfigLike): void;
  decode(chunk: VideoChunkLike): void;
  close(): void;
}

/** Browser codec surface, injectable so tests can use doubles. */
export interface CodecAdapter {
  createDecoder(init: VideoDecoderInitLike): VideoDecoderLike;
  createChunk(init: VideoChunkInit): VideoChunkLike;
}

function createBrowserDecoder(init: VideoDecoderInitLike): VideoDecoderLike {
  const decoder = new VideoDecoder({
    output: (frame) => {
      init.output(frame);
    },
    error: (error) => {
      init.error(error);
    },
  });

  return {
    configure: (config) => {
      decoder.configure(config);
    },
    decode: (chunk) => {
      // Chunks always come from `createChunk`, which builds an EncodedVideoChunk.
      decoder.decode(chunk as EncodedVideoChunk);
    },
    close: () => {
      decoder.close();
    },
  };
}

const browserCodecAdapter: CodecAdapter = {
  createDecoder: createBrowserDecoder,
  createChunk: (init) => {
    const chunk = new EncodedVideoChunk({
      type: init.type,
      timestamp: init.timestamp,
      data: init.data,
    });

    return chunk;
  },
};

function byteAt(bytes: Uint8Array, index: number): number {
  const value = bytes[index];
  if (value === undefined) {
    throw new StreamError("decoder", `avcC is too short: missing byte ${index}`);
  }

  return value;
}

function toHexByte(value: number): string {
  return value.toString(16).padStart(2, "0");
}

/**
 * Builds the WebCodecs codec string (`avc1.PPCCLL`) from the avcC record:
 * configurationVersion, profile, compatibility and level.
 */
export function codecStringFromAvcC(avcc: Uint8Array): string {
  const profile = byteAt(avcc, 1);
  const compatibility = byteAt(avcc, 2);
  const level = byteAt(avcc, 3);

  return `avc1.${toHexByte(profile)}${toHexByte(compatibility)}${toHexByte(level)}`;
}

function closeQuietly(decoder: VideoDecoderLike | null): void {
  if (decoder === null) return;
  try {
    decoder.close();
  } catch {
    // The decoder may already be closed or in an error state.
  }
}

export class WebCodecsSink implements VideoSink {
  readonly #handlers: VideoSinkHandlers;
  readonly #codecs: CodecAdapter;
  #decoder: VideoDecoderLike | null = null;
  #disposed = false;

  constructor(
    handlers: VideoSinkHandlers,
    codecs: CodecAdapter = browserCodecAdapter,
  ) {
    this.#handlers = handlers;
    this.#codecs = codecs;
  }

  onConfig(config: VideoConfig): void {
    if (this.#disposed) return;

    this.#closeDecoder();

    let codec: string;
    try {
      codec = codecStringFromAvcC(config.avcc);
    } catch (cause) {
      this.#fail(cause);
      return;
    }

    let decoder: VideoDecoderLike | null = null;
    try {
      decoder = this.#codecs.createDecoder({
        output: (frame) => {
          try {
            this.#handlers.draw(frame);
          } finally {
            frame.close();
          }
        },
        error: (error) => {
          this.#fail(new StreamError("decoder", error.message));
        },
      });
      decoder.configure({
        codec,
        description: config.avcc.slice(),
        optimizeForLatency: true,
      });
    } catch (cause) {
      closeQuietly(decoder);
      this.#fail(cause);
      return;
    }

    this.#decoder = decoder;
  }

  onFrame(frame: VideoFrameData): void {
    const decoder = this.#decoder;
    if (this.#disposed || decoder === null) return;

    try {
      const chunk = this.#codecs.createChunk({
        type: frame.keyframe ? "key" : "delta",
        timestamp: frame.ptsUs,
        data: frame.data,
      });
      decoder.decode(chunk);
    } catch (cause) {
      this.#fail(cause);
    }
  }

  dispose(): void {
    if (this.#disposed) return;

    this.#disposed = true;
    this.#closeDecoder();
  }

  #fail(cause: unknown): void {
    if (this.#disposed) return;

    this.#closeDecoder();
    if (cause instanceof StreamError) {
      this.#handlers.onError(cause);
      return;
    }

    const message = cause instanceof Error ? cause.message : "Decoder failure";
    this.#handlers.onError(new StreamError("decoder", message));
  }

  #closeDecoder(): void {
    const decoder = this.#decoder;
    this.#decoder = null;
    closeQuietly(decoder);
  }
}
