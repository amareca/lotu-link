import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ApiError, type LotuApi } from "./api";
import {
  parseDeviceDtos,
  parseRecordingDto,
  parseRecordingStateDto,
  parseReplaySessionDto,
  parseScreenshotDto,
  parseSessionDto,
  parseRsaFingerprintDto,
  parseSettingsDto,
  toErrorDto,
  type DeviceDto,
  type KeyAction,
  type KeyInput,
  type PointerInput,
  type RecordingDto,
  type RecordingStateDto,
  type ReplaySessionDto,
  type RsaFingerprintDto,
  type ScreenshotDto,
  type ScrollInput,
  type SessionDto,
  type SettingsDto,
  type VideoSettingsDto,
} from "./dto";

/**
 * The only frontend module allowed to import Tauri.
 * Commands keep their Rust names; DTOs are validated before reaching the ViewModels.
 */
export class TauriApi implements LotuApi {
  async listDevices(): Promise<DeviceDto[]> {
    try {
      const payload: unknown = await invoke<unknown>("list_devices");
      return parseDeviceDtos(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async startReplay(): Promise<ReplaySessionDto> {
    try {
      const payload: unknown = await invoke<unknown>("start_replay");
      return parseReplaySessionDto(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async startSession(serial: string): Promise<SessionDto> {
    try {
      const payload: unknown = await invoke<unknown>("start_session", { serial });
      return parseSessionDto(payload);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async stopSession(): Promise<void> {
    try {
      await invoke<unknown>("stop_session");
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async sendPointer(input: PointerInput): Promise<void> {
    await this.#invokeInput("send_pointer", { input });
  }

  async sendScroll(input: ScrollInput): Promise<void> {
    await this.#invokeInput("send_scroll", { input });
  }

  async sendKey(input: KeyInput): Promise<void> {
    await this.#invokeInput("send_key", { input });
  }

  async sendText(text: string): Promise<void> {
    await this.#invokeInput("send_text", { text });
  }

  async sendBack(action: KeyAction): Promise<void> {
    await this.#invokeInput("send_back", { action });
  }

  async rotate(): Promise<void> {
    await this.#invokeInput("rotate", {});
  }

  async pasteText(text: string): Promise<void> {
    await this.#invokeInput("paste_text", { text });
  }

  async copyText(): Promise<string> {
    try {
      return await invoke<string>("copy_text");
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async captureScreenshot(): Promise<ScreenshotDto> {
    try {
      return parseScreenshotDto(await invoke<unknown>("capture_screenshot"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async startRecording(): Promise<RecordingDto> {
    try {
      return parseRecordingDto(await invoke<unknown>("start_recording"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async stopRecording(): Promise<RecordingDto> {
    try {
      return parseRecordingDto(await invoke<unknown>("stop_recording"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async getSettings(): Promise<SettingsDto> {
    try {
      return parseSettingsDto(await invoke<unknown>("get_settings"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async saveSettings(video: VideoSettingsDto): Promise<SettingsDto> {
    try {
      return parseSettingsDto(await invoke<unknown>("save_settings", { video }));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async markOnboardingSeen(): Promise<SettingsDto> {
    try {
      return parseSettingsDto(await invoke<unknown>("mark_onboarding_seen"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  async getRsaFingerprint(): Promise<RsaFingerprintDto> {
    try {
      return parseRsaFingerprintDto(await invoke<unknown>("get_rsa_fingerprint"));
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }

  onRecordingState(listener: (state: RecordingStateDto) => void): () => void {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    void listen<unknown>("recording-state", (event) => {
      listener(parseRecordingStateDto(event.payload));
    }).then((handle) => {
      if (disposed) handle();
      else unlisten = handle;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }

  async #invokeInput(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await invoke<unknown>(command, args);
    } catch (cause) {
      throw new ApiError(toErrorDto(cause));
    }
  }
}
