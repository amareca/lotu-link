import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import { ApiError } from "./api";
import { TauriApi } from "./tauriApi";

const DEVICE_PAYLOAD = {
  serial: "PIXEL8PRO",
  name: "Pixel 8 Pro",
  state: "connected",
  androidVersion: "15",
  resolution: { width: 1080, height: 2400 },
  batteryPercent: 78,
  transport: "usb",
} as const;

beforeEach(() => {
  invokeMock.mockReset();
});

describe("TauriApi.startSession", () => {
  it("invokes start_session with the serial and parses the payload", async () => {
    invokeMock.mockResolvedValue({
      wsUrl: "ws://127.0.0.1:45998/stream?token=live",
      device: DEVICE_PAYLOAD,
    });

    const session = await new TauriApi().startSession("PIXEL8PRO");

    expect(invokeMock).toHaveBeenCalledWith("start_session", {
      serial: "PIXEL8PRO",
    });
    expect(session.wsUrl).toBe("ws://127.0.0.1:45998/stream?token=live");
    expect(session.device.serial).toBe("PIXEL8PRO");
  });

  it("wraps a typed command failure in ApiError", async () => {
    invokeMock.mockRejectedValue({
      code: "device_not_connected",
      message: "device is not connected",
    });

    const error = await new TauriApi()
      .startSession("GALAXYS23")
      .catch((cause: unknown) => cause);

    expect(error).toBeInstanceOf(ApiError);
    if (error instanceof ApiError) {
      expect(error.dto).toEqual({
        code: "device_not_connected",
        message: "device is not connected",
      });
    }
  });

  it("rejects a malformed payload", async () => {
    invokeMock.mockResolvedValue({ wsUrl: "/stream", device: DEVICE_PAYLOAD });

    await expect(new TauriApi().startSession("PIXEL8PRO")).rejects.toBeInstanceOf(
      ApiError,
    );
  });
});

describe("TauriApi.stopSession", () => {
  it("invokes stop_session without arguments", async () => {
    invokeMock.mockResolvedValue(null);

    await new TauriApi().stopSession();

    expect(invokeMock).toHaveBeenCalledWith("stop_session");
  });

  it("wraps a command failure in ApiError", async () => {
    invokeMock.mockRejectedValue({
      code: "internal",
      message: "cleanup failed",
    });

    const error = await new TauriApi()
      .stopSession()
      .catch((cause: unknown) => cause);

    expect(error).toBeInstanceOf(ApiError);
    if (error instanceof ApiError) {
      expect(error.dto.code).toBe("internal");
    }
  });
});
