import { describe, expect, it } from "vitest";
import { FakeApi } from "../models/api/fakeApi";
import { SettingsViewModel } from "./SettingsViewModel.svelte";

describe("SettingsViewModel", () => {
  it("loads the saved snapshot and tracks dirty state", async () => {
    const api = new FakeApi({
      settings: {
        video: { maxSize: 1280, maxFps: 30, bitRate: 4_000_000 },
        restartRequired: false,
      },
    });
    const settings = new SettingsViewModel(api);
    await settings.load();

    expect(settings.dirty).toBe(false);
    expect(settings.draft.maxSize).toBe(1280);

    settings.setResolution(1920);
    expect(settings.dirty).toBe(true);
    settings.reset();
    expect(settings.draft.maxSize).toBe(1280);
    expect(settings.dirty).toBe(false);
  });

  it("saves the draft and accepts the restart flag from the backend", async () => {
    const api = new FakeApi();
    const settings = new SettingsViewModel(api);
    await settings.load();

    settings.setFps(30);
    settings.setBitrate(4.5);
    expect(settings.draft.bitRate).toBe(4_500_000);

    expect(await settings.save()).toBe(true);
    expect(api.saveSettingsCalls).toBe(1);
    expect(api.currentSettings.video).toEqual({
      maxSize: 0,
      maxFps: 30,
      bitRate: 4_500_000,
    });
    expect(settings.dirty).toBe(false);
  });

  it("keeps the draft on a failed save", async () => {
    const api = new FakeApi({
      settingsFailure: { code: "settings_io", message: "disk full" },
    });
    const settings = new SettingsViewModel(api);
    await settings.load();
    settings.setResolution(1280);

    expect(await settings.save()).toBe(false);
    expect(settings.notice?.code).toBe("settings_io");
    expect(settings.draft.maxSize).toBe(1280);
    expect(settings.dirty).toBe(true);
  });
});
