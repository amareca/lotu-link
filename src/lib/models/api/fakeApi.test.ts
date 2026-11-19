import { describe, expect, it } from "vitest";
import { ApiError } from "./api";
import { FAKE_DEVICES, FakeApi } from "./fakeApi";
import type { SessionDto } from "./dto";

const LIVE_SESSION: SessionDto = {
  wsUrl: "ws://127.0.0.1:45998/stream?token=live",
  device: {
    serial: "PIXEL8PRO",
    name: "Pixel 8 Pro",
    state: "connected",
    androidVersion: "15",
    resolution: { width: 1080, height: 2400 },
    batteryPercent: 78,
    transport: "usb",
  },
};

describe("FakeApi session commands", () => {
  it("returns the configured live session and records commands", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });

    await expect(api.startSession("PIXEL8PRO")).resolves.toEqual(LIVE_SESSION);
    await api.stopSession();

    expect(api.startSessionCalls).toEqual(["PIXEL8PRO"]);
    expect(api.stopSessionCalls).toBe(1);
  });

  it("clones the session so callers cannot mutate the fixture", async () => {
    const api = new FakeApi({ session: LIVE_SESSION });
    const session = await api.startSession("PIXEL8PRO");

    session.device.name = "Mutated";
    if (session.device.resolution !== null) {
      session.device.resolution.width = 1;
    }

    const again = await api.startSession("PIXEL8PRO");
    expect(again.device.name).toBe("Pixel 8 Pro");
    expect(again.device.resolution).toEqual({ width: 1080, height: 2400 });
  });

  it("fails with session_failed when no live session is configured", async () => {
    const api = new FakeApi();

    const error = await api.startSession(FAKE_DEVICES[0]?.serial ?? "X").catch(
      (cause: unknown) => cause,
    );

    expect(error).toBeInstanceOf(ApiError);
    if (error instanceof ApiError) {
      expect(error.dto.code).toBe("session_failed");
    }
  });

  it("uses the configurable session failure", async () => {
    const api = new FakeApi({
      sessionFailure: { code: "vendor_missing", message: "jar not found" },
    });

    const error = await api.startSession("PIXEL8PRO").catch((cause: unknown) => cause);

    expect(error).toBeInstanceOf(ApiError);
    if (error instanceof ApiError) {
      expect(error.dto).toEqual({
        code: "vendor_missing",
        message: "jar not found",
      });
    }
  });
});
