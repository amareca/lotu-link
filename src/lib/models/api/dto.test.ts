import { describe, expect, it } from "vitest";
import {
  parseDeviceDto,
  parseDeviceDtos,
  parseRecordingDto,
  parseRecordingStateDto,
  parseReplaySessionDto,
  parseScreenshotDto,
  parseSessionDto,
  parseSettingsDto,
  toErrorDto,
  type DeviceDto,
  type SessionDto,
} from "./dto";

const CONNECTED_PAYLOAD = {
  serial: "PIXEL8PRO",
  name: "Pixel 8 Pro",
  state: "connected",
  androidVersion: "15",
  resolution: { width: 1080, height: 2400 },
  batteryPercent: 78,
  transport: "usb",
} as const;

const CONNECTED_DEVICE: DeviceDto = {
  serial: "PIXEL8PRO",
  name: "Pixel 8 Pro",
  state: "connected",
  androidVersion: "15",
  resolution: { width: 1080, height: 2400 },
  batteryPercent: 78,
  transport: "usb",
};

describe("parseDeviceDto", () => {
  it("maps the Rust camelCase payload without changing fields", () => {
    expect(parseDeviceDto(CONNECTED_PAYLOAD)).toEqual(CONNECTED_DEVICE);
  });

  it("keeps unauthorized devices with null optional fields", () => {
    const device = parseDeviceDto({
      serial: "GALAXYS23",
      name: "Galaxy S23",
      state: "unauthorized",
      androidVersion: null,
      resolution: null,
      batteryPercent: null,
      transport: "usb",
    });

    expect(device.state).toBe("unauthorized");
    expect(device.androidVersion).toBeNull();
    expect(device.resolution).toBeNull();
    expect(device.batteryPercent).toBeNull();
  });

  it("degrades malformed optional fields to null", () => {
    const device = parseDeviceDto({
      ...CONNECTED_PAYLOAD,
      androidVersion: 15,
      resolution: { width: -1, height: 2400 },
      batteryPercent: 140,
    });

    expect(device.androidVersion).toBeNull();
    expect(device.resolution).toBeNull();
    expect(device.batteryPercent).toBeNull();
  });

  it("rejects an unknown state instead of inventing one", () => {
    expect(() =>
      parseDeviceDto({ ...CONNECTED_PAYLOAD, state: "authorizing" }),
    ).toThrow(/unknown state/);
  });

  it("rejects a non-USB transport", () => {
    expect(() =>
      parseDeviceDto({ ...CONNECTED_PAYLOAD, transport: "wifi" }),
    ).toThrow(/unknown transport/);
  });
});

describe("parseDeviceDtos", () => {
  it("maps an array of devices", () => {
    const devices = parseDeviceDtos([
      CONNECTED_PAYLOAD,
      {
        serial: "OFFLINE1",
        name: "Offline",
        state: "offline",
        androidVersion: null,
        resolution: null,
        batteryPercent: null,
        transport: "usb",
      },
    ]);

    expect(devices).toHaveLength(2);
    expect(devices[0]?.state).toBe("connected");
    expect(devices[1]?.state).toBe("offline");
  });

  it("accepts an empty list as a valid result", () => {
    expect(parseDeviceDtos([])).toEqual([]);
  });

  it("rejects a non-array payload", () => {
    expect(() => parseDeviceDtos({ devices: [] })).toThrow(/expected an array/);
  });
});

describe("parseReplaySessionDto", () => {
  const REPLAY_PAYLOAD = {
    wsUrl: "ws://127.0.0.1:45999/stream?token=abc",
    device: CONNECTED_PAYLOAD,
  } as const;

  it("parses the loopback WebSocket URL and the device", () => {
    expect(parseReplaySessionDto(REPLAY_PAYLOAD)).toEqual({
      wsUrl: "ws://127.0.0.1:45999/stream?token=abc",
      device: CONNECTED_DEVICE,
    });
  });

  it("accepts a secure WebSocket URL", () => {
    const session = parseReplaySessionDto({
      ...REPLAY_PAYLOAD,
      wsUrl: "wss://127.0.0.1:45999/stream",
    });

    expect(session.wsUrl).toBe("wss://127.0.0.1:45999/stream");
  });

  it("rejects a relative URL", () => {
    expect(() =>
      parseReplaySessionDto({ ...REPLAY_PAYLOAD, wsUrl: "/stream" }),
    ).toThrow(/absolute URL/);
  });

  it("rejects a non-WebSocket scheme", () => {
    expect(() =>
      parseReplaySessionDto({
        ...REPLAY_PAYLOAD,
        wsUrl: "http://127.0.0.1:45999/stream",
      }),
    ).toThrow(/ws or wss/);
  });

  it("rejects a missing or empty wsUrl", () => {
    expect(() =>
      parseReplaySessionDto({ ...REPLAY_PAYLOAD, wsUrl: "" }),
    ).toThrow(/missing wsUrl/);
    expect(() =>
      parseReplaySessionDto({ device: CONNECTED_PAYLOAD }),
    ).toThrow(/missing wsUrl/);
  });

  it("rejects a non-object payload", () => {
    expect(() => parseReplaySessionDto(null)).toThrow(/expected an object/);
  });

  it("validates the nested device", () => {
    expect(() =>
      parseReplaySessionDto({
        ...REPLAY_PAYLOAD,
        device: { ...CONNECTED_PAYLOAD, state: "authorizing" },
      }),
    ).toThrow(/unknown state/);
  });
});

describe("parseSessionDto", () => {
  const SESSION_PAYLOAD = {
    wsUrl: "ws://127.0.0.1:45998/stream?token=live",
    device: CONNECTED_PAYLOAD,
  } as const;

  it("parses the start_session payload", () => {
    const session: SessionDto = parseSessionDto(SESSION_PAYLOAD);

    expect(session).toEqual({
      wsUrl: "ws://127.0.0.1:45998/stream?token=live",
      device: CONNECTED_DEVICE,
    });
  });

  it("is the same parser used by the replay alias", () => {
    expect(parseReplaySessionDto(SESSION_PAYLOAD)).toEqual(
      parseSessionDto(SESSION_PAYLOAD),
    );
  });

  it("rejects an invalid nested device", () => {
    expect(() =>
      parseSessionDto({
        ...SESSION_PAYLOAD,
        device: { ...CONNECTED_PAYLOAD, serial: "" },
      }),
    ).toThrow(/missing serial/);
  });

  it("rejects a non-WebSocket URL", () => {
    expect(() =>
      parseSessionDto({
        ...SESSION_PAYLOAD,
        wsUrl: "file:///tmp/stream",
      }),
    ).toThrow(/ws or wss/);
  });
});

describe("M3 capture and recording DTOs", () => {
  it("parses a screenshot with native dimensions", () => {
    expect(parseScreenshotDto({ path: "shot.png", width: 1080, height: 2400 })).toEqual({
      path: "shot.png",
      width: 1080,
      height: 2400,
    });
    expect(() => parseScreenshotDto({ path: "shot.png", width: 0, height: 10 })).toThrow();
  });

  it("parses recording statistics and state", () => {
    expect(
      parseRecordingDto({
        path: "clip.mp4",
        frames: 120,
        durationUs: 4_000_000,
        segments: 2,
        paths: ["clip.mp4", "clip-2.mp4"],
      }),
    ).toEqual({
      path: "clip.mp4",
      frames: 120,
      durationUs: 4_000_000,
      segments: 2,
      paths: ["clip.mp4", "clip-2.mp4"],
    });
    expect(() =>
      parseRecordingDto({ path: "clip.mp4", frames: -1, durationUs: 0 }),
    ).toThrow();
    expect(parseRecordingStateDto({ active: true, path: "clip.mp4" })).toEqual({
      active: true,
      path: "clip.mp4",
    });
    expect(parseRecordingStateDto(null)).toEqual({ active: false, path: null });
  });

  it("accepts the M3 error codes", () => {
    const codes = [
      "capture_timeout",
      "capture_invalid",
      "capture_failed",
      "recording_already_active",
      "recording_inactive",
      "recording_failed",
      "user_cancelled",
      "io_failed",
    ] as const;
    for (const code of codes) {
      expect(toErrorDto({ code, message: "diagnostic" }).code).toBe(code);
    }
  });
});

describe("M4 settings DTO", () => {
  it("parses video settings and the restart flag", () => {
    expect(
      parseSettingsDto({
        video: { maxSize: 1280, maxFps: 30, bitRate: 4_000_000 },
        restartRequired: true,
      }),
    ).toEqual({
      video: { maxSize: 1280, maxFps: 30, bitRate: 4_000_000 },
      restartRequired: true,
    });
  });

  it("rejects out-of-range values", () => {
    const base = { video: { maxSize: 0, maxFps: 60, bitRate: 8_000_000 } };
    expect(() => parseSettingsDto({ ...base, video: { ...base.video, bitRate: 0 } })).toThrow();
    expect(() => parseSettingsDto({ ...base, video: { ...base.video, maxSize: 9000 } })).toThrow();
    expect(() => parseSettingsDto({ ...base, video: { ...base.video, maxFps: 241 } })).toThrow();
  });

  it("accepts the settings error codes", () => {
    expect(toErrorDto({ code: "settings_invalid", message: "x" }).code).toBe("settings_invalid");
    expect(toErrorDto({ code: "settings_io", message: "x" }).code).toBe("settings_io");
  });
});

describe("toErrorDto", () => {
  it("preserves a typed ErrorDto", () => {
    expect(
      toErrorDto({ code: "adb_timeout", message: "adb timed out" }),
    ).toEqual({ code: "adb_timeout", message: "adb timed out" });
  });

  it("accepts the M1 session error codes", () => {
    const codes = [
      "device_not_found",
      "device_not_connected",
      "vendor_missing",
      "session_timeout",
      "session_failed",
    ] as const;

    for (const code of codes) {
      expect(toErrorDto({ code, message: "diagnostic" })).toEqual({
        code,
        message: "diagnostic",
      });
    }
  });

  it("falls back to internal for unknown rejections", () => {
    expect(toErrorDto(new Error("boom"))).toEqual({
      code: "internal",
      message: "boom",
    });
    expect(toErrorDto("boom")).toEqual({
      code: "internal",
      message: "boom",
    });
    expect(toErrorDto(null)).toEqual({
      code: "internal",
      message: "Unexpected error",
    });
  });
});
