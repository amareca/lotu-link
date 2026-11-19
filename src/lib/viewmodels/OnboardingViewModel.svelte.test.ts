import { describe, expect, it } from "vitest";
import { FakeApi } from "../models/api/fakeApi";
import { OnboardingViewModel } from "./OnboardingViewModel.svelte";

describe("OnboardingViewModel", () => {
  it("loads the real fingerprint", async () => {
    const api = new FakeApi({
      fingerprint: { fingerprint: "11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00", error: null },
    });
    const onboarding = new OnboardingViewModel(api);
    await onboarding.load();

    expect(onboarding.fingerprint).toBe("11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00");
    expect(onboarding.error).toBeNull();
    expect(onboarding.loading).toBe(false);
  });

  it("shows a safe reason when the key is missing", async () => {
    const api = new FakeApi({
      fingerprint: { fingerprint: null, error: "the ADB key is not available yet" },
    });
    const onboarding = new OnboardingViewModel(api);
    await onboarding.load();

    expect(onboarding.fingerprint).toBeNull();
    expect(onboarding.error).toContain("ADB key");
  });
});
