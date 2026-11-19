import { describe, expect, it } from "vitest";
import { StreamError } from "./streamClient";
import {
  WebCodecsSink,
  codecStringFromAvcC,
  type CodecAdapter,
  type VideoChunkInit,
  type VideoChunkLike,
  type VideoConfig,
  type VideoDecoderConfigLike,
  type VideoDecoderInitLike,
  type VideoDecoderLike,
  type VideoFrameData,
} from "./webCodecsSink";

/** avcC: version 1, high profile (0x64), compatibility 0x00, level 0x28. */
const AVCC = new Uint8Array([
  0x01, 0x64, 0x00, 0x28, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x64, 0x00, 0x28, 0x01,
  0x00, 0x04, 0x68, 0xee, 0x3c, 0x80,
]);

class FakeDecoder implements VideoDecoderLike {
  readonly init: VideoDecoderInitLike;
  readonly configs: VideoDecoderConfigLike[] = [];
  readonly chunks: VideoChunkLike[] = [];
  closed = 0;

  constructor(init: VideoDecoderInitLike) {
    this.init = init;
  }

  configure(config: VideoDecoderConfigLike): void {
    this.configs.push(config);
  }

  decode(chunk: VideoChunkLike): void {
    this.chunks.push(chunk);
  }

  close(): void {
    this.closed += 1;
  }
}

class FakeAdapter implements CodecAdapter {
  readonly decoders: FakeDecoder[] = [];
  readonly chunkInits: VideoChunkInit[] = [];

  createDecoder(init: VideoDecoderInitLike): VideoDecoderLike {
    const decoder = new FakeDecoder(init);
    this.decoders.push(decoder);

    return decoder;
  }

  createChunk(init: VideoChunkInit): VideoChunkLike {
    this.chunkInits.push(init);

    return { type: init.type, timestamp: init.timestamp };
  }
}

class FakeFrame {
  readonly displayWidth = 1080;
  readonly displayHeight = 2400;
  closed = false;

  close(): void {
    this.closed = true;
  }
}

interface SinkHarness {
  readonly sink: WebCodecsSink;
  readonly codecs: FakeAdapter;
  readonly frames: VideoFrame[];
  readonly errors: StreamError[];
}

function createSink(draw?: (frame: VideoFrame) => void): SinkHarness {
  const codecs = new FakeAdapter();
  const frames: VideoFrame[] = [];
  const errors: StreamError[] = [];

  const sink = new WebCodecsSink(
    {
      draw: (frame) => {
        frames.push(frame);
        draw?.(frame);
      },
      onError: (error) => {
        errors.push(error);
      },
    },
    codecs,
  );

  return { sink, codecs, frames, errors };
}

function config(avcc: Uint8Array = AVCC): VideoConfig {
  return { width: 1080, height: 2400, avcc };
}

function frameData(ptsUs: number, keyframe: boolean): VideoFrameData {
  return { ptsUs, keyframe, data: new Uint8Array([0x65, 0x88]) };
}

function last<T>(items: readonly T[]): T {
  const item = items[items.length - 1];
  if (item === undefined) throw new Error("Expected at least one item");

  return item;
}

describe("codecStringFromAvcC", () => {
  it("builds avc1.PPCCLL from the profile, compatibility and level", () => {
    expect(codecStringFromAvcC(new Uint8Array([0x01, 0x42, 0xc0, 0x1f]))).toBe(
      "avc1.42c01f",
    );
    expect(codecStringFromAvcC(AVCC)).toBe("avc1.640028");
  });

  it("pads single-digit hex bytes", () => {
    expect(codecStringFromAvcC(new Uint8Array([0x01, 0x0a, 0x0b, 0x0c]))).toBe(
      "avc1.0a0b0c",
    );
  });

  it("rejects a truncated avcC record", () => {
    expect(() => codecStringFromAvcC(new Uint8Array([0x01, 0x64]))).toThrow(
      StreamError,
    );
  });
});

describe("WebCodecsSink", () => {
  it("recreates the decoder when a new configuration arrives", () => {
    const { sink, codecs } = createSink();

    sink.onConfig(config());
    const first = last(codecs.decoders);
    sink.onConfig({ ...config(), width: 2400, height: 1080 });

    expect(codecs.decoders).toHaveLength(2);
    expect(first.closed).toBe(1);
    expect(last(codecs.decoders).closed).toBe(0);

    sink.onFrame(frameData(0, true));
    expect(first.chunks).toHaveLength(0);
    expect(last(codecs.decoders).chunks).toHaveLength(1);
  });

  it("configures the decoder with the derived codec string and avcC", () => {
    const { sink, codecs } = createSink();

    sink.onConfig(config());

    const decoderConfig = last(last(codecs.decoders).configs);
    expect(decoderConfig.codec).toBe("avc1.640028");
    expect(decoderConfig.optimizeForLatency).toBe(true);
    expect(Array.from(decoderConfig.description)).toEqual(Array.from(AVCC));
    // The record is copied so later mutation of the socket buffer cannot leak.
    expect(decoderConfig.description).not.toBe(AVCC);
  });

  it("maps keyframe/PTS into the encoded chunk type and timestamp", () => {
    const { sink, codecs } = createSink();
    sink.onConfig(config());

    sink.onFrame(frameData(33_333, true));
    sink.onFrame(frameData(66_666, false));

    expect(codecs.chunkInits).toHaveLength(2);
    expect(codecs.chunkInits[0]).toMatchObject({
      type: "key",
      timestamp: 33_333,
    });
    expect(last(codecs.chunkInits)).toMatchObject({
      type: "delta",
      timestamp: 66_666,
    });
    expect(Array.from(last(codecs.chunkInits).data)).toEqual([0x65, 0x88]);

    const chunks = last(codecs.decoders).chunks;
    expect(chunks.map((chunk) => chunk.type)).toEqual(["key", "delta"]);
    expect(chunks.map((chunk) => chunk.timestamp)).toEqual([33_333, 66_666]);
  });

  it("ignores frames received before a configuration", () => {
    const { sink, codecs, errors } = createSink();

    sink.onFrame(frameData(0, true));

    expect(codecs.chunkInits).toHaveLength(0);
    expect(codecs.decoders).toHaveLength(0);
    expect(errors).toEqual([]);
  });

  it("always closes the decoded frame, even when draw throws", () => {
    const { sink, codecs } = createSink(() => {
      throw new Error("canvas exploded");
    });
    sink.onConfig(config());

    const frame = new FakeFrame();
    expect(() =>
      last(codecs.decoders).init.output(frame as unknown as VideoFrame),
    ).toThrow("canvas exploded");
    expect(frame.closed).toBe(true);
  });

  it("closes the decoded frame after a successful draw", () => {
    const { sink, codecs, frames } = createSink();
    sink.onConfig(config());

    const frame = new FakeFrame();
    last(codecs.decoders).init.output(frame as unknown as VideoFrame);

    expect(frames).toHaveLength(1);
    expect(frames[0]).toBe(frame as unknown as VideoFrame);
    expect(frame.closed).toBe(true);
  });

  it("reports a decoder error once and closes the decoder", () => {
    const { sink, codecs, errors } = createSink();
    sink.onConfig(config());
    const decoder = last(codecs.decoders);

    decoder.init.error(new DOMException("hardware failure", "EncodingError"));

    expect(errors).toHaveLength(1);
    expect(errors[0]).toBeInstanceOf(StreamError);
    expect(errors[0]?.code).toBe("decoder");
    expect(errors[0]?.message).toBe("hardware failure");
    expect(decoder.closed).toBe(1);

    sink.onFrame(frameData(0, true));
    expect(codecs.chunkInits).toHaveLength(0);
  });

  it("reports a malformed avcC without creating a decoder", () => {
    const { sink, codecs, errors } = createSink();

    sink.onConfig({ width: 1080, height: 2400, avcc: new Uint8Array([0x01]) });

    expect(codecs.decoders).toHaveLength(0);
    expect(errors).toHaveLength(1);
    expect(errors[0]?.code).toBe("decoder");
    expect(errors[0]?.message).toMatch(/avcC is too short/);
  });

  it("closes the decoder on dispose and ignores late messages", () => {
    const { sink, codecs } = createSink();
    sink.onConfig(config());
    const decoder = last(codecs.decoders);

    sink.dispose();
    expect(decoder.closed).toBe(1);

    sink.onConfig(config());
    sink.onFrame(frameData(0, true));

    expect(codecs.decoders).toHaveLength(1);
    expect(codecs.chunkInits).toHaveLength(0);
  });
});
