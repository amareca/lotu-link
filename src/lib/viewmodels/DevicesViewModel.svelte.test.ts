import { describe, expect, it } from "vitest";
import type { DeviceDto } from "../models/api/dto";
import { FAKE_DEVICES, FakeApi } from "../models/api/fakeApi";
import { DevicesViewModel } from "./DevicesViewModel.svelte";

const OFFLINE_DEVICE: DeviceDto = {
  serial: "OFFLINE01",
  name: "Pixel 6",
  state: "offline",
  androidVersion: null,
  resolution: null,
  batteryPercent: null,
  transport: "usb",
};

describe("DevicesViewModel", () => {
  it("loads the fake devices and records the scan time", async () => {
    const api = new FakeApi();
    const viewModel = new DevicesViewModel(api);
    const before = Date.now();

    await viewModel.refresh();

    expect(viewModel.devices).toHaveLength(2);
    expect(viewModel.devices[0]?.name).toBe("Pixel 8 Pro");
    expect(viewModel.devices[0]?.state).toBe("connected");
    expect(viewModel.devices[1]?.state).toBe("unauthorized");
    expect(viewModel.loading).toBe(false);
    expect(viewModel.error).toBeNull();
    expect(viewModel.lastScanAt).toBeInstanceOf(Date);
    expect(viewModel.lastScanAt?.getTime()).toBeGreaterThanOrEqual(before);

    viewModel.dispose();
  });

  it("accepts an empty device list as a successful scan", async () => {
    const api = new FakeApi({ devices: [] });
    const viewModel = new DevicesViewModel(api);

    await viewModel.refresh();

    expect(viewModel.devices).toEqual([]);
    expect(viewModel.error).toBeNull();
    expect(viewModel.lastScanAt).toBeInstanceOf(Date);

    viewModel.dispose();
  });

  it("exposes a typed error and keeps the previous successful scan", async () => {
    const api = new FakeApi();
    const viewModel = new DevicesViewModel(api);

    await viewModel.refresh();
    const scannedAt = viewModel.lastScanAt;
    expect(scannedAt).not.toBeNull();

    api.setFailure({ code: "adb_unavailable", message: "adb not found" });
    await viewModel.refresh();

    expect(viewModel.error).toEqual({
      code: "adb_unavailable",
      message: "adb not found",
    });
    expect(viewModel.lastScanAt).toBe(scannedAt);
    expect(viewModel.devices).toHaveLength(2);
    expect(viewModel.loading).toBe(false);

    viewModel.dispose();
  });

  it("replaces the list on refresh and clears a previous error", async () => {
    const api = new FakeApi({
      devices: FAKE_DEVICES.slice(0, 1),
      failure: { code: "adb_failed", message: "first attempt failed" },
    });
    const viewModel = new DevicesViewModel(api);

    await viewModel.refresh();
    expect(viewModel.error?.code).toBe("adb_failed");
    expect(viewModel.lastScanAt).toBeNull();

    api.setFailure(null);
    api.setDevices([...FAKE_DEVICES, OFFLINE_DEVICE]);
    await viewModel.refresh();

    expect(viewModel.error).toBeNull();
    expect(viewModel.devices).toHaveLength(3);
    expect(viewModel.devices[2]?.state).toBe("offline");
    expect(viewModel.lastScanAt).toBeInstanceOf(Date);

    viewModel.dispose();
  });

  it("tracks the loading flag while a refresh is in flight", async () => {
    const api = new FakeApi({ delayMs: 10 });
    const viewModel = new DevicesViewModel(api);

    const pending = viewModel.refresh();
    expect(viewModel.loading).toBe(true);

    await pending;
    expect(viewModel.loading).toBe(false);

    viewModel.dispose();
  });

  it("ignores late results after dispose", async () => {
    const api = new FakeApi({ delayMs: 10 });
    const viewModel = new DevicesViewModel(api);

    const pending = viewModel.refresh();
    viewModel.dispose();
    await pending;

    expect(viewModel.devices).toEqual([]);
    expect(viewModel.lastScanAt).toBeNull();
    expect(viewModel.loading).toBe(false);
  });
});
